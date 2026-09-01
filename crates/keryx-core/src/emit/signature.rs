//! The §13.1 signature lines — the text a `core.lp` `#defined` carries as its `%!` doc.
//! Pure string formatting over the `Mapping` in the spec's own vocabulary (`sort`, `×
//! index`, `->`); no themelios. Deterministic.

use crate::descriptor::model::{Openness, Scalar};
use crate::policy::model::{
    EmitForm, EnumMapping, FieldMapping, SortMapping, Totality, ValueMapping,
};

/// A sort's line: `sort reading/1` (+ `  (recursive)` under a containment cycle, §8).
pub(super) fn sort(sort: &SortMapping) -> String {
    let line = format!("sort {}/1", sort.predicate().as_str());
    if sort.is_recursive() {
        format!("{line}  (recursive)")
    } else {
        line
    }
}

/// An enum's line: `enum level/1  (open)`.
pub(super) fn enumeration(enumeration: &EnumMapping) -> String {
    let openness = if matches!(enumeration.openness(), Openness::Open) {
        "open"
    } else {
        "closed"
    };
    format!("enum {}/1  ({openness})", enumeration.predicate().as_str())
}

/// A field's line (spec §13.1): `sensor : reading -> string  (total)`,
/// `readings : reading_batch × index -> reading  (sequence)`,
/// `counts : inventory × string -> int32  (map)`,
/// `dock : shipment -> string  (oneof handoff, partial)`.
pub(super) fn field(parent: &SortMapping, field: &FieldMapping) -> String {
    format!(
        "{} : {} -> {}  ({})",
        field.predicate().as_str(),
        domain(parent.predicate().as_str(), field.form()),
        range(field.value()),
        shape(field),
    )
}

fn domain(parent: &str, form: &EmitForm) -> String {
    match form {
        EmitForm::Function | EmitForm::OneofArm { .. } | EmitForm::Set => parent.to_owned(),
        EmitForm::Sequence => format!("{parent} × index"),
        EmitForm::Map { key } => format!("{parent} × {}", Scalar::from(*key).as_str()),
    }
}

fn range(value: &ValueMapping) -> String {
    match value {
        ValueMapping::Scalar { kind, .. } => kind.as_str().to_owned(),
        ValueMapping::Message(name) | ValueMapping::Enum(name) => name.as_str().to_owned(),
    }
}

fn shape(field: &FieldMapping) -> String {
    match field.form() {
        EmitForm::Function => totality(field.presence()).to_owned(),
        EmitForm::Sequence => "sequence".to_owned(),
        EmitForm::Set => "set".to_owned(),
        EmitForm::Map { .. } => "map".to_owned(),
        EmitForm::OneofArm { oneof } => format!("oneof {oneof}, {}", totality(field.presence())),
    }
}

fn totality(t: Totality) -> &'static str {
    match t {
        Totality::Total => "total",
        Totality::Partial => "partial",
    }
}
