//! Stage 1 — the mapping policy (architecture §3, R3; spec §21.3): computed in **Rust**
//! (keryx invokes no solver, R4), a pure, deterministic, unique function from the
//! de-sugared [`Schema`] to the [`Mapping`] — name assignment and qualification, presence
//! classification, treatment selection, and reserved-word escapes. The optional ASP
//! co-artifact and its cross-check (spec §21.3) wait for the estate's own
//! elenctic-on-themelios (below the D1 solve boundary); `explain` renders the `Mapping`
//! directly for inspection meanwhile. Submodules: `model` (the mapping model), `names`
//! (un-collided base assignment and reserved-word escapes), `qualify` (the injectivity
//! optimization only — the table it resolves already carries `names`' escapes).
//!
//! [`Schema`]: crate::descriptor::model::Schema
//! [`Mapping`]: model::Mapping

pub mod model;
mod names;
mod qualify;

pub use model::{
    EmitForm, EnumMapping, EnumValueMapping, FieldMapping, Mapping, ScalarTreatment, SortMapping,
    Totality, Unit, ValueMapping, ViewKind,
};

use std::collections::{BTreeMap, BTreeSet};

use themelios_program::Name;

use crate::descriptor::model::{Enum, FqName, Message, Schema};
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
/// [`Diagnostics`] when a lowered name is not a valid ASP identifier, or a field's value
/// type references a message/enum path absent from the schema (both near-impossible on a
/// well-formed `Schema`; checked rather than assumed, §6) — and, reachably from valid input,
/// when two distinct sorts collapse to one predicate qualification cannot separate (§4.2), or
/// two values of one enum lower to a single constant (§7.4).
pub fn map(schema: &Schema) -> Result<Mapping, Diagnostics> {
    let sorts = qualify::resolve(&names::sort_table(schema)?)?; // path -> final sort Name
    let sort_of = |path: &FqName| {
        sorts
            .get(path.as_str())
            .cloned()
            .ok_or_else(|| unresolved_reference(path))
    };
    assemble(schema, &sort_of)
}

/// Group the schema's messages and enums by package into `Unit`s (spec §13), each sort's
/// predicate taken from the qualified `sort_of` map so a reference and its referent agree.
/// Deterministic (P3): units by package; since `Schema` is already path-ordered, sorts and
/// enums within a unit stay path-ordered.
fn assemble(
    schema: &Schema,
    sort_of: &impl Fn(&FqName) -> Result<Name, Diagnostics>,
) -> Result<Mapping, Diagnostics> {
    // Every element's `file()` names a file `ingest` already populated into
    // `schema.files()`, so the `.get` below cannot miss on a well-formed `Schema`;
    // `unwrap_or("")` is defensive, and its default also happens to coincide with the
    // legitimate no-package case (a file that declares no `package`).
    let package_of: BTreeMap<&str, &str> = schema
        .files()
        .iter()
        .map(|file| (file.name.as_str(), file.package.as_str()))
        .collect();
    let mut units: BTreeMap<&str, (Vec<SortMapping>, Vec<EnumMapping>)> = BTreeMap::new();
    for message in schema.messages() {
        let package = package_of.get(message.file()).copied().unwrap_or("");
        let sort = build_sort(message, sort_of)?;
        units.entry(package).or_default().0.push(sort);
    }
    for enumeration in schema.enums() {
        let package = package_of.get(enumeration.file()).copied().unwrap_or("");
        let mapping = build_enum(enumeration, sort_of)?;
        units.entry(package).or_default().1.push(mapping);
    }
    Ok(Mapping {
        units: units
            .into_iter()
            .map(|(package, (sorts, enums))| Unit {
                package: package.to_owned(),
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
) -> Result<SortMapping, Diagnostics> {
    let mut fields = Vec::new();
    for field in message.fields() {
        let oneof = message
            .oneofs()
            .iter()
            .find(|oneof| oneof.arms.contains(&field.number()))
            .map(|oneof| oneof.name.as_str());
        fields.push(names::field_mapping(
            field,
            names::field_name(field)?,
            oneof,
            sort_of,
        )?);
    }
    Ok(SortMapping {
        proto: message.path().clone(),
        predicate: sort_of(message.path())?,
        recursive: message.is_recursive(),
        doc: message.doc().map(str::to_owned),
        fields,
    })
}

/// One enum's `EnumMapping`: its **qualified** sort predicate (from `sort_of`, so a
/// message/enum base-name collision qualifies the enum too — *not* `names::enum_name`, which
/// is the pre-qualification base), and its value constants under the §7.4 strip.
fn build_enum(
    enumeration: &Enum,
    sort_of: &impl Fn(&FqName) -> Result<Name, Diagnostics>,
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
    // the codec increment's (Increment 5); at M1 a residual is reported (loud), never a
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
    Ok(EnumMapping {
        proto: enumeration.path().clone(),
        predicate: sort_of(enumeration.path())?,
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
