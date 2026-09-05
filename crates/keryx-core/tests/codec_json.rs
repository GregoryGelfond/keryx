//! The inbound codec's JSON form (spec §26; §11, §22): a payload in the protobuf JSON mapping
//! shreds through the same `Codec`, the same walk, and the same §6 policy as its binary and text
//! forms, to the same facts.
//!
//! **Parity.** The example's committed `batch.json` — the §28 thermal batch as canonical JSON —
//! yields, on both delivery seams, exactly what `batch.binpb` and `batch.txtpb` yield and what the
//! committed golden holds (§27: the example documents the JSON form as it documents the wire and
//! text forms, and regresses it the same way): the three-way parity §26 asks, on the spec's own
//! payload.

use std::path::{Path, PathBuf};

use keryx_test_support as support;
use keryx_test_support::wire::{batch, reading};

use keryx_core::codec::{Codec, Facts, PayloadFormat, Root};
use keryx_core::diagnostics::Diagnostics;

/// The thermal example's directory (`examples/thermal`), the subject of spec §28.
fn thermal_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal")
}

/// The thermal codec, through the descriptor-set door.
fn thermal_codec() -> Codec {
    let set = support::compile_in(&[thermal_dir(), support::vendored()], "thermal.proto");
    Codec::new(&set).expect("the thermal example builds a codec")
}

/// A payload in the form `format` shredded as `root_type` from the fresh root `r0`.
fn shred(
    codec: &Codec,
    root_type: &str,
    payload: &[u8],
    format: PayloadFormat,
) -> Result<Facts, Diagnostics> {
    codec.shred(root_type, payload, format, &Root::fresh(0))
}

/// One of the committed example's three forms (`examples/thermal/batch.*`): the §28 batch on the
/// wire, as text, or as canonical JSON.
fn example(name: &str) -> Vec<u8> {
    std::fs::read(thermal_dir().join(name)).expect("payload present")
}

/// The committed facts of the §28 batch (`examples/thermal/gen/thermal.v1.facts.lp`), the golden
/// every form of the example is held to.
fn golden() -> String {
    std::fs::read_to_string(thermal_dir().join("gen/thermal.v1.facts.lp")).expect("golden present")
}

#[test]
fn the_committed_json_example_shreds_to_the_facts_the_committed_binpb_and_txtpb_do() {
    // §26 parity on the spec's own payload (§28), through the example's three committed forms:
    // the JSON form, the text form, and the wire form of one message are one shred — the same
    // symbols in `Symbol::Ord` on the library seam, the same `.lp` text on the CLI seam, and all
    // three the committed golden the binary example is held to.
    let codec = thermal_codec();
    let json = example("batch.json");
    assert!(
        std::str::from_utf8(&json)
            .expect("the example is UTF-8 text")
            .contains("\"tempC\""),
        "the example spells its fields as the JSON mapping does"
    );
    let from_json =
        shred(&codec, "ReadingBatch", &json, PayloadFormat::Json).expect("the example shreds");

    let binary = example("batch.binpb");
    assert_eq!(
        binary,
        batch(&[reading("s-101", 44), reading("s-107", 21)]),
        "the committed payload is the §28 batch on the wire"
    );
    let from_binary = shred(&codec, "ReadingBatch", &binary, PayloadFormat::Binary)
        .expect("the binary payload shreds");
    let from_text = shred(
        &codec,
        "ReadingBatch",
        &example("batch.txtpb"),
        PayloadFormat::Textproto,
    )
    .expect("the text payload shreds");
    assert_eq!(from_json.symbols(), from_binary.symbols());
    assert_eq!(from_json.symbols(), from_text.symbols());

    let rendered = from_json.render().expect("the facts render");
    assert_eq!(rendered, from_binary.render().expect("the facts render"));
    assert_eq!(rendered, from_text.render().expect("the facts render"));
    assert_eq!(rendered, golden());
    assert_eq!(
        rendered,
        "reading(readings(r0, 0)).\n\
         reading(readings(r0, 1)).\n\
         reading_batch(r0).\n\
         sensor(readings(r0, 0), \"s-101\").\n\
         sensor(readings(r0, 1), \"s-107\").\n\
         temp_c(readings(r0, 0), 44).\n\
         temp_c(readings(r0, 1), 21).\n"
    );

    // The empty object is the empty message, as an empty wire and an empty text are: the root's
    // sort atom alone.
    let empty =
        shred(&codec, "ReadingBatch", b"{}", PayloadFormat::Json).expect("the empty object shreds");
    assert_eq!(
        empty.symbols(),
        shred(&codec, "ReadingBatch", &[], PayloadFormat::Binary)
            .expect("the empty wire shreds")
            .symbols()
    );
    assert_eq!(empty.render().expect("renders"), "reading_batch(r0).\n");
}
