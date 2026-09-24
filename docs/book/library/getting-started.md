# Getting started

keryx is a library first: the `keryx` command is a thin shell over `keryx-core`, and a tool that embeds keryx gets the whole bridge without shelling out. This part is for that tool builder.

## Depending on keryx

keryx is not published on crates.io yet (its ASP layer is a git dependency). Depend on it by git, pinned to a release tag:

```toml
[dependencies]
keryx-core = { git = "https://github.com/GregoryGelfond/keryx", tag = "v1.0.0" }
```

The build fetches the pinned ASP-layer dependency over HTTPS — no credentials, locally and in CI.

## The shape of the API

Everything a consuming tool needs is in `keryx_core`:

- [`Mapping`](mapping.md) — what a schema compiles to: how each message, field, and enum lowers. The stable interface.
- [`Codec`](codec.md) — the bidirectional payload codec: `shred` a message to facts, `reassemble` an answer set to a message.
- The **value plane** — `Symbol`, `Name`, and `Sign`, re-exported from the ASP layer so keryx's public surface is self-contained. A payload's facts *are* `Symbol`s; the library seam hands them to you directly, with no text in between.
- [Diagnostics](diagnostics.md) — every foreign input crosses a `Result` boundary returning a typed diagnostic at a field-path locus, never a panic or a bare string.
- The [fault boundary](faults.md) — a dependency's misbehavior on a foreign-input path is contained as a value, not a crash.

## A first call

Compile a schema and read the mapping it produced — no payload, no solver:

```rust,ignore
{{#include ../examples/inspect.rs:example}}
```

The [`codec`](codec.md) chapter takes it further — shredding a message to facts and reassembling one back.
