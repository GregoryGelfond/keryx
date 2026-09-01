//! Custom-option reading — the §20 dynamic-layer rule, enforced by construction.
//! Options are read ONLY through prost-reflect's dynamic reflection
//! (`DynamicMessage::extensions`), never a typed prost struct, which would
//! silently drop the extension bytes carrying keryx's annotations (§20,
//! load-bearing). There is no other option-reading path in keryx, so an
//! annotation cannot be dropped; `tests/dynamic_options.rs` proves it survives.

use prost_reflect::{DynamicMessage, ExtensionDescriptor, Kind, Value};

use super::model::{Annotation, AnnotationValue};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

/// The keryx annotations set on an options message (a field's, message's, enum's,
/// or enum value's `.options()`), read dynamically (§20). Non-keryx extensions and
/// base fields are ignored; a repeated option expands to one [`Annotation`] per
/// element; the result is key-ordered (stable, so repeated elements keep order),
/// so the schema is deterministic (P3). `locus` names the element for any
/// malformed-value diagnostic.
pub(super) fn read(options: &DynamicMessage, locus: &str) -> Result<Vec<Annotation>, Diagnostics> {
    let mut out = Vec::new();
    for (extension, value) in options.extensions() {
        let Some(key) = extension
            .full_name()
            .strip_prefix("keryx.")
            .map(str::to_owned)
        else {
            continue;
        };
        match value {
            Value::List(items) => {
                for item in items {
                    out.push(Annotation {
                        key: key.clone(),
                        value: lower(&extension, item, locus)?,
                    });
                }
            }
            scalar => out.push(Annotation {
                key,
                value: lower(&extension, scalar, locus)?,
            }),
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// Lower one option value to an [`AnnotationValue`] (§15). Faithful, not
/// interpreted — scalar-policy meaning is Increment 2's. An enum value is resolved
/// to its name by the extension's own `Kind::Enum`; an integer wider than `i64` is
/// the only malformed case a keryx option can present.
fn lower(
    extension: &ExtensionDescriptor,
    value: &Value,
    locus: &str,
) -> Result<AnnotationValue, Diagnostics> {
    Ok(match value {
        Value::Bool(flag) => AnnotationValue::Bool(*flag),
        Value::I32(number) => AnnotationValue::Int(i64::from(*number)),
        Value::I64(number) => AnnotationValue::Int(*number),
        Value::U32(number) => AnnotationValue::Int(i64::from(*number)),
        Value::U64(number) => AnnotationValue::Int(
            i64::try_from(*number).map_err(|_| malformed(locus, "option integer exceeds i64"))?,
        ),
        Value::String(text) => AnnotationValue::Text(text.clone()),
        Value::EnumNumber(number) => AnnotationValue::Enum(enum_value_name(extension, *number)),
        other => {
            return Err(malformed(locus, &format!("unsupported option value {other:?}")).into());
        }
    })
}

fn enum_value_name(extension: &ExtensionDescriptor, number: i32) -> String {
    match extension.kind() {
        Kind::Enum(enumeration) => enumeration
            .get_value(number)
            .map_or_else(|| number.to_string(), |value| value.name().to_owned()),
        _ => number.to_string(),
    }
}

fn malformed(locus: &str, detail: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::MalformedOption, Locus::at(locus), detail)
}
