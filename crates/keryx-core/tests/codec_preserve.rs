//! `(keryx.unknown) = PRESERVE` (spec §7.4): an undeclared wire number of an open enum lowers to
//! the escape term `unknown(N)` inbound (rather than the loud `UnknownEnumValue`) and raises back
//! to the wire number `N` outbound, so it round-trips byte-for-byte where the default refuses it.
//! An `unknown(N)` naming a *declared* value is refused outbound — the model must use the constant,
//! keeping term↔wire injective — and a non-`PRESERVE` enum refuses `unknown(N)` altogether (§12.3).

use std::path::Path;

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::diagnostics::DiagnosticKind;
use keryx_test_support::{fixtures, vendored, wire};
use themelios_program::{Name, Sign, Symbol};

/// The PRESERVE fixture's codec — a proto3 (open) `Signal` enum under `(keryx.unknown) = PRESERVE`.
fn preserve_codec() -> Codec {
    Codec::from_source(
        &[fixtures().join("preserve.proto")],
        &[fixtures(), vendored()],
    )
    .expect("the preserve fixture compiles")
}

/// The enum example's codec — its `Phase` enum is open but *not* `PRESERVE`, so it refuses an
/// unknown number the ordinary way (the outbound half of that refusal is pinned here).
fn non_preserve_codec() -> Codec {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/enum");
    Codec::from_source(
        &[example.join("signals.proto")],
        std::slice::from_ref(&example),
    )
    .expect("the enum example compiles")
}

fn constant(name: &str) -> Symbol {
    atom(name, Vec::new())
}

fn atom(pred: &str, arguments: Vec<Symbol>) -> Symbol {
    Symbol::Function {
        name: Name::new(pred).expect("an identifier"),
        arguments,
        sign: Sign::Positive,
    }
}

/// The escape term `unknown(n)`.
fn unknown(n: i32) -> Symbol {
    atom("unknown", vec![Symbol::Number(n)])
}

#[test]
fn an_undeclared_number_under_preserve_shreds_to_the_escape_term() {
    // `Panel { current: 99 }` — 99 is a `Signal` number the enum does not declare.
    let codec = preserve_codec();
    let mut panel = Vec::new();
    wire::int32(1, 99, &mut panel);
    let facts = codec
        .shred(
            "keryx.preserve.Panel",
            &panel,
            PayloadFormat::Binary,
            &Root::fresh(0),
        )
        .expect("the panel shreds under PRESERVE");
    let rendered = facts.render().expect("the facts render");
    assert!(
        rendered.contains("current(r0, unknown(99))"),
        "expected the escape term in\n{rendered}"
    );
}

#[test]
fn a_declared_number_under_preserve_still_shreds_to_its_constant() {
    // `Panel { current: SIGNAL_GO }` (1) — a declared number lowers to its constant, so
    // `unknown(N)` arises only for a genuinely undeclared `N`.
    let codec = preserve_codec();
    let mut panel = Vec::new();
    wire::int32(1, 1, &mut panel);
    let facts = codec
        .shred(
            "keryx.preserve.Panel",
            &panel,
            PayloadFormat::Binary,
            &Root::fresh(0),
        )
        .expect("the panel shreds");
    let rendered = facts.render().expect("the facts render");
    assert!(
        rendered.contains("current(r0, go)"),
        "expected the declared constant in\n{rendered}"
    );
}

#[test]
fn an_escape_term_reassembles_to_its_wire_number() {
    let codec = preserve_codec();
    let answer = vec![
        atom("emit_panel", vec![constant("p0")]),
        atom("panel", vec![constant("p0")]),
        atom("current", vec![constant("p0"), unknown(99)]),
    ];
    let out = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("the escape term reassembles");
    assert_eq!(out.messages().len(), 1);
    let mut expected = Vec::new();
    wire::int32(1, 99, &mut expected);
    assert_eq!(out.messages()[0].bytes(), expected);
}

#[test]
fn an_escape_term_naming_a_declared_value_is_refused_outbound() {
    // `unknown(1)` — 1 is `SIGNAL_GO`; the model must use the constant `go`, so the escape term is
    // a shape violation (term↔wire injectivity, §12.3).
    let codec = preserve_codec();
    let answer = vec![
        atom("emit_panel", vec![constant("p0")]),
        atom("panel", vec![constant("p0")]),
        atom("current", vec![constant("p0"), unknown(1)]),
    ];
    let diagnostics = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("an escape term naming a declared value is refused");
    assert_eq!(
        diagnostics.iter().next().expect("one diagnostic").kind(),
        DiagnosticKind::ShapeViolation
    );
}

#[test]
fn an_escape_term_on_a_non_preserve_enum_is_refused_outbound() {
    // `Light { intersection: "x"; phase: unknown(99) }` — `Phase` is not `PRESERVE`, so the escape
    // term has no meaning outbound and is refused (the symmetric loud default, §12.3).
    let codec = non_preserve_codec();
    let answer = vec![
        atom("emit_light", vec![constant("l0")]),
        atom("light", vec![constant("l0")]),
        atom(
            "intersection",
            vec![constant("l0"), Symbol::String("x".to_owned())],
        ),
        atom("phase", vec![constant("l0"), unknown(99)]),
    ];
    let diagnostics = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("an escape term on a non-PRESERVE enum is refused");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.kind() == DiagnosticKind::TermTypeMismatch),
        "expected a term-type mismatch, got {diagnostics:?}"
    );
}

#[test]
fn an_undeclared_number_round_trips_byte_for_byte() {
    // wire 99 → unknown(99) → wire 99 (the round-trip property on the escape term).
    let codec = preserve_codec();
    let mut panel = Vec::new();
    wire::int32(1, 99, &mut panel);
    let facts = codec
        .shred(
            "keryx.preserve.Panel",
            &panel,
            PayloadFormat::Binary,
            &Root::fresh(0),
        )
        .expect("the panel shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(atom("emit_panel", vec![constant("r0")]));
    let out = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("the facts reassemble");
    assert_eq!(out.messages().len(), 1);
    assert_eq!(out.messages()[0].bytes(), panel);
}
