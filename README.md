# keryx

κῆρυξ, *herald* — a bidirectional bridge between Protocol Buffers and Answer Set Programming.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
![Rust 1.97+](https://img.shields.io/badge/rust-1.97%2B-orange?style=flat-square)
[![Coverage 97%](https://img.shields.io/badge/coverage-97%25-brightgreen?style=flat-square)](CONTRIBUTING.md#verification-and-review)
[![Documentation](https://img.shields.io/badge/docs-the%20keryx%20book-blue?style=flat-square)](https://gregorygelfond.github.io/keryx/)

**Documentation: [the keryx Book](https://gregorygelfond.github.io/keryx/).**

keryx compiles a `.proto` schema into an ASP vocabulary and translates messages
into ground facts — and answer sets back into messages. The message side never
learns ASP; the model side never learns the wire.

keryx doesn't solve — bring your own solver (clingo today).

## What it looks like

A message becomes a **sort**; its scalar fields become **predicates** over that
sort; a message-typed field becomes an occupant term with a relational **view**
to join on.

```proto
message Reading { string sensor = 1; int32 temp_c = 2; }
message Batch   { repeated Reading readings = 1; }
```

**`keryx gen`** compiles the schema into the vocabulary a model is written against.
`core.lp` declares the sorts and base-fact field predicates with `#defined`,
carrying the readable signature as a `%!` doc comment; `views.lp` adds a
relational view rule per message-typed field:

```prolog
%! sort batch/1
%! readings : batch × index -> reading  (sequence)
#defined batch/1.
%! sort reading/1
#defined reading/1.
%! sensor : reading -> string  (total)
#defined sensor/2.
%! temp_c : reading -> int32  (total)
#defined temp_c/2.

readings(P, I, E) :- reading(E), E = readings(P, I).   % views.lp
```

Then the bridge runs. A **`Batch` message becomes ground facts** over that
vocabulary — each nested reading is the access-path term `readings(b, i)`, not a
minted handle:

```prolog
batch(b).
reading(readings(b,0)).  sensor(readings(b,0),"s1").  temp_c(readings(b,0),20).
reading(readings(b,1)).  sensor(readings(b,1),"s2").  temp_c(readings(b,1),105).
```

Those are the base facts. Loaded together with `views.lp`, the view rule *derives*
the `readings/3` relation, which a client's own model joins on — e.g. flagging an
overheating reading:

```prolog
hot(B, R) :- readings(B, _, R), temp_c(R, T), T > 100.   % ⊢ hot(b, readings(b,1))
```

Solve that with **your own clingo**, and keryx turns the answer set **back into a
`Batch`** — byte-for-byte the payload you started from. keryx never runs the solver.

## Install

keryx is not on crates.io yet (its ASP layer is a git dependency). Install the CLI from the tagged release:

```sh
cargo install --git https://github.com/GregoryGelfond/keryx --tag v1.0.0 keryx-cli
keryx --help
```

The build fetches the pinned ASP-layer dependency over HTTPS — no credentials needed.

## Documentation

- **[The keryx Book](https://gregorygelfond.github.io/keryx/)** — the manual: the bridge, the Rust library, and the command reference.
- [`examples/`](examples/) — seven runnable, golden-tested worked examples, schema to vocabulary to facts and back, plus a schema's evolution.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — the standard the work is held to.
- The design of record: [`docs/design/architecture.md`](docs/design/architecture.md), [`docs/specification.md`](docs/specification.md), and [`docs/design/threat-model.md`](docs/design/threat-model.md).

## Use from Rust

keryx is a library first; the command is a thin shell over `keryx-core`. A tool embeds the bridge directly — `Mapping`, `Codec::shred`/`reassemble`, and the value plane (`Symbol`/`Name`/`Sign`) — with no text between it and its solver. See the Book's [Rust library](https://gregorygelfond.github.io/keryx/library/getting-started.html) part.

## Status

keryx translates **proto2 and proto3** schemas and messages in both directions, on all
three wire forms:

- **`keryx gen`** / **`keryx explain`** — a schema becomes the ASP vocabulary
  (`core.lp`, `views.lp`, `emit.lp`, and a manifest), and the mapping is inspectable.
- **`keryx facts`** — a message becomes ground facts (binary, textproto, or JSON payload).
- **`keryx emit`** — an answer set becomes a message again, a byte-for-byte round trip in
  every format.
- **`keryx diff`** — two versions of a schema (`.proto` or `.binpb` apiece) become a migration
  report for the model side: what a model written against the old vocabulary survives and what
  it does not, the changeset as JSON (`--json`), and a bridge view per clean rename through
  which the old model reads the new facts (`--bridge`); `--exit-code` makes a breaking change
  the exit.
- **Annotations** give a field or enum its precise ASP treatment, validated at the policy
  door (a mis-targeted or malformed option is a structured diagnostic, never a silent
  mis-lowering): `(keryx.set)` for a repeated field whose order is incidental (a membership
  relation, not a sequence), `(keryx.numeric)` for integer width, `(keryx.scale)` /
  `(keryx.opaque)` for floats, and `(keryx.unknown) = PRESERVE` to carry an open enum's
  unknown wire values through translation. A proto2 `required` field carries its outbound
  totality obligation — full proto2/proto3 parity.

**Not yet:**

- **`views.lp` usage descriptions** — the projection views carry their signature line today;
  the schema-composed usage prose is in progress.
- **Editions** (2023, 2024) — an editions schema is refused with a specific diagnostic,
  since keryx's descriptor engine has no editions support yet; it becomes a drop-in when the
  engine does. keryx branches on *resolved features*, not syntax era, so editions land
  without a redesign. See [`docs/proto-support.md`](docs/proto-support.md).

## Built on

[themelios](https://github.com/GregoryGelfond/themelios) — the ASP program
representation keryx builds on. keryx is a translation library: a consuming tool
composes it around its own solver, using keryx for the protobuf↔ASP bridge on
both sides.

## License

MIT. See [LICENSE](LICENSE).
