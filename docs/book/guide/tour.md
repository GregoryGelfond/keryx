# A guided tour

keryx turns a **schema** into an ASP *vocabulary*, a **message** into ground *facts* over that vocabulary — and an *answer set* back into a message:

```text
message ──keryx facts──▶ facts ──your solver──▶ answer set ──keryx emit──▶ message
                                (keryx runs none)
```

keryx never runs a solver. The solve in the middle is yours (clingo today), over the vocabulary keryx generated. This tour walks the whole motion on a small schema; the [`thermal`](https://github.com/GregoryGelfond/keryx/tree/main/examples/thermal) example is the same thing, runnable and golden-tested.

## The schema

Four messages in package `thermal.v1`:

```proto
syntax = "proto3";
package thermal.v1;

message Reading      { string sensor = 1; int32 temp_c = 2; }
message ReadingBatch { repeated Reading readings = 1; }

message Alert    { string sensor = 1; int32 temp_c = 2; }
message AlertSet { repeated Alert alerts = 1 [(keryx.set) = true]; }
```

`sensor` and `temp_c` are declared in both `Reading` and `Alert` — a shared field name, which keryx handles by *polymorphism-by-sort*: one predicate serves both. `AlertSet.alerts` carries `(keryx.set)`, marking it a membership relation rather than a sequence.

## The vocabulary — `keryx gen`

```sh
keryx gen thermal.proto -I . -o gen/
```

`gen` writes one file set per package: `core.lp` (the sorts and base-fact predicates), `views.lp` (a relational view per message-typed field), `emit.lp` (the outbound theory), and a manifest. `core.lp` declares each sort and field with `#defined` and a readable `%!` signature, so a model is written *against* a vocabulary rather than handed a bare dump of facts:

```prolog
%! sort reading/1
#defined reading/1.
%! sensor : alert -> string  (total)
%! sensor : reading -> string  (total)
#defined sensor/2.
```

`sensor/2` appears once, carrying both sorts' signatures — the shared-field merge. A model that only ever *consumes* a predicate draws no "atom does not occur in any rule" warning, because the vocabulary declares its own shape.

## The facts — `keryx facts`

Shred a batch of two readings into ground facts over that vocabulary:

```sh
keryx facts --root ReadingBatch=batch.binpb thermal.proto -I .
```

```prolog
{{#include ../../../examples/thermal/gen/thermal.v1.facts.lp}}
```

Each reading is the access-path term `readings(r0, i)` — its position in the sequence, hanging off the root — not a minted handle, and the very term the `readings/3` view in `views.lp` joins on. The same payload shreds to the same facts from binary, textproto, or JSON.

## The round trip — `keryx emit`

Between `facts` and `emit` sits **your** solver, ranging over this vocabulary. Its answer set — the facts a model exports under a marker like `emit_reading_batch(r0)` — reassembles back to a message:

```sh
keryx emit --root ReadingBatch answer.lp thermal.proto -I . > out.binpb
```

`out.binpb` is byte-for-byte the payload you started from. `emit.lp` guarantees this can't go wrong: an answer set that could not be a message is not an answer set (UNSAT under the strict theory) or is reported at the field path (under the diagnostic theory) — never garbage bytes.

Read on for [the translation model](translation-model.md) — how each protobuf construct becomes vocabulary — or jump to [any command](../reference/commands.md).
