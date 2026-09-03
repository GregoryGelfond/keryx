//! Self-application (spec §21.2): descriptor.proto describes itself, so the tool's input
//! language runs through the tool. The gen increment proves the dogfood — keryx ingests and
//! gens the schema-of-schemas totally, and its hand-written stage-0 facts are golden-pinned. The
//! deeper equivalence (stage-0 definable as a view over keryx(descriptor.proto)) is deferred to a
//! future ASP-contract checker over themelios.

use std::path::{Path, PathBuf};

use keryx_core::descriptor::compile;
use keryx_core::{emit, facts, policy};

fn includes() -> Vec<PathBuf> {
    // Any include dir; the bundled GoogleFileResolver resolves google/protobuf/*.
    vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")]
}

#[test]
fn keryx_ingests_and_gens_descriptor_proto_totally() {
    let schema = compile(&["google/protobuf/descriptor.proto"], &includes())
        .expect("keryx ingests the schema-of-schemas");
    // Non-vacuous (the crux): descriptor.proto is the *subject*, not skipped as a well-known
    // dependency, so it has messages — a regression that re-skipped it would fail here, never
    // pass green.
    assert!(
        schema
            .messages()
            .iter()
            .any(|m| m.path().as_str() == "google.protobuf.FieldDescriptorProto"),
        "descriptor.proto is ingested as a subject, not skipped as a dependency"
    );
    // gen is total on it: at least one unit, and policy + emit succeed for every unit.
    let mapping = policy::map(&schema).expect("descriptor.proto maps");
    assert!(
        !mapping.units().is_empty(),
        "the schema-of-schemas yields a generation unit"
    );
    for unit in mapping.units() {
        emit::core(unit).expect("core emits");
        emit::views(unit).expect("views emit");
    }
}

#[test]
fn descriptor_proto_stage0_facts_are_pinned() {
    let schema = compile(&["google/protobuf/descriptor.proto"], &includes()).expect("ingests");
    let facts = facts::render(&schema).expect("renders");
    // Non-vacuous: an empty golden (the pre-fix vacuous case) could never contain this.
    assert!(
        facts.contains(r#"message("google.protobuf.FieldDescriptorProto""#),
        "the stage-0 facts describe descriptor.proto's own messages"
    );
    assert_eq!(facts, include_str!("golden/descriptor.facts.lp"));
}
