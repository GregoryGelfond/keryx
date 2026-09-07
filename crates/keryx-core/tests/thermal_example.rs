//! The thermal example is documentation by example and a regression suite in one (spec §27):
//! regenerate its `gen/` artifacts — the vocabulary from the schema, the facts from the committed
//! payload — and assert they equal the committed `examples/thermal/gen/*`. A drift here is a real
//! change to the generated vocabulary or to the shred, intended or a regression.

use std::path::{Path, PathBuf};

use keryx_test_support::wire::{batch, reading};

use keryx_core::codec::{Codec, PayloadFormat, Root, raise_answer_set};
use keryx_core::descriptor::compile;
use keryx_core::emit::Shape;
use keryx_core::{emit, manifest, policy};

/// The example's directory (`examples/thermal`).
fn example() -> PathBuf {
    // keryx-core/ -> crates/ -> repo root.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal")
}

/// keryx-core's vendored `proto/`, for the `keryx/options.proto` the schema imports.
fn vendored() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("proto")
}

/// The committed text of the `gen/` artifact `name`.
fn golden(name: &str) -> String {
    std::fs::read_to_string(example().join("gen").join(name)).expect("golden present")
}

#[test]
fn thermal_gen_matches_the_committed_example() {
    let example = example();
    let schema = compile(
        &[example.join("thermal.proto")],
        &[example.clone(), vendored()],
    )
    .expect("thermal compiles");
    let mapping = policy::map(&schema).expect("maps");
    let unit = mapping.units().first().expect("thermal.v1 unit");
    assert_eq!(
        emit::core(unit).expect("core"),
        golden("thermal.v1.core.lp")
    );
    assert_eq!(
        emit::views(unit).expect("views"),
        golden("thermal.v1.views.lp")
    );
    // The strict theory — what `keryx gen` writes by default (§13.3) — and the manifest that
    // records that choice (§13.4).
    assert_eq!(
        emit::emit_strict(unit).expect("emit"),
        golden("thermal.v1.emit.lp")
    );
    assert_eq!(
        manifest::write(unit, "-", Shape::Strict),
        golden("thermal.v1.keryx-manifest")
    );
}

#[test]
fn thermal_facts_match_the_committed_example() {
    // The committed payload is the spec's own (§28) — two readings — on the wire exactly as the
    // shared builders write it.
    let example = example();
    let payload = std::fs::read(example.join("batch.binpb")).expect("payload present");
    assert_eq!(
        payload,
        batch(&[reading("s-101", 44), reading("s-107", 21)])
    );

    // Its shred from the root `r0` — the constant the CLI mints (§4.1 item 6) — is the committed
    // fact module: what `keryx facts --root ReadingBatch=batch.binpb thermal.proto -I .` prints.
    let codec = Codec::from_source(
        &[example.join("thermal.proto")],
        &[example.clone(), vendored()],
    )
    .expect("thermal builds a codec");
    let facts = codec
        .shred(
            "thermal.v1.ReadingBatch",
            &payload,
            PayloadFormat::Binary,
            &Root::fresh(0),
        )
        .expect("the batch shreds");
    assert_eq!(
        facts.render().expect("the facts render"),
        golden("thermal.v1.facts.lp")
    );
}

#[test]
fn thermal_reassembles_the_committed_answer_set() {
    // The outbound half of the round trip (spec §12.3, §28): the committed answer set — the shred's
    // seven facts plus the `emit_reading_batch(r0)` marker a model asserts to export the tree —
    // reads through the `.lp` door and reassembles to the committed golden, which is `batch.binpb`
    // again, byte for byte. `answer.lp` → reassemble → the payload it was shredded from: the
    // identity the round trip is defined on (`keryx emit … answer.lp`), solver-free.
    let example = example();
    let codec = Codec::from_source(
        &[example.join("thermal.proto")],
        &[example.clone(), vendored()],
    )
    .expect("thermal builds a codec");
    let answer = std::fs::read_to_string(example.join("answer.lp")).expect("answer set present");
    let symbols = raise_answer_set(&answer).expect("the answer set reads");
    let reassembled = codec
        .reassemble(&symbols, PayloadFormat::Binary)
        .expect("the answer set reassembles");
    let messages = reassembled.messages();
    assert_eq!(messages.len(), 1, "one root marker, one message");
    assert_eq!(messages[0].type_name(), "thermal.v1.ReadingBatch");

    let golden = std::fs::read(example.join("batch.reassembled.binpb")).expect("golden present");
    assert_eq!(
        messages[0].bytes(),
        golden,
        "reassembly matches the committed golden"
    );
    let payload = std::fs::read(example.join("batch.binpb")).expect("payload present");
    assert_eq!(
        golden, payload,
        "the round trip returns the original payload byte for byte"
    );
}
