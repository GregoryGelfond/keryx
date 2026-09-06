//! The binary round-trip property (the threat model's property 4, integrity): a payload shredded to
//! facts and reassembled from them is the payload again, and an answer set reassembled to a payload
//! and shredded is the answer set again — on the canonical forms, byte-for-byte and symbol-for-symbol.
//! The forward half here is the payload→facts→payload identity on the thermal story (§28); the walk's
//! own shape checks are in `codec_reassemble.rs` and the module tests.

use std::path::Path;

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_test_support::wire;
use themelios_program::{Name, Sign, Symbol};

/// The thermal example's codec (spec §28), through the source door.
fn thermal_codec() -> Codec {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    Codec::from_source(&[example.join("thermal.proto")], &[example, vendored])
        .expect("the thermal example compiles")
}

/// A constant symbol — a positive zero-argument function.
fn constant(name: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(name).expect("an identifier"),
        arguments: Vec::new(),
        sign: Sign::Positive,
    }
}

/// The `emit_<sort>(root)` marker exporting the tree under `root`.
fn marker(sort_marker: &str, root: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(sort_marker).expect("an identifier"),
        arguments: vec![constant(root)],
        sign: Sign::Positive,
    }
}

#[test]
fn a_payload_shredded_and_reassembled_is_the_payload_again() {
    // `ReadingBatch { repeated Reading readings }` with two readings: shred it to facts under the
    // root `r0`, export the root with its marker, and reassemble — the bytes are the canonical
    // payload again (payload → facts → payload, the round-trip's forward half on the worked story).
    let codec = thermal_codec();
    let payload = wire::batch(&[wire::reading("s-1", 1), wire::reading("s-2", 2)]);
    let facts = codec
        .shred(
            "ReadingBatch",
            &payload,
            PayloadFormat::Binary,
            &Root::named(Name::new("r0").expect("an identifier")),
        )
        .expect("the batch shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(marker("emit_reading_batch", "r0"));

    let out = codec.reassemble(&answer).expect("the facts reassemble");
    assert_eq!(out.messages().len(), 1);
    assert_eq!(out.messages()[0].type_name(), "thermal.v1.ReadingBatch");
    assert_eq!(out.messages()[0].bytes(), payload);
}

#[test]
fn a_reassembled_message_emits_the_canonical_form_omitting_a_zero_implicit_field() {
    // The shred materialises an implicit field's zero (§5), so the answer set carries `temp_c(r0, 0)`
    // even for a payload that omitted it; the reassembly builds `temp_c = 0` and encodes the
    // *canonical* proto3 form, which omits a zero implicit scalar. So a `Reading` with `temp_c = 0`
    // reassembles to the sensor field alone — the canonical form the round-trip is defined on, and
    // the semantic identity of the payload whether or not the zero was on the wire.
    let codec = thermal_codec();
    // A non-canonical payload that spells the zero out; the shred reads it, the reassembly drops it.
    let non_canonical = wire::reading("only-sensor", 0);
    let facts = codec
        .shred(
            "Reading",
            &non_canonical,
            PayloadFormat::Binary,
            &Root::named(Name::new("r0").expect("an identifier")),
        )
        .expect("the reading shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(marker("emit_reading", "r0"));

    let out = codec.reassemble(&answer).expect("the facts reassemble");
    assert_eq!(out.messages().len(), 1);
    // The canonical form: the sensor field alone (field 1), the zero `temp_c` omitted.
    let mut canonical = Vec::new();
    wire::delimited(1, b"only-sensor", &mut canonical);
    assert_eq!(out.messages()[0].bytes(), canonical);
}
