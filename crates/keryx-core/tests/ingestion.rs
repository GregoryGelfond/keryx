//! The de-sugared schema model, asserted against the proto2/proto3/maps fixtures:
//! presence across the three labels, proto3-optional and map de-sugaring, real vs.
//! synthetic oneofs, closed vs. open enums, and determinism (P3).

use keryx_test_support as support;

use keryx_core::descriptor::model::{
    Enum, Field, FieldShape, FqName, Message, Openness, Presence, Scalar, ValueType,
};
use keryx_core::descriptor::{Schema, ingest};
use keryx_core::diagnostics::DiagnosticKind;

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
        .flat_map(|o| o.arms.iter().copied())
        .collect();
    assert!(
        !arm_numbers.contains(&3),
        "proto3-optional field #3 is not a oneof arm"
    );
    // the real oneof `source` carries arms #6 and #7.
    let source = reading
        .oneofs()
        .iter()
        .find(|o| o.name == "source")
        .expect("real oneof present");
    assert_eq!(source.arms, [6, 7]);
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
fn well_known_type_referents_translate_structurally() {
    // §10/§20: a well-known type a subject field references is an ordinary message and translates
    // structurally — it becomes a sort even though its file (google/protobuf/*) is not a subject. The
    // referent closure reaches it, so `Timestamp` is `google.protobuf.Timestamp` with its scalar
    // fields, the referencing field resolves to it, and Duration and the wrapper follow.
    let schema = schema("well_known.proto");

    let timestamp = message(&schema, "google.protobuf.Timestamp");
    assert!(matches!(
        field(timestamp, "seconds").shape(),
        FieldShape::Singular {
            value: ValueType::Scalar(Scalar::Int64),
            ..
        }
    ));
    let _ = field(timestamp, "nanos");
    // Duration and the Int32Value wrapper translate too.
    let _ = message(&schema, "google.protobuf.Duration");
    let _ = message(&schema, "google.protobuf.Int32Value");

    // The referencing field resolves to the well-known type, no longer dangling.
    let event = message(&schema, "keryx.wkt.Event");
    assert!(matches!(
        field(event, "at").shape(),
        FieldShape::Singular {
            value: ValueType::Message(referent),
            ..
        } if referent.as_str() == "google.protobuf.Timestamp"
    ));
}

#[test]
fn a_referenced_nested_dependency_enum_pulls_in_its_container() {
    // A subject field that references an enum *nested* inside a dependency file brings in not only that
    // enum but its lexical container, so the enum's `outer` names a declared element rather than
    // dangling. `google.protobuf.Field.Kind` is an enum nested in the message `google.protobuf.Field`
    // (google/protobuf/type.proto); referencing it makes both schema elements.
    let schema = schema("nested_dependency.proto");
    let kind = enumeration(&schema, "google.protobuf.Field.Kind");
    assert_eq!(
        kind.outer().map(FqName::as_str),
        Some("google.protobuf.Field")
    );
    // the container the nested enum names as its `outer` is itself an element — no dangling outer.
    let field_message = message(&schema, "google.protobuf.Field");
    assert!(field_message.outer().is_none());
}

#[test]
fn a_referenced_nested_dependency_message_pulls_in_its_container() {
    // The message-branch parent enqueue: a subject field that references a message *nested* inside a
    // dependency file brings in the nested message AND its container message.
    // `google.protobuf.SourceCodeInfo.Location` is nested in `google.protobuf.SourceCodeInfo`
    // (google/protobuf/descriptor.proto); referencing it makes both schema elements, so the nested
    // one's `outer` names a declared element.
    let schema = schema("nested_dependency.proto");
    let location = message(&schema, "google.protobuf.SourceCodeInfo.Location");
    assert_eq!(
        location.outer().map(FqName::as_str),
        Some("google.protobuf.SourceCodeInfo")
    );
    // the container is itself an element (top-level, no outer) — pulled in by the message branch.
    let container = message(&schema, "google.protobuf.SourceCodeInfo");
    assert!(container.outer().is_none());
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

#[test]
fn an_editions_descriptor_set_is_a_diagnostic_not_a_panic() {
    // The descriptor engine (prost-reflect 0.16.5) has no editions `Syntax` and *panics* building
    // a pool from an editions FileDescriptorSet; keryx detects editions before the engine and
    // refuses each editions file with an `UnsupportedEdition` diagnostic at that file's locus, so
    // ingestion stays total (§6). This test running to completion is itself the proof no panic
    // escapes.
    //
    // `fixtures/editions.binpb` is protoc-compiled from `fixtures/editions_probe.proto` (an
    // `edition = "2023"` file); regenerate it with:
    //   protoc --descriptor_set_out=fixtures/editions.binpb --include_imports --include_source_info \
    //          -I fixtures fixtures/editions_probe.proto
    let error = ingest(include_bytes!("fixtures/editions.binpb"))
        .expect_err("an editions set keryx cannot read is a diagnostic, not a panic");
    let diagnostic = error.iter().next().expect("one diagnostic");
    assert_eq!(diagnostic.kind(), DiagnosticKind::UnsupportedEdition);
    assert_eq!(
        diagnostic.locus().path(),
        Some("editions_probe.proto"),
        "the diagnostic names the offending file, not the whole input: {diagnostic}"
    );
    assert!(
        format!("{diagnostic}").contains("editions"),
        "the message names editions specifically: {diagnostic}"
    );
}

#[test]
fn every_editions_file_in_a_set_is_reported() {
    // Totality: `ingest` reports one `UnsupportedEdition` per editions file, at that file's locus —
    // not only the first. `fixtures/editions_multi.binpb` is protoc-compiled from
    // `editions_probe.proto` and `editions_probe2.proto` (both `edition = "2023"`); regenerate with:
    //   protoc --descriptor_set_out=fixtures/editions_multi.binpb --include_imports --include_source_info \
    //          -I fixtures fixtures/editions_probe.proto fixtures/editions_probe2.proto
    let error = ingest(include_bytes!("fixtures/editions_multi.binpb"))
        .expect_err("a two-editions set is a diagnostic, not a panic");
    assert_eq!(error.iter().count(), 2, "one diagnostic per editions file");
    let loci: Vec<Option<&str>> = error.iter().map(|d| d.locus().path()).collect();
    assert!(
        loci.contains(&Some("editions_probe.proto")),
        "names the first editions file: {loci:?}"
    );
    assert!(
        loci.contains(&Some("editions_probe2.proto")),
        "names the second editions file: {loci:?}"
    );
    for diagnostic in error.iter() {
        assert_eq!(diagnostic.kind(), DiagnosticKind::UnsupportedEdition);
    }
}
