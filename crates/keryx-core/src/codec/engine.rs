//! The decode engine's adapter (architecture §5, inbound) — the one place in `codec` that names
//! prost-reflect. A payload is decoded against its root message's descriptor under foreign-fault
//! containment, and the decoded tree is presented in keryx's own **borrowing** value vocabulary:
//! [`Decoded`] owns the one decoded tree, and [`SubMessage`], [`FieldValue`], [`Element`], [`Key`],
//! and [`Datum`] borrow into it — so the walk and the §6 scalar policy read in keryx's terms, and
//! the engine stays swappable behind this file. No prost-reflect type crosses out of it: the
//! descriptor comes *in* through the retained pool's crate-internal seam
//! (`RetainedPool::message_by_name`), and only keryx types go out. Every message that seam
//! yields has passed the retaining door's pool-wide map-entry check, so the tree's shape — a
//! map value is a scalar or a message — holds for whatever root a caller names.
//!
//! **Borrowed, not cloned (the cost model).** A set field's value is read through the engine's
//! accessor, which yields a *borrow* of the stored value — prost-reflect 0.16.5
//! `src/dynamic/fields.rs:73-78` returns `Cow::Borrowed` for a stored field and materialises an
//! owned default only for an absent one (`src/dynamic/mod.rs:196-198` and `273-277` route there) —
//! so a sub-message is a handle over the root's tree, never a clone per ancestor level, and a walk
//! holds one tree however deep it goes.
//!
//! **No presence decision (spec §5).** [`SubMessage::value`] reads what the wire carried, or the
//! field kind's *zero value* when it carried nothing, uniformly; whether an atom exists is the
//! walk's decision from the mapping's totality, asked of [`SubMessage::is_present`] for a partial
//! field only. A declared default (proto2 or editions `default = …`) is never materialised inbound:
//! §5 assigns it to the generator's totalized view, not to the shred.

use std::borrow::Cow;

use prost_reflect::{
    DynamicMessage, FieldDescriptor, Kind, MapKey, MessageDescriptor, ReflectMessage as _, Value,
};

use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::fault::{Dependency, contain};

/// Decode a binary (`.binpb`) payload as an instance of `desc` — the payload door's engine call,
/// the one crossing into prost-reflect on the payload path — and return the tree, owned. The
/// engine bounds a payload's message nesting at its decode recursion limit (a message chain
/// `descriptor::RECURSION_LIMIT` deep or deeper is a decode error), so a binary payload this door
/// admits nests at most `RECURSION_LIMIT - 1` levels.
///
/// Total on foreign input (§6): a payload that does not decode as `desc` — malformed, truncated,
/// or over-deep — is `UndecodablePayload` at the whole-payload locus, and an unforeseen engine
/// fault is contained as a `DependencyFault` (the threat model's dependency boundary) rather than
/// unwinding into keryx's caller. The frame is defense-in-depth on this door: no payload is known
/// to fault the engine's binary decode, whose failures are values.
///
/// # Errors
///
/// `UndecodablePayload` when the bytes do not decode as `desc`; `DependencyFault` for a contained
/// engine panic.
// The codec's walk is this adapter's production caller and lands with it; until then every item
// of the adapter's surface is exercised only by this module's own tests, so each states its
// expectation for the library build alone (an unfulfilled expectation is itself a lint) and
// retires, item by item, as the walk consumes it.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no production caller until the codec lands")
)]
pub(crate) fn decode_binary(
    desc: &MessageDescriptor,
    bytes: &[u8],
) -> Result<Decoded, Diagnostics> {
    // The closure borrows only `desc` (a handle over the pool's shared state, cloned in) and
    // `bytes`; a fault drops the half-built message with the unwind, so nothing keryx observes
    // survives it. A binary decode reads the pool only through that handle — the engine's global
    // well-known-type pool is consulted on the descriptor's option decode and on the text and JSON
    // formats, never here — so no process-global state can be left inconsistent, and keryx's own
    // logic inside the frame is one infallible clone; the `AssertUnwindSafe` is sound.
    let decoded = contain(Dependency::ProstReflect, "decoding a payload", || {
        DynamicMessage::decode(desc.clone(), bytes)
    })?;
    decoded
        .map(|root| Decoded { root })
        .map_err(|error| undecodable(desc, &error.to_string()))
}

/// Compose the `UndecodablePayload` for bytes that did not decode as `desc`: the whole-payload
/// locus (the wire itself is unreadable, so no field path is finer), naming the root type, with
/// the engine's own message composed into the detail (§6), never exposed as its type.
fn undecodable(desc: &MessageDescriptor, error: &str) -> Diagnostics {
    Diagnostic::new(
        DiagnosticKind::UndecodablePayload,
        Locus::whole(),
        format!(
            "the payload did not decode as `{}`: {error}",
            desc.full_name()
        ),
    )
    .into()
}

/// The one decoded tree of a payload, owned. Every view beneath it borrows from here — the root
/// handle, each sub-message, each datum — so the tree is decoded once and never copied, and it
/// lives as long as the walk that reads it.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no production caller until the codec lands")
)]
pub(crate) struct Decoded {
    root: DynamicMessage,
}

impl Decoded {
    /// The root message as a borrowing handle — the walk's first work item, from which every
    /// sub-message it reaches is a handle over this same tree.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no production caller until the codec lands")
    )]
    pub(crate) fn root(&self) -> SubMessage<'_> {
        SubMessage(&self.root)
    }

    /// As [`SubMessage::is_present`], asked of the root.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no production caller until the codec lands")
    )]
    pub(crate) fn is_present(&self, number: i32) -> bool {
        self.root().is_present(number)
    }

    /// As [`SubMessage::value`], asked of the root.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no production caller until the codec lands")
    )]
    pub(crate) fn value(&self, number: i32) -> Option<FieldValue<'_>> {
        self.root().value(number)
    }
}

/// A message within the decoded tree — the root, or a sub-message reached from it — as a
/// copyable **borrowing** handle. `'a` is the tree's lifetime, not the handle's: a value read
/// through a handle borrows the tree, so a walk can hold the child it read beside the parent it
/// read it from, and let go of either first.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no production caller until the codec lands")
)]
pub(crate) struct SubMessage<'a>(&'a DynamicMessage);

impl<'a> SubMessage<'a> {
    /// The engine's presence for the field numbered `number`: for a field with explicit presence
    /// (a message-typed field, a `oneof` arm, a proto3 `optional`, every proto2 singular field),
    /// whether the wire carried it; for a field without (an IMPLICIT scalar, a list, a map),
    /// whether its value is non-default — the engine's notion, which the walk asks only of a
    /// partial field (spec §5: presence is decided from the mapping's totality, never here).
    /// `false` for a number the message does not declare, and for a negative one.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no production caller until the codec lands")
    )]
    pub(crate) fn is_present(self, number: i32) -> bool {
        u32::try_from(number).is_ok_and(|number| self.0.has_field_by_number(number))
    }

    /// The value of the field numbered `number`, in keryx's vocabulary, borrowing the tree: the
    /// value the wire carried, or — when it carried none — the field kind's zero value (spec §5's
    /// materialised default for an IMPLICIT field: `0`, `false`, `""`, no bytes, an enum's first
    /// value, an empty list or map), read the same way whatever the field's presence, so the view
    /// decides nothing about presence. `None` when the message declares no field of this number
    /// (or the number is negative), and for a singular **message** field the wire did not carry:
    /// a message has no zero value — its absence is its zero — and every message-typed field has
    /// explicit presence, so the walk asks [`is_present`](Self::is_present) first and never reads
    /// an absent one.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no production caller until the codec lands")
    )]
    pub(crate) fn value(self, number: i32) -> Option<FieldValue<'a>> {
        let number = u32::try_from(number).ok()?;
        let field = self.0.descriptor().get_field(number)?;
        match self.0.get_field(&field) {
            Cow::Borrowed(value) => Some(present(value)),
            Cow::Owned(_) => zero(&field),
        }
    }
}

/// A field's value, in keryx's vocabulary, borrowing the decoded tree — what
/// [`SubMessage::value`] reads.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no production caller until the codec lands")
)]
pub(crate) enum FieldValue<'a> {
    /// A singular scalar or enum value — the wire's, or the kind's zero.
    Scalar(Datum<'a>),
    /// A singular message the wire carried, as a handle over the tree.
    Message(SubMessage<'a>),
    /// A repeated field's elements, in wire order — the sequence's index order (spec §7.1).
    Elements(Vec<Element<'a>>),
    /// A map field's entries, sorted by key: the engine's map is unordered, so keryx orders it
    /// once here and a payload's facts are the same whatever the wire's (or the engine's table's)
    /// order — the determinism the threat model requires.
    Entries(Vec<(Key<'a>, Element<'a>)>),
}

/// A repeated field's element, or a map's value: a scalar or a message — never a list or a map,
/// which protobuf's grammar puts only at field level (the retaining door refuses, over the whole
/// pool, the one crafted shape — a map entry with a repeated value field — that would put one in
/// value position).
#[derive(Clone, Copy, Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no production caller until the codec lands")
)]
pub(crate) enum Element<'a> {
    /// A scalar or enum element.
    Scalar(Datum<'a>),
    /// A message element, as a handle over the tree.
    Message(SubMessage<'a>),
}

/// A map key (spec §7.2), in keryx's vocabulary — protobuf admits only integral, `bool`, and
/// `string` keys, and the descriptor door refuses any other. `Ord` orders the entries of one map,
/// whose keys share a kind; the derived order across kinds is never exercised.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no production caller until the codec lands")
)]
pub(crate) enum Key<'a> {
    /// A `bool` key.
    Bool(bool),
    /// An `int32`, `sint32`, or `sfixed32` key.
    I32(i32),
    /// An `int64`, `sint64`, or `sfixed64` key.
    I64(i64),
    /// A `uint32` or `fixed32` key.
    U32(u32),
    /// A `uint64` or `fixed64` key.
    U64(u64),
    /// A `string` key, borrowed.
    Str(&'a str),
}

/// A scalar value as the wire carried it, or a kind's zero, in keryx's vocabulary — the §6
/// scalar policy's input. `Str` and `Bytes` borrow the tree. `float` is widened to `F64`
/// (lossless) — harmless while `float` and `double` are refused unannotated (§6), though a
/// fixed-point `(keryx.scale)` range check differs between the two widths, so the origin width
/// may need carrying when that annotation lands. An enum value travels as its number; the walk
/// resolves it against the enum's mapping (§7.4).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no production caller until the codec lands")
)]
pub(crate) enum Datum<'a> {
    /// An `int32`, `sint32`, or `sfixed32` value.
    I32(i32),
    /// An `int64`, `sint64`, or `sfixed64` value.
    I64(i64),
    /// A `uint32` or `fixed32` value.
    U32(u32),
    /// A `uint64` or `fixed64` value.
    U64(u64),
    /// A `double` value, or a `float` widened.
    F64(f64),
    /// A `bool` value.
    Bool(bool),
    /// A `string` value, borrowed.
    Str(&'a str),
    /// A `bytes` value, borrowed.
    Bytes(&'a [u8]),
    /// An enum value, by number.
    Enum(i32),
}

/// A stored field value, borrowed, in keryx's vocabulary.
fn present(value: &Value) -> FieldValue<'_> {
    match value {
        Value::Message(message) => FieldValue::Message(SubMessage(message)),
        Value::List(items) => FieldValue::Elements(items.iter().map(element).collect()),
        Value::Map(map) => {
            let mut entries: Vec<(Key<'_>, Element<'_>)> = map
                .iter()
                .map(|(map_key, value)| (key(map_key), element(value)))
                .collect();
            entries.sort_unstable_by_key(|(key, _)| *key);
            FieldValue::Entries(entries)
        }
        scalar => FieldValue::Scalar(datum(scalar)),
    }
}

/// A repeated element or a map value, borrowed: a scalar or a message (see [`Element`]).
fn element(value: &Value) -> Element<'_> {
    match value {
        Value::Message(message) => Element::Message(SubMessage(message)),
        scalar => Element::Scalar(datum(scalar)),
    }
}

/// A stored scalar, borrowed, as its [`Datum`]. A list or a map never reaches here: at field
/// level [`present`] takes them first, and in element position protobuf's grammar excludes them
/// (a list element is of its field's kind, and the retaining door refuses, pool-wide, a map entry
/// whose key or value field is repeated — so no descriptor a retained pool yields can decode one,
/// whatever root a caller names) — the `unreachable` states that invariant, a keryx error and
/// never foreign input.
fn datum(value: &Value) -> Datum<'_> {
    match value {
        Value::Bool(value) => Datum::Bool(*value),
        Value::I32(value) => Datum::I32(*value),
        Value::I64(value) => Datum::I64(*value),
        Value::U32(value) => Datum::U32(*value),
        Value::U64(value) => Datum::U64(*value),
        Value::F32(value) => Datum::F64(f64::from(*value)),
        Value::F64(value) => Datum::F64(*value),
        Value::String(value) => Datum::Str(value),
        Value::Bytes(value) => Datum::Bytes(value),
        Value::EnumNumber(value) => Datum::Enum(*value),
        Value::Message(_) | Value::List(_) | Value::Map(_) => {
            unreachable!("a list or map is a field's value, never an element's")
        }
    }
}

/// A stored map key, borrowed, as its [`Key`].
fn key(key: &MapKey) -> Key<'_> {
    match key {
        MapKey::Bool(value) => Key::Bool(*value),
        MapKey::I32(value) => Key::I32(*value),
        MapKey::I64(value) => Key::I64(*value),
        MapKey::U32(value) => Key::U32(*value),
        MapKey::U64(value) => Key::U64(*value),
        MapKey::String(value) => Key::Str(value),
    }
}

/// The zero value of a field the wire did not carry, in keryx's vocabulary — what an IMPLICIT
/// field materialises (spec §5): an empty list or map, the kind's zero scalar (for an enum, its
/// first declared value — the pool's build refuses an enum with none), and `None` for a singular
/// message, which has no zero value. The kind's zero, never a declared default. Mirrors the
/// engine's own `Kind::default_value` (prost-reflect 0.16.5 `src/descriptor/api.rs:117-131`)
/// without materialising anything.
fn zero(field: &FieldDescriptor) -> Option<FieldValue<'static>> {
    if field.is_list() {
        return Some(FieldValue::Elements(Vec::new()));
    }
    if field.is_map() {
        return Some(FieldValue::Entries(Vec::new()));
    }
    let datum = match field.kind() {
        Kind::Double | Kind::Float => Datum::F64(0.0),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => Datum::I32(0),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => Datum::I64(0),
        Kind::Uint32 | Kind::Fixed32 => Datum::U32(0),
        Kind::Uint64 | Kind::Fixed64 => Datum::U64(0),
        Kind::Bool => Datum::Bool(false),
        Kind::String => Datum::Str(""),
        Kind::Bytes => Datum::Bytes(&[]),
        Kind::Enum(enumeration) => Datum::Enum(enumeration.default_value().number()),
        Kind::Message(_) => return None,
    };
    Some(FieldValue::Scalar(datum))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::path::Path;

    use prost::Message as _;
    use prost::encoding::{self, WireType};
    use prost_reflect::{MessageDescriptor, Value};
    use prost_types::{
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
        MessageOptions,
    };

    use super::{Datum, Decoded, Element, FieldValue, Key, SubMessage, decode_binary};
    use crate::descriptor::{self, RetainedPool};
    use crate::diagnostics::DiagnosticKind;

    /// The thermal example's pool (spec §28), through the source door's retaining variant — the
    /// `ReadingBatch` of record, decoded against the very pool its schema came from.
    fn thermal_pool() -> RetainedPool {
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
        let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
        let (_, pool) = descriptor::source::compile_retaining(
            &[example.join("thermal.proto")],
            &[example, vendored],
        )
        .expect("the thermal example compiles");
        pool
    }

    /// A fixture's pool, through the descriptor door's retaining variant.
    fn fixture_pool(name: &str) -> RetainedPool {
        let (_, pool) = descriptor::ingest_retaining(&keryx_test_support::compile_fixture(name))
            .expect("the fixture ingests");
        pool
    }

    /// A hand-built pool carrying the kinds no fixture declares: the `uint32` and `double` scalars
    /// (`u` #1, `d` #2) and the `bool`, `int32`, `uint32`, and `uint64` map keys (`bools` #3,
    /// `ints` #4, `uints` #5, `longs` #6, each to an `int32` value).
    fn kinds_pool() -> RetainedPool {
        let field = |name: &str, number: i32, r#type: i32| FieldDescriptorProto {
            name: Some(name.to_owned()),
            number: Some(number),
            label: Some(1), // optional
            r#type: Some(r#type),
            ..Default::default()
        };
        let map = |name: &str, number: i32, entry: &str, key_type: i32| {
            let field = FieldDescriptorProto {
                name: Some(name.to_owned()),
                number: Some(number),
                label: Some(3),   // repeated
                r#type: Some(11), // message
                type_name: Some(format!(".k.Kinds.{entry}")),
                ..Default::default()
            };
            let entry = DescriptorProto {
                name: Some(entry.to_owned()),
                field: vec![field_of("key", 1, key_type), field_of("value", 2, 5)], // int32 value
                options: Some(MessageOptions {
                    map_entry: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            };
            (field, entry)
        };
        let (bools, bools_entry) = map("bools", 3, "BoolsEntry", 8); // bool
        let (ints, ints_entry) = map("ints", 4, "IntsEntry", 5); // int32
        let (uints, uints_entry) = map("uints", 5, "UintsEntry", 13); // uint32
        let (longs, longs_entry) = map("longs", 6, "LongsEntry", 4); // uint64
        let set = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("kinds.proto".to_owned()),
                package: Some("k".to_owned()),
                syntax: Some("proto3".to_owned()),
                message_type: vec![DescriptorProto {
                    name: Some("Kinds".to_owned()),
                    field: vec![
                        field("u", 1, 13), // uint32
                        field("d", 2, 1),  // double
                        bools,
                        ints,
                        uints,
                        longs,
                    ],
                    nested_type: vec![bools_entry, ints_entry, uints_entry, longs_entry],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
        .encode_to_vec();
        let (_, pool) = descriptor::ingest_retaining(&set).expect("the set ingests");
        pool
    }

    /// A singular field of a hand-built message, by name, number, and `FieldDescriptorProto` type.
    fn field_of(name: &str, number: i32, r#type: i32) -> FieldDescriptorProto {
        FieldDescriptorProto {
            name: Some(name.to_owned()),
            number: Some(number),
            label: Some(1), // optional
            r#type: Some(r#type),
            ..Default::default()
        }
    }

    fn descriptor_of(pool: &RetainedPool, name: &str) -> MessageDescriptor {
        pool.message_by_name(name).expect("a declared message")
    }

    // Wire-format builders over prost's encoding primitives — the payloads are written as bytes
    // on the wire, not through the engine's own encoder, so the door is seen to read the wire.

    fn delimited(tag: u32, payload: &[u8], buf: &mut Vec<u8>) {
        encoding::encode_key(tag, WireType::LengthDelimited, buf);
        encoding::encode_varint(
            u64::try_from(payload.len()).expect("a test payload fits"),
            buf,
        );
        buf.extend_from_slice(payload);
    }

    /// A thermal `Reading { sensor = 1; temp_c = 2 }` on the wire.
    fn reading(sensor: &str, temp_c: i32) -> Vec<u8> {
        let mut buf = Vec::new();
        delimited(1, sensor.as_bytes(), &mut buf);
        encoding::int32::encode(2, &temp_c, &mut buf);
        buf
    }

    // Extractors: each names the shape a test expects and fails loudly on any other.

    fn scalar(value: Option<FieldValue<'_>>) -> Datum<'_> {
        match value {
            Some(FieldValue::Scalar(datum)) => datum,
            other => panic!("a scalar, not {other:?}"),
        }
    }

    fn message_of(value: Option<FieldValue<'_>>) -> SubMessage<'_> {
        match value {
            Some(FieldValue::Message(message)) => message,
            other => panic!("a message, not {other:?}"),
        }
    }

    fn elements(value: Option<FieldValue<'_>>) -> Vec<Element<'_>> {
        match value {
            Some(FieldValue::Elements(elements)) => elements,
            other => panic!("elements, not {other:?}"),
        }
    }

    fn entries(value: Option<FieldValue<'_>>) -> Vec<(Key<'_>, Element<'_>)> {
        match value {
            Some(FieldValue::Entries(entries)) => entries,
            other => panic!("entries, not {other:?}"),
        }
    }

    fn element_scalar(element: Element<'_>) -> Datum<'_> {
        match element {
            Element::Scalar(datum) => datum,
            Element::Message(_) => panic!("a scalar element, not a message"),
        }
    }

    fn element_message(element: Element<'_>) -> SubMessage<'_> {
        match element {
            Element::Message(message) => message,
            Element::Scalar(datum) => panic!("a message element, not {datum:?}"),
        }
    }

    /// The walk's need in miniature: a value read through a sub-message handle borrows the *tree*,
    /// so it outlives the handle it was read through (the handle is a local here).
    fn note_of(decoded: &Decoded) -> Datum<'_> {
        let detail = message_of(decoded.value(5));
        scalar(detail.value(1))
    }

    #[test]
    fn a_reading_batch_reads_as_keryx_values() {
        // The spec's own payload (§28): two readings, a sequence of messages over two scalars.
        let pool = thermal_pool();
        let batch = descriptor_of(&pool, "thermal.v1.ReadingBatch");
        let mut bytes = Vec::new();
        delimited(1, &reading("s-101", 44), &mut bytes);
        delimited(1, &reading("s-107", 21), &mut bytes);
        let decoded = decode_binary(&batch, &bytes).expect("a well-formed batch decodes");
        assert!(
            decoded.is_present(1),
            "a set repeated field is present to the engine"
        );
        let readings = elements(decoded.value(1));
        assert_eq!(readings.len(), 2, "two elements, in wire order");
        let first = element_message(readings[0]);
        assert_eq!(scalar(first.value(1)), Datum::Str("s-101"));
        assert_eq!(scalar(first.value(2)), Datum::I32(44));
        let second = element_message(readings[1]);
        assert_eq!(scalar(second.value(1)), Datum::Str("s-107"));
        assert_eq!(scalar(second.value(2)), Datum::I32(21));
    }

    #[test]
    fn an_implicit_scalar_the_wire_did_not_carry_reads_as_its_zero() {
        // `temp_c` left unset on an IMPLICIT field: the value view materialises the zero (spec §5 —
        // the atom always exists, the default materialised), and decides nothing about presence;
        // the engine's own notion of presence for it is "non-default", which the walk never asks
        // of a total field.
        let pool = thermal_pool();
        let reading_desc = descriptor_of(&pool, "thermal.v1.Reading");
        let mut bytes = Vec::new();
        delimited(1, b"s-101", &mut bytes);
        let decoded = decode_binary(&reading_desc, &bytes).expect("decodes");
        assert_eq!(scalar(decoded.value(2)), Datum::I32(0));
        assert!(!decoded.is_present(2));
        // An empty payload: every scalar reads as its zero, a sequence as empty.
        let empty = decode_binary(&reading_desc, &[]).expect("an empty payload decodes");
        assert_eq!(scalar(empty.value(1)), Datum::Str(""));
        assert_eq!(scalar(empty.value(2)), Datum::I32(0));
        let batch = decode_binary(&descriptor_of(&pool, "thermal.v1.ReadingBatch"), &[])
            .expect("an empty batch decodes");
        assert!(elements(batch.value(1)).is_empty());
        assert!(!batch.is_present(1));
    }

    #[test]
    fn a_nested_message_is_borrowed_from_the_root_not_cloned() {
        let pool = fixture_pool("proto3.proto");
        let reading = descriptor_of(&pool, "keryx.p3.Reading");
        let mut detail = Vec::new();
        delimited(1, b"calibrated", &mut detail);
        let mut bytes = Vec::new();
        delimited(1, b"s-1", &mut bytes);
        delimited(5, &detail, &mut bytes);
        let decoded = decode_binary(&reading, &bytes).expect("decodes");
        assert!(decoded.is_present(5), "a set message field is present");
        let sub = message_of(decoded.value(5));
        assert_eq!(scalar(sub.value(1)), Datum::Str("calibrated"));
        assert_eq!(
            note_of(&decoded),
            Datum::Str("calibrated"),
            "the value borrows the tree, not the handle"
        );
        // The no-per-level-clone claim, pinned: the engine's accessor yields a *borrow* of the
        // stored sub-message (prost-reflect 0.16.5 `src/dynamic/fields.rs:73-78`), and the handle
        // points at the very message the root stores.
        let Some(Cow::Borrowed(Value::Message(stored))) = decoded.root.get_field_by_number(5)
        else {
            panic!("the engine materialised a set message field rather than borrowing it")
        };
        assert!(
            std::ptr::eq(sub.0, stored),
            "the handle is the stored message"
        );
    }

    #[test]
    fn a_value_is_read_free_of_presence_and_an_absent_message_has_no_zero() {
        let pool = fixture_pool("proto3.proto");
        let reading = descriptor_of(&pool, "keryx.p3.Reading");
        // Nothing carried: the proto3-optional scalar (#3) reads as its zero yet is not present;
        // the enum (#4) reads as its first value; the message field (#5) has no zero to
        // materialise — its absence is its zero — and is not present.
        let empty = decode_binary(&reading, &[]).expect("decodes");
        assert_eq!(scalar(empty.value(3)), Datum::I32(0));
        assert!(!empty.is_present(3));
        assert_eq!(scalar(empty.value(4)), Datum::Enum(0));
        assert!(empty.value(5).is_none());
        assert!(!empty.is_present(5));
        // The optional scalar carried as an explicit zero, and one `oneof` arm carried: the same
        // zero, now present — the value view decides nothing about presence; the other arm reads
        // as its zero and is not present.
        let mut bytes = Vec::new();
        encoding::int32::encode(3, &0, &mut bytes);
        delimited(6, b"dev", &mut bytes);
        let carried = decode_binary(&reading, &bytes).expect("decodes");
        assert_eq!(scalar(carried.value(3)), Datum::I32(0));
        assert!(carried.is_present(3));
        assert_eq!(scalar(carried.value(6)), Datum::Str("dev"));
        assert!(carried.is_present(6));
        assert_eq!(scalar(carried.value(7)), Datum::Str(""));
        assert!(!carried.is_present(7));
    }

    #[test]
    fn every_value_kind_reads_as_its_datum() {
        // The scalar-treatment fixture's `Sample` carries every value variant but two — `uint32`
        // and `double` come from the hand-built `Kinds` — each read once as its keryx datum, with
        // `float` widened to `F64`.
        let pool = fixture_pool("scalar_treatment.proto");
        let sample = descriptor_of(&pool, "keryx.scalars.Sample");
        let mut note_a = Vec::new();
        delimited(1, b"A", &mut note_a);
        let mut note_b = Vec::new();
        delimited(1, b"B", &mut note_b);
        let mut entry_b = Vec::new();
        delimited(1, b"b", &mut entry_b);
        encoding::int32::encode(2, &1, &mut entry_b);
        let mut entry_a = Vec::new();
        delimited(1, b"a", &mut entry_a);
        encoding::int32::encode(2, &0, &mut entry_a);
        let mut bytes = Vec::new();
        encoding::int32::encode(1, &7, &mut bytes);
        encoding::int64::encode(2, &-1, &mut bytes);
        encoding::uint64::encode(3, &u64::MAX, &mut bytes);
        encoding::float::encode(4, &1.5, &mut bytes);
        encoding::bool::encode(5, &true, &mut bytes);
        delimited(6, &[0xde, 0xad], &mut bytes);
        delimited(7, b"lbl", &mut bytes);
        encoding::int32::encode(8, &1, &mut bytes); // KIND_FIRST
        delimited(9, &note_a, &mut bytes);
        delimited(9, &note_b, &mut bytes);
        encoding::int32::encode_packed(10, &[1, 0], &mut bytes);
        delimited(11, &entry_b, &mut bytes);
        delimited(11, &entry_a, &mut bytes);
        let decoded = decode_binary(&sample, &bytes).expect("decodes");
        assert_eq!(scalar(decoded.value(1)), Datum::I32(7));
        assert_eq!(scalar(decoded.value(2)), Datum::I64(-1));
        assert_eq!(scalar(decoded.value(3)), Datum::U64(u64::MAX));
        assert_eq!(scalar(decoded.value(4)), Datum::F64(1.5));
        assert_eq!(scalar(decoded.value(5)), Datum::Bool(true));
        assert_eq!(scalar(decoded.value(6)), Datum::Bytes(&[0xde, 0xad]));
        assert_eq!(scalar(decoded.value(7)), Datum::Str("lbl"));
        assert_eq!(scalar(decoded.value(8)), Datum::Enum(1));
        let notes: Vec<Datum<'_>> = elements(decoded.value(9))
            .into_iter()
            .map(|element| scalar(element_message(element).value(1)))
            .collect();
        assert_eq!(notes, [Datum::Str("A"), Datum::Str("B")]);
        let kinds: Vec<Datum<'_>> = elements(decoded.value(10))
            .into_iter()
            .map(element_scalar)
            .collect();
        assert_eq!(kinds, [Datum::Enum(1), Datum::Enum(0)]);
        let tags: Vec<(Key<'_>, Datum<'_>)> = entries(decoded.value(11))
            .into_iter()
            .map(|(key, element)| (key, element_scalar(element)))
            .collect();
        assert_eq!(
            tags,
            [
                (Key::Str("a"), Datum::Enum(0)),
                (Key::Str("b"), Datum::Enum(1))
            ]
        );

        let pool = kinds_pool();
        let kinds = descriptor_of(&pool, "k.Kinds");
        let mut bytes = Vec::new();
        encoding::uint32::encode(1, &u32::MAX, &mut bytes);
        encoding::double::encode(2, &2.5, &mut bytes);
        let decoded = decode_binary(&kinds, &bytes).expect("decodes");
        assert_eq!(scalar(decoded.value(1)), Datum::U32(u32::MAX));
        assert_eq!(scalar(decoded.value(2)), Datum::F64(2.5));
    }

    #[test]
    fn an_empty_payload_reads_every_kind_as_its_zero() {
        let pool = fixture_pool("scalar_treatment.proto");
        let sample = descriptor_of(&pool, "keryx.scalars.Sample");
        let decoded = decode_binary(&sample, &[]).expect("an empty payload decodes");
        assert_eq!(scalar(decoded.value(1)), Datum::I32(0));
        assert_eq!(scalar(decoded.value(2)), Datum::I64(0));
        assert_eq!(scalar(decoded.value(3)), Datum::U64(0));
        assert_eq!(scalar(decoded.value(4)), Datum::F64(0.0));
        assert_eq!(scalar(decoded.value(5)), Datum::Bool(false));
        assert_eq!(scalar(decoded.value(6)), Datum::Bytes(&[]));
        assert_eq!(scalar(decoded.value(7)), Datum::Str(""));
        assert_eq!(scalar(decoded.value(8)), Datum::Enum(0));
        assert!(elements(decoded.value(9)).is_empty());
        assert!(elements(decoded.value(10)).is_empty());
        assert!(entries(decoded.value(11)).is_empty());
        let pool = kinds_pool();
        let decoded =
            decode_binary(&descriptor_of(&pool, "k.Kinds"), &[]).expect("an empty payload decodes");
        assert_eq!(scalar(decoded.value(1)), Datum::U32(0));
        assert_eq!(scalar(decoded.value(2)), Datum::F64(0.0));
    }

    #[test]
    fn map_entries_read_key_sorted_regardless_of_wire_order() {
        // The engine's map is unordered; keryx orders the entries by key once here, so the same
        // payload always reads the same way whatever the wire (or the engine's table) order.
        let pool = fixture_pool("maps.proto");
        let inventory = descriptor_of(&pool, "keryx.maps.Inventory");
        let count = |key: &[u8], value: i32| {
            let mut entry = Vec::new();
            delimited(1, key, &mut entry);
            encoding::int32::encode(2, &value, &mut entry);
            entry
        };
        let item = |key: i64, sku: &[u8]| {
            let mut item = Vec::new();
            delimited(1, sku, &mut item);
            let mut entry = Vec::new();
            encoding::int64::encode(1, &key, &mut entry);
            delimited(2, &item, &mut entry);
            entry
        };
        let mut bytes = Vec::new();
        delimited(1, &count(b"b", 2), &mut bytes);
        delimited(1, &count(b"a", 1), &mut bytes);
        delimited(1, &count(b"c", 3), &mut bytes);
        delimited(2, &item(20, b"x"), &mut bytes);
        delimited(2, &item(-1, b"y"), &mut bytes);
        delimited(2, &item(3, b"z"), &mut bytes);
        let decoded = decode_binary(&inventory, &bytes).expect("decodes");
        let counts: Vec<(Key<'_>, Datum<'_>)> = entries(decoded.value(1))
            .into_iter()
            .map(|(key, element)| (key, element_scalar(element)))
            .collect();
        assert_eq!(
            counts,
            [
                (Key::Str("a"), Datum::I32(1)),
                (Key::Str("b"), Datum::I32(2)),
                (Key::Str("c"), Datum::I32(3)),
            ]
        );
        let items: Vec<(Key<'_>, Datum<'_>)> = entries(decoded.value(2))
            .into_iter()
            .map(|(key, element)| (key, scalar(element_message(element).value(1))))
            .collect();
        assert_eq!(
            items,
            [
                (Key::I64(-1), Datum::Str("y")),
                (Key::I64(3), Datum::Str("z")),
                (Key::I64(20), Datum::Str("x")),
            ]
        );
    }

    #[test]
    fn every_map_key_kind_reads_as_its_key_in_order() {
        // The four key kinds no fixture declares, each map written with its entries out of order
        // (and one all-default entry), read back key-sorted — unsigned keys by magnitude, `false`
        // before `true`.
        let pool = kinds_pool();
        let kinds = descriptor_of(&pool, "k.Kinds");
        let entry = |key: &dyn Fn(&mut Vec<u8>), value: i32| {
            let mut entry = Vec::new();
            key(&mut entry);
            encoding::int32::encode(2, &value, &mut entry);
            entry
        };
        let mut bytes = Vec::new();
        delimited(
            3,
            &entry(&|e| encoding::bool::encode(1, &true, e), 1),
            &mut bytes,
        );
        delimited(
            3,
            &entry(&|e| encoding::bool::encode(1, &false, e), 0),
            &mut bytes,
        );
        delimited(
            4,
            &entry(&|e| encoding::int32::encode(1, &5, e), 50),
            &mut bytes,
        );
        delimited(
            4,
            &entry(&|e| encoding::int32::encode(1, &-5, e), -50),
            &mut bytes,
        );
        delimited(
            5,
            &entry(&|e| encoding::uint32::encode(1, &u32::MAX, e), 1),
            &mut bytes,
        );
        delimited(
            5,
            &entry(&|e| encoding::uint32::encode(1, &0, e), 0),
            &mut bytes,
        );
        delimited(
            6,
            &entry(&|e| encoding::uint64::encode(1, &u64::MAX, e), 1),
            &mut bytes,
        );
        delimited(
            6,
            &entry(&|e| encoding::uint64::encode(1, &1, e), 2),
            &mut bytes,
        );
        let decoded = decode_binary(&kinds, &bytes).expect("decodes");
        let read = |number: i32| -> Vec<(Key<'_>, Datum<'_>)> {
            entries(decoded.value(number))
                .into_iter()
                .map(|(key, element)| (key, element_scalar(element)))
                .collect()
        };
        assert_eq!(
            read(3),
            [
                (Key::Bool(false), Datum::I32(0)),
                (Key::Bool(true), Datum::I32(1))
            ]
        );
        assert_eq!(
            read(4),
            [
                (Key::I32(-5), Datum::I32(-50)),
                (Key::I32(5), Datum::I32(50))
            ]
        );
        assert_eq!(
            read(5),
            [
                (Key::U32(0), Datum::I32(0)),
                (Key::U32(u32::MAX), Datum::I32(1))
            ]
        );
        assert_eq!(
            read(6),
            [
                (Key::U64(1), Datum::I32(2)),
                (Key::U64(u64::MAX), Datum::I32(1))
            ]
        );
    }

    #[test]
    fn a_malformed_payload_is_undecodable_at_the_whole_payload_locus() {
        // A length-delimited key promising five bytes the payload does not carry: the engine's
        // decode error becomes `UndecodablePayload` at the whole-payload locus, naming the root
        // type, with the engine's message composed into the detail — one diagnosis, no panic.
        let pool = thermal_pool();
        let batch = descriptor_of(&pool, "thermal.v1.ReadingBatch");
        let diagnostics =
            decode_binary(&batch, &[0x0a, 0x05, b's']).expect_err("a truncated payload is refused");
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.iter().next().unwrap();
        assert_eq!(diagnostic.kind(), DiagnosticKind::UndecodablePayload);
        assert!(diagnostic.locus().is_whole());
        assert!(
            diagnostic.detail().contains("thermal.v1.ReadingBatch"),
            "the detail names the root type: {diagnostic}"
        );
    }

    #[test]
    fn an_undeclared_or_negative_number_reads_as_absent() {
        let pool = thermal_pool();
        let reading = descriptor_of(&pool, "thermal.v1.Reading");
        let decoded = decode_binary(&reading, &[]).expect("decodes");
        assert!(decoded.value(99).is_none());
        assert!(decoded.value(-1).is_none());
        assert!(!decoded.is_present(99));
        assert!(!decoded.is_present(-1));
    }
}
