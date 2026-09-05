//! The inbound codec's JSON form (spec §26; §11, §22), instrumented as its binary and text forms
//! are (`codec_totality.rs`, `codec_depth.rs`, `codec_determinism.rs`, `codec_textproto.rs`): a
//! payload in the protobuf JSON mapping shreds through the same `Codec`, the same walk, and the
//! same §6 policy as its binary and text forms, to the same facts.
//!
//! **Parity.** The example's committed `batch.json` — the §28 thermal batch as canonical JSON —
//! yields, on both delivery seams, exactly what `batch.binpb` and `batch.txtpb` yield and what the
//! committed golden holds (§27: the example documents the JSON form as it documents the wire and
//! text forms, and regresses it the same way): the three-way parity §26 asks, on the spec's own
//! payload. And the mapping's alternative spellings of one message — a field by its JSON name or
//! its proto name, an integer as a number or as a string of digits, a character raw or escaped,
//! the members in any order, whitespace anywhere — are one shred.
//!
//! **The door's own refusals** are diagnoses at the whole-payload locus, before any fact: a payload
//! that is not UTF-8, one that is not one JSON value of the root type — a member the type does
//! not declare, a value outside its kind, a value left open, text after the value — and the empty
//! text, which is no JSON value where the wire and the text format read the empty message from
//! nothing (the one asymmetry across the three forms; the JSON form's empty message is `{}`), are
//! `UndecodablePayload`, naming the type.
//!
//! **Bounded depth, branch (a)** (spec §8; the threat model's property 3). No guard precedes this
//! decode: the deserializer bounds its own nesting — its count refuses the 128th nested array or
//! object — and the walk's uniform ceiling stands beneath that count, so which of the two a
//! payload meets first goes by its fields' form, and the instrument pins both boundaries against
//! the pinned engine. A chain of **singular** message fields spends one object a level and reaches
//! the ceiling inside the count: 99 levels shred whole, to the facts the same chain shreds to from
//! the wire; 100 through 126 — every depth the deserializer admits past the ceiling, each
//! deserialized whole on the thread the decode sizes for that admit (`engine::JSON_DECODE_STACK`)
//! — are the walk's `PayloadTooDeep`, so this is the door on which the walk's counter is the
//! binding refusal for more than a single level; and 127, the 128th object, is the deserializer's
//! own refusal, `UndecodablePayload`, with no walk and no call stack spent past the count. That
//! last case is the pin on the door's premise, that the deserializer's limit is on: were it ever
//! lifted, the payload would deserialize and the shred would be the walk's `PayloadTooDeep`, and
//! the case would fail. A **repeated** chain spends an array and an object a level and meets the
//! count first: 63 levels shred, to the facts the same chain shreds to from the wire and from
//! text, and 64 — which the wire admits and shreds whole — is `UndecodablePayload`, a refusal at
//! a shallower message depth than the ceiling, never admission past it. A `google.protobuf.Value`
//! chain binds earlier still, at the engine's own message-decode limit, as the doc of
//! `engine::JSON_DECODE_STACK` records; no fixture of this corpus declares one, so that binding is
//! recorded there and not instrumented here.
//!
//! **Totality over arbitrary bytes.** `Codec::shred` returns facts or typed `Diagnostics` over
//! *any* payload — never a panic, an abort, or a hang — checked over a generator mixing arbitrary
//! bytes and characters with JSON spelled in the schemas' own vocabularies, so a meaningful share
//! deserializes and reaches the walk and every one of its refusals, and nesting on either side of
//! both boundaries; a fixed-seed tally pins that reach. A second generator, over one message whose
//! every field can carry a value §6 refuses, is checked against a model of the policy as the
//! binary door's is: each hostile value yields exactly its refusal, at its field, and a benign one
//! its fact, whatever spelling the mapping admits for it. As on the other doors, no generated
//! payload trips the containment frame — a `DependencyFault` would be a found trigger, to be
//! recorded, not hidden — and the door leaves no frame live on the thread that called it.
//!
//! **Determinism.** The same JSON payload yields identical facts on both seams, however often it
//! is shredded and from whichever codec over the schema, whatever the order of its members or of
//! a map's keys; and every fact is delivered exactly once on each seam.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use keryx_test_support as support;
use keryx_test_support::wire::{self, batch, delimited, reading};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

use keryx_core::codec::{Codec, Facts, PayloadFormat, Root};
use keryx_core::diagnostics::{DiagnosticKind, Diagnostics};

/// The uniform payload nesting ceiling (`codec::walk::NESTING_CEILING`): one below the engine's
/// binary decode recursion limit of 100, every format alike.
const CEILING: usize = 99;

/// The deepest nesting of arrays and objects the deserializer admits: its count starts at 128 and
/// refuses the container that would spend the last of it (`serde_json` 1.0.151 `src/de.rs:63`,
/// `:1372-1384`), so 127 nested containers deserialize and a 128th is its own refusal.
const CONTAINERS: usize = 127;

/// The message the deserializer's count refusal carries — the exact text `serde_json` 1.0.151
/// emits for it (`src/error.rs:384`), composed into the door's `UndecodablePayload` detail. A
/// deliberate coupling to the pinned dependency, as `CONTAINERS` is to its count: the text is what
/// tells the deserializer's own refusal from every other undecodable payload, so the instruments
/// assert it where they pin that the count, and not the walk, refused; and it is re-owed, with the
/// count, on any deliberate bump of `serde_json`.
const COUNT_EXCEEDED: &str = "recursion limit exceeded";

/// The deepest chain of singular message fields the deserializer admits — one object a level
/// below the root's own — past which its count, not the walk's ceiling, is the refusal.
const SINGULAR_DEEPEST: usize = CONTAINERS - 1;

/// The deepest chain of repeated message fields the deserializer admits — an array and an object
/// a level below the root's own — short of the walk's ceiling, so its count is the binding
/// refusal for the form.
const REPEATED_DEEPEST: usize = (CONTAINERS - 1) / 2;

/// The recursion fixture's mutual pair below an `A` root as a chain of singular message fields:
/// `A.b` is a `B`, `B.a` an `A`, and round again — one object a level.
const SINGULAR: &[&str] = &["b", "a"];

/// The recursion fixture's `Tree` as a chain of its repeated `children` — an array and an object a
/// level.
const REPEATED: &[&str] = &["children"];

/// The thermal example's directory (`examples/thermal`), the subject of spec §28.
fn thermal_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal")
}

/// The thermal codec, through the descriptor-set door.
fn thermal_codec() -> Codec {
    let set = support::compile_in(&[thermal_dir(), support::vendored()], "thermal.proto");
    Codec::new(&set).expect("the thermal example builds a codec")
}

/// A fixture's codec, through the descriptor-set door.
fn fixture_codec(name: &str) -> Codec {
    Codec::new(&support::compile_fixture(name)).expect("the fixture builds a codec")
}

/// The codecs the arbitrary-payload generator shreds against, each with the root it deserializes
/// as — the binary and text instruments' five, and the recursion fixture a second time rooted at
/// `A`, whose singular chain the deserializer admits deeper than the walk's ceiling. Built once:
/// a codec is per schema, never per payload.
static CODECS: LazyLock<Vec<(Codec, &'static str)>> = LazyLock::new(|| {
    vec![
        (thermal_codec(), "thermal.v1.ReadingBatch"),
        (fixture_codec("refusals.proto"), "keryx.refusals.Probe"),
        (fixture_codec("maps.proto"), "keryx.maps.Inventory"),
        (fixture_codec("recursion.proto"), "keryx.rec.Tree"),
        (fixture_codec("recursion.proto"), "keryx.rec.A"),
        (
            fixture_codec("scalar_treatment.proto"),
            "keryx.scalars.Sample",
        ),
    ]
});

/// The refusals probe's codec, for the hostile-value generator.
static PROBE: LazyLock<Codec> = LazyLock::new(|| fixture_codec("refusals.proto"));

/// A payload in the form `format` shredded as `root_type` from the fresh root `r0`.
fn shred(
    codec: &Codec,
    root_type: &str,
    payload: &[u8],
    format: PayloadFormat,
) -> Result<Facts, Diagnostics> {
    codec.shred(root_type, payload, format, &Root::fresh(0))
}

/// A JSON payload shredded as `root_type` from the fresh root `r0`.
fn shred_json(codec: &Codec, root_type: &str, json: &str) -> Result<Facts, Diagnostics> {
    shred(codec, root_type, json.as_bytes(), PayloadFormat::Json)
}

/// The one diagnosis a refused payload carries, at the whole-payload locus: its kind and detail.
fn the_one_refusal(diagnostics: &Diagnostics) -> (DiagnosticKind, &str) {
    assert_eq!(diagnostics.len(), 1, "one diagnosis: {diagnostics}");
    let diagnostic = diagnostics.iter().next().expect("one diagnostic");
    assert!(
        diagnostic.locus().is_whole(),
        "the whole-payload locus: {diagnostic}"
    );
    (diagnostic.kind(), diagnostic.detail())
}

/// A JSON payload refused as `root_type`: its one diagnosis, at the whole-payload locus.
fn refused(codec: &Codec, root_type: &str, payload: &[u8]) -> (DiagnosticKind, String) {
    let diagnostics =
        shred(codec, root_type, payload, PayloadFormat::Json).expect_err("the payload is refused");
    let (kind, detail) = the_one_refusal(&diagnostics);
    (kind, detail.to_owned())
}

/// Every fact once on each seam: the symbols sorted and pairwise distinct, and the rendering one
/// line per symbol — the rendering's canonical program spells each fact once, so a count equality
/// proves no fact was emitted twice. The rendering, for the caller's own comparison.
fn assert_once_each(facts: &Facts) -> String {
    let symbols = facts.symbols();
    assert!(
        symbols.windows(2).all(|pair| pair[0] < pair[1]),
        "the symbol seam is sorted with no fact twice: {symbols:?}"
    );
    let rendered = facts.render().expect("the facts render");
    assert_eq!(
        rendered.lines().count(),
        symbols.len(),
        "one rendered fact per symbol: {rendered}"
    );
    rendered
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

#[test]
fn the_mapping_s_alternative_spellings_of_one_message_are_one_shred() {
    // The JSON mapping spells one message many ways — a field by its JSON name (`tempC`) or its
    // proto name (`temp_c`), an integer as a number, as a string of digits, or as a number with
    // an empty fraction, a character raw or as a Unicode escape, the members in any order,
    // whitespace anywhere — and none of it is content: each spelling of the §28 batch shreds to
    // the example's facts.
    let codec = thermal_codec();
    let example = shred(
        &codec,
        "ReadingBatch",
        &example("batch.json"),
        PayloadFormat::Json,
    )
    .expect("the example shreds");
    for spelling in [
        r#"{"readings":[{"sensor":"s-101","tempC":44},{"sensor":"s-107","tempC":21}]}"#,
        r#"{"readings": [{"temp_c": 44, "sensor": "s-101"}, {"temp_c": 21, "sensor": "s-107"}]}"#,
        "{\n\t\"readings\" : [ {\"sensor\":\"s-101\", \"tempC\":\"44\"},\n\t\t\
         {\"sensor\":\"s-\\u0031\\u0030\\u0037\", \"tempC\": 21.0} ]\n}\n",
    ] {
        let facts = shred_json(&codec, "ReadingBatch", spelling).expect("the spelling shreds");
        assert_eq!(facts.symbols(), example.symbols(), "{spelling}");
    }
}

#[test]
fn a_json_payload_that_is_not_utf_8_is_undecodable_at_the_whole_payload_locus() {
    // JSON is UTF-8 text: a payload that is not — here a Latin-1 `é` inside the sensor's string
    // — is the deserializer's refusal, `UndecodablePayload` at the whole-payload locus naming the
    // root type and the failure's position, never its bytes.
    let codec = thermal_codec();
    let (kind, detail) = refused(
        &codec,
        "ReadingBatch",
        b"{\"readings\": [{\"sensor\": \"s-\xe9\", \"tempC\": 44}]}",
    );
    assert_eq!(kind, DiagnosticKind::UndecodablePayload);
    assert!(
        detail.contains("thermal.v1.ReadingBatch"),
        "the detail names the root type: {detail}"
    );
    assert!(
        !detail.contains("s-") && !detail.contains("sensor"),
        "the detail echoes nothing of the payload: {detail}"
    );
}

#[test]
fn a_json_payload_that_is_not_one_canonical_value_of_the_root_type_is_undecodable() {
    // A member the type does not declare, a value left open, the binary wire handed over as
    // JSON, an array or `null` where the root object belongs, text after the one value, an
    // object where a repeated field's array belongs, and a value outside its kind — a string of
    // letters, a fraction, or a number past the width, refused by the engine at the decode where
    // the walk's `ValueOutOfRange` is for a value the kind carries and the dialect cannot: each
    // is the engine's refusal, composed as `UndecodablePayload` at the whole-payload locus naming
    // the root type — one diagnosis, no facts beside it, no panic.
    let codec = thermal_codec();
    let binary = batch(&[reading("s-101", 44)]);
    for payload in [
        &br#"{"readings": [{"sensor": "s-101", "pressure": 1}]}"#[..],
        br#"{"readings": [{"sensor": "s-101""#,
        &binary,
        b"[]",
        b"null",
        b"{} x",
        br#"{"readings": {}}"#,
        br#"{"readings": [{"tempC": "hot"}]}"#,
        br#"{"readings": [{"tempC": 1.5}]}"#,
        br#"{"readings": [{"tempC": 4294967296}]}"#,
    ] {
        let (kind, detail) = refused(&codec, "ReadingBatch", payload);
        assert_eq!(
            kind,
            DiagnosticKind::UndecodablePayload,
            "{}",
            String::from_utf8_lossy(payload)
        );
        assert!(
            detail.contains("thermal.v1.ReadingBatch"),
            "the detail names the root type: {detail}"
        );
    }
}

#[test]
fn the_empty_text_is_no_json_value_where_the_wire_and_the_text_format_read_the_empty_message() {
    // Across the three forms, one asymmetry: an empty payload is the empty message on the wire
    // and as text — the root's sort atom alone — and is no JSON value at all, so for the JSON
    // form it is `UndecodablePayload`, as a text of whitespace alone is. The JSON form's empty
    // message is `{}`, whitespace around it or not, and it is the message the other two forms
    // read from nothing.
    let codec = thermal_codec();
    let empty_wire =
        shred(&codec, "ReadingBatch", &[], PayloadFormat::Binary).expect("the empty wire shreds");
    assert_eq!(
        empty_wire.render().expect("renders"),
        "reading_batch(r0).\n"
    );
    let empty_text = shred(&codec, "ReadingBatch", b"", PayloadFormat::Textproto)
        .expect("the empty text shreds");
    assert_eq!(empty_text.symbols(), empty_wire.symbols());
    for payload in [&b""[..], b" \n\t "] {
        let (kind, detail) = refused(&codec, "ReadingBatch", payload);
        assert_eq!(kind, DiagnosticKind::UndecodablePayload, "{payload:?}");
        assert!(
            detail.contains("thermal.v1.ReadingBatch"),
            "the detail names the root type: {detail}"
        );
    }
    for spelling in ["{}", " \n{ }\n"] {
        let facts = shred_json(&codec, "ReadingBatch", spelling).expect("the empty object shreds");
        assert_eq!(facts.symbols(), empty_wire.symbols(), "{spelling:?}");
    }
}

// The depth boundaries through the door: the walk's ceiling and the deserializer's count, and
// which binds for which form.

/// A JSON object nesting message values `levels` deep through the fields of `chain`, taken in
/// turn and round again, with `innermost` as the deepest message's members: for a singular chain
/// `{"f": {"g": {"f": … {innermost} …}}}`, one object a level below the root's; for a `repeated`
/// one `{"f": [{"f": [… {innermost} …]}]}`, an array and an object a level.
fn nested(chain: &[&str], repeated: bool, levels: usize, innermost: &str) -> String {
    let mut text = String::new();
    for level in 0..levels {
        text.push_str("{\"");
        text.push_str(chain[level % chain.len()]);
        text.push_str(if repeated { "\": [" } else { "\": " });
    }
    text.push('{');
    text.push_str(innermost);
    text.push('}');
    for _ in 0..levels {
        text.push_str(if repeated { "]}" } else { "}" });
    }
    text
}

/// The same chain on the binary wire: `levels` messages nested through the field numbered `tag`,
/// each carrying nothing but the next and the innermost nothing at all, built from the inside out.
fn wire_chain(tag: u32, levels: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..levels {
        let mut outer = Vec::new();
        delimited(tag, &bytes, &mut outer);
        bytes = outer;
    }
    bytes
}

/// The same chain as text: `levels` trees nested through `children`, each holding the next.
fn text_chain(levels: usize) -> String {
    format!("{}{}", "children { ".repeat(levels), "} ".repeat(levels))
}

#[test]
fn a_singular_chain_meets_the_walk_s_ceiling_inside_the_deserializer_s_count() {
    let codec = fixture_codec("recursion.proto");

    // 99 levels: the deserializer admits it and the walk shreds it — the root and 99 nested
    // messages, each its sort atom, the deepest occupant 99 applications below `r0` (a hundred
    // parentheses on its line, the sort's and its levels'), every fact once on both seams — and
    // exactly the facts the same chain on the binary wire shreds to: parity at the ceiling.
    let facts = shred_json(&codec, "keryx.rec.A", &nested(SINGULAR, false, CEILING, ""))
        .expect("the deepest payload the ceiling admits shreds");
    assert_eq!(facts.symbols().len(), CEILING + 1);
    let rendered = facts.render().expect("renders");
    assert_eq!(rendered.lines().count(), facts.symbols().len());
    assert_eq!(
        rendered.lines().map(|line| line.matches('(').count()).max(),
        Some(CEILING + 1)
    );
    let from_binary = shred(
        &codec,
        "keryx.rec.A",
        &wire_chain(1, CEILING),
        PayloadFormat::Binary,
    )
    .expect("the binary chain shreds");
    assert_eq!(facts.symbols(), from_binary.symbols());

    // 100 levels, and 126 — the deepest singular chain the deserializer admits, 127 objects: each
    // is deserialized whole, on the thread the decode sizes for that admit, and refused by the
    // walk — `PayloadTooDeep`, once, at the whole-payload locus, naming the over-deep sort (the
    // `A` at level 100, the first the walk meets past the ceiling), its depth, and the ceiling,
    // with no facts beside it. The walk's refusal and no other's — the text door's guard names
    // no sort, the deserializer's refusal names no ceiling — on the one door where the ceiling
    // is the binding refusal for more than a single level.
    for levels in [CEILING + 1, SINGULAR_DEEPEST] {
        let (kind, detail) = refused(
            &codec,
            "keryx.rec.A",
            nested(SINGULAR, false, levels, "").as_bytes(),
        );
        assert_eq!(kind, DiagnosticKind::PayloadTooDeep, "{levels}");
        assert!(
            detail.contains("keryx.rec.A") && detail.contains("100") && detail.contains("99"),
            "the detail names the sort, the depth, and the ceiling: {detail}"
        );
        assert!(
            !detail.contains(COUNT_EXCEEDED),
            "the walk's refusal, not the deserializer's: {detail}"
        );
    }
}

#[test]
fn a_repeated_chain_meets_the_deserializer_s_count_before_the_walk_s_ceiling() {
    let codec = fixture_codec("recursion.proto");

    // 63 levels — 127 containers, the deepest repeated chain the deserializer admits: the root
    // and 63 nested trees shred, each its sort atom and its materialised label, the deepest
    // occupant 63 `children` applications below `r0`, every fact once on both seams — and
    // exactly the facts the same chain shreds to from the wire and from text: three-way parity at
    // the form's deepest admitted level.
    let facts = shred_json(
        &codec,
        "keryx.rec.Tree",
        &nested(REPEATED, true, REPEATED_DEEPEST, ""),
    )
    .expect("the deepest payload the deserializer admits shreds");
    assert_eq!(facts.symbols().len(), 2 * (REPEATED_DEEPEST + 1));
    let rendered = facts.render().expect("renders");
    assert_eq!(rendered.lines().count(), facts.symbols().len());
    assert_eq!(
        rendered
            .lines()
            .map(|line| line.matches("children(").count())
            .max(),
        Some(REPEATED_DEEPEST)
    );
    let from_binary = shred(
        &codec,
        "keryx.rec.Tree",
        &wire_chain(2, REPEATED_DEEPEST),
        PayloadFormat::Binary,
    )
    .expect("the binary chain shreds");
    assert_eq!(facts.symbols(), from_binary.symbols());
    let from_text = shred(
        &codec,
        "keryx.rec.Tree",
        text_chain(REPEATED_DEEPEST).as_bytes(),
        PayloadFormat::Textproto,
    )
    .expect("the text chain shreds");
    assert_eq!(facts.symbols(), from_text.symbols());

    // 64 levels — the 128th container is the 64th level's array: the deserializer's own refusal,
    // `UndecodablePayload` naming the root type and the count exceeded, before any walk — at a
    // message depth the wire admits and shreds whole, 35 levels short of the ceiling. Refusal in
    // the safe direction: the JSON form never admits a message deeper than the ceiling, and here
    // refuses shallower than it.
    let (kind, detail) = refused(
        &codec,
        "keryx.rec.Tree",
        nested(REPEATED, true, REPEATED_DEEPEST + 1, "").as_bytes(),
    );
    assert_eq!(kind, DiagnosticKind::UndecodablePayload);
    assert!(
        detail.contains("keryx.rec.Tree") && detail.contains(COUNT_EXCEEDED),
        "the detail names the root type and the count exceeded: {detail}"
    );
    assert!(
        !detail.contains("ceiling"),
        "the deserializer's refusal, not the walk's: {detail}"
    );
    let on_the_wire = shred(
        &codec,
        "keryx.rec.Tree",
        &wire_chain(2, REPEATED_DEEPEST + 1),
        PayloadFormat::Binary,
    )
    .expect("the wire admits what the JSON form refuses");
    assert_eq!(on_the_wire.symbols().len(), 2 * (REPEATED_DEEPEST + 2));
}

#[test]
fn past_the_deserializer_s_count_the_decode_refuses_before_any_walk() {
    // The 128th container and far beyond, in either form: the deserializer's count trips at that
    // container — `UndecodablePayload` at the whole-payload locus, naming the root type and the
    // limit exceeded — and nothing is walked. A chain ten thousand levels deep (some 60 KB of
    // JSON as a singular chain, 150 KB as a repeated one) is refused the same way: the
    // deserializer recurses only as far as the 127 containers its count admits, on the thread
    // the decode sizes for exactly that, so there is no call stack to exhaust on the way. Were
    // the limit ever lifted, the singular chain of 127 would deserialize and be the walk's
    // `PayloadTooDeep`, and the deeper ones would spend call stack on every level.
    let codec = fixture_codec("recursion.proto");
    for levels in [SINGULAR_DEEPEST + 1, 1_000, 10_000] {
        let (kind, detail) = refused(
            &codec,
            "keryx.rec.A",
            nested(SINGULAR, false, levels, "").as_bytes(),
        );
        assert_eq!(
            kind,
            DiagnosticKind::UndecodablePayload,
            "singular {levels}"
        );
        assert!(
            detail.contains("keryx.rec.A") && detail.contains(COUNT_EXCEEDED),
            "the detail names the root type and the count exceeded: {detail}"
        );
        assert!(!detail.contains("ceiling"), "no walk ran: {detail}");
    }
    for levels in [REPEATED_DEEPEST + 1, 1_000, 10_000] {
        let (kind, detail) = refused(
            &codec,
            "keryx.rec.Tree",
            nested(REPEATED, true, levels, "").as_bytes(),
        );
        assert_eq!(
            kind,
            DiagnosticKind::UndecodablePayload,
            "repeated {levels}"
        );
        assert!(
            detail.contains("keryx.rec.Tree") && detail.contains(COUNT_EXCEEDED),
            "the detail names the root type and the count exceeded: {detail}"
        );
        assert!(!detail.contains("ceiling"), "no walk ran: {detail}");
    }
}

// Determinism: the same payload, the same facts, on both seams.

/// A `counts` entry of the maps fixture's `Inventory` on the wire: `map<string, int32>`.
fn count(key: &str, value: i32) -> Vec<u8> {
    let mut entry = Vec::new();
    delimited(1, key.as_bytes(), &mut entry);
    wire::int32(2, value, &mut entry);
    entry
}

/// An `items` entry of the maps fixture's `Inventory` on the wire: `map<int64, Item>`, the item
/// its `sku`.
fn item(key: i64, sku: &str) -> Vec<u8> {
    let mut item = Vec::new();
    delimited(1, sku.as_bytes(), &mut item);
    let mut entry = Vec::new();
    wire::int64(1, key, &mut entry);
    delimited(2, &item, &mut entry);
    entry
}

#[test]
fn the_same_json_payload_shreds_to_identical_facts_however_often_and_from_whichever_codec() {
    // One payload — three readings, one of them empty; the same codec twice, a second codec over
    // the same set, and a codec from the source the set was compiled from: one symbol sequence
    // and one rendering, every time — and the rendering the same readings yield from the wire.
    let payload =
        r#"{"readings": [{"sensor": "s-107", "tempC": 21}, {"sensor": "s-101", "tempC": 44}, {}]}"#;
    let codec = thermal_codec();
    let first = shred_json(&codec, "ReadingBatch", payload).expect("shreds");
    let again = shred_json(&codec, "ReadingBatch", payload).expect("shreds");
    let rebuilt = shred_json(&thermal_codec(), "ReadingBatch", payload).expect("shreds");
    let from_source = Codec::from_source(
        &[thermal_dir().join("thermal.proto")],
        &[thermal_dir(), support::vendored()],
    )
    .expect("the thermal source builds a codec");
    let from_source = shred_json(&from_source, "ReadingBatch", payload).expect("shreds");

    let rendered = assert_once_each(&first);
    assert_eq!(
        rendered.lines().count(),
        10,
        "the batch atom and three readings' three facts"
    );
    for other in [&again, &rebuilt, &from_source] {
        assert_eq!(first.symbols(), other.symbols());
        assert_eq!(rendered, assert_once_each(other));
    }
    let on_the_wire = shred(
        &codec,
        "ReadingBatch",
        &batch(&[reading("s-107", 21), reading("s-101", 44), reading("", 0)]),
        PayloadFormat::Binary,
    )
    .expect("shreds");
    assert_eq!(rendered, assert_once_each(&on_the_wire));
}

#[test]
fn a_map_shreds_the_same_whatever_its_key_order_and_as_its_wire_form_does() {
    // `Inventory`'s two maps — three scalar-valued entries and two message-valued ones under
    // `int64` keys, which the mapping spells as strings — in three member and key orders. The
    // engine's map is unordered; keryx orders the entries once by key, so every order shreds to
    // the one key-sorted fact set, on both seams, and to the facts the same maps shred to from
    // the wire.
    let codec = fixture_codec("maps.proto");
    let shreds: Vec<Facts> = [
        r#"{"counts": {"b": 2, "a": 1, "c": 3}, "items": {"20": {"sku": "x"}, "-1": {"sku": "y"}}}"#,
        r#"{"items": {"-1": {"sku": "y"}, "20": {"sku": "x"}}, "counts": {"c": 3, "a": 1, "b": 2}}"#,
        r#"{"counts": {"a": 1, "b": 2, "c": 3}, "items": {"-1": {"sku": "y"}, "20": {"sku": "x"}}}"#,
    ]
    .iter()
    .map(|order| shred_json(&codec, "Inventory", order).expect("shreds"))
    .collect();
    let rendered = assert_once_each(&shreds[0]);
    assert_eq!(
        rendered,
        "counts(r0, \"a\", 1).\n\
         counts(r0, \"b\", 2).\n\
         counts(r0, \"c\", 3).\n\
         inventory(r0).\n\
         item(items(r0, \"-1\")).\n\
         item(items(r0, \"20\")).\n\
         sku(items(r0, \"-1\"), \"y\").\n\
         sku(items(r0, \"20\"), \"x\").\n"
    );
    for other in &shreds[1..] {
        assert_eq!(shreds[0].symbols(), other.symbols());
        assert_eq!(rendered, assert_once_each(other));
    }
    let mut bytes = Vec::new();
    delimited(1, &count("b", 2), &mut bytes);
    delimited(1, &count("a", 1), &mut bytes);
    delimited(1, &count("c", 3), &mut bytes);
    delimited(2, &item(20, "x"), &mut bytes);
    delimited(2, &item(-1, "y"), &mut bytes);
    let on_the_wire =
        shred(&codec, "Inventory", &bytes, PayloadFormat::Binary).expect("the wire form shreds");
    assert_eq!(rendered, assert_once_each(&on_the_wire));
}

#[test]
fn every_fact_is_delivered_once_on_each_seam_even_from_a_payload_that_repeats_a_member() {
    // A JSON object that repeats a member — a singular field twice, a map key twice — is read as
    // a wire that repeats a field is: the last occurrence wins, and yields one fact, so there is
    // no duplicate in the fact set for either seam to de-duplicate. And identical *values* at
    // distinct occupants are distinct facts, not duplicates: three children with one label are
    // three label atoms.
    let thermal = thermal_codec();
    let facts = shred_json(
        &thermal,
        "Reading",
        r#"{"sensor": "s-101", "tempC": 44, "sensor": "s-202", "tempC": 45}"#,
    )
    .expect("shreds");
    assert_eq!(
        assert_once_each(&facts),
        "reading(r0).\n\
         sensor(r0, \"s-202\").\n\
         temp_c(r0, 45).\n"
    );

    let maps = fixture_codec("maps.proto");
    let facts = shred_json(
        &maps,
        "Inventory",
        r#"{"counts": {"a": 1, "a": 2, "b": 3, "a": 4}}"#,
    )
    .expect("shreds");
    assert_eq!(
        assert_once_each(&facts),
        "counts(r0, \"a\", 4).\n\
         counts(r0, \"b\", 3).\n\
         inventory(r0).\n"
    );

    let trees = fixture_codec("recursion.proto");
    let facts = shred_json(
        &trees,
        "Tree",
        r#"{"children": [{"label": "x"}, {"label": "x"}, {"label": "x"}]}"#,
    )
    .expect("shreds");
    let rendered = assert_once_each(&facts);
    assert_eq!(
        facts.symbols().len(),
        8,
        "four tree atoms, four label atoms"
    );
    assert_eq!(rendered.matches(", \"x\").").count(), 3);
}

// Totality over arbitrary payloads: the generator, in the schemas' own vocabularies and outside
// them.

/// How a field's value is spelled in the JSON mapping: an integer, a float, a string, a `bytes`
/// value (base64), an enum value (a name or a number), a boolean, or an object over the fields of
/// the message named. Each form's strategy draws values the walk carries, values it refuses (§6),
/// and, at a low weight, spellings the deserializer refuses — so a payload spelled in a vocabulary
/// mostly deserializes, and the walk's refusals are reached often, not rarely.
#[derive(Clone, Copy, Debug)]
enum Form {
    Integer,
    Float,
    Text,
    Bytes,
    Enum,
    Bool,
    Message(&'static str),
}

/// A field's shape in the mapping: one value, an array of values, or an object keyed by strings
/// spelling keys of the form given — the three the mapping spells differently, where the text
/// format spells them alike.
#[derive(Clone, Copy, Debug)]
enum Shape {
    One,
    Many,
    Keyed(Form),
}

/// A field as the vocabulary spells it: its JSON name, its shape, and the form of its values.
type Field = (&'static str, Shape, Form);

/// The messages of the codecs' schemas as the JSON mapping spells their fields — the vocabulary a
/// payload is drawn in so it can deserialize as one of the six roots: each root and every message
/// reachable from it, a field by its JSON name (and `temp_c` by its proto name too, which the
/// mapping admits), a map as the keyed object the mapping spells it as.
const MESSAGES: &[(&str, &[Field])] = &[
    (
        "thermal.v1.ReadingBatch",
        &[("readings", Shape::Many, Form::Message("thermal.v1.Reading"))],
    ),
    (
        "thermal.v1.Reading",
        &[
            ("sensor", Shape::One, Form::Text),
            ("tempC", Shape::One, Form::Integer),
            ("temp_c", Shape::One, Form::Integer),
        ],
    ),
    (
        "keryx.refusals.Probe",
        &[
            ("count", Shape::One, Form::Integer),
            ("stamp", Shape::One, Form::Integer),
            ("label", Shape::One, Form::Text),
            ("kind", Shape::One, Form::Enum),
            ("tags", Shape::Keyed(Form::Text), Form::Integer),
            ("ratio", Shape::One, Form::Message("keryx.refusals.Ratio")),
        ],
    ),
    (
        "keryx.refusals.Ratio",
        &[("value", Shape::One, Form::Float)],
    ),
    (
        "keryx.maps.Inventory",
        &[
            ("counts", Shape::Keyed(Form::Text), Form::Integer),
            (
                "items",
                Shape::Keyed(Form::Integer),
                Form::Message("keryx.maps.Item"),
            ),
        ],
    ),
    ("keryx.maps.Item", &[("sku", Shape::One, Form::Text)]),
    (
        "keryx.rec.Tree",
        &[
            ("label", Shape::One, Form::Text),
            ("children", Shape::Many, Form::Message("keryx.rec.Tree")),
        ],
    ),
    (
        "keryx.rec.A",
        &[("b", Shape::One, Form::Message("keryx.rec.B"))],
    ),
    (
        "keryx.rec.B",
        &[("a", Shape::One, Form::Message("keryx.rec.A"))],
    ),
    (
        "keryx.scalars.Sample",
        &[
            ("count", Shape::One, Form::Integer),
            ("total", Shape::One, Form::Integer),
            ("checksum", Shape::One, Form::Integer),
            ("ratio", Shape::One, Form::Float),
            ("active", Shape::One, Form::Bool),
            ("payload", Shape::One, Form::Bytes),
            ("label", Shape::One, Form::Text),
            ("kind", Shape::One, Form::Enum),
            ("notes", Shape::Many, Form::Message("keryx.scalars.Note")),
            ("kinds", Shape::Many, Form::Enum),
            ("tags", Shape::Keyed(Form::Text), Form::Enum),
        ],
    ),
    ("keryx.scalars.Note", &[("text", Shape::One, Form::Text)]),
];

/// The tokens a stray lands as, in place of a member: a closer with nothing to close, an opener
/// never closed, a separator out of place, a lone quote or backslash, a comment (JSON has none), a
/// single-quoted string, a literal misspelled, a number the grammar refuses, a key with no value,
/// and a member no type declares.
const STRAYS: &[&str] = &[
    "}", "]", "{", "[", ",", ":", "\"", "\\", "/", "#", "// c", "/* c */", "'a'", "tru", "nul",
    "+1", "01", "1e", "0x1", "NaN", "\"a\":", "\"a\": 1",
];

/// The fields of the message `name` in [`MESSAGES`]. A name the table lacks is a broken
/// vocabulary — a mistyped entry — failed here, by name, rather than as an empty selection deep
/// in a strategy's construction.
fn fields_of(name: &str) -> &'static [Field] {
    MESSAGES
        .iter()
        .find(|(message, _)| *message == name)
        .map_or_else(
            || panic!("`{name}` is a message the vocabulary names"),
            |(_, fields)| *fields,
        )
}

/// One element of an object spelled in a vocabulary: a member — a field and its value's text, a
/// scalar literal, an array, or an object — or a stray token the grammar does not expect where it
/// lands.
#[derive(Clone, Debug)]
enum Node {
    Member(&'static str, String),
    Stray(&'static str),
}

/// `nodes` as a JSON object: the elements comma-separated between braces.
fn render(nodes: &[Node]) -> String {
    let mut text = String::from("{");
    for (at, node) in nodes.iter().enumerate() {
        if at > 0 {
            text.push_str(", ");
        }
        match node {
            Node::Member(name, value) => {
                text.push('"');
                text.push_str(name);
                text.push_str("\": ");
                text.push_str(value);
            }
            Node::Stray(token) => text.push_str(token),
        }
    }
    text.push('}');
    text
}

/// One of `spellings`, drawn uniformly.
fn one_of(spellings: &'static [&'static str]) -> impl Strategy<Value = String> {
    prop::sample::select(spellings).prop_map(str::to_owned)
}

/// An integer as the mapping spells one: a native non-negative number — every integer kind takes
/// it; a negative, which the unsigned kinds refuse at the decode; one from the range past the
/// native one, 2³¹ through 2³² − 1 — `ValueOutOfRange` where the walk reaches it on a 32-bit
/// unsigned kind, a decimal string on a 64-bit one (§6), the deserializer's refusal on an
/// `int32`; any `u64`; a number as a string of digits, which the mapping admits; and a spelling
/// no integer field takes — a fraction, a number the deserializer cannot hold, a word.
fn integer() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => (0..=i32::MAX).prop_map(|n| n.to_string()),
        2 => (i32::MIN..0).prop_map(|n| n.to_string()),
        3 => (2_147_483_648_u64..=4_294_967_295).prop_map(|n| n.to_string()),
        1 => any::<u64>().prop_map(|n| n.to_string()),
        1 => (0..=i32::MAX).prop_map(|n| format!("\"{n}\"")),
        1 => one_of(&["1.5", "1.0", "1e3", "-0", "1e400", "\"abc\"", "true", "null"]),
    ]
}

/// A float as the mapping spells one: a decimal with or without a fraction or an exponent; an
/// integer; an infinity or a NaN by the mapping's string spelling, or a finite value as a string;
/// and a spelling no float field takes — a bare `NaN`, a word, a number past the width. Every one
/// the walk reaches is `UnannotatedFloat`.
fn float() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => "-?[0-9]{1,3}(\\.[0-9]{1,3})?([eE]-?[0-9]{1,2})?",
        2 => any::<i32>().prop_map(|n| n.to_string()),
        1 => one_of(&["\"NaN\"", "\"Infinity\"", "\"-Infinity\"", "\"1.5\"", "\"1e2\""]),
        1 => one_of(&["NaN", "Infinity", "\"x\"", "1e400", "1.", ".5"]),
    ]
}

/// A string literal: the spellable alphabet, quoted; a NUL, by Unicode escape (`InteriorNul`);
/// another control character — a tab, a DEL, a C0 or a CR by escape, an ESC, a C1 escaped or raw
/// (`UnrepresentableText`); the escapes the walk carries — a quote, a backslash, a newline, a
/// solidus, a Unicode escape, a surrogate pair — non-ASCII text raw, and brackets as content; and
/// what the deserializer refuses — a raw control character, an escape it has no form for, a
/// literal never closed, a lone surrogate, single quotes.
fn string() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => "[a-zA-Z0-9 ]{0,10}".prop_map(|text| format!("\"{text}\"")),
        2 => one_of(&["\"\\u0000\"", "\"a\\u0000b\"", "\"\\u0000\\u0000\""]),
        2 => one_of(&[
            "\"a\\tb\"", "\"\\u007f\"", "\"\\u0001\"", "\"\\r\"", "\"\\u001b\"", "\"\\u0085\"",
            "\"\u{9f}\"",
        ]),
        2 => one_of(&[
            "\"\\\"\"", "\"\\\\\"", "\"\\n\"", "\"\\/\"", "\"\\u00e9\"", "\"é字\"",
            "\"\\ud83d\\ude00\"", "\"{}[]\"",
        ]),
        1 => one_of(&["\"\n\"", "\"\\q\"", "\"open", "\"\\ud800\"", "'single'"]),
    ]
}

/// A `bytes` value as the mapping spells one, a base64 string: the standard alphabet, padded; the
/// URL-safe alphabet, unpadded; empty; and what the deserializer refuses — text that is no
/// base64, a number, an array.
fn bytes_literal() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => one_of(&["\"\"", "\"AAEC\"", "\"3q2+7w==\"", "\"3q2-7w\"", "\"AA==\""]),
        1 => one_of(&["\"not base64!\"", "\"A\"", "12", "[1, 2]"]),
    ]
}

/// An enum value: a name or number both schemas' `Kind`s declare; a name one declares and the
/// other does not; a number neither declares — the deserializer passes it, the walk refuses it
/// (`UnknownEnumValue`); and what the deserializer refuses — a name neither declares, a number as
/// a string, a number past the width, a fraction — or leaves unset, `null`.
fn enum_value() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => one_of(&["\"KIND_UNSPECIFIED\"", "0", "1"]),
        1 => one_of(&["\"KIND_ONE\"", "\"KIND_FIRST\""]),
        3 => one_of(&["2", "7", "-1", "2147483647"]),
        1 => one_of(&["\"KIND_NONE\"", "\"2\"", "2147483648", "1.5", "null"]),
    ]
}

/// A boolean, in the two spellings the mapping takes, and ones it does not.
fn boolean() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => one_of(&["true", "false"]),
        1 => one_of(&["\"true\"", "1", "0", "null", "True"]),
    ]
}

/// A map key spelling an integer, as the mapping spells every key — a string: any `i64`; and
/// what the deserializer refuses for an integral key — a word, nothing, a fraction, a number past
/// the width.
fn integer_key() -> impl Strategy<Value = String> {
    prop_oneof![
        5 => any::<i64>().prop_map(|n| format!("\"{n}\"")),
        1 => one_of(&["\"x\"", "\"\"", "\"1.5\"", "\"9223372036854775808\"", "\"-0\""]),
    ]
}

/// A value of `form`, rendered: a scalar literal, or an object in *its* message's vocabulary
/// while `budget` allows, else the empty object.
fn value(form: Form, budget: u32) -> BoxedStrategy<String> {
    match form {
        Form::Integer => integer().boxed(),
        Form::Float => float().boxed(),
        Form::Text => string().boxed(),
        Form::Bytes => bytes_literal().boxed(),
        Form::Enum => enum_value().boxed(),
        Form::Bool => boolean().boxed(),
        Form::Message(child) if budget > 0 => body(child, budget - 1)
            .prop_map(|nodes| render(&nodes))
            .boxed(),
        Form::Message(_) => Just("{}".to_owned()).boxed(),
    }
}

/// A map key of `form`, rendered as the string the mapping spells every key as: a string
/// literal's own spellings for a `string` key — a NUL or a control character in it is the walk's
/// refusal at the map field (§7.2) — and an integer's for an integral one.
fn key(form: Form) -> BoxedStrategy<String> {
    match form {
        Form::Text => string().boxed(),
        Form::Integer | Form::Float | Form::Bytes | Form::Enum | Form::Bool | Form::Message(_) => {
            integer_key().boxed()
        }
    }
}

/// One field of `fields` as a member, its value in the field's shape: one value; an array of up
/// to two; or an object of up to two entries keyed as the field's keys are.
fn field(fields: &'static [Field], budget: u32) -> BoxedStrategy<Node> {
    prop::sample::select(fields)
        .prop_flat_map(move |(name, shape, form)| match shape {
            Shape::One => value(form, budget)
                .prop_map(move |value| Node::Member(name, value))
                .boxed(),
            Shape::Many => prop::collection::vec(value(form, budget), 0..3)
                .prop_map(move |values| Node::Member(name, format!("[{}]", values.join(", "))))
                .boxed(),
            Shape::Keyed(key_form) => {
                prop::collection::vec((key(key_form), value(form, budget)), 0..3)
                    .prop_map(move |entries| {
                        let entries: Vec<String> = entries
                            .iter()
                            .map(|(key, value)| format!("{key}: {value}"))
                            .collect();
                        Node::Member(name, format!("{{{}}}", entries.join(", ")))
                    })
                    .boxed()
            }
        })
        .boxed()
}

/// An object body in the vocabulary of the message `name`: up to four elements, each one of its
/// fields as a member with a value of the field's shape and form — nested objects `budget` levels
/// deep at most — or, rarely, a stray token the deserializer refuses.
fn body(name: &'static str, budget: u32) -> BoxedStrategy<Vec<Node>> {
    let element = prop_oneof![
        20 => field(fields_of(name), budget),
        1 => prop::sample::select(STRAYS).prop_map(Node::Stray),
    ];
    prop::collection::vec(element, 0..5).boxed()
}

/// Arbitrary bytes, fewer than `len`: the regime the deserializer refuses at a byte it has no
/// token for — non-UTF-8 among them — or at its first token.
fn bytes(len: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..len)
}

/// Arbitrary characters — control characters, NUL, and non-ASCII among them — as a text of fewer
/// than `len` characters: well-formed UTF-8 that is almost never JSON.
fn characters(len: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..len)
        .prop_map(|characters| characters.into_iter().collect())
}

/// An object spelled in one root's vocabulary, rendered: the share that deserializes as that root
/// and reaches the walk, or — with a stray token mixed in, or a value of a form the field refuses
/// — the deserializer's refusal on a well-formed neighbourhood.
fn spelled() -> impl Strategy<Value = String> {
    let roots: Vec<&'static str> = CODECS.iter().map(|(_, root)| *root).collect();
    prop::sample::select(roots)
        .prop_flat_map(|root| body(root, 3))
        .prop_map(|nodes| render(&nodes))
}

/// A spelled object with up to three arbitrary characters spliced in at a character boundary: a
/// token broken, a bracket or a quote added — the regime where the deserializer refuses partway
/// through a payload that was well formed.
fn spliced() -> impl Strategy<Value = String> {
    (spelled(), characters(4), any::<prop::sample::Index>()).prop_map(|(mut text, junk, at)| {
        let at = at.index(text.chars().count() + 1);
        let byte = text
            .char_indices()
            .nth(at)
            .map_or(text.len(), |(byte, _)| byte);
        text.insert_str(byte, &junk);
        text
    })
}

/// A chain nesting on either side of both boundaries, with an innermost body the deepest message
/// may or may not declare: the recursion fixture's singular pair from 90 to 130 levels — across
/// the walk's ceiling at 99 and the deserializer's count at 126 — and its repeated `children`
/// from 55 to 70, across the count at 63. Past the count it is the deserializer's refusal on every
/// codec; within it, the chain deserializes only on the codec rooted at its own message and with
/// a body that message declares — nothing, or a `label` for a tree — and is the deserializer's
/// refusal otherwise: another root, another codec, or a `sensor` neither message declares.
fn deep() -> impl Strategy<Value = String> {
    let innermost = || {
        prop::sample::select(vec![
            "",
            "\"label\": \"leaf\"",
            "\"sensor\": \"s\", \"tempC\": 1",
        ])
    };
    prop_oneof![
        ((CEILING - 9)..=(CONTAINERS + 3), innermost())
            .prop_map(|(levels, innermost)| nested(SINGULAR, false, levels, innermost)),
        ((REPEATED_DEEPEST - 8)..=(REPEATED_DEEPEST + 7), innermost())
            .prop_map(|(levels, innermost)| nested(REPEATED, true, levels, innermost)),
    ]
}

/// An arbitrary payload, in six regimes: arbitrary bytes, short and long; arbitrary characters;
/// an object spelled in a schema's vocabulary — nearly half the draws, the share that reaches the
/// walk; one spelled and then spliced; and a chain nesting about the boundaries.
fn payload() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        2 => bytes(64),
        1 => bytes(2048),
        1 => characters(64).prop_map(String::into_bytes),
        6 => spelled().prop_map(String::into_bytes),
        2 => spliced().prop_map(String::into_bytes),
        1 => deep().prop_map(String::into_bytes),
    ]
}

/// Whether `kind` is one the door's contract names for a JSON payload deserialized as a root the
/// schema declares: the decode's refusal, the walk's ceiling, and the walk's §6 refusals — never a
/// contained fault (none is known on this door) and never a kind of another door.
fn is_a_payload_refusal(kind: DiagnosticKind) -> bool {
    matches!(
        kind,
        DiagnosticKind::UndecodablePayload
            | DiagnosticKind::PayloadTooDeep
            | DiagnosticKind::ValueOutOfRange
            | DiagnosticKind::InteriorNul
            | DiagnosticKind::UnrepresentableText
            | DiagnosticKind::UnknownEnumValue
            | DiagnosticKind::UnannotatedFloat
    )
}

// The hostile-value generator over the refusals probe, checked against a model of the §6 policy.

/// One generated `Probe` (`refusals.proto`), as the binary instrument draws it: each field's
/// value from a strategy mixing the benign and the hostile, so the generator reaches every
/// refusal and every fact.
#[derive(Clone, Debug)]
struct Probe {
    count: u32,
    stamp: u32,
    label: String,
    kind: i32,
    tags: BTreeMap<String, i32>,
    ratio: Option<f32>,
}

/// How a probe's values are spelled, among the spellings the mapping admits for one value: an
/// integer as a JSON number or as a string of digits; a declared enum value by its name or by its
/// number; the members in declaration order or reversed. None is content: the facts and the
/// refusals are the probe's, whatever the spelling.
#[derive(Clone, Copy, Debug)]
struct Spelling {
    quoted_integers: bool,
    enum_by_name: bool,
    reversed: bool,
}

/// The largest `uint32`/`fixed32` a native clingo integer carries: `i32::MAX`, as the wire's `u32`.
fn top() -> u32 {
    u32::try_from(i32::MAX).expect("i32::MAX is a u32")
}

/// A 32-bit unsigned value: within the native range, just past it, at the width's maximum, or
/// anywhere.
fn unsigned() -> impl Strategy<Value = u32> {
    prop_oneof![
        4 => 0..=top(),
        1 => Just(top() + 1),
        1 => Just(u32::MAX),
        1 => any::<u32>(),
    ]
}

/// A short lowercase run on either side of one `character`.
fn around(character: impl Strategy<Value = char>) -> impl Strategy<Value = String> {
    ("[a-z]{0,4}", character, "[a-z]{0,4}")
        .prop_map(|(before, character, after)| format!("{before}{character}{after}"))
}

/// A string: the dialect's spellable alphabet (a newline, a quote, a backslash, non-ASCII text
/// included); one carrying a NUL; one carrying another control character — C0, DEL, C1; or
/// arbitrary characters, which fall on either side.
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        5 => r#"[a-zA-Z0-9 \n"\\é字]{0,10}"#,
        1 => around(Just('\0')),
        1 => around(prop::sample::select(vec![
            '\t', '\r', '\u{1b}', '\u{7f}', '\u{85}', '\u{9f}',
        ])),
        1 => prop::collection::vec(any::<char>(), 0..8)
            .prop_map(|characters| characters.into_iter().collect()),
    ]
}

/// An enum number: a declared one (`0`, `1`), or one the enum does not declare, on either side.
fn number() -> impl Strategy<Value = i32> {
    prop_oneof![
        5 => 0..=1i32,
        1 => 2..=i32::MAX,
        1 => i32::MIN..=-1,
    ]
}

/// The carried `Ratio`'s float, or none: a finite value well inside the width — so its decimal
/// spelling reads back inside the width too, and the deserializer admits it — or an infinity or
/// a NaN, which the mapping spells as strings. Every one the walk reaches is `UnannotatedFloat`.
fn ratio() -> impl Strategy<Value = Option<f32>> {
    prop_oneof![
        3 => Just(None),
        1 => prop_oneof![
            4 => (-1e30_f32..1e30).prop_map(Some),
            1 => Just(Some(f32::NAN)),
            1 => Just(Some(f32::INFINITY)),
            1 => Just(Some(f32::NEG_INFINITY)),
        ],
    ]
}

fn probe() -> impl Strategy<Value = Probe> {
    (
        unsigned(),
        unsigned(),
        text(),
        number(),
        prop::collection::btree_map(text(), any::<i32>(), 0..3),
        ratio(),
    )
        .prop_map(|(count, stamp, label, kind, tags, ratio)| Probe {
            count,
            stamp,
            label,
            kind,
            tags,
            ratio,
        })
}

fn spelling() -> impl Strategy<Value = Spelling> {
    (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(
        |(quoted_integers, enum_by_name, reversed)| Spelling {
            quoted_integers,
            enum_by_name,
            reversed,
        },
    )
}

/// A JSON string literal carrying `value`: a quote, a backslash, and every C0 control character
/// escaped — the grammar's requirement, not a choice; a raw control character is no JSON string
/// — and everything else raw, non-ASCII text, DEL, and the C1 range included.
fn json_string(value: &str) -> String {
    let mut text = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\t' => text.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                write!(text, "\\u{:04x}", u32::from(control))
                    .expect("writing to a string cannot fail");
            }
            other => text.push(other),
        }
    }
    text.push('"');
    text
}

/// A `float` as the mapping spells one: a JSON number when finite, else the mapping's string.
fn json_float(value: f32) -> String {
    if value.is_nan() {
        "\"NaN\"".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 {
            "\"Infinity\""
        } else {
            "\"-Infinity\""
        }
        .to_owned()
    } else {
        format!("{value:?}")
    }
}

/// The probe as canonical JSON under `spelling`: every field a member, the map's entries in key
/// order, the `Ratio` only when the probe holds one.
fn encode_json(probe: &Probe, spelling: Spelling) -> String {
    let integer = |value: u32| {
        if spelling.quoted_integers {
            format!("\"{value}\"")
        } else {
            value.to_string()
        }
    };
    let kind = match (spelling.enum_by_name, probe.kind) {
        (true, 0) => "\"KIND_UNSPECIFIED\"".to_owned(),
        (true, 1) => "\"KIND_ONE\"".to_owned(),
        (_, number) => number.to_string(),
    };
    let tags: Vec<String> = probe
        .tags
        .iter()
        .map(|(key, value)| format!("{}: {value}", json_string(key)))
        .collect();
    let mut members = vec![
        format!("\"count\": {}", integer(probe.count)),
        format!("\"stamp\": {}", integer(probe.stamp)),
        format!("\"label\": {}", json_string(&probe.label)),
        format!("\"kind\": {kind}"),
        format!("\"tags\": {{{}}}", tags.join(", ")),
    ];
    if let Some(ratio) = probe.ratio {
        members.push(format!("\"ratio\": {{\"value\": {}}}", json_float(ratio)));
    }
    if spelling.reversed {
        members.reverse();
    }
    format!("{{{}}}", members.join(", "))
}

/// The §6 text refusal for `value`, as the policy decides it: `InteriorNul` for a NUL anywhere,
/// else `UnrepresentableText` for any other control character but `\n`, else none.
fn text_refusal(value: &str) -> Option<DiagnosticKind> {
    if value.contains('\0') {
        Some(DiagnosticKind::InteriorNul)
    } else if value
        .chars()
        .any(|character| character != '\n' && character.is_control())
    {
        Some(DiagnosticKind::UnrepresentableText)
    } else {
        None
    }
}

/// The refusals `probe` must draw, in the order the walk collects them: the probe's fields in
/// number order (a map's entries in key order), then the carried `Ratio`'s float — a child is
/// walked after its parent's fields.
fn expected_refusals(probe: &Probe) -> Vec<(DiagnosticKind, Option<&'static str>)> {
    let mut expected = Vec::new();
    if probe.count > top() {
        expected.push((
            DiagnosticKind::ValueOutOfRange,
            Some("keryx.refusals.Probe.count"),
        ));
    }
    if probe.stamp > top() {
        expected.push((
            DiagnosticKind::ValueOutOfRange,
            Some("keryx.refusals.Probe.stamp"),
        ));
    }
    if let Some(kind) = text_refusal(&probe.label) {
        expected.push((kind, Some("keryx.refusals.Probe.label")));
    }
    if !(0..=1).contains(&probe.kind) {
        expected.push((
            DiagnosticKind::UnknownEnumValue,
            Some("keryx.refusals.Probe.kind"),
        ));
    }
    for key in probe.tags.keys() {
        if let Some(kind) = text_refusal(key) {
            expected.push((kind, Some("keryx.refusals.Probe.tags")));
        }
    }
    if probe.ratio.is_some() {
        expected.push((
            DiagnosticKind::UnannotatedFloat,
            Some("keryx.refusals.Ratio.value"),
        ));
    }
    expected
}

/// The `.lp` spelling of a string the policy admits: the dialect's three escapes.
fn lp_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

/// The facts a benign `probe` shreds to, as `.lp` lines in no particular order: the sort atom, one
/// fact per field, one per map entry.
fn expected_facts(probe: &Probe) -> Vec<String> {
    let mut facts = vec![
        "probe(r0).".to_owned(),
        format!("count(r0, {}).", probe.count),
        format!("stamp(r0, {}).", probe.stamp),
        format!("label(r0, {}).", lp_string(&probe.label)),
        format!(
            "kind(r0, {}).",
            if probe.kind == 0 {
                "unspecified"
            } else {
                "one"
            }
        ),
    ];
    facts.extend(
        probe
            .tags
            .iter()
            .map(|(key, value)| format!("tags(r0, {}, {value}).", lp_string(key))),
    );
    facts
}

/// The kinds and loci of `diagnostics`, in the order collected.
fn located(diagnostics: &Diagnostics) -> Vec<(DiagnosticKind, Option<&str>)> {
    diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.kind(), diagnostic.locus().path()))
        .collect()
}

proptest! {
    // The default case count, not the binary instrument's 1024: each case, shredded against six
    // codecs, spawns the decode thread the door sizes for the deserializer's admit six times over
    // — the door's one cost the binary form does not pay — so the count is held where the suite
    // stays quick; the fixed-seed tally below pins that the generator reaches every outcome
    // within it.
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn shred_is_total_over_arbitrary_json(payload in payload()) {
        // Facts or `Diagnostics`, never a panic — against every codec, so the payload that
        // deserializes as one root reaches that schema's forms and kinds. Facts render: the §6
        // policy refused every value the dialect cannot spell before it was built into one. A
        // refusal is one the door's contract names; a `DependencyFault` would be a found trigger
        // for the containment frame the module doc records as having none.
        for (codec, root_type) in CODECS.iter() {
            match shred(codec, root_type, &payload, PayloadFormat::Json) {
                Ok(facts) => prop_assert!(facts.render().is_ok(), "the facts render"),
                Err(diagnostics) => {
                    for diagnostic in diagnostics.iter() {
                        prop_assert!(
                            is_a_payload_refusal(diagnostic.kind()),
                            "a refusal the door's contract names, not {diagnostic}"
                        );
                    }
                }
            }
            prop_assert!(
                !keryx_core::is_containing(),
                "the door leaves no containment frame live on the thread that called it"
            );
        }
    }

    #[test]
    fn every_hostile_value_draws_exactly_its_refusal_and_every_benign_one_its_fact(
        (probe, spelling) in (probe(), spelling())
    ) {
        // The policy modelled, under any spelling the mapping admits: the shred is refused
        // exactly when the model refuses, with exactly the model's refusals in the walk's order;
        // else the facts are exactly the model's.
        let expected = expected_refusals(&probe);
        let json = encode_json(&probe, spelling);
        match shred_json(&PROBE, "keryx.refusals.Probe", &json) {
            Ok(facts) => {
                prop_assert!(
                    expected.is_empty(),
                    "shredded, yet §6 refuses {expected:?}: {json}"
                );
                let rendered = facts.render().expect("the facts render");
                let mut lines: Vec<&str> = rendered.lines().collect();
                lines.sort_unstable();
                let mut wanted = expected_facts(&probe);
                wanted.sort_unstable();
                prop_assert_eq!(lines, wanted.iter().map(String::as_str).collect::<Vec<&str>>());
                prop_assert_eq!(facts.symbols().len(), 5 + probe.tags.len());
            }
            Err(diagnostics) => {
                prop_assert_eq!(located(&diagnostics), expected, "{}", json);
            }
        }
        prop_assert!(!keryx_core::is_containing());
    }

    #[test]
    fn arbitrary_json_shreds_identically_twice(payload in payload()) {
        // Whatever the payload — facts or a refusal — two shreds agree on every seam: the symbols
        // and the rendering, each fact once; or the diagnostics, kind for kind and locus for
        // locus.
        for (codec, root_type) in CODECS.iter() {
            match (
                shred(codec, root_type, &payload, PayloadFormat::Json),
                shred(codec, root_type, &payload, PayloadFormat::Json),
            ) {
                (Ok(first), Ok(again)) => {
                    prop_assert_eq!(first.symbols(), again.symbols());
                    let rendered = first.render().expect("renders");
                    prop_assert_eq!(&rendered, &again.render().expect("renders"));
                    prop_assert_eq!(rendered.lines().count(), first.symbols().len());
                }
                (Err(first), Err(again)) => prop_assert_eq!(first, again),
                (first, again) => {
                    return Err(TestCaseError::fail(format!(
                        "one shred succeeded and the other did not: {first:?} / {again:?}"
                    )));
                }
            }
        }
    }
}

#[test]
fn a_benign_probe_shreds_to_its_facts_and_a_hostile_one_draws_every_refusal_in_walk_order() {
    // Benign — every field carried at a spellable value, the map's keys in order, the enum by
    // its name: the facts, exactly, on the `.lp` seam, and exactly the facts the same probe
    // shreds to from the wire.
    let benign = shred_json(
        &PROBE,
        "keryx.refusals.Probe",
        r#"{"count": 7, "stamp": 2147483647, "label": "ok \"quoted\"\\\n", "kind": "KIND_ONE",
            "tags": {"a": -1, "b": 2}}"#,
    )
    .expect("shreds");
    let rendered = assert_once_each(&benign);
    assert_eq!(
        rendered,
        "count(r0, 7).\n\
         kind(r0, one).\n\
         label(r0, \"ok \\\"quoted\\\"\\\\\\n\").\n\
         probe(r0).\n\
         stamp(r0, 2147483647).\n\
         tags(r0, \"a\", -1).\n\
         tags(r0, \"b\", 2).\n"
    );
    let mut wire_probe = Vec::new();
    wire::uint32(1, 7, &mut wire_probe);
    wire::fixed32(2, top(), &mut wire_probe);
    delimited(3, b"ok \"quoted\"\\\n", &mut wire_probe);
    wire::int32(4, 1, &mut wire_probe);
    for (key, value) in [("a", -1), ("b", 2)] {
        let mut entry = Vec::new();
        delimited(1, key.as_bytes(), &mut entry);
        wire::int32(2, value, &mut entry);
        delimited(5, &entry, &mut wire_probe);
    }
    let from_wire = shred(
        &PROBE,
        "keryx.refusals.Probe",
        &wire_probe,
        PayloadFormat::Binary,
    )
    .expect("shreds");
    assert_eq!(benign.symbols(), from_wire.symbols());

    // Hostile — every refusal at once, one per value, in the spellings the mapping gives them: a
    // count past the native range as a number and, again, as a string of digits; a stamp at the
    // width's maximum; a tab in the label; an enum number neither constant declares; a NUL and
    // an ESC in two map keys by Unicode escape; a `NaN` by the mapping's string. Collected in the
    // walk's order — the fields in number order, the map's entries in key order, then the carried
    // child's float — and no facts beside them; the spelling of a refused value is not content
    // either.
    for count in ["2147483648", "\"2147483648\""] {
        let hostile = format!(
            r#"{{"count": {count}, "stamp": 4294967295, "label": "a\tb", "kind": 2,
                "tags": {{"x\u0000": 1, "y\u001b": 2, "z": 3}}, "ratio": {{"value": "NaN"}}}}"#
        );
        let diagnostics =
            shred_json(&PROBE, "keryx.refusals.Probe", &hostile).expect_err("refused");
        assert_eq!(
            located(&diagnostics),
            [
                (
                    DiagnosticKind::ValueOutOfRange,
                    Some("keryx.refusals.Probe.count")
                ),
                (
                    DiagnosticKind::ValueOutOfRange,
                    Some("keryx.refusals.Probe.stamp")
                ),
                (
                    DiagnosticKind::UnrepresentableText,
                    Some("keryx.refusals.Probe.label")
                ),
                (
                    DiagnosticKind::UnknownEnumValue,
                    Some("keryx.refusals.Probe.kind")
                ),
                (
                    DiagnosticKind::InteriorNul,
                    Some("keryx.refusals.Probe.tags")
                ),
                (
                    DiagnosticKind::UnrepresentableText,
                    Some("keryx.refusals.Probe.tags")
                ),
                (
                    DiagnosticKind::UnannotatedFloat,
                    Some("keryx.refusals.Ratio.value")
                ),
            ],
            "{count}"
        );
    }
}

#[test]
fn the_json_generator_reaches_every_outcome_of_the_door() {
    // The generator is not vacuous — pinned at a fixed seed, so the shares are reproducible: over
    // a run of draws shredded against every codec, payloads reach the walk and shred to facts;
    // are refused by the deserializer (`UndecodablePayload`), by the walk's ceiling
    // (`PayloadTooDeep` — the singular chain between 100 and 126 levels, on the codec rooted at
    // `A`), and by the walk at each of its §6 refusals — so the property above is checked at
    // every step of the door, not at the first alone. The sample holds the door's contract as
    // the property does — facts render, a refusal is one the contract names — so it is that check
    // too, made deterministic beside the property's re-rolled cases.
    let mut runner = TestRunner::deterministic();
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    for _ in 0..2048 {
        let payload = payload().new_tree(&mut runner).expect("a draw").current();
        for (codec, root_type) in CODECS.iter() {
            match shred(codec, root_type, &payload, PayloadFormat::Json) {
                Ok(facts) => {
                    assert!(facts.render().is_ok(), "the facts render");
                    *tally.entry("facts".to_owned()).or_default() += 1;
                }
                Err(diagnostics) => {
                    for diagnostic in diagnostics.iter() {
                        assert!(
                            is_a_payload_refusal(diagnostic.kind()),
                            "a refusal the door's contract names, not {diagnostic}"
                        );
                        *tally.entry(format!("{:?}", diagnostic.kind())).or_default() += 1;
                    }
                }
            }
        }
    }
    for outcome in [
        "facts",
        "UndecodablePayload",
        "PayloadTooDeep",
        "ValueOutOfRange",
        "InteriorNul",
        "UnrepresentableText",
        "UnknownEnumValue",
        "UnannotatedFloat",
    ] {
        assert!(
            tally.get(outcome).is_some_and(|&count| count > 0),
            "the generator never reaches `{outcome}`: {tally:?}"
        );
    }
}
