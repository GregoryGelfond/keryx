//! `core.lp` (spec §13.1): the honorary signature — one documented `#defined` per sort and per
//! base-fact (scalar or enum) field predicate. A message-typed field has no base predicate here
//! (its relational view lives in `views.lp`, §13.2); its functional signature rides on its
//! parent sort's `#defined`, so `core.lp` stays the complete functional canon even when a
//! project excludes `views.lp`. Rendered in themelios's canonical Ord order (P3); a field
//! predicate shared across sorts de-duplicates to one `#defined`, its `%!` docs unioned across
//! the sorts that share it (themelios's content-equal provenance merge).

use crate::diagnostics::Diagnostics;
use crate::emit::{build, doc_line, render, signature};
use crate::policy::model::Unit;

/// Render one generation unit's `core.lp` (spec §13.1). Total (§6).
///
/// # Errors
///
/// [`Diagnostics`] (`UnrenderableFacts`) if themelios cannot spell a symbol (near-
/// impossible for constructed vocabulary).
pub fn core(unit: &Unit) -> Result<String, Diagnostics> {
    let mut statements = Vec::new();
    for sort in unit.sorts() {
        // The sort's `#defined` carries the honorary signature (§13.1): the sort line, then each
        // message-typed field's functional signature (its occupant access-path term, with the
        // field's proto doc) — a message field has no base predicate of its own in `core.lp`, so
        // its signature rides here rather than being lost when `views.lp` is excluded (§13.2).
        let mut sig = signature::sort(sort);
        for field in sort.fields() {
            if field.view().is_some() {
                sig.push('\n');
                sig.push_str(&doc_line(field.doc(), &signature::field(sort, field)));
            }
        }
        statements.push(build::defined(
            sort.predicate().clone(),
            1,
            doc_line(sort.doc(), &sig),
        ));
        // A base-fact field (scalar or enum) carries its own signature on its own `#defined`; a
        // message field's line went to the sort above and it has no declaration here (its view
        // is in `views.lp`), so excluding `views.lp` leaves no dangling declaration.
        for field in sort.fields() {
            if field.view().is_none() {
                statements.push(build::defined(
                    field.predicate().clone(),
                    field.arity(),
                    doc_line(field.doc(), &signature::field(sort, field)),
                ));
            }
        }
    }
    for enumeration in unit.enums() {
        statements.push(build::defined(
            enumeration.predicate().clone(),
            1,
            doc_line(enumeration.doc(), &signature::enumeration(enumeration)),
        ));
    }
    render(statements)
}
