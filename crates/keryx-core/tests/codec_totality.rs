//! Totality of the payload door (§6; the threat model's totality property) — the hot adversarial
//! door: `Codec::shred` returns facts or typed `Diagnostics` over *any* payload; it never panics,
//! aborts, or hangs. Two generators, as the model records for the descriptor door. **Arbitrary
//! bytes** overwhelmingly fail at the decoder (`UndecodablePayload`); the binary wire format is
//! permissive enough — an unknown field is skipped, a varint reads from almost any byte — that a
//! small, steady share decode and reach the walk (about one short string in forty; a long one
//! rarely, so the generator leans short), which is why the second generator is the one that
//! reaches the §6 refusals. **Valid encodings carrying hostile values** — a generator over one
//! message whose every field can carry a value §6 refuses — are checked against a model of the
//! policy: each hostile value yields exactly its refusal, at its field, and a benign one its fact,
//! so a value is refused or faithfully carried, never both and never neither (the integrity
//! property's two halves).
//!
//! **Containment, recorded honestly.** The door's one crossing into foreign code — the engine's
//! binary decode — runs inside the foreign-fault containment frame, so an unforeseen engine panic
//! would return as a `DependencyFault` value (the threat model's dependency boundary) rather than
//! unwind into the caller. On this door the frame is defense-in-depth: no payload is known to
//! fault the engine's binary decode, whose failures are values, so no case here stages one — unlike
//! the descriptor door, whose `decode_fault_set` drives a real contained fault. The mechanism (a
//! synthetic panic becoming the diagnostic; the flag's save and restore under nesting) is covered
//! where it lives, in `fault.rs`. What this suite observes from outside is the posture's two
//! consequences: no generated payload trips the frame — a `DependencyFault` under these generators
//! would be a found trigger, to be recorded, not hidden — and the door leaves no frame live behind
//! it.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use keryx_test_support as support;
use keryx_test_support::wire::{self, delimited};
use proptest::prelude::*;

use keryx_core::codec::{Codec, Facts, PayloadFormat, Root};
use keryx_core::diagnostics::{DiagnosticKind, Diagnostics};

/// A fixture's codec, through the descriptor-set door.
fn fixture_codec(name: &str) -> Codec {
    Codec::new(&support::compile_fixture(name)).expect("the fixture builds a codec")
}

/// The codecs the arbitrary-bytes generator shreds against, each with the root it decodes as: the
/// thermal story (§28), the refusals probe, maps, a recursive tree, and the scalar-treatment sample
/// — between them every field form, every scalar kind, an enum, a map, and compositional nesting.
/// Built once: a codec is per schema, never per payload.
static CODECS: LazyLock<Vec<(Codec, &'static str)>> = LazyLock::new(|| {
    let thermal = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let thermal = Codec::new(&support::compile_in(
        &[thermal, support::vendored()],
        "thermal.proto",
    ))
    .expect("the thermal example builds a codec");
    vec![
        (thermal, "thermal.v1.ReadingBatch"),
        (fixture_codec("refusals.proto"), "keryx.refusals.Probe"),
        (fixture_codec("maps.proto"), "keryx.maps.Inventory"),
        (fixture_codec("recursion.proto"), "keryx.rec.Tree"),
        (
            fixture_codec("scalar_treatment.proto"),
            "keryx.scalars.Sample",
        ),
    ]
});

/// The refusals probe's codec, for the hostile-value generator.
static PROBE: LazyLock<Codec> = LazyLock::new(|| fixture_codec("refusals.proto"));

/// Shred `payload` as `root_type` under `codec`, from the fresh root `r0`.
fn shred(codec: &Codec, root_type: &str, payload: &[u8]) -> Result<Facts, Diagnostics> {
    codec.shred(root_type, payload, PayloadFormat::Binary, &Root::fresh(0))
}

/// Whether `kind` is one the door's contract names for a payload decoded as a root the schema
/// declares: the decoder's refusal, the walk's ceiling, and the §6 refusals — never a contained
/// fault (none is known on this door) and never a kind of another door.
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

/// One generated `Probe` (`refusals.proto`): each field's value drawn from a strategy mixing the
/// benign and the hostile, so the generator reaches every refusal and every fact.
#[derive(Clone, Debug)]
struct Probe {
    count: u32,
    stamp: u32,
    label: String,
    kind: i32,
    tags: BTreeMap<String, i32>,
    ratio: Option<f32>,
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

fn probe() -> impl Strategy<Value = Probe> {
    (
        unsigned(),
        unsigned(),
        text(),
        number(),
        prop::collection::btree_map(text(), any::<i32>(), 0..3),
        prop_oneof![3 => Just(None), 1 => any::<f32>().prop_map(Some)],
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

/// The probe on the wire, written as bytes: every field carried, the map's entries in key order,
/// the `Ratio` only when the probe holds one.
fn encode(probe: &Probe) -> Vec<u8> {
    let mut buf = Vec::new();
    wire::uint32(1, probe.count, &mut buf);
    wire::fixed32(2, probe.stamp, &mut buf);
    delimited(3, probe.label.as_bytes(), &mut buf);
    wire::int32(4, probe.kind, &mut buf);
    for (key, value) in &probe.tags {
        let mut entry = Vec::new();
        delimited(1, key.as_bytes(), &mut entry);
        wire::int32(2, *value, &mut entry);
        delimited(5, &entry, &mut buf);
    }
    if let Some(ratio) = probe.ratio {
        let mut inner = Vec::new();
        wire::float(1, ratio, &mut inner);
        delimited(6, &inner, &mut buf);
    }
    buf
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
fn spelled(value: &str) -> String {
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
        format!("label(r0, {}).", spelled(&probe.label)),
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
            .map(|(key, value)| format!("tags(r0, {}, {value}).", spelled(key))),
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

/// Arbitrary payload bytes, leaning short: a short string decodes whole — and so reaches the
/// walk — far more often than a long one, whose every extra byte is another chance to fail the
/// decoder; the long regime keeps the decoder's own totality exercised across the byte space.
fn bytes() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        prop::collection::vec(any::<u8>(), 0..64),
        prop::collection::vec(any::<u8>(), 0..4096),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn shred_is_total_over_arbitrary_bytes(bytes in bytes()) {
        // Facts or `Diagnostics`, never a panic — against every codec, so the bytes that decode as
        // one root reach that schema's forms and kinds. A refusal is one the door's contract
        // names; a `DependencyFault` would be a found trigger for the containment frame the module
        // doc records as having none.
        for (codec, root_type) in CODECS.iter() {
            if let Err(diagnostics) = shred(codec, root_type, &bytes) {
                for diagnostic in diagnostics.iter() {
                    prop_assert!(
                        is_a_payload_refusal(diagnostic.kind()),
                        "a refusal the door's contract names, not {diagnostic}"
                    );
                }
            }
            prop_assert!(
                !keryx_core::is_containing(),
                "the door leaves no containment frame live behind it"
            );
        }
    }

    #[test]
    fn every_hostile_value_draws_exactly_its_refusal_and_every_benign_one_its_fact(probe in probe()) {
        // The policy modelled: the shred is refused exactly when the model refuses, with exactly
        // the model's refusals in the walk's order; else the facts are exactly the model's.
        let expected = expected_refusals(&probe);
        match shred(&PROBE, "keryx.refusals.Probe", &encode(&probe)) {
            Ok(facts) => {
                prop_assert!(
                    expected.is_empty(),
                    "shredded, yet §6 refuses {expected:?}: {probe:?}"
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
                prop_assert_eq!(located(&diagnostics), expected, "{:?}", probe);
            }
        }
        prop_assert!(!keryx_core::is_containing());
    }
}

#[test]
fn a_benign_probe_shreds_to_its_facts_and_a_hostile_one_draws_every_refusal_in_walk_order() {
    // Benign — every field carried at a spellable value, the map's keys in order: the facts,
    // exactly, on the `.lp` seam.
    let benign = Probe {
        count: 7,
        stamp: top(),
        label: "ok \"quoted\"\\\n".to_owned(),
        kind: 1,
        tags: [("a".to_owned(), -1), ("b".to_owned(), 2)].into(),
        ratio: None,
    };
    let facts = shred(&PROBE, "keryx.refusals.Probe", &encode(&benign)).expect("shreds");
    assert_eq!(
        facts.render().expect("renders"),
        "count(r0, 7).\n\
         kind(r0, one).\n\
         label(r0, \"ok \\\"quoted\\\"\\\\\\n\").\n\
         probe(r0).\n\
         stamp(r0, 2147483647).\n\
         tags(r0, \"a\", -1).\n\
         tags(r0, \"b\", 2).\n"
    );

    // Hostile — every refusal at once, one per value, collected in the walk's order: the fields in
    // number order, the map's entries in key order, then the carried child's float — and no facts
    // beside them.
    let hostile = Probe {
        count: top() + 1,
        stamp: u32::MAX,
        label: "a\tb".to_owned(),
        kind: 2,
        tags: [
            ("x\0".to_owned(), 1),
            ("y\u{1b}".to_owned(), 2),
            ("z".to_owned(), 3),
        ]
        .into(),
        ratio: Some(f32::NAN),
    };
    let diagnostics =
        shred(&PROBE, "keryx.refusals.Probe", &encode(&hostile)).expect_err("refused");
    assert_eq!(located(&diagnostics), expected_refusals(&hostile));
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
        ]
    );
}
