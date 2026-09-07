//! Totality of the outbound door (§6; the threat model's totality property) — the hot adversarial
//! door's response half: `Codec::reassemble` returns messages or typed `Diagnostics` over *any*
//! answer set; it never panics, aborts, or hangs, and every diagnosis is one of the reassembler's
//! own kinds — never a foreign kind, never a contained fault under these generators (the encode's
//! mismatch panic is foreclosed by the validating setter, so no generated answer set is known to
//! trip the containment frame). One generator, mixing the schema's own vocabulary (`reading`,
//! `sensor`, `temp_c`, the markers) with arbitrary identifiers, numbers, strings, and nesting — so
//! it reaches the walk's shape checks, the inverse §6 refusals, and the ceiling, not only the index.

use std::path::Path;
use std::sync::LazyLock;

use keryx_core::codec::{Codec, PayloadFormat};
use keryx_core::diagnostics::DiagnosticKind;
use proptest::prelude::*;
use themelios_program::{Name, Sign, Symbol};

/// The thermal codec (`Reading`, `ReadingBatch` — a scalar message and a message sequence), built once.
static CODEC: LazyLock<Codec> = LazyLock::new(|| {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    Codec::from_source(&[example.join("thermal.proto")], &[example, vendored])
        .expect("the thermal example compiles")
});

/// Whether `kind` is one the reassembler's contract names — the walk's shape checks, the inverse §6
/// refusals, the ceiling, and (defense-in-depth) a contained encode fault — never a kind of another
/// door.
fn is_a_reassembler_kind(kind: DiagnosticKind) -> bool {
    matches!(
        kind,
        DiagnosticKind::ShapeViolation
            | DiagnosticKind::TermTypeMismatch
            | DiagnosticKind::ValueOutOfRange
            | DiagnosticKind::UnknownEnumValue
            | DiagnosticKind::UnannotatedFloat
            | DiagnosticKind::ReassembledTooDeep
            | DiagnosticKind::DependencyFault
    )
}

/// A predicate name: the schema's own vocabulary (to reach the walk), or an arbitrary identifier.
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
    .prop_map(|name| Name::new(name).expect("the generator writes identifiers"))
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

    /// Over any answer set, `reassemble` returns — never panics — and every diagnosis is a
    /// reassembler kind. A `DependencyFault` under this generator would be a found trigger of the
    /// encode's containment frame, to be recorded, not hidden; none is expected (the mismatch axis is
    /// foreclosed by the validating setter).
    #[test]
    fn reassemble_returns_over_any_answer_set(answer in prop::collection::vec(arb_symbol(), 0..24)) {
        match CODEC.reassemble(&answer, PayloadFormat::Binary) {
            Ok(_) => {}
            Err(diagnostics) => {
                for diagnostic in diagnostics.iter() {
                    prop_assert!(
                        is_a_reassembler_kind(diagnostic.kind()),
                        "an unexpected diagnostic kind reached the outbound door: {:?}",
                        diagnostic.kind()
                    );
                }
            }
        }
    }
}
