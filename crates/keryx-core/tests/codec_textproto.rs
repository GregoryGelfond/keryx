//! The inbound codec's textproto form (spec §26; §11, §22), instrumented as its binary form is
//! (`codec_totality.rs`, `codec_depth.rs`): a payload in the protobuf text format shreds through
//! the same `Codec`, the same walk, and the same §6 policy as its binary form, to the same facts.
//!
//! **Parity.** The example's committed `batch.txtpb` — the §28 thermal batch written as text —
//! yields, on both delivery seams, exactly what `batch.binpb` yields and what the committed golden
//! holds (§27: the example documents the text form as it documents the wire form, and regresses it
//! the same way); and the format's alternative spellings of one message are one shred.
//!
//! **The door's own refusals** are diagnoses at the whole-payload locus, before any fact: a payload
//! that is not UTF-8 (the text format is text) and one that does not parse as the root type are
//! `UndecodablePayload`, naming the type and no byte of the payload.
//!
//! **Bounded depth, branch (b)** (spec §8; the threat model's property 3). The engine's text
//! parser recurses natively and bounds nothing, so on this door the uniform ceiling binds *before*
//! the parser, in the decode's pre-parse guard: 99 nested message values are admitted, parsed, and
//! shredded; 100 are `PayloadTooDeep` before the parser sees a token, and so is any depth beyond,
//! with no call stack spent on the way. The deepest admitted parse is this suite's proof of the
//! thread the decode sizes for it (`engine::TEXTPROTO_PARSE_STACK`): in a debug build, 99 nested
//! message values need more parser stack than the test harness's own threads carry, so the parse
//! completes under `cargo test` only because the decode runs it on a stack it sized itself. And
//! the guard's premise — that its scanner ends a `#` comment at the byte the engine's lexer does,
//! `\n` and no other — is held through the door: a payload that is one comment over a thousand
//! nested message values, with no `\n` in it, is admitted and shredded as the empty message,
//! where a lexer ending the comment at a byte the scanner does not would hand the parser every
//! level the guard never measured.
//!
//! **Totality over arbitrary text.** `Codec::shred` returns facts or typed `Diagnostics` over *any*
//! text — never a panic, an abort, or a hang — checked over a generator mixing arbitrary characters
//! with text spelled in the schemas' own vocabularies, so a meaningful share passes the guard,
//! parses, and reaches the walk and every one of its refusals; a fixed-seed tally pins that reach.
//! As on the binary door, no generated text trips the containment frame — a `DependencyFault`
//! would be a found trigger, to be recorded, not hidden — and the door leaves no frame live on the
//! thread that called it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use keryx_test_support as support;
use keryx_test_support::wire::{batch, delimited, reading};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

use keryx_core::codec::{Codec, Facts, PayloadFormat, Root};
use keryx_core::diagnostics::{DiagnosticKind, Diagnostics};

/// The uniform payload nesting ceiling (`codec::walk::NESTING_CEILING`, the guard's bound): one
/// below the engine's binary decode recursion limit of 100, every format alike.
const CEILING: usize = 99;

const BRACES: (char, char) = ('{', '}');
const ANGLES: (char, char) = ('<', '>');

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

/// The codecs the arbitrary-text generator shreds against, each with the root it parses as — the
/// binary totality instrument's five: the thermal story (§28), the refusals probe, maps, a
/// recursive tree, and the scalar-treatment sample. Built once: a codec is per schema, never per
/// payload.
static CODECS: LazyLock<Vec<(Codec, &'static str)>> = LazyLock::new(|| {
    vec![
        (thermal_codec(), "thermal.v1.ReadingBatch"),
        (fixture_codec("refusals.proto"), "keryx.refusals.Probe"),
        (fixture_codec("maps.proto"), "keryx.maps.Inventory"),
        (fixture_codec("recursion.proto"), "keryx.rec.Tree"),
        (
            fixture_codec("scalar_treatment.proto"),
            "keryx.scalars.Sample",
        ),
    ]
});

/// A payload in the form `format` shredded as `root_type` from the fresh root `r0`.
fn shred(
    codec: &Codec,
    root_type: &str,
    payload: &[u8],
    format: PayloadFormat,
) -> Result<Facts, Diagnostics> {
    codec.shred(root_type, payload, format, &Root::fresh(0))
}

/// A textproto payload shredded as `root_type` from the fresh root `r0`.
fn shred_text(codec: &Codec, root_type: &str, text: &str) -> Result<Facts, Diagnostics> {
    shred(codec, root_type, text.as_bytes(), PayloadFormat::Textproto)
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

/// A textproto payload refused as `root_type`: its one diagnosis, at the whole-payload locus.
fn refused(codec: &Codec, root_type: &str, payload: &[u8]) -> (DiagnosticKind, String) {
    let diagnostics = shred(codec, root_type, payload, PayloadFormat::Textproto)
        .expect_err("the payload is refused");
    let (kind, detail) = the_one_refusal(&diagnostics);
    (kind, detail.to_owned())
}

/// The committed text-format example (`examples/thermal/batch.txtpb`): the §28 batch written as
/// text — the same message `batch.binpb` carries on the wire — with the format's header
/// convention naming its schema and root type in comments.
fn txtpb() -> String {
    std::fs::read_to_string(thermal_dir().join("batch.txtpb")).expect("payload present")
}

/// The committed facts of the §28 batch (`examples/thermal/gen/thermal.v1.facts.lp`), the golden
/// the binary example is held to.
fn golden() -> String {
    std::fs::read_to_string(thermal_dir().join("gen/thermal.v1.facts.lp")).expect("golden present")
}

#[test]
fn the_committed_txtpb_example_shreds_to_the_facts_the_committed_binpb_does() {
    // §26 parity on the spec's own payload (§28), through the example's two committed forms: the
    // text form and the wire form of one message are one shred — the same symbols in
    // `Symbol::Ord` on the library seam, the same `.lp` text on the CLI seam, and both the
    // committed golden the binary example is held to.
    let codec = thermal_codec();
    let text = txtpb();
    assert!(
        text.contains("# proto-message: thermal.v1.ReadingBatch"),
        "the example names its own root type in the format's header comment: {text}"
    );
    let from_text = shred_text(&codec, "ReadingBatch", &text).expect("the example shreds");

    let binary = std::fs::read(thermal_dir().join("batch.binpb")).expect("payload present");
    assert_eq!(
        binary,
        batch(&[reading("s-101", 44), reading("s-107", 21)]),
        "the committed payload is the §28 batch on the wire"
    );
    let from_binary = shred(&codec, "ReadingBatch", &binary, PayloadFormat::Binary)
        .expect("the binary payload shreds");
    assert_eq!(from_text.symbols(), from_binary.symbols());

    let rendered = from_text.render().expect("the facts render");
    assert_eq!(rendered, from_binary.render().expect("the facts render"));
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

    // An empty text is the empty message, as an empty wire is: the root's sort atom alone.
    let empty = shred_text(&codec, "ReadingBatch", "").expect("the empty text shreds");
    assert_eq!(
        empty.symbols(),
        shred(&codec, "ReadingBatch", &[], PayloadFormat::Binary)
            .expect("the empty wire shreds")
            .symbols()
    );
    assert_eq!(empty.render().expect("renders"), "reading_batch(r0).\n");
}

#[test]
fn the_format_s_alternative_spellings_of_one_message_are_one_shred() {
    // The text format spells one message many ways — `{ }` or `< >` around a message value, a
    // repeated field as repeated occurrences or as one `[ ]` list, the fields in any order, a `:`
    // before a message value or none, either quote around a string, comments and whitespace
    // anywhere — and none of it is content: each spelling of the §28 batch shreds to the
    // example's facts.
    let codec = thermal_codec();
    let example = shred_text(&codec, "ReadingBatch", &txtpb()).expect("the example shreds");
    for spelling in [
        "readings: [{ sensor: \"s-101\" temp_c: 44 }, { sensor: \"s-107\" temp_c: 21 }]",
        "readings < temp_c: 44 sensor: \"s-101\" > readings: < temp_c: 21 sensor: \"s-107\" >",
        "# two readings\nreadings {\n  sensor: 's-101' # the sensor\n  temp_c: 44\n}\n\
         readings{sensor:\"s-\" \"107\"temp_c:21}\n",
    ] {
        let facts = shred_text(&codec, "ReadingBatch", spelling).expect("the spelling shreds");
        assert_eq!(facts.symbols(), example.symbols(), "{spelling}");
    }
}

#[test]
fn a_textproto_payload_that_is_not_utf_8_is_undecodable_at_the_whole_payload_locus() {
    // The text format is UTF-8 text: a payload that is not — here a Latin-1 `é` inside the
    // sensor's string — is refused before the engine sees it, `UndecodablePayload` at the
    // whole-payload locus naming the root type and the failure's position, never its bytes.
    let codec = thermal_codec();
    let (kind, detail) = refused(
        &codec,
        "ReadingBatch",
        b"readings { sensor: \"s-\xe9\" temp_c: 44 }",
    );
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
        let (kind, detail) = refused(&codec, "ReadingBatch", payload);
        assert_eq!(kind, DiagnosticKind::UndecodablePayload);
        assert!(
            detail.contains("thermal.v1.ReadingBatch"),
            "the detail names the root type: {detail}"
        );
    }
}

// The depth boundary through the door (the guard's unit tests pin the measure; these pin the
// door): the recursion fixture's `Tree` nests through its repeated `children`.

/// A textproto nesting the message-typed field `field` `levels` deep through the bracket pair
/// `open`/`close`, with `innermost` as the deepest message's body:
/// `field open field open … innermost close close` — for `children`, `levels` trees below the
/// root, each holding the next.
fn nested(field: &str, (open, close): (char, char), levels: usize, innermost: &str) -> String {
    let mut text = String::new();
    for _ in 0..levels {
        text.push_str(field);
        text.push(' ');
        text.push(open);
        text.push(' ');
    }
    text.push_str(innermost);
    for _ in 0..levels {
        text.push(' ');
        text.push(close);
    }
    text
}

/// The same chain on the binary wire: `levels` trees nested through `children` (#2), each
/// carrying nothing but the next and the innermost nothing at all, built from the inside out.
fn chain(levels: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..levels {
        let mut outer = Vec::new();
        delimited(2, &bytes, &mut outer);
        bytes = outer;
    }
    bytes
}

/// The recursion fixture's codec: `Tree` nests through its repeated `children`.
fn tree_codec() -> Codec {
    fixture_codec("recursion.proto")
}

#[test]
fn a_textproto_at_the_ceiling_parses_and_shreds_whole_and_one_past_it_is_refused() {
    let codec = tree_codec();

    // 99 levels: the guard admits it, the engine parses it, and the walk shreds it — the root
    // and 99 nested trees, each its sort atom and its materialised label, the deepest occupant
    // 99 `children` applications below `r0`, every fact once on both seams — and exactly the
    // facts the same chain on the binary wire shreds to: parity at the ceiling. The parse runs
    // on the thread the decode sizes for this depth (`engine::TEXTPROTO_PARSE_STACK`): in a
    // debug build, 99 nested message values need some 2.5 MB of parser stack, more than the
    // 2 MB thread this harness runs a test on, so this parse completes under `cargo test` only
    // because the decode runs it on a stack it sized itself — the construction this test is the
    // committed proof of.
    let facts = shred_text(
        &codec,
        "keryx.rec.Tree",
        &nested("children", BRACES, CEILING, ""),
    )
    .expect("the deepest admitted payload parses and shreds");
    assert_eq!(facts.symbols().len(), 2 * (CEILING + 1));
    let rendered = facts.render().expect("renders");
    assert_eq!(rendered.lines().count(), facts.symbols().len());
    assert_eq!(
        rendered
            .lines()
            .map(|line| line.matches("children(").count())
            .max(),
        Some(CEILING)
    );
    let from_binary = shred(
        &codec,
        "keryx.rec.Tree",
        &chain(CEILING),
        PayloadFormat::Binary,
    )
    .expect("the binary chain shreds");
    assert_eq!(facts.symbols(), from_binary.symbols());

    // Through `< >`: one message value to the parser, one level to the guard — the same facts.
    let angled = shred_text(
        &codec,
        "keryx.rec.Tree",
        &nested("children", ANGLES, CEILING, ""),
    )
    .expect("the deepest admitted payload parses through either bracket pair");
    assert_eq!(angled.symbols(), facts.symbols());

    // 100 levels: `PayloadTooDeep` — once, at the whole-payload locus, naming the depth and the
    // ceiling and nothing of the payload, through either bracket pair. The guard's refusal, not
    // the walk's: the walk's names the over-deep sort, and this one names none, the walk never
    // having run.
    for pair in [BRACES, ANGLES] {
        let (kind, detail) = refused(
            &codec,
            "keryx.rec.Tree",
            nested("children", pair, CEILING + 1, "").as_bytes(),
        );
        assert_eq!(kind, DiagnosticKind::PayloadTooDeep);
        assert!(
            detail.contains("100") && detail.contains("99"),
            "the detail names the depth and the ceiling: {detail}"
        );
        assert!(
            !detail.contains("keryx.rec.Tree") && !detail.contains("children"),
            "the detail names no sort and echoes nothing of the payload: {detail}"
        );
    }
}

#[test]
fn the_guard_measures_before_the_parser_and_exactly_as_the_parser_would() {
    let codec = tree_codec();

    // The order of the door's steps, observed from outside: a field the type does not declare,
    // nested 99 deep, passes the guard and fails the parser — `UndecodablePayload`, the engine's
    // refusal, naming the root type — while the same field 100 deep is `PayloadTooDeep`: the
    // guard measured and refused before the parser saw a token, or the refusal would have been
    // the parser's.
    let (kind, detail) = refused(
        &codec,
        "keryx.rec.Tree",
        nested("secret", BRACES, CEILING, "").as_bytes(),
    );
    assert_eq!(kind, DiagnosticKind::UndecodablePayload);
    assert!(
        detail.contains("keryx.rec.Tree"),
        "the parser's refusal names the root type: {detail}"
    );
    let (kind, detail) = refused(
        &codec,
        "keryx.rec.Tree",
        nested("secret", BRACES, CEILING + 1, "").as_bytes(),
    );
    assert_eq!(kind, DiagnosticKind::PayloadTooDeep);
    assert!(
        !detail.contains("secret"),
        "the guard's refusal echoes nothing of the payload: {detail}"
    );

    // And exact: a bracket inside the deepest literal, or in a comment, is content, not a level
    // — the payload at the ceiling still passes the guard, parses, and carries the literal.
    let facts = shred_text(
        &codec,
        "keryx.rec.Tree",
        &nested("children", BRACES, CEILING, "label: \"{<\" # {\n"),
    )
    .expect("a bracket in a literal or a comment is not a level");
    assert_eq!(facts.symbols().len(), 2 * (CEILING + 1));
    assert!(
        facts.render().expect("renders").contains(", \"{<\").\n"),
        "the deepest tree carries the literal"
    );
}

#[test]
fn a_comment_the_guard_passes_over_is_a_comment_to_the_parser_to_the_same_byte() {
    // The guard's dominance rests on its scanner ending a `#` comment where the engine's lexer
    // does — at `\n`, and at no other byte (`#[^\n]*\n?`, prost-reflect 0.16.5
    // `src/dynamic/text_format/parse/lex.rs:10`; `codec::guard`'s tests pin the scanner to it).
    // Through the door, the premise itself: a payload that is one `#` comment opening with a
    // carriage return, a Unicode line separator, or a next-line character, then `children`
    // nested a thousand levels deep, and no `\n` anywhere — so to scanner and lexer alike every
    // bracket is comment content. The guard measures depth 0 and admits it; the parser skips it
    // whole and yields the empty tree — the facts the empty text and the empty wire shred to.
    // Were the lexer to end a comment at a byte the scanner does not, the parser would see a
    // thousand nested message values the guard never measured and recurse on every one — some
    // three times the depth the sized parse thread carries in a debug build
    // (`engine::TEXTPROTO_PARSE_STACK` records the measure) — and this test would abort the
    // harness rather than fail, no frame catching an overflow; in a release build, whose stack
    // carries that depth, the parse would complete and the shred would be the walk's
    // `PayloadTooDeep`, not these facts. Its passing is the premise, held on the pinned engine.
    let codec = tree_codec();
    let empty = shred_text(&codec, "keryx.rec.Tree", "").expect("the empty text shreds");
    assert_eq!(
        empty.symbols(),
        shred(&codec, "keryx.rec.Tree", &[], PayloadFormat::Binary)
            .expect("the empty wire shreds")
            .symbols()
    );
    for line_break in ["\r", "\u{2028}", "\u{85}"] {
        let payload = format!("# {line_break}{}", nested("children", BRACES, 1_000, ""));
        assert!(
            !payload.contains('\n'),
            "the whole payload is one comment: no newline"
        );
        let facts = shred_text(&codec, "keryx.rec.Tree", &payload)
            .expect("a payload that is one comment is the empty message");
        assert_eq!(facts.symbols(), empty.symbols(), "{line_break:?}");
        assert!(
            !facts.render().expect("renders").contains("children("),
            "nothing nested was parsed: {line_break:?}"
        );
    }
}

#[test]
fn far_past_the_ceiling_the_guard_refuses_with_no_call_stack_spent() {
    // 101 levels and far beyond: the guard's one pass over the bytes measures the nesting and
    // refuses — `PayloadTooDeep` at the whole-payload locus, naming the depth and the ceiling —
    // and the parser, which would recurse once per level, never runs. A payload a hundred
    // thousand levels deep (some 1.3 MB of text) is refused the same way, with no call stack to
    // exhaust on the way.
    let codec = tree_codec();
    for levels in [CEILING + 2, 1_000, 10_000, 100_000] {
        let (kind, detail) = refused(
            &codec,
            "keryx.rec.Tree",
            nested("children", BRACES, levels, "").as_bytes(),
        );
        assert_eq!(kind, DiagnosticKind::PayloadTooDeep, "{levels}");
        assert!(
            detail.contains(&levels.to_string()) && detail.contains("99"),
            "the detail names the depth and the ceiling: {detail}"
        );
    }
}

// Totality over arbitrary text: the generator, in the schemas' own vocabularies and outside them.

/// How a field's value is spelled in the text format: an integer, a float, a quoted string, an
/// enum value (a name or a number), a boolean, or a message value over the fields of the message
/// named. Each form's strategy draws values the walk carries, values it refuses (§6), and, at a
/// low weight, spellings the parser refuses — so a text spelled in a vocabulary mostly parses,
/// and the walk's refusals are reached often, not rarely.
#[derive(Clone, Copy, Debug)]
enum Form {
    Integer,
    Float,
    Text,
    Enum,
    Bool,
    Message(&'static str),
}

/// The messages of the codecs' schemas as the text format spells their fields — the vocabulary a
/// text is drawn in so it can parse as one of the five roots: each root and every message
/// reachable from it, a map's entries as the `key`/`value` messages the text format spells them
/// as.
const MESSAGES: &[(&str, &[(&str, Form)])] = &[
    (
        "thermal.v1.ReadingBatch",
        &[("readings", Form::Message("thermal.v1.Reading"))],
    ),
    (
        "thermal.v1.Reading",
        &[("sensor", Form::Text), ("temp_c", Form::Integer)],
    ),
    (
        "keryx.refusals.Probe",
        &[
            ("count", Form::Integer),
            ("stamp", Form::Integer),
            ("label", Form::Text),
            ("kind", Form::Enum),
            ("tags", Form::Message("keryx.refusals.Probe.TagsEntry")),
            ("ratio", Form::Message("keryx.refusals.Ratio")),
        ],
    ),
    (
        "keryx.refusals.Probe.TagsEntry",
        &[("key", Form::Text), ("value", Form::Integer)],
    ),
    ("keryx.refusals.Ratio", &[("value", Form::Float)]),
    (
        "keryx.maps.Inventory",
        &[
            ("counts", Form::Message("keryx.maps.Inventory.CountsEntry")),
            ("items", Form::Message("keryx.maps.Inventory.ItemsEntry")),
        ],
    ),
    (
        "keryx.maps.Inventory.CountsEntry",
        &[("key", Form::Text), ("value", Form::Integer)],
    ),
    (
        "keryx.maps.Inventory.ItemsEntry",
        &[
            ("key", Form::Integer),
            ("value", Form::Message("keryx.maps.Item")),
        ],
    ),
    ("keryx.maps.Item", &[("sku", Form::Text)]),
    (
        "keryx.rec.Tree",
        &[
            ("label", Form::Text),
            ("children", Form::Message("keryx.rec.Tree")),
        ],
    ),
    (
        "keryx.scalars.Sample",
        &[
            ("count", Form::Integer),
            ("total", Form::Integer),
            ("checksum", Form::Integer),
            ("ratio", Form::Float),
            ("active", Form::Bool),
            ("payload", Form::Text),
            ("label", Form::Text),
            ("kind", Form::Enum),
            ("notes", Form::Message("keryx.scalars.Note")),
            ("kinds", Form::Enum),
            ("tags", Form::Message("keryx.scalars.Sample.TagsEntry")),
        ],
    ),
    ("keryx.scalars.Note", &[("text", Form::Text)]),
    (
        "keryx.scalars.Sample.TagsEntry",
        &[("key", Form::Text), ("value", Form::Enum)],
    ),
];

/// The tokens a stray lands as: a closer with nothing to close, an opener never closed, a
/// separator out of place, a lone quote, backslash, or comment mark.
const STRAYS: &[&str] = &[
    "}", ">", "]", "{", "<", "[", ":", ",", ";", "/", "\"", "'", "\\", "#", ".", "-",
];

/// The fields of the message `name` in [`MESSAGES`]. A name the table lacks is a broken
/// vocabulary — a mistyped entry — failed here, by name, rather than as an empty selection deep
/// in a strategy's construction.
fn fields_of(name: &str) -> &'static [(&'static str, Form)] {
    MESSAGES
        .iter()
        .find(|(message, _)| *message == name)
        .map_or_else(
            || panic!("`{name}` is a message the vocabulary names"),
            |(_, fields)| *fields,
        )
}

/// One element of a text spelled in a vocabulary: a field and its scalar value; a field and its
/// message value, through `< >` or `{ }`; a `#` comment; or a stray token the grammar does not
/// expect where it lands.
#[derive(Clone, Debug)]
enum Node {
    Scalar(&'static str, String),
    Message(&'static str, bool, Vec<Node>),
    Comment(String),
    Stray(&'static str),
}

/// `nodes` as text-format source: elements separated by a space, a comment to its line's end.
fn render(nodes: &[Node]) -> String {
    let mut text = String::new();
    for node in nodes {
        match node {
            Node::Scalar(name, value) => {
                text.push_str(name);
                text.push_str(": ");
                text.push_str(value);
            }
            Node::Message(name, angles, body) => {
                let (open, close) = if *angles { ANGLES } else { BRACES };
                text.push_str(name);
                text.push(' ');
                text.push(open);
                text.push(' ');
                text.push_str(&render(body));
                text.push(close);
            }
            Node::Comment(comment) => {
                text.push_str("# ");
                text.push_str(comment);
                text.push('\n');
            }
            Node::Stray(token) => text.push_str(token),
        }
        text.push(' ');
    }
    text
}

/// One of `spellings`, drawn uniformly.
fn one_of(spellings: &'static [&'static str]) -> impl Strategy<Value = String> {
    prop::sample::select(spellings).prop_map(str::to_owned)
}

/// An integer as the text format spells one: a native non-negative, in decimal, hexadecimal, or
/// octal — every integer kind takes it; a negative, which the unsigned kinds refuse at the
/// parse; one from the range past the native one, 2³¹ through 2³² − 1 — `ValueOutOfRange` where
/// the walk reaches it on a 32-bit unsigned kind, a decimal string on a 64-bit one (§6), the
/// parser's refusal on an `int32`; any `u64`; and a spelling no integer field takes.
fn integer() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => (0..=i32::MAX).prop_map(|n| n.to_string()),
        2 => (i32::MIN..0).prop_map(|n| n.to_string()),
        1 => (0..=0x7fff_ffff_u32).prop_map(|n| format!("{n:#x}")),
        1 => (0..=0o777_u32).prop_map(|n| format!("0{n:o}")),
        3 => (2_147_483_648_u64..=4_294_967_295).prop_map(|n| n.to_string()),
        1 => any::<u64>().prop_map(|n| n.to_string()),
        1 => one_of(&["1.5", "inf", "nan", "1e3", "-", "0x"]),
    ]
}

/// A float as the text format spells one: a decimal with or without a fraction, an exponent, or
/// the `f` suffix; an integer; an infinity or a NaN; and a spelling no float field takes. Every
/// one the walk reaches is `UnannotatedFloat`.
fn float() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => "-?[0-9]{1,3}(\\.[0-9]{0,3})?([eE]-?[0-9]{1,2})?[fF]?",
        2 => any::<i32>().prop_map(|n| n.to_string()),
        1 => one_of(&["inf", "-inf", "nan", "infinity", "-Infinity"]),
        1 => one_of(&["1.5.5", "e3", "x"]),
    ]
}

/// A string literal: the spellable alphabet, quoted either way — brackets and a comment mark
/// inside a literal are content, never a level; a NUL, by octal or hex escape (`InteriorNul`);
/// another control character — a tab, a DEL, a C0 by octal, a CR, an ESC by Unicode escape
/// (`UnrepresentableText`); the escapes the walk carries — a quote, a backslash, a newline, a
/// Unicode escape — non-ASCII text raw, and two literals concatenated; and what the lexer
/// refuses — a bare newline, an escape it has no form for, a literal never closed.
fn string() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => "[a-zA-Z0-9 ]{0,10}".prop_map(|text| format!("\"{text}\"")),
        1 => "[a-z {}<>#]{0,6}".prop_map(|text| format!("'{text}'")),
        2 => one_of(&["\"\\000\"", "\"a\\0b\"", "'\\x00'"]),
        2 => one_of(&["\"a\\tb\"", "\"\\x7f\"", "\"\\001\"", "\"\\r\"", "\"\\u001b\""]),
        2 => one_of(&[
            "\"\\\"\"", "'\\''", "\"\\\\\"", "\"\\n\"", "\"\\u00e9\"", "\"é字\"", "\"a\" \"b\"",
        ]),
        1 => one_of(&["\"\n\"", "\"\\q\"", "\"open"]),
    ]
}

/// An enum value: a name or number both schemas' `Kind`s declare; a name one declares and the
/// other does not; a number neither declares — the parser passes it, the walk refuses it
/// (`UnknownEnumValue`); and a name neither declares, the parser's refusal.
fn enum_value() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => one_of(&["KIND_UNSPECIFIED", "0", "1"]),
        1 => one_of(&["KIND_ONE", "KIND_FIRST"]),
        3 => one_of(&["2", "7", "-1", "2147483647"]),
        1 => Just("KIND_NONE".to_owned()),
    ]
}

/// A boolean, in the spellings the parser takes, and one it does not.
fn boolean() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => one_of(&["true", "false", "True", "False", "t", "f", "1", "0"]),
        1 => Just("yes".to_owned()),
    ]
}

/// One field of `fields` with a value of its form: a message field's value a body in *its*
/// message's vocabulary while `budget` allows, else an empty message value.
fn field(fields: &'static [(&'static str, Form)], budget: u32) -> BoxedStrategy<Node> {
    prop::sample::select(fields)
        .prop_flat_map(move |(name, form)| match form {
            Form::Integer => integer()
                .prop_map(move |value| Node::Scalar(name, value))
                .boxed(),
            Form::Float => float()
                .prop_map(move |value| Node::Scalar(name, value))
                .boxed(),
            Form::Text => string()
                .prop_map(move |value| Node::Scalar(name, value))
                .boxed(),
            Form::Enum => enum_value()
                .prop_map(move |value| Node::Scalar(name, value))
                .boxed(),
            Form::Bool => boolean()
                .prop_map(move |value| Node::Scalar(name, value))
                .boxed(),
            Form::Message(child) if budget > 0 => (any::<bool>(), body(child, budget - 1))
                .prop_map(move |(angles, body)| Node::Message(name, angles, body))
                .boxed(),
            Form::Message(_) => Just(Node::Message(name, false, Vec::new())).boxed(),
        })
        .boxed()
}

/// A body in the vocabulary of the message `name`: up to four elements, each one of its fields
/// with a value of the field's form — nested message values `budget` levels deep at most — or,
/// rarely, a comment (which the parser skips) or a stray token (which it refuses).
fn body(name: &'static str, budget: u32) -> BoxedStrategy<Vec<Node>> {
    let element = prop_oneof![
        20 => field(fields_of(name), budget),
        1 => "[a-z {}<>\"']{0,8}".prop_map(Node::Comment),
        1 => prop::sample::select(STRAYS).prop_map(Node::Stray),
    ];
    prop::collection::vec(element, 0..5).boxed()
}

/// Arbitrary characters — control characters, NUL, and non-ASCII among them — as a text of fewer
/// than `len` characters: the regime the lexer refuses at a character it has no token for, or
/// the parser at its first token.
fn characters(len: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..len)
        .prop_map(|characters| characters.into_iter().collect())
}

/// A text spelled in one root's vocabulary, rendered: the share that parses as that root and
/// reaches the walk, or — with a comment or a stray token mixed in, or a value of a form the
/// field refuses — the parser's refusal on a well-formed neighbourhood.
fn spelled() -> impl Strategy<Value = String> {
    let roots: Vec<&'static str> = CODECS.iter().map(|(_, root)| *root).collect();
    prop::sample::select(roots)
        .prop_flat_map(|root| body(root, 3))
        .prop_map(|nodes| render(&nodes))
}

/// A spelled text with up to three arbitrary characters spliced in at a character boundary: a
/// token broken, a bracket or a quote added — the regime where the lexer or the parser refuses
/// partway through a payload that was well formed.
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

/// A text nesting one field on either side of the ceiling — 90 to 110 levels — through either
/// bracket pair, with an innermost body the deepest message may or may not declare. Past the
/// ceiling it is the guard's refusal on every codec. At or below, it parses and shreds only as
/// `children` on the tree's codec with a body `Tree` declares — nothing, a `label`, a comment —
/// and is the parser's refusal otherwise: another field, another codec, or a `sensor` the tree
/// does not declare.
fn deep() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec!["children", "readings", "ratio", "items", "notes", "x"]),
        any::<bool>(),
        (CEILING - 9)..=(CEILING + 11),
        prop::sample::select(vec![
            "",
            "label: \"leaf\"",
            "sensor: \"s\" temp_c: 1",
            "# }\n",
        ]),
    )
        .prop_map(|(field, angles, levels, innermost)| {
            nested(
                field,
                if angles { ANGLES } else { BRACES },
                levels,
                innermost,
            )
        })
}

/// Arbitrary text, in five regimes: arbitrary characters, short and long; a text spelled in a
/// schema's vocabulary — half the draws, the share that reaches the walk; one spelled and then
/// spliced; and one nesting about the ceiling.
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        2 => characters(64),
        1 => characters(2048),
        6 => spelled(),
        2 => spliced(),
        1 => deep(),
    ]
}

/// Whether `kind` is one the door's contract names for a text payload parsed as a root the schema
/// declares: the decode's refusal, the guard's ceiling, and the walk's §6 refusals — never a
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

proptest! {
    // The default case count, not the binary instrument's 1024: nearly every case passes the
    // guard, and each such case, shredded against five codecs, spawns the parse thread the decode
    // sizes for the ceiling five times over — the door's one cost the binary form does not pay —
    // so the count is held where the suite stays quick; the fixed-seed tally below pins that the
    // generator reaches every outcome within it.
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn shred_is_total_over_arbitrary_text(text in text()) {
        // Facts or `Diagnostics`, never a panic — against every codec, so the text that parses as
        // one root reaches that schema's forms and kinds. Facts render: the §6 policy refused
        // every value the dialect cannot spell before it was built into one. A refusal is one the
        // door's contract names; a `DependencyFault` would be a found trigger for the containment
        // frame the module doc records as having none.
        for (codec, root_type) in CODECS.iter() {
            match shred_text(codec, root_type, &text) {
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
}

#[test]
fn the_text_generator_reaches_every_outcome_of_the_door() {
    // The generator is not vacuous — pinned at a fixed seed, so the shares are reproducible: over
    // a run of draws shredded against every codec, texts reach the walk and shred to facts; are
    // refused by the parser (`UndecodablePayload`), by the guard (`PayloadTooDeep`), and by the
    // walk at each of its §6 refusals — so the property above is checked at every step of the
    // door, not at the first alone. The sample holds the door's contract as the property does —
    // facts render, a refusal is one the contract names — so it is that check too, made
    // deterministic beside the property's re-rolled cases.
    let mut runner = TestRunner::deterministic();
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    for _ in 0..2048 {
        let text = text().new_tree(&mut runner).expect("a draw").current();
        for (codec, root_type) in CODECS.iter() {
            match shred_text(codec, root_type, &text) {
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
