//! `core.lp` (spec §13.1): the honorary signature as one documented `#defined` per sort and
//! per base-fact (scalar or enum) field predicate — a message field's predicate is its
//! relational view, declared in `views.lp` (§13.2), not here. Rendered in themelios's canonical
//! Ord order (P3); a field predicate shared across sorts de-duplicates to one `#defined`, its
//! `%!` docs unioned across the sorts that share it (themelios's content-equal provenance merge).

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
        statements.push(build::defined(
            sort.predicate().clone(),
            1,
            doc_line(sort.doc(), &signature::sort(sort)),
        ));
        for field in sort.fields() {
            // A message-typed field's predicate is its relational view (§13.2), declared and
            // defined in `views.lp`; `core.lp` declares only the sorts and the base-fact
            // predicates (scalar and enum fields), so excluding `views.lp` cannot leave a
            // declaration here with no rule to populate it.
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
