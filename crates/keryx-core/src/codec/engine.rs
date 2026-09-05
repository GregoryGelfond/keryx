//! The decode engine's adapter (architecture §5, inbound) — the one place in `codec` that names
//! prost-reflect, and `serde_json`, the deserializer the JSON form drives the engine with. A
//! payload — in the binary wire format, the text format, or the JSON mapping — is decoded against
//! its root message's descriptor under foreign-fault containment, and the decoded tree is
//! presented in keryx's own **borrowing** value vocabulary:
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
use std::panic::resume_unwind;
use std::thread;

use prost_reflect::{
    DynamicMessage, FieldDescriptor, Kind, MapKey, MessageDescriptor, ReflectMessage as _, Value,
};

use super::guard;
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::fault::{Dependency, contain};

/// Decode a binary (`.binpb`) payload as an instance of `desc` — the payload door's engine call,
/// the one crossing into prost-reflect on the payload path — and return the tree, owned. The
/// engine bounds a payload's message nesting at its decode recursion limit: each nested message
/// spends one level of `descriptor::RECURSION_LIMIT`, so a payload nesting message-typed fields
/// exactly `RECURSION_LIMIT` levels below the root still decodes and one level deeper is a decode
/// error — the deepest tree this door delivers nests `RECURSION_LIMIT` levels, one past the walk's
/// uniform ceiling, which refuses that level itself (`super::walk::NESTING_CEILING`).
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
pub(crate) fn decode_binary(
    desc: &MessageDescriptor,
    bytes: &[u8],
) -> Result<Decoded, Diagnostics> {
    // The closure borrows only `desc` (a handle over the pool's shared state, cloned in) and
    // `bytes`; a fault drops the half-built message with the unwind, so nothing keryx observes
    // survives it. A binary decode reads the pool only through that handle — the engine consults
    // its global well-known-type pool on a descriptor's option decode and in its reflection of
    // prost-types' own well-known types (prost-reflect 0.16.5 `src/descriptor/api.rs:1959`,
    // `src/descriptor/build/options.rs:310`, `src/reflect/wkt.rs:5485`, its every `global()` site),
    // never on a payload decode in any format — so no process-global state can be left
    // inconsistent, and keryx's own logic inside the frame is one infallible clone; the
    // `AssertUnwindSafe` is sound.
    let decoded = contain(Dependency::ProstReflect, "decoding a payload", || {
        DynamicMessage::decode(desc.clone(), bytes)
    })?;
    decoded
        .map(|root| Decoded { root })
        .map_err(|error| undecodable(desc, &error.to_string()))
}

/// The stack the textproto parse runs on: 8 MiB, on a thread keryx sizes itself. The engine's
/// text-format parser recurses natively on every nested message value and bounds nothing
/// (`super::guard`, which derives the parser's frames per level and so the bounded need), so the
/// deepest payload the guard admits — `NESTING_CEILING` levels — must fit whatever stack carries
/// the parse. **The measure of record**, which the guard's doc and the door's instruments cite
/// rather than restate — against the pinned engine, prost-reflect 0.16.5, in debug and release
/// builds: 99 levels need some 2.5 MB in a debug build (2 MB overflows — some 25 KB a level; an
/// 8 MB stack overflows between 300 and 400 levels) and 256 KB in release (192 KB overflows —
/// some 2.5 KB a level; 8 MB overflows between 3,000 and 4,000 levels). This size carries the
/// ceiling's need some three times over in debug and thirty in release — where a spawned
/// thread's 2 MB default, the test harness's threads among them, would not carry it in debug at
/// all — and it holds whatever thread the caller decodes on, so the margin is keryx's by
/// construction, not the host's. The stack is reserved address space, committed a page at a time
/// as the parse descends, so a shallow payload pays a thread spawn and little else.
const TEXTPROTO_PARSE_STACK: usize = 8 << 20;

/// Decode a textproto (`.txtpb`) payload as an instance of `desc` — the payload door's second
/// engine crossing, the text format's — and return the tree, owned: the same [`Decoded`] view the
/// binary decode yields, so the walk and the §6 policy read a text payload exactly as they read
/// its binary form (spec §26 parity). The door is three steps, each total. The bytes must be
/// UTF-8 — the text format is text — or the payload is `UndecodablePayload`. The text's message
/// nesting is then measured and bounded by the pre-parse guard (`super::guard`), so the engine's
/// parser, which recurses natively and bounds nothing, sees no payload nesting past the uniform
/// ceiling (`PayloadTooDeep` past `super::walk::NESTING_CEILING`). And the parse itself runs on a
/// thread keryx sizes for the deepest admitted payload ([`TEXTPROTO_PARSE_STACK`]), with the
/// dependency boundary's containment frame *inside* it. That order is load-bearing: a stack
/// overflow aborts the process rather than unwinding, so no frame can catch one — the guard and
/// the sized thread *prevent* it, for every payload this door admits — and the frame catches what
/// does unwind, an unforeseen engine panic, as a `DependencyFault`. The engine's own parse
/// failure — a field the type does not declare, a value left open, a literal outside its kind's
/// range — is `UndecodablePayload`, its message composed into the detail (§6), never its type
/// exposed.
///
/// The parse is single-threaded for all that it runs on its own thread: the thread exists for its
/// stack alone, the caller waits on it, and the tree comes back owned — so a text payload's facts
/// are the same function of the payload a binary one's are (the threat model's determinism).
///
/// # Errors
///
/// `UndecodablePayload` when the bytes are not UTF-8 or do not parse as `desc`; `PayloadTooDeep`
/// when the text nests message values past the uniform ceiling; `DependencyFault` for a contained
/// engine panic.
pub(crate) fn decode_textproto(
    desc: &MessageDescriptor,
    bytes: &[u8],
) -> Result<Decoded, Diagnostics> {
    // The text format is UTF-8 text: a payload that is not is refused before the engine sees it,
    // with the failure's position in the detail — the error names an index and a length — and
    // never its bytes.
    let text = std::str::from_utf8(bytes).map_err(|error| {
        undecodable(
            desc,
            &format!("the text format is UTF-8 text, and the payload is not ({error})"),
        )
    })?;
    // Measured and bounded on the caller's thread — one pass over the bytes, no recursion — so the
    // parse below sees only a text nesting at most `NESTING_CEILING` levels, the depth its stack
    // is sized for.
    guard::depth(text)?;
    let operation = "parsing a textproto payload";
    let parsed = thread::scope(|scope| {
        // The closure borrows only `desc` (a handle over the pool's shared state, cloned in) and
        // `text`; a fault drops the half-built message with the unwind, so nothing keryx observes
        // survives it. The parser reads the pool only through that handle — an `Any` value's type
        // resolves against the root descriptor's own pool (prost-reflect 0.16.5
        // `src/dynamic/text_format/parse/mod.rs:104-108`), never the engine's global one — so no
        // process-global state can be left inconsistent, and keryx's own logic inside the frame
        // is one infallible clone; the `AssertUnwindSafe` is sound. The frame's thread-local flag
        // is set on the thread the parse runs on — the thread a panic hook consults it from.
        let handle = thread::Builder::new()
            .name("keryx-textproto".to_owned())
            .stack_size(TEXTPROTO_PARSE_STACK)
            .spawn_scoped(scope, || {
                contain(Dependency::ProstReflect, operation, || {
                    DynamicMessage::parse_text_format(desc.clone(), text)
                })
            })
            // A thread the host cannot spawn is the host out of a resource — threads, or the
            // address space to reserve the stack in — and nothing the payload's content brings
            // about: by here the payload is validated, measured, and bounded, and the spawn asks
            // the same of the host for every payload alike. Repetition can bring it about — the
            // threat model's adversary may repeat the call, and calls admitted faster than their
            // parse threads retire could run the host out of either — and the model assigns that
            // to the consuming service: its side of the division of labor is isolation of the
            // translation under resource limits, so a host exhausted by load is the operating
            // system's to contain, as an abort or a hang is, where keryx's guarantees hold per
            // call. Discharged as a host invariant, then, against the adversary the model names
            // and not on payload-independence alone: a foreign-input path is one a payload's
            // content reaches (§6), and an exhausted host is the service's to bound, not this
            // door's to diagnose.
            .expect("the host can spawn the textproto parse thread");
        handle.join().unwrap_or_else(|unwind| {
            // A panic that escaped the frame — none can, the frame catching every unwind inside it
            // — is re-raised inside a frame here, on the caller's thread, so it is contained as it
            // would have been: the same fault, from the one seam. `resume_unwind` runs no panic
            // hook, so the fault is reported once.
            contain(Dependency::ProstReflect, operation, || {
                resume_unwind(unwind)
            })
        })
    })?;
    parsed
        .map(|root| Decoded { root })
        .map_err(|error| undecodable(desc, &error.to_string()))
}

/// The stack the JSON decode runs on: 8 MiB, on a thread keryx sizes itself. No guard precedes
/// this decode — the deserializer bounds its own nesting, refusing the 128th nested array or
/// object (`serde_json` 1.0.151 `src/de.rs:63`, `:1372-1384`), a count the engine's `serde`
/// mapping recurses natively beneath with no counter of its own — so the deepest payload the
/// decode admits nests 127 containers, and it is deserialized *whole* on this thread before the
/// walk, on the caller's thread, applies the uniform ceiling: the thread carries the
/// deserializer's full admit, not the ceiling's 99 levels. **The measure of record**, which the
/// door's doc and its instruments cite rather than restate — against the pinned engine,
/// prost-reflect 0.16.5 over `serde_json` 1.0.151, in debug and release builds, for the deepest
/// payload of each form the deserializer admits: a chain of 126 singular message fields (127
/// objects) needs some 896 KB in a debug build (768 KB overflows — some 7 KB a level) and 160 KB
/// in release (128 KB overflows); a chain of 63 repeated or 63 map-of-message fields (127
/// containers again) less — 512 KB and 640 KB in debug, 80 KB and 128 KB in release. The
/// costliest is an `Any`: one whose `@type` follows the value it holds is buffered by the engine
/// as it reads and then deserialized again from the buffer, natively, at the `Any`'s depth, at
/// some 14 KB a level in debug — twice a plain level — so an `Any` whose `@type` follows a
/// 125-level chain needs 1.75 MB in debug (1.5 MB overflows) and 224 KB in release (192 KB
/// overflows), and a nest of 126 such `Any` values, each buffered by the one above, 2 MB in debug
/// (1.75 MB overflows) and 320 KB in release (256 KB overflows). A `google.protobuf.Value` chain
/// binds earlier, at the engine's own message-decode limit as it materialises the value (50
/// levels admitted, 51 a decode error), and needs 448 KB in debug, 96 KB in release. This size
/// carries the costliest need four times over in debug and twenty-five in release — where a
/// spawned thread's 2 MB default, the test harness's threads among them, would carry it in debug
/// with nothing to spare, and a 1 MB main thread would not — and it holds whatever thread the
/// caller decodes on, so the margin is keryx's by construction, not the host's. The stack is
/// reserved address space, committed a page at a time as the deserialization descends, so a
/// shallow payload pays a thread spawn and little else.
const JSON_DECODE_STACK: usize = 8 << 20;

/// Decode a canonical JSON (`.json`) payload as an instance of `desc` — the payload door's third
/// engine crossing, the JSON mapping's — and return the tree, owned: the same [`Decoded`] view
/// the binary and text decodes yield, so the walk and the §6 policy read a JSON payload exactly as
/// they read its other forms (spec §26 parity). The door is one step, total, with no guard before
/// it: the deserializer bounds its own nesting — `serde_json`'s recursion limit, on by default
/// and never lifted, counts down from 128 and refuses the 128th nested array or object (1.0.151
/// `src/de.rs:63`, `:1372-1384`) — so the engine's `serde` mapping, which recurses natively
/// beneath that count with no counter of its own (prost-reflect 0.16.5 `src/dynamic/serde/de/`:
/// `KindSeed::deserialize` `kind.rs:20` → `deserialize_message` `mod.rs:14` →
/// `MessageVisitor::visit_map` `kind.rs:544` → `MessageVisitorInner::visit_map` `kind.rs:563` →
/// the next field's seed), sees no payload nesting past 127 containers. The whole admitted payload
/// is deserialized here, on a thread keryx sizes for that admit ([`JSON_DECODE_STACK`]), *before*
/// the walk applies the uniform ceiling on the caller's thread — so a chain of singular message
/// fields 100 to 126 levels deep, which the deserializer admits, is the walk's `PayloadTooDeep`,
/// and one 127 deep, or a repeated or map chain 64 deep (two containers a level), is the
/// deserializer's own refusal, `UndecodablePayload` — a shallower message depth than the ceiling,
/// never a deeper one. The containment frame sits *inside* the sized thread, the order the text
/// decode keeps and for the same reason: a stack overflow aborts rather than unwinds, so no frame
/// can catch one — the sized thread *prevents* it, for every payload the deserializer admits —
/// and the frame catches what does unwind, an unforeseen fault in the deserializer or in the
/// engine's visitors beneath it, as a `DependencyFault` naming the code keryx drives,
/// `serde_json`.
///
/// Canonical (spec §26), by the deserializer's defaults: a field the type does not declare — by
/// its JSON name or its proto name, both of which the mapping admits — is refused, as the
/// mapping asks of a conforming parser by default and as the text format's parser refuses one,
/// where the binary wire keeps an unknown field it cannot name; a payload is one JSON value, so
/// text after the value is refused too; and the empty message is `{}` — an empty text is no
/// value. Every such failure — an undeclared field, a value outside its kind, malformed or
/// non-UTF-8 text, the deserializer's count exceeded, text after the value — is
/// `UndecodablePayload`, its message composed into the detail (§6), never its type exposed.
///
/// Single-threaded for all that it runs on its own thread, as the text decode is: the thread
/// exists for its stack alone, the caller waits on it, and the tree comes back owned — so a JSON
/// payload's facts are the same function of the payload a binary one's are (the threat model's
/// determinism).
///
/// # Errors
///
/// `UndecodablePayload` when the bytes are not one canonical JSON value of `desc`;
/// `DependencyFault` for a contained fault.
pub(crate) fn decode_json(desc: &MessageDescriptor, bytes: &[u8]) -> Result<Decoded, Diagnostics> {
    let operation = "decoding a JSON payload";
    let deserialized = thread::scope(|scope| {
        // The closure borrows only `desc` (a handle over the pool's shared state, cloned in) and
        // `bytes`; a fault drops the half-built message with the unwind, so nothing keryx observes
        // survives it. The deserialization reads the pool only through that handle — an `Any`
        // value's type resolves against the root descriptor's own pool (prost-reflect 0.16.5
        // `src/dynamic/serde/de/mod.rs:24`, `desc.parent_pool()`), never the engine's global one
        // — so no process-global state can be left inconsistent, and keryx's own logic inside the
        // frame is one infallible clone and two `?`s; the `AssertUnwindSafe` is sound. The frame's
        // thread-local flag is set on the thread the deserialization runs on — the thread a panic
        // hook consults it from.
        let handle = thread::Builder::new()
            .name("keryx-json".to_owned())
            .stack_size(JSON_DECODE_STACK)
            .spawn_scoped(scope, || {
                contain(
                    Dependency::SerdeJson,
                    operation,
                    || -> Result<DynamicMessage, serde_json::Error> {
                        // The deserializer as built: its recursion limit on, the engine's
                        // `deny_unknown_fields` on. A payload is one value, so `end` refuses text
                        // after it.
                        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
                        let root = DynamicMessage::deserialize(desc.clone(), &mut deserializer)?;
                        deserializer.end()?;
                        Ok(root)
                    },
                )
            })
            // A thread the host cannot spawn is the host out of a resource — threads, or the
            // address space to reserve the stack in — and nothing the payload's content brings
            // about: the spawn reads none of it and asks the same of the host for every payload
            // alike. Discharged as `decode_textproto` discharges the same spawn — a host invariant
            // against the adversary the threat model names, whose repetition the consuming
            // service's resource limits bound, not this door's diagnosis.
            .expect("the host can spawn the JSON decode thread");
        handle.join().unwrap_or_else(|unwind| {
            // A panic that escaped the frame — none can, the frame catching every unwind inside it
            // — is re-raised inside a frame here, on the caller's thread, so it is contained as it
            // would have been: the same fault, from the one seam. `resume_unwind` runs no panic
            // hook, so the fault is reported once.
            contain(Dependency::SerdeJson, operation, || resume_unwind(unwind))
        })
    })?;
    deserialized
        .map(|root| Decoded { root })
        .map_err(|error| undecodable(desc, &error.to_string()))
}

/// Compose the `UndecodablePayload` for bytes that did not decode as `desc`: the whole-payload
/// locus (the wire itself is unreadable, so no field path is finer), naming the root type, with
/// the failure's own message — the engine's, or the UTF-8 check's — composed into the detail (§6),
/// never exposed as its type.
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
pub(crate) struct Decoded {
    root: DynamicMessage,
}

impl Decoded {
    /// The root message as a borrowing handle — the walk's first work item, from which every
    /// sub-message it reaches is a handle over this same tree, and through which the root's own
    /// fields are read like any other message's.
    pub(crate) fn root(&self) -> SubMessage<'_> {
        SubMessage(&self.root)
    }
}

/// A message within the decoded tree — the root, or a sub-message reached from it — as a
/// copyable **borrowing** handle. `'a` is the tree's lifetime, not the handle's: a value read
/// through a handle borrows the tree, so a walk can hold the child it read beside the parent it
/// read it from, and let go of either first.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubMessage<'a>(&'a DynamicMessage);

impl<'a> SubMessage<'a> {
    /// The engine's presence for the field numbered `number`: for a field with explicit presence
    /// (a message-typed field, a `oneof` arm, a proto3 `optional`, every proto2 singular field),
    /// whether the wire carried it; for a field without (an IMPLICIT scalar, a list, a map),
    /// whether its value is non-default — the engine's notion, which the walk asks only of a
    /// partial field (spec §5: presence is decided from the mapping's totality, never here).
    /// `false` for a number the message does not declare, and for a negative one.
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

/// A map key is a scalar in key position (spec §7.2: keys map per §6), so it lowers as the datum
/// of its kind — the §6 policy's input, read from the same key the entries were ordered by.
impl<'a> From<Key<'a>> for Datum<'a> {
    fn from(key: Key<'a>) -> Datum<'a> {
        match key {
            Key::Bool(value) => Datum::Bool(value),
            Key::I32(value) => Datum::I32(value),
            Key::I64(value) => Datum::I64(value),
            Key::U32(value) => Datum::U32(value),
            Key::U64(value) => Datum::U64(value),
            Key::Str(value) => Datum::Str(value),
        }
    }
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

    use keryx_test_support::wire::delimited;
    use prost::Message as _;
    use prost::encoding;
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

    // The payloads are written as bytes on the wire — prost's encoding primitives and the shared
    // `delimited` builder — not through the engine's own encoder, so the door is seen to read
    // the wire.

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
        let detail = message_of(decoded.root().value(5));
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
            decoded.root().is_present(1),
            "a set repeated field is present to the engine"
        );
        let readings = elements(decoded.root().value(1));
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
        assert_eq!(scalar(decoded.root().value(2)), Datum::I32(0));
        assert!(!decoded.root().is_present(2));
        // An empty payload: every scalar reads as its zero, a sequence as empty.
        let empty = decode_binary(&reading_desc, &[]).expect("an empty payload decodes");
        assert_eq!(scalar(empty.root().value(1)), Datum::Str(""));
        assert_eq!(scalar(empty.root().value(2)), Datum::I32(0));
        let batch = decode_binary(&descriptor_of(&pool, "thermal.v1.ReadingBatch"), &[])
            .expect("an empty batch decodes");
        assert!(elements(batch.root().value(1)).is_empty());
        assert!(!batch.root().is_present(1));
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
        assert!(
            decoded.root().is_present(5),
            "a set message field is present"
        );
        let sub = message_of(decoded.root().value(5));
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
        assert_eq!(scalar(empty.root().value(3)), Datum::I32(0));
        assert!(!empty.root().is_present(3));
        assert_eq!(scalar(empty.root().value(4)), Datum::Enum(0));
        assert!(empty.root().value(5).is_none());
        assert!(!empty.root().is_present(5));
        // The optional scalar carried as an explicit zero, and one `oneof` arm carried: the same
        // zero, now present — the value view decides nothing about presence; the other arm reads
        // as its zero and is not present.
        let mut bytes = Vec::new();
        encoding::int32::encode(3, &0, &mut bytes);
        delimited(6, b"dev", &mut bytes);
        let carried = decode_binary(&reading, &bytes).expect("decodes");
        assert_eq!(scalar(carried.root().value(3)), Datum::I32(0));
        assert!(carried.root().is_present(3));
        assert_eq!(scalar(carried.root().value(6)), Datum::Str("dev"));
        assert!(carried.root().is_present(6));
        assert_eq!(scalar(carried.root().value(7)), Datum::Str(""));
        assert!(!carried.root().is_present(7));
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
        assert_eq!(scalar(decoded.root().value(1)), Datum::I32(7));
        assert_eq!(scalar(decoded.root().value(2)), Datum::I64(-1));
        assert_eq!(scalar(decoded.root().value(3)), Datum::U64(u64::MAX));
        assert_eq!(scalar(decoded.root().value(4)), Datum::F64(1.5));
        assert_eq!(scalar(decoded.root().value(5)), Datum::Bool(true));
        assert_eq!(scalar(decoded.root().value(6)), Datum::Bytes(&[0xde, 0xad]));
        assert_eq!(scalar(decoded.root().value(7)), Datum::Str("lbl"));
        assert_eq!(scalar(decoded.root().value(8)), Datum::Enum(1));
        let notes: Vec<Datum<'_>> = elements(decoded.root().value(9))
            .into_iter()
            .map(|element| scalar(element_message(element).value(1)))
            .collect();
        assert_eq!(notes, [Datum::Str("A"), Datum::Str("B")]);
        let kinds: Vec<Datum<'_>> = elements(decoded.root().value(10))
            .into_iter()
            .map(element_scalar)
            .collect();
        assert_eq!(kinds, [Datum::Enum(1), Datum::Enum(0)]);
        let tags: Vec<(Key<'_>, Datum<'_>)> = entries(decoded.root().value(11))
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
        assert_eq!(scalar(decoded.root().value(1)), Datum::U32(u32::MAX));
        assert_eq!(scalar(decoded.root().value(2)), Datum::F64(2.5));
    }

    #[test]
    fn an_empty_payload_reads_every_kind_as_its_zero() {
        let pool = fixture_pool("scalar_treatment.proto");
        let sample = descriptor_of(&pool, "keryx.scalars.Sample");
        let decoded = decode_binary(&sample, &[]).expect("an empty payload decodes");
        assert_eq!(scalar(decoded.root().value(1)), Datum::I32(0));
        assert_eq!(scalar(decoded.root().value(2)), Datum::I64(0));
        assert_eq!(scalar(decoded.root().value(3)), Datum::U64(0));
        assert_eq!(scalar(decoded.root().value(4)), Datum::F64(0.0));
        assert_eq!(scalar(decoded.root().value(5)), Datum::Bool(false));
        assert_eq!(scalar(decoded.root().value(6)), Datum::Bytes(&[]));
        assert_eq!(scalar(decoded.root().value(7)), Datum::Str(""));
        assert_eq!(scalar(decoded.root().value(8)), Datum::Enum(0));
        assert!(elements(decoded.root().value(9)).is_empty());
        assert!(elements(decoded.root().value(10)).is_empty());
        assert!(entries(decoded.root().value(11)).is_empty());
        let pool = kinds_pool();
        let decoded =
            decode_binary(&descriptor_of(&pool, "k.Kinds"), &[]).expect("an empty payload decodes");
        assert_eq!(scalar(decoded.root().value(1)), Datum::U32(0));
        assert_eq!(scalar(decoded.root().value(2)), Datum::F64(0.0));
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
        let counts: Vec<(Key<'_>, Datum<'_>)> = entries(decoded.root().value(1))
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
        let items: Vec<(Key<'_>, Datum<'_>)> = entries(decoded.root().value(2))
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
            entries(decoded.root().value(number))
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
        assert!(decoded.root().value(99).is_none());
        assert!(decoded.root().value(-1).is_none());
        assert!(!decoded.root().is_present(99));
        assert!(!decoded.root().is_present(-1));
    }
}
