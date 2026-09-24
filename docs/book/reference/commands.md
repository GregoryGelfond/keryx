# Using the keryx command

`keryx` is a bidirectional bridge between Protocol Buffers and Answer Set Programming. It compiles schemas, translates messages to facts, reassembles answer sets into messages, and diffs schema versions. It never invokes a solver.

Each command reads a `.proto` (with `-I` include roots) or a self-contained `.binpb` descriptor set.

## `keryx gen`

Compile a schema into the ASP vocabulary a model is written against: `core.lp` (sorts and base-fact predicates as `#defined` declarations, each carrying its signature as a `%!` doc comment), `views.lp` (a relational view per message-typed field), `emit.lp` (the outbound theory), and a manifest.

```sh
keryx gen schema.proto -o out/
```

## `keryx explain`

Report the mapping verdict for a schema without writing artifacts — how each message, field, and enum lowers, and why.

## `keryx facts`

Translate a message into ground facts over a schema's vocabulary. The payload may be the binary wire form, the Protocol Buffers text format, or the canonical JSON mapping.

```sh
keryx facts --root pkg.Msg=payload.binpb schema.proto
```

## `keryx emit`

Reassemble an answer set — read from an `.lp` file — back into a message, serialized to the binary wire form, textproto, or JSON: a byte-for-byte round trip of what `facts` produced.

```sh
keryx emit --root pkg.Msg answer.lp schema.proto --out binpb > message.binpb
```

## `keryx diff`

Compare two versions of a schema at the level of the ASP vocabulary: what a model written against the old vocabulary survives and what it does not. Emits a migration report, a JSON changeset (`--json`), and a bridge view per clean rename (`--bridge <path>`) through which the old model reads the new facts. `--exit-code` turns a breaking change into the process exit.

```sh
keryx diff old.proto new.proto
keryx diff --json old.binpb new.binpb
```

## Exit codes

keryx separates failure classes by exit code, so a script can branch on them: success, an internal error, a usage error, an unreadable input, a schema rejection, a shape violation, a translation refusal, a dependency fault, and — under `keryx diff --exit-code` — a breaking divergence are each their own class. The exact values are listed in the architecture (`docs/design/architecture.md`).
