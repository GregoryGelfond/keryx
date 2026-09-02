//! The de-sugared schema model, asserted against the proto2/proto3/maps fixtures:
//! presence across the three labels, proto3-optional and map de-sugaring, real vs.
//! synthetic oneofs, closed vs. open enums, and determinism (P3).

use keryx_test_support as support;

use keryx_core::descriptor::model::{
    Enum, Field, FieldShape, FqName, Message, Openness, Presence, Scalar, ValueType,
};
use keryx_core::descriptor::{Schema, ingest};

fn schema(fixture: &str) -> Schema {
    ingest(&support::compile_fixture(fixture)).expect("the fixture ingests")
}

fn message<'a>(schema: &'a Schema, path: &str) -> &'a Message {
    schema
        .messages()
        .iter()
        .find(|m| m.path().as_str() == path)
        .expect("message present")
}

fn field<'a>(message: &'a Message, name: &str) -> &'a Field {
    message
        .fields()
        .iter()
        .find(|f| f.name() == name)
        .expect("field present")
}

fn enumeration<'a>(schema: &'a Schema, path: &str) -> &'a Enum {
    schema
        .enums()
        .iter()
        .find(|e| e.path().as_str() == path)
        .expect("enum present")
}

#[test]
fn proto2_presence_and_closed_enum() {
    let schema = schema("proto2.proto");
    let order = message(&schema, "keryx.p2.Order");
    assert!(matches!(
        field(order, "id").shape(),
        FieldShape::Singular {
            presence: Presence::LegacyRequired,
            value: ValueType::Scalar(Scalar::String)
        }
    ));
    assert!(matches!(
        field(order, "quantity").shape(),
        FieldShape::Singular {
            presence: Presence::Explicit,
            value: ValueType::Scalar(Scalar::Int32)
        }
    ));
    assert!(matches!(
        field(order, "tags").shape(),
        FieldShape::Repeated {
            value: ValueType::Scalar(Scalar::String)
        }
    ));
    assert_eq!(
        enumeration(&schema, "keryx.p2.Grade").openness(),
        Openness::Closed
    );
}

#[test]
fn proto3_optional_desugars_and_oneof_is_real() {
    let schema = schema("proto3.proto");
    let reading = message(&schema, "keryx.p3.Reading");
    // proto3 implicit scalar.
    assert!(matches!(
        field(reading, "sensor").shape(),
        FieldShape::Singular {
            presence: Presence::Implicit,
            ..
        }
    ));
    // proto3-optional: EXPLICIT singular, and NOT recorded under any oneof.
    assert!(matches!(
        field(reading, "calibration").shape(),
        FieldShape::Singular {
            presence: Presence::Explicit,
            ..
        }
    ));
    let arm_numbers: Vec<i32> = reading
        .oneofs()
        .iter()
        .flat_map(|o| o.arms().iter().copied())
        .collect();
    assert!(
        !arm_numbers.contains(&3),
        "proto3-optional field #3 is not a oneof arm"
    );
    // the real oneof `source` carries arms #6 and #7.
    let source = reading
        .oneofs()
        .iter()
        .find(|o| o.name() == "source")
        .expect("real oneof present");
    assert_eq!(source.arms(), &[6, 7]);
    // message field -> EXPLICIT; enum field -> IMPLICIT; open enum.
    assert!(matches!(
        field(reading, "detail").shape(),
        FieldShape::Singular {
            presence: Presence::Explicit,
            value: ValueType::Message(_)
        }
    ));
    assert!(matches!(
        field(reading, "level").shape(),
        FieldShape::Singular {
            presence: Presence::Implicit,
            value: ValueType::Enum(_)
        }
    ));
    assert_eq!(
        enumeration(&schema, "keryx.p3.Level").openness(),
        Openness::Open
    );
}

#[test]
fn maps_desugar_and_hide_entry_messages() {
    let schema = schema("maps.proto");
    let inventory = message(&schema, "keryx.maps.Inventory");
    assert!(matches!(
        field(inventory, "counts").shape(),
        FieldShape::Map {
            key: keryx_core::descriptor::model::MapKey::String,
            value: ValueType::Scalar(Scalar::Int32)
        }
    ));
    match field(inventory, "items").shape() {
        FieldShape::Map { key, value } => {
            assert_eq!(*key, keryx_core::descriptor::model::MapKey::Int64);
            assert!(
                matches!(value, ValueType::Message(name) if name.as_str() == "keryx.maps.Item")
            );
        }
        other => panic!("items is a map, got {other:?}"),
    }
    // the synthetic *Entry messages are not schema subjects.
    assert!(
        schema
            .messages()
            .iter()
            .all(|m| !m.path().as_str().ends_with("Entry"))
    );
}

#[test]
fn nested_types_carry_their_outer() {
    let schema = schema("nested.proto");
    let inner = message(&schema, "keryx.nest.Outer.Inner");
    assert_eq!(inner.outer().map(FqName::as_str), Some("keryx.nest.Outer"));
    let kind = enumeration(&schema, "keryx.nest.Outer.Kind");
    assert_eq!(kind.outer().map(FqName::as_str), Some("keryx.nest.Outer"));
    // the top-level Outer has no outer.
    assert!(message(&schema, "keryx.nest.Outer").outer().is_none());
}

#[test]
fn ingestion_is_deterministic() {
    let bytes = support::compile_fixture("proto3.proto");
    assert_eq!(ingest(&bytes).unwrap(), ingest(&bytes).unwrap());
    // messages are path-ordered.
    let schema = ingest(&bytes).unwrap();
    let paths: Vec<&str> = schema
        .messages()
        .iter()
        .map(|m| m.path().as_str())
        .collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted);
}
