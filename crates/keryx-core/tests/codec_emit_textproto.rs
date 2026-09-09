//! The textproto encode's map determinism (property 5), and the `expand_any(false)` that guards it.
//! `encode_textproto` sorts a message's map entries by key over prost-reflect's own text output
//! (`codec::canonical_text`), so identical answer sets yield identical bytes. Two door-level
//! instruments on real engine output: a map's entries come out key-ascending; and a *resolvable*
//! `google.protobuf.Any` beside a map stays in its raw `type_url`/`value` form rather than expanding
//! to `[type…]{…}` — the shape the canonicalizer cannot parse, which would silently drop the sort —
//! so the sibling map stays ordered and the output is deterministic (SR3-F1, CR3-F2).

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::{Name, Sign, Symbol};
use keryx_test_support as support;
use keryx_test_support::wire::{delimited, int32};

fn codec() -> Codec {
    Codec::new(&support::compile_fixture("emit_any_map.proto")).expect("the fixture builds a codec")
}

fn constant(name: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(name).expect("an identifier"),
        arguments: Vec::new(),
        sign: Sign::Positive,
    }
}

/// The `emit_outer(r0)` marker exporting the tree under the fresh root `r0`.
fn marker() -> Symbol {
    Symbol::Function {
        name: Name::new("emit_outer").expect("an identifier"),
        arguments: vec![constant("r0")],
        sign: Sign::Positive,
    }
}

/// A `map<string, int32>` entry on the wire: field-1 key, field-2 value.
fn entry(key: &str, value: i32) -> Vec<u8> {
    let mut buf = Vec::new();
    delimited(1, key.as_bytes(), &mut buf);
    int32(2, value, &mut buf);
    buf
}

/// Shred an `Outer` payload under the fresh root `r0`, then add the marker.
fn answer_set(codec: &Codec, payload: &[u8]) -> Vec<Symbol> {
    let facts = codec
        .shred(
            "keryx.emit_anymap.Outer",
            payload,
            PayloadFormat::Binary,
            &Root::fresh(0),
        )
        .expect("the outer shreds");
    let mut answer = facts.symbols().to_vec();
    answer.push(marker());
    answer
}

/// The textproto encode of the single reassembled message, as a string.
fn textproto(codec: &Codec, answer: &[Symbol]) -> String {
    let out = codec
        .reassemble(answer, PayloadFormat::Textproto)
        .expect("reassembles as textproto");
    assert_eq!(out.messages().len(), 1, "one marker, one message");
    String::from_utf8(out.messages()[0].bytes().to_vec()).expect("textproto is UTF-8")
}

/// The byte offset of the map key `key` (rendered `"key"`) in the output — its place in entry order.
fn key_pos(text: &str, key: &str) -> usize {
    text.find(&format!("\"{key}\""))
        .unwrap_or_else(|| panic!("the output carries key `{key}`: {text}"))
}

#[test]
fn a_maps_entries_come_out_key_ascending_in_textproto() {
    // The map atoms arrive out of order; the door's output has them key-ascending — asserted on real
    // engine output, not only that two input orders agree, so a canonicalizer silently disabled by a
    // grammar change fails here rather than passing quietly.
    let codec = codec();
    let mut payload = Vec::new();
    delimited(1, &entry("c", 3), &mut payload);
    delimited(1, &entry("a", 1), &mut payload);
    delimited(1, &entry("b", 2), &mut payload);
    let answer = answer_set(&codec, &payload);
    let text = textproto(&codec, &answer);
    assert!(
        key_pos(&text, "a") < key_pos(&text, "b") && key_pos(&text, "b") < key_pos(&text, "c"),
        "map entries are key-ascending in the textproto output: {text}"
    );
}

#[test]
fn a_resolvable_any_beside_a_map_stays_raw_and_keeps_the_map_ordered() {
    // `thing` is a *resolvable* Any — its type_url names `Point`, which the pool carries, and its
    // value decodes as one — so with `expand_any` on it would render `[type…]{…}`, a shape the
    // canonicalizer cannot parse, leaving the sibling map `m` in hash order. `expand_any(false)`
    // keeps it raw, so the map stays key-ascending and two atom orders reassemble byte-identically.
    let codec = codec();
    let point = {
        let mut buf = Vec::new();
        int32(1, 5, &mut buf); // Point { x = 5 }
        buf
    };
    let mut any = Vec::new();
    delimited(1, b"type.googleapis.com/keryx.emit_anymap.Point", &mut any); // type_url
    delimited(2, &point, &mut any); // value = an encoded Point, so the type_url resolves and decodes
    let mut payload = Vec::new();
    delimited(1, &entry("b", 2), &mut payload);
    delimited(1, &entry("a", 1), &mut payload);
    delimited(2, &any, &mut payload); // thing = Any
    let answer = answer_set(&codec, &payload);

    let text = textproto(&codec, &answer);
    // The Any stays raw — its two structural fields, never the expanded `[type…]{…}`.
    assert!(
        text.contains("type_url"),
        "the Any is in its raw form: {text}"
    );
    assert!(
        !text.contains("[type.googleapis.com"),
        "the Any is not expanded: {text}"
    );
    // So the sibling map stays key-ascending.
    assert!(
        key_pos(&text, "a") < key_pos(&text, "b"),
        "the sibling map is key-ascending: {text}"
    );
    // And the same answer set, its atoms reversed, reassembles byte-identically.
    let mut shuffled = answer.clone();
    shuffled.reverse();
    assert_eq!(
        text,
        textproto(&codec, &shuffled),
        "the Any-plus-map message reassembles byte-identically whatever order its atoms arrive in"
    );
}
