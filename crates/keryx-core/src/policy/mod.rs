//! Stage 1 — the mapping policy (architecture §3, R3; spec §21.3): computed in **Rust**
//! (keryx invokes no solver, R4), a pure, deterministic, unique function from the
//! de-sugared [`Schema`] to the [`Mapping`] — name assignment and qualification, presence
//! classification, treatment selection, and reserved-word escapes. The optional ASP
//! co-artifact and its cross-check (spec §21.3) are deferred; `explain` renders the `Mapping`
//! directly for inspection meanwhile.
//! Submodules: `model` (the mapping model), `names`
//! (un-collided base assignment and reserved-word escapes), `qualify` (the injectivity
//! optimization only — the table it resolves already carries `names`' escapes).
//!
//! [`Schema`]: crate::descriptor::model::Schema
//! [`Mapping`]: model::Mapping

mod annotate;
pub mod model;
pub(crate) mod names;
mod qualify;

pub use model::{
    Element, EmitForm, EnumMapping, EnumValueMapping, FieldMapping, Mapping, ScalarTreatment,
    SortMapping, Totality, Unit, ValueMapping, ViewKind,
};

use std::collections::{BTreeMap, BTreeSet};

use themelios_program::Name;

use crate::descriptor::model::{Enum, FqName, Message, Package, Schema};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

/// Compute the [`Mapping`] for a schema (spec §21.3) — a pure, deterministic, unique
/// function (R3/R4): base name assignment and reserved-word escapes (`names`), sort
/// collision qualification (`qualify`), then assembly with references already final so a
/// referring field points at its referent's final sort predicate. Total (§6): a name that
/// is not a themelios identifier, or a field whose value type references a path absent
/// from the schema, composes an `UnmappableName` diagnostic rather than panicking.
///
/// # Errors
///
/// [`Diagnostics`] when a subject file declares no `package` (`PackagelessFile`); when a schema name
/// cannot map to an ASP symbol (`UnmappableName`, whose cases that kind's doc enumerates); when two
/// values of one enum lower to a single constant (`AmbiguousConstant`, §7.4); or when a schema
/// element's predicate collides with a generated `has_`/`ok_` auxiliary
/// (`GeneratedPredicateCollision`, §12.2).
pub fn map(schema: &Schema) -> Result<Mapping, Diagnostics> {
    reject_packageless(schema)?;
    let sorts = qualify::resolve(&names::sort_table(schema)?)?; // path -> resolved name + decisions
    let mapping = assemble(schema, &sorts)?;
    // The generated theory reserves the `has_<field>`/`ok_<enum>` auxiliary predicates (§12.2);
    // unlike the marker/`reach`/`violates`/`ep` names, their prefixes are not escaped, so a schema
    // element that lowers onto one is refused rather than silently sharing its extension.
    if let Some(collision) = mapping.units().iter().find_map(first_generated_collision) {
        return Err(Diagnostics::from(collision));
    }
    Ok(mapping)
}

/// Refuse a package-less subject file before any mapping (spec §13, §6): keryx generates one file
/// set per package, so a file with no `package` has no set to name — its generated `.lp`/manifest
/// would be hidden dotfiles. Diagnosed in the library (not only the CLI), one per offending file at
/// that file's locus, so a consumer that links `keryx-core` is told, and told which file — before
/// any `Unit` is formed, so no package-less unit reaches emission. Only a file that contributes a
/// sort or enum can produce a unit, so only those are refused. Both `keryx gen` and `keryx explain`
/// refuse here — `map` is their shared gate — so a package-less file's mapping is neither emitted
/// nor shown.
fn reject_packageless(schema: &Schema) -> Result<(), Diagnostics> {
    let with_subjects: BTreeSet<&str> = schema
        .messages()
        .iter()
        .map(Message::file)
        .chain(schema.enums().iter().map(Enum::file))
        .collect();
    let offenders = schema
        .files()
        .iter()
        .filter(|file| file.package.is_empty() && with_subjects.contains(file.name.as_str()))
        .map(|file| {
            Diagnostic::new(
                DiagnosticKind::PackagelessFile,
                Locus::at(file.name.clone()),
                "a package-less .proto is not supported — declare a `package` (keryx generates one file set per package, §13)",
            )
        });
    Diagnostics::collect(offenders).map_or(Ok(()), Err)
}

/// Group the schema's messages and enums by package into `Unit`s (spec §13), each sort's
/// predicate taken from the qualified `sort_of` map so a reference and its referent agree.
/// Deterministic (P3): units by package; since `Schema` is already path-ordered, sorts and
/// enums within a unit stay path-ordered.
fn assemble(
    schema: &Schema,
    sorts: &BTreeMap<String, qualify::Qualified>,
) -> Result<Mapping, Diagnostics> {
    let sort_of = |path: &FqName| {
        sorts
            .get(path.as_str())
            .map(|q| q.name.clone())
            .ok_or_else(|| missing_sort_entry(path))
    };
    // Every element's `file()` names a file `ingest` already populated into `schema.files()`, so the
    // `.get` below cannot miss on a well-formed `Schema`; a miss is diagnosed (`missing_file`, at the
    // element's locus, the same `UnmappableName` posture `build_sort` uses for a missing sort) — never
    // mapped to a silent empty `Package`, which is a *valid* value ("no package") and would slip a
    // hidden dotfile past `reject_packageless`. The unit carries the validated `Package` straight from
    // the file, so no `Unit` re-derives it from a bare string.
    let package_of: BTreeMap<&str, &Package> = schema
        .files()
        .iter()
        .map(|file| (file.name.as_str(), &file.package))
        .collect();
    let mut units: BTreeMap<&Package, (Vec<SortMapping>, Vec<EnumMapping>)> = BTreeMap::new();
    for message in schema.messages() {
        let package = package_of
            .get(message.file())
            .copied()
            .ok_or_else(|| missing_file(message.path(), message.file()))?;
        let sort = build_sort(message, &sort_of, sorts)?;
        units.entry(package).or_default().0.push(sort);
    }
    for enumeration in schema.enums() {
        let package = package_of
            .get(enumeration.file())
            .copied()
            .ok_or_else(|| missing_file(enumeration.path(), enumeration.file()))?;
        let mapping = build_enum(enumeration, sorts)?;
        units.entry(package).or_default().1.push(mapping);
    }
    Ok(Mapping {
        units: units
            .into_iter()
            .map(|(package, (sorts, enums))| Unit {
                package: package.clone(),
                sorts,
                enums,
            })
            .collect(),
    })
}

/// One message's `SortMapping`: its qualified sort predicate, and a `FieldMapping` per
/// field — the field's oneof (if any) found by scanning the parent's real oneofs for the
/// field number (synthetic proto3-`optional` oneofs are de-sugared away in Increment 1, so
/// only real oneof arms match).
fn build_sort(
    message: &Message,
    sort_of: &impl Fn(&FqName) -> Result<Name, Diagnostics>,
    sorts: &BTreeMap<String, qualify::Qualified>,
) -> Result<SortMapping, Diagnostics> {
    // Option-admission integrity (§21.3): a keryx option on the wrong element category — a field or
    // enum option applied to this message — is a mis-target at the message's locus.
    annotate::reject_foreign_options(
        message.path().as_str(),
        message.options(),
        annotate::Category::Message,
    )?;
    let mut fields = Vec::new();
    for field in message.fields() {
        let oneof = message
            .oneofs()
            .iter()
            .find(|oneof| oneof.arms.contains(&field.number()))
            .map(|oneof| oneof.name.as_str());
        let (predicate, escaped) = names::field_name(field)?;
        fields.push(names::field_mapping(
            field, predicate, escaped, oneof, sort_of,
        )?);
    }
    if let Some(collision) = first_field_collision(&fields) {
        return Err(field_collision(collision));
    }
    // One lookup, one failure posture: a missing entry is `missing_sort_entry` (§6), never a
    // silent default that could drop a real qualifier/escape decision. The `sort_of` closure
    // stays for this message's field referents, which resolve *other* sorts' paths.
    let resolved = sorts
        .get(message.path().as_str())
        .ok_or_else(|| missing_sort_entry(message.path()))?;
    Ok(SortMapping {
        proto: message.path().clone(),
        predicate: resolved.name.clone(),
        qualifier: resolved.qualifier.clone(),
        escaped: resolved.escaped,
        recursive: message.is_recursive(),
        doc: message.doc().map(str::to_owned),
        fields,
    })
}

/// The first field whose emitted (predicate, arity) duplicates an earlier field of the same
/// message (§4.2, §6) — a within-message collision: two distinct proto fields lowered to one
/// predicate (reachable via `lower_snake`, e.g. `camelField`/`camel_field` both `camel_field`,
/// or via a reserved-word escape, e.g. `reach`/`reach_` both `reach_`). `None` when the field
/// predicates are injective. Diagnosed, never a silent non-injective merge.
fn first_field_collision(fields: &[FieldMapping]) -> Option<&FieldMapping> {
    let mut seen: BTreeSet<(&str, u32)> = BTreeSet::new();
    fields
        .iter()
        .find(|field| !seen.insert((field.predicate().as_str(), field.arity())))
}

/// The within-message field-collision diagnostic (§6), at the offending field's locus.
fn field_collision(field: &FieldMapping) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::UnmappableName,
        Locus::at(field.proto().as_str()),
        format!(
            "two fields of this message lower to the predicate `{}/{}`",
            field.predicate().as_str(),
            field.arity()
        ),
    ))
}

/// The first schema element of `unit` whose emitted predicate collides — same name and arity —
/// with a `has_<field>` witness or `ok_<enum>` membership table that `emit.lp` generates
/// (spec §12.2, §6). The marker `emit_<sort>`, `reach`, `violates`, and `ep` are collision-proof by
/// the reserved-word escape (`names::escape_reserved`); the `has_`/`ok_` prefixes are deliberately
/// *not* escaped — escaping those common prefixes wholesale would rename innocent fields like
/// `has_permission` — so a schema whose own message/enum/field lowers onto one is diagnosed here
/// rather than silently sharing the auxiliary's extension, which would corrupt the generated theory
/// for the consuming tool's solver. The reserved set is [`generated_auxiliaries`]. Per unit (a
/// package's `emit.lp`); a cross-package collision — a user predicate meeting another unit's
/// auxiliary when both `.lp` files load together — is a narrower, load-dependent residual, not
/// diagnosed here. `None` when the unit's user predicates are disjoint from its generated auxiliaries.
fn first_generated_collision(unit: &Unit) -> Option<Diagnostic> {
    let auxiliaries = generated_auxiliaries(unit);
    // The unit's user predicates in deterministic order (P3): each sort, its fields, then each enum.
    // The first that occupies an auxiliary's (name, arity) is the offender.
    for sort in unit.sorts() {
        if let Some(origin) = auxiliaries.get(&(sort.predicate().as_str().to_owned(), 1)) {
            return Some(generated_collision(
                sort.proto(),
                sort.predicate(),
                1,
                origin,
            ));
        }
        for field in sort.fields() {
            if let Some(origin) =
                auxiliaries.get(&(field.predicate().as_str().to_owned(), field.arity()))
            {
                return Some(generated_collision(
                    field.proto(),
                    field.predicate(),
                    field.arity(),
                    origin,
                ));
            }
        }
    }
    for enumeration in unit.enums() {
        if let Some(origin) = auxiliaries.get(&(enumeration.predicate().as_str().to_owned(), 1)) {
            return Some(generated_collision(
                enumeration.proto(),
                enumeration.predicate(),
                1,
                origin,
            ));
        }
    }
    None
}

/// The `(predicate, arity)` each `has_<field>` witness and `ok_<enum>` membership table `emit.lp`
/// mints for `unit`, mapped to a phrase naming the field or enum it belongs to. Two halves are
/// protected differently, and only both together keep this set equal to the emitted one: the
/// **names** cannot drift — this and `emit.lp` both take them through the same
/// `names::witness`/`names::member`; the **form→arity classification** — *which* form at *which*
/// arity mints an auxiliary — is hand-mirrored here against `emit_lp::singular` (a presence witness
/// `has_<f>/1` for a total singular non-message field), `emit_lp::sequence` (an index witness
/// `has_<f>/2` for a sequence field), and `emit_lp::membership_table` (a table `ok_<e>/1` per enum),
/// and must be updated with them. That the two agree is not left to discipline: the test
/// `the_reserved_auxiliary_set_equals_what_emit_lp_mints` asserts this set equals the `has_`/`ok_`
/// heads `emit_strict` actually emits, so a desync fails a test rather than silently corrupting the
/// theory — Increment 5's `(keryx.set)` and annotation-driven treatments move arities across exactly
/// this surface.
fn generated_auxiliaries(unit: &Unit) -> BTreeMap<(String, u32), String> {
    // Owned keys: an auxiliary name is a fresh `Name`.
    let mut auxiliaries: BTreeMap<(String, u32), String> = BTreeMap::new();
    for sort in unit.sorts() {
        for field in sort.fields() {
            let arity = match field.form() {
                EmitForm::Function | EmitForm::OneofArm { .. }
                    if field.view().is_none()
                        && matches!(field.presence(), Totality::Total | Totality::Required) =>
                {
                    1 // the presence witness `has_f(P)` (emit_lp::singular) — `Total`/`Required`
                }
                EmitForm::Sequence => 2, // the index witness `has_f(P, I)` (emit_lp::sequence)
                _ => continue, // a message view, an EXPLICIT partial, a map, or a set: no witness
            };
            auxiliaries.insert(
                (names::witness(field.predicate()).as_str().to_owned(), arity),
                format!(
                    "the witness `emit.lp` generates for field `{}`",
                    field.proto().as_str()
                ),
            );
        }
    }
    for enumeration in unit.enums() {
        auxiliaries.insert(
            (
                names::member(enumeration.predicate()).as_str().to_owned(),
                1,
            ),
            format!(
                "the membership table `emit.lp` generates for enum `{}`",
                enumeration.proto().as_str()
            ),
        );
    }
    auxiliaries
}

/// The `GeneratedPredicateCollision` diagnostic (§6, §12.2), at the offending element's locus: its
/// predicate collides with `origin` (a phrase naming the auxiliary's field or enum). Names the fix —
/// rename the element — since keryx does not escape the `has_`/`ok_` auxiliary prefixes.
fn generated_collision(proto: &FqName, predicate: &Name, arity: u32, origin: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::GeneratedPredicateCollision,
        Locus::at(proto.as_str()),
        format!(
            "this element lowers to the predicate `{}/{arity}`, which collides with {origin}; rename \
             it (keryx does not escape the generated `has_`/`ok_` auxiliary prefixes)",
            predicate.as_str(),
        ),
    )
}

/// One enum's `EnumMapping`: its **qualified** sort predicate (from `sort_of`, so a
/// message/enum base-name collision qualifies the enum too), and its value constants under the
/// §7.4 strip. Its sort has no field referents, so — unlike `build_sort` — it needs no `sort_of`
/// closure: the one lookup on the resolved table gives predicate, qualifier, and escape together.
fn build_enum(
    enumeration: &Enum,
    sorts: &BTreeMap<String, qualify::Qualified>,
) -> Result<EnumMapping, Diagnostics> {
    // Option-admission integrity (§21.3): validate `(keryx.unknown)` against the enum's openness at
    // the policy door — a `PRESERVE` on a closed enum is a mis-target diagnostic here, never a silent
    // no-op. The resolved flag is consumed on `EnumMapping` in the PRESERVE task (Increment 5).
    annotate::enum_preserve(enumeration)?;
    // And a keryx option on the wrong element category — a field or message option on this enum — is
    // a mis-target here.
    annotate::reject_foreign_options(
        enumeration.path().as_str(),
        enumeration.options(),
        annotate::Category::Enum,
    )?;
    let strip = names::enum_strip(enumeration);
    let mut values = Vec::new();
    for value in enumeration.values() {
        values.push(names::enum_constant(value, strip)?);
    }
    // Within-enum constant injectivity (§7.4, §6): after the prefix-strip fallback two
    // values may still lower to one constant (a case-only difference, e.g. `FOO_BAR`/
    // `FooBar`, or a separator-run difference, e.g. `FOO__BAR`/`FOO_BAR`, since
    // `lower_snake` collapses `_` runs). §7.4 resolves such residuals by qualification —
    // the annotation increment's (Increment 5); at present a residual is reported (loud), never a
    // silent duplicate constant.
    let mut seen = BTreeSet::new();
    for value in &values {
        if !seen.insert(value.constant().as_str()) {
            return Err(Diagnostics::from(Diagnostic::new(
                DiagnosticKind::AmbiguousConstant,
                Locus::at(enumeration.path().as_str()),
                format!(
                    "two values of this enum lower to the constant `{}`",
                    value.constant().as_str()
                ),
            )));
        }
    }
    let resolved = sorts
        .get(enumeration.path().as_str())
        .ok_or_else(|| missing_sort_entry(enumeration.path()))?;
    Ok(EnumMapping {
        proto: enumeration.path().clone(),
        predicate: resolved.name.clone(),
        qualifier: resolved.qualifier.clone(),
        escaped: resolved.escaped,
        openness: enumeration.openness(),
        doc: enumeration.doc().map(str::to_owned),
        values,
    })
}

/// A message or enum path with no entry in the resolved sort table (§6 — total: a well-formed
/// `Schema` from `ingest` never triggers it, but `map` stays total rather than panicking on a lookup
/// miss). Two lookups miss into this: a field's value-type referent absent from the schema (the
/// `sort_of` closure), and an element's own entry absent from `qualify::resolve`'s output
/// (`build_sort`/`build_enum`). `UnmappableName` at the path's locus — for the referent lookup, the
/// referent's; for the self-lookup, the element's own.
fn missing_sort_entry(path: &FqName) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::UnmappableName,
        Locus::at(path.as_str()),
        format!("`{}` has no resolved sort entry", path.as_str()),
    ))
}

/// An element whose declaring `file` is absent from `schema.files()` (§6 — total: `ingest` pushes a
/// `File` for every element's declaring file before the element — subjects in the first pass,
/// referent-closure sorts in the second — so a well-formed `Schema` never triggers it, but the lookup
/// is checked, not assumed). `UnmappableName` at the element's locus; the absence named is
/// the *file*, not a sort entry (`missing_sort_entry`) or a lowered name (`names::identifier`).
fn missing_file(path: &FqName, file: &str) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::UnmappableName,
        Locus::at(path.as_str()),
        format!(
            "`{}` is declared in `{file}`, which is not in the schema",
            path.as_str()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::map;
    use crate::descriptor::model::{
        Field, FieldShape, File, FqName, Message, Package, Presence, Scalar, Schema, ValueType,
    };
    use crate::diagnostics::DiagnosticKind;

    /// A within-message field collision (two distinct fields lowering to one predicate) is not
    /// reachable through a protoc/protox-compiled `.proto` — the compiler rejects the ambiguous
    /// field names first — but a descriptor set supplied directly (`gen foo.binpb`, or another
    /// producer) can carry it, so `map` diagnoses rather than silently conflating (§4.2, §6).
    #[test]
    fn colliding_field_predicates_are_diagnosed() {
        let field = |number: i32, name: &str| Field {
            number,
            name: name.to_owned(),
            path: FqName::new(format!("m.Clash.{name}")),
            shape: FieldShape::Singular {
                value: ValueType::Scalar(Scalar::String),
                presence: Presence::Implicit,
            },
            options: Vec::new(),
            doc: None,
        };
        // `Foo` and `FOO` both lower to `foo/2` on one message.
        let schema = Schema {
            files: vec![File {
                name: "m.proto".to_owned(),
                package: Package::parse("m").expect("valid package"),
            }],
            messages: vec![Message {
                path: FqName::new("m.Clash"),
                file: "m.proto".to_owned(),
                outer: None,
                fields: vec![field(1, "Foo"), field(2, "FOO")],
                oneofs: Vec::new(),
                options: Vec::new(),
                doc: None,
                recursive: false,
            }],
            enums: Vec::new(),
        };

        let error = map(&schema).expect_err("a within-message field collision is diagnosed");
        assert_eq!(
            error.iter().next().unwrap().kind(),
            DiagnosticKind::UnmappableName
        );
    }

    #[test]
    fn an_element_whose_file_is_absent_is_diagnosed_not_defaulted() {
        // The package lookup's can't-happen miss — an element declaring a file absent from the
        // schema's file list — is diagnosed at the element's locus (`missing_file`, `UnmappableName`),
        // never mapped to an empty `Package` that would slip a hidden dotfile past `reject_packageless`.
        // `ingest` cannot produce this (it populates every subject file first); a hand-built `Schema`
        // reaches the branch, as the crate tests its other can't-happen guards.
        let schema = Schema {
            files: Vec::new(), // no file list — the message's `file()` resolves to nothing
            messages: vec![Message {
                path: FqName::new("m.M"),
                file: "m.proto".to_owned(),
                outer: None,
                fields: Vec::new(),
                oneofs: Vec::new(),
                options: Vec::new(),
                doc: None,
                recursive: false,
            }],
            enums: Vec::new(),
        };
        let error = map(&schema).expect_err("an element with an absent file is diagnosed");
        let diagnostic = error.iter().next().expect("one diagnostic");
        assert_eq!(diagnostic.kind(), DiagnosticKind::UnmappableName);
        assert_eq!(diagnostic.locus().path(), Some("m.M"));
    }

    #[test]
    fn a_dangling_field_referent_is_diagnosed_at_the_referent() {
        // A field whose value type names a message the schema lacks: the referent lookup (`sort_of`)
        // misses and composes `missing_sort_entry` at the *referent's* locus (`UnmappableName`) — the
        // locus the kind's contract promises for this case. Unreachable through `ingest` (which never
        // leaves a dangling reference); a hand-built `Schema` reaches it.
        let schema = Schema {
            files: vec![File {
                name: "m.proto".to_owned(),
                package: Package::parse("m").expect("valid package"),
            }],
            messages: vec![Message {
                path: FqName::new("m.M"),
                file: "m.proto".to_owned(),
                outer: None,
                fields: vec![Field {
                    number: 1,
                    name: "f".to_owned(),
                    path: FqName::new("m.M.f"),
                    shape: FieldShape::Singular {
                        value: ValueType::Message(FqName::new("m.Absent")),
                        presence: Presence::Implicit,
                    },
                    options: Vec::new(),
                    doc: None,
                }],
                oneofs: Vec::new(),
                options: Vec::new(),
                doc: None,
                recursive: false,
            }],
            enums: Vec::new(),
        };
        let error = map(&schema).expect_err("a dangling field referent is diagnosed");
        let diagnostic = error.iter().next().expect("one diagnostic");
        assert_eq!(diagnostic.kind(), DiagnosticKind::UnmappableName);
        assert_eq!(diagnostic.locus().path(), Some("m.Absent"));
    }

    // --- generated-auxiliary collision (`GeneratedPredicateCollision`, §12.2) ---
    //
    // A schema element whose predicate lowers onto a `has_<field>`/`ok_<enum>` auxiliary `emit.lp`
    // generates is refused (the marker/`reach`/`violates`/`ep` names are escaped; these prefixes
    // deliberately are not). Unreachable through a compile of a schema that does not itself declare
    // such names; a directly-supplied descriptor set can carry them, so `map` diagnoses.

    use crate::descriptor::model::{Enum, EnumValue, Openness};

    fn m_schema(messages: Vec<Message>, enums: Vec<Enum>) -> Schema {
        Schema {
            files: vec![File {
                name: "m.proto".to_owned(),
                package: Package::parse("m").expect("valid package"),
            }],
            messages,
            enums,
        }
    }

    fn message(name: &str, fields: Vec<Field>) -> Message {
        Message {
            path: FqName::new(format!("m.{name}")),
            file: "m.proto".to_owned(),
            outer: None,
            fields,
            oneofs: Vec::new(),
            options: Vec::new(),
            doc: None,
            recursive: false,
        }
    }

    fn singular_field(owner: &str, number: i32, name: &str, presence: Presence) -> Field {
        Field {
            number,
            name: name.to_owned(),
            path: FqName::new(format!("m.{owner}.{name}")),
            shape: FieldShape::Singular {
                value: ValueType::Scalar(Scalar::String),
                presence,
            },
            options: Vec::new(),
            doc: None,
        }
    }

    fn sequence_field(owner: &str, number: i32, name: &str) -> Field {
        Field {
            number,
            name: name.to_owned(),
            path: FqName::new(format!("m.{owner}.{name}")),
            shape: FieldShape::Repeated {
                value: ValueType::Scalar(Scalar::String),
            },
            options: Vec::new(),
            doc: None,
        }
    }

    fn level_enum(name: &str) -> Enum {
        let screaming = name.to_ascii_uppercase();
        Enum {
            path: FqName::new(format!("m.{name}")),
            file: "m.proto".to_owned(),
            outer: None,
            openness: Openness::Closed,
            values: vec![EnumValue {
                name: format!("{screaming}_LOW"),
                number: 0,
                path: FqName::new(format!("m.{name}.{screaming}_LOW")),
                options: Vec::new(),
                doc: None,
            }],
            options: Vec::new(),
            doc: None,
        }
    }

    fn is_generated_collision(schema: &Schema) -> bool {
        map(schema).is_err_and(|error| {
            error
                .iter()
                .any(|diagnostic| diagnostic.kind() == DiagnosticKind::GeneratedPredicateCollision)
        })
    }

    #[test]
    fn a_sort_colliding_with_a_presence_witness_is_diagnosed() {
        // Message `HasSensor` -> `has_sensor/1`; a total singular field `sensor` -> its presence
        // witness `has_sensor/1`. Same unit, same arity: refused.
        let schema = m_schema(
            vec![
                message("HasSensor", vec![]),
                message(
                    "Reading",
                    vec![singular_field("Reading", 1, "sensor", Presence::Implicit)],
                ),
            ],
            vec![],
        );
        assert!(is_generated_collision(&schema));
    }

    #[test]
    fn a_singular_field_colliding_with_an_index_witness_is_diagnosed() {
        // A singular field `has_notes` -> `has_notes/2`; a sequence field `notes` -> its index
        // witness `has_notes/2`. Same unit, same arity: refused.
        let schema = m_schema(
            vec![message(
                "Note",
                vec![
                    sequence_field("Note", 1, "notes"),
                    singular_field("Note", 2, "has_notes", Presence::Implicit),
                ],
            )],
            vec![],
        );
        assert!(is_generated_collision(&schema));
    }

    #[test]
    fn a_sort_colliding_with_a_membership_table_is_diagnosed() {
        // Message `OkLevel` -> `ok_level/1`; enum `Level` -> its membership table `ok_level/1`.
        let schema = m_schema(vec![message("OkLevel", vec![])], vec![level_enum("Level")]);
        assert!(is_generated_collision(&schema));
    }

    #[test]
    fn a_predicate_at_a_different_arity_than_the_witness_is_not_a_collision() {
        // A singular field `has_sensor` -> `has_sensor/2`; the total singular field `sensor` -> a
        // presence witness `has_sensor/1`. Arities differ, so no collision: the mapping succeeds.
        let schema = m_schema(
            vec![message(
                "Reading",
                vec![
                    singular_field("Reading", 1, "sensor", Presence::Implicit),
                    singular_field("Reading", 2, "has_sensor", Presence::Implicit),
                ],
            )],
            vec![],
        );
        assert!(map(&schema).is_ok());
    }

    #[test]
    fn a_sort_beside_a_partial_field_generating_no_witness_is_not_a_collision() {
        // A partial singular field `sensor` generates no presence witness (only a total one does),
        // so `has_sensor/1` is unclaimed and message `HasSensor` -> `has_sensor/1` does not collide.
        let schema = m_schema(
            vec![
                message("HasSensor", vec![]),
                message(
                    "Reading",
                    vec![singular_field("Reading", 1, "sensor", Presence::Explicit)],
                ),
            ],
            vec![],
        );
        assert!(map(&schema).is_ok());
    }

    #[test]
    fn the_reserved_auxiliary_set_equals_what_emit_lp_mints() {
        // The collision check is sound only if `generated_auxiliaries` reserves *exactly* the
        // `(name, arity)` set `emit.lp` mints. The names are shared through `names::witness`/
        // `names::member`; the form→arity classification is hand-mirrored against
        // `emit_lp::{singular,sequence,membership_table}`. This test makes that mirror mechanical —
        // for every fixture unit, the reserved set must equal the `has_`/`ok_` heads `emit_strict`
        // actually emits, so a future desync (Increment 5 moves arities across this surface) fails
        // here rather than silently under-reserving (theory corruption) or over-refusing.
        use std::collections::BTreeSet;

        use keryx_test_support as support;

        use crate::descriptor::ingest;
        use crate::emit;

        // The `has_`/`ok_` rule/fact heads of an emitted theory, as (predicate, arity). A head is
        // the text before `:-` (a rule) or the trailing `.` (a fact); `%!` docs, `#` directives, and
        // integrity constraints (empty head before `:-`) contribute none, and `has_`/`ok_` in a body
        // is after `:-`, unseen. The auxiliaries' heads are flat — `has_f(P)`, `has_f(P, I)`,
        // `ok_e(c)` — so arity is the top-level argument count.
        fn emitted_aux_heads(theory: &str) -> BTreeSet<(String, u32)> {
            let mut heads = BTreeSet::new();
            for line in theory.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('%') || line.starts_with('#') {
                    continue;
                }
                let head = line.split(":-").next().unwrap_or("").trim();
                let head = head.strip_suffix('.').unwrap_or(head).trim();
                let Some(open) = head.find('(') else { continue };
                let name = &head[..open];
                if !(name.starts_with("has_") || name.starts_with("ok_")) {
                    continue;
                }
                let close = head.rfind(')').expect("a head opening `(` also closes");
                let args = head[open + 1..close].trim();
                let arity = if args.is_empty() {
                    0
                } else {
                    u32::try_from(args.split(',').count()).expect("a small arity")
                };
                heads.insert((name.to_owned(), arity));
            }
            heads
        }

        for fixture in [
            "obligations.proto",
            "reach.proto",
            "proto2.proto",
            "gmap.proto",
        ] {
            let schema = ingest(&support::compile_fixture(fixture)).expect("ingests");
            let mapping = map(&schema).expect("maps");
            for unit in mapping.units() {
                let reserved: BTreeSet<(String, u32)> =
                    super::generated_auxiliaries(unit).into_keys().collect();
                let emitted = emitted_aux_heads(&emit::emit_strict(unit).expect("emits"));
                assert_eq!(
                    reserved,
                    emitted,
                    "reserved auxiliaries must equal emitted heads for {fixture} unit `{}`",
                    unit.package().as_str()
                );
            }
        }
    }
}
