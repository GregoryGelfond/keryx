//! Canonical map-entry ordering for the outbound textproto encode (property 5 — determinism).
//!
//! prost-reflect 0.16.5 emits a map field's entries in its `HashMap`'s iteration order
//! (`to_text_format`, `src/dynamic/text_format/format.rs:123`, `map.iter()`), which is not a function
//! of the map's contents — as the binary and JSON encoders would too. The binary form is fixed by a
//! wire re-sort (`super::canonical`) and the JSON form by routing through `serde_json`'s sorted
//! `BTreeMap` intermediate; the text format has no such intermediate, so keryx re-orders each map
//! field's `[…]` entries by key over the engine's own compact-textproto output, recursively through
//! nested messages — the outbound mirror of the inbound read-time sort, for the text form.
//!
//! It runs over keryx's own well-formed output — never adversarial input, since the reassembler
//! validated the answer set and the engine produced this text — and is total: any shape the engine's
//! writer does not produce leaves the text unchanged rather than failing.
//!
//! The compact grammar it walks (`format.rs`, `pretty = false`, `skip_default_fields = true`): a
//! message body is comma-separated fields; a singular scalar is `name:value`, a singular message
//! `name{body}`, and a repeated or map field `name:[elem,…]` — a map element being `{key:K,value:V}`
//! (or `{key:K,value{…}}` for a message value). Only map-field element order varies; everything else
//! is a deterministic function of the message.

use prost_reflect::{Kind, MessageDescriptor};

/// Re-emit `text` — one message of `descriptor` in the engine's compact text format — with every map
/// field's entries ordered by key, recursively. Total: text the engine's writer does not produce
/// leaves it unchanged.
pub(crate) fn canonicalize_text(text: &str, descriptor: &MessageDescriptor) -> String {
    canonicalize_body(text, descriptor).unwrap_or_else(|| text.to_owned())
}

/// Re-emit a message body (its comma-separated fields) with map fields ordered. `None` on a shape the
/// grammar above does not admit (the caller then leaves the text unchanged).
fn canonicalize_body(body: &str, descriptor: &MessageDescriptor) -> Option<String> {
    if body.is_empty() {
        return Some(String::new());
    }
    let fields = split_top(body, ',');
    let mut out = Vec::with_capacity(fields.len());
    for field in fields {
        out.push(canonicalize_field(field, descriptor)?);
    }
    Some(out.join(","))
}

/// Re-emit one field, ordering it if it is a map. `None` on an unparseable field.
fn canonicalize_field(field: &str, descriptor: &MessageDescriptor) -> Option<String> {
    let (name, sep, rest) = split_name(field)?;
    match sep {
        // `name{body}` — a singular message field: recurse into its body.
        '{' => {
            let inner = strip(rest, '{', '}')?;
            let child = message_kind(descriptor.get_field_by_name(name))?;
            Some(format!("{name}{{{}}}", canonicalize_body(inner, &child)?))
        }
        // `rest` begins with the separator, so a repeated/map list is `:[…]` and a scalar `:value`.
        ':' => match rest.strip_prefix(":[") {
            Some(after) => {
                let inner = after.strip_suffix(']')?;
                Some(format!(
                    "{name}:[{}]",
                    canonicalize_list(inner, name, descriptor)?
                ))
            }
            None => Some(field.to_owned()), // a singular scalar: verbatim
        },
        _ => Some(field.to_owned()),
    }
}

/// Re-emit a `[…]` list's elements: a map field's ordered by key (each message value recursed), a
/// repeated message field's recursed in place, a scalar list verbatim.
fn canonicalize_list(inner: &str, name: &str, descriptor: &MessageDescriptor) -> Option<String> {
    let field = descriptor.get_field_by_name(name)?;
    if inner.is_empty() {
        return Some(String::new());
    }
    let elements = split_top(inner, ',');
    if field.is_map() {
        let value_kind = map_value_kind(&field);
        let mut entries: Vec<(SortKey, String)> = Vec::with_capacity(elements.len());
        for element in elements {
            entries.push((
                entry_key(element)?,
                canonicalize_entry(element, value_kind.as_ref())?,
            ));
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        Some(
            entries
                .into_iter()
                .map(|(_, text)| text)
                .collect::<Vec<_>>()
                .join(","),
        )
    } else if let Kind::Message(child) = field.kind() {
        // A repeated message field: recurse each `{…}` element, order preserved.
        let mut out = Vec::with_capacity(elements.len());
        for element in elements {
            let body = strip(element, '{', '}')?;
            out.push(format!("{{{}}}", canonicalize_body(body, &child)?));
        }
        Some(out.join(","))
    } else {
        Some(inner.to_owned()) // a repeated scalar list: verbatim
    }
}

/// Re-emit a map entry `{key:K,value:V}` (or `{key:K,value{…}}`), recursing a message value's maps.
fn canonicalize_entry(element: &str, value_kind: Option<&MessageDescriptor>) -> Option<String> {
    let body = strip(element, '{', '}')?;
    let parts = split_top(body, ',');
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        match (part.strip_prefix("value{"), value_kind) {
            (Some(after), Some(child)) => {
                let inner = after.strip_suffix('}')?;
                out.push(format!("value{{{}}}", canonicalize_body(inner, child)?));
            }
            _ => out.push(part.to_owned()),
        }
    }
    Some(format!("{{{}}}", out.join(",")))
}

/// The sort key of a map entry — its `key:K` value decoded: a number to its integer value (so
/// numeric keys order numerically, not by digit text), anything else (a quoted string, `true`/
/// `false`) by its token (quotes and `false`/`true` already order correctly).
fn entry_key(element: &str) -> Option<SortKey> {
    let body = strip(element, '{', '}')?;
    let key = split_top(body, ',')
        .into_iter()
        .find_map(|part| part.strip_prefix("key:"))
        .unwrap_or(""); // a default key (proto3 may omit it) sorts as the empty token
    Some(match key.parse::<i128>() {
        Ok(number) => SortKey::Number(number),
        Err(_) => SortKey::Token(key.to_owned()),
    })
}

/// A map entry's ordering key — numbers before tokens, each ordered within its kind.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Number(i128),
    Token(String),
}

/// The message descriptor of a field's kind, or `None` if it is not a message field.
fn message_kind(field: Option<prost_reflect::FieldDescriptor>) -> Option<MessageDescriptor> {
    match field?.kind() {
        Kind::Message(message) => Some(message),
        _ => None,
    }
}

/// The message descriptor of a map field's *value*, when that value is a message.
fn map_value_kind(field: &prost_reflect::FieldDescriptor) -> Option<MessageDescriptor> {
    let Kind::Message(entry) = field.kind() else {
        return None;
    };
    match entry.get_field(2)?.kind() {
        Kind::Message(message) => Some(message),
        _ => None,
    }
}

/// Split `name` off the front of a field: the name, the separator (`:` or `{`) that ends it, and the
/// remainder (from the separator on). `None` if neither separator appears at the top level.
fn split_name(field: &str) -> Option<(&str, char, &str)> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in field.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == ':' || ch == '{' {
            return Some((&field[..index], ch, &field[index..]));
        }
    }
    None
}

/// If `s` is `open … close` (a balanced, string-aware pair), the content between them.
fn strip(s: &str, open: char, close: char) -> Option<&str> {
    let s = s.strip_prefix(open)?.strip_suffix(close)?;
    Some(s)
}

/// Split `s` on `sep` at the top level — depth 0 over `{}`/`[]`, outside `"…"` strings (respecting
/// `\` escapes). Empty input yields no parts.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0;
    for (index, ch) in s.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else {
            match ch {
                '"' => in_string = true,
                '{' | '[' => depth += 1,
                '}' | ']' => depth = depth.saturating_sub(1),
                c if c == sep && depth == 0 => {
                    parts.push(&s[start..index]);
                    start = index + c.len_utf8();
                }
                _ => {}
            }
        }
    }
    parts.push(&s[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use prost::Message as _;
    use prost_reflect::{DynamicMessage, MapKey, MessageDescriptor, Value};
    use prost_types::{
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
        MessageOptions,
    };

    use super::canonicalize_text;
    use crate::descriptor;

    /// A `MessageDescriptor` for `M { map<int32, int32> m = 1; string s = 2; }`.
    fn descriptor_of() -> MessageDescriptor {
        let field = |name: &str, number: i32, r#type: i32, label: i32, type_name: Option<&str>| {
            FieldDescriptorProto {
                name: Some(name.to_owned()),
                number: Some(number),
                label: Some(label),
                r#type: Some(r#type),
                type_name: type_name.map(str::to_owned),
                ..Default::default()
            }
        };
        let entry = DescriptorProto {
            name: Some("MEntry".to_owned()),
            field: vec![field("key", 1, 5, 1, None), field("value", 2, 5, 1, None)],
            options: Some(MessageOptions {
                map_entry: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let set = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("t.proto".to_owned()),
                package: Some("t".to_owned()),
                syntax: Some("proto3".to_owned()),
                message_type: vec![DescriptorProto {
                    name: Some("M".to_owned()),
                    field: vec![
                        field("m", 1, 11, 3, Some(".t.M.MEntry")),
                        field("s", 2, 9, 1, None),
                    ],
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

    /// The compact text form of `M` with `m = {entries}` and `s = "z"`, in the given entry order.
    fn text_of(descriptor: &MessageDescriptor, entries: &[(i32, i32)]) -> String {
        let mut message = DynamicMessage::new(descriptor.clone());
        message
            .try_set_field_by_number(
                1,
                Value::Map(
                    entries
                        .iter()
                        .map(|&(k, v)| (MapKey::I32(k), Value::I32(v)))
                        .collect(),
                ),
            )
            .expect("the map fits");
        message
            .try_set_field_by_number(2, Value::String("z".to_owned()))
            .expect("the string fits");
        message.to_text_format()
    }

    #[test]
    fn split_top_respects_braces_and_strings() {
        assert_eq!(super::split_top("a,b,c", ','), vec!["a", "b", "c"]);
        assert_eq!(super::split_top("a{b,c},d", ','), vec!["a{b,c}", "d"]);
        assert_eq!(
            super::split_top(r#"a:"x,y",b:1"#, ','),
            vec![r#"a:"x,y""#, "b:1"]
        );
    }

    #[test]
    fn map_entries_are_ordered_by_key_deterministically() {
        // The same map, two `HashMap` orders, canonicalise to identical text with keys ascending.
        let descriptor = descriptor_of();
        let one = canonicalize_text(
            &text_of(&descriptor, &[(3, 30), (1, 10), (2, 20)]),
            &descriptor,
        );
        let other = canonicalize_text(
            &text_of(&descriptor, &[(2, 20), (3, 30), (1, 10)]),
            &descriptor,
        );
        assert_eq!(one, other, "any input order canonicalises identically");
        // Keys ascend: `1` before `2` before `3` in the text.
        let at = |k: &str| one.find(&format!("key:{k}")).expect("the key is present");
        assert!(
            at("1") < at("2") && at("2") < at("3"),
            "entries ascend by key: {one}"
        );
    }

    #[test]
    fn negative_keys_order_by_value_not_by_token() {
        // A negative key's token (`-1`) sorts after `5` as text but before it as a number.
        let descriptor = descriptor_of();
        let text = canonicalize_text(
            &text_of(&descriptor, &[(5, 1), (-1, 2), (0, 3)]),
            &descriptor,
        );
        let at = |k: &str| text.find(&format!("key:{k}")).expect("present");
        assert!(
            at("-1") < at("0") && at("0") < at("5"),
            "ascend by value: {text}"
        );
    }

    #[test]
    fn text_without_a_map_field_is_unchanged() {
        let descriptor = descriptor_of();
        assert_eq!(canonicalize_text("s:\"hello\"", &descriptor), "s:\"hello\"");
        // A quoted comma is not a field separator.
        assert_eq!(canonicalize_text("s:\"a,b\"", &descriptor), "s:\"a,b\"");
    }
}
