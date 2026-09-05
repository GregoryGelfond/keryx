//! The inbound codec's textproto form (spec §26; §11, §22): a payload in the protobuf text format
//! shreds through the same `Codec`, the same walk, and the same §6 policy as its binary form, to
//! the same facts — the §28 thermal batch written as text yields, on both delivery seams, exactly
//! what `batch.binpb` yields and what the committed golden holds (format parity). The text door's
//! own refusals are diagnoses at the whole-payload locus, before any fact: a payload that is not
//! UTF-8 (the text format is text) and one that does not parse as the root type are
//! `UndecodablePayload`, naming the type and no byte of the payload.

use std::path::{Path, PathBuf};

use keryx_test_support as support;
use keryx_test_support::wire::{batch, reading};

use keryx_core::codec::{Codec, Facts, PayloadFormat, Root};
use keryx_core::diagnostics::{DiagnosticKind, Diagnostics};

/// The thermal example's directory (`examples/thermal`), the subject of spec §28.
fn thermal_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal")
}

/// The thermal codec, through the descriptor-set door.
fn thermal_codec() -> Codec {
    let set = support::compile_in(&[thermal_dir(), support::vendored()], "thermal.proto");
    Codec::new(&set).expect("the thermal example builds a codec")
}

/// The §28 batch in the protobuf text format: the same two readings `batch.binpb` carries on the
/// wire, each a message value in the format's `{ }` form.
const SECTION_28_TEXTPROTO: &str = "\
readings { sensor: \"s-101\" temp_c: 44 }
readings { sensor: \"s-107\" temp_c: 21 }
";

/// A payload in the form `format` shredded from the fresh root `r0`.
fn shred(codec: &Codec, payload: &[u8], format: PayloadFormat) -> Facts {
    codec
        .shred("ReadingBatch", payload, format, &Root::fresh(0))
        .expect("the payload shreds")
}

/// The one diagnosis a refused textproto payload carries, at the whole-payload locus: its kind
/// and detail.
fn the_one_refusal(codec: &Codec, payload: &[u8]) -> (DiagnosticKind, String) {
    let diagnostics: Diagnostics = codec
        .shred(
            "ReadingBatch",
            payload,
            PayloadFormat::Textproto,
            &Root::fresh(0),
        )
        .expect_err("the payload is refused");
    assert_eq!(diagnostics.len(), 1, "one diagnosis: {diagnostics}");
    let diagnostic = diagnostics.iter().next().expect("one diagnostic");
    assert!(
        diagnostic.locus().is_whole(),
        "the whole-payload locus: {diagnostic}"
    );
    (diagnostic.kind(), diagnostic.detail().to_owned())
}

#[test]
fn the_thermal_batch_as_textproto_shreds_to_the_facts_its_binary_form_does() {
    // §26 parity on the spec's own payload (§28): the text form and the wire form of one message
    // are one shred — the same symbols in `Symbol::Ord` on the library seam, the same `.lp` text
    // on the CLI seam, and both the committed golden the binary example is held to.
    let codec = thermal_codec();
    let from_text = shred(
        &codec,
        SECTION_28_TEXTPROTO.as_bytes(),
        PayloadFormat::Textproto,
    );

    let binary = std::fs::read(thermal_dir().join("batch.binpb")).expect("payload present");
    assert_eq!(
        binary,
        batch(&[reading("s-101", 44), reading("s-107", 21)]),
        "the committed payload is the §28 batch on the wire"
    );
    let from_binary = shred(&codec, &binary, PayloadFormat::Binary);
    assert_eq!(from_text.symbols(), from_binary.symbols());

    let rendered = from_text.render().expect("the facts render");
    assert_eq!(rendered, from_binary.render().expect("the facts render"));
    assert_eq!(
        rendered,
        std::fs::read_to_string(thermal_dir().join("gen/thermal.v1.facts.lp"))
            .expect("golden present")
    );
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

    // An empty text is the empty message, as an empty wire is: the root's sort atom alone.
    let empty = shred(&codec, b"", PayloadFormat::Textproto);
    assert_eq!(
        empty.symbols(),
        shred(&codec, &[], PayloadFormat::Binary).symbols()
    );
    assert_eq!(empty.render().expect("renders"), "reading_batch(r0).\n");
}

#[test]
fn a_textproto_payload_that_is_not_utf_8_is_undecodable_at_the_whole_payload_locus() {
    // The text format is UTF-8 text: a payload that is not — here a Latin-1 `é` inside the
    // sensor's string — is refused before the engine sees it, `UndecodablePayload` at the
    // whole-payload locus naming the root type and the failure's position, never its bytes.
    let codec = thermal_codec();
    let (kind, detail) = the_one_refusal(&codec, b"readings { sensor: \"s-\xe9\" temp_c: 44 }");
    assert_eq!(kind, DiagnosticKind::UndecodablePayload);
    assert!(
        detail.contains("thermal.v1.ReadingBatch") && detail.contains("UTF-8"),
        "the detail names the root type and the requirement: {detail}"
    );
    assert!(
        !detail.contains("s-") && !detail.contains("sensor"),
        "the detail echoes nothing of the payload: {detail}"
    );
}

#[test]
fn a_textproto_payload_that_does_not_parse_as_the_root_type_is_undecodable() {
    // A field the type does not declare, a message value left open, and the binary wire form
    // handed over as text: each is the engine's parse failure, composed as `UndecodablePayload`
    // at the whole-payload locus naming the root type — one diagnosis, no facts beside it, no
    // panic.
    let codec = thermal_codec();
    let binary = batch(&[reading("s-101", 44)]);
    for payload in [
        &b"readings { sensor: \"s-101\" pressure: 1 }"[..],
        b"readings { sensor: \"s-101\"",
        &binary,
    ] {
        let (kind, detail) = the_one_refusal(&codec, payload);
        assert_eq!(kind, DiagnosticKind::UndecodablePayload);
        assert!(
            detail.contains("thermal.v1.ReadingBatch"),
            "the detail names the root type: {detail}"
        );
    }
}
