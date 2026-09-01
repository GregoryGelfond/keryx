# The thermal example — `keryx gen`

A worked example of keryx's solver-free half (spec §28): a small Protocol Buffers schema
of sensor readings and overheating alerts, and the Answer Set Programming *vocabulary* keryx
generates from it. This is the front half of the bridge — schema to vocabulary — with no
solver in the loop; the round trip through clingo is sketched at the end.

## The schema

[`thermal.proto`](thermal.proto) — four messages in package `thermal.v1`:

```proto
syntax = "proto3";
package thermal.v1;

import "keryx/options.proto";

// A single sensor reading.
message Reading      { string sensor = 1; int32 temp_c = 2; }
// A batch of readings — a sequence.
message ReadingBatch { repeated Reading readings = 1; }

// An alert raised for an overheating reading.
message Alert    { string sensor = 1; int32 temp_c = 2; }
// The set of alerts emitted for a batch.
message AlertSet { repeated Alert alerts = 1 [(keryx.set) = true]; }
```

Two things to note in the schema itself:

- **`sensor` and `temp_c` are shared field names**, declared in both `Reading` and `Alert`.
  This is polymorphism-by-sort (spec §4.2): one predicate `sensor/2` serves both sorts, its
  meaning fixed by which sort the first argument inhabits. The generated vocabulary reflects
  this — a single declaration carrying *both* sorts' signatures.
- **`AlertSet.alerts` carries `[(keryx.set) = true]`**, keryx's annotation vocabulary
  (spec Appendix A). See the scope note at the bottom for what it does — and does not yet —
  mean.

Editions is transliterated to proto3 here (the pure-Rust front-door compiler does not yet
cover editions; spec §31). proto3's implicit scalars resolve exactly as an edition-2023
default would, so the generated vocabulary is the same.

## Generating the vocabulary

Run `keryx gen` with the vendored keryx option registry on the include path (the schema
imports `keryx/options.proto` for `(keryx.set)`):

```sh
keryx gen thermal.proto -I . -I ../../crates/keryx-core/proto -o gen/
```

keryx writes one file set per package (spec §13). For `thermal.v1` that is the three files in
[`gen/`](gen/): `thermal.v1.core.lp`, `thermal.v1.views.lp`, and `thermal.v1.keryx-manifest`.

## What it generates

### `thermal.v1.core.lp` — the honorary signature (spec §13.1)

The sorts and field predicates, one **`#defined`** declaration each:

```prolog
%!An alert raised for an overheating reading.
%!sort alert/1
#defined alert/1.
…
%!sensor : alert -> string  (total)
%!sensor : reading -> string  (total)
#defined sensor/2.
```

Each declaration reads as a signature: `alert/1` is a sort (a unary membership predicate);
`sensor : reading -> string (total)` is a total function from the `reading` sort to a string.
The signature — and any proto doc comment — rides as `%!` documentation lines, but the
`#defined` beneath is a real declaration, not a comment: it tells the grounder the predicate
exists and with what arity, so a predicate that is only ever *consumed* (say, by a model the
reader writes over this vocabulary) does not draw an "atom does not occur in any rule"
warning. The vocabulary declares its own shape.

`sensor/2` and `temp_c/2` each appear once, carrying both `alert` and `reading` signatures —
the shared-field merge (spec §4.2). `alerts/3` and `readings/3` are the repeated fields, each
an index-keyed family (a sequence, spec §7.1).

### `thermal.v1.views.lp` — the relational views (spec §13.2)

One access-path view rule per message-typed occupant, so a downstream model can range over
the elements of a sequence by sort:

```prolog
%!readings : reading_batch × index -> reading  (sequence)
readings(P, I, E) :- reading(E), E = readings(P, I).
```

`readings` and `alerts` each get a **sequence** view (their elements are messages —
`Reading`, `Alert`). The scalar fields `sensor` and `temp_c` need no view.

### `thermal.v1.keryx-manifest` — the evolution contract (spec §13.4)

The number↔name binding: every proto path and field number, and the emitted predicate,
arity, shape, and totality it maps to.

```
thermal.v1.AlertSet.alerts #1 fam  alerts/3 -> alert  alert  total  ; view alerts/3
```

This is the contract a later revision of the schema is checked against — the record of what
each element *became*, so a rename, a renumber, or a treatment change is a visible, reviewable
diff rather than a silent break (schema-diff checking lands at Increment 5).

## The solver-free path

`gen` is the front half of the bridge. The whole round trip is:

1. **`keryx gen`** — schema → the ASP vocabulary above (this example).
2. **`keryx` fact codec** — a `ReadingBatch` payload → ground facts over that vocabulary
   (the inbound codec, Increments 3–4).
3. **your clingo** — the facts plus your own model (constraints, derivations) → an answer set.
   keryx invokes no solver; the solver is yours.
4. **reassemble** — an answer set → an outbound protobuf payload (Increment 4).

Everything keryx does here is a pure, deterministic function of the schema — no solver, no
network, golden-comparable. The end-to-end transient solve is wired together at Increment 4;
this example is the piece that is real today.

## M1 scope

- **`(keryx.set)` is inert at M1.** keryx ingests the annotation (it appears as an `opt/3`
  descriptor fact) but does not yet read it for translation, so `AlertSet.alerts` is generated
  as a **sequence**, exactly like `ReadingBatch.readings` — `alerts/3` with a sequence view.
  Set semantics — order- and multiplicity-insensitive membership — arrive with annotation
  reading at Increment 5, at which point `alerts` becomes a membership relation.
- **No `shape.lp` yet.** The serializability guard that constrains an answer set to a
  reassemblable shape (spec §13.3) is generated with the outbound codec at Increment 4.
