//! `Codec::reassemble` (spec §12.3) — the outbound door, over the public surface: an answer set of
//! `Symbol`s in the generated vocabulary reassembles to the messages its `emit_<sort>` markers name,
//! each carrying its root and encoded to the binary wire form. The round-trip property proper is in
//! `codec_roundtrip.rs`; this pins the door's shape — one message per marker, the root carried, the
//! results marker-ordered.

use std::path::Path;

use keryx_core::codec::{Codec, PayloadFormat};
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
    atom(name, Vec::new())
}

/// A positive atom `pred(args…)`.
fn atom(pred: &str, arguments: Vec<Symbol>) -> Symbol {
    Symbol::Function {
        name: Name::new(pred).expect("an identifier"),
        arguments,
        sign: Sign::Positive,
    }
}

/// The facts of one `Reading` under the root `root`, exported by its marker.
fn reading(root: &str, sensor: &str, temp_c: i32) -> Vec<Symbol> {
    vec![
        atom("emit_reading", vec![constant(root)]),
        atom("reading", vec![constant(root)]),
        atom(
            "sensor",
            vec![constant(root), Symbol::String(sensor.to_owned())],
        ),
        atom("temp_c", vec![constant(root), Symbol::Number(temp_c)]),
    ]
}

#[test]
fn an_answer_set_reassembles_one_message_carrying_its_type_root_and_bytes() {
    let codec = thermal_codec();
    let out = codec
        .reassemble(&reading("r0", "s-1", 21), PayloadFormat::Binary)
        .expect("the answer set reassembles");
    assert_eq!(out.messages().len(), 1);
    let message = &out.messages()[0];
    assert_eq!(message.type_name(), "thermal.v1.Reading");
    assert_eq!(
        message.bytes(),
        keryx_test_support::wire::reading("s-1", 21)
    );
    assert_eq!(message.root(), &constant("r0"));
}

#[test]
fn two_roots_of_one_type_are_distinct_and_marker_ordered() {
    // Two `Reading` roots in one answer set, given out of order: the results are distinguished by
    // their root (F5) and ordered by the marker atom's `Symbol::Ord` — `emit_reading(r0)` before
    // `emit_reading(r1)` — regardless of the answer set's order, so the door is deterministic.
    let codec = thermal_codec();
    let mut answer = reading("r1", "b", 2);
    answer.extend(reading("r0", "a", 1));
    let out = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("both roots reassemble");
    assert_eq!(out.messages().len(), 2);
    assert_eq!(out.messages()[0].root(), &constant("r0"));
    assert_eq!(out.messages()[1].root(), &constant("r1"));
    assert_eq!(
        out.messages()[0].bytes(),
        keryx_test_support::wire::reading("a", 1)
    );
    assert_eq!(
        out.messages()[1].bytes(),
        keryx_test_support::wire::reading("b", 2)
    );
}

#[test]
fn a_broken_answer_set_is_every_diagnosis_never_a_partial_reassembly() {
    // One good root and one with a duplicate singular: the whole call fails (property 4), no
    // messages beside the diagnosis.
    let codec = thermal_codec();
    let mut answer = reading("r0", "a", 1);
    answer.extend(reading("r1", "b", 2));
    answer.push(atom(
        "sensor",
        vec![constant("r1"), Symbol::String("dup".to_owned())],
    ));
    let error = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("a duplicate singular fails the whole reassembly");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == keryx_core::diagnostics::DiagnosticKind::ShapeViolation),
        "the duplicate is a shape violation"
    );
}
