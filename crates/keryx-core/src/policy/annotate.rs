//! The annotation resolution-and-validation step (spec §21.3, arch §3) — **the option-admission
//! integrity surface** (`docs/design/threat-model.md`, the descriptor door's *named vector*).
//!
//! Annotations ride on the schema model since Increment 1 (`Field::options()`,
//! `Enum::options()` — `Vec<Annotation>`), keyed by the option name with `keryx.` stripped
//! (`set`, `scale`, `opaque`, `numeric`, `unknown`). This module resolves each applied option
//! against its target — the field's scalar kind and cardinality, the enum's openness — into the
//! `Mapping`, or refuses it with a structured diagnostic. It is **total**: every option value is
//! validated, and a mis-targeted, malformed, or profile-gated option is a `MalformedOption`
//! diagnostic at the annotated element's proto path, never a panic, an `unwrap`, or a silent
//! mis-lowering (arch §6; threat-model property 4). The codec (`walk`/`assemble`/`scalar`) and
//! `emit` dispatch on the resolved `Mapping`, never on an `Annotation` (threat-model.md).
//!
//! **The classification rule (F4c), applied uniformly:** an *inapplicable* option — one that can
//! never have its intended effect on this target (`scale`/`opaque` on a non-float, `set` on a
//! non-repeated field, `PRESERVE` on a closed enum, `numeric` on a non-integer) — is a **mis-target
//! diagnostic**; an option whose effect *coincides with the target's default* (`DECIMAL_STRING` on a
//! 64-bit field, `NATIVE_CHECKED` on `uint32`/`fixed32`) is a **faithful no-op admit**, the §6
//! default returned unchanged. The mapping a hostile annotation set yields is a faithful `Mapping`
//! or a diagnosis — never one that mis-serialises.
//!
//! Per option, its **positive production** (the `ScalarTreatment`/`EmitForm` override an applicable
//! annotation resolves to) lands in that option's own task, so no commit carries a `Mapping` variant
//! that is producible but unhandled (F5). This module establishes the framework, the validation
//! table, the classification rule, and the default-and-no-op path; today it produces no new variant
//! — an applicable-but-not-yet-produced annotation (a scaled float, a `NATIVE_CHECKED` 64-bit field,
//! a set on a repeated field) resolves to the §6/§7 default until its task turns on the override.

use crate::descriptor::model::{
    AnnotationValue, Enum, Field, FieldShape, MapKey, Openness, Scalar,
};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::policy::model::{EmitForm, ScalarTreatment};

/// Whether a scalar kind is a floating-point type — the target `(keryx.scale)`/`(keryx.opaque)`
/// apply to (§6).
fn is_float(scalar: Scalar) -> bool {
    matches!(scalar, Scalar::Float | Scalar::Double)
}

/// Whether a scalar kind is an integer type — the target `(keryx.numeric)` applies to (§6, §7.2).
fn is_integer(scalar: Scalar) -> bool {
    matches!(
        scalar,
        Scalar::Int32
            | Scalar::Sint32
            | Scalar::Sfixed32
            | Scalar::Uint32
            | Scalar::Fixed32
            | Scalar::Int64
            | Scalar::Uint64
            | Scalar::Fixed64
            | Scalar::Sfixed64
            | Scalar::Sint64
    )
}

/// A rejection at a field's proto path — a `MalformedOption` naming the option and the rule it
/// broke (never the adversary's option value; P1).
fn reject_field(field: &Field, detail: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::MalformedOption,
        Locus::at(field.path().as_str()),
        detail.to_owned(),
    )
}

/// A rejection at an enum's proto path.
fn reject_enum(enumeration: &Enum, detail: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::MalformedOption,
        Locus::at(enumeration.path().as_str()),
        detail.to_owned(),
    )
}

/// Validate `(keryx.numeric)` against a target integer kind, pushing any rejection. `target` is
/// `Some(scalar)` for the field or key kind the option applies to, `None` for a non-scalar value
/// (a message/enum field), where it can never apply. `NATIVE_CHECKED`/`DECIMAL_STRING` on a
/// non-integer are a mis-target; `CLINGCON` is refused up front (profile-gated, deferred); any other
/// value is malformed. An applicable value produces no override here — Task 6 turns on
/// `NativeChecked` and extends `DecimalString` to a 32-bit unsigned.
fn reject_numeric(
    field: &Field,
    target: Option<Scalar>,
    value: &AnnotationValue,
    rejections: &mut Vec<Diagnostic>,
) {
    match value {
        AnnotationValue::Enum(name) => match name.as_str() {
            "NATIVE_CHECKED" | "DECIMAL_STRING" => {
                if !target.is_some_and(is_integer) {
                    rejections.push(reject_field(
                        field,
                        "(keryx.numeric) applies only to an integer field or map key",
                    ));
                }
            }
            "CLINGCON" => rejections.push(reject_field(
                field,
                "(keryx.numeric) = CLINGCON requires the clingcon profile, which keryx does not yet support",
            )),
            // The enum's zero value is "no policy" — the §6 default; not a mis-application.
            "NUMERIC_POLICY_UNSPECIFIED" => {}
            _ => rejections.push(reject_field(
                field,
                "(keryx.numeric) takes NATIVE_CHECKED or DECIMAL_STRING",
            )),
        },
        _ => rejections.push(reject_field(
            field,
            "(keryx.numeric) takes NATIVE_CHECKED or DECIMAL_STRING",
        )),
    }
}

/// Validate a field's scalar-value options (`scale`/`opaque`/`numeric`) against its value kind,
/// pushing any rejection — the shared core of [`field_treatment`] (a scalar value, which also
/// produces the treatment) and [`reject_nonscalar_options`] (a message- or enum-valued field, which
/// produces none). `target` is `Some(scalar)` for a scalar value, `None` for a non-scalar one, where
/// none of the three can apply — each present one is a mis-target. `(keryx.numeric)` on a map targets
/// the key ([`key_treatment`]), not the value, so it is skipped here for a map field.
fn validate_value_options(field: &Field, target: Option<Scalar>, rejections: &mut Vec<Diagnostic>) {
    let is_map = matches!(field.shape(), FieldShape::Map { .. });
    for annotation in field.options() {
        match annotation.key.as_str() {
            "scale" => {
                if target.is_some_and(is_float) {
                    if !matches!(annotation.value, AnnotationValue::Int(_)) {
                        rejections.push(reject_field(
                            field,
                            "(keryx.scale) takes an integer exponent",
                        ));
                    }
                } else {
                    rejections.push(reject_field(
                        field,
                        "(keryx.scale) applies only to a float or double field",
                    ));
                }
            }
            "opaque" => {
                if target.is_some_and(is_float) {
                    if !matches!(annotation.value, AnnotationValue::Bool(true)) {
                        rejections.push(reject_field(field, "(keryx.opaque) takes the value true"));
                    }
                } else {
                    rejections.push(reject_field(
                        field,
                        "(keryx.opaque) applies only to a float or double field",
                    ));
                }
            }
            "numeric" if !is_map => {
                reject_numeric(field, target, &annotation.value, rejections);
            }
            _ => {}
        }
    }
}

/// The §6 scalar treatment of a field's value under its `(keryx.scale)`/`(keryx.opaque)`/
/// `(keryx.numeric)` annotations. Returns the §6 default plus any diagnostics; the positive
/// treatment overrides (`FixedPoint`, `OpaqueFloat`, `NativeChecked`, and `DecimalString` on a
/// 32-bit unsigned) extend this in Tasks 6/7. `(keryx.numeric)` on a **map** targets the key, not
/// the value ([`key_treatment`]), so it is not read here for a map field's value.
pub(super) fn field_treatment(
    field: &Field,
    scalar: Scalar,
) -> Result<ScalarTreatment, Diagnostics> {
    let default = super::names::scalar_treatment(scalar);
    let mut rejections = Vec::new();
    validate_value_options(field, Some(scalar), &mut rejections);
    Diagnostics::collect(rejections).map_or(Ok(default), Err)
}

/// Validate the scalar-value options on a **message- or enum-valued** field, where none of
/// `(keryx.scale)`/`(keryx.opaque)`/`(keryx.numeric)` can apply — each present one is a mis-target
/// diagnostic (the F4c rule, applied uniformly). Called for every non-scalar-valued field as
/// [`field_form`] is called for every field's `(keryx.set)`, so a mis-placed scalar-value option is
/// refused rather than silently dropped. Produces no treatment (the field has no scalar value).
pub(super) fn reject_nonscalar_options(field: &Field) -> Result<(), Diagnostics> {
    let mut rejections = Vec::new();
    validate_value_options(field, None, &mut rejections);
    Diagnostics::collect(rejections).map_or(Ok(()), Err)
}

/// The emit form of a field under `(keryx.set)`. Returns `default` (the §7 form) plus any
/// diagnostics; the positive `EmitForm::Set` production for a set-annotated repeated field lands in
/// the set vertical's last task (Slice 4).
pub(super) fn field_form(field: &Field, default: EmitForm) -> Result<EmitForm, Diagnostics> {
    let is_repeated = matches!(field.shape(), FieldShape::Repeated { .. });
    let mut rejections = Vec::new();
    for annotation in field.options() {
        if annotation.key == "set" {
            if !is_repeated {
                rejections.push(reject_field(
                    field,
                    "(keryx.set) applies only to a repeated field",
                ));
            } else if !matches!(annotation.value, AnnotationValue::Bool(true)) {
                rejections.push(reject_field(field, "(keryx.set) takes the value true"));
            }
        }
    }
    Diagnostics::collect(rejections).map_or(Ok(default), Err)
}

/// The §6 treatment of a map key under `(keryx.numeric)` (§7.2). Returns the key kind's §6 default
/// plus any diagnostics; the positive overrides extend this in Task 6.
pub(super) fn key_treatment(field: &Field, key: MapKey) -> Result<ScalarTreatment, Diagnostics> {
    let scalar = Scalar::from(key);
    let default = super::names::scalar_treatment(scalar);
    let mut rejections = Vec::new();
    for annotation in field.options() {
        if annotation.key == "numeric" {
            reject_numeric(field, Some(scalar), &annotation.value, &mut rejections);
        }
    }
    Diagnostics::collect(rejections).map_or(Ok(default), Err)
}

/// Whether an enum preserves unknown wire values under `(keryx.unknown) = PRESERVE` (§7.4). Returns
/// the resolved flag (default `false` — an unknown value is refused) plus any diagnostics; the flag
/// is consumed on `EnumMapping` in Task 8.
pub(super) fn enum_preserve(enumeration: &Enum) -> Result<bool, Diagnostics> {
    let mut preserve = false;
    let mut rejections = Vec::new();
    for annotation in enumeration.options() {
        if annotation.key == "unknown" {
            match &annotation.value {
                AnnotationValue::Enum(name) => match name.as_str() {
                    "PRESERVE" => {
                        if enumeration.openness() == Openness::Closed {
                            rejections.push(reject_enum(
                                enumeration,
                                "(keryx.unknown) = PRESERVE applies only to an open enum",
                            ));
                        } else {
                            preserve = true;
                        }
                    }
                    // The default and the enum's zero value both refuse an unknown value.
                    "REJECT" | "UNKNOWN_POLICY_UNSPECIFIED" => {}
                    _ => rejections.push(reject_enum(
                        enumeration,
                        "(keryx.unknown) takes PRESERVE or REJECT",
                    )),
                },
                _ => rejections.push(reject_enum(
                    enumeration,
                    "(keryx.unknown) takes PRESERVE or REJECT",
                )),
            }
        }
    }
    Diagnostics::collect(rejections).map_or(Ok(preserve), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::model::{Annotation, EnumValue, FqName, Presence, ValueType};

    fn ann(key: &str, value: AnnotationValue) -> Annotation {
        Annotation {
            key: key.to_owned(),
            value,
        }
    }

    fn field(shape: FieldShape, options: Vec<Annotation>) -> Field {
        Field {
            number: 1,
            name: "f".to_owned(),
            path: FqName::new("m.M.f"),
            shape,
            options,
            doc: None,
        }
    }

    fn singular(scalar: Scalar, options: Vec<Annotation>) -> Field {
        field(
            FieldShape::Singular {
                value: ValueType::Scalar(scalar),
                presence: Presence::Explicit,
            },
            options,
        )
    }

    fn repeated(scalar: Scalar, options: Vec<Annotation>) -> Field {
        field(
            FieldShape::Repeated {
                value: ValueType::Scalar(scalar),
            },
            options,
        )
    }

    fn map(key: MapKey, value: Scalar, options: Vec<Annotation>) -> Field {
        field(
            FieldShape::Map {
                key,
                value: ValueType::Scalar(value),
            },
            options,
        )
    }

    fn singular_message(options: Vec<Annotation>) -> Field {
        field(
            FieldShape::Singular {
                value: ValueType::Message(FqName::new("m.Foo")),
                presence: Presence::Explicit,
            },
            options,
        )
    }

    fn singular_enum(options: Vec<Annotation>) -> Field {
        field(
            FieldShape::Singular {
                value: ValueType::Enum(FqName::new("m.E")),
                presence: Presence::Explicit,
            },
            options,
        )
    }

    fn message_valued_map(options: Vec<Annotation>) -> Field {
        field(
            FieldShape::Map {
                key: MapKey::Int64,
                value: ValueType::Message(FqName::new("m.Foo")),
            },
            options,
        )
    }

    fn enumeration(openness: Openness, options: Vec<Annotation>) -> Enum {
        Enum {
            path: FqName::new("m.E"),
            file: "m.proto".to_owned(),
            outer: None,
            openness,
            values: vec![EnumValue {
                name: "E_UNSPECIFIED".to_owned(),
                number: 0,
                path: FqName::new("m.E.E_UNSPECIFIED"),
                options: Vec::new(),
                doc: None,
            }],
            options,
            doc: None,
        }
    }

    fn kind_at_locus(
        result: Result<impl std::fmt::Debug, Diagnostics>,
    ) -> (DiagnosticKind, String) {
        let diagnostics = result.expect_err("a rejection");
        assert_eq!(
            diagnostics.iter().len(),
            1,
            "one rejection: {diagnostics:?}"
        );
        let diagnostic = diagnostics.iter().next().expect("one");
        (
            diagnostic.kind(),
            diagnostic.locus().path().unwrap_or_default().to_owned(),
        )
    }

    // --- (keryx.scale) / (keryx.opaque): float-only (F4c inapplicable → mis-target) ---

    #[test]
    fn scale_on_a_non_float_field_is_a_mis_target() {
        let (kind, locus) = kind_at_locus(field_treatment(
            &singular(Scalar::Int32, vec![ann("scale", AnnotationValue::Int(2))]),
            Scalar::Int32,
        ));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
        assert_eq!(locus, "m.M.f");
    }

    #[test]
    fn opaque_on_bytes_is_a_mis_target() {
        let (kind, _) = kind_at_locus(field_treatment(
            &singular(
                Scalar::Bytes,
                vec![ann("opaque", AnnotationValue::Bool(true))],
            ),
            Scalar::Bytes,
        ));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    #[test]
    fn scale_on_a_float_admits_today_as_the_needs_annotation_default() {
        // Applicable (float) but not yet produced: Task 7 turns on `FixedPoint`; the §6 default
        // (`NeedsAnnotation`) stands here, and no diagnostic fires.
        let treatment = field_treatment(
            &singular(Scalar::Double, vec![ann("scale", AnnotationValue::Int(2))]),
            Scalar::Double,
        )
        .expect("scale on a double is applicable");
        assert_eq!(treatment, ScalarTreatment::NeedsAnnotation);
    }

    // --- (keryx.numeric): integer-only; no-op vs mis-target (F4c) ---

    #[test]
    fn native_checked_on_uint32_is_a_no_op_admit() {
        // Coincides with the native default → admit unchanged.
        let treatment = field_treatment(
            &singular(
                Scalar::Uint32,
                vec![ann(
                    "numeric",
                    AnnotationValue::Enum("NATIVE_CHECKED".to_owned()),
                )],
            ),
            Scalar::Uint32,
        )
        .expect("NATIVE_CHECKED on uint32 coincides with the default");
        assert_eq!(treatment, ScalarTreatment::Native);
    }

    #[test]
    fn decimal_string_on_a_64_bit_field_is_a_no_op_admit() {
        let treatment = field_treatment(
            &singular(
                Scalar::Int64,
                vec![ann(
                    "numeric",
                    AnnotationValue::Enum("DECIMAL_STRING".to_owned()),
                )],
            ),
            Scalar::Int64,
        )
        .expect("DECIMAL_STRING on int64 coincides with the default");
        assert_eq!(treatment, ScalarTreatment::DecimalString);
    }

    #[test]
    fn numeric_on_a_non_integer_field_is_a_mis_target() {
        let (kind, _) = kind_at_locus(field_treatment(
            &singular(
                Scalar::String,
                vec![ann(
                    "numeric",
                    AnnotationValue::Enum("NATIVE_CHECKED".to_owned()),
                )],
            ),
            Scalar::String,
        ));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    #[test]
    fn clingcon_is_refused_up_front_as_deferred() {
        let (kind, _) = kind_at_locus(field_treatment(
            &singular(
                Scalar::Int64,
                vec![ann("numeric", AnnotationValue::Enum("CLINGCON".to_owned()))],
            ),
            Scalar::Int64,
        ));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    #[test]
    fn numeric_on_a_map_targets_the_key_not_the_value() {
        // `(keryx.numeric)` on a `map<int64, string>` is the key's (§7.2), so the value treatment
        // (a string) does not read it — no mis-target on the value.
        let treatment = field_treatment(
            &map(
                MapKey::Int64,
                Scalar::String,
                vec![ann(
                    "numeric",
                    AnnotationValue::Enum("NATIVE_CHECKED".to_owned()),
                )],
            ),
            Scalar::String,
        )
        .expect("numeric on a map is the key's, not the value's");
        assert_eq!(treatment, ScalarTreatment::Text);
        // And the key reads it: NATIVE_CHECKED on an int64 key admits.
        key_treatment(
            &map(
                MapKey::Int64,
                Scalar::String,
                vec![ann(
                    "numeric",
                    AnnotationValue::Enum("NATIVE_CHECKED".to_owned()),
                )],
            ),
            MapKey::Int64,
        )
        .expect("NATIVE_CHECKED on an int64 key is applicable");
    }

    #[test]
    fn numeric_on_a_non_integer_map_key_is_a_mis_target() {
        let (kind, _) = kind_at_locus(key_treatment(
            &map(
                MapKey::String,
                Scalar::Int32,
                vec![ann(
                    "numeric",
                    AnnotationValue::Enum("NATIVE_CHECKED".to_owned()),
                )],
            ),
            MapKey::String,
        ));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    // --- scale/opaque/numeric on message/enum-valued fields: F4c applied uniformly (mis-target) ---

    #[test]
    fn scale_on_a_message_field_is_a_mis_target() {
        let (kind, locus) = kind_at_locus(reject_nonscalar_options(&singular_message(vec![ann(
            "scale",
            AnnotationValue::Int(2),
        )])));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
        assert_eq!(locus, "m.M.f");
    }

    #[test]
    fn opaque_on_a_message_field_is_a_mis_target() {
        let (kind, _) = kind_at_locus(reject_nonscalar_options(&singular_message(vec![ann(
            "opaque",
            AnnotationValue::Bool(true),
        )])));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    #[test]
    fn numeric_on_an_enum_field_is_a_mis_target() {
        let (kind, _) = kind_at_locus(reject_nonscalar_options(&singular_enum(vec![ann(
            "numeric",
            AnnotationValue::Enum("NATIVE_CHECKED".to_owned()),
        )])));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    #[test]
    fn numeric_on_a_message_valued_map_is_the_key_s_not_a_value_mis_target() {
        // On a `map<int64, Message>`, `(keryx.numeric)` targets the key (`key_treatment`); the
        // message value does not read it, so `reject_nonscalar_options` admits.
        reject_nonscalar_options(&message_valued_map(vec![ann(
            "numeric",
            AnnotationValue::Enum("NATIVE_CHECKED".to_owned()),
        )]))
        .expect("numeric on a map value is the key's, not a value mis-target");
    }

    #[test]
    fn scale_on_a_message_valued_map_is_a_mis_target() {
        // `(keryx.scale)` is not the key's; on a message-valued map it is a mis-target on the value.
        let (kind, _) = kind_at_locus(reject_nonscalar_options(&message_valued_map(vec![ann(
            "scale",
            AnnotationValue::Int(2),
        )])));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    #[test]
    fn a_message_field_carrying_no_scalar_value_option_is_admitted() {
        // `reject_nonscalar_options` handles only the scalar-value options; `(keryx.set)` on a
        // message field is `field_form`'s mis-target, not this pass's, so this admits.
        reject_nonscalar_options(&singular_message(vec![ann(
            "set",
            AnnotationValue::Bool(true),
        )]))
        .expect("set is not a scalar-value option");
    }

    // --- (keryx.set): repeated-only ---

    #[test]
    fn set_on_a_singular_field_is_a_mis_target() {
        let (kind, _) = kind_at_locus(field_form(
            &singular(
                Scalar::String,
                vec![ann("set", AnnotationValue::Bool(true))],
            ),
            EmitForm::Function,
        ));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    #[test]
    fn set_on_a_repeated_field_admits_today_as_the_sequence_default() {
        // Applicable (repeated) but not yet produced: Slice 4 turns on `EmitForm::Set`; the default
        // `Sequence` stands here.
        let form = field_form(
            &repeated(
                Scalar::String,
                vec![ann("set", AnnotationValue::Bool(true))],
            ),
            EmitForm::Sequence,
        )
        .expect("set on a repeated field is applicable");
        assert_eq!(form, EmitForm::Sequence);
    }

    #[test]
    fn set_with_a_non_true_value_is_malformed() {
        let (kind, _) = kind_at_locus(field_form(
            &repeated(
                Scalar::String,
                vec![ann("set", AnnotationValue::Bool(false))],
            ),
            EmitForm::Sequence,
        ));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    // --- (keryx.unknown) = PRESERVE: open-enum-only ---

    #[test]
    fn preserve_on_a_closed_enum_is_a_mis_target() {
        let (kind, locus) = kind_at_locus(enum_preserve(&enumeration(
            Openness::Closed,
            vec![ann("unknown", AnnotationValue::Enum("PRESERVE".to_owned()))],
        )));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
        assert_eq!(locus, "m.E");
    }

    #[test]
    fn preserve_on_an_open_enum_resolves_to_the_flag() {
        assert!(
            enum_preserve(&enumeration(
                Openness::Open,
                vec![ann("unknown", AnnotationValue::Enum("PRESERVE".to_owned()))],
            ))
            .expect("PRESERVE on an open enum is applicable")
        );
    }

    #[test]
    fn an_unknown_annotation_value_is_malformed() {
        let (kind, _) = kind_at_locus(enum_preserve(&enumeration(
            Openness::Open,
            vec![ann("unknown", AnnotationValue::Enum("BOGUS".to_owned()))],
        )));
        assert_eq!(kind, DiagnosticKind::MalformedOption);
    }

    // --- the un-annotated baseline: today's Mapping, unchanged ---

    #[test]
    fn an_un_annotated_field_resolves_to_its_section_6_default() {
        assert_eq!(
            field_treatment(&singular(Scalar::Int64, Vec::new()), Scalar::Int64)
                .expect("no annotation, no diagnostic"),
            ScalarTreatment::DecimalString
        );
        assert_eq!(
            field_form(&repeated(Scalar::String, Vec::new()), EmitForm::Sequence).expect("no set"),
            EmitForm::Sequence
        );
        assert!(!enum_preserve(&enumeration(Openness::Open, Vec::new())).expect("no unknown"));
    }
}
