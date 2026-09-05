//! Well-known types at the payload door (spec §10): a `google.protobuf.Timestamp`, `Duration`, or
//! wrapper a subject field references is an ordinary message — a sort with its scalar fields,
//! translated structurally, unconditionally — so its values shred as any message's do, under the
//! §6 defaults of their kinds: a `Timestamp`'s `int64 seconds` travels as a decimal string (the
//! 64-bit path — never range-checked, faithful past clingo's 32-bit integer width) and its `int32
//! nanos` as a native integer. No well-known type is special-cased on the way in: the same walk,
//! the same policy, the same path terms, the same occupancy atom.

use keryx_test_support as support;
use keryx_test_support::wire::{self, delimited};

use keryx_core::codec::{Codec, Facts, PayloadFormat, Root};
use keryx_core::descriptor::Scalar;
use keryx_core::policy::{Element, ScalarTreatment, ValueMapping};
use keryx_core::{Name, Sign, Symbol};

/// The well-known-type fixture's codec: an `Event` carrying a `Timestamp` (`at`, #2), a
/// `Duration` (`elapsed`, #3), and an `Int32Value` (`retries`, #4) beside its `name` (#1).
fn codec() -> Codec {
    Codec::new(&support::compile_fixture("well_known.proto")).expect("the fixture builds a codec")
}

/// An `Event` payload shredded from the fresh root `r0`.
fn shred(codec: &Codec, payload: &[u8]) -> Facts {
    codec
        .shred(
            "keryx.wkt.Event",
            payload,
            PayloadFormat::Binary,
            &Root::fresh(0),
        )
        .expect("the event shreds")
}

/// The proto kind and the §6 default treatment the mapping records for the scalar field at
/// `path` — the treatment the walk lowers its values under.
fn treatment(codec: &Codec, path: &str) -> (Scalar, ScalarTreatment) {
    let Some(Element::Field(field)) = codec.mapping().element(path) else {
        panic!("`{path}` is a field of the mapping")
    };
    let ValueMapping::Scalar { kind, treatment } = field.value() else {
        panic!("`{path}` is a scalar field")
    };
    (*kind, *treatment)
}

/// A `Timestamp` or `Duration` — `{ int64 seconds = 1; int32 nanos = 2; }` — on the wire.
fn seconds_nanos(seconds: i64, nanos: i32) -> Vec<u8> {
    let mut buf = Vec::new();
    wire::int64(1, seconds, &mut buf);
    wire::int32(2, nanos, &mut buf);
    buf
}

// Expected symbols, built as a client of keryx builds them: through the re-exported `Symbol`,
// `Name`, and `Sign` alone — nothing of themelios named directly (R1).

fn name(text: &str) -> Name {
    Name::new(text).expect("an identifier")
}

fn function(functor: &str, arguments: Vec<Symbol>) -> Symbol {
    Symbol::Function {
        name: name(functor),
        arguments,
        sign: Sign::Positive,
    }
}

fn constant(text: &str) -> Symbol {
    function(text, Vec::new())
}

fn text(value: &str) -> Symbol {
    Symbol::String(value.to_owned())
}

fn number(value: i32) -> Symbol {
    Symbol::Number(value)
}

/// The occupant term of the event's timestamp (§4.1): `at(r0)`.
fn at() -> Symbol {
    function("at", vec![constant("r0")])
}

/// Whether `facts` carries `symbol` on the symbol seam.
fn has(facts: &Facts, symbol: &Symbol) -> bool {
    facts.symbols().contains(symbol)
}

#[test]
fn a_timestamp_shreds_its_seconds_as_a_decimal_string_and_its_nanos_natively() {
    let codec = codec();
    // The mapping's record of the §6 defaults the walk lowers under: the 64-bit `seconds` a
    // decimal string, the 32-bit `nanos` native.
    assert_eq!(
        treatment(&codec, "google.protobuf.Timestamp.seconds"),
        (Scalar::Int64, ScalarTreatment::DecimalString)
    );
    assert_eq!(
        treatment(&codec, "google.protobuf.Timestamp.nanos"),
        (Scalar::Int32, ScalarTreatment::Native)
    );

    // `Event { name = "boot"; at = Timestamp { seconds = 1700000000; nanos = 500 } }`: the
    // timestamp is the occupant `at(r0)` with its occupancy atom, its seconds a string term and
    // its nanos an integer term — on the symbol seam, and spelled so on the `.lp` seam.
    let mut payload = Vec::new();
    delimited(1, b"boot", &mut payload);
    delimited(2, &seconds_nanos(1_700_000_000, 500), &mut payload);
    let facts = shred(&codec, &payload);
    assert!(
        has(&facts, &function("seconds", vec![at(), text("1700000000")])),
        "a string term, not an integer: {:?}",
        facts.symbols()
    );
    assert!(has(&facts, &function("nanos", vec![at(), number(500)])));
    assert!(has(&facts, &function("timestamp", vec![at()])));
    assert_eq!(
        facts.render().expect("renders"),
        "event(r0).\n\
         name(r0, \"boot\").\n\
         nanos(at(r0), 500).\n\
         seconds(at(r0), \"1700000000\").\n\
         timestamp(at(r0)).\n"
    );
}

#[test]
fn seconds_past_the_32_bit_width_travel_faithfully_and_nanos_fills_its_range_natively() {
    // The 64-bit path is never range-checked: the last second of the year 9999, the first of the
    // year 1, and the width's minimum are decimal strings past any native integer — refused
    // nowhere — while `nanos` at the top of its range stays a native integer.
    let codec = codec();
    for (seconds, spelled) in [
        (253_402_300_799_i64, "253402300799"),
        (-62_135_596_800, "-62135596800"),
        (i64::MIN, "-9223372036854775808"),
    ] {
        let mut payload = Vec::new();
        delimited(2, &seconds_nanos(seconds, 999_999_999), &mut payload);
        let facts = shred(&codec, &payload);
        assert!(
            has(&facts, &function("seconds", vec![at(), text(spelled)])),
            "{seconds}: {:?}",
            facts.symbols()
        );
        assert!(has(
            &facts,
            &function("nanos", vec![at(), number(999_999_999)])
        ));
    }
}

#[test]
fn every_well_known_referent_is_an_ordinary_sort() {
    // `Timestamp`, `Duration` (the same two fields under the same treatments), and the
    // `Int32Value` wrapper (`value`, native) are sorts of the mapping like any message: an event
    // carrying all three shreds each as an occupant with its occupancy atom and its fields, and
    // one carrying none emits none of them — every message field is partial (§5).
    let codec = codec();
    for path in [
        "google.protobuf.Timestamp",
        "google.protobuf.Duration",
        "google.protobuf.Int32Value",
    ] {
        assert!(
            matches!(codec.mapping().element(path), Some(Element::Sort(_))),
            "`{path}` is a sort of the mapping"
        );
    }
    assert_eq!(
        treatment(&codec, "google.protobuf.Duration.seconds"),
        (Scalar::Int64, ScalarTreatment::DecimalString)
    );
    assert_eq!(
        treatment(&codec, "google.protobuf.Duration.nanos"),
        (Scalar::Int32, ScalarTreatment::Native)
    );
    assert_eq!(
        treatment(&codec, "google.protobuf.Int32Value.value"),
        (Scalar::Int32, ScalarTreatment::Native)
    );

    let mut retries = Vec::new();
    wire::int32(1, 5, &mut retries);
    let mut payload = Vec::new();
    delimited(1, b"tick", &mut payload);
    delimited(2, &seconds_nanos(1, 2), &mut payload);
    delimited(3, &seconds_nanos(-3, 4), &mut payload);
    delimited(4, &retries, &mut payload);
    let facts = shred(&codec, &payload);
    assert_eq!(
        facts.render().expect("renders"),
        "duration(elapsed(r0)).\n\
         event(r0).\n\
         int32_value(retries(r0)).\n\
         name(r0, \"tick\").\n\
         nanos(at(r0), 2).\n\
         nanos(elapsed(r0), 4).\n\
         seconds(at(r0), \"1\").\n\
         seconds(elapsed(r0), \"-3\").\n\
         timestamp(at(r0)).\n\
         value(retries(r0), 5).\n"
    );

    let facts = shred(&codec, &[]);
    assert_eq!(
        facts.render().expect("renders"),
        "event(r0).\n\
         name(r0, \"\").\n"
    );
}
