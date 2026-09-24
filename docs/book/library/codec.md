# The codec

`Codec` is the bidirectional payload codec: it shreds a message into ground facts and reassembles an answer set back into a message. One `Codec` is built per schema and drives both directions.

## Building a codec

From a compiled descriptor set, or from `.proto` source with its include roots:

```rust,ignore
use keryx_core::codec::Codec;

let codec = Codec::new(&descriptor_set)?;         // a self-contained .binpb
// or
let codec = Codec::from_source(&protos, &includes)?;
```

## Shredding — message to facts

`shred` reads a payload as an instance of a named root type, from a fresh root constant, and returns the facts:

```rust,ignore
use keryx_core::codec::{PayloadFormat, Root};

let facts = codec.shred(
    "thermal.v1.ReadingBatch",   // the root type the payload instantiates
    &payload,                    // the bytes
    PayloadFormat::Binary,       // Binary, Textproto, or Json
    &Root::fresh(0),             // the root constant, r0
)?;

for symbol in facts.symbols() {  // the facts, as Symbols — no text between
    // hand them to your solver
}
let lp: String = facts.render()?; // or render them as an .lp module
```

The same payload shreds to the same facts in any of the three formats, under the same field-path refusals and the same nesting ceiling ([Payloads to facts](../guide/payload-formats.md)).

## Reassembling — answer set to message

`reassemble` takes an answer set — a `Vec<Symbol>`, which you can read from `.lp` text with `raise_answer_set` — and rebuilds the messages its markers export:

```rust,ignore
use keryx_core::codec::{Codec, raise_answer_set};

let symbols = raise_answer_set(&lp_text)?;        // .lp → Vec<Symbol>
let reassembled = codec.reassemble(&symbols, out_format)?;
for message in reassembled.messages() {
    let _ = message.type_name();
    let _ = message.bytes();                      // the canonical wire bytes
}
```

An answer set that could not be a message is refused at its field path (`ShapeViolation`), never serialized as the nearest thing — the outbound counterpart of the inbound refusals. The round trip is byte-for-byte: a payload shredded and then reassembled is the payload again.
