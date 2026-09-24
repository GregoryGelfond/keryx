# keryx-core

The solver-free core of keryx: the Protocol Buffers → ASP-vocabulary compiler and the message ⇄ ground-facts codec, over themelios's `Symbol` algebra — plus the evolution instrument's comparison. It invokes no solver; a consuming tool brings its own and composes keryx's translation on both sides.

Start with **the keryx Book** ([`docs/book/`](../../docs/book/) — the library programmer's manual is Part II) and the worked [`examples/`](../../examples/).

```rust,ignore
use keryx_core::codec::Codec;

let codec = Codec::new(&descriptor_set)?;              // a schema → a Mapping + codec
let facts = codec.shred(root_type, &payload, format, &root)?;   // a message → Symbols
```

Not published to crates.io yet (its ASP layer is a git dependency); depend on it by git — see the Book's *Getting started*.
