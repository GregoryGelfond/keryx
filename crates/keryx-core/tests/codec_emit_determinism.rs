//! The reassembler's determinism (the threat model's property, the outbound half): the same answer
//! set reassembles to byte-identical output — the messages in the same order (the markers'
//! `Symbol::Ord`), each root's bytes the same — whatever order the answer set spells its atoms in,
//! in every output form. A map is the sharp case: its entries arrive unordered (the engine's map is
//! a hash map), so byte-determinism rests on keryx sorting each map by key before it encodes — the
//! binary wire re-sort, the textproto sorter, and `serde_json`'s ordered map — one instrument per
//! form (§26, §12.2).

use std::path::Path;

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_test_support as support;
use keryx_test_support::wire::{delimited, int32};
use themelios_program::{Name, Sign, Symbol};

/// The three output forms, so a determinism test names each once.
const FORMATS: [PayloadFormat; 3] = [
    PayloadFormat::Binary,
    PayloadFormat::Textproto,
    PayloadFormat::Json,
];

fn thermal_codec() -> Codec {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    Codec::from_source(&[example.join("thermal.proto")], &[example, vendored])
        .expect("the thermal example compiles")
}

/// A fixture's codec, through the descriptor-set door.
fn fixture_codec(name: &str) -> Codec {
    Codec::new(&support::compile_fixture(name)).expect("the fixture builds a codec")
}

/// A `map<string, int32>` entry — its field-1 key and field-2 value on the wire.
fn string_int_entry(key: &str, value: i32) -> Vec<u8> {
    let mut entry = Vec::new();
    delimited(1, key.as_bytes(), &mut entry);
    int32(2, value, &mut entry);
    entry
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

/// A message's identity for comparison: type, root, bytes — reassembled in `format`.
fn shape(
    codec: &Codec,
    answer: &[Symbol],
    format: PayloadFormat,
) -> Vec<(String, Symbol, Vec<u8>)> {
    codec
        .reassemble(answer, format)
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
    for format in FORMATS {
        assert_eq!(
            shape(&codec, &answer, format),
            shape(&codec, &answer, format)
        );
    }
}

#[test]
fn the_atom_order_does_not_change_the_output() {
    // The same facts in two spellings — three roots given in different orders, each root's atoms
    // shuffled — reassemble to the same messages in the same order, in every form.
    let codec = thermal_codec();
    let mut one = reading("r0", "a", 1);
    one.extend(reading("r1", "b", 2));
    let mut other = reading("r1", "b", 2);
    other.reverse();
    other.extend(reading("r0", "a", 1).into_iter().rev());
    for format in FORMATS {
        assert_eq!(shape(&codec, &one, format), shape(&codec, &other, format));
    }
}

#[test]
fn a_set_reassembles_identically_under_any_atom_order() {
    // A `(keryx.set)` field is a set of atoms — order-free. A model-computed AlertSet (membership
    // atoms `alerts(out, al(N))` over provenance occupants) and its exact reverse reassemble to
    // byte-identical output in every form: members ordered by `Symbol::Ord`, the canonical set
    // serialization (§7.1, §26).
    let codec = thermal_codec();
    let al = |n: i32| atom("al", vec![Symbol::Number(n)]);
    let alert = |n: i32, sensor: &str, temp: i32| {
        vec![
            atom("alert", vec![al(n)]),
            atom("sensor", vec![al(n), Symbol::String(sensor.to_owned())]),
            atom("temp_c", vec![al(n), Symbol::Number(temp)]),
            atom("alerts", vec![constant("out"), al(n)]),
        ]
    };
    let mut answer = vec![
        atom("emit_alert_set", vec![constant("out")]),
        atom("alert_set", vec![constant("out")]),
    ];
    answer.extend(alert(0, "s-1", 1));
    answer.extend(alert(1, "s-2", 2));
    let mut reversed = answer.clone();
    reversed.reverse();
    for format in FORMATS {
        assert_eq!(
            shape(&codec, &answer, format),
            shape(&codec, &reversed, format),
            "a set reassembles identically regardless of atom order"
        );
    }
}

#[test]
fn a_maps_entries_reassemble_in_key_order_in_every_form() {
    // A `Gauge` with a three-entry `counts` map: the map atoms arrive unordered (the engine's map is
    // a hash map), so byte-identical output across two atom orders is the sort-by-key keryx applies
    // before it encodes — the binary re-sort, the textproto sorter, serde_json's ordered map, one
    // per form. A regression that dropped a canonicalizer would make one form's two runs differ.
    let codec = fixture_codec("obligations.proto");
    let root = Root::named(Name::new("r0").expect("an identifier"));
    let mut payload = Vec::new();
    delimited(1, b"g", &mut payload); // sensor
    delimited(4, &string_int_entry("b", 2), &mut payload);
    delimited(4, &string_int_entry("a", 1), &mut payload);
    delimited(4, &string_int_entry("c", 3), &mut payload);
    let facts = codec
        .shred("Gauge", &payload, PayloadFormat::Binary, &root)
        .expect("the gauge shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(atom("emit_gauge", vec![constant("r0")]));
    let mut shuffled = answer.clone();
    shuffled.reverse();
    for format in FORMATS {
        assert_eq!(
            shape(&codec, &answer, format),
            shape(&codec, &shuffled, format),
            "the map reassembles identically whatever order its entries arrive in, in {format:?}"
        );
    }
}
