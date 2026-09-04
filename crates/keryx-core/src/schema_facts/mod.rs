//! Descriptor facts (spec Appendix C, §21.1): the hand-written stage 0 — the
//! de-sugared [`Schema`] lowered to a flat fact base and rendered to ASP text
//! through themelios `construct`/`render`. Surfaced by `keryx schema-facts` and the
//! §21.2 self-application, which pins these facts golden; the equivalence cross-check
//! against `keryx(descriptor.proto)` is deferred (architecture §11). Not a policy input
//! under the Rust policy.
//! Plain (canonical) `render`: proto docs are `doc/2` facts, so no `%!` annotation
//! or free-standing comment is needed (themelios gap #2 untouched). Deterministic,
//! de-duplicated (themelios's canonical statement order) — golden-comparable (P3).
//!
//! [`Schema`]: crate::descriptor::model::Schema

use themelios_program::prelude::*;
use themelios_program::render::render as render_ast;

use crate::descriptor::model::{
    Annotation, AnnotationValue, Enum, Field, FieldShape, Message, Oneof, Openness, Presence,
    Scalar, Schema, ValueType,
};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::terms;

/// Render `schema` to its descriptor facts as clingo-dialect ASP text (Appendix C).
/// Total over any input: doc and path text always render as string terms, keryx's
/// own vocabulary is a fixed, valid set, and an option key that is not a
/// themelios identifier is diagnosed here rather than panicked through
/// `terms::constant` — see `terms`'s module doc for why a key cannot be assumed
/// valid.
///
/// # Errors
///
/// Returns [`Diagnostics`] (`UnrenderableFacts`) when an annotation's option key
/// is not a themelios identifier (locus: the annotated element's path), or when
/// themelios cannot spell a symbol — composed from an `Unspellable`, never
/// exposed or panicked (§6).
pub fn render(schema: &Schema) -> Result<String, Diagnostics> {
    let program = Program::of(statements(schema)?);
    render_ast(&program, Dialect::Clingo).map_err(|unspellable| {
        Diagnostics::from(Diagnostic::new(
            DiagnosticKind::UnrenderableFacts,
            Locus::whole(),
            format!("{unspellable}"),
        ))
    })
}

fn statements(schema: &Schema) -> Result<Vec<WithProvenance<Statement>>, Diagnostics> {
    let mut out = Vec::new();
    for file in schema.files() {
        out.push(terms::fact(
            "file",
            vec![terms::text(&file.name), terms::text(file.package.as_str())],
        ));
    }
    for message in schema.messages() {
        message_facts(message, &mut out)?;
    }
    for enumeration in schema.enums() {
        enum_facts(enumeration, &mut out)?;
    }
    Ok(out)
}

fn message_facts(
    message: &Message,
    out: &mut Vec<WithProvenance<Statement>>,
) -> Result<(), Diagnostics> {
    let path = message.path().as_str();
    out.push(terms::fact(
        "message",
        vec![terms::text(path), terms::text(message.file())],
    ));
    if let Some(outer) = message.outer() {
        out.push(terms::fact(
            "nested",
            vec![terms::text(path), terms::text(outer.as_str())],
        ));
    }
    if message.is_recursive() {
        out.push(terms::fact("recursive", vec![terms::text(path)]));
    }
    for field in message.fields() {
        field_facts(path, field, out)?;
    }
    for oneof in message.oneofs() {
        oneof_facts(path, oneof, out);
    }
    annotation_facts(path, message.options(), out)?;
    doc_fact(path, message.doc(), out);
    Ok(())
}

fn field_facts(
    message_path: &str,
    field: &Field,
    out: &mut Vec<WithProvenance<Statement>>,
) -> Result<(), Diagnostics> {
    let (type_term, presence, cardinality) = shape_terms(field.shape());
    out.push(terms::fact(
        "field",
        vec![
            terms::text(message_path),
            terms::int(field.number()),
            terms::text(field.name()),
            type_term,
            terms::constant(presence),
            terms::constant(cardinality),
        ],
    ));
    annotation_facts(field.path().as_str(), field.options(), out)?;
    doc_fact(field.path().as_str(), field.doc(), out);
    Ok(())
}

/// The `field/6` type term, presence, and cardinality for a shape. Repeated and
/// map fields carry `implicit` collection presence (§5); the value type of a map
/// rides inside the `map(K, V)` term.
fn shape_terms(shape: &FieldShape) -> (Term, &'static str, &'static str) {
    match shape {
        FieldShape::Singular { value, presence } => {
            (value_term(value), presence_name(*presence), "singular")
        }
        FieldShape::Repeated { value } => (value_term(value), "implicit", "repeated"),
        FieldShape::Map { key, value } => (
            terms::function(
                "map",
                vec![
                    terms::constant(Scalar::from(*key).as_str()),
                    value_term(value),
                ],
            ),
            "implicit",
            "map",
        ),
    }
}

fn value_term(value: &ValueType) -> Term {
    match value {
        ValueType::Scalar(scalar) => terms::constant(scalar.as_str()),
        ValueType::Message(name) => terms::function("msg", vec![terms::text(name.as_str())]),
        ValueType::Enum(name) => terms::function("enum", vec![terms::text(name.as_str())]),
    }
}

fn oneof_facts(message_path: &str, oneof: &Oneof, out: &mut Vec<WithProvenance<Statement>>) {
    for arm in &oneof.arms {
        out.push(terms::fact(
            "oneof",
            vec![
                terms::text(message_path),
                terms::text(&oneof.name),
                terms::int(*arm),
            ],
        ));
    }
    doc_fact(oneof.path.as_str(), oneof.doc.as_deref(), out);
}

fn enum_facts(
    enumeration: &Enum,
    out: &mut Vec<WithProvenance<Statement>>,
) -> Result<(), Diagnostics> {
    let path = enumeration.path().as_str();
    out.push(terms::fact(
        "enum_t",
        vec![
            terms::text(path),
            terms::text(enumeration.file()),
            terms::constant(openness_name(enumeration.openness())),
        ],
    ));
    if let Some(outer) = enumeration.outer() {
        out.push(terms::fact(
            "nested",
            vec![terms::text(path), terms::text(outer.as_str())],
        ));
    }
    for value in enumeration.values() {
        out.push(terms::fact(
            "enum_value",
            vec![
                terms::text(path),
                terms::text(&value.name),
                terms::int(value.number),
            ],
        ));
        annotation_facts(value.path.as_str(), &value.options, out)?;
        doc_fact(value.path.as_str(), value.doc.as_deref(), out);
    }
    annotation_facts(path, enumeration.options(), out)?;
    doc_fact(path, enumeration.doc(), out);
    Ok(())
}

/// One `opt/3` fact per annotation. The key lowering is total (§6): a key that
/// is not a themelios identifier composes an `UnmappableOptionKey` diagnostic (a
/// schema-input error) at the annotated element's `path`, rather than panicking
/// through `terms::constant`.
fn annotation_facts(
    path: &str,
    options: &[Annotation],
    out: &mut Vec<WithProvenance<Statement>>,
) -> Result<(), Diagnostics> {
    for annotation in options {
        let key = terms::try_constant(&annotation.key).map_err(|_| {
            Diagnostic::new(
                DiagnosticKind::UnmappableOptionKey,
                Locus::at(path),
                format!(
                    "option key `{}` is not a themelios identifier",
                    annotation.key
                ),
            )
        })?;
        out.push(terms::fact(
            "opt",
            vec![
                terms::text(path),
                key,
                annotation_value_term(&annotation.value),
            ],
        ));
    }
    Ok(())
}

fn annotation_value_term(value: &AnnotationValue) -> Term {
    match value {
        AnnotationValue::Bool(flag) => terms::constant(if *flag { "true" } else { "false" }),
        AnnotationValue::Int(number) => match i32::try_from(*number) {
            Ok(small) => terms::int(small),
            Err(_) => terms::text(&number.to_string()), // decimal-string when out of i32 range (§6)
        },
        AnnotationValue::Text(text) | AnnotationValue::Enum(text) => terms::text(text),
    }
}

fn doc_fact(path: &str, doc: Option<&str>, out: &mut Vec<WithProvenance<Statement>>) {
    if let Some(text) = doc {
        out.push(terms::fact(
            "doc",
            vec![terms::text(path), terms::text(text)],
        ));
    }
}

fn presence_name(presence: Presence) -> &'static str {
    match presence {
        Presence::Implicit => "implicit",
        Presence::Explicit => "explicit",
        Presence::LegacyRequired => "legacy_required",
    }
}

fn openness_name(openness: Openness) -> &'static str {
    match openness {
        Openness::Open => "open",
        Openness::Closed => "closed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::model::FqName;

    // A minimal one-message `Schema` whose message carries a single annotation
    // under `key` — enough surface to drive `render` directly, without ingest or
    // protox (this module has the model's `pub(crate)` constructors in scope).
    fn schema_with_option_key(key: &str) -> Schema {
        let message = Message {
            path: FqName::new("keryx.adversarial.Sample"),
            file: "adversarial.proto".to_owned(),
            outer: None,
            fields: Vec::new(),
            oneofs: Vec::new(),
            options: vec![Annotation {
                key: key.to_owned(),
                value: AnnotationValue::Bool(true),
            }],
            doc: None,
            recursive: false,
        };
        Schema {
            files: Vec::new(),
            messages: vec![message],
            enums: Vec::new(),
        }
    }

    // `descriptor::options::read`'s admission filter is a
    // file-name heuristic, not true extension identity (see its doc), so a
    // crafted set can carry a non-identifier key this far. Before the total
    // key lowering, this would have panicked at `Name::new`; it must not — it
    // diagnoses, at the annotated element's own path.
    #[test]
    fn a_non_identifier_option_key_is_diagnosed_not_panicked() {
        let schema = schema_with_option_key("Evil");
        let diagnostics = render(&schema).expect_err("a non-identifier option key must not render");
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.iter().next().expect("one diagnostic");
        assert_eq!(diagnostic.kind(), DiagnosticKind::UnmappableOptionKey);
        assert_eq!(diagnostic.locus().path(), Some("keryx.adversarial.Sample"));
    }

    // A genuine keryx-vocabulary key (lowercase-initial) still renders — the
    // total lowering does not reject a valid key.
    #[test]
    fn a_genuine_identifier_option_key_renders() {
        let schema = schema_with_option_key("set");
        render(&schema).expect("a themelios-identifier key renders");
    }
}
