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

use prost::Message as _;
use prost_reflect::text_format::FormatOptions;
use prost_reflect::{
    DynamicMessage, FieldDescriptor, Kind, MapKey, MessageDescriptor, ReflectMessage as _, Value,
};

use super::{PayloadFormat, canonical, canonical_text, guard};
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

/// Run `work` — keryx's own step over a contained foreign call — on a thread keryx sizes (`stack`),
/// returning its result or a contained `DependencyFault`. The one sized-thread-with-containment
/// skeleton the payload door's two decodes and the outbound encode share ([`decode_textproto`],
/// [`decode_json`], [`encode_binary`]): each supplies what runs on the thread, this supplies the
/// thread and the fault seam around it.
///
/// **The thread, sized above the deepest admitted input.** The foreign engine call recurses natively
/// per nested message and bounds nothing, and a stack overflow *aborts* the process rather than
/// unwinding — no frame can catch one. So the door that admits an input at the reconstruction ceiling
/// runs the engine on a thread keryx sizes to hold it, and the size is what *prevents* the overflow;
/// the containment frame `work` places *inside* the thread catches what does unwind (an unforeseen
/// engine panic), as the one `DependencyFault` seam. A panic that escaped that inner frame — none can
/// — is re-raised here inside a fresh frame on the caller's thread, so it is contained identically,
/// from the same seam; `resume_unwind` runs no panic hook, so the fault is reported once. The call is
/// single-threaded for all that it runs on its own thread: the thread exists for its stack alone, the
/// caller waits on it, and the result comes back owned — so an input's translation is the same
/// function of the input however the door runs it (the threat model's determinism).
///
/// **A thread the host cannot spawn is the host's, not the input's.** By the time a door spawns here
/// its input is validated and bounded, and the spawn asks the same of the host for every input alike
/// — so a spawn failure is the host out of a resource (threads, or the address space to reserve the
/// stack), never anything the input's content brings about. Repetition can exhaust the host — the
/// threat model's adversary may repeat the call — and the model assigns that to the consuming
/// service's resource limits, its side of the division of labor, as it assigns an abort or a hang;
/// keryx's guarantees hold per call, so the failed spawn is a discharged host invariant.
fn on_sized_thread<R: Send>(
    name: &str,
    stack: usize,
    dependency: Dependency,
    operation: &str,
    work: impl FnOnce() -> Result<R, Diagnostics> + Send,
) -> Result<R, Diagnostics> {
    thread::scope(|scope| {
        let handle = thread::Builder::new()
            .name(name.to_owned())
            .stack_size(stack)
            .spawn_scoped(scope, work)
            .expect("the host can spawn a keryx-sized thread");
        handle
            .join()
            .unwrap_or_else(|unwind| contain(dependency, operation, || resume_unwind(unwind)))
    })
}

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
/// **The one sized-thread skeleton, lifted.** The thread keryx sizes, its inside containment, the
/// spawn-as-host-invariant, and the re-containment of an unwind that escaped the inner frame are
/// [`on_sized_thread`], shared with the JSON decode ([`decode_json`]) and the outbound encode
/// ([`encode_binary`]) — the third sized-thread door that earned the lift. What stays at each door is
/// what the lift cannot carry without splitting a reading in two: for this door, the UTF-8 check and
/// the pre-parse guard that precede the thread on the caller's thread (the JSON decode has neither,
/// its deserializer bounding itself); the stack it is sized by ([`TEXTPROTO_PARSE_STACK`], for the
/// guard's ceiling — [`JSON_DECODE_STACK`] and [`ENCODE_STACK`] size the others, each measured and
/// owed separately); the dependency the frame names; the engine call; and the argument that
/// containing *this* call is sound (the pool-handle reasoning at the closure below).
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
    let parsed = on_sized_thread(
        "keryx-textproto",
        TEXTPROTO_PARSE_STACK,
        Dependency::ProstReflect,
        operation,
        || {
            // The closure borrows only `desc` (a handle over the pool's shared state, cloned in) and
            // `text`; a fault drops the half-built message with the unwind, so nothing keryx observes
            // survives it. The parser reads the pool only through that handle — an `Any` value's type
            // resolves against the root descriptor's own pool (prost-reflect 0.16.5
            // `src/dynamic/text_format/parse/mod.rs:104-108`), never the engine's global one — so no
            // process-global state can be left inconsistent, and keryx's own logic inside the frame
            // is one infallible clone; the `AssertUnwindSafe` inside `contain` is sound.
            contain(Dependency::ProstReflect, operation, || {
                DynamicMessage::parse_text_format(desc.clone(), text)
            })
        },
    )?;
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
/// Runs on the shared sized-thread skeleton ([`on_sized_thread`], argued at [`decode_textproto`]):
/// this door's parts that stay off it are that no guard precedes the thread — the deserializer bounds
/// its own nesting — the stack it is sized by ([`JSON_DECODE_STACK`], the deserializer's deeper
/// admit), the dependency the frame names (`SerdeJson`), and the argument that containing it is sound
/// (the pool-handle reasoning at the closure below).
///
/// # Errors
///
/// `UndecodablePayload` when the bytes are not one canonical JSON value of `desc`;
/// `DependencyFault` for a contained fault.
pub(crate) fn decode_json(desc: &MessageDescriptor, bytes: &[u8]) -> Result<Decoded, Diagnostics> {
    let operation = "decoding a JSON payload";
    let deserialized = on_sized_thread(
        "keryx-json",
        JSON_DECODE_STACK,
        Dependency::SerdeJson,
        operation,
        || {
            // The closure borrows only `desc` (a handle over the pool's shared state, cloned in) and
            // `bytes`; a fault drops the half-built message with the unwind, so nothing keryx observes
            // survives it. The deserialization reads the pool only through that handle — an `Any`
            // value's type resolves against the root descriptor's own pool (prost-reflect 0.16.5
            // `src/dynamic/serde/de/mod.rs:24`, `desc.parent_pool()`), never the engine's global one
            // — so no process-global state can be left inconsistent, and keryx's own logic inside the
            // frame is one infallible clone and two `?`s; the `AssertUnwindSafe` inside `contain` is
            // sound.
            contain(
                Dependency::SerdeJson,
                operation,
                || -> Result<DynamicMessage, serde_json::Error> {
                    // The deserializer as built: its recursion limit on, the engine's
                    // `deny_unknown_fields` on. A payload is one value, so `end` refuses text after it.
                    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
                    let root = DynamicMessage::deserialize(desc.clone(), &mut deserializer)?;
                    deserializer.end()?;
                    Ok(root)
                },
            )
        },
    )?;
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

/// The stack the binary encode runs on: 8 MiB, on a thread keryx sizes itself — the outbound mirror
/// of [`TEXTPROTO_PARSE_STACK`]/[`JSON_DECODE_STACK`]. The engine's binary serializer recurses
/// natively on every nested message and bounds nothing (`encode_raw` → `encode_field` → the nested
/// `encode_raw`, prost-reflect 0.16.5 `src/dynamic/message.rs:134`), so a message admitted at the
/// reconstruction ceiling — `super::walk::NESTING_CEILING` levels, the depth the reassembly walk
/// admits before it builds anything — must fit whatever stack carries the encode, and a
/// sub-standard caller thread must not overflow. Depth-bounding alone does not close the abort axis
/// (a message *at* the ceiling still recurses that deep natively at encode), so the encode runs
/// here with the dependency boundary's containment frame *inside* the thread, exactly as the
/// inbound parse and decode do — the margin keryx's by construction, not the host's. Sized by the
/// inbound threads' precedent, and now **measured** against the pinned engine (prost-reflect 0.16.5)
/// in debug and release, for a message at the reconstruction ceiling — 99 message-typed levels, the
/// deepest the reassembly walk admits: the costliest of the three encoders needs some 640 KiB in a
/// debug build (576 KiB overflows — some 6.5 KB a level; the binary form is cheaper, overflowing
/// below 512 KiB) and 192 KiB in release (128 KiB overflows — some 2 KB a level). This 8 MiB carries
/// that need some twelve times over in debug and forty in release — where a spawned thread's 2 MB
/// default would carry it in debug and a 512 KiB caller thread would not — so the margin is keryx's,
/// not the host's. Measured by encoding a ceiling-deep chain on threads of decreasing size and
/// bracketing the overflow (an abort, uncatchable), the method the inbound stacks' measures cite.
const ENCODE_STACK: usize = 8 << 20;

/// A message under construction — keryx's builder seam over prost-reflect, so the reassembly walk
/// (`super::assemble`) sets fields in keryx's terms. It wraps a [`DynamicMessage`] and routes every
/// field through the engine's **validating** setter ([`DynamicMessage::try_set_field_by_number`]),
/// which admits a value only when `Value::is_valid_for_field` holds (prost-reflect 0.16.5
/// `src/dynamic/mod.rs:639`) — the complement of the switch the encoders panic outside
/// (`src/dynamic/message.rs`). That check is *deep*: a list's every element, a map's every key and
/// value (recursively, `is_valid_for_field` for a message value), and a scalar, message, or enum at
/// the top, each against its kind (`is_valid`, `:678`). So a value that does not fit is refused here
/// as a `TermTypeMismatch`, before any message is encoded, at **each** of the encode's mismatch-panic
/// sites alike — a scalar, a list element or a packed field, a map key, a map value, a nested
/// message, an enum — never reaching the panic. keryx's own inverse §6 lowering
/// (`super::scalar::raise`) validates each value first, so a refusal at this seam is defense-in-depth
/// over an axis already foreclosed — a keryx invariant, not a payload's fault. The boundary test
/// `the_setter_forecloses_a_mismatch_at_every_encode_site` drives a mismatch at each site.
pub(crate) struct Building {
    message: DynamicMessage,
}

impl Building {
    /// A new, empty message of `desc`.
    pub(crate) fn new(desc: &MessageDescriptor) -> Building {
        Building {
            message: DynamicMessage::new(desc.clone()),
        }
    }

    /// Set the field numbered `number` to `value` through the validating setter — a value whose type
    /// does not fit the field, or a number the descriptor does not declare, is a `TermTypeMismatch`
    /// at `at`, never a panic. Defense-in-depth: keryx's inverse §6 lowering validated `value`
    /// first, so a refusal here would be a keryx invariant reached, caught before the encode.
    ///
    /// # Errors
    ///
    /// `TermTypeMismatch` at `at` when the engine's setter rejects the `(value, field)` pairing.
    pub(crate) fn set(&mut self, number: i32, value: Value, at: &str) -> Result<(), Diagnostic> {
        let field = u32::try_from(number).map_err(|_| set_mismatch(number, at))?;
        self.message
            .try_set_field_by_number(field, value)
            .map_err(|_| set_mismatch(number, at))
    }

    /// The built message as a field value, for nesting into a parent slot (a singular message field,
    /// a sequence element, a map value).
    pub(crate) fn into_value(self) -> Value {
        Value::Message(self.message)
    }
}

/// Encode a built message to binary wire bytes (`.binpb`) — the outbound mirror of
/// [`decode_binary`]. The encode runs on the shared sized-thread skeleton ([`on_sized_thread`],
/// [`ENCODE_STACK`]) with the containment frame *inside* it, as the inbound sized decodes do: a message admitted at the
/// reconstruction ceiling recurses that deep natively in the engine's unbounded serializer, and a
/// stack overflow aborts rather than unwinds, so the sized thread *prevents* it (for every message
/// the walk admits) and the frame catches what does unwind — an unforeseen engine panic — as a
/// `DependencyFault`. The mismatch-panic axis is foreclosed before here by [`Building::set`]'s
/// validating setter. The encode is single-threaded for all that it runs on its own thread: the
/// thread exists for its stack alone, the caller waits on it, and the bytes come back owned.
///
/// # Errors
///
/// `DependencyFault` for a contained engine panic (defense-in-depth: no keryx-admitted message is
/// known to fault the encode, the mismatch axis being foreclosed at the setter).
pub(crate) fn encode_binary(building: Building) -> Result<Vec<u8>, Diagnostics> {
    let message = building.message;
    let descriptor = message.descriptor();
    let operation = "encoding a payload";
    on_sized_thread(
        "keryx-encode",
        ENCODE_STACK,
        Dependency::ProstReflect,
        operation,
        move || {
            // The closure owns `message`; a fault drops it with the unwind, so nothing keryx observes
            // survives it. The encode reads no process-global state — it serializes the owned tree —
            // so the `AssertUnwindSafe` inside `contain` is sound.
            let bytes = contain(Dependency::ProstReflect, operation, || {
                message.encode_to_vec()
            })?;
            // Order every map field's entries by key (property 5, determinism): the engine holds a
            // map as a `HashMap` and encodes it in iteration order, which is not a function of its
            // contents, so keryx canonicalises its own well-formed output here — on this sized
            // thread, its native recursion bounded by the same ceiling. It is total by construction
            // (`canonical::canonicalize_map_order` never fails), so `on_sized_thread`'s join — which
            // would re-contain any unwind here as a `ProstReflect` fault — never fires for it; a bug
            // in it is a keryx bug to surface, not a dependency fault to mask.
            Ok(canonical::canonicalize_map_order(&bytes, &descriptor))
        },
    )
}

/// Encode a built message to the wire form `format` names — the outbound dispatch. Each runs the
/// engine's serializer on a thread keryx sizes ([`ENCODE_STACK`]) with the containment frame inside
/// it, as the binary encode does, and delivers its form's map-key determinism (property 5).
pub(crate) fn encode(building: Building, format: PayloadFormat) -> Result<Vec<u8>, Diagnostics> {
    match format {
        PayloadFormat::Binary => encode_binary(building),
        PayloadFormat::Textproto => encode_textproto(building),
        PayloadFormat::Json => encode_json(building),
    }
}

/// Encode to the protobuf text format (`.txtpb`) — `to_text_format_with_options` with
/// `expand_any(false)`, then keryx's map re-ordering over its own output ([`canonical_text`]): the
/// text writer emits maps in `HashMap` order and, unlike the JSON form, has no sorted intermediate
/// to route through. `expand_any(false)` keeps a `google.protobuf.Any` in its raw `type_url`/`value`
/// form — the form `canonical_text` can parse, and the form matching keryx's opaque `Any` shred
/// (§10). An *expanded* `[type…]{…}` is a grammar the canonicalizer cannot parse, so it would fall
/// through and leave that message's maps — the `Any`'s own and any sibling's — in `HashMap` order,
/// silently breaking determinism (property 5). UTF-8 bytes.
pub(crate) fn encode_textproto(building: Building) -> Result<Vec<u8>, Diagnostics> {
    let message = building.message;
    let descriptor = message.descriptor();
    let operation = "serializing a textproto message";
    on_sized_thread(
        "keryx-textproto-encode",
        ENCODE_STACK,
        Dependency::ProstReflect,
        operation,
        move || {
            let text = contain(Dependency::ProstReflect, operation, || {
                message.to_text_format_with_options(&FormatOptions::new().expand_any(false))
            })?;
            // Order every map field's entries by key (property 5): total keryx code over its own
            // well-formed text, so `on_sized_thread`'s join never re-contains it (as at
            // `encode_binary`); a bug in it is a keryx bug to surface, not a dependency fault.
            Ok(canonical_text::canonicalize_text(&text, &descriptor).into_bytes())
        },
    )
}

/// Encode to the canonical JSON mapping (`.json`) — routed through `serde_json`'s `Value`, whose
/// object is a sorted `BTreeMap` (no `preserve_order`), so map keys serialize in a deterministic
/// order (property 5), then to bytes.
pub(crate) fn encode_json(building: Building) -> Result<Vec<u8>, Diagnostics> {
    let message = building.message;
    // The root type, captured before `message` moves onto the thread: a well-known-type value the
    // serializer refuses names no field path, and validating the WKT at reassembly would
    // special-case §10's structural model (the symmetry keryx keeps across the forms), so the
    // refusal is named at the whole-message locus by this type.
    let type_name = message.descriptor().full_name().to_owned();
    let operation = "serializing a JSON message";
    let serialized = on_sized_thread(
        "keryx-json-encode",
        ENCODE_STACK,
        Dependency::SerdeJson,
        operation,
        move || {
            contain(
                Dependency::SerdeJson,
                operation,
                || -> Result<Vec<u8>, serde_json::Error> {
                    // `to_value` drives prost-reflect's `Serialize`, whose well-known-type
                    // serializers validate JSON-specific invariants the reassembler does not: an
                    // out-of-range `Timestamp`/`Duration`, an `Any` whose `type_url` the pool cannot
                    // resolve (§10 keeps `Any` opaque). Those are *values* — a `serde_json::Error`
                    // returned from the frame and mapped to `UnrepresentableJson` below — never an
                    // `expect`, whose panic `contain` would misattribute to serde_json (a keryx
                    // invariant is a bug to surface, not a dependency fault to mask). A genuine
                    // `Serialize` *panic* in prost-reflect is still contained here as a fault.
                    let value = serde_json::to_value(&message)?;
                    serde_json::to_vec(&value)
                },
            )
        },
    )?;
    serialized.map_err(|error| unrepresentable_json(&type_name, &error.to_string()))
}

/// `TermTypeMismatch` at `at`: the validating setter rejected a value keryx's inverse §6 lowering
/// had already accepted — a keryx invariant reached, caught before the encode. Names the field
/// number, never the value (an answer set's is the adversary's).
fn set_mismatch(number: i32, at: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::TermTypeMismatch,
        Locus::at(at),
        format!(
            "the value built for field number {number} does not fit its declared type; the engine's validating setter refused it after keryx's own §6 validation (a keryx invariant, caught before any encode)"
        ),
    )
}

/// Compose the `UnrepresentableJson` for a message a well-known-type value makes canonical JSON
/// unable to represent — an out-of-range `Timestamp`/`Duration`, or an `Any` whose `type_url` the
/// pool cannot resolve (§10 keeps `Any` opaque, so keryx does not resolve it). The whole-message
/// locus (the serializer names no field path, and validating the WKT at reassembly would
/// special-case §10's structural model — the symmetry keryx keeps across the forms), naming the
/// root type, with `serde_json`'s own message composed into the detail — never the value — and the
/// forms that carry it structurally named. The JSON counterpart of [`undecodable`]'s inbound
/// refusal and of `UnrepresentableText`: each output form refuses only what it alone cannot
/// represent.
fn unrepresentable_json(type_name: &str, error: &str) -> Diagnostics {
    Diagnostic::new(
        DiagnosticKind::UnrepresentableJson,
        Locus::whole(),
        format!(
            "a well-known-type value of `{type_name}` cannot be represented in canonical JSON: {error}; emit it as `--out binpb` or `--out txtpb`, which carry it structurally"
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
    use prost_reflect::{MapKey, MessageDescriptor, Value};
    use prost_types::{
        DescriptorProto, EnumDescriptorProto, EnumValueDescriptorProto, FieldDescriptorProto,
        FileDescriptorProto, FileDescriptorSet, MessageOptions,
    };

    use super::{
        Building, Datum, Decoded, Element, FieldValue, Key, SubMessage, decode_binary,
        encode_binary,
    };
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

    #[test]
    fn a_built_message_encodes_to_the_canonical_wire_bytes() {
        // Build a `Reading { string sensor = 1; int32 temp_c = 2; }` through the seam, encode it,
        // and get exactly the canonical wire form the reference builder writes (fields in number
        // order) — the outbound mirror of `decode_binary`, and the byte-identity the round-trip
        // property rests on. It also decodes back to a tree the walk would read the same.
        let pool = thermal_pool();
        let reading = pool
            .message_by_name("thermal.v1.Reading")
            .expect("the pool declares Reading");
        let mut building = Building::new(&reading);
        building
            .set(
                1,
                Value::String("s-1".to_owned()),
                "thermal.v1.Reading.sensor",
            )
            .expect("sensor sets");
        building
            .set(2, Value::I32(21), "thermal.v1.Reading.temp_c")
            .expect("temp_c sets");
        let bytes = encode_binary(building).expect("the built message encodes");
        assert_eq!(bytes, keryx_test_support::wire::reading("s-1", 21));
        // And it decodes back as `Reading` — no panic, a clean tree.
        decode_binary(&reading, &bytes).expect("the encoded bytes decode back as Reading");
    }

    #[test]
    fn a_mismatched_set_is_a_term_type_mismatch_never_a_panic() {
        // `sensor` (field 1) is a `string`; a numeric value does not fit its kind. The validating
        // setter refuses it as a `TermTypeMismatch` at the field's path, before any encode — the
        // engine's encode-time mismatch panic never reached (the axis `Building::set` forecloses).
        let pool = thermal_pool();
        let reading = pool
            .message_by_name("thermal.v1.Reading")
            .expect("the pool declares Reading");
        let mut building = Building::new(&reading);
        let error = building
            .set(1, Value::I32(7), "thermal.v1.Reading.sensor")
            .expect_err("a numeric value does not fit a string field");
        assert_eq!(error.kind(), DiagnosticKind::TermTypeMismatch);
        assert_eq!(error.locus().path(), Some("thermal.v1.Reading.sensor"));
    }

    /// A field of a hand-built message, with an optional referent (`type_name`).
    fn shape_field(
        name: &str,
        number: i32,
        r#type: i32,
        label: i32,
        type_name: Option<&str>,
    ) -> FieldDescriptorProto {
        FieldDescriptorProto {
            name: Some(name.to_owned()),
            number: Some(number),
            label: Some(label),
            r#type: Some(r#type),
            type_name: type_name.map(str::to_owned),
            ..Default::default()
        }
    }

    /// A pool with `Shapes { int32 s=1; repeated int32 list=2; map<int32,int32> m=3; Inner msg=4;
    /// map<int32,Inner> mmap=6; E e=7; }` — one field of every shape the engine's encode panics at.
    fn shapes_pool() -> RetainedPool {
        let map_entry = |name: &str, value_type: i32, value_name: Option<&str>| DescriptorProto {
            name: Some(name.to_owned()),
            field: vec![
                shape_field("key", 1, 5, 1, None), // int32 key
                shape_field("value", 2, value_type, 1, value_name),
            ],
            options: Some(MessageOptions {
                map_entry: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let inner = DescriptorProto {
            name: Some("Inner".to_owned()),
            field: vec![shape_field("x", 1, 5, 1, None)],
            ..Default::default()
        };
        let e = EnumDescriptorProto {
            name: Some("E".to_owned()),
            value: vec![
                EnumValueDescriptorProto {
                    name: Some("E_ZERO".to_owned()),
                    number: Some(0),
                    ..Default::default()
                },
                EnumValueDescriptorProto {
                    name: Some("E_ONE".to_owned()),
                    number: Some(1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let set = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("shapes.proto".to_owned()),
                package: Some("s".to_owned()),
                syntax: Some("proto3".to_owned()),
                message_type: vec![DescriptorProto {
                    name: Some("Shapes".to_owned()),
                    field: vec![
                        shape_field("s", 1, 5, 1, None),                            // int32
                        shape_field("list", 2, 5, 3, None), // repeated int32 (packed)
                        shape_field("m", 3, 11, 3, Some(".s.Shapes.MEntry")), // map<int32,int32>
                        shape_field("msg", 4, 11, 1, Some(".s.Shapes.Inner")), // singular message
                        shape_field("mmap", 6, 11, 3, Some(".s.Shapes.MmapEntry")), // map<int32,Inner>
                        shape_field("e", 7, 14, 1, Some(".s.Shapes.E")),            // enum
                    ],
                    nested_type: vec![
                        inner,
                        map_entry("MEntry", 5, None),
                        map_entry("MmapEntry", 11, Some(".s.Shapes.Inner")),
                    ],
                    enum_type: vec![e],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
        .encode_to_vec();
        let (_, pool) = descriptor::ingest_retaining(&set).expect("the shapes set ingests");
        pool
    }

    #[test]
    fn the_setter_forecloses_a_mismatch_at_every_encode_site() {
        // The engine panics at encode on a (Value, Kind) mismatch at each of its sites — a scalar, a
        // list element (and a packed field), a map key, a map value, a nested message, an enum.
        // `Building::set` routes every value through the validating setter (`try_set_field_by_number`
        // → `Value::is_valid_for_field`, prost-reflect 0.16.5 `src/dynamic/mod.rs:639`), which checks
        // the value's shape at every level — a list's each element, a map's each key and value
        // (recursively for a message value) — so a mismatch at any site is a `TermTypeMismatch` here,
        // before any encode, never a panic.
        let pool = shapes_pool();
        let desc = pool
            .message_by_name("s.Shapes")
            .expect("the pool declares Shapes");
        let mismatch = |number: i32, value: Value| {
            Building::new(&desc)
                .set(number, value, "s.Shapes")
                .expect_err("a mismatched value is refused before encode")
                .kind()
        };
        let map_of = |entries: [(MapKey, Value); 1]| Value::Map(entries.into_iter().collect());
        // A wrong-typed value at each site's shape is refused by the setter.
        assert_eq!(
            mismatch(1, Value::String("x".to_owned())),
            DiagnosticKind::TermTypeMismatch
        );
        assert_eq!(
            mismatch(2, Value::List(vec![Value::String("x".to_owned())])),
            DiagnosticKind::TermTypeMismatch
        );
        assert_eq!(
            mismatch(3, map_of([(MapKey::String("x".to_owned()), Value::I32(1))])),
            DiagnosticKind::TermTypeMismatch
        );
        assert_eq!(
            mismatch(3, map_of([(MapKey::I32(1), Value::String("x".to_owned()))])),
            DiagnosticKind::TermTypeMismatch
        );
        assert_eq!(mismatch(4, Value::I32(7)), DiagnosticKind::TermTypeMismatch);
        assert_eq!(
            mismatch(6, map_of([(MapKey::I32(1), Value::I32(7))])),
            DiagnosticKind::TermTypeMismatch
        );
        assert_eq!(
            mismatch(7, Value::String("x".to_owned())),
            DiagnosticKind::TermTypeMismatch
        );

        // The complement: a correctly-typed value at each site is admitted — the setter accepts
        // exactly what the encoders accept, so it refuses mismatches, not valid values.
        let mut ok = Building::new(&desc);
        ok.set(1, Value::I32(5), "s.Shapes").expect("a scalar fits");
        ok.set(2, Value::List(vec![Value::I32(0)]), "s.Shapes")
            .expect("a list element fits");
        ok.set(3, map_of([(MapKey::I32(1), Value::I32(2))]), "s.Shapes")
            .expect("a map fits");
        ok.set(7, Value::EnumNumber(1), "s.Shapes")
            .expect("an enum number fits");
    }

    #[test]
    fn a_nested_message_sequence_encodes_through_into_value() {
        // A `ReadingBatch { repeated Reading readings = 1; }`: each element is a `Building` finished
        // through `into_value` to a `Value::Message`, gathered into a `Value::List`, and set on the
        // parent — the nested-message and sequence path the reassembly walk composes. The bytes are
        // the canonical wire form the reference batch builder writes.
        let pool = thermal_pool();
        let batch_desc = pool
            .message_by_name("thermal.v1.ReadingBatch")
            .expect("the pool declares ReadingBatch");
        let reading_desc = pool
            .message_by_name("thermal.v1.Reading")
            .expect("the pool declares Reading");
        let reading = |sensor: &str, temp_c: i32| {
            let mut building = Building::new(&reading_desc);
            building
                .set(
                    1,
                    Value::String(sensor.to_owned()),
                    "thermal.v1.Reading.sensor",
                )
                .expect("sensor sets");
            building
                .set(2, Value::I32(temp_c), "thermal.v1.Reading.temp_c")
                .expect("temp_c sets");
            building.into_value()
        };
        let mut batch = Building::new(&batch_desc);
        batch
            .set(
                1,
                Value::List(vec![reading("s-1", 1), reading("s-2", 2)]),
                "thermal.v1.ReadingBatch.readings",
            )
            .expect("readings sets");
        let bytes = encode_binary(batch).expect("the batch encodes");
        assert_eq!(
            bytes,
            keryx_test_support::wire::batch(&[
                keryx_test_support::wire::reading("s-1", 1),
                keryx_test_support::wire::reading("s-2", 2),
            ])
        );
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
