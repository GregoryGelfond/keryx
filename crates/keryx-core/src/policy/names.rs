//! Stage-1 base assignment (spec §4.2, §5, §6, §7): message→sort name, field→predicate,
//! enum value→constant, before collision resolution (`qualify`). Base names are
//! the proto name lowered to `lower_snake` (fields already are; a message `Reading`→
//! `reading`; an enum value strips a shared `ENUM_NAME_` prefix, §7.4). Presence and the
//! emit form/treatment follow §5/§6/§7. Every produced name is validated into a themelios
//! `Name` here, so downstream is total by construction (§6).

use themelios_program::Name;
use themelios_program::symbol::NotAnIdentifier;

use crate::descriptor::model::{
    Enum, EnumValue, Field, FieldShape, FqName, Presence, Scalar, Schema, ValueType,
};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::policy::model::{
    EmitForm, EnumValueMapping, FieldMapping, ScalarTreatment, Totality, ValueMapping, ViewKind,
};

/// Lower a message's short name to its base sort predicate (`lower_snake` of the final
/// path segment, then reserved-word escaped): `dispatch.v1.Reading` → `reading`.
/// Validated into a `Name` (§6). Collision qualification is `qualify`'s.
pub(super) fn sort_name(path: &FqName) -> Result<Name, Diagnostics> {
    identifier(
        &escape_reserved(&lower_snake(final_segment(path.as_str()))),
        path,
    )
}

/// Lower an enum's short name to its base sort predicate, as [`sort_name`].
pub(super) fn enum_name(path: &FqName) -> Result<Name, Diagnostics> {
    sort_name(path)
}

/// The base predicate name of a field: its proto name (already `lower_snake` by protobuf
/// convention, §4.2), reserved-word escaped, validated into a `Name`. Field names are
/// intentionally shared across sorts (§4.2 — polymorphism-by-disjoint-sorts), so they are
/// not collision-qualified; only reserved-word escaping applies.
pub(super) fn field_name(field: &Field) -> Result<Name, Diagnostics> {
    identifier(&escape_reserved(field.name()), field.path())
}

/// The generated-infrastructure and clingo-reserved identifiers a base name must avoid
/// (spec §4.2, §6 — a named table, no magic strings): `not` cannot be a predicate (clingo
/// parses it as negation); `reach`/`violates`/`ep` and the `emit_` prefix are keryx's own
/// generated names (§12.1). An emitted name equal to one of these, or beginning `emit_`,
/// is suffixed `_` and the escape is recorded in the manifest (§13.4).
const RESERVED: &[&str] = &["not", "reach", "violates", "ep"];

/// Escape a lowered name that would collide with a reserved or generated-infrastructure
/// identifier (spec §4.2): suffix `_`. Idempotent on already-legal names. Deterministic.
fn escape_reserved(name: &str) -> String {
    if RESERVED.contains(&name) || name.starts_with("emit_") {
        format!("{name}_")
    } else {
        name.to_owned()
    }
}

/// A message or enum in the pre-qualification sort table (`qualify`'s input): its proto path
/// and its escaped base sort predicate. Message and enum sorts share one /1 namespace, so
/// the entry does not record which it is — a message and an enum with the same base name
/// collide and qualify identically (§4.2); the message-vs-enum distinction lives in the
/// `Mapping` (a `SortMapping` vs. an `EnumMapping`), not here.
pub(super) struct SortEntry {
    pub(super) path: FqName,
    pub(super) base: Name,
}

/// The base sort table: every message and enum path → its escaped base sort entry, in
/// fq-path order (P3). The input to `qualify::resolve`, which resolves any base-name
/// collisions and returns the final `path → Name` map `assemble` reads.
pub(super) fn sort_table(schema: &Schema) -> Result<Vec<SortEntry>, Diagnostics> {
    let mut entries = Vec::new();
    for message in schema.messages() {
        entries.push(SortEntry {
            path: message.path().clone(),
            base: sort_name(message.path())?,
        });
    }
    for enumeration in schema.enums() {
        entries.push(SortEntry {
            path: enumeration.path().clone(),
            base: enum_name(enumeration.path())?,
        });
    }
    // `Schema` already orders messages and enums by fq-path; interleave the two lists by
    // path so the table (and thus qualification's iteration) is deterministic (P3).
    entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    Ok(entries)
}

/// The presence classification (§5): IMPLICIT → total, `EXPLICIT/LEGACY_REQUIRED` → partial.
pub(super) fn totality(presence: Presence) -> Totality {
    match presence {
        Presence::Implicit => Totality::Total,
        Presence::Explicit | Presence::LegacyRequired => Totality::Partial,
    }
}

/// The §6 default scalar treatment (the classification; not enforced at M1).
pub(super) fn scalar_treatment(scalar: Scalar) -> ScalarTreatment {
    match scalar {
        Scalar::Int32 | Scalar::Sint32 | Scalar::Sfixed32 | Scalar::Uint32 | Scalar::Fixed32 => {
            ScalarTreatment::Native
        }
        Scalar::Int64 | Scalar::Uint64 | Scalar::Fixed64 | Scalar::Sfixed64 | Scalar::Sint64 => {
            ScalarTreatment::DecimalString
        }
        Scalar::Float | Scalar::Double => ScalarTreatment::NeedsAnnotation,
        Scalar::Bool => ScalarTreatment::Bool,
        Scalar::String => ScalarTreatment::Text,
        Scalar::Bytes => ScalarTreatment::HexString,
    }
}

/// Map one field to its `FieldMapping` (spec §4.1, §7). `sort_of` resolves
/// a referenced message/enum path to its emitted sort predicate (built by the caller from
/// the same base assignment, so a reference and its referent agree). The oneof name, when
/// the field is an arm, is supplied by the caller (the parent message knows its oneofs).
pub(super) fn field_mapping(
    field: &Field,
    predicate: Name,
    oneof: Option<&str>,
    sort_of: &impl Fn(&FqName) -> Result<Name, Diagnostics>,
) -> Result<FieldMapping, Diagnostics> {
    let (form, arity, value, view) = shape(field.shape(), oneof, sort_of)?;
    let presence = match field.shape() {
        FieldShape::Singular { presence, .. } => totality(*presence),
        FieldShape::Repeated { .. } | FieldShape::Map { .. } => Totality::Total,
    };
    Ok(FieldMapping {
        proto: field.path().clone(),
        number: field.number(),
        predicate,
        arity,
        form,
        value,
        presence,
        view,
        doc: field.doc().map(str::to_owned),
    })
}

/// The (form, arity, value, view) for a field shape (spec §4.1, §7, §13.2). Repeated is always a
/// `Sequence` at M1; `Set` waits for `(keryx.set)` semantics (Increment 5).
fn shape(
    shape: &FieldShape,
    oneof: Option<&str>,
    sort_of: &impl Fn(&FqName) -> Result<Name, Diagnostics>,
) -> Result<(EmitForm, u32, ValueMapping, Option<ViewKind>), Diagnostics> {
    Ok(match shape {
        FieldShape::Singular { value, .. } => {
            let (mapped, view) = singular_value(value, sort_of)?;
            let form = match oneof {
                Some(name) => EmitForm::OneofArm {
                    oneof: name.to_owned(),
                },
                None => EmitForm::Function,
            };
            (form, 2, mapped, view)
        }
        FieldShape::Repeated { value } => match value {
            ValueType::Scalar(scalar) => (
                EmitForm::Sequence,
                3,
                ValueMapping::Scalar {
                    kind: *scalar,
                    treatment: scalar_treatment(*scalar),
                },
                None,
            ),
            ValueType::Message(path) => (
                EmitForm::Sequence,
                3,
                ValueMapping::Message(sort_of(path)?),
                Some(ViewKind::Sequence),
            ),
            ValueType::Enum(path) => (
                EmitForm::Sequence,
                3,
                ValueMapping::Enum(sort_of(path)?),
                None,
            ),
        },
        FieldShape::Map { key, value } => {
            let (arity, mapped, view) = match value {
                ValueType::Scalar(scalar) => (
                    3,
                    ValueMapping::Scalar {
                        kind: *scalar,
                        treatment: scalar_treatment(*scalar),
                    },
                    None,
                ),
                ValueType::Message(path) => (
                    3,
                    ValueMapping::Message(sort_of(path)?),
                    Some(ViewKind::Map),
                ),
                ValueType::Enum(path) => (3, ValueMapping::Enum(sort_of(path)?), None),
            };
            (EmitForm::Map { key: *key }, arity, mapped, view)
        }
    })
}

/// A singular field's value treatment and view: a scalar (no view), a message (the
/// `Singular` view over the referent sort), or an enum (no view).
fn singular_value(
    value: &ValueType,
    sort_of: &impl Fn(&FqName) -> Result<Name, Diagnostics>,
) -> Result<(ValueMapping, Option<ViewKind>), Diagnostics> {
    Ok(match value {
        ValueType::Scalar(scalar) => (
            ValueMapping::Scalar {
                kind: *scalar,
                treatment: scalar_treatment(*scalar),
            },
            None,
        ),
        ValueType::Message(path) => (
            ValueMapping::Message(sort_of(path)?),
            Some(ViewKind::Singular),
        ),
        ValueType::Enum(path) => (ValueMapping::Enum(sort_of(path)?), None),
    })
}

/// The §7.4 prefix-strip length for an enum's value constants, **keyed to the enum name**
/// (not to whatever prefix the values happen to share): the length of
/// `<SCREAMING_SNAKE(enum short name)>_` iff every value name begins with it and leaves a
/// non-empty, identifier-opening, still-distinct remainder; else `0` (§7.4's fallback to
/// unstripped on a collision, an empty remainder, or a remainder that could not open an ASP
/// identifier). So `Level{LEVEL_LOW,…}` strips `LEVEL_` → `low`, but `Status{STATE_OK,…}`
/// (values not sharing the *enum name* `STATUS_`) does not strip → `state_ok`.
/// `SCREAMING_SNAKE` is `lower_snake` upper-cased, so the one word-split rule serves both (a
/// multi-word enum `HttpStatus` → prefix `HTTP_STATUS_`).
pub(super) fn enum_strip(enumeration: &Enum) -> usize {
    let prefix = format!(
        "{}_",
        lower_snake(final_segment(enumeration.path().as_str())).to_ascii_uppercase()
    );
    let all_prefixed = enumeration
        .values()
        .iter()
        .all(|value| value.name.len() > prefix.len() && value.name.starts_with(&prefix));
    if !all_prefixed {
        return 0;
    }
    let mut seen = std::collections::BTreeSet::new();
    for value in enumeration.values() {
        // Compare the *lowered* remainder: stripping that collapses two constants (e.g. a
        // case-only difference `FOO`/`Foo` → `foo`) must fall back to unstripped (§7.4) — a
        // pre-lowering comparison would miss it and keep a strip that produces a collision.
        let remainder = lower_snake(&value.name[prefix.len()..]);
        // …and stripping must not produce a constant that cannot open an ASP identifier. A
        // digit-initial remainder is the case the §21.2 dogfood surfaced: descriptor.proto's
        // own `Edition{EDITION_2023, EDITION_1_TEST_ONLY, …}` strips to `2023`/`1_test_only`,
        // neither a legal identifier, so the strip falls back to unstripped for the whole enum
        // (`edition_2023`, `edition_1_test_only`, …) — the same fallback §7.4 takes for an empty
        // or colliding remainder. `lower_snake` yields a lowercase-letter or digit initial (it
        // trims a leading `_`), so "opens with a lowercase letter" is exactly "opens an ASP
        // identifier"; the per-value [`identifier`] check remains the backstop for the residual.
        if !remainder.starts_with(|c: char| c.is_ascii_lowercase()) {
            return 0;
        }
        if !seen.insert(remainder) {
            return 0;
        }
    }
    prefix.len()
}

/// Lower an enum value to its constant (spec §7.4): strip the enum-name prefix (`strip`
/// chars, from [`enum_strip`]) and lowercase the remainder (`LEVEL_LOW` → `low`), then
/// reserved-word escape. Validated into a `Name`.
pub(super) fn enum_constant(
    value: &EnumValue,
    strip: usize,
) -> Result<EnumValueMapping, Diagnostics> {
    // `strip` is `enum_strip`'s result for this same enum: 0, or an ASCII-prefix byte
    // length it confirmed is a valid char boundary strictly less than every sibling
    // value's name length — never a panic here.
    let lowered = escape_reserved(&lower_snake(&value.name[strip..]));
    Ok(EnumValueMapping {
        proto_name: value.name.clone(),
        number: value.number,
        constant: identifier(&lowered, &value.path)?,
        doc: value.doc.clone(),
    })
}

/// The final dotted segment of a path (`a.b.C` → `C`).
fn final_segment(path: &str) -> &str {
    // `rsplit` always yields at least one item, even for a `path` with no `.` or the
    // empty string, so `unwrap_or` never actually falls back — kept as a total, zero-cost
    // guard rather than an `expect` asserting an iterator-API detail on the caller's behalf.
    path.rsplit('.').next().unwrap_or(path)
}

/// `UpperCamel`/`SCREAMING_SNAKE` → `lower_snake`: insert `_` at lower→upper boundaries,
/// collapse runs of `_`, lowercase. Proto identifiers are `[A-Za-z_][A-Za-z0-9_]*`, so the
/// result is a legal ASP identifier body for almost every proto name — but a name that is
/// one or more leading underscores immediately followed by a digit (e.g. `_2foo`, or a
/// message `_2Foo`) loses that underscore prefix here (the leading-underscore case below
/// starts from an empty `out`, so nothing is pushed) and surfaces the digit as the new
/// leading character: `_2foo` → `2foo`, `_2Foo` → `2_foo`, both real leading-digit results.
/// Deterministic; see [`identifier`] for how that rare shape is handled, not assumed away.
pub(super) fn lower_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut prev_lower_or_digit = false;
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            if prev_lower_or_digit {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_lower_or_digit = false;
        } else if ch == '_' {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_lower_or_digit = false;
        } else {
            out.push(ch);
            prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    out.trim_matches('_').to_owned()
}

/// Validate a lowered name into a themelios identifier `Name`, or compose an
/// `UnmappableName` diagnostic at `locus` (§6 — total). An identifier must start with a
/// lowercase letter; `lower_snake` yields that for nearly every proto name, but not for one
/// that is one or more leading underscores immediately followed by a digit (see
/// [`lower_snake`]) — a real, if rare, *live* rejection, which is exactly why this is
/// checked, not `expect`ed: the input is schema-derived (the estate posture: the one
/// runtime-derived string that reaches a `Name::new` door is checked, cf.
/// `facts::terms::try_konst`).
pub(super) fn identifier(text: &str, locus: &FqName) -> Result<Name, Diagnostics> {
    Name::new(text).map_err(|_: NotAnIdentifier| {
        Diagnostics::from(Diagnostic::new(
            DiagnosticKind::UnmappableName,
            Locus::at(locus.as_str()),
            format!("`{text}` is not a valid ASP identifier"),
        ))
    })
}
