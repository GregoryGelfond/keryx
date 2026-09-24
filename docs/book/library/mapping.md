# The mapping

A schema compiles to a `Mapping` — keryx's account of how every message, field, and enum in the schema lowers to the ASP vocabulary. It is the stable interface between the compiler and everything downstream: `gen` renders it to `.lp` modules, `explain` reports it, the codec shreds and reassembles against it, and `diff` compares two of them.

## Producing a mapping

`policy::map` takes a validated `Schema` and returns a `Mapping` or a diagnostic:

```rust,ignore
use keryx_core::policy::{self, Mapping};

let mapping: Mapping = policy::map(&schema)?;
```

A `Codec` holds one too — `codec.mapping()` — so a tool that already has a codec need not map separately.

## What it carries

For each subject in the schema, the mapping records the lowering keryx decided: a message's sort predicate, a scalar or enum field's base-fact predicate and its totality, a message-typed field's occupant term and relational view, and the treatment any annotation set (a `(keryx.set)` membership relation, a numeric width, a float scale). Because the mapping is a value — not rendered text — two mappings can be *compared* directly, which is exactly what [`keryx diff`](../guide/evolution.md) does: it never re-parses generated `.lp`, it compares the models.

## Why a value, not text

Keeping the mapping a structured value is what lets keryx guarantee its outputs cohere. The report, the JSON changeset, and a rename bridge are each a pure view of the same mapping, so they cannot disagree; and no keryx output is produced by string-templating — the emission layer constructs ASP syntax values from the mapping and renders them, never assembles text by hand.
