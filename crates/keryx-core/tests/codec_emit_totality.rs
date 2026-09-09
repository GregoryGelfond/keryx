//! Totality of the outbound door (§6; the threat model's totality property) — the hot adversarial
//! door's response half: `Codec::reassemble` returns messages or typed `Diagnostics` over *any*
//! answer set, in *any* output form; it never panics, aborts, or hangs, and every diagnosis is one
//! of the reassembler's own kinds — never a foreign kind, never a contained fault under these
//! generators (the encode's mismatch panic is foreclosed by the validating setter, so no generated
//! answer set is known to trip the containment frame). Two generators. The first mixes the schema's
//! own vocabulary (`reading`, `sensor`, `temp_c`, the markers) with arbitrary identifiers, numbers,
//! strings, and nesting — so it reaches the walk's shape checks, the inverse §6 refusals, and the
//! ceiling, not only the index — and runs each case in all three output forms. The second is
//! vocabulary-aware: it sweeps valid reach trees of growing breadth (a batch's readings, a gauge's
//! map entries), deterministic by construction (no random seed), and tallies that every one
//! reassembles *and* round-trips in every form — so the success path is exercised, not only the
//! refusals.

use std::path::Path;
use std::sync::LazyLock;

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::diagnostics::DiagnosticKind;
use keryx_test_support as support;
use keryx_test_support::wire::{delimited, int32};
use proptest::prelude::*;
use themelios_program::{Name, Sign, Symbol};

/// The three output forms, so a per-format assertion names each once.
const FORMATS: [PayloadFormat; 3] = [
    PayloadFormat::Binary,
    PayloadFormat::Textproto,
    PayloadFormat::Json,
];

/// The thermal codec (`Reading`, `ReadingBatch` — a scalar message and a message sequence), built once.
static CODEC: LazyLock<Codec> = LazyLock::new(|| {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    Codec::from_source(&[example.join("thermal.proto")], &[example, vendored])
        .expect("the thermal example compiles")
});

/// The `obligations.proto` codec (`Gauge` — the scalar-map/enum shapes), built once.
static GAUGE: LazyLock<Codec> =
    LazyLock::new(|| Codec::new(&support::compile_fixture("obligations.proto")).expect("builds"));

/// Whether `kind` is one the reassembler's contract names — the walk's shape checks, the inverse §6
/// refusals, the ceiling, the JSON encode's well-known-type refusal, and (defense-in-depth) a
/// contained encode fault — never a kind of another door.
fn is_a_reassembler_kind(kind: DiagnosticKind) -> bool {
    matches!(
        kind,
        DiagnosticKind::ShapeViolation
            | DiagnosticKind::TermTypeMismatch
            | DiagnosticKind::ValueOutOfRange
            | DiagnosticKind::UnknownEnumValue
            | DiagnosticKind::UnannotatedFloat
            | DiagnosticKind::ReassembledTooDeep
            | DiagnosticKind::UnrepresentableJson
            | DiagnosticKind::DependencyFault
    )
}

/// A predicate name: the schema's own vocabulary (to reach the walk), or an arbitrary identifier.
/// The arbitrary branch is filtered through `Name::new`, so a draw that spells an ASP reserved
/// word (`not`, say) is resampled, never admitted: an answer set's predicate names are always
/// identifiers — themelios refuses a reserved word as a `Name` — so the generator samples that
/// same domain and never itself panics constructing one.
fn arb_name() -> impl Strategy<Value = Name> {
    prop_oneof![
        Just("emit_reading".to_owned()),
        Just("emit_reading_batch".to_owned()),
        Just("reading".to_owned()),
        Just("reading_batch".to_owned()),
        Just("sensor".to_owned()),
        Just("temp_c".to_owned()),
        Just("readings".to_owned()),
        "[a-z][a-z0-9_]{0,8}",
    ]
    .prop_filter_map(
        "an answer set's predicate names are identifiers, never reserved words",
        |name| Name::new(name).ok(),
    )
}

/// An arbitrary ground symbol — a number, a string, a term-order bound, a function over a
/// vocabulary-or-arbitrary name, or a tuple — bounded in depth and breadth.
fn arb_symbol() -> impl Strategy<Value = Symbol> {
    let leaf = prop_oneof![
        any::<i32>().prop_map(Symbol::Number),
        "[a-zA-Z0-9 ]{0,12}".prop_map(Symbol::String),
        Just(Symbol::Infimum),
        Just(Symbol::Supremum),
    ];
    leaf.prop_recursive(4, 48, 5, |inner| {
        prop_oneof![
            (arb_name(), prop::collection::vec(inner.clone(), 0..4)).prop_map(
                |(name, arguments)| Symbol::Function {
                    name,
                    arguments,
                    sign: Sign::Positive,
                }
            ),
            prop::collection::vec(inner, 0..4).prop_map(Symbol::Tuple),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Over any answer set, in every output form, `reassemble` returns — never panics — and every
    /// diagnosis is a reassembler kind. A `DependencyFault` under this generator would be a found
    /// trigger of the encode's containment frame, to be recorded, not hidden; none is expected (the
    /// mismatch axis is foreclosed by the validating setter, in each form).
    #[test]
    fn reassemble_returns_over_any_answer_set(answer in prop::collection::vec(arb_symbol(), 0..24)) {
        for format in FORMATS {
            match CODEC.reassemble(&answer, format) {
                Ok(_) => {}
                Err(diagnostics) => {
                    for diagnostic in diagnostics.iter() {
                        prop_assert!(
                            is_a_reassembler_kind(diagnostic.kind()),
                            "an unexpected diagnostic kind reached the {format:?} outbound door: {:?}",
                            diagnostic.kind()
                        );
                    }
                }
            }
        }
    }
}

/// A constant symbol — a positive zero-argument function.
fn constant(name: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(name).expect("an identifier"),
        arguments: Vec::new(),
        sign: Sign::Positive,
    }
}

/// The `emit_<sort>(r0)` marker exporting the tree under `r0`.
fn marker(sort_marker: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(sort_marker).expect("an identifier"),
        arguments: vec![constant("r0")],
        sign: Sign::Positive,
    }
}

/// Reassemble the valid tree `answer` (type `root_type`, exported by `marker`) in every form and
/// shred each result back: assert it reassembles to one message and round-trips to the same facts,
/// counting the successes so the caller can tally that the success path was genuinely reached.
fn reassembles_and_round_trips(codec: &Codec, root_type: &str, answer: &[Symbol]) -> usize {
    let root = Root::named(Name::new("r0").expect("an identifier"));
    let facts: Vec<Symbol> = answer
        .iter()
        .filter(
            |s| !matches!(s, Symbol::Function { name, .. } if name.as_str().starts_with("emit_")),
        )
        .cloned()
        .collect();
    let mut successes = 0;
    for format in FORMATS {
        let out = codec.reassemble(answer, format).unwrap_or_else(|d| {
            panic!("a valid {root_type} tree reassembles in {format:?}: {d:?}")
        });
        assert_eq!(out.messages().len(), 1, "one marker, one message");
        let again = codec
            .shred(root_type, out.messages()[0].bytes(), format, &root)
            .unwrap_or_else(|d| panic!("the reassembled {format:?} bytes shred: {d:?}"));
        assert_eq!(
            again.symbols(),
            facts.as_slice(),
            "the {root_type} tree round-trips in {format:?}"
        );
        successes += 1;
    }
    successes
}

#[test]
fn the_vocabulary_aware_trees_reassemble_and_round_trip_in_every_form() {
    // The second generator: valid reach trees of growing breadth — a `ReadingBatch` of 0..=12
    // readings (message-sequence reach) and a `Gauge` with a 0..=8-entry scalar map (map breadth) —
    // built by shredding a generated payload so the facts are exactly the shred's, then exported and
    // reassembled in every form. Every tree reassembles and round-trips; the tally proves the
    // success path is reached across breadths and forms, not only the arbitrary generator's refusals.
    let root = Root::named(Name::new("r0").expect("an identifier"));
    let mut tally = 0;

    for n in 0..=12i32 {
        let mut payload = Vec::new();
        for i in 0..n {
            let mut r = Vec::new();
            delimited(1, format!("s{i}").as_bytes(), &mut r); // Reading.sensor
            int32(2, i, &mut r); // Reading.temp_c
            delimited(1, &r, &mut payload); // ReadingBatch.readings[i]
        }
        let facts = CODEC
            .shred("ReadingBatch", &payload, PayloadFormat::Binary, &root)
            .expect("the batch shreds");
        let mut answer = facts.symbols().to_vec();
        answer.push(marker("emit_reading_batch"));
        tally += reassembles_and_round_trips(&CODEC, "ReadingBatch", &answer);
    }

    for k in 0..=8i32 {
        let mut payload = Vec::new();
        delimited(1, b"g", &mut payload); // Gauge.sensor (total)
        for j in 0..k {
            let mut entry = Vec::new();
            delimited(1, format!("k{j}").as_bytes(), &mut entry); // counts key
            int32(2, j, &mut entry); // counts value
            delimited(4, &entry, &mut payload); // Gauge.counts[k{j}]
        }
        let facts = GAUGE
            .shred("Gauge", &payload, PayloadFormat::Binary, &root)
            .expect("the gauge shreds");
        let mut answer = facts.symbols().to_vec();
        answer.push(marker("emit_gauge"));
        tally += reassembles_and_round_trips(&GAUGE, "Gauge", &answer);
    }

    // 13 batch breadths + 9 gauge breadths, each in 3 forms.
    assert_eq!(
        tally,
        (13 + 9) * 3,
        "every generated tree reassembled in every form"
    );
}
