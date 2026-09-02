//! `views.lp` (spec §13.2): the relational views — one rule per message-typed field with an
//! access-path occupant, so a model author reaches the field by the anonymous-variable
//! projection idiom (`readings(B, _, E)`) rather than the occupant term. A pure function of the
//! `Mapping`; the field's signature/doc rides as the rule's `%!` doc. Scalar fields and set
//! membership need no view.

use themelios_program::prelude::*;

use crate::diagnostics::Diagnostics;
use crate::emit::{build, doc_line, render, signature};
use crate::policy::model::{FieldMapping, SortMapping, Unit, ValueMapping, ViewKind};

/// Render one generation unit's `views.lp` (spec §13.2). Total (§6).
///
/// # Errors
///
/// [`Diagnostics`] as [`crate::emit::core`].
pub fn views(unit: &Unit) -> Result<String, Diagnostics> {
    let mut statements = Vec::new();
    for sort in unit.sorts() {
        for field in sort.fields() {
            if let Some(kind) = field.view() {
                statements.push(view(sort, field, kind));
            }
        }
    }
    render(statements)
}

/// The relational view rule for one message-typed field (spec §13.2's table). Total: policy
/// sets `view` only on a message-typed field (`FieldMapping::view`, §13.2), so a non-message
/// value reaching here is a keryx-internal invariant violation, not foreign input — the
/// `expect` is discharged by that invariant (§6), the same posture as `facts::terms`'s
/// `vocabulary`, not a diagnostic (there is no adversarial `Mapping`: it is built only at
/// the policy door).
fn view(parent: &SortMapping, field: &FieldMapping, kind: ViewKind) -> WithProvenance<Statement> {
    let referent = match field.value() {
        ValueMapping::Message(name) => Some(name.clone()),
        ValueMapping::Scalar { .. } | ValueMapping::Enum(_) => None,
    }
    .expect("policy sets a view only on a message-typed field (§13.2)");
    let f = field.predicate().clone();
    let doc = doc_line(field.doc(), &signature::field(parent, field));
    let p = build::var("P");
    match kind {
        ViewKind::Singular => build::view_rule(
            build::atom(f.clone(), [p.clone(), build::var("A")]),
            build::atom(referent, [build::var("A")]),
            build::var("A"),
            build::apply(f, vec![p]),
            doc,
        ),
        ViewKind::Sequence => build::view_rule(
            build::atom(f.clone(), [p.clone(), build::var("I"), build::var("E")]),
            build::atom(referent, [build::var("E")]),
            build::var("E"),
            build::apply(f, vec![p, build::var("I")]),
            doc,
        ),
        ViewKind::Map => build::view_rule(
            build::atom(f.clone(), [p.clone(), build::var("K"), build::var("E")]),
            build::atom(referent, [build::var("E")]),
            build::var("E"),
            build::apply(f, vec![p, build::var("K")]),
            doc,
        ),
    }
}
