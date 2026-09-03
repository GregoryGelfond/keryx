//! Totality of the descriptor door (§6; the threat model's totality property): `ingest` returns a
//! value or a typed `Diagnostics` over *any* input — it never panics, aborts, or hangs. Two
//! generators, as the model records: **arbitrary bytes**, which overwhelmingly fail at the decoder;
//! and **valid encodings of structurally-invalid descriptors** — a proptest strategy over
//! `FileDescriptorProto` with adversarial packages/names, nested types, and uninterpreted options —
//! which is the one that reaches keryx's own pre-emption and walk (a structured generator finds the
//! F-class shapes — an arbitrary package, an uninterpreted option — that arbitrary bytes never do).
//! The hand-built sets in the `descriptor` module carry the specific refusals a golden asserts.

use proptest::prelude::*;
use prost::Message as _;
use prost_types::uninterpreted_option::NamePart;
use prost_types::{
    DescriptorProto, EnumDescriptorProto, EnumValueDescriptorProto, FieldDescriptorProto,
    FileDescriptorProto, FileDescriptorSet, MessageOptions, UninterpretedOption,
};

/// An adversarial name/package alphabet: valid proto identifiers, plus the shapes the door refuses —
/// a leading dot, a path separator, a quote, an empty string — so the generator reaches both the
/// admitted and the pre-empted branches.
fn adversarial_name() -> impl Strategy<Value = String> {
    prop_oneof![
        "[A-Za-z_][A-Za-z0-9_]{0,8}",
        "[A-Za-z_][A-Za-z0-9_]{0,4}(\\.[A-Za-z_][A-Za-z0-9_]{0,4}){0,3}",
        "\\.[A-Za-z]{0,4}",
        "[./\"a-z ]{0,8}",
        Just(String::new()),
    ]
}

fn field_strategy() -> impl Strategy<Value = FieldDescriptorProto> {
    // The number range spans negatives too, so the generator exercises `field_number`'s out-of-`i32`
    // branch (prost-reflect reads a field number as `u32`, so a negative one is out of range).
    (adversarial_name(), -5i32..2000, 0i32..4, 0i32..19).prop_map(|(name, number, label, ty)| {
        FieldDescriptorProto {
            name: Some(name),
            number: Some(number),
            label: Some(label),
            r#type: Some(ty),
            ..Default::default()
        }
    })
}

/// A `MessageOptions` carrying an uninterpreted option (which the door refuses) — a shape a structured
/// generator reaches and arbitrary bytes do not.
fn uninterpreted_options() -> MessageOptions {
    MessageOptions {
        uninterpreted_option: vec![UninterpretedOption {
            name: vec![NamePart {
                name_part: "x".to_owned(),
                is_extension: true,
            }],
            aggregate_value: Some("f { g < h > }".to_owned()),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn message_strategy() -> impl Strategy<Value = DescriptorProto> {
    let leaf = (
        adversarial_name(),
        prop::collection::vec(field_strategy(), 0..3),
        any::<bool>(),
    )
        .prop_map(|(name, field, uninterpreted)| DescriptorProto {
            name: Some(name),
            field,
            options: uninterpreted.then(uninterpreted_options),
            ..Default::default()
        });
    // Bounded nesting — shallow, so no input approaches the abort axes the door pre-empts; the point
    // is structural variety (nested names, nested options), not depth (the guards' own tests cover that).
    leaf.prop_recursive(3, 24, 2, |inner| {
        (
            adversarial_name(),
            prop::collection::vec(field_strategy(), 0..2),
            prop::collection::vec(inner, 0..2),
        )
            .prop_map(|(name, field, nested_type)| DescriptorProto {
                name: Some(name),
                field,
                nested_type,
                ..Default::default()
            })
    })
}

fn enum_strategy() -> impl Strategy<Value = EnumDescriptorProto> {
    (
        adversarial_name(),
        prop::collection::vec((adversarial_name(), any::<i32>()), 0..4),
    )
        .prop_map(|(name, values)| EnumDescriptorProto {
            name: Some(name),
            value: values
                .into_iter()
                .map(|(name, number)| EnumValueDescriptorProto {
                    name: Some(name),
                    number: Some(number),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })
}

fn file_strategy() -> impl Strategy<Value = FileDescriptorProto> {
    (
        adversarial_name(),
        prop::sample::select(vec!["proto2", "proto3", "editions", "proto4", ""]),
        prop::collection::vec(message_strategy(), 0..3),
        prop::collection::vec(enum_strategy(), 0..2),
    )
        .prop_map(
            |(package, syntax, message_type, enum_type)| FileDescriptorProto {
                name: Some("gen.proto".to_owned()),
                package: Some(package),
                syntax: Some(syntax.to_owned()),
                message_type,
                enum_type,
                ..Default::default()
            },
        )
}

proptest! {
    #[test]
    fn ingest_is_total_over_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        // A value or `Diagnostics`, never a panic — this exercises the decoder's totality across the
        // whole byte space (arbitrary bytes overwhelmingly fail at `DescriptorPool::decode`, which is
        // exactly why the second generator carries the refusals keryx's own logic determines).
        let _ = keryx_core::descriptor::ingest(&bytes);
    }

    #[test]
    fn ingest_is_total_over_structured_descriptors(file in file_strategy()) {
        // The second generator: valid *encodings* of possibly-structurally-invalid descriptors — an
        // adversarial package or name, nested types, an uninterpreted option — the shapes that reach
        // keryx's own pre-emption and walk, which arbitrary bytes never form. `ingest` returns a value
        // or `Diagnostics`, never a panic, abort, or hang.
        let bytes = FileDescriptorSet { file: vec![file] }.encode_to_vec();
        let _ = keryx_core::descriptor::ingest(&bytes);
    }
}
