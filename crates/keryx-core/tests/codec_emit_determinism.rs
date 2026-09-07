//! The reassembler's determinism (the threat model's property, the outbound half): the same answer
//! set reassembles to byte-identical output — the messages in the same order (the markers'
//! `Symbol::Ord`), each root's bytes the same — whatever order the answer set spells its atoms in.

use std::path::Path;

use keryx_core::codec::{Codec, PayloadFormat};
use themelios_program::{Name, Sign, Symbol};

fn thermal_codec() -> Codec {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    Codec::from_source(&[example.join("thermal.proto")], &[example, vendored])
        .expect("the thermal example compiles")
}

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

/// A message's identity for comparison: type, root, bytes.
fn shape(codec: &Codec, answer: &[Symbol]) -> Vec<(String, Symbol, Vec<u8>)> {
    codec
        .reassemble(answer, PayloadFormat::Binary)
        .expect("reassembles")
        .messages()
        .iter()
        .map(|m| {
            (
                m.type_name().to_owned(),
                m.root().clone(),
                m.bytes().to_vec(),
            )
        })
        .collect()
}

#[test]
fn the_same_answer_set_reassembles_identically() {
    let codec = thermal_codec();
    let mut answer = reading("r2", "c", 3);
    answer.extend(reading("r0", "a", 1));
    answer.extend(reading("r1", "b", 2));
    assert_eq!(shape(&codec, &answer), shape(&codec, &answer));
}

#[test]
fn the_atom_order_does_not_change_the_output() {
    // The same facts in two spellings — three roots given in different orders, each root's atoms
    // shuffled — reassemble to the same messages in the same order.
    let codec = thermal_codec();
    let mut one = reading("r0", "a", 1);
    one.extend(reading("r1", "b", 2));
    let mut other = reading("r1", "b", 2);
    other.reverse();
    other.extend(reading("r0", "a", 1).into_iter().rev());
    assert_eq!(shape(&codec, &one), shape(&codec, &other));
}
