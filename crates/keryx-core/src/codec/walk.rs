//! The shredding walk (architecture §5, inbound; spec §4.1, §5, §7, §11): one decoded payload,
//! read through the engine's borrowing value view, lowered under the mapping model to the ground
//! facts of one root — a sort atom for every occupied slot, a fact for every scalar or enum value
//! — on an explicit heap stack of work items, never native recursion (the threat model's
//! property 3), under the uniform payload nesting ceiling.
//!
//! **Presence is the mapping's decision (spec §5).** A field's [`Totality`] decides whether its
//! atom exists: a `Total` field always emits, its zero materialised, and a `Total` collection
//! emits per element (an empty one nothing); a `Partial` field — EXPLICIT, every message-typed
//! field, every `oneof` arm, `LEGACY_REQUIRED` — emits iff the wire carried it, which is the one
//! question the walk asks the engine's presence. Nothing here re-derives presence from a value.
//!
//! **The shape of a fact is the mapping's form (§4.1, §7).** A singular field (a `oneof` arm is one,
//! §7.3) is `f(P, V)` for a value and the occupant `f(P)` for a message; a sequence `f(P, I, V)`
//! and `f(P, I)`; a map `f(P, K, V)` and `f(P, K)`, its key lowered per §6 like a value (§7.2);
//! an enum value is its declared constant, an undeclared number a refusal (§7.4). A message
//! occupant becomes a work item — its occupancy atom `s(occupant)` and its fields are emitted when
//! the item is popped — so nesting lives inside the path terms and the walk's memory is the heap's.
//!
//! **One representation per fact.** [`Walker::emit`] builds a fact's head [`Symbol`] through
//! `crate::terms` — the one structure [`Facts`] holds; the `.lp` seam's statements are derived
//! from those symbols when the facts render (`Facts::render`), so the two seams cannot disagree:
//! there is nothing beside the symbols to disagree with (spec §11).
//!
//! **Every refusal is collected (§6, §26).** A refused value is a [`Diagnostic`] at its field's
//! fully-qualified proto path; the walk continues, and delivers either every fact or every
//! diagnosis — never a partial shred beside a diagnosis.
//!
//! **The can't-happens, discharged.** The mapping and the decoded tree derive from one descriptor
//! pool: the codec walks a payload against the very pool its schema came from, and resolves the
//! root and every referent through the [`Index`] built over that mapping. So a decoded value's
//! shape is its field's form, a datum's kind its field's kind, and every referent a sort or enum of
//! the mapping — each stated once, at its arm, as a keryx error that no foreign input reaches.

use std::collections::BTreeMap;

use themelios_program::prelude::*;

use crate::codec::Facts;
use crate::codec::engine::{Datum, Element, FieldValue, SubMessage};
use crate::codec::scalar;
use crate::descriptor::RECURSION_LIMIT;
use crate::descriptor::model::{FqName, Scalar};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::policy::model::{
    EmitForm, EnumMapping, FieldMapping, Mapping, SortMapping, Totality, ValueMapping,
};
use crate::terms;

/// The uniform payload nesting ceiling (spec §8, §26; the threat model's property 3): the deepest
/// compositional nesting — message-typed fields below the root — a payload of any format may
/// carry, `RECURSION_LIMIT − 1`, one below the engine's decode-recursion limit. A door-admission
/// policy, not a limit of the translation: §8's path terms impose no ceiling, and the walk runs on
/// a managed stack; the counter here is where the ceiling binds for a format whose decoder admits
/// deeper — JSON, and binary at exactly one level, since the engine decodes a payload nesting
/// `RECURSION_LIMIT` levels and refuses only past that (`engine::decode_binary`), so the counter
/// refuses that deepest decodable level itself and stands as defense-in-depth beneath the engine
/// from there on. Named once, derived from the engine's constant.
pub(crate) const NESTING_CEILING: usize = RECURSION_LIMIT - 1;

/// The referent index over a [`Mapping`], built once per codec: predicate-keyed sort and enum
/// resolution for the walk — a field's value names its referent by emitted predicate
/// (`ValueMapping::Message`, `ValueMapping::Enum`), and the `/1` sort namespace is injective
/// (spec §4.2), so a predicate names one sort or enum — and name-keyed root resolution for the
/// codec's door, by fully-qualified path or by a short name no other message shares. Every entry
/// is a position into the mapping the index was built over, which the codec owns beside it,
/// unchanged, so the positions hold for the codec's life.
#[derive(Debug)]
pub(crate) struct Index {
    sorts_by_predicate: BTreeMap<Name, SortRef>,
    enums_by_predicate: BTreeMap<Name, EnumRef>,
    messages_by_name: BTreeMap<String, SortRef>,
}

/// A sort of the mapping, by position: the `sort`th sort of the `unit`th unit. Minted only by
/// [`Index::build`], over the mapping it indexes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SortRef {
    unit: usize,
    sort: usize,
}

/// An enum of the mapping, by position: the `enumeration`th enum of the `unit`th unit. Minted only
/// by [`Index::build`], over the mapping it indexes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EnumRef {
    unit: usize,
    enumeration: usize,
}

impl SortRef {
    /// The sort this reference names in `mapping` — the mapping it was minted over: the index's
    /// build is the one minting site and the codec keeps index and mapping together, so the
    /// positions hold (the indexing is discharged by that construction).
    pub(crate) fn in_mapping(self, mapping: &Mapping) -> &SortMapping {
        &mapping.units()[self.unit].sorts()[self.sort]
    }
}

impl EnumRef {
    /// The enum this reference names in `mapping`, as [`SortRef::in_mapping`].
    fn in_mapping(self, mapping: &Mapping) -> &EnumMapping {
        &mapping.units()[self.unit].enums()[self.enumeration]
    }
}

impl Index {
    /// Index `mapping` — every sort and enum by predicate, every message by fully-qualified name
    /// and, where no other message shares it, by short name — and check the closed world the walk
    /// then assumes: no two sorts or enums share a predicate, and every field's referent is a sort
    /// or enum of the mapping. The policy establishes both (§4.2 injectivity; referents resolved
    /// from the one sort table), so a violation is a keryx error — checked rather than assumed,
    /// and diagnosed as `policy::map` diagnoses its own can't-happen lookups (`UnmappableName` at
    /// the offending element), never carried into a walk that would `expect` it.
    ///
    /// # Errors
    ///
    /// `UnmappableName` at the second sort or enum of a shared predicate, and at every field
    /// whose referent predicate names no sort (a message) or enum (an enum value) of the mapping.
    pub(crate) fn build(mapping: &Mapping) -> Result<Index, Diagnostics> {
        let mut index = Index {
            sorts_by_predicate: BTreeMap::new(),
            enums_by_predicate: BTreeMap::new(),
            messages_by_name: BTreeMap::new(),
        };
        // A short name shared by two messages resolves to neither (`None`): only the
        // fully-qualified name separates them.
        let mut by_short_name: BTreeMap<&str, Option<SortRef>> = BTreeMap::new();
        let mut problems = Vec::new();
        for (unit_at, unit) in mapping.units().iter().enumerate() {
            for (sort_at, sort) in unit.sorts().iter().enumerate() {
                let reference = SortRef {
                    unit: unit_at,
                    sort: sort_at,
                };
                let taken = index.enums_by_predicate.contains_key(sort.predicate())
                    || index
                        .sorts_by_predicate
                        .insert(sort.predicate().clone(), reference)
                        .is_some();
                if taken {
                    problems.push(shared_predicate(sort.proto(), sort.predicate()));
                }
                index
                    .messages_by_name
                    .insert(sort.proto().as_str().to_owned(), reference);
                by_short_name
                    .entry(final_segment(sort.proto().as_str()))
                    .and_modify(|unique| *unique = None)
                    .or_insert(Some(reference));
            }
            for (enum_at, enumeration) in unit.enums().iter().enumerate() {
                let reference = EnumRef {
                    unit: unit_at,
                    enumeration: enum_at,
                };
                let taken = index
                    .sorts_by_predicate
                    .contains_key(enumeration.predicate())
                    || index
                        .enums_by_predicate
                        .insert(enumeration.predicate().clone(), reference)
                        .is_some();
                if taken {
                    problems.push(shared_predicate(
                        enumeration.proto(),
                        enumeration.predicate(),
                    ));
                }
            }
        }
        for (short, unique) in by_short_name {
            if let Some(reference) = unique {
                // A fully-qualified name carries its package's dots and a short name none, so the
                // two never coincide; the entry form keeps a path's own entry first regardless.
                index
                    .messages_by_name
                    .entry(short.to_owned())
                    .or_insert(reference);
            }
        }
        for field in mapping
            .units()
            .iter()
            .flat_map(|unit| unit.sorts().iter().flat_map(|sort| sort.fields().iter()))
        {
            let resolved = match field.value() {
                ValueMapping::Scalar { .. } => true,
                ValueMapping::Message(predicate) => {
                    index.sorts_by_predicate.contains_key(predicate)
                }
                ValueMapping::Enum(predicate) => index.enums_by_predicate.contains_key(predicate),
            };
            if !resolved {
                problems.push(dangling_referent(field));
            }
        }
        Diagnostics::collect(problems).map_or(Ok(index), Err)
    }

    /// Resolve the root type a caller named (spec §4.1 item 6, the codec's one resolution site):
    /// a fully-qualified proto path, or a short name that exactly one message of the mapping
    /// bears. A miss — no such message, or a short name more than one message shares — is
    /// `UnknownRootType` at the whole-payload locus, its detail naming the type as given and, for a
    /// shared short name, the messages that share it, so the caller can give the one that
    /// separates them.
    ///
    /// # Errors
    ///
    /// `UnknownRootType` for a name that resolves to no message, or to more than one.
    pub(crate) fn root(&self, mapping: &Mapping, name: &str) -> Result<SortRef, Diagnostics> {
        self.messages_by_name
            .get(name)
            .copied()
            .ok_or_else(|| unknown_root_type(mapping, name))
    }

    /// The sort a referent predicate names, if any.
    fn sort_of(&self, predicate: &Name) -> Option<SortRef> {
        self.sorts_by_predicate.get(predicate).copied()
    }

    /// The enum a referent predicate names, if any.
    fn enum_of(&self, predicate: &Name) -> Option<EnumRef> {
        self.enums_by_predicate.get(predicate).copied()
    }
}

/// The final dot-separated segment of a proto path — a message's short name.
fn final_segment(path: &str) -> &str {
    // `rsplit` always yields at least one item, so the fallback never fires; a total guard
    // rather than an `expect` over an iterator-API detail.
    path.rsplit('.').next().unwrap_or(path)
}

/// `UnmappableName` at `path`: a second sort or enum lowering to `predicate`, which the policy's
/// injectivity backstop (§4.2) never emits — checked, not assumed.
fn shared_predicate(path: &FqName, predicate: &Name) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::UnmappableName,
        Locus::at(path.as_str()),
        format!(
            "two sorts of the mapping lower to the predicate `{}/1`",
            predicate.as_str()
        ),
    )
}

/// `UnmappableName` at the field: its referent predicate names no sort (or enum) of the mapping,
/// which the policy's referent resolution never leaves dangling — checked, not assumed.
fn dangling_referent(field: &FieldMapping) -> Diagnostic {
    let (referent, kind) = match field.value() {
        ValueMapping::Message(predicate) => (predicate, "sort"),
        ValueMapping::Enum(predicate) => (predicate, "enum"),
        ValueMapping::Scalar { .. } => unreachable!("a scalar field has no referent to dangle"),
    };
    Diagnostic::new(
        DiagnosticKind::UnmappableName,
        Locus::at(field.proto().as_str()),
        format!(
            "the field's referent `{}` is not a {kind} of the mapping",
            referent.as_str()
        ),
    )
}

/// `UnknownRootType` at the whole-payload locus for `name`: the messages sharing it as a short
/// name when it is ambiguous (found by a scan of the mapping — the error path only), else a plain
/// miss. `name` is the caller's own argument, so echoing it as given is the doc's contract; the
/// boundary's renderers escape and bound it like any other detail.
fn unknown_root_type(mapping: &Mapping, name: &str) -> Diagnostics {
    let namesakes: Vec<&str> = mapping
        .units()
        .iter()
        .flat_map(|unit| unit.sorts().iter())
        .map(|sort| sort.proto().as_str())
        .filter(|path| final_segment(path) == name)
        .collect();
    let detail = if namesakes.len() > 1 {
        format!(
            "`{name}` names more than one message ({}); give the fully-qualified name",
            namesakes.join(", ")
        )
    } else {
        format!("`{name}` names no message of the schema")
    };
    Diagnostic::new(DiagnosticKind::UnknownRootType, Locus::whole(), detail).into()
}

/// One message of the payload awaiting its walk: the occupant term it hangs from (`parent` — the
/// root constant, or an access-path term `f(P…)`, spec §4.1), the message as a borrowing handle
/// over the decoded tree, its sort, and its compositional depth — the root at 0, a message-typed
/// field's value one deeper than the message carrying it.
struct Work<'a> {
    parent: Term,
    message: SubMessage<'a>,
    sort: SortRef,
    depth: usize,
}

/// The walk's state: the managed stack, every fact emitted so far as its head symbol, and every
/// diagnosis collected. `'m` is the mapping's lifetime, `'a` the decoded tree's.
struct Walker<'m, 'a> {
    mapping: &'m Mapping,
    index: &'m Index,
    stack: Vec<Work<'a>>,
    symbols: Vec<Symbol>,
    diagnostics: Vec<Diagnostic>,
    /// Whether the ceiling has been diagnosed: once per shred, the locus being the whole
    /// payload — a wide over-deep layer adds nothing a second copy would say.
    too_deep: bool,
}

/// Shred one decoded payload: the facts of `message`, an instance of the sort `sort`, hanging from
/// the root term `root` (spec §11). The root's own depth is 0.
///
/// # Errors
///
/// Every diagnosis the walk collected — the §6 refusals at their fields' paths, `UnknownEnumValue`
/// for an undeclared enum number (§7.4), `PayloadTooDeep` past the uniform ceiling — with no facts
/// beside them.
pub(crate) fn shred(
    mapping: &Mapping,
    index: &Index,
    root: Term,
    message: SubMessage<'_>,
    sort: SortRef,
) -> Result<Facts, Diagnostics> {
    run(
        mapping,
        index,
        Work {
            parent: root,
            message,
            sort,
            depth: 0,
        },
    )
}

/// Run the walk from one seed item to the stack's exhaustion.
fn run(mapping: &Mapping, index: &Index, seed: Work<'_>) -> Result<Facts, Diagnostics> {
    let mut walker = Walker {
        mapping,
        index,
        stack: vec![seed],
        symbols: Vec::new(),
        diagnostics: Vec::new(),
        too_deep: false,
    };
    while let Some(work) = walker.stack.pop() {
        walker.visit(&work);
    }
    walker.finish()
}

impl<'m, 'a> Walker<'m, 'a> {
    /// Walk one message: refuse it past the ceiling; else emit its occupancy atom (§4.1 item 4)
    /// and, for each field in number order whose presence holds (§5), lower its value. The
    /// message-typed slots it occupies are pushed in field order — so they are popped, and their
    /// diagnoses collected, in that order.
    fn visit(&mut self, work: &Work<'a>) {
        let sort = work.sort.in_mapping(self.mapping);
        if work.depth > NESTING_CEILING {
            self.refuse_depth(sort, work.depth);
            return;
        }
        self.emit(sort.predicate(), vec![work.parent.clone()]);
        let mut children = Vec::new();
        for field in sort.fields() {
            let present = match field.presence() {
                Totality::Total => true,
                Totality::Partial => work.message.is_present(field.number()),
            };
            if !present {
                continue;
            }
            // A present field reads a value: the view reads `None` only for a singular message
            // the wire did not carry — and every message-typed field is `Partial` (§5), gated
            // above — or for a number the message does not declare, which a mapping walked from
            // the same pool never carries. Discharged loud, never a subtree dropped in silence.
            let Some(value) = work.message.value(field.number()) else {
                unreachable!(
                    "the present field `{}` has no value; a Total message field or an undeclared number is a keryx error",
                    field.proto().as_str()
                )
            };
            self.field(work, field, value, &mut children);
        }
        self.stack.extend(children.into_iter().rev());
    }

    /// Lower one present field under its form (§4.1, §7): the value's shape is the form's — a
    /// singular value for a function or `oneof` arm, elements for a sequence, entries for a map —
    /// since the mapping and the tree derive from one pool; the `Set` form is reserved (§7.1,
    /// Increment 5) and never produced by the policy. Both are discharged at their arms.
    fn field(
        &mut self,
        work: &Work<'a>,
        field: &'m FieldMapping,
        value: FieldValue<'a>,
        children: &mut Vec<Work<'a>>,
    ) {
        let parent = &work.parent;
        match (field.form(), value) {
            (EmitForm::Function | EmitForm::OneofArm { .. }, FieldValue::Scalar(datum)) => {
                self.slot(
                    work,
                    field,
                    vec![parent.clone()],
                    Element::Scalar(datum),
                    children,
                );
            }
            (EmitForm::Function | EmitForm::OneofArm { .. }, FieldValue::Message(child)) => {
                self.slot(
                    work,
                    field,
                    vec![parent.clone()],
                    Element::Message(child),
                    children,
                );
            }
            (EmitForm::Sequence, FieldValue::Elements(elements)) => {
                for (position, element) in elements.into_iter().enumerate() {
                    let Ok(index) = i32::try_from(position) else {
                        self.diagnostics.push(index_out_of_range(field));
                        break;
                    };
                    self.slot(
                        work,
                        field,
                        vec![parent.clone(), terms::int(index)],
                        element,
                        children,
                    );
                }
            }
            (EmitForm::Map { key, key_treatment }, FieldValue::Entries(entries)) => {
                for (map_key, element) in entries {
                    match scalar::lower(
                        Scalar::from(*key),
                        *key_treatment,
                        &Datum::from(map_key),
                        field.proto().as_str(),
                    ) {
                        Ok(key_term) => self.slot(
                            work,
                            field,
                            vec![parent.clone(), key_term],
                            element,
                            children,
                        ),
                        Err(diagnostic) => self.diagnostics.push(diagnostic),
                    }
                }
            }
            (
                EmitForm::Set,
                FieldValue::Scalar(_)
                | FieldValue::Message(_)
                | FieldValue::Elements(_)
                | FieldValue::Entries(_),
            ) => unreachable!(
                "`{}` maps to the `(keryx.set)` membership form, which the policy does not produce until the annotation is read; shredding it is a keryx error",
                field.proto().as_str()
            ),
            (
                EmitForm::Function | EmitForm::OneofArm { .. },
                FieldValue::Elements(_) | FieldValue::Entries(_),
            )
            | (
                EmitForm::Sequence,
                FieldValue::Scalar(_) | FieldValue::Message(_) | FieldValue::Entries(_),
            )
            | (
                EmitForm::Map { .. },
                FieldValue::Scalar(_) | FieldValue::Message(_) | FieldValue::Elements(_),
            ) => unreachable!(
                "the value of `{}` has the shape of its field's form, the mapping and the decoded tree deriving from one descriptor pool; a mismatch is a keryx error",
                field.proto().as_str()
            ),
        }
    }

    /// Lower one occupied slot — a singular field's value, a sequence's element, or a map's value
    /// — whose place is `field.predicate()` over `arguments` (§4.1): a scalar or enum value
    /// becomes the fact `f(P…, V)`; a message becomes the occupant term `f(P…)`, a work item whose
    /// occupancy atom and fields are emitted when it is popped. A datum's kind is its field's
    /// kind, the decode giving a datum the kind of its field and the policy giving the field the
    /// mapping of its kind — the scalar arm hands both to `scalar::lower`, whose table discharges
    /// the pairing; the enum and message arms discharge theirs here.
    fn slot(
        &mut self,
        work: &Work<'a>,
        field: &'m FieldMapping,
        mut arguments: Vec<Term>,
        element: Element<'a>,
        children: &mut Vec<Work<'a>>,
    ) {
        let at = field.proto().as_str();
        let lowered = match (field.value(), element) {
            (ValueMapping::Scalar { kind, treatment }, Element::Scalar(datum)) => {
                scalar::lower(*kind, *treatment, &datum, at)
            }
            (ValueMapping::Enum(predicate), Element::Scalar(Datum::Enum(number))) => {
                self.enum_constant(predicate, number, at)
            }
            (ValueMapping::Message(predicate), Element::Message(child)) => {
                // `Index::build` resolved every referent of the mapping before any walk.
                let sort = self
                    .index
                    .sort_of(predicate)
                    .expect("every message referent of the mapping is a sort of its index");
                children.push(Work {
                    parent: terms::apply(field.predicate().clone(), arguments),
                    message: child,
                    sort,
                    depth: work.depth + 1,
                });
                return;
            }
            (
                ValueMapping::Enum(_),
                Element::Scalar(
                    Datum::I32(_)
                    | Datum::I64(_)
                    | Datum::U32(_)
                    | Datum::U64(_)
                    | Datum::F64(_)
                    | Datum::Bool(_)
                    | Datum::Str(_)
                    | Datum::Bytes(_),
                ),
            )
            | (ValueMapping::Message(_), Element::Scalar(_))
            | (ValueMapping::Scalar { .. } | ValueMapping::Enum(_), Element::Message(_)) => {
                unreachable!(
                    "the value of `{at}` is of its field's kind, the mapping and the decoded tree deriving from one descriptor pool; a mismatch is a keryx error"
                )
            }
        };
        match lowered {
            Ok(term) => {
                arguments.push(term);
                self.emit(field.predicate(), arguments);
            }
            Err(diagnostic) => self.diagnostics.push(diagnostic),
        }
    }

    /// The constant an enum value's wire `number` lowers to (spec §7.4): the declared value of
    /// that number in the referent enum's mapping — the first in `values()` iteration order, should
    /// an alias share the number — or `UnknownEnumValue` at `at` for a number the enum does not
    /// declare.
    fn enum_constant(&self, predicate: &Name, number: i32, at: &str) -> Result<Term, Diagnostic> {
        // `Index::build` resolved every referent of the mapping before any walk.
        let enumeration = self
            .index
            .enum_of(predicate)
            .expect("every enum referent of the mapping is an enum of its index")
            .in_mapping(self.mapping);
        enumeration
            .values()
            .iter()
            .find(|value| value.number() == number)
            .map(|value| terms::apply(value.constant().clone(), Vec::new()))
            .ok_or_else(|| unknown_enum_value(enumeration, number, at))
    }

    /// Emit one fact `predicate(arguments…)` as its head symbol, through themelios's own
    /// canonicalization at `crate::terms` — the fact's one representation; the `.lp` seam's
    /// statement is derived from it at the rendering, so the seams carry identical content
    /// (spec §11).
    fn emit(&mut self, predicate: &Name, arguments: Vec<Term>) {
        self.symbols
            .push(terms::atom_symbol(predicate.clone(), arguments));
    }

    /// Refuse a message past the ceiling: `PayloadTooDeep` at the whole-payload locus, once per
    /// shred; the message and everything beneath it are left unwalked, and the walk goes on
    /// collecting elsewhere.
    fn refuse_depth(&mut self, sort: &SortMapping, depth: usize) {
        if self.too_deep {
            return;
        }
        self.too_deep = true;
        self.diagnostics.push(Diagnostic::new(
            DiagnosticKind::PayloadTooDeep,
            Locus::whole(),
            format!(
                "a `{}` value sits {depth} levels of message-typed fields below the root, past keryx's payload nesting ceiling of {NESTING_CEILING}",
                sort.proto().as_str()
            ),
        ));
    }

    /// The walk's result: every diagnosis, or every fact — the symbols in `Symbol::Ord`.
    fn finish(self) -> Result<Facts, Diagnostics> {
        if let Some(diagnostics) = Diagnostics::collect(self.diagnostics) {
            return Err(diagnostics);
        }
        let mut symbols = self.symbols;
        symbols.sort_unstable();
        Ok(Facts { symbols })
    }
}

/// `UnknownEnumValue` at `at`: the number and the enum, with §7.4's opt-in named — never a
/// constant the value is not.
fn unknown_enum_value(enumeration: &EnumMapping, number: i32, at: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::UnknownEnumValue,
        Locus::at(at),
        format!(
            "the value {number} matches no declared value of the enum `{}`; an unknown number of an open enum is a translation error by default (§7.4) — annotate the field `(keryx.unknown) = PRESERVE` to carry it as `unknown({number})`",
            enumeration.proto().as_str()
        ),
    )
}

/// `ValueOutOfRange` at the sequence field: an element's index past `i32::MAX`, the widest integer
/// a native clingo term carries (§6) — a sequence that long is beyond any payload the door admits,
/// but the bound is checked, not assumed.
fn index_out_of_range(field: &FieldMapping) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::ValueOutOfRange,
        Locus::at(field.proto().as_str()),
        format!(
            "the sequence carries more than {} elements, so an element's index exceeds the widest integer a native clingo term carries",
            i32::MAX
        ),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use keryx_test_support::wire::delimited;
    use prost::encoding;
    use themelios_program::prelude::*;

    use super::{Index, NESTING_CEILING, Work, run};
    use crate::codec::engine;
    use crate::descriptor::{self, RECURSION_LIMIT, RetainedPool};
    use crate::diagnostics::{Diagnostic, DiagnosticKind};
    use crate::policy::{self, EmitForm, FieldMapping, Mapping, Totality, ValueMapping};
    use crate::terms;

    /// The proto3 presence fixture's mapping and pool, through the retaining descriptor door.
    fn proto3() -> (Mapping, RetainedPool) {
        let (schema, pool) =
            descriptor::ingest_retaining(&keryx_test_support::compile_fixture("proto3.proto"))
                .expect("the fixture ingests");
        (policy::map(&schema).expect("maps"), pool)
    }

    /// The thermal example's mapping and pool, through the retaining source door.
    fn thermal() -> (Mapping, RetainedPool) {
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
        let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
        let (schema, pool) = descriptor::source::compile_retaining(
            &[example.join("thermal.proto")],
            &[example, vendored],
        )
        .expect("the thermal example compiles");
        (policy::map(&schema).expect("maps"), pool)
    }

    /// A thermal `Reading { sensor = 1; temp_c = 2 }` on the wire.
    fn reading() -> Vec<u8> {
        let mut reading = Vec::new();
        delimited(1, b"s-101", &mut reading);
        encoding::int32::encode(2, &44, &mut reading);
        reading
    }

    /// A batch of one reading on the wire.
    fn one_reading_batch() -> Vec<u8> {
        let mut bytes = Vec::new();
        delimited(1, &reading(), &mut bytes);
        bytes
    }

    /// The field of `mapping` at the fully-qualified `path`, for a test to alter — the mapping
    /// being the policy's, no public door constructs an inconsistent one.
    fn field_mut<'m>(mapping: &'m mut Mapping, path: &str) -> &'m mut FieldMapping {
        mapping
            .units
            .iter_mut()
            .flat_map(|unit| unit.sorts.iter_mut())
            .flat_map(|sort| sort.fields.iter_mut())
            .find(|field| field.proto.as_str() == path)
            .expect("a field of the mapping")
    }

    /// Walk `bytes`, an instance of the thermal `root_type`, seeded at `depth`, over a mapping
    /// `alter` may have changed: the fact count, or the kinds of the diagnoses.
    fn run_from(
        depth: usize,
        root_type: &str,
        bytes: &[u8],
        alter: impl FnOnce(&mut Mapping),
    ) -> Result<usize, Vec<DiagnosticKind>> {
        run_over(thermal(), depth, root_type, bytes, alter)
    }

    /// As [`run_from`], over any schema's mapping and pool.
    fn run_over(
        (mut mapping, pool): (Mapping, RetainedPool),
        depth: usize,
        root_type: &str,
        bytes: &[u8],
        alter: impl FnOnce(&mut Mapping),
    ) -> Result<usize, Vec<DiagnosticKind>> {
        alter(&mut mapping);
        let index = Index::build(&mapping).expect("the index builds");
        let sort = index.root(&mapping, root_type).expect("the root resolves");
        let descriptor = pool
            .message_by_name(sort.in_mapping(&mapping).proto().as_str())
            .expect("declared");
        let decoded = engine::decode_binary(&descriptor, bytes).expect("decodes");
        run(
            &mapping,
            &index,
            Work {
                parent: terms::constant("r0"),
                message: decoded.root(),
                sort,
                depth,
            },
        )
        .map(|facts| facts.symbols().len())
        .map_err(|diagnostics| diagnostics.iter().map(Diagnostic::kind).collect())
    }

    /// Walk the one-reading batch from the root, over a mapping `alter` may have changed.
    fn walk_batch(alter: impl FnOnce(&mut Mapping)) -> Result<usize, Vec<DiagnosticKind>> {
        run_from(0, "ReadingBatch", &one_reading_batch(), alter)
    }

    #[test]
    #[should_panic(expected = "has no value")]
    fn a_total_message_field_the_wire_did_not_carry_is_a_keryx_error() {
        // A singular message field is `Partial` from every real descriptor (§5); a mapping that
        // marks one `Total` and a payload that omits it would otherwise drop the field's
        // occupancy atom and its whole subtree in silence — so the discharge is loud, like every
        // other can't-happen of the walk.
        let _ = run_over(proto3(), 0, "Reading", &[], |mapping| {
            field_mut(mapping, "keryx.p3.Reading.detail").presence = Totality::Total;
        });
    }

    #[test]
    fn the_ceiling_is_one_below_the_engine_s_recursion_limit_and_refuses_the_level_past_it() {
        // The uniform ceiling: a message at nesting level 99 walks; one at level 100 is
        // `PayloadTooDeep`. The counter is seeded past the ceiling here, as a deeper-admitting
        // format's decoder leaves it; a binary payload reaches level 100 exactly — the engine
        // decodes that level and refuses the next — which `tests/codec_depth.rs` pins through the
        // door.
        assert_eq!(NESTING_CEILING, RECURSION_LIMIT - 1);
        assert_eq!(NESTING_CEILING, 99);
        assert_eq!(
            run_from(NESTING_CEILING, "Reading", &reading(), |_| {}),
            Ok(3),
            "the reading and its two fields"
        );
        assert_eq!(
            run_from(NESTING_CEILING + 1, "Reading", &reading(), |_| {}),
            Err(vec![DiagnosticKind::PayloadTooDeep])
        );
        // The ceiling counts message-typed nesting below the root: a batch seeded at the
        // ceiling walks, but its reading — one level deeper — is past it.
        assert_eq!(
            run_from(
                NESTING_CEILING,
                "ReadingBatch",
                &one_reading_batch(),
                |_| {}
            ),
            Err(vec![DiagnosticKind::PayloadTooDeep])
        );
    }

    #[test]
    fn the_ceiling_is_diagnosed_once_per_shred_at_the_whole_payload_locus() {
        // Two readings seeded so that both sit past the ceiling: one diagnosis, not one per
        // over-deep item (the locus is the whole payload; a wide layer adds nothing), and the
        // walk still delivers no facts beside it.
        let (mapping, pool) = thermal();
        let index = Index::build(&mapping).expect("builds");
        let mut bytes = Vec::new();
        delimited(1, &reading(), &mut bytes);
        delimited(1, &reading(), &mut bytes);
        let batch = pool
            .message_by_name("thermal.v1.ReadingBatch")
            .expect("declared");
        let decoded = engine::decode_binary(&batch, &bytes).expect("decodes");
        let diagnostics = run(
            &mapping,
            &index,
            Work {
                parent: terms::constant("r0"),
                message: decoded.root(),
                sort: index.root(&mapping, "ReadingBatch").expect("resolves"),
                depth: NESTING_CEILING,
            },
        )
        .expect_err("two readings past the ceiling");
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.iter().next().expect("one diagnostic");
        assert_eq!(diagnostic.kind(), DiagnosticKind::PayloadTooDeep);
        assert!(diagnostic.locus().is_whole());
        assert!(
            diagnostic.detail().contains("thermal.v1.Reading")
                && diagnostic.detail().contains("100"),
            "the detail names the over-deep sort and its depth: {diagnostic}"
        );
    }

    #[test]
    fn the_index_resolves_a_root_by_path_or_unique_short_name() {
        let (mapping, _) = thermal();
        let index = Index::build(&mapping).expect("builds");
        let by_path = index
            .root(&mapping, "thermal.v1.ReadingBatch")
            .expect("resolves");
        let by_short = index.root(&mapping, "ReadingBatch").expect("resolves");
        assert_eq!(by_path, by_short);
        assert_eq!(
            by_path.in_mapping(&mapping).proto().as_str(),
            "thermal.v1.ReadingBatch"
        );
        let miss = index
            .root(&mapping, "Nowhere")
            .expect_err("no such message");
        assert_eq!(
            miss.iter().next().expect("one").kind(),
            DiagnosticKind::UnknownRootType
        );
    }

    #[test]
    fn a_dangling_referent_is_diagnosed_when_the_index_is_built() {
        // The closed world the walk assumes, checked at the build: a field naming a sort or enum
        // the mapping lacks (unreachable through the policy) is `UnmappableName` at the field,
        // never an `expect` met mid-walk.
        let (mut mapping, _) = thermal();
        field_mut(&mut mapping, "thermal.v1.ReadingBatch.readings").value =
            ValueMapping::Message(Name::new("nowhere").expect("an identifier"));
        field_mut(&mut mapping, "thermal.v1.Reading.temp_c").value =
            ValueMapping::Enum(Name::new("no_enum").expect("an identifier"));
        let diagnostics = Index::build(&mapping).expect_err("two dangling referents");
        let located: Vec<(DiagnosticKind, Option<&str>)> = diagnostics
            .iter()
            .map(|d| (d.kind(), d.locus().path()))
            .collect();
        assert_eq!(
            located,
            [
                (
                    DiagnosticKind::UnmappableName,
                    Some("thermal.v1.Reading.temp_c")
                ),
                (
                    DiagnosticKind::UnmappableName,
                    Some("thermal.v1.ReadingBatch.readings")
                ),
            ]
        );
    }

    #[test]
    fn a_shared_sort_predicate_is_diagnosed_when_the_index_is_built() {
        // Injectivity of the `/1` namespace (§4.2) is the policy's guarantee; the index checks
        // it rather than letting a second sort silently shadow the first.
        let (mut mapping, _) = thermal();
        let alert = Name::new("alert").expect("an identifier");
        for sort in &mut mapping.units[0].sorts {
            if sort.proto.as_str() == "thermal.v1.Reading" {
                sort.predicate = alert.clone();
            }
        }
        let diagnostics = Index::build(&mapping).expect_err("a shared predicate");
        let diagnostic = diagnostics.iter().next().expect("one");
        assert_eq!(diagnostic.kind(), DiagnosticKind::UnmappableName);
        assert_eq!(diagnostic.locus().path(), Some("thermal.v1.Reading"));
    }

    #[test]
    #[should_panic(expected = "shredding it is a keryx error")]
    fn the_set_form_is_a_discharged_can_t_happen_not_a_silent_wildcard() {
        // The policy never produces `Set` until `(keryx.set)` is read (Increment 5): reaching the
        // arm is a keryx error, loud — never a sequence shredded as a set or vice versa.
        let _ = walk_batch(|mapping| {
            field_mut(mapping, "thermal.v1.ReadingBatch.readings").form = EmitForm::Set;
        });
    }

    #[test]
    #[should_panic(expected = "has the shape of its field's form")]
    fn a_form_the_value_s_shape_contradicts_is_a_keryx_error() {
        // A sequence's elements under a singular form: impossible from one pool, loud if a
        // mapping ever disagrees with the tree it is walked against.
        let _ = walk_batch(|mapping| {
            field_mut(mapping, "thermal.v1.ReadingBatch.readings").form = EmitForm::Function;
        });
    }

    #[test]
    #[should_panic(expected = "is of its field's kind")]
    fn a_value_kind_the_datum_contradicts_is_a_keryx_error() {
        // A message referent on a field whose datum is a string: the same discharge, on the
        // value axis.
        let _ = walk_batch(|mapping| {
            field_mut(mapping, "thermal.v1.Reading.sensor").value =
                ValueMapping::Message(Name::new("reading").expect("an identifier"));
        });
    }
}
