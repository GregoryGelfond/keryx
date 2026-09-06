//! `emit.lp` (spec §13.3): the serializability theory. Outbound, the vocabulary's invariants
//! are obligations rather than theorems (Part III), and this module renders the frame they
//! stand on — the response-root markers and the reachability closure (§12.1). Every message
//! sort gets a marker `emit_<sort>/1`: the model asserts `emit_<sort>(term)` to export the tree
//! under `term`, and an unasserted marker is inert, so root designation costs a schema nothing
//! before an annotation names one. `reach/1` then closes over the message-typed slots from
//! every asserted root, so an obligation guarded by it binds only what is exported and the
//! model's working predicates stay unconstrained. A client of `core.lp` alone: the closure
//! joins on occupancy — the child sort atom — never on the `views.lp` relations, so a project
//! that excludes `views.lp` loses nothing here. Rendered in canonical order (P3). The two modes
//! (§12.2) differ only in how an obligation is written; the obligations are not yet emitted,
//! so each mode's function renders the frame.

use themelios_program::prelude::*;

use crate::diagnostics::Diagnostics;
use crate::emit::{build, render_client_of_core, signature};
use crate::policy::model::{FieldMapping, SortMapping, Unit, ValueMapping, ViewKind};
use crate::policy::names;

/// Render one generation unit's `emit.lp` in strict mode (spec §12.2, §13.3), where an
/// obligation is an integrity constraint and an unserializable answer set is UNSAT. Total
/// (§6). At present the frame alone — the markers and the closure (§12.1); the obligations
/// follow.
///
/// # Errors
///
/// [`Diagnostics`] as [`crate::emit::core`].
pub fn emit_strict(unit: &Unit) -> Result<String, Diagnostics> {
    render_client_of_core(unit, frame(unit))
}

/// Render one generation unit's `emit.lp` in diagnostic mode (spec §12.2, §13.3), where an
/// obligation derives `violates(field-path, occupant)` and the model survives for the
/// reassembler to report. Total (§6). At present the frame alone — the markers and the
/// closure (§12.1); the obligations follow.
///
/// # Errors
///
/// [`Diagnostics`] as [`crate::emit::core`].
pub fn emit_diagnostic(unit: &Unit) -> Result<String, Diagnostics> {
    render_client_of_core(unit, frame(unit))
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
            // is `Some` iff the value is a message (and the form is not `Set`), so pairing the
            // view kind with the referent in one match puts the child sort in hand.
            if let (Some(kind), ValueMapping::Message(referent)) = (field.view(), field.value()) {
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
    let args = match kind {
        ViewKind::Singular => vec![x.clone()],
        ViewKind::Sequence => vec![x.clone(), build::var("I")],
        ViewKind::Map => vec![x.clone(), build::var("K")],
    };
    build::rule(
        build::atom(reach.clone(), [a.clone()]),
        vec![
            build::positive(build::atom(reach, [x.clone()])),
            build::positive(build::atom(parent.predicate().clone(), [x])),
            build::positive(build::atom(referent, [a.clone()])),
            build::compare(
                a,
                Relation::Eq,
                build::apply(field.predicate().clone(), args),
            ),
        ],
        // The rule's `%!` doc is the field's one-line signature; its proto prose lives on the
        // parent sort's `#defined` in `core.lp` (§13.1), which this file includes.
        signature::field(parent, field),
    )
}
