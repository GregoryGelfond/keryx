# Payloads to facts

`keryx facts` shreds a message into ground facts over its schema's vocabulary. A payload may arrive in any of the three wire forms Protocol Buffers defines, and all three shred to the *same* facts:

- the **binary** wire format (`.binpb`),
- the protobuf **text** format (`.txtpb`),
- the canonical **JSON** mapping (`.json`).

```sh
keryx facts --root ReadingBatch=batch.binpb thermal.proto -I .
keryx facts --root ReadingBatch=batch.txtpb thermal.proto -I .   # the same facts
keryx facts --root ReadingBatch=batch.json  thermal.proto -I .   # and again
```

One `Codec`, one walk, and one admission policy serve all three: the same field-path refusals, the same facts, the same uniform nesting ceiling of **99** message-typed levels below the root. The forms differ only in what each *admits* on the way in, and in how each decoder's own bound meets that ceiling.

## The nesting ceiling

keryx bounds how deep a payload may nest, so no adversarial input can exhaust the stack:

- **binary** is admitted to 99 message-typed levels; deeper is refused whole (`PayloadTooDeep`, then the engine's own limit past it).
- **textproto** is bounded *ahead* of the engine's unbounded text parser by a pre-parse guard that measures `{ }`/`< >` nesting and refuses past 99 before a token is parsed. The measure is exact for singular and repeated message fields and conservative for map entries and expanded `Any` (it over-refuses, never admits deeper).
- **JSON** carries no guard of keryx's: the deserializer bounds its own nesting (128 containers), and the walk's ceiling binds beneath that for a chain of singular message fields, while a repeated or map chain meets the deserializer's count first — refusal always in the safe direction.

These are the same conservative bound reached three ways; a payload keryx admits in one form shreds to the same facts as in the others.

## What is refused

A payload that does not decode as the named root type is `UndecodablePayload`. Everything else — a value outside a field's type, a member the schema does not declare — is a structured diagnostic at that field's path, never a silent coercion. keryx reads a payload as one canonical value of the root type or refuses it; it never reads it as the nearest thing.

The full support matrix — per version, per format, with the exact bounds — is in [Protocol Buffer support](../reference/proto-support.md).
