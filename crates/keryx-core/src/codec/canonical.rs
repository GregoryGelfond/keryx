//! Canonical map-entry ordering for the outbound encode (property 5 — determinism / auditability).
//!
//! prost-reflect 0.16.5 holds a map field as a `HashMap<MapKey, Value>` and encodes its entries in
//! that map's iteration order (`for (key, value) in values`, `src/dynamic/message.rs`), which is not
//! a function of the map's contents — so two encodes of one answer set can differ in map-entry byte
//! order, and neither keryx's `Value::Map` nor the validating setter can steer it (the container
//! discards any order keryx imposes). keryx's inbound side sorts a map's entries by key on read
//! (`super::engine::present`); this restores the symmetry on write: after the engine encodes, keryx
//! re-orders every map field's wire entries by their key — recursively through nested messages and
//! message-valued map entries — so identical answer sets yield identical bytes.
//!
//! It runs over keryx's own freshly-encoded, well-formed bytes — never adversarial input, since the
//! reassembler validated the answer set and the engine produced these bytes — on the sized encode
//! thread (`super::engine::ENCODE_STACK`), so its native recursion, bounded by the same
//! reconstruction ceiling the message was built under, is covered by the same stack guarantee as the
//! engine's serializer beside it. It is total: any wire shape the engine's serializer never emits (a
//! group field, a truncated run) leaves the bytes unchanged rather than failing.

use std::collections::BTreeMap;

use prost_reflect::{FieldDescriptor, Kind, MessageDescriptor};

/// The protobuf field numbers of a map entry's key and value (the wire shape of `map<K, V>`).
const MAP_ENTRY_KEY: u32 = 1;
const MAP_ENTRY_VALUE: u32 = 2;

/// Protobuf wire types: a varint, a 64-bit fixed run, a length-delimited run, a 32-bit fixed run.
const WIRE_VARINT: u8 = 0;
const WIRE_I64: u8 = 1;
const WIRE_LEN: u8 = 2;
const WIRE_I32: u8 = 5;

/// Re-emit `bytes` — one encoded message of `descriptor` — with every map field's entries ordered by
/// key, recursively through nested messages. Total: a wire shape the engine's serializer never
/// produces (a group, a truncated field) leaves the bytes unchanged rather than failing.
pub(crate) fn canonicalize_map_order(bytes: &[u8], descriptor: &MessageDescriptor) -> Vec<u8> {
    canonicalize_message(bytes, descriptor).unwrap_or_else(|| bytes.to_vec())
}

/// One parsed wire field: its number, its wire type, and the bytes of its value — for a varint the
/// varint's own bytes, for a fixed run the fixed bytes, for a length-delimited run the inner bytes.
struct Field<'a> {
    number: u32,
    wire: u8,
    payload: &'a [u8],
}

/// An entry's sort key, decoded from a map entry's key field per its declared kind — so entries
/// order by key *value* (numeric, or lexicographic for strings), matching the inbound read-time sort
/// and the "maps by key" contract, not by their raw encoding. Within one map field every entry
/// yields the same variant, so the derived order compares like with like.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Signed(i64),
    Unsigned(u64),
    Text(Vec<u8>),
}

/// Re-emit one encoded message with map fields ordered by key, recursively. `None` on a wire shape
/// the engine's own output never carries (the caller then leaves the bytes unchanged).
fn canonicalize_message(bytes: &[u8], descriptor: &MessageDescriptor) -> Option<Vec<u8>> {
    let fields = parse_fields(bytes)?;
    // Group occurrences by field number, ascending (the engine's own emit order — its `fields` are a
    // `BTreeMap<u32, _>`), occurrence order preserved within a number so repeated fields stay ordered.
    let mut by_number: BTreeMap<u32, Vec<Field<'_>>> = BTreeMap::new();
    for field in fields {
        by_number.entry(field.number).or_default().push(field);
    }
    let mut out = Vec::with_capacity(bytes.len());
    for (number, records) in by_number {
        match descriptor.get_field(number) {
            Some(field) if field.is_map() => emit_sorted_map(&mut out, number, &records, &field)?,
            Some(field) => match field.kind() {
                Kind::Message(child) => {
                    for record in &records {
                        if record.wire == WIRE_LEN {
                            let inner = canonicalize_map_order(record.payload, &child);
                            write_len_field(&mut out, number, &inner);
                        } else {
                            write_field(&mut out, record);
                        }
                    }
                }
                _ => {
                    for record in &records {
                        write_field(&mut out, record);
                    }
                }
            },
            None => {
                for record in &records {
                    write_field(&mut out, record);
                }
            }
        }
    }
    Some(out)
}

/// Emit a map field's entries under `number`, ordered by key, each entry canonicalised (a
/// message-valued entry's nested maps ordered too).
fn emit_sorted_map(
    out: &mut Vec<u8>,
    number: u32,
    records: &[Field<'_>],
    field: &FieldDescriptor,
) -> Option<()> {
    let Kind::Message(entry) = field.kind() else {
        return None; // a map field's kind is its entry message
    };
    let key_kind = entry.get_field(MAP_ENTRY_KEY)?.kind();
    let value_message = match entry.get_field(MAP_ENTRY_VALUE)?.kind() {
        Kind::Message(message) => Some(message),
        _ => None,
    };
    let mut entries: Vec<(SortKey, Vec<u8>)> = Vec::with_capacity(records.len());
    for record in records {
        if record.wire != WIRE_LEN {
            return None; // a map entry is length-delimited
        }
        let fields = parse_fields(record.payload)?;
        let key = entry_sort_key(&fields, &key_kind);
        entries.push((key, rebuild_entry(&fields, value_message.as_ref())));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    for (_, entry) in entries {
        write_len_field(out, number, &entry);
    }
    Some(())
}

/// Re-emit a map entry's fields in number order (key then value, the engine's own order),
/// canonicalising a message value's nested maps.
fn rebuild_entry(fields: &[Field<'_>], value_message: Option<&MessageDescriptor>) -> Vec<u8> {
    let mut by_number: BTreeMap<u32, Vec<&Field<'_>>> = BTreeMap::new();
    for field in fields {
        by_number.entry(field.number).or_default().push(field);
    }
    let mut out = Vec::new();
    for (number, records) in by_number {
        for record in records {
            if number == MAP_ENTRY_VALUE
                && record.wire == WIRE_LEN
                && let Some(message) = value_message
            {
                let inner = canonicalize_map_order(record.payload, message);
                write_len_field(&mut out, number, &inner);
            } else {
                write_field(&mut out, record);
            }
        }
    }
    out
}

/// The sort key of a map entry — its key field decoded per `key_kind`, or that kind's default when
/// the key field is absent (a proto3 default key — 0, `false`, `""` — may be omitted on the wire).
fn entry_sort_key(fields: &[Field<'_>], key_kind: &Kind) -> SortKey {
    match fields.iter().find(|field| field.number == MAP_ENTRY_KEY) {
        Some(field) => decode_key(field, key_kind).unwrap_or_else(|| default_key(key_kind)),
        None => default_key(key_kind),
    }
}

/// Decode a map entry's key field to a comparable value per its declared kind. `None` on a wire
/// shape the kind never carries (unreachable for the engine's own output).
fn decode_key(field: &Field<'_>, kind: &Kind) -> Option<SortKey> {
    Some(match kind {
        Kind::Int32 | Kind::Int64 => SortKey::Signed(reinterpret(read_varint(field.payload)?.0)),
        Kind::Sint32 | Kind::Sint64 => SortKey::Signed(unzigzag(read_varint(field.payload)?.0)),
        Kind::Uint32 | Kind::Uint64 | Kind::Bool => {
            SortKey::Unsigned(read_varint(field.payload)?.0)
        }
        Kind::Fixed32 => SortKey::Unsigned(u64::from(u32::from_le_bytes(
            field.payload.try_into().ok()?,
        ))),
        Kind::Sfixed32 => SortKey::Signed(i64::from(i32::from_le_bytes(
            field.payload.try_into().ok()?,
        ))),
        Kind::Fixed64 => SortKey::Unsigned(u64::from_le_bytes(field.payload.try_into().ok()?)),
        Kind::Sfixed64 => SortKey::Signed(i64::from_le_bytes(field.payload.try_into().ok()?)),
        Kind::String => SortKey::Text(field.payload.to_vec()),
        _ => return None, // not a valid map key kind (proto restricts keys to integral/bool/string)
    })
}

/// The default sort key for a key kind — proto3's zero value, for an entry that omits its key.
fn default_key(kind: &Kind) -> SortKey {
    match kind {
        Kind::Int32
        | Kind::Int64
        | Kind::Sint32
        | Kind::Sint64
        | Kind::Sfixed32
        | Kind::Sfixed64 => SortKey::Signed(0),
        Kind::String => SortKey::Text(Vec::new()),
        _ => SortKey::Unsigned(0),
    }
}

/// Reinterpret a varint's bits as a two's-complement `i64` (an `int32`/`int64` key; a negative value
/// encodes as a full-width varint), without a wrapping `as` cast.
fn reinterpret(value: u64) -> i64 {
    i64::from_ne_bytes(value.to_ne_bytes())
}

/// Un-zigzag a `sint32`/`sint64` key's varint to its signed value.
fn unzigzag(value: u64) -> i64 {
    reinterpret(value >> 1) ^ -reinterpret(value & 1)
}

/// Parse a run of wire fields. `None` on a group field or a truncated run (the engine emits neither).
fn parse_fields(bytes: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut fields = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let (key, key_len) = read_varint(bytes.get(offset..)?)?;
        offset += key_len;
        let number = u32::try_from(key >> 3).ok()?;
        let wire = u8::try_from(key & 0x7).ok()?;
        let payload = match wire {
            WIRE_VARINT => {
                let (_, len) = read_varint(bytes.get(offset..)?)?;
                let slice = bytes.get(offset..)?.get(..len)?;
                offset += len;
                slice
            }
            WIRE_I64 => {
                let slice = bytes.get(offset..)?.get(..8)?;
                offset += 8;
                slice
            }
            WIRE_I32 => {
                let slice = bytes.get(offset..)?.get(..4)?;
                offset += 4;
                slice
            }
            WIRE_LEN => {
                let (len64, len_len) = read_varint(bytes.get(offset..)?)?;
                offset += len_len;
                let len = usize::try_from(len64).ok()?;
                let slice = bytes.get(offset..)?.get(..len)?;
                offset += len;
                slice
            }
            _ => return None, // a group (3/4) or an invalid wire type: the engine emits none
        };
        fields.push(Field {
            number,
            wire,
            payload,
        });
    }
    Some(fields)
}

/// Read a base-128 varint from the front of `bytes`, returning its value and the bytes it consumed.
/// `None` on an over-long or truncated varint (the engine emits neither).
fn read_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if shift >= 64 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
        shift += 7;
    }
    None
}

/// Append a base-128 varint.
fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = u8::try_from(value & 0x7f).expect("seven bits fit a byte");
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// Append a field key (number and wire type).
fn write_key(out: &mut Vec<u8>, number: u32, wire: u8) {
    write_varint(out, (u64::from(number) << 3) | u64::from(wire));
}

/// The length of a wire payload as a `u64` (a slice length always fits).
fn len_of(payload: &[u8]) -> u64 {
    u64::try_from(payload.len()).expect("a wire payload length fits u64")
}

/// Re-emit a parsed field verbatim — its key, then its length (for a length-delimited run) and its
/// payload.
fn write_field(out: &mut Vec<u8>, field: &Field<'_>) {
    write_key(out, field.number, field.wire);
    if field.wire == WIRE_LEN {
        write_varint(out, len_of(field.payload));
    }
    out.extend_from_slice(field.payload);
}

/// Append a length-delimited field: its key, its length, its payload.
fn write_len_field(out: &mut Vec<u8>, number: u32, payload: &[u8]) {
    write_key(out, number, WIRE_LEN);
    write_varint(out, len_of(payload));
    out.extend_from_slice(payload);
}

#[cfg(test)]
mod tests {
    use prost::Message as _;
    use prost_reflect::MessageDescriptor;
    use prost_types::{
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
        MessageOptions,
    };

    use super::{
        MAP_ENTRY_KEY, MAP_ENTRY_VALUE, WIRE_VARINT, canonicalize_map_order, parse_fields,
        read_varint, reinterpret, write_key, write_len_field, write_varint,
    };
    use crate::descriptor;

    /// A `MessageDescriptor` for `M { map<int32, int32> m = 1; }`.
    fn int_map_descriptor() -> MessageDescriptor {
        let scalar = |name: &str, number: i32| FieldDescriptorProto {
            name: Some(name.to_owned()),
            number: Some(number),
            label: Some(1),  // optional
            r#type: Some(5), // int32
            ..Default::default()
        };
        let entry = DescriptorProto {
            name: Some("MEntry".to_owned()),
            field: vec![scalar("key", 1), scalar("value", 2)],
            options: Some(MessageOptions {
                map_entry: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let map_field = FieldDescriptorProto {
            name: Some("m".to_owned()),
            number: Some(1),
            label: Some(3),   // repeated
            r#type: Some(11), // message
            type_name: Some(".t.M.MEntry".to_owned()),
            ..Default::default()
        };
        let set = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("t.proto".to_owned()),
                package: Some("t".to_owned()),
                syntax: Some("proto3".to_owned()),
                message_type: vec![DescriptorProto {
                    name: Some("M".to_owned()),
                    field: vec![map_field],
                    nested_type: vec![entry],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
        .encode_to_vec();
        let (_, pool) = descriptor::ingest_retaining(&set).expect("the set ingests");
        pool.message_by_name("t.M").expect("M is in the pool")
    }

    /// Encode a `map<int32, int32>` field (number 1) with `entries` in the given order.
    fn map_bytes(entries: &[(i64, i64)]) -> Vec<u8> {
        let mut out = Vec::new();
        for &(key, value) in entries {
            let mut entry = Vec::new();
            write_key(&mut entry, MAP_ENTRY_KEY, WIRE_VARINT);
            write_varint(&mut entry, u64::from_ne_bytes(key.to_ne_bytes()));
            write_key(&mut entry, MAP_ENTRY_VALUE, WIRE_VARINT);
            write_varint(&mut entry, u64::from_ne_bytes(value.to_ne_bytes()));
            write_len_field(&mut out, 1, &entry);
        }
        out
    }

    /// The map field's entry keys, in wire order, decoded as `int32`s.
    fn keys_in_order(bytes: &[u8]) -> Vec<i64> {
        parse_fields(bytes)
            .expect("well-formed")
            .into_iter()
            .filter(|field| field.number == 1)
            .map(|entry| {
                let inner = parse_fields(entry.payload).expect("a map entry parses");
                let key = inner
                    .iter()
                    .find(|field| field.number == MAP_ENTRY_KEY)
                    .expect("an entry carries its key");
                reinterpret(read_varint(key.payload).expect("a varint key").0)
            })
            .collect()
    }

    #[test]
    fn read_and_write_varint_round_trip() {
        for value in [0u64, 1, 127, 128, 300, 1 << 63, u64::MAX] {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            assert_eq!(read_varint(&buf).expect("round-trips"), (value, buf.len()));
        }
    }

    #[test]
    fn map_entries_are_reordered_to_key_order_deterministically() {
        // The same map in two wire orders — as prost-reflect's HashMap iteration would hand back —
        // canonicalises to identical bytes, entries ascending by key.
        let descriptor = int_map_descriptor();
        let one = map_bytes(&[(3, 30), (1, 10), (2, 20)]);
        let other = map_bytes(&[(2, 20), (3, 30), (1, 10)]);
        let canon_one = canonicalize_map_order(&one, &descriptor);
        let canon_other = canonicalize_map_order(&other, &descriptor);
        assert_eq!(
            canon_one, canon_other,
            "any input order canonicalises identically"
        );
        assert_eq!(
            keys_in_order(&canon_one),
            vec![1, 2, 3],
            "entries ascend by key"
        );
    }

    #[test]
    fn negative_int_keys_sort_by_value_not_by_encoding() {
        // A negative `int32` key encodes as a full-width varint (larger *bytes* than a small
        // positive), so a raw-byte sort would misorder it; keryx sorts by decoded value.
        let descriptor = int_map_descriptor();
        let canon = canonicalize_map_order(&map_bytes(&[(5, 1), (-1, 2), (0, 3)]), &descriptor);
        assert_eq!(keys_in_order(&canon), vec![-1, 0, 5]);
    }

    #[test]
    fn a_message_with_no_map_field_is_unchanged() {
        // A message the descriptor has no map for is returned byte-identical (nothing to reorder).
        let descriptor = int_map_descriptor();
        let mut bytes = Vec::new();
        write_key(&mut bytes, 7, WIRE_VARINT); // an unknown field number: copied through verbatim
        write_varint(&mut bytes, 42);
        assert_eq!(canonicalize_map_order(&bytes, &descriptor), bytes);
    }
}
