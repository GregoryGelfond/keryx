//! `emit.lp` (spec §13.3): the serializability theory. Outbound, the vocabulary's invariants
//! are obligations rather than theorems (Part III). This module renders the frame they stand
//! on — the response-root markers and the reachability closure (§12.1) — and the obligations
//! themselves (§12.2), each guarded by `reach/1` and relative to its sort: the vocabulary is
//! sort-polymorphic (§4.2 — one predicate serves every sort that declares the field), so an
//! obligation carries its sort atom, or it would fire on another sort's occupants and refuse
//! an answer set that is serializable. Every message sort gets a marker `emit_<sort>/1`: the
//! model asserts `emit_<sort>(term)` to export the tree under `term`, an unasserted marker is
//! inert, and `reach/1` closes over the message-typed slots from every asserted root, so an
//! obligation binds only what is exported and the model's working predicates stay
//! unconstrained. A client of `core.lp` alone: the closure and every obligation over a
//! message-typed slot join on occupancy — the child sort atom — never on the `views.lp`
//! relations (a message slot has no field atom of its own, §4.1), so a project that excludes
//! `views.lp` loses nothing here. Rendered in canonical order (P3). The two modes (§12.2)
//! differ only in how an obligation is written — an integrity constraint, or a rule deriving
//! `violates(path, occupant)` — so one assembly serves both.

use themelios_program::prelude::*;

use crate::descriptor::model::{MapKey, Scalar};
use crate::diagnostics::Diagnostics;
use crate::emit::{build, render_client_of_core, signature};
use crate::policy::model::{
    EmitForm, EnumMapping, FieldMapping, ScalarTreatment, SortMapping, Totality, Unit,
    ValueMapping, ViewKind,
};
use crate::policy::names;

/// Render one generation unit's `emit.lp` in strict mode (spec §12.2, §13.3), where an
/// obligation is an integrity constraint and an unserializable answer set is UNSAT. Total
/// (§6).
///
/// # Errors
///
/// [`Diagnostics`] as [`crate::emit::core`].
pub fn emit_strict(unit: &Unit) -> Result<String, Diagnostics> {
    render_client_of_core(unit, theory(unit, Mode::Strict))
}

/// Render one generation unit's `emit.lp` in diagnostic mode (spec §12.2, §13.3), where an
/// obligation derives `violates(field-path, occupant)` and the model survives for the
/// reassembler to report. Total (§6).
///
/// # Errors
///
/// [`Diagnostics`] as [`crate::emit::core`].
pub fn emit_diagnostic(unit: &Unit) -> Result<String, Diagnostics> {
    render_client_of_core(unit, theory(unit, Mode::Diagnostic))
}

/// How an obligation is written (spec §12.2).
#[derive(Clone, Copy)]
enum Mode {
    /// An integrity constraint `:- body.` — an unserializable answer set is UNSAT.
    Strict,
    /// A rule `violates(path, occupant) :- body.` — the model survives, and the reassembler
    /// reports the field path, never an atom (P1).
    Diagnostic,
}

/// The obligation kinds (spec §12.2), each phrased once for the `%!` doc its statements carry
/// — `<kind> of <signature line>` — so a reader of `emit.lp` sees which obligation a statement
/// is and over which field.
#[derive(Clone, Copy)]
enum Kind {
    Functionality,
    Totality,
    Contiguity,
    KeyFunctionality,
    Exclusivity,
    Membership,
    Range,
    KeyRange,
    Occupancy,
}

impl Kind {
    fn phrase(self) -> &'static str {
        match self {
            Kind::Functionality => "functionality",
            Kind::Totality => "totality",
            Kind::Contiguity => "contiguity",
            Kind::KeyFunctionality => "key functionality",
            Kind::Exclusivity => "exclusivity",
            Kind::Membership => "membership",
            Kind::Range => "range",
            Kind::KeyRange => "key range",
            Kind::Occupancy => "occupancy",
        }
    }
}

/// The `%!` doc an obligation's statements carry: its kind over the signature line it holds.
fn doc(kind: Kind, line: &str) -> String {
    format!("{} of {line}", kind.phrase())
}

/// One obligation before its mode is applied: the fully-qualified proto path the diagnostic
/// head names (§12.2's field-path descriptor), the occupant it is about (the head's second
/// argument), its body, and its doc.
struct Obligation {
    path: String,
    subject: Term,
    body: Vec<BodyElement>,
    doc: String,
}

impl Obligation {
    /// The obligation as `mode` writes it (spec §12.2).
    fn statement(self, mode: Mode) -> WithProvenance<Statement> {
        match mode {
            Mode::Strict => build::constraint(self.body, self.doc),
            Mode::Diagnostic => build::rule(
                build::atom(names::violates(), [build::text(&self.path), self.subject]),
                self.body,
                self.doc,
            ),
        }
    }
}

/// The whole theory in one mode: the frame, then per sort its obligations and the witnesses
/// they read, then per enum its membership table. Assembled in schema order; `render` puts it
/// in canonical order.
fn theory(unit: &Unit, mode: Mode) -> Vec<WithProvenance<Statement>> {
    let mut statements = frame(unit);
    let mut obligations = Vec::new();
    for sort in unit.sorts() {
        sort_obligations(unit, sort, &mut statements, &mut obligations);
    }
    for enumeration in unit.enums() {
        statements.extend(membership_table(enumeration));
    }
    statements.extend(
        obligations
            .into_iter()
            .map(|obligation| obligation.statement(mode)),
    );
    statements
}

/// The frame both modes share (spec §12.1): per message sort, its root marker's `#defined` and
/// the root step `reach(X) :- emit_<sort>(X).`; per message-typed field, the closure step
/// reaching its occupant. Assembled in schema order; `render` puts it in canonical order.
fn frame(unit: &Unit) -> Vec<WithProvenance<Statement>> {
    let mut statements = Vec::new();
    for sort in unit.sorts() {
        let marker = names::marker(sort.predicate());
        // The marker and its root step carry the marker's signature line alone: the sort's
        // proto prose is on its `#defined` in `core.lp`, which this file includes — as a
        // `views.lp` rule carries its field's line (§13.2).
        let line = signature::root(&marker, sort);
        statements.push(build::defined(marker.clone(), 1, line.clone()));
        statements.push(build::rule(
            build::atom(names::reach(), [build::var("X")]),
            build::atom(marker, [build::var("X")]),
            line,
        ));
        for field in sort.fields() {
            // Exactly a message-typed field has a slot the closure crosses: `FieldMapping::view`
            // yields the view kind together with the referent sort predicate iff the value is a
            // message (and the form is not `Set`), so the child sort is in hand here.
            if let Some((kind, referent)) = field.view() {
                statements.push(step(sort, field, kind, referent.clone()));
            }
        }
    }
    statements
}

/// The closure step for one message-typed field (spec §12.1):
/// `reach(A) :- reach(X), <sort>(X), <child>(A), A = f(X[, I | K]).` — the parent `X` is
/// reached and of this sort (the step is the sort's own: the vocabulary is sort-polymorphic,
/// §4.2, so a shared field predicate's other sort stays out), the child sort atom binds the
/// occupant `A`, and the equality deconstructs it against the field's access-path term,
/// binding the index or key by unification. Binding `A` by a positive atom before the
/// equality is what makes the rule safe — an index or key the equality alone bound would be
/// unsafe, and the grounder would reject the rule — the same idiom as the `views.lp` rule
/// (§13.2), and the join on occupancy that keeps `emit.lp` a client of `core.lp` alone.
fn step(
    parent: &SortMapping,
    field: &FieldMapping,
    kind: ViewKind,
    referent: Name,
) -> WithProvenance<Statement> {
    let reach = names::reach();
    let x = build::var("X");
    let a = build::var("A");
    build::rule(
        build::atom(reach.clone(), [a.clone()]),
        vec![
            build::positive(build::atom(reach, [x.clone()])),
            build::positive(build::atom(parent.predicate().clone(), [x.clone()])),
            build::positive(build::atom(referent, [a.clone()])),
            build::compare(a, Relation::Eq, slot(field, kind, x)),
        ],
        // The rule's `%!` doc is the field's one-line signature; its proto prose lives on the
        // parent sort's `#defined` in `core.lp` (§13.1), which this file includes.
        signature::field(parent, field),
    )
}

/// A message-typed field's slot on parent `x` — its access-path term (§4.1): `f(X)`,
/// `f(X, I)`, or `f(X, K)` per the view kind, the index or key a variable the joining atom
/// binds.
fn slot(field: &FieldMapping, kind: ViewKind, x: Term) -> Term {
    let args = match kind {
        ViewKind::Singular => vec![x],
        ViewKind::Sequence => vec![x, build::var("I")],
        ViewKind::Map => vec![x, build::var("K")],
    };
    build::apply(field.predicate().clone(), args)
}

/// One sort's obligations (spec §12.2): per field, the root instance of occupancy and the
/// field's own obligations by form; per message-typed field, occupancy over its slot; then the
/// sort's oneofs' exclusivity. The witnesses an obligation reads are mode-free rules and go to
/// `auxiliaries`; the obligations wait for their mode in `obligations`.
fn sort_obligations(
    unit: &Unit,
    sort: &SortMapping,
    auxiliaries: &mut Vec<WithProvenance<Statement>>,
    obligations: &mut Vec<Obligation>,
) {
    for field in sort.fields() {
        obligations.extend(root_occupancy(sort, field));
        match field.form() {
            EmitForm::Function | EmitForm::OneofArm { .. } => {
                singular(sort, field, auxiliaries, obligations);
            }
            EmitForm::Sequence => sequence(sort, field, auxiliaries, obligations),
            EmitForm::Map { key, key_treatment } => {
                map(sort, field, *key, *key_treatment, obligations);
            }
            // A set (§7.1) drops the index, so it has no contiguity; its membership and range
            // instances over `f(P, V)`, and the occupant order that makes its serialization
            // canonical, arrive with the form's semantics — `(keryx.set)`, Increment 5 — and
            // the mapping does not yet produce the form.
            EmitForm::Set => {}
        }
        if let Some((kind, child)) = field.view() {
            obligations.extend(slot_occupancy(unit, sort, field, kind, child));
        }
    }
    obligations.extend(exclusivity(sort));
}

/// The guard every obligation opens with (spec §12.2): `reach(P), <sort>(P)` — bound to what
/// is exported, and to this sort's occupants alone, so a predicate the sort shares with
/// another (§4.2) is held only where this sort declares it.
fn guard(sort: &SortMapping, subject: &Term) -> Vec<BodyElement> {
    vec![
        build::positive(build::atom(names::reach(), [subject.clone()])),
        build::positive(build::atom(sort.predicate().clone(), [subject.clone()])),
    ]
}

/// A field's fully-qualified proto path — the diagnostic head's descriptor (P1).
fn path(field: &FieldMapping) -> String {
    field.proto().as_str().to_owned()
}

/// A singular field's obligations (`Function` or `OneofArm`) over a base-fact field — scalar
/// or enum: functionality, totality when the field is total, and the value's membership or
/// range. Totality is emitted for `Total` and `Required` alike — IMPLICIT presence and proto2
/// `required` (`Totality::Required`, E1): both mint the presence witness and carry the totality
/// constraint, so a proto2 `required` field's completeness is enforced outbound (full
/// proto2/proto3 parity), while an EXPLICIT (`Partial`) field gets functionality only. A
/// message-typed slot's functionality is structural (its occupant `f(P)` is one term) and its
/// presence is its occupancy, held from the parent over the slot ([`slot_occupancy`]) — except a
/// `Required` (proto2 `required`) message slot, which additionally carries a totality obligation
/// over its occupant, `not <child>(f(P))` (below; E1).
fn singular(
    sort: &SortMapping,
    field: &FieldMapping,
    auxiliaries: &mut Vec<WithProvenance<Statement>>,
    obligations: &mut Vec<Obligation>,
) {
    let line = signature::field(sort, field);
    if let Some((kind, child)) = field.view() {
        // A message-typed slot: functionality is structural (one occupant term), and its presence
        // is held from the parent (`slot_occupancy`). A `Required` (proto2 `required`) message field
        // adds a totality obligation over occupancy — the occupant `f(P)` must be a `<child>` — the
        // message counterpart of the scalar witness-and-constraint below, so `emit.lp` enforces a
        // `required` field on both the scalar and the message side (E1, full proto2/proto3 parity).
        if field.presence() == Totality::Required {
            let p = build::var("P");
            let mut body = guard(sort, &p);
            body.push(build::not_atom(build::atom(
                child.clone(),
                [slot(field, kind, p.clone())],
            )));
            obligations.push(Obligation {
                path: path(field),
                subject: p,
                body,
                doc: doc(Kind::Totality, &line),
            });
        }
        return;
    }
    let p = build::var("P");
    obligations.push(functional(sort, field, Kind::Functionality, &[], &line));
    // A `Total` (IMPLICIT) or a `Required` (proto2 `required`) field is totality-obliged (E1):
    // both mint the presence witness and carry the totality constraint; an EXPLICIT one does not.
    if matches!(field.presence(), Totality::Total | Totality::Required) {
        // The witness `has_f(P) :- f(P, _).` is what the obligation negates: a default-negated
        // occurrence with a projected value would be unsafe as written, so the projection is
        // a positive rule of its own.
        let witness = names::witness(field.predicate());
        auxiliaries.push(build::rule(
            build::atom(witness.clone(), [p.clone()]),
            occurrence(field, p.clone(), None),
            doc(Kind::Totality, &line),
        ));
        let mut body = guard(sort, &p);
        body.push(build::not_atom(build::atom(witness, [p.clone()])));
        obligations.push(Obligation {
            path: path(field),
            subject: p,
            body,
            doc: doc(Kind::Totality, &line),
        });
    }
    obligations.extend(value_obligation(sort, field, &[], &line));
}

/// `:- reach(P), <sort>(P), f(P, …, V1), f(P, …, V2), V1 != V2.` — two values at one place
/// violate the field: functionality of a singular field (no place), key functionality of a
/// map (the place `K`).
fn functional(
    sort: &SortMapping,
    field: &FieldMapping,
    kind: Kind,
    places: &[Term],
    line: &str,
) -> Obligation {
    let p = build::var("P");
    let (first, second) = (build::var("V1"), build::var("V2"));
    let at = |value: Term| {
        let mut args = vec![p.clone()];
        args.extend(places.iter().cloned());
        args.push(value);
        build::positive(build::atom(field.predicate().clone(), args))
    };
    let mut body = guard(sort, &p);
    body.push(at(first.clone()));
    body.push(at(second.clone()));
    body.push(build::compare(first, Relation::Neq, second));
    Obligation {
        path: path(field),
        subject: p,
        body,
        doc: doc(kind, line),
    }
}

/// A sequence's obligations: contiguity from 0 (§7.1) — the index witness `has_f(P, I)` over
/// the field's occurrence at `I`, the gap rule (an index above 0 whose predecessor is absent),
/// and the non-negative rule — then, over a base-fact sequence, each element's membership or
/// range. A sequence of messages keeps its indices in its occupants' slots, so its witness and
/// its non-negative rule read the occupancy atom `u(f(P, I))`.
fn sequence(
    sort: &SortMapping,
    field: &FieldMapping,
    auxiliaries: &mut Vec<WithProvenance<Statement>>,
    obligations: &mut Vec<Obligation>,
) {
    let line = signature::field(sort, field);
    let p = build::var("P");
    let i = build::var("I");
    let witness = names::witness(field.predicate());
    let at_i = occurrence(field, p.clone(), Some(i.clone()));
    auxiliaries.push(build::rule(
        build::atom(witness.clone(), [p.clone(), i.clone()]),
        at_i.clone(),
        doc(Kind::Contiguity, &line),
    ));
    let mut gap = guard(sort, &p);
    gap.push(build::positive(build::atom(
        witness.clone(),
        [p.clone(), i.clone()],
    )));
    gap.push(build::compare(i.clone(), Relation::Gt, build::int(0)));
    gap.push(build::not_atom(build::atom(
        witness,
        [p.clone(), build::minus(i.clone(), 1)],
    )));
    obligations.push(Obligation {
        path: path(field),
        subject: p.clone(),
        body: gap,
        doc: doc(Kind::Contiguity, &line),
    });
    let mut negative = guard(sort, &p);
    negative.push(build::positive(at_i));
    negative.push(build::compare(i.clone(), Relation::Lt, build::int(0)));
    obligations.push(Obligation {
        path: path(field),
        subject: p,
        body: negative,
        doc: doc(Kind::Contiguity, &line),
    });
    obligations.extend(value_obligation(sort, field, &[i], &line));
}

/// A map's obligations (§7.2): over a base-fact map, key functionality and each value's
/// membership or range; over any map keyed by an unsigned native integer (`uint32`/`fixed32`,
/// §6), the key's range — read from the field's occurrence at `K`, so a message-valued map's
/// key is held through its occupancy atom. A message-valued map's key functionality is
/// structural: its occupant `f(P, K)` is one term per key.
fn map(
    sort: &SortMapping,
    field: &FieldMapping,
    key: MapKey,
    key_treatment: ScalarTreatment,
    obligations: &mut Vec<Obligation>,
) {
    let line = signature::field(sort, field);
    let p = build::var("P");
    let k = build::var("K");
    if field.view().is_none() {
        obligations.push(functional(
            sort,
            field,
            Kind::KeyFunctionality,
            std::slice::from_ref(&k),
            &line,
        ));
        obligations.extend(value_obligation(
            sort,
            field,
            std::slice::from_ref(&k),
            &line,
        ));
    }
    if unsigned(Scalar::from(key), key_treatment) {
        let mut body = guard(sort, &p);
        body.push(build::positive(occurrence(
            field,
            p.clone(),
            Some(k.clone()),
        )));
        body.push(build::compare(k, Relation::Lt, build::int(0)));
        obligations.push(Obligation {
            path: path(field),
            subject: p,
            body,
            doc: doc(Kind::KeyRange, &line),
        });
    }
}

/// A value's own obligation at the field's place (`[]`, `[I]`, or `[K]`; the value `V` follows):
/// an enum value is held to its enum's membership table, `not ok_e(V)`; an unsigned native
/// integer (`uint32`/`fixed32`, §6) to `V < 0` — the upper bound is the engine's own width (an
/// emitted integer is an `i32`), and the reassembler's inverse re-checks the unsigned range.
/// No other value carries an obligation the theory states: a decimal string's range and a hex
/// string's shape are term-type conditions, the reassembler's (§12.3), and a message value's
/// presence is its occupancy.
fn value_obligation(
    sort: &SortMapping,
    field: &FieldMapping,
    places: &[Term],
    line: &str,
) -> Option<Obligation> {
    let p = build::var("P");
    let v = build::var("V");
    let (kind, test) = match field.value() {
        ValueMapping::Enum(enumeration) => (
            Kind::Membership,
            build::not_atom(build::atom(names::member(enumeration), [v.clone()])),
        ),
        ValueMapping::Scalar { kind, treatment } if unsigned(*kind, *treatment) => (
            Kind::Range,
            build::compare(v.clone(), Relation::Lt, build::int(0)),
        ),
        ValueMapping::Scalar { .. } | ValueMapping::Message(_) => return None,
    };
    let mut args = vec![p.clone()];
    args.extend(places.iter().cloned());
    args.push(v);
    let mut body = guard(sort, &p);
    body.push(build::positive(build::atom(
        field.predicate().clone(),
        args,
    )));
    body.push(test);
    Some(Obligation {
        path: path(field),
        subject: p,
        body,
        doc: doc(kind, line),
    })
}

/// Whether a scalar carries the unsigned range obligation (§6): a `uint32`/`fixed32` under the
/// native treatment — a native integer that must not be negative. Under another treatment
/// (an annotation's decimal string, Increment 5) the value is not an integer term at all.
fn unsigned(kind: Scalar, treatment: ScalarTreatment) -> bool {
    treatment == ScalarTreatment::Native && matches!(kind, Scalar::Uint32 | Scalar::Fixed32)
}

/// Oneof exclusivity (§7.3): for each pair of arms of one oneof — grouped by the oneof's proto
/// name on this sort, the one identity the mapping carries for it — `:- reach(P), <sort>(P),
/// armᵢ present, armⱼ present.`, an arm's presence its field atom `arm(P, _)` or, for a
/// message-typed arm, its slot's occupancy atom `u(arm(P))`. The diagnostic head names the
/// oneof's own path, `<sort path>.<oneof>` — protobuf's full name for it. The oneof name enters
/// that head string (and the `%!` doc via [`signature::arms`]) verbatim; it is a validated proto
/// identifier — the descriptor door refuses any other (`descriptor::pre_validate`) — so it carries
/// no quote, newline, or control character that could break the quoted string or the doc line.
fn exclusivity(sort: &SortMapping) -> Vec<Obligation> {
    let p = build::var("P");
    let arms: Vec<(&str, &FieldMapping)> = sort
        .fields()
        .iter()
        .filter_map(|field| match field.form() {
            EmitForm::OneofArm { oneof } => Some((oneof.as_str(), field)),
            EmitForm::Function | EmitForm::Sequence | EmitForm::Set | EmitForm::Map { .. } => None,
        })
        .collect();
    let mut obligations = Vec::new();
    for (index, (oneof, first)) in arms.iter().enumerate() {
        for (other, second) in &arms[index + 1..] {
            if oneof != other {
                continue;
            }
            let mut body = guard(sort, &p);
            body.push(build::positive(occurrence(first, p.clone(), None)));
            body.push(build::positive(occurrence(second, p.clone(), None)));
            obligations.push(Obligation {
                path: format!("{}.{oneof}", sort.proto().as_str()),
                subject: p.clone(),
                body,
                doc: doc(
                    Kind::Exclusivity,
                    &signature::arms(sort, first, second, oneof),
                ),
            });
        }
    }
    obligations
}

/// The root instance of occupancy consistency (§12.2): a marked root carrying a field atom of
/// its sort must carry the sort atom — `:- emit_u(P), <g present on P>, not u(P).` per field
/// `g` of the sort. The diagnostic head names the sort's path: the violation is the root's.
fn root_occupancy(sort: &SortMapping, field: &FieldMapping) -> Option<Obligation> {
    let p = build::var("P");
    let present = witness(field, p.clone())?;
    let marker = names::marker(sort.predicate());
    let body = vec![
        build::positive(build::atom(marker.clone(), [p.clone()])),
        build::positive(present),
        build::not_atom(build::atom(sort.predicate().clone(), [p.clone()])),
    ];
    Some(Obligation {
        path: sort.proto().as_str().to_owned(),
        subject: p,
        body,
        doc: doc(Kind::Occupancy, &signature::root(&marker, sort)),
    })
}

/// Occupancy consistency from the parent over a slot (§12.2, in the field-atom ⇒ sort-atom
/// direction — an empty message keeps its sort atom through its parent's occupancy, which
/// the spec's "iff" would refuse): for parent sort `t`, message field `f` with child sort `u`,
/// and each field `g` of `u` — `:- reach(X), t(X), <g present on f(X[, I | K])>,
/// not u(f(X[, I | K])).` Safe: `X` is bound by `reach`, the index or key by the positive
/// occurrence of `g`. Emitted from the parent because the child sort is the schema's
/// knowledge, not the shared predicate's — and only for a child declared in this unit: a
/// referent in another package keeps its fields in its own unit, out of this file's sight, so
/// that slot's consistency is the reassembler's occupancy check alone. The diagnostic head
/// names the slot's field path and the occupant term.
fn slot_occupancy(
    unit: &Unit,
    parent: &SortMapping,
    field: &FieldMapping,
    kind: ViewKind,
    child: &Name,
) -> Vec<Obligation> {
    let Some(sort) = unit.sorts().iter().find(|sort| sort.predicate() == child) else {
        return Vec::new();
    };
    let x = build::var("X");
    let occupant = slot(field, kind, x.clone());
    let line = signature::field(parent, field);
    sort.fields()
        .iter()
        .filter_map(|g| {
            let present = witness(g, occupant.clone())?;
            let mut body = guard(parent, &x);
            body.push(build::positive(present));
            body.push(build::not_atom(build::atom(
                child.clone(),
                [occupant.clone()],
            )));
            Some(Obligation {
                path: path(field),
                subject: occupant.clone(),
                body,
                doc: doc(Kind::Occupancy, &line),
            })
        })
        .collect()
}

/// The atom witnessing that `field` occurs at `place` on `subject`: its own atom with the
/// value anonymous — `f(S, _)`, `f(S, I, _)` — or, for a message-typed field, the occupancy
/// atom of the slot's occupant — `u(f(S))`, `u(f(S, I))`: a message slot has no field atom of
/// its own (§4.1); its presence *is* its occupant's sort atom, which is what keeps every
/// obligation a client of `core.lp` alone. `place` is a sequence's index or a map's key — a
/// named variable where the obligation reads it, `_` where it does not — and none for a
/// singular field.
fn occurrence(field: &FieldMapping, subject: Term, place: Option<Term>) -> Atom {
    let mut args = vec![subject];
    args.extend(place);
    match field.value() {
        ValueMapping::Message(child) => build::atom(
            child.clone(),
            [build::apply(field.predicate().clone(), args)],
        ),
        ValueMapping::Scalar { .. } | ValueMapping::Enum(_) => {
            args.push(build::anonymous());
            build::atom(field.predicate().clone(), args)
        }
    }
}

/// The atom witnessing that `field` is present on `subject` at all — [`occurrence`] with the
/// place anonymous — or `None` for a set, whose obligations are Increment 5's (see
/// [`sort_obligations`]).
fn witness(field: &FieldMapping, subject: Term) -> Option<Atom> {
    let place = match field.form() {
        EmitForm::Function | EmitForm::OneofArm { .. } => None,
        EmitForm::Sequence | EmitForm::Map { .. } => Some(build::anonymous()),
        EmitForm::Set => return None,
    };
    Some(occurrence(field, subject, place))
}

/// An enum's membership table (§12.2, §7.4): `ok_e(c).` per declared constant — the table a
/// membership obligation holds an enum-valued field to. Every enum of the unit gets its table,
/// referenced or not: the table is the enum's own, so a field in another package holds its
/// values to it when the packages' `emit.lp` files load together, as their `core.lp` files
/// do for a cross-package sort. Only declared constants are serializable (§7.4); the
/// `unknown(N)` admission under `(keryx.unknown) = PRESERVE` is Increment 5's.
fn membership_table(enumeration: &EnumMapping) -> Vec<WithProvenance<Statement>> {
    let table = names::member(enumeration.predicate());
    let line = signature::enumeration(enumeration);
    enumeration
        .values()
        .iter()
        .map(|value| {
            build::fact(
                build::atom(
                    table.clone(),
                    [build::apply(value.constant().clone(), Vec::new())],
                ),
                doc(Kind::Membership, &line),
            )
        })
        .collect()
}
