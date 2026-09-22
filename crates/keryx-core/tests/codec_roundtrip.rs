//! The round-trip property (the threat model's property 4, integrity): a payload shredded to facts
//! and reassembled from them is the payload again, and an answer set reassembled to a payload and
//! shredded is the answer set again — on the canonical forms, byte-for-byte and symbol-for-symbol.
//! The thermal story (§28) carries the forward identity in every output form; the composite field
//! forms — scalar and message-valued maps, enums, singular and repeated — round-trip in all three
//! forms too (§26 parity), so no output form is faithful only for the sequence-of-messages shape.
//! The walk's own shape checks are in `codec_reassemble.rs` and the module tests.

use std::path::Path;

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_test_support as support;
use keryx_test_support::wire::{self, delimited, int32, int64};
use themelios_program::{Name, Sign, Symbol};

/// The three output forms, so a parity test names each once.
const FORMATS: [PayloadFormat; 3] = [
    PayloadFormat::Binary,
    PayloadFormat::Textproto,
    PayloadFormat::Json,
];

/// The thermal example's codec (spec §28), through the source door.
fn thermal_codec() -> Codec {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    Codec::from_source(&[example.join("thermal.proto")], &[example, vendored])
        .expect("the thermal example compiles")
}

/// A constant symbol — a positive zero-argument function.
fn constant(name: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(name).expect("an identifier"),
        arguments: Vec::new(),
        sign: Sign::Positive,
    }
}

/// The `emit_<sort>(root)` marker exporting the tree under `root`.
fn marker(sort_marker: &str, root: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(sort_marker).expect("an identifier"),
        arguments: vec![constant(root)],
        sign: Sign::Positive,
    }
}

/// A positive atom `pred(args…)`.
fn atom(pred: &str, arguments: Vec<Symbol>) -> Symbol {
    Symbol::Function {
        name: Name::new(pred).expect("an identifier"),
        arguments,
        sign: Sign::Positive,
    }
}

#[test]
fn a_payload_shredded_and_reassembled_is_the_payload_again() {
    // `ReadingBatch { repeated Reading readings }` with two readings: shred it to facts under the
    // root `r0`, export the root with its marker, and reassemble — the bytes are the canonical
    // payload again (payload → facts → payload, the round-trip's forward half on the worked story).
    let codec = thermal_codec();
    let payload = wire::batch(&[wire::reading("s-1", 1), wire::reading("s-2", 2)]);
    let facts = codec
        .shred(
            "ReadingBatch",
            &payload,
            PayloadFormat::Binary,
            &Root::named(Name::new("r0").expect("an identifier")),
        )
        .expect("the batch shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(marker("emit_reading_batch", "r0"));

    let out = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("the facts reassemble");
    assert_eq!(out.messages().len(), 1);
    assert_eq!(out.messages()[0].type_name(), "thermal.v1.ReadingBatch");
    assert_eq!(out.messages()[0].bytes(), payload);
}

#[test]
fn a_reassembled_message_emits_the_canonical_form_omitting_a_zero_implicit_field() {
    // The shred materialises an implicit field's zero (§5), so the answer set carries `temp_c(r0, 0)`
    // even for a payload that omitted it; the reassembly builds `temp_c = 0` and encodes the
    // *canonical* proto3 form, which omits a zero implicit scalar. So a `Reading` with `temp_c = 0`
    // reassembles to the sensor field alone — the canonical form the round-trip is defined on, and
    // the semantic identity of the payload whether or not the zero was on the wire.
    let codec = thermal_codec();
    // A non-canonical payload that spells the zero out; the shred reads it, the reassembly drops it.
    let non_canonical = wire::reading("only-sensor", 0);
    let facts = codec
        .shred(
            "Reading",
            &non_canonical,
            PayloadFormat::Binary,
            &Root::named(Name::new("r0").expect("an identifier")),
        )
        .expect("the reading shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(marker("emit_reading", "r0"));

    let out = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("the facts reassemble");
    assert_eq!(out.messages().len(), 1);
    // The canonical form: the sensor field alone (field 1), the zero `temp_c` omitted.
    let mut canonical = Vec::new();
    wire::delimited(1, b"only-sensor", &mut canonical);
    assert_eq!(out.messages()[0].bytes(), canonical);
}

#[test]
fn the_reassembled_batch_round_trips_in_all_three_formats() {
    // The thermal `ReadingBatch` reassembles to each output form, and each shreds back to the same
    // facts — the binary, textproto, and JSON encoders are faithful inverses of the shred (three-way
    // round-trip parity, property 4/§26).
    let codec = thermal_codec();
    let payload = wire::batch(&[wire::reading("s-1", 1), wire::reading("s-2", 2)]);
    let root = Root::named(Name::new("r0").expect("an identifier"));
    let facts = codec
        .shred("ReadingBatch", &payload, PayloadFormat::Binary, &root)
        .expect("the batch shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(marker("emit_reading_batch", "r0"));
    for format in [
        PayloadFormat::Binary,
        PayloadFormat::Textproto,
        PayloadFormat::Json,
    ] {
        let out = codec
            .reassemble(&answer, format)
            .expect("the facts reassemble");
        assert_eq!(out.messages().len(), 1);
        let again = codec
            .shred("ReadingBatch", out.messages()[0].bytes(), format, &root)
            .expect("the reassembled bytes shred");
        assert_eq!(
            facts.symbols(),
            again.symbols(),
            "the batch round-trips in {format:?}"
        );
    }
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

/// Shred `payload` (of type `root_type`) to facts, export the root with `sort_marker`, reassemble
/// in `format`, and shred the reassembled bytes in `format`: the facts are identical iff `format`'s
/// encoder inverts the shred over the forms `payload` carries — the parity instrument, run per
/// format so a form faithful for one shape is proven faithful for all.
fn assert_round_trips_in(
    codec: &Codec,
    root_type: &str,
    sort_marker: &str,
    payload: &[u8],
    format: PayloadFormat,
) {
    let root = Root::named(Name::new("r0").expect("an identifier"));
    let facts = codec
        .shred(root_type, payload, PayloadFormat::Binary, &root)
        .expect("the payload shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(marker(sort_marker, "r0"));
    let out = codec
        .reassemble(&answer, format)
        .unwrap_or_else(|d| panic!("reassembles in {format:?}: {d:?}"));
    assert_eq!(out.messages().len(), 1, "one message per marker");
    let again = codec
        .shred(root_type, out.messages()[0].bytes(), format, &root)
        .unwrap_or_else(|d| panic!("the reassembled {format:?} bytes shred: {d:?}"));
    assert_eq!(
        facts.symbols(),
        again.symbols(),
        "the payload round-trips in {format:?}"
    );
}

#[test]
fn an_inventory_round_trips_its_maps_in_all_three_formats() {
    // `maps.proto`'s `Inventory`: `map<string, int32>` (two entries) and `map<int64, Item>` (a
    // message-valued map). Both reassemble and shred back to the same facts in every form — the
    // canonicalizers (the binary wire re-sort, the textproto sorter) and serde_json's ordered map
    // agree, so a map is key-ordered whichever form carries it.
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
    for format in FORMATS {
        assert_round_trips_in(&codec, "Inventory", "emit_inventory", &payload, format);
    }
}

#[test]
fn a_gauge_round_trips_its_enums_and_maps_in_all_three_formats() {
    // `obligations.proto`'s `Gauge`: a repeated scalar, a scalar map, an enum, a repeated enum, and
    // a map of enums — the composite forms the thermal batch does not carry. Each is faithful in
    // every output form (an enum is a name in JSON and textproto, a number on the wire; a map is
    // key-ordered), so the shape coverage of `codec_emit_shapes.rs` holds across all three forms.
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
    for format in FORMATS {
        assert_round_trips_in(&codec, "Gauge", "emit_gauge", &payload, format);
    }
}

#[test]
fn the_annotated_scalar_treatments_round_trip_in_all_three_formats() {
    // `floats.proto`'s `Amount`: the Increment-5 scalar treatments on both codec ends — a scaled
    // `float` (`(keryx.scale)`), an opaque `double` (`(keryx.opaque)`), a `NATIVE_CHECKED` unsigned
    // counter, and a scaled `map<string, float>` *value* (the annotation on a map targets the value,
    // a float being no legal map key). Each on-grid, in-range value shredded to facts and reassembled
    // is the payload again, in every output form — the byte-for-byte round trip (property 4) the float
    // honesty rests on, proven end to end past the `scalar` unit tests.
    let codec = fixture_codec("floats.proto");
    let mut payload = Vec::new();
    wire::float(1, 1.5, &mut payload); // ratio: on the grid at scale 3 (1500 / 1000)
    wire::double(2, 2.5, &mut payload); // note: opaque, the decimal "2.5"
    wire::uint64(3, 5, &mut payload); // count: native-checked, within clingo's i32
    let mut entry = Vec::new();
    delimited(1, b"k", &mut entry); // factors key "k"
    wire::float(2, 1.25, &mut entry); // factors value: on the grid at scale 2 (125 / 100)
    delimited(4, &entry, &mut payload);
    for format in FORMATS {
        assert_round_trips_in(&codec, "Amount", "emit_amount", &payload, format);
    }
}

#[test]
fn a_set_annotated_field_round_trips_in_all_forms() {
    // AlertSet.alerts carries `(keryx.set)` (§28): an AlertSet shreds to its occupancy and
    // membership facts and reassembles to the payload again — then shreds back to the same facts —
    // in binary, textproto, and JSON. AlertSet is wire-identical to ReadingBatch and Alert to
    // Reading, so the wire helpers serve both.
    let codec = thermal_codec();
    let payload = wire::batch(&[wire::reading("s-1", 1), wire::reading("s-2", 2)]);
    for format in FORMATS {
        assert_round_trips_in(&codec, "AlertSet", "emit_alert_set", &payload, format);
    }
}

#[test]
fn a_model_computed_set_reassembles_its_members_by_provenance() {
    // The set form's reason to exist (§4.1, §28), and the outbound half set production turns on: a model
    // *computing* a set names its members functionally, by their own provenance — occupancy
    // `alert(al(N))` and membership `alerts(out, al(N))` — not positional occupants. The
    // membership-join reach binds each member whatever its functor, the reassembler reads the
    // membership atoms and orders members by `Symbol::Ord`, and re-checks each well-sorted; a
    // sequence's occupancy-join could reach none of them.
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
        marker("emit_alert_set", "out"),
        atom("alert_set", vec![constant("out")]),
    ];
    // Provenance `al(1)` asserted before `al(0)`: the order must come from `Symbol::Ord`, not the
    // answer set.
    answer.extend(alert(1, "s-2", 2));
    answer.extend(alert(0, "s-1", 1));
    let out = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("the model-computed set reassembles");
    assert_eq!(out.messages().len(), 1);
    assert_eq!(out.messages()[0].type_name(), "thermal.v1.AlertSet");
    assert_eq!(
        out.messages()[0].bytes(),
        wire::batch(&[wire::reading("s-1", 1), wire::reading("s-2", 2)]),
        "members serialize in Symbol::Ord order of their provenance occupants: al(0) before al(1)"
    );
}
