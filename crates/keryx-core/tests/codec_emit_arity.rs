//! The reassembler matches a field's atoms by name AND arity (spec §12.1; ASP identifies a predicate
//! by name and arity). An atom of a field's name but another arity is a *different* predicate — the
//! model's private business — ignored, never read by position as the field: the same posture the
//! marker (`[root]`) and occupancy (`[occupant]`) atoms already keep at the slot index. So a
//! wrong-arity `sensor` atom cannot inject its extra argument as `sensor`'s value (property 4,
//! integrity): with no well-formed atom the total field is simply missing, never the decoy's value.

use std::path::Path;
use std::sync::LazyLock;

use keryx_core::codec::{Codec, PayloadFormat};
use keryx_core::diagnostics::DiagnosticKind;
use keryx_core::{Name, Sign, Symbol};

/// The thermal codec — `Reading { string sensor = 1; int32 temp_c = 2; }`, both proto3 total.
static CODEC: LazyLock<Codec> = LazyLock::new(|| {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    Codec::from_source(&[example.join("thermal.proto")], &[example, vendored])
        .expect("the thermal example compiles")
});

fn constant(name: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(name).expect("an identifier"),
        arguments: Vec::new(),
        sign: Sign::Positive,
    }
}

fn atom(pred: &str, arguments: Vec<Symbol>) -> Symbol {
    Symbol::Function {
        name: Name::new(pred).expect("an identifier"),
        arguments,
        sign: Sign::Positive,
    }
}

#[test]
fn a_wrong_arity_field_atom_is_ignored_never_read_as_the_value() {
    // `sensor(r0, "real", "evil")` is `sensor/3` — a different predicate from the `sensor/2` field.
    // It must be ignored, never read by `last_argument` (its final argument) as `sensor = "evil"`.
    // With no well-formed `sensor` atom, the total field is missing → a `ShapeViolation` at `sensor`,
    // never a `Reading` whose sensor is the decoy's extra argument.
    let decoy = vec![
        atom("emit_reading", vec![constant("r0")]),
        atom("reading", vec![constant("r0")]),
        atom(
            "sensor",
            vec![
                constant("r0"),
                Symbol::String("real".to_owned()),
                Symbol::String("evil".to_owned()),
            ],
        ),
        atom("temp_c", vec![constant("r0"), Symbol::Number(21)]),
    ];
    let diagnostics = CODEC.reassemble(&decoy, PayloadFormat::Binary).expect_err(
        "a total field with no well-formed atom is a shape violation, not an injection",
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.kind() == DiagnosticKind::ShapeViolation
                && d.locus().path() == Some("thermal.v1.Reading.sensor")),
        "the arity-3 `sensor` atom is ignored — sensor is missing, not set to the decoy's value: {diagnostics:?}"
    );

    // The control: a well-formed `sensor/2` atom sets the value and the message reassembles — so the
    // difference above is the arity match, not a broken sensor.
    let good = vec![
        atom("emit_reading", vec![constant("r0")]),
        atom("reading", vec![constant("r0")]),
        atom(
            "sensor",
            vec![constant("r0"), Symbol::String("real".to_owned())],
        ),
        atom("temp_c", vec![constant("r0"), Symbol::Number(21)]),
    ];
    let out = CODEC
        .reassemble(&good, PayloadFormat::Binary)
        .expect("a well-formed Reading reassembles");
    assert_eq!(out.messages().len(), 1, "one marker, one message");
}

#[test]
fn a_too_short_field_atom_is_ignored_too() {
    // `sensor(r0)` is `sensor/1` — again a different predicate, missing the value position entirely.
    // Ignored, not read: the total field is missing, never silently the parent term or a panic.
    let answer = vec![
        atom("emit_reading", vec![constant("r0")]),
        atom("reading", vec![constant("r0")]),
        atom("sensor", vec![constant("r0")]),
        atom("temp_c", vec![constant("r0"), Symbol::Number(7)]),
    ];
    let diagnostics = CODEC
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("a missing total field is a shape violation");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.kind() == DiagnosticKind::ShapeViolation
                && d.locus().path() == Some("thermal.v1.Reading.sensor")),
        "the arity-1 `sensor` atom is ignored — sensor is missing: {diagnostics:?}"
    );
}

#[test]
fn a_wrong_arity_atom_on_an_undeclared_reachable_parent_is_not_a_spurious_orphan() {
    // `sensor(readings(b0, 0), "x", "y")` is `sensor/3` — a different predicate; its parent
    // `readings(b0, 0)` descends from the marker root `b0` but is declared by no occupancy atom. The
    // orphan pass refuses a *field atom* whose parent is reachable yet undeclared, so were `sensor/3`
    // filed as one it would be a spurious `ShapeViolation`. Because arity discrimination happens at the
    // slot index, `sensor/3` is filed as neither a slot entry nor an orphan candidate (§12.1, private
    // business) — so the batch reassembles, its readings empty, exactly as the field planner ignores it.
    let readings0 = atom("readings", vec![constant("b0"), Symbol::Number(0)]);
    let answer = vec![
        atom("emit_reading_batch", vec![constant("b0")]),
        atom("reading_batch", vec![constant("b0")]),
        atom(
            "sensor",
            vec![
                readings0,
                Symbol::String("x".to_owned()),
                Symbol::String("y".to_owned()),
            ],
        ),
    ];
    let out = CODEC
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("a wrong-arity atom on an undeclared parent is private business, not an orphan");
    assert_eq!(
        out.messages().len(),
        1,
        "the batch reassembles, its readings empty"
    );
}
