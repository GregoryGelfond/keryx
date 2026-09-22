//! The reassembly walk (architecture §5, outbound; spec §12.3) — the top-down mirror of
//! [`walk::shred`]. One `emit_<sort>(root)` marker names a message to rebuild; the walk reads the
//! answer set's field and occupancy atoms through a [`SlotIndex`], reconstructs the message tree,
//! and encodes it. No prost-reflect type crosses out: the walk builds through [`engine::Building`]
//! and the [`SlotIndex`] holds only answer-set [`Symbol`]s and mapping positions.
//!
//! **The slot index (spec §12.1, the "which slot, of which sort" crux).** A field's atoms and a
//! message slot's children are found by one key — `(field predicate, parent occupant)` — filled by
//! two rules: a **field atom** `f(P, …)` files under `(f, P)` by its predicate and first argument;
//! an **occupancy atom** `u(t)` — recognised because `u` is a sort ([`Index::sort_of`]) — files its
//! occupant `t = f(P, …)` under `(f, P)` by *deconstructing* `t`. So one query yields a scalar
//! field's atoms and a message field's child occupants alike, and the walk joins on occupancy
//! without the `views.lp` relations (§4.1). The answer set's solver-working predicates (`reach`,
//! `has_*`, `ok_*`) are keyed by neither rule and never queried; `violates(path, occupant)` atoms
//! are collected apart, for the diagnostic theory's reports.
//!
//! **Two phases, an explicit heap stack, no native recursion (the threat model's property 3,
//! branch (b)).** Discovery walks top-down from the root on a managed stack, refusing an occupant
//! chain past [`walk::NESTING_CEILING`] (`ReassembledTooDeep`) *before* anything is built, and
//! plans each occupant's fields — raising each scalar once ([`scalar::raise`]), resolving each enum,
//! ordering each sequence, keying each map — collecting every refusal. The build then runs only if
//! discovery found none (every message or every diagnosis, never partial — property 4): it
//! constructs each occupant bottom-up (children before parents, discovery order reversed) into a
//! [`engine::Building`] and encodes the root in the wire form `format` names — the binary wire, the
//! protobuf text format, or canonical JSON (an answer set read from an `.lp` module by
//! `codec::answer` reassembles the same way).

use std::collections::{BTreeMap, BTreeSet};

use prost_reflect::{MapKey, Value};
use themelios_program::prelude::*;

use crate::codec::PayloadFormat;
use crate::codec::engine::{self, Building};
use crate::codec::scalar;
use crate::codec::walk::{self, Index, SortRef};
use crate::descriptor::RetainedPool;
use crate::descriptor::model::Scalar;
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::policy::model::{
    EmitForm, EnumMapping, FieldMapping, Mapping, SortMapping, Totality, Unit, ValueMapping,
};
use crate::policy::names;

/// The answer set's atoms, keyed for the walk: each slot's entries by `(field predicate, parent
/// occupant)`, the `emit_<sort>(root)` markers to rebuild, the `violates(…)` atoms to report, and
/// the diagnostics for field atoms that descend from a marker root but whose parent occupant no
/// occupancy atom declares (property 4's orphan refusal, scoped to reachability — §12.1). Built once
/// per reassemble over the whole answer set (`'a`), holding references into it.
pub(crate) struct SlotIndex<'a> {
    slots: BTreeMap<(Name, Symbol), Vec<&'a Symbol>>,
    markers: Vec<(SortRef, &'a Symbol)>,
    violations: Vec<&'a Symbol>,
    orphans: Vec<Diagnostic>,
}

impl<'a> SlotIndex<'a> {
    /// Key `answer_set` for the walk (see the module doc). `index` classifies a predicate as a sort
    /// (an occupancy atom) or not; `mapping` gives the marker and field predicates. Every entry is a
    /// reference into `answer_set`.
    pub(crate) fn build(
        mapping: &Mapping,
        index: &Index,
        answer_set: &'a [Symbol],
    ) -> SlotIndex<'a> {
        let markers_by_predicate: BTreeMap<Name, SortRef> = mapping
            .units()
            .iter()
            .flat_map(Unit::sorts)
            .filter_map(|sort| {
                index
                    .sort_of(sort.predicate())
                    .map(|reference| (names::marker(sort.predicate()), reference))
            })
            .collect();
        // The arities a field of each predicate takes — name AND arity identify an ASP predicate, so
        // a field atom whose arity no field of that name takes is a *different* predicate, filed below
        // into no slot and counted as no orphan (the model's private business, §12.1), exactly as the
        // marker and occupancy branches ignore a wrong-arity `[root]`/`[occupant]`.
        let mut arities_by_predicate: BTreeMap<Name, BTreeSet<usize>> = BTreeMap::new();
        for field in mapping
            .units()
            .iter()
            .flat_map(Unit::sorts)
            .flat_map(SortMapping::fields)
        {
            arities_by_predicate
                .entry(field.predicate().clone())
                .or_default()
                .insert(expected_arity(field));
        }
        let violates = names::violates();

        let mut slots: BTreeMap<(Name, Symbol), Vec<&'a Symbol>> = BTreeMap::new();
        let mut markers = Vec::new();
        let mut violations = Vec::new();
        // For the orphan check (property 4, scoped to reachability — spec §12.1, arch §7): the
        // occupants an atom declares (an occupancy atom's occupant, a marker's root), the marker
        // roots that source reachability, and every field atom's parent. A field atom whose parent is
        // undeclared yet descends from a marker root — a child positioned within an exported tree but
        // missing its occupancy atom — is a refused orphan (real dropped data); one descending from
        // no marker is the model's private business (§12.1), ignored, never refused.
        let mut declared: BTreeSet<&Symbol> = BTreeSet::new();
        let mut marker_roots: BTreeSet<&'a Symbol> = BTreeSet::new();
        let mut field_parents: Vec<(&'a Symbol, &'a Symbol)> = Vec::new();

        for atom in answer_set {
            let Symbol::Function {
                name,
                arguments,
                sign: Sign::Positive,
            } = atom
            else {
                continue; // a bare number/string/tuple or a strongly-negated atom is not a fact keryx reads
            };
            if let Some(&reference) = markers_by_predicate.get(name) {
                if let [root] = arguments.as_slice() {
                    markers.push((reference, atom));
                    declared.insert(root);
                    marker_roots.insert(root);
                }
            } else if index.sort_of(name).is_some() {
                // An occupancy atom `u(occupant)`: file the occupant under its (functor, parent).
                if let [occupant] = arguments.as_slice() {
                    declared.insert(occupant);
                    if let Symbol::Function {
                        name: field,
                        arguments: occupant_args,
                        ..
                    } = occupant
                        && let Some(parent) = occupant_args.first()
                    {
                        slots
                            .entry((field.clone(), parent.clone()))
                            .or_default()
                            .push(occupant);
                    }
                }
            } else if *name == violates {
                violations.push(atom);
            } else if let Some(arities) = arities_by_predicate.get(name) {
                // A field atom `f(P, …)`: file it under `(f, P)` by predicate and first argument, but
                // only when its arity is one a field of that name takes — an atom of the name but
                // another arity is a different predicate, ignored here (filed into no slot, counted as
                // no orphan) so the slot, the field planner, and the orphan pass agree by construction.
                if arities.contains(&arguments.len())
                    && let Some(parent) = arguments.first()
                {
                    slots
                        .entry((name.clone(), parent.clone()))
                        .or_default()
                        .push(atom);
                    field_parents.push((parent, atom));
                }
            }
            // else: a solver-working predicate (`reach`, `has_*`, `ok_*`) — keyed by neither rule.
        }

        let orphans = field_parents
            .into_iter()
            .filter(|(parent, _)| {
                !declared.contains(parent) && descends_from_marker(parent, &marker_roots)
            })
            .map(|(_, atom)| orphan(atom))
            .collect();

        SlotIndex {
            slots,
            markers,
            violations,
            orphans,
        }
    }

    /// The `emit_<sort>(root)` marker atoms naming messages to rebuild, each with its sort. In the
    /// answer set's order; the caller orders the results by the marker atom's `Symbol::Ord`
    /// ([`marker_root`] reads the root occupant from one).
    pub(crate) fn markers(&self) -> &[(SortRef, &'a Symbol)] {
        &self.markers
    }

    /// The `violates(path, occupant)` atoms present — the diagnostic theory's reports, refused
    /// mode-free (property 4).
    pub(crate) fn violations(&self) -> &[&'a Symbol] {
        &self.violations
    }

    /// The refusals for field atoms that descend from a marker root yet whose parent occupant no
    /// occupancy atom declares — a child within an exported tree missing its sort (property 4, scoped
    /// to reachability; §12.1). An atom descending from no marker is private business, not here.
    pub(crate) fn orphans(&self) -> &[Diagnostic] {
        &self.orphans
    }

    /// The entries filed under one slot — a scalar field's atoms, or a message field's child
    /// occupants — or an empty slice when the slot is unset.
    fn slot(&self, field: &Name, parent: &Symbol) -> &[&'a Symbol] {
        // `get` needs an owned key; the clones are bounded by the field count, per query.
        self.slots
            .get(&(field.clone(), parent.clone()))
            .map_or(&[], Vec::as_slice)
    }
}

/// Rebuild one message — the `sort` instance named by the `root` occupant — from the answer set,
/// and encode it to the wire form `format` names. Every message or every diagnosis, never a partial
/// build beside a diagnosis (§6, property 4): discovery collects every refusal, and the build (and
/// the encode) run only if there is none.
///
/// # Errors
///
/// `ReassembledTooDeep` past the ceiling; `ShapeViolation` for a duplicate singular, a non-dense
/// sequence, a oneof with two arms, a missing total field, or a present `violates` atom naming this
/// root's occupants; `TermTypeMismatch`/`ValueOutOfRange`/`UnknownEnumValue`/`UnannotatedFloat` for
/// a value that does not lower to its field's type; `UnrepresentableJson` for a well-known-type
/// value canonical JSON cannot represent (the JSON form only); `DependencyFault` for a contained
/// encode fault.
pub(crate) fn assemble(
    mapping: &Mapping,
    index: &Index,
    pool: &RetainedPool,
    slots: &SlotIndex<'_>,
    root: &Symbol,
    sort: SortRef,
    format: PayloadFormat,
) -> Result<Vec<u8>, Diagnostics> {
    let mut walker = Assembler {
        mapping,
        index,
        slots,
        diagnostics: Vec::new(),
        plans: Vec::new(),
        too_deep: false,
    };
    walker.discover(root.clone(), sort);
    if let Some(diagnostics) = Diagnostics::collect(std::mem::take(&mut walker.diagnostics)) {
        return Err(diagnostics);
    }
    walker.build(pool, format)
}

/// One occupant to rebuild: the message `sort` instance under `occupant`, at `depth` below its root.
struct Discover {
    occupant: Symbol,
    sort: SortRef,
    depth: usize,
}

/// A planned occupant — its `occupant` term, its `sort`, and how to set each of its fields — built
/// by discovery, consumed by the build in reverse (children before parents).
struct Plan {
    occupant: Symbol,
    sort: SortRef,
    fields: Vec<Planned>,
}

/// How one field is set on its message: a raised scalar/enum value, a sequence of them, a map of
/// them, or a message slot whose value is the child occupant's built message (looked up by term).
enum Planned {
    Value {
        number: i32,
        value: Value,
    },
    List {
        number: i32,
        values: Vec<Value>,
    },
    Map {
        number: i32,
        entries: Vec<(MapKey, Value)>,
    },
    Message {
        number: i32,
        child: Symbol,
    },
    Messages {
        number: i32,
        children: Vec<Symbol>,
    },
    MessageMap {
        number: i32,
        entries: Vec<(MapKey, Symbol)>,
    },
}

/// The walk's state: the discovered plans in top-down order, and every diagnosis collected.
struct Assembler<'m, 'a> {
    mapping: &'m Mapping,
    index: &'m Index,
    slots: &'m SlotIndex<'a>,
    diagnostics: Vec<Diagnostic>,
    plans: Vec<Plan>,
    /// Whether the ceiling has been diagnosed: once per reassemble, the locus the whole answer set.
    too_deep: bool,
}

impl Assembler<'_, '_> {
    /// Discover and validate the tree top-down on a managed stack (the mirror of `walk::run`):
    /// refuse an occupant past the ceiling before it is planned; else plan each field. Children are
    /// pushed so a parent's plan is recorded before its descendants' — the build then reverses the
    /// order to construct children first.
    fn discover(&mut self, root: Symbol, sort: SortRef) {
        let mut stack = vec![Discover {
            occupant: root,
            sort,
            depth: 0,
        }];
        while let Some(work) = stack.pop() {
            let sort = work.sort.in_mapping(self.mapping);
            if work.depth > walk::NESTING_CEILING {
                self.refuse_depth(sort, work.depth);
                continue;
            }
            let mut fields = Vec::new();
            for field in sort.fields() {
                self.plan_field(&work, field, &mut fields, &mut stack);
            }
            self.plans.push(Plan {
                occupant: work.occupant,
                sort: work.sort,
                fields,
            });
        }
    }

    /// Plan one field from its slot under the form the mapping fixes (§4.1, §7): a singular value, a
    /// sequence, a map, or a oneof arm — the answer set's shape is checked against it, never trusted.
    /// The slot index filed only atoms whose arity a field of this predicate takes (name AND arity
    /// identify an ASP predicate; a wrong-arity atom is a different predicate, ignored as the model's
    /// private business, §12.1 — the posture the marker and occupancy atoms keep with
    /// `[root]`/`[occupant]`), so a slot holds more than this *field*'s exact arity only for a predicate
    /// shared across sorts at different arities; the planner filters again to that arity (a scalar atom
    /// `f(P, V)`/`f(P, I, V)`/`f(P, K, V)`, a message occupant `f(P)`/`f(P, I)`/`f(P, K)` —
    /// [`expected_arity`]) only then, borrowing the slot slice unchanged in the common case.
    fn plan_field(
        &mut self,
        work: &Discover,
        field: &FieldMapping,
        fields: &mut Vec<Planned>,
        stack: &mut Vec<Discover>,
    ) {
        let arity = expected_arity(field);
        let slot = self.slots.slot(field.predicate(), &work.occupant);
        // The index already filed only atoms whose arity a field of this name takes, so a slot holds
        // more than this field's arity only for a predicate shared across sorts at *different* arities;
        // borrow the slice when it already matches (the common case, no allocation), filter only then.
        let filtered: Vec<&Symbol>;
        let entries: &[&Symbol] = if slot.iter().all(|&entry| is_function_of_arity(entry, arity)) {
            slot
        } else {
            filtered = slot
                .iter()
                .copied()
                .filter(|&entry| is_function_of_arity(entry, arity))
                .collect();
            &filtered
        };
        match field.form() {
            EmitForm::Function | EmitForm::OneofArm { .. } => {
                self.plan_singular(work, field, entries, fields, stack);
            }
            EmitForm::Sequence => self.plan_sequence(work, field, entries, fields, stack),
            EmitForm::Map { .. } => self.plan_map(work, field, entries, fields, stack),
            // A set (§7.1) is not produced by the mapping until its annotation lands (Increment 5),
            // so no answer set the walk dispatches from carries the form: planning one is a keryx
            // error in the mapping, discharged loud as `walk::run` discharges the same inbound.
            EmitForm::Set => unreachable!(
                "the mapping produces no `Set` form before Increment 5; planning one is a keryx error"
            ),
        }
    }

    /// A singular field: at most one entry. Absent is unset for a partial field, but a `Total`
    /// (implicit-presence) field missing its value is a `ShapeViolation` — a serializable answer set
    /// carries it (§5, §12.2). A message slot's one child occupant is recursed into; a scalar/enum's
    /// value is raised.
    fn plan_singular(
        &mut self,
        work: &Discover,
        field: &FieldMapping,
        entries: &[&Symbol],
        fields: &mut Vec<Planned>,
        stack: &mut Vec<Discover>,
    ) {
        let [entry] = entries else {
            if entries.is_empty() {
                // A `Required` (proto2 `required`) field is totality-obliged outbound as a
                // `Total` (IMPLICIT) one is — an answer set omitting it is a shape violation.
                if matches!(field.presence(), Totality::Total | Totality::Required) {
                    self.diagnostics.push(missing_total(field));
                }
            } else {
                self.diagnostics.push(duplicate_singular(field));
            }
            return;
        };
        match field.value() {
            ValueMapping::Message(referent) => {
                let child = self.plan_child(work, referent, entry, stack);
                fields.push(Planned::Message {
                    number: field.number(),
                    child,
                });
            }
            _ => {
                if let Some(value) = self.value(field, entry) {
                    fields.push(Planned::Value {
                        number: field.number(),
                        value,
                    });
                }
            }
        }
    }

    /// A sequence field: its entries indexed 0..n, dense from 0 (§7.1) — a gap or a duplicate index
    /// is a `ShapeViolation` — each a scalar/enum value raised or a message child recursed into.
    fn plan_sequence(
        &mut self,
        work: &Discover,
        field: &FieldMapping,
        entries: &[&Symbol],
        fields: &mut Vec<Planned>,
        stack: &mut Vec<Discover>,
    ) {
        // Order by index (arg after the parent); refuse a non-integer index, a gap, or a duplicate.
        let mut indexed: Vec<(i32, &Symbol)> = Vec::with_capacity(entries.len());
        for entry in entries {
            if let Some(Symbol::Number(index)) = place(entry) {
                indexed.push((*index, entry));
            } else {
                self.diagnostics
                    .push(shape(field, "a sequence element has no integer index"));
                return;
            }
        }
        indexed.sort_by_key(|(index, _)| *index);
        for (position, (index, _)) in indexed.iter().enumerate() {
            let expected = i32::try_from(position).ok();
            if Some(*index) != expected {
                self.diagnostics
                    .push(shape(field, "a sequence is not dense from index 0"));
                return;
            }
        }
        if let ValueMapping::Message(referent) = field.value() {
            let mut children = Vec::with_capacity(indexed.len());
            for (_, entry) in &indexed {
                children.push(self.plan_child(work, referent, entry, stack));
            }
            fields.push(Planned::Messages {
                number: field.number(),
                children,
            });
        } else {
            let mut values = Vec::with_capacity(indexed.len());
            for (_, entry) in &indexed {
                if let Some(value) = self.value(field, entry) {
                    values.push(value);
                }
            }
            fields.push(Planned::List {
                number: field.number(),
                values,
            });
        }
    }

    /// A map field: its entries keyed by the map key (arg after the parent), raised under the key's
    /// §6 treatment; a duplicate key is a `ShapeViolation`. A value is a scalar/enum raised or a
    /// message child recursed into.
    fn plan_map(
        &mut self,
        work: &Discover,
        field: &FieldMapping,
        entries: &[&Symbol],
        fields: &mut Vec<Planned>,
        stack: &mut Vec<Discover>,
    ) {
        let EmitForm::Map { key, key_treatment } = field.form() else {
            unreachable!("plan_field routes only a map field to plan_map")
        };
        let (key, key_treatment) = (*key, *key_treatment);
        let mut seen: BTreeSet<MapKey> = BTreeSet::new();
        let is_message = matches!(field.value(), ValueMapping::Message(_));
        let mut scalar_entries = Vec::new();
        let mut message_entries = Vec::new();
        for entry in entries {
            let Some(key_symbol) = place(entry) else {
                self.diagnostics
                    .push(shape(field, "a map entry has no key"));
                return;
            };
            let key_value = match scalar::raise(
                key_symbol,
                Scalar::from(key),
                key_treatment,
                field.proto().as_str(),
            ) {
                Ok(value) => value,
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    continue;
                }
            };
            let Some(map_key) = key_value.into_map_key() else {
                self.diagnostics
                    .push(shape(field, "a map key is not a scalar key type"));
                continue;
            };
            if !seen.insert(map_key.clone()) {
                self.diagnostics
                    .push(shape(field, "a map has two entries for one key"));
                continue;
            }
            if is_message {
                if let ValueMapping::Message(referent) = field.value() {
                    let child = self.plan_child(work, referent, entry, stack);
                    message_entries.push((map_key, child));
                }
            } else if let Some(value) = self.value(field, entry) {
                scalar_entries.push((map_key, value));
            }
        }
        if is_message {
            fields.push(Planned::MessageMap {
                number: field.number(),
                entries: message_entries,
            });
        } else {
            fields.push(Planned::Map {
                number: field.number(),
                entries: scalar_entries,
            });
        }
    }

    /// Push a message slot's child occupant for discovery (the occupant term `f(P[, I | K])`), and
    /// return it for the parent's plan. The referent sort is resolved from the mapping (`Index` built
    /// it before any walk), so a message field always names a sort — a miss is a keryx error,
    /// discharged loud (as `walk::run` discharges the same inbound), never carried into the build.
    fn plan_child(
        &mut self,
        work: &Discover,
        referent: &Name,
        occupant: &Symbol,
        stack: &mut Vec<Discover>,
    ) -> Symbol {
        let sort = self
            .index
            .sort_of(referent)
            .expect("every message referent of the mapping is a sort of its index");
        stack.push(Discover {
            occupant: occupant.clone(),
            sort,
            depth: work.depth + 1,
        });
        occupant.clone()
    }

    /// Raise one scalar or enum entry to its value (the inverse §6 policy), or collect its refusal.
    /// The value is the entry atom's last argument — `f(P, V)`, `f(P, I, V)`, `f(P, K, V)`.
    fn value(&mut self, field: &FieldMapping, entry: &Symbol) -> Option<Value> {
        let Some(symbol) = last_argument(entry) else {
            self.diagnostics
                .push(shape(field, "a field atom carries no value"));
            return None;
        };
        let at = field.proto().as_str();
        let lowered = match field.value() {
            ValueMapping::Scalar { kind, treatment } => {
                scalar::raise(symbol, *kind, *treatment, at)
            }
            ValueMapping::Enum { referent, .. } => self.enum_number(field, referent, symbol),
            // A message value is routed to `plan_child` by the singular/sequence/map planners before
            // here; reaching the scalar path is a keryx error, discharged loud as `walk` does inbound.
            ValueMapping::Message(_) => unreachable!(
                "a message field is routed to plan_child before value; a scalar path here is a keryx error"
            ),
        };
        match lowered {
            Ok(value) => Some(value),
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                None
            }
        }
    }

    /// The enum number a value constant names (spec §7.4): the declared value whose constant matches
    /// the symbol — a positive zero-argument function — or `UnknownEnumValue` for a constant the
    /// enum does not declare, `TermTypeMismatch` for a symbol that is not a constant.
    fn enum_number(
        &self,
        field: &FieldMapping,
        referent: &Name,
        symbol: &Symbol,
    ) -> Result<Value, Diagnostic> {
        let at = field.proto().as_str();
        let Symbol::Function {
            name,
            arguments,
            sign: Sign::Positive,
        } = symbol
        else {
            return Err(term_mismatch(at, "an enum value is not a constant"));
        };
        let enumeration = self
            .index
            .enum_of(referent)
            .expect("every enum referent of the mapping is an enum of its index")
            .in_mapping(self.mapping);
        // §7.4: a PRESERVE open enum admits the escape term `unknown(N)`, raising it to the wire
        // number `N`. The branch is guarded by arity: only a 1-ary `unknown(N)` reaches it, so a
        // declared value named `*_UNKNOWN` — which lowers to the 0-ary constant `unknown` — takes
        // the declared-constant lookup below and raises to its own number, coexisting by arity with
        // the escape term (§7.4; the routing the `unknown`-not-reserved decision rests on).
        if enumeration.preserve() && name.as_str() == names::UNKNOWN_FUNCTOR && arguments.len() == 1
        {
            return preserved_number(field, enumeration, arguments);
        }
        if !arguments.is_empty() {
            return Err(term_mismatch(at, "an enum value is not a constant"));
        }
        enumeration
            .values()
            .iter()
            .find(|value| value.constant().as_str() == name.as_str())
            .map(|value| Value::EnumNumber(value.number()))
            .ok_or_else(|| unknown_enum(at, name.as_str()))
    }

    /// Build the discovered tree bottom-up — children before parents, discovery order reversed — and
    /// encode the root. Runs only when discovery found no diagnosis (property 4). Each occupant's
    /// built message is a `Value::Message` its parent's message field draws by term; the last plan
    /// (the root, first discovered) is encoded.
    fn build(self, pool: &RetainedPool, format: PayloadFormat) -> Result<Vec<u8>, Diagnostics> {
        let mapping = self.mapping;
        let mut built: BTreeMap<Symbol, Value> = BTreeMap::new();
        // Children before parents: discovery recorded each parent before its descendants, so the
        // reversed order builds the deepest occupants first, each already holding its children in
        // `built`. The last built — the root, discovered first — is encoded, not stored. A setter
        // refusal here is defense-in-depth over the inverse §6 validation discovery already ran (a
        // keryx invariant reached), returned rather than encoded.
        let mut plans = self.plans.into_iter().rev().peekable();
        while let Some(plan) = plans.next() {
            let sort = plan.sort.in_mapping(mapping);
            let descriptor = pool
                .message_by_name(sort.proto().as_str())
                .expect("a sort of the mapping is a message of the pool it was walked from");
            let mut building = Building::new(&descriptor);
            for planned in plan.fields {
                set_planned(&mut building, planned, &mut built, sort)?;
            }
            if plans.peek().is_none() {
                return engine::encode(building, format);
            }
            built.insert(plan.occupant, building.into_value());
        }
        unreachable!("at least the root occupant was planned")
    }

    /// Refuse an occupant past the ceiling: `ReassembledTooDeep` at the whole-answer-set locus, once
    /// per reassemble; the occupant and everything below it are left unplanned.
    fn refuse_depth(&mut self, sort: &SortMapping, depth: usize) {
        if self.too_deep {
            return;
        }
        self.too_deep = true;
        self.diagnostics.push(Diagnostic::new(
            DiagnosticKind::ReassembledTooDeep,
            Locus::whole(),
            format!(
                "a `{}` occupant sits {depth} levels of message-typed fields below the root, past keryx's reconstruction ceiling of {}",
                sort.proto().as_str(),
                walk::NESTING_CEILING
            ),
        ));
    }
}

/// Set one planned field on its message. A scalar/enum value or a list/map of them goes straight to
/// the validating setter; a message field draws its child's built message from `built` (built first,
/// the discovery order reversed) — removed, since each child has one parent.
fn set_planned(
    building: &mut Building,
    planned: Planned,
    built: &mut BTreeMap<Symbol, Value>,
    sort: &SortMapping,
) -> Result<(), Diagnostic> {
    let at = |number: i32| field_path(sort, number);
    match planned {
        Planned::Value { number, value } => building.set(number, value, &at(number)),
        Planned::List { number, values } => building.set(number, Value::List(values), &at(number)),
        Planned::Map { number, entries } => building.set(
            number,
            Value::Map(entries.into_iter().collect()),
            &at(number),
        ),
        Planned::Message { number, child } => {
            let value = built
                .remove(&child)
                .expect("every planned child is built before its parent");
            building.set(number, value, &at(number))
        }
        Planned::Messages { number, children } => {
            let values = children
                .into_iter()
                .map(|child| {
                    built
                        .remove(&child)
                        .expect("every planned child is built before its parent")
                })
                .collect();
            building.set(number, Value::List(values), &at(number))
        }
        Planned::MessageMap { number, entries } => {
            let map = entries
                .into_iter()
                .map(|(key, child)| {
                    (
                        key,
                        built
                            .remove(&child)
                            .expect("every planned child is built before its parent"),
                    )
                })
                .collect();
            building.set(number, Value::Map(map), &at(number))
        }
    }
}

/// The root occupant a marker atom `emit_<sort>(root)` names — its single argument.
pub(crate) fn marker_root(marker: &Symbol) -> Option<&Symbol> {
    match marker {
        Symbol::Function { arguments, .. } => arguments.first(),
        _ => None,
    }
}

/// The place argument of an entry — a sequence index or a map key, the argument after the parent —
/// or `None` for a singular field's atom (which has only the parent and, for a scalar, the value).
fn place(entry: &Symbol) -> Option<&Symbol> {
    match entry {
        Symbol::Function { arguments, .. } => arguments.get(1),
        _ => None,
    }
}

/// The last argument of a field atom — its value.
fn last_argument(entry: &Symbol) -> Option<&Symbol> {
    match entry {
        Symbol::Function { arguments, .. } => arguments.last(),
        _ => None,
    }
}

/// The exact arity a field's atoms carry under its form and value kind: a scalar or enum field atom
/// `f(P, V)`/`f(P, I, V)`/`f(P, K, V)` (2 for a singular field or oneof arm, 3 for a sequence or map),
/// or a message occupant term `f(P)`/`f(P, I)`/`f(P, K)` (one fewer — a message carries its value in
/// the child, not a last argument). An atom of the field's name but another arity is a *different*
/// predicate (ASP identifies a predicate by name and arity), filtered out before the field is planned
/// ([`Discover::plan_field`]).
fn expected_arity(field: &FieldMapping) -> usize {
    let base = match field.form() {
        EmitForm::Function | EmitForm::OneofArm { .. } => 2,
        EmitForm::Sequence | EmitForm::Map { .. } => 3,
        EmitForm::Set => unreachable!(
            "the mapping produces no `Set` form before Increment 5; planning one is a keryx error"
        ),
    };
    if matches!(field.value(), ValueMapping::Message(_)) {
        base - 1
    } else {
        base
    }
}

/// Whether `symbol` is a function of exactly `arity` arguments — the shape a field's atom must take
/// under its form ([`expected_arity`]); a term of another arity is a different predicate, ignored.
fn is_function_of_arity(symbol: &Symbol, arity: usize) -> bool {
    matches!(symbol, Symbol::Function { arguments, .. } if arguments.len() == arity)
}

/// The fully-qualified proto path of a field by number, for a diagnostic locus.
fn field_path(sort: &SortMapping, number: i32) -> String {
    sort.fields()
        .iter()
        .find(|field| field.number() == number)
        .map_or_else(
            || sort.proto().as_str().to_owned(),
            |field| field.proto().as_str().to_owned(),
        )
}

/// `ShapeViolation` at a field's path.
fn shape(field: &FieldMapping, detail: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::ShapeViolation,
        Locus::at(field.proto().as_str()),
        detail.to_owned(),
    )
}

/// `ShapeViolation`: a singular field with two values.
fn duplicate_singular(field: &FieldMapping) -> Diagnostic {
    shape(field, "a singular field has two values")
}

/// `ShapeViolation`: a total (implicit-presence) field with no value.
fn missing_total(field: &FieldMapping) -> Diagnostic {
    shape(field, "a total or required field is missing its value")
}

/// The wire number a PRESERVE enum's escape term `unknown(N)` raises to (spec §7.4, §12.3): its
/// single integer argument `N`, refused as a `ShapeViolation` when `N` names a declared value —
/// the model must spell that value with its constant, keeping term↔wire injective — or when the
/// term is not the shape `unknown(<integer>)`.
fn preserved_number(
    field: &FieldMapping,
    enumeration: &EnumMapping,
    arguments: &[Symbol],
) -> Result<Value, Diagnostic> {
    let [Symbol::Number(number)] = arguments else {
        return Err(shape(
            field,
            "an unknown enum value carries one integer, `unknown(N)`",
        ));
    };
    if enumeration
        .values()
        .iter()
        .any(|value| value.number() == *number)
    {
        return Err(shape(
            field,
            "an unknown enum value names a declared number; the model must use its constant",
        ));
    }
    Ok(Value::EnumNumber(*number))
}

/// Whether `occupant`'s parent spine — each term's first argument, followed inward — reaches a
/// marker root, so the occupant is positioned within a tree some `emit_<sort>` marker exports (spec
/// §12.1). A field atom on an occupant that descends from no marker is the model's private business,
/// not an orphan: reachability, not mere presence, is what the orphan refusal is scoped to.
fn descends_from_marker(occupant: &Symbol, marker_roots: &BTreeSet<&Symbol>) -> bool {
    let mut term = occupant;
    loop {
        if marker_roots.contains(term) {
            return true;
        }
        match term {
            Symbol::Function { arguments, .. } => match arguments.first() {
                Some(parent) => term = parent,
                None => return false, // a constant that is not itself a marker root
            },
            _ => return false, // a number/string/tuple/inf/sup: no parent spine, no marker
        }
    }
}

/// `ShapeViolation`: a field atom whose parent occupant descends from a marker root but is declared
/// by no occupancy atom — a child positioned within an exported tree with its sort undeclared
/// (property 4's orphan, scoped to reachability; §12.1). An atom descending from no marker is the
/// model's private business and never reaches here.
fn orphan(atom: &Symbol) -> Diagnostic {
    let path = match atom {
        Symbol::Function { name, .. } => name.as_str().to_owned(),
        _ => String::new(),
    };
    Diagnostic::new(
        DiagnosticKind::ShapeViolation,
        Locus::at(path),
        "a field atom's parent occupant descends from a marker root but is declared by no occupancy atom"
            .to_owned(),
    )
}

/// `TermTypeMismatch` at a field's path.
fn term_mismatch(at: &str, detail: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::TermTypeMismatch,
        Locus::at(at),
        detail.to_owned(),
    )
}

/// `UnknownEnumValue` at a field's path.
fn unknown_enum(at: &str, constant: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::UnknownEnumValue,
        Locus::at(at),
        format!("the enum declares no value with the constant `{constant}`"),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use themelios_program::prelude::*;

    use super::{SlotIndex, assemble};
    use crate::codec::walk::Index;
    use crate::descriptor::{self, RetainedPool};
    use crate::diagnostics::DiagnosticKind;
    use crate::policy::{self, Mapping};

    /// The thermal example's mapping and the pool it was walked from (spec §28).
    fn thermal() -> (Mapping, RetainedPool) {
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
        let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
        let (schema, pool) = descriptor::source::compile_retaining(
            &[example.join("thermal.proto")],
            &[example, vendored],
        )
        .expect("the thermal example compiles");
        let mapping = policy::map(&schema).expect("maps");
        (mapping, pool)
    }

    /// A constant symbol `name` — a positive zero-argument function.
    fn constant(name: &str) -> Symbol {
        atom(name, Vec::new())
    }

    /// A positive atom `pred(args…)`.
    fn atom(pred: &str, arguments: Vec<Symbol>) -> Symbol {
        Symbol::Function {
            name: Name::new(pred).expect("an identifier"),
            arguments,
            sign: Sign::Positive,
        }
    }

    /// Assemble the one root of `answer` (its single marker), a `Reading`/`ReadingBatch` test.
    fn reassemble_one(answer: &[Symbol]) -> Result<Vec<u8>, Vec<DiagnosticKind>> {
        let (mapping, pool) = thermal();
        let index = Index::build(&mapping).expect("indexes");
        let slots = SlotIndex::build(&mapping, &index, answer);
        let (sort, marker) = slots.markers()[0];
        let root = super::marker_root(marker).expect("a marker names a root");
        assemble(
            &mapping,
            &index,
            &pool,
            &slots,
            root,
            sort,
            crate::codec::PayloadFormat::Binary,
        )
        .map_err(|d| d.iter().map(crate::diagnostics::Diagnostic::kind).collect())
    }

    #[test]
    fn a_reading_answer_set_reassembles_to_the_canonical_bytes() {
        // The occupancy atom `reading(r0)`, the two total scalar fields, and the marker — the facts
        // a shred of `wire::reading("s-1", 21)` produces, exported under a root marker — reassemble
        // to exactly those bytes (the round-trip's forward half on a flat message).
        let r0 = constant("r0");
        let answer = vec![
            atom("emit_reading", vec![r0.clone()]),
            atom("reading", vec![r0.clone()]),
            atom("sensor", vec![r0.clone(), Symbol::String("s-1".to_owned())]),
            atom("temp_c", vec![r0.clone(), Symbol::Number(21)]),
        ];
        assert_eq!(
            reassemble_one(&answer).expect("reassembles"),
            keryx_test_support::wire::reading("s-1", 21)
        );
    }

    #[test]
    fn a_reading_batch_reassembles_its_message_sequence_from_occupancy_atoms() {
        // A `ReadingBatch` holding two `Reading`s: each element is named by an occupancy atom
        // `reading(readings(b0, I))`, the slot index keying it under `(readings, b0)`; the walk
        // recurses into each and the build nests them (children before parent). The bytes are the
        // canonical batch — the message-sequence and recursion path.
        let b0 = constant("b0");
        let element = |index: i32| atom("readings", vec![b0.clone(), Symbol::Number(index)]);
        let answer = vec![
            atom("emit_reading_batch", vec![b0.clone()]),
            atom("reading_batch", vec![b0.clone()]),
            atom("reading", vec![element(0)]),
            atom("sensor", vec![element(0), Symbol::String("s-1".to_owned())]),
            atom("temp_c", vec![element(0), Symbol::Number(1)]),
            atom("reading", vec![element(1)]),
            atom("sensor", vec![element(1), Symbol::String("s-2".to_owned())]),
            atom("temp_c", vec![element(1), Symbol::Number(2)]),
        ];
        assert_eq!(
            reassemble_one(&answer).expect("reassembles"),
            keryx_test_support::wire::batch(&[
                keryx_test_support::wire::reading("s-1", 1),
                keryx_test_support::wire::reading("s-2", 2),
            ])
        );
    }

    #[test]
    fn a_duplicate_singular_is_a_shape_violation() {
        // Two `sensor` atoms on one `Reading` — the answer set is not one the theory admits; refused,
        // never one value silently chosen.
        let r0 = constant("r0");
        let answer = vec![
            atom("emit_reading", vec![r0.clone()]),
            atom("reading", vec![r0.clone()]),
            atom("sensor", vec![r0.clone(), Symbol::String("a".to_owned())]),
            atom("sensor", vec![r0.clone(), Symbol::String("b".to_owned())]),
            atom("temp_c", vec![r0.clone(), Symbol::Number(1)]),
        ];
        assert_eq!(
            reassemble_one(&answer).expect_err("refused"),
            vec![DiagnosticKind::ShapeViolation]
        );
    }

    #[test]
    fn a_reachable_child_missing_its_occupancy_is_an_orphan_but_a_private_atom_is_not() {
        // The orphan refusal is scoped to reachability (property 4, spec §12.1). A field atom whose
        // parent descends from no marker is the model's private business — ignored, never refused.
        let (mapping, _pool) = thermal();
        let index = Index::build(&mapping).expect("indexes");
        let private = vec![atom(
            "sensor",
            vec![constant("ghost"), Symbol::String("x".to_owned())],
        )];
        assert!(
            SlotIndex::build(&mapping, &index, &private)
                .orphans()
                .is_empty(),
            "an atom over an occupant no marker reaches is private business, not an orphan"
        );

        // But a field atom on a child positioned within an exported tree — `readings(b0, 0)`, whose
        // spine reaches the marker root `b0` — that carries no `reading(readings(b0,0))` occupancy
        // atom is a refused orphan: real dropped data, its sort undeclared.
        let b0 = constant("b0");
        let element = atom("readings", vec![b0.clone(), Symbol::Number(0)]);
        let reachable = vec![
            atom("emit_reading_batch", vec![b0.clone()]),
            atom("sensor", vec![element, Symbol::String("x".to_owned())]),
        ];
        let slots = SlotIndex::build(&mapping, &index, &reachable);
        assert_eq!(slots.orphans().len(), 1);
        assert_eq!(slots.orphans()[0].kind(), DiagnosticKind::ShapeViolation);
    }

    #[test]
    fn a_violates_atom_is_collected_by_the_slot_index() {
        // A diagnostic theory's `violates(path, occupant)` atom is collected apart, for the
        // reassemble to refuse mode-free (property 4).
        let (mapping, _pool) = thermal();
        let index = Index::build(&mapping).expect("indexes");
        let answer = vec![atom(
            "violates",
            vec![
                Symbol::String("thermal.v1.Reading.sensor".to_owned()),
                constant("r0"),
            ],
        )];
        let slots = SlotIndex::build(&mapping, &index, &answer);
        assert_eq!(slots.violations().len(), 1);
    }

    #[test]
    fn a_missing_total_field_is_a_shape_violation() {
        // `temp_c` (implicit-presence, total) omitted — a serializable answer set carries it (§12.2);
        // its absence is refused, never defaulted silently.
        let r0 = constant("r0");
        let answer = vec![
            atom("emit_reading", vec![r0.clone()]),
            atom("reading", vec![r0.clone()]),
            atom("sensor", vec![r0.clone(), Symbol::String("s".to_owned())]),
        ];
        assert_eq!(
            reassemble_one(&answer).expect_err("refused"),
            vec![DiagnosticKind::ShapeViolation]
        );
    }
}
