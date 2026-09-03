//! `views.lp` (spec §13.2): the relational views — one rule per message-typed field with an
//! access-path occupant, so a model author reaches the field by the anonymous-variable
//! projection idiom (`readings(B, _, E)`) rather than the occupant term. A client of `core.lp`:
//! it opens with `#include "<pkg>.core.lp".` — it joins on the sorts and access-path terms
//! `core.lp` declares, so it is loadable on its own — and each rule carries its one-line
//! signature as its `%!` doc (the proto prose and the functional canon are `core.lp`'s, §13.1).
//! Scalar fields and set membership need no view.

use themelios_program::prelude::*;

use crate::diagnostics::Diagnostics;
use crate::emit::{build, render, signature};
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
            // A view exists exactly for a message-typed field: `FieldMapping::view` is `Some`
            // iff the value is a message (and the form is not `Set`), so pairing the view kind
            // with the referent in one match puts the referent in hand with no re-extraction —
            // "a view on a non-message field" is not a state that reaches here to guard against.
            if let (Some(kind), ValueMapping::Message(referent)) = (field.view(), field.value()) {
                statements.push(view(sort, field, kind, referent.clone()));
            }
        }
    }
    // Open as a client of `core.lp` (§13.2): the include makes `views.lp` loadable on its own,
    // resolving the sorts and access-path terms its rules join on. Emitted as a raw clingo
    // directive — `#include` is a loader meta-statement themelios does not model. The operand is
    // `unit.package()` — a validated `Package` (a dotted identifier, no `"` or control byte), so the
    // interpolation cannot break out of the quoted string; the door represents that shape rather than
    // this site re-checking it (the threat model's descriptor-door package boundary).
    Ok(format!(
        "#include \"{}.core.lp\".\n{}",
        unit.package().as_str(),
        render(statements)?,
    ))
}

/// The relational view rule for one message-typed field (spec §13.2's table): the referent
/// sort is supplied by the caller's match, so this is total by construction with no discharged
/// `expect`.
fn view(
    parent: &SortMapping,
    field: &FieldMapping,
    kind: ViewKind,
    referent: Name,
) -> WithProvenance<Statement> {
    let f = field.predicate().clone();
    // The rule's `%!` doc is the one-line signature only; the field's proto prose lives on the
    // parent sort's `#defined` in `core.lp` (§13.1), which this file includes.
    let doc = signature::field(parent, field);
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
