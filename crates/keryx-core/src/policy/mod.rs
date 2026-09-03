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

pub mod model;
mod names;
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
/// [`Diagnostics`] when a subject file declares no `package` (`PackagelessFile`), when a lowered
/// name is not a valid ASP identifier, or a field's value type references a message/enum path
/// absent from the schema (both near-impossible on a well-formed `Schema`; checked rather than
/// assumed, §6) — and, reachably from valid input, when two distinct sorts collapse to one
/// predicate qualification cannot separate (§4.2), or two values of one enum lower to a single
/// constant (§7.4).
pub fn map(schema: &Schema) -> Result<Mapping, Diagnostics> {
    reject_packageless(schema)?;
    let sorts = qualify::resolve(&names::sort_table(schema)?)?; // path -> resolved name + decisions
    assemble(schema, &sorts)
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
    let mut offenders = schema
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
    match offenders.next() {
        None => Ok(()),
        Some(first) => {
            let mut diagnostics = Diagnostics::one(first);
            for diagnostic in offenders {
                diagnostics.push(diagnostic);
            }
            Err(diagnostics)
        }
    }
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
            .ok_or_else(|| unresolved_reference(path))
    };
    // Every element's `file()` names a file `ingest` already populated into
    // `schema.files()`, so the `.get` below cannot miss on a well-formed `Schema`;
    // `unwrap_or(&empty)` is defensive, and the empty `Package` also happens to coincide with the
    // legitimate no-package case (a file that declares no `package`). The unit carries the validated
    // `Package` straight from the file, so no `Unit` re-derives it from a bare string.
    let empty = Package::default();
    let package_of: BTreeMap<&str, &Package> = schema
        .files()
        .iter()
        .map(|file| (file.name.as_str(), &file.package))
        .collect();
    let mut units: BTreeMap<&Package, (Vec<SortMapping>, Vec<EnumMapping>)> = BTreeMap::new();
    for message in schema.messages() {
        let package = package_of.get(message.file()).copied().unwrap_or(&empty);
        let sort = build_sort(message, &sort_of, sorts)?;
        units.entry(package).or_default().0.push(sort);
    }
    for enumeration in schema.enums() {
        let package = package_of
            .get(enumeration.file())
            .copied()
            .unwrap_or(&empty);
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
    // One lookup, one failure posture: a missing entry is `unresolved_reference` (§6), never a
    // silent default that could drop a real qualifier/escape decision. The `sort_of` closure
    // stays for this message's field referents, which resolve *other* sorts' paths.
    let resolved = sorts
        .get(message.path().as_str())
        .ok_or_else(|| unresolved_reference(message.path()))?;
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

/// One enum's `EnumMapping`: its **qualified** sort predicate (from `sort_of`, so a
/// message/enum base-name collision qualifies the enum too), and its value constants under the
/// §7.4 strip. Its sort has no field referents, so — unlike `build_sort` — it needs no `sort_of`
/// closure: the one lookup on the resolved table gives predicate, qualifier, and escape together.
fn build_enum(
    enumeration: &Enum,
    sorts: &BTreeMap<String, qualify::Qualified>,
) -> Result<EnumMapping, Diagnostics> {
    let strip = names::enum_strip(enumeration);
    let mut values = Vec::new();
    for value in enumeration.values() {
        values.push(names::enum_constant(value, strip)?);
    }
    // Within-enum constant injectivity (§7.4, §6): after the prefix-strip fallback two
    // values may still lower to one constant (a case-only difference, e.g. `FOO_BAR`/
    // `FooBar`, or a separator-run difference, e.g. `FOO__BAR`/`FOO_BAR`, since
    // `lower_snake` collapses `_` runs). §7.4 resolves such residuals by qualification —
    // the codec increment's (Increment 5); at present a residual is reported (loud), never a
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
        .ok_or_else(|| unresolved_reference(enumeration.path()))?;
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

/// A reference to a type absent from the schema (§6 — total: a well-formed `Schema` from
/// `ingest` never triggers it, but `map` stays total rather than panicking on a lookup miss).
fn unresolved_reference(path: &FqName) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::UnmappableName,
        Locus::at(path.as_str()),
        format!("`{}` references a type not in the schema", path.as_str()),
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
}
