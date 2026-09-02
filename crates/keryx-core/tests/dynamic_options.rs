//! The §20 rule, proven: keryx's annotations — custom options a typed prost
//! struct would silently drop — survive ingestion as `opt`-bearing annotations,
//! read by extension identity through the dynamic layer.

use keryx_test_support as support;

use keryx_core::descriptor::ingest;
use keryx_core::descriptor::model::{Annotation, AnnotationValue};

fn annotations<'a>(list: &'a [Annotation], key: &str) -> Vec<&'a AnnotationValue> {
    list.iter()
        .filter(|a| a.key == key)
        .map(|a| &a.value)
        .collect()
}

#[test]
fn keryx_options_survive_ingestion() {
    let schema = ingest(&support::compile_fixture("options.proto")).expect("ingests");
    let sample = schema
        .messages()
        .iter()
        .find(|m| m.path().as_str() == "keryx.opt.Sample")
        .unwrap();

    // message options: a bool and a repeated string expanded to two annotations.
    assert_eq!(
        annotations(sample.options(), "value"),
        vec![&AnnotationValue::Bool(true)]
    );
    assert_eq!(
        annotations(sample.options(), "any_types"),
        vec![
            &AnnotationValue::Text("a.B".into()),
            &AnnotationValue::Text("c.D".into())
        ]
    );

    // field options across every value kind.
    let field = |name: &str| sample.fields().iter().find(|f| f.name() == name).unwrap();
    assert_eq!(
        annotations(field("tags").options(), "set"),
        vec![&AnnotationValue::Bool(true)]
    );
    assert_eq!(
        annotations(field("tick").options(), "numeric"),
        vec![&AnnotationValue::Enum("NATIVE_CHECKED".into())]
    );
    assert_eq!(
        annotations(field("ratio").options(), "scale"),
        vec![&AnnotationValue::Int(3)]
    );
    assert_eq!(
        annotations(field("priority").options(), "default"),
        vec![&AnnotationValue::Text("3".into())]
    );

    // enum-target option.
    let signal = schema
        .enums()
        .iter()
        .find(|e| e.path().as_str() == "keryx.opt.Signal")
        .unwrap();
    assert_eq!(
        annotations(signal.options(), "zero"),
        vec![&AnnotationValue::Enum("ABSENT".into())]
    );
}
