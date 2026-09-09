//! The outbound well-known-type boundary (spec §10, §26): `keryx emit --out json` refuses a WKT
//! value the reassembler admits but canonical JSON cannot represent — an out-of-range `Timestamp`
//! or `Duration`, or an `Any` whose `type_url` the pool cannot resolve — with `UnrepresentableJson`,
//! while the binary and text forms carry it structurally. keryx keeps `Any` opaque and never
//! range-checks a WKT at reassembly (that would special-case §10's structural model), so the refusal
//! lives at the JSON encode alone: the model is symmetric across the forms, each refusing only what
//! it alone cannot represent (the outbound counterpart of the inbound `.lp` dialect's
//! `UnrepresentableText`). The code fix behind these tests is that the JSON encode maps `serde_json`'s
//! serialization error to a diagnostic rather than an `expect` — an adversary-steerable WKT error is
//! a clean outbound refusal, never a contained-but-misattributed dependency fault.

use keryx_test_support as support;
use keryx_test_support::wire::{self, delimited};

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::diagnostics::{Diagnostic, DiagnosticKind};
use keryx_core::{Name, Sign, Symbol};

/// The `Boxed` fixture's codec: a `Timestamp` (`at`, #1), a `Duration` (`elapsed`, #2), and an
/// `Any` (`thing`, #3).
fn codec() -> Codec {
    Codec::new(&support::compile_fixture("emit_wkt.proto")).expect("the fixture builds a codec")
}

/// A constant symbol — a positive zero-argument function.
fn constant(name: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(name).expect("an identifier"),
        arguments: Vec::new(),
        sign: Sign::Positive,
    }
}

/// The `emit_boxed(r0)` marker exporting the tree under the fresh root `r0` — the same root the
/// shred below names.
fn marker() -> Symbol {
    Symbol::Function {
        name: Name::new("emit_boxed").expect("an identifier"),
        arguments: vec![constant("r0")],
        sign: Sign::Positive,
    }
}

/// Shred `payload` as a `Boxed` under the fresh root `r0`, then add the marker: the outbound door's
/// answer set, produced by the inbound door so the facts are exactly a real shred's.
fn answer_set(codec: &Codec, payload: &[u8]) -> Vec<Symbol> {
    let facts = codec
        .shred(
            "keryx.emit_wkt.Boxed",
            payload,
            PayloadFormat::Binary,
            &Root::fresh(0),
        )
        .expect("the boxed message shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(marker());
    answer
}

/// A `Timestamp` or `Duration` on the wire: `{ int64 seconds = 1; int32 nanos = 2; }`.
fn seconds_nanos(seconds: i64, nanos: i32) -> Vec<u8> {
    let mut buf = Vec::new();
    wire::int64(1, seconds, &mut buf);
    wire::int32(2, nanos, &mut buf);
    buf
}

/// Assert `answer` reassembles to one message in the binary and text forms (they carry the value
/// structurally) but is refused in JSON with `UnrepresentableJson` at the whole-message locus — the
/// symmetric-model boundary this increment closes.
fn only_json_refuses(codec: &Codec, answer: &[Symbol], what: &str) {
    for structural in [PayloadFormat::Binary, PayloadFormat::Textproto] {
        let out = codec
            .reassemble(answer, structural)
            .unwrap_or_else(|d| panic!("{structural:?} carries {what}: {d:?}"));
        assert_eq!(
            out.messages().len(),
            1,
            "one marker, one message ({structural:?}, {what})"
        );
    }
    let refusal = codec
        .reassemble(answer, PayloadFormat::Json)
        .expect_err(&format!("canonical JSON cannot represent {what}"));
    assert!(
        refusal
            .iter()
            .any(|d| d.kind() == DiagnosticKind::UnrepresentableJson),
        "the JSON refusal of {what} is UnrepresentableJson, not {:?}",
        refusal.iter().map(Diagnostic::kind).collect::<Vec<_>>()
    );
    // Named at the whole-message locus (the serializer gives no field path), never the value.
    assert!(
        refusal
            .iter()
            .filter(|d| d.kind() == DiagnosticKind::UnrepresentableJson)
            .all(|d| d.locus().is_whole()),
        "the UnrepresentableJson refusal of {what} names the whole-message locus"
    );
}

#[test]
fn an_out_of_range_timestamp_is_unrepresentable_in_json_only() {
    // seconds = 253_402_300_800 is one past 9999-12-31T23:59:59Z, the last instant canonical JSON
    // represents. The 64-bit seconds path admits it inbound (never range-checked, spec §6), and
    // binary and textproto emit the field structurally, but prost's `check_timestamp` rejects it —
    // it normalizes a nanos overflow into seconds, so it is the seconds date-range that bounds a
    // Timestamp's JSON representability.
    let codec = codec();
    let mut payload = Vec::new();
    delimited(1, &seconds_nanos(253_402_300_800, 0), &mut payload); // at = Timestamp, past year 9999
    let answer = answer_set(&codec, &payload);
    only_json_refuses(&codec, &answer, "an out-of-range Timestamp");
}

#[test]
fn an_out_of_range_duration_is_unrepresentable_in_json_only() {
    // A Duration's nanos, unlike a Timestamp's, are not normalized: nanos = 2_000_000_000 is out of
    // `check_duration`'s [-1e9, 1e9] range — a distinct canonical-JSON serializer, the same keryx
    // refusal, where binary and textproto emit the two fields structurally. The boundary is the WKT
    // family's, not one type's.
    let codec = codec();
    let mut payload = Vec::new();
    delimited(2, &seconds_nanos(0, 2_000_000_000), &mut payload); // elapsed = Duration
    let answer = answer_set(&codec, &payload);
    only_json_refuses(&codec, &answer, "an out-of-range Duration");
}

#[test]
fn an_unresolvable_any_is_unrepresentable_in_json_only() {
    // An `Any` whose `type_url` names a type the pool does not carry: keryx keeps it opaque
    // (type_url + payload) on the way in and reassembles it structurally, but canonical JSON must
    // resolve the type to emit the message, so `serialize_any` errors — where binary and textproto
    // emit the raw two fields.
    let codec = codec();
    let mut any = Vec::new();
    delimited(
        1,
        b"type.googleapis.com/keryx.emit_wkt.Nonexistent",
        &mut any,
    ); // type_url
    delimited(2, b"\x08\x2a", &mut any); // value — opaque bytes, never decoded
    let mut payload = Vec::new();
    delimited(3, &any, &mut payload); // thing = Any
    let answer = answer_set(&codec, &payload);
    only_json_refuses(&codec, &answer, "an unresolvable Any");
}
