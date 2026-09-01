//! Coverage for stage-1 base assignment (spec §21.3, §4.2, §5, §6, §7): `policy::map`
//! over the Increment-1 fixture corpus proves the un-collided vocabulary — sort and
//! field predicates, presence, emitted form and value treatment, view selection, and
//! the §7.4 enum-constant lowering, including its loud within-enum collision report.
//! Every fixture here is collision-free by construction; qualification is a later
//! pass's concern.

mod support;

use keryx_core::descriptor::{MapKey, Openness, Scalar, Schema, ingest};
use keryx_core::diagnostics::DiagnosticKind;
use keryx_core::policy::{
    self, EmitForm, EnumMapping, EnumValueMapping, FieldMapping, Mapping, ScalarTreatment,
    SortMapping, Totality, Unit, ValueMapping, ViewKind,
};

fn schema(fixture: &str) -> Schema {
    ingest(&support::compile_fixture(fixture)).expect("the fixture ingests")
}

fn mapping(fixture: &str) -> Mapping {
    policy::map(&schema(fixture)).expect("the fixture maps")
}

fn sort<'a>(mapping: &'a Mapping, proto: &str) -> &'a SortMapping {
    mapping
        .units()
        .iter()
        .flat_map(Unit::sorts)
        .find(|sort| sort.proto().as_str() == proto)
        .expect("sort present")
}

fn enumeration<'a>(mapping: &'a Mapping, proto: &str) -> &'a EnumMapping {
    mapping
        .units()
        .iter()
        .flat_map(Unit::enums)
        .find(|e| e.proto().as_str() == proto)
        .expect("enum present")
}

fn field<'a>(sort: &'a SortMapping, short_name: &str) -> &'a FieldMapping {
    sort.fields()
        .iter()
        .find(|field| field.proto().as_str().rsplit('.').next() == Some(short_name))
        .expect("field present")
}

fn enum_value<'a>(enumeration: &'a EnumMapping, proto_name: &str) -> &'a EnumValueMapping {
    enumeration
        .values()
        .iter()
        .find(|value| value.proto_name() == proto_name)
        .expect("enum value present")
}

#[test]
fn reading_sort_and_implicit_scalar_field() {
    let mapping = mapping("proto3.proto");
    let reading = sort(&mapping, "keryx.p3.Reading");
    assert_eq!(reading.predicate().as_str(), "reading");

    let sensor = field(reading, "sensor");
    assert_eq!(sensor.predicate().as_str(), "sensor");
    assert_eq!(sensor.arity(), 2);
    assert_eq!(sensor.form(), &EmitForm::Function);
    assert_eq!(sensor.presence(), Totality::Total);
    assert_eq!(sensor.view(), None);
}

#[test]
fn reading_message_field_is_partial_with_a_singular_view() {
    let mapping = mapping("proto3.proto");
    let reading = sort(&mapping, "keryx.p3.Reading");

    let detail = field(reading, "detail");
    assert_eq!(detail.predicate().as_str(), "detail");
    assert_eq!(detail.arity(), 2);
    assert_eq!(detail.form(), &EmitForm::Function);
    assert_eq!(detail.presence(), Totality::Partial);
    assert_eq!(detail.view(), Some(ViewKind::Singular));
    match detail.value() {
        ValueMapping::Message(referent) => assert_eq!(referent.as_str(), "detail"),
        ValueMapping::Scalar { .. } | ValueMapping::Enum(_) => {
            panic!("expected `detail` to carry a message value")
        }
    }
}

#[test]
fn reading_oneof_arms_are_partial_oneof_arm_functions() {
    let mapping = mapping("proto3.proto");
    let reading = sort(&mapping, "keryx.p3.Reading");

    for arm in ["device", "gateway"] {
        let arm_field = field(reading, arm);
        assert_eq!(
            arm_field.form(),
            &EmitForm::OneofArm {
                oneof: "source".to_owned()
            }
        );
        assert_eq!(arm_field.presence(), Totality::Partial);
    }
}

#[test]
fn maps_scalar_value_has_no_view() {
    let mapping = mapping("maps.proto");
    let inventory = sort(&mapping, "keryx.maps.Inventory");

    let counts = field(inventory, "counts");
    assert_eq!(counts.predicate().as_str(), "counts");
    assert_eq!(counts.arity(), 3);
    assert_eq!(
        counts.form(),
        &EmitForm::Map {
            key: MapKey::String
        }
    );
    assert_eq!(
        counts.value(),
        &ValueMapping::Scalar {
            kind: Scalar::Int32,
            treatment: ScalarTreatment::Native
        }
    );
    assert_eq!(counts.view(), None);
}

#[test]
fn maps_message_value_gets_a_map_view() {
    let mapping = mapping("maps.proto");
    let inventory = sort(&mapping, "keryx.maps.Inventory");

    let items = field(inventory, "items");
    assert_eq!(items.predicate().as_str(), "items");
    assert_eq!(items.arity(), 3);
    assert_eq!(items.form(), &EmitForm::Map { key: MapKey::Int64 });
    assert_eq!(items.view(), Some(ViewKind::Map));
    match items.value() {
        ValueMapping::Message(referent) => assert_eq!(referent.as_str(), "item"),
        ValueMapping::Scalar { .. } | ValueMapping::Enum(_) => {
            panic!("expected `items` to carry a message value")
        }
    }
}

#[test]
fn proto2_presence_across_the_three_labels() {
    let mapping = mapping("proto2.proto");
    let order = sort(&mapping, "keryx.p2.Order");

    assert_eq!(field(order, "id").presence(), Totality::Partial);
    assert_eq!(field(order, "quantity").presence(), Totality::Partial);

    let tags = field(order, "tags");
    assert_eq!(tags.predicate().as_str(), "tags");
    assert_eq!(tags.arity(), 3);
    assert_eq!(tags.form(), &EmitForm::Sequence);
    assert_eq!(tags.presence(), Totality::Total);
}

#[test]
fn level_enum_lowering_strips_the_shared_prefix() {
    let mapping = mapping("proto3.proto");
    let level = enumeration(&mapping, "keryx.p3.Level");
    assert_eq!(level.predicate().as_str(), "level");
    assert_eq!(level.openness(), Openness::Open);

    assert_eq!(
        enum_value(level, "LEVEL_UNSPECIFIED").constant().as_str(),
        "unspecified"
    );
    assert_eq!(enum_value(level, "LEVEL_LOW").constant().as_str(), "low");
    assert_eq!(enum_value(level, "LEVEL_HIGH").constant().as_str(), "high");
}

#[test]
fn proto2_enum_is_closed() {
    let mapping = mapping("proto2.proto");
    let grade = enumeration(&mapping, "keryx.p2.Grade");
    assert_eq!(grade.openness(), Openness::Closed);
}

#[test]
fn recursive_sorts_are_flagged_and_others_are_not() {
    let recursive = mapping("recursion.proto");
    assert!(sort(&recursive, "keryx.rec.Tree").is_recursive());
    assert!(sort(&recursive, "keryx.rec.A").is_recursive());
    assert!(sort(&recursive, "keryx.rec.B").is_recursive());

    // A separate, non-recursive fixture is not flagged (guards against a mutation that
    // hardcodes `recursive` to always-true).
    let acyclic = mapping("maps.proto");
    assert!(!sort(&acyclic, "keryx.maps.Item").is_recursive());
}

#[test]
fn docs_carry_through_to_the_mapping() {
    let mapping = mapping("docs.proto");

    let note = sort(&mapping, "keryx.docs.Note");
    assert_eq!(note.doc(), Some("A leading comment on the message."));
    assert_eq!(
        field(note, "text").doc(),
        Some("A leading comment on the field.")
    );

    let status = enumeration(&mapping, "keryx.docs.Status");
    assert_eq!(status.doc(), Some("A leading comment on the enum."));
    assert_eq!(
        enum_value(status, "STATUS_ACTIVE").doc(),
        Some("A leading comment on the enum value.")
    );
}

#[test]
fn scalar_treatment_covers_every_class() {
    let mapping = mapping("scalar_treatment.proto");
    let sample = sort(&mapping, "keryx.scalars.Sample");

    // At least one field per §6 treatment family (two for DecimalString), so a mutation
    // in any single match arm of `scalar_treatment` (e.g. `Bool` mapping to `Native`, or
    // `Bytes` to `Text`) fails here instead of passing unnoticed.
    let cases = [
        ("count", Scalar::Int32, ScalarTreatment::Native),
        ("total", Scalar::Int64, ScalarTreatment::DecimalString),
        ("checksum", Scalar::Uint64, ScalarTreatment::DecimalString),
        ("ratio", Scalar::Float, ScalarTreatment::NeedsAnnotation),
        ("active", Scalar::Bool, ScalarTreatment::Bool),
        ("payload", Scalar::Bytes, ScalarTreatment::HexString),
        ("label", Scalar::String, ScalarTreatment::Text),
    ];
    for (name, kind, treatment) in cases {
        assert_eq!(
            field(sample, name).value(),
            &ValueMapping::Scalar { kind, treatment },
            "field `{name}`"
        );
    }
}

#[test]
fn singular_enum_field_has_no_view() {
    let mapping = mapping("scalar_treatment.proto");
    let sample = sort(&mapping, "keryx.scalars.Sample");

    let kind = field(sample, "kind");
    assert_eq!(kind.arity(), 2);
    assert_eq!(kind.form(), &EmitForm::Function);
    assert_eq!(kind.presence(), Totality::Total);
    assert_eq!(kind.view(), None);
    match kind.value() {
        ValueMapping::Enum(referent) => assert_eq!(referent.as_str(), "kind"),
        ValueMapping::Scalar { .. } | ValueMapping::Message(_) => {
            panic!("expected `kind` to carry an enum value")
        }
    }
}

#[test]
fn repeated_message_field_gets_a_sequence_view() {
    let mapping = mapping("scalar_treatment.proto");
    let sample = sort(&mapping, "keryx.scalars.Sample");

    let notes = field(sample, "notes");
    assert_eq!(notes.arity(), 3);
    assert_eq!(notes.form(), &EmitForm::Sequence);
    assert_eq!(notes.presence(), Totality::Total);
    assert_eq!(notes.view(), Some(ViewKind::Sequence));
    match notes.value() {
        ValueMapping::Message(referent) => assert_eq!(referent.as_str(), "note"),
        ValueMapping::Scalar { .. } | ValueMapping::Enum(_) => {
            panic!("expected `notes` to carry a message value")
        }
    }
}

#[test]
fn repeated_and_mapped_enum_values_have_no_view() {
    let mapping = mapping("scalar_treatment.proto");
    let sample = sort(&mapping, "keryx.scalars.Sample");

    let kinds = field(sample, "kinds");
    assert_eq!(kinds.form(), &EmitForm::Sequence);
    assert_eq!(kinds.view(), None);
    match kinds.value() {
        ValueMapping::Enum(referent) => assert_eq!(referent.as_str(), "kind"),
        ValueMapping::Scalar { .. } | ValueMapping::Message(_) => {
            panic!("expected `kinds` to carry an enum value")
        }
    }

    let tags = field(sample, "tags");
    assert_eq!(
        tags.form(),
        &EmitForm::Map {
            key: MapKey::String
        }
    );
    assert_eq!(tags.view(), None);
    match tags.value() {
        ValueMapping::Enum(referent) => assert_eq!(referent.as_str(), "kind"),
        ValueMapping::Scalar { .. } | ValueMapping::Message(_) => {
            panic!("expected `tags` to carry an enum value")
        }
    }
}

#[test]
fn within_enum_constant_collision_is_reported_loud_not_deduplicated() {
    let schema = schema("enum_collision.proto");
    let error = policy::map(&schema).expect_err("a within-enum constant collision is an error");

    assert_eq!(error.len(), 1);
    let diagnostic = error.iter().next().expect("one diagnostic");
    assert_eq!(diagnostic.kind(), DiagnosticKind::AmbiguousConstant);
    assert_eq!(diagnostic.locus().as_str(), "keryx.enumcoll.Mixed");
}

#[test]
fn map_is_pure_and_deterministically_ordered() {
    let schema = schema("proto3.proto");
    let first = policy::map(&schema).expect("the fixture maps");
    let second = policy::map(&schema).expect("the fixture maps");
    assert_eq!(first, second);

    assert_eq!(first.units().len(), 1);
    let unit = &first.units()[0];
    assert_eq!(unit.package(), "keryx.p3");

    let sort_paths: Vec<&str> = unit.sorts().iter().map(|s| s.proto().as_str()).collect();
    assert_eq!(sort_paths, vec!["keryx.p3.Detail", "keryx.p3.Reading"]);

    let reading = sort(&first, "keryx.p3.Reading");
    let numbers: Vec<i32> = reading.fields().iter().map(FieldMapping::number).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5, 6, 7]);
}
