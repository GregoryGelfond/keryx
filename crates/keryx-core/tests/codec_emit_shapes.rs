//! The reassembler over the composite field forms (spec §7, §12.3) — the map, enum,
//! singular-message, and repeated-scalar paths the thermal and recursion instruments do not reach.
//! Each round-trips a payload through the door: shred it to facts, export the root, reassemble, and
//! shred the reassembled bytes again — the facts are identical, so the reassembler inverts the shred
//! over every form (property 4's round-trip half, for these forms). Two refusals pin the paths' own
//! diagnostics: an undeclared enum constant, and a duplicate map key.

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::diagnostics::DiagnosticKind;
use keryx_test_support as support;
use keryx_test_support::wire::{delimited, int32, int64};
use themelios_program::{Name, Sign, Symbol};

/// A fixture's codec, through the descriptor-set door.
fn fixture_codec(name: &str) -> Codec {
    Codec::new(&support::compile_fixture(name)).expect("the fixture builds a codec")
}

/// The root every payload here shreds under and every marker exports.
fn root() -> Root {
    Root::named(Name::new("r0").expect("an identifier"))
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

/// Shred `payload`, reassemble it under `sort_marker`'s `emit_<sort>(r0)` marker, and shred the
/// reassembled bytes again — asserting the two fact sets are identical (the reassembler inverts the
/// shred over the forms `payload` carries).
fn assert_round_trips(codec: &Codec, root_type: &str, sort_marker: &str, payload: &[u8]) {
    let facts = codec
        .shred(root_type, payload, PayloadFormat::Binary, &root())
        .expect("the payload shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(atom(sort_marker, vec![constant("r0")]));
    let out = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("the facts reassemble");
    assert_eq!(out.messages().len(), 1, "one message per marker");
    let facts_again = codec
        .shred(
            root_type,
            out.messages()[0].bytes(),
            PayloadFormat::Binary,
            &root(),
        )
        .expect("the reassembled bytes shred");
    assert_eq!(
        facts.symbols(),
        facts_again.symbols(),
        "the reassembled payload shreds to the same facts"
    );
}

/// A `map<string, int32>` entry — its field-1 key and field-2 value.
fn string_int_entry(key: &str, value: i32) -> Vec<u8> {
    let mut entry = Vec::new();
    delimited(1, key.as_bytes(), &mut entry);
    int32(2, value, &mut entry);
    entry
}

#[test]
fn a_gauge_round_trips_every_scalar_map_and_enum_form() {
    // `obligations.proto`'s `Gauge` carries a repeated scalar (`samples`), a scalar map (`counts`),
    // an enum (`level`), a repeated enum (`history`), and a map of enums (`by_name`) — none of which
    // the thermal or recursion instruments reach. All round-trip through the reassembler.
    let codec = fixture_codec("obligations.proto");
    let mut payload = Vec::new();
    delimited(1, b"g", &mut payload); // sensor
    int32(3, 10, &mut payload); // samples[0]
    int32(3, 11, &mut payload); // samples[1]
    delimited(4, &string_int_entry("x", 5), &mut payload); // counts["x"] = 5
    int32(7, 1, &mut payload); // level = LEVEL_HIGH
    int32(9, 0, &mut payload); // history[0] = LEVEL_LOW
    int32(9, 1, &mut payload); // history[1] = LEVEL_HIGH
    let mut by_name = Vec::new();
    delimited(1, b"a", &mut by_name);
    int32(2, 1, &mut by_name);
    delimited(10, &by_name, &mut payload); // by_name["a"] = LEVEL_HIGH
    assert_round_trips(&codec, "Gauge", "emit_gauge", &payload);
}

#[test]
fn an_inventory_round_trips_a_scalar_map_and_a_message_valued_map() {
    // `maps.proto`'s `Inventory`: `map<string, int32>` and `map<int64, Item>`. The message-valued
    // map exercises `plan_child` and the `MessageMap` build arm.
    let codec = fixture_codec("maps.proto");
    let mut payload = Vec::new();
    delimited(1, &string_int_entry("a", 1), &mut payload);
    delimited(1, &string_int_entry("b", 2), &mut payload);
    let mut sku = Vec::new();
    delimited(1, b"x", &mut sku); // Item.sku
    let mut item = Vec::new();
    int64(1, 20, &mut item); // items key
    delimited(2, &sku, &mut item); // items value (the Item message)
    delimited(2, &item, &mut payload);
    assert_round_trips(&codec, "Inventory", "emit_inventory", &payload);
}

#[test]
fn a_reading_round_trips_a_singular_message_field() {
    // `proto3.proto`'s `Reading` has a singular message field `Detail detail` and an enum `Level
    // level` — the `Planned::Message` build arm, uncovered by the sequence-only nesting instruments.
    let codec = fixture_codec("proto3.proto");
    let mut payload = Vec::new();
    delimited(1, b"s", &mut payload); // sensor
    int32(2, 5, &mut payload); // temp_c
    int32(4, 2, &mut payload); // level = LEVEL_HIGH (2)
    let mut detail = Vec::new();
    delimited(1, b"n", &mut detail); // Detail.note
    delimited(5, &detail, &mut payload); // detail
    assert_round_trips(&codec, "Reading", "emit_reading", &payload);
}

/// Replace a `level(r0, _)` atom's value with `value`; other atoms pass through unchanged.
fn swap_level_value(symbol: &Symbol, value: Symbol) -> Symbol {
    if let Symbol::Function {
        name,
        arguments,
        sign,
    } = symbol
        && name.as_str() == "level"
        && arguments.len() == 2
    {
        return Symbol::Function {
            name: name.clone(),
            arguments: vec![arguments[0].clone(), value],
            sign: *sign,
        };
    }
    symbol.clone()
}

/// A valid `Gauge` (`sensor` + `level`), read to facts under `r0` — the base for mutating its
/// `level` value into an enum refusal.
fn gauge_with_level() -> (Codec, Vec<Symbol>) {
    let codec = fixture_codec("obligations.proto");
    let mut payload = Vec::new();
    delimited(1, b"g", &mut payload); // sensor
    int32(7, 1, &mut payload); // level = LEVEL_HIGH
    let facts = codec
        .shred("Gauge", &payload, PayloadFormat::Binary, &root())
        .expect("the payload shreds");
    (codec, facts.symbols().to_vec())
}

#[test]
fn an_undeclared_enum_constant_is_refused() {
    // A valid `Gauge`, its `level` value replaced by a constant the enum does not declare: the enum
    // path refuses it (`UnknownEnumValue`), never mapping it to a constant it is not (spec §7.4).
    let (codec, facts) = gauge_with_level();
    let mut answer: Vec<Symbol> = facts
        .iter()
        .map(|s| swap_level_value(s, constant("not_a_declared_level")))
        .collect();
    answer.push(atom("emit_gauge", vec![constant("r0")]));
    let error = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("an undeclared enum constant is refused");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == DiagnosticKind::UnknownEnumValue),
        "the undeclared constant is an UnknownEnumValue"
    );
}

#[test]
fn a_non_constant_enum_value_is_a_term_type_mismatch() {
    // A valid `Gauge`, its `level` value replaced by a number rather than a constant: `enum_number`'s
    // other refusal — a term of the wrong shape for an enum is a `TermTypeMismatch`, never coerced.
    let (codec, facts) = gauge_with_level();
    let mut answer: Vec<Symbol> = facts
        .iter()
        .map(|s| swap_level_value(s, Symbol::Number(5)))
        .collect();
    answer.push(atom("emit_gauge", vec![constant("r0")]));
    let error = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("a non-constant enum value is refused");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == DiagnosticKind::TermTypeMismatch),
        "a non-constant enum value is a TermTypeMismatch"
    );
}

#[test]
fn a_duplicate_map_key_is_a_shape_violation() {
    // A valid `Gauge` with two `counts` entries for one key — the map path refuses it, never
    // choosing one value silently (§12.2).
    let codec = fixture_codec("obligations.proto");
    let mut payload = Vec::new();
    delimited(1, b"g", &mut payload); // sensor
    delimited(4, &string_int_entry("k", 1), &mut payload); // counts["k"] = 1
    let facts = codec
        .shred("Gauge", &payload, PayloadFormat::Binary, &root())
        .expect("the payload shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(atom(
        "counts",
        vec![
            constant("r0"),
            Symbol::String("k".to_owned()),
            Symbol::Number(2),
        ],
    )); // a second entry for the key "k"
    answer.push(atom("emit_gauge", vec![constant("r0")]));
    let error = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("a duplicate map key is refused");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == DiagnosticKind::ShapeViolation),
        "the duplicate key is a ShapeViolation"
    );
}
