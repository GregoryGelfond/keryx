//! The config example (§11–13, §28): the computed round trip, documentation by example and a
//! regression suite in one. keryx shreds a `Deployment` to facts; the consuming tool's `model.lp`
//! derives a `Report`; keryx reassembles that `Report` — a *different* message than went in. keryx
//! wires the two ends; the solve between them is the consumer's clingo (see `scripts/ground.sh`,
//! test infrastructure — keryx spawns no solver). Here: regenerate the vocabulary and the shred,
//! and reassemble the committed answer set to the committed `Report`, byte for byte.

use std::path::{Path, PathBuf};

use keryx_core::codec::{Codec, PayloadFormat, Root, raise_answer_set};
use keryx_core::descriptor::compile;
use keryx_core::emit::Shape;
use keryx_core::{emit, manifest, policy};

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/config")
}

fn gen_golden(name: &str) -> String {
    std::fs::read_to_string(example().join("gen").join(name)).expect("golden present")
}

#[test]
fn config_gen_matches_the_committed_example() {
    let example = example();
    let schema = compile(
        &[example.join("deployment.proto")],
        std::slice::from_ref(&example),
    )
    .expect("deployment compiles");
    let mapping = policy::map(&schema).expect("maps");
    let unit = mapping.units().first().expect("deploy.v1 unit");
    assert_eq!(
        emit::core(unit).expect("core"),
        gen_golden("deploy.v1.core.lp")
    );
    assert_eq!(
        emit::views(unit).expect("views"),
        gen_golden("deploy.v1.views.lp")
    );
    assert_eq!(
        emit::emit_strict(unit).expect("emit"),
        gen_golden("deploy.v1.emit.lp")
    );
    assert_eq!(
        manifest::write(unit, "-", Shape::Strict),
        gen_golden("deploy.v1.keryx-manifest")
    );
}

#[test]
fn config_facts_match_the_committed_bad_config() {
    let example = example();
    let payload = std::fs::read(example.join("config.bad.txtpb")).expect("payload present");
    let codec = Codec::from_source(
        &[example.join("deployment.proto")],
        std::slice::from_ref(&example),
    )
    .expect("the config example builds a codec");
    let facts = codec
        .shred(
            "deploy.v1.Deployment",
            &payload,
            PayloadFormat::Textproto,
            &Root::fresh(0),
        )
        .expect("the config shreds");
    assert_eq!(
        facts.render().expect("the facts render"),
        std::fs::read_to_string(example.join("facts.bad.lp")).expect("facts golden present")
    );
}

#[test]
fn config_facts_match_the_committed_ok_config() {
    let example = example();
    let payload = std::fs::read(example.join("config.ok.txtpb")).expect("payload present");
    let codec = Codec::from_source(
        &[example.join("deployment.proto")],
        std::slice::from_ref(&example),
    )
    .expect("the config example builds a codec");
    let facts = codec
        .shred(
            "deploy.v1.Deployment",
            &payload,
            PayloadFormat::Textproto,
            &Root::fresh(0),
        )
        .expect("the valid config shreds");
    assert_eq!(
        facts.render().expect("the facts render"),
        std::fs::read_to_string(example.join("facts.ok.lp")).expect("facts golden present")
    );
}

#[test]
fn config_reassembles_the_computed_report() {
    // The consuming tool's clingo turned the Deployment facts + model.lp into this Report; keryx
    // reassembles it — a *different* message than the Deployment that went in — byte for byte.
    let example = example();
    let codec = Codec::from_source(
        &[example.join("deployment.proto")],
        std::slice::from_ref(&example),
    )
    .expect("the config example builds a codec");
    let answer =
        std::fs::read_to_string(example.join("answer.bad.lp")).expect("answer set present");
    let symbols = raise_answer_set(&answer).expect("the answer set reads");
    let reassembled = codec
        .reassemble(&symbols, PayloadFormat::Binary)
        .expect("the answer set reassembles");
    let messages = reassembled.messages();
    assert_eq!(messages.len(), 1, "one root marker, one message");
    assert_eq!(messages[0].type_name(), "deploy.v1.Report");
    let golden = std::fs::read(example.join("report.bad.binpb")).expect("golden present");
    assert_eq!(
        messages[0].bytes(),
        golden,
        "reassembly matches the committed Report"
    );

    // The textproto and JSON renderings the README quotes verbatim, pinned the same way.
    let txtpb = codec
        .reassemble(&symbols, PayloadFormat::Textproto)
        .expect("reassembles to textproto");
    assert_eq!(
        txtpb.messages()[0].bytes(),
        std::fs::read(example.join("report.bad.txtpb")).expect("txtpb golden present"),
        "textproto rendering matches the committed golden"
    );
    let json = codec
        .reassemble(&symbols, PayloadFormat::Json)
        .expect("reassembles to json");
    assert_eq!(
        json.messages()[0].bytes(),
        std::fs::read(example.join("report.bad.json")).expect("json golden present"),
        "json rendering matches the committed golden"
    );
}
