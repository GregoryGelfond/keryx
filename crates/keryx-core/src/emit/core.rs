//! `core.lp` (spec §13.1): the honorary signature — one documented `#defined` per sort and per
//! base-fact (scalar or enum) field predicate. A message-typed field has no base predicate here
//! (its relational view lives in `views.lp`, §13.2); its functional signature rides on its
//! parent sort's `#defined`, so `core.lp` stays the complete functional canon even when a
//! project excludes `views.lp`. Rendered in themelios's canonical Ord order (P3); a field
//! predicate shared across sorts de-duplicates to one `#defined`, its `%!` docs unioned across
//! the sorts that share it (themelios's content-equal provenance merge).

use crate::diagnostics::Diagnostics;
use crate::emit::{build, doc_line, render, signature};
use crate::policy::model::{Unit, ValueMapping};

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
        // The honorary `#defined e/1` carries the enum's signature line (§13.1) — redundant-but-
        // harmless beside the facts below, which now define the same predicate.
        statements.push(build::defined(
            enumeration.predicate().clone(),
            1,
            doc_line(enumeration.doc(), &signature::enumeration(enumeration)),
        ));
        // Materialize the value domain as a populated sort (§7.4): one ground fact `e(c)` per
        // declared constant, so a model may quantify over the enum's values directly — a
        // conditional literal `f(P, V) : e(V)`, a constraint ranging over every value — exactly
        // as it quantifies over a message sort's occupants (§4). This is core.lp's own
        // model-facing sort, distinct from emit.lp's outbound membership table `ok_e` (§12.2).
        for value in enumeration.values() {
            statements.push(build::fact_bare(build::atom(
                enumeration.predicate().clone(),
                [build::apply(value.constant().clone(), Vec::new())],
            )));
        }
    }
    // Under `(keryx.unknown) = PRESERVE`, admit the escape term `unknown(N)` to the value sort —
    // one rule per enum-valued field of a preserve enum (§7.4), `signal(unknown(N)) :- f(_, …,
    // unknown(N)).`, `N` bound only through the field atom so the rule grounds safe. The referent
    // enum's `preserve` flag rides on the field (denormalized by `policy::resolve_enum_preserve`), so
    // the rule is emitted here even when the enum is imported from another package — the head names
    // the referent sort, which resolves when that package's `core.lp` loads alongside.
    for sort in unit.sorts() {
        for field in sort.fields() {
            let ValueMapping::Enum { referent, preserve } = field.value() else {
                continue;
            };
            if *preserve {
                statements.push(build::escape_admission(
                    referent.clone(),
                    field.predicate().clone(),
                    field.arity(),
                    doc_line(field.doc(), &signature::field(sort, field)),
                ));
            }
        }
    }
    render(statements)
}

#[cfg(test)]
mod tests {
    use themelios_program::Name;

    use super::core;
    use crate::descriptor::model::{FqName, Package, Scalar};
    use crate::policy::model::{
        EmitForm, FieldMapping, ScalarTreatment, SortMapping, Totality, Unit, ValueMapping,
    };

    fn name(text: &str) -> Name {
        Name::new(text).expect("test name is a valid identifier")
    }

    /// A unit whose one composite sort carries two `(keryx.set)` fields — a scalar set `tags` and a
    /// message set `items` — built by hand so the gen artifacts are exercised over the set form over a
    /// minimal unit directly, as the codec tests do, independent of the annotation path that produces
    /// the form in the pipeline.
    fn container_with_two_sets() -> Unit {
        let set_field = |proto: &str, pred: &str, value: ValueMapping, number: i32| FieldMapping {
            proto: FqName::new(proto),
            number,
            predicate: name(pred),
            arity: 2,
            form: EmitForm::Set,
            value,
            presence: Totality::Total,
            escaped: false,
            doc: None,
        };
        let container = SortMapping {
            proto: FqName::new("keryx.t.Container"),
            predicate: name("container"),
            qualifier: Vec::new(),
            escaped: false,
            recursive: false,
            doc: None,
            fields: vec![
                set_field(
                    "keryx.t.Container.tags",
                    "tags",
                    ValueMapping::Scalar {
                        kind: Scalar::String,
                        treatment: ScalarTreatment::Text,
                    },
                    1,
                ),
                set_field(
                    "keryx.t.Container.items",
                    "items",
                    ValueMapping::Message(name("item")),
                    2,
                ),
            ],
        };
        let item = SortMapping {
            proto: FqName::new("keryx.t.Item"),
            predicate: name("item"),
            qualifier: Vec::new(),
            escaped: false,
            recursive: false,
            doc: None,
            fields: vec![FieldMapping {
                proto: FqName::new("keryx.t.Item.sku"),
                number: 1,
                predicate: name("sku"),
                arity: 2,
                form: EmitForm::Function,
                value: ValueMapping::Scalar {
                    kind: Scalar::String,
                    treatment: ScalarTreatment::Text,
                },
                presence: Totality::Total,
                escaped: false,
                doc: None,
            }],
        };
        Unit {
            package: Package::parse("keryx.t").expect("valid package"),
            sorts: vec![container, item],
            enums: Vec::new(),
        }
    }

    #[test]
    fn a_set_field_gets_its_own_defined_in_core_with_the_set_signature() {
        // A set is a first-class membership relation `f/2` — a scalar set `f(P, V)`, a message set
        // `f(P, E)` — so it takes its own `#defined f/2` on `core.lp`'s base-fact path, exactly as
        // any other relation of the vocabulary does. The signature's domain is the parent sort with
        // no `× index` (that is a sequence's), the shape word `set` (§13.1, §7.1).
        let core = core(&container_with_two_sets()).expect("core.lp renders");
        assert!(
            core.contains("tags : container -> string  (set)"),
            "scalar-set signature line missing:\n{core}"
        );
        assert!(
            core.contains("items : container -> item  (set)"),
            "message-set signature line missing:\n{core}"
        );
        assert!(
            core.contains("tags/2"),
            "scalar-set #defined arity 2:\n{core}"
        );
        assert!(
            core.contains("items/2"),
            "message-set #defined arity 2:\n{core}"
        );
    }

    #[test]
    fn a_set_field_generates_no_views_rule() {
        // Membership is a base relation the model asserts, not a projection over occupancy (§7.1),
        // so a set contributes no `views.lp` rule — unlike a message *sequence*, whose
        // `f(P, I, E) :- …` view projects its indexed occupants. The only message-typed field here
        // is the message set `items`, so `views.lp` carries no rule at all.
        let views = crate::emit::views(&container_with_two_sets()).expect("views.lp renders");
        assert!(
            !views.contains(":-"),
            "a set must contribute no views.lp rule:\n{views}"
        );
    }
}
