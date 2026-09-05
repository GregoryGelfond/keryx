# The thermal example — `keryx gen` and `keryx facts`

A worked example of keryx's solver-free half (spec §28): a small Protocol Buffers schema
of sensor readings and overheating alerts, the Answer Set Programming *vocabulary* keryx
generates from it, and a batch of readings shredded to ground *facts* over that vocabulary.
This is the front half of the bridge — schema to vocabulary, payload to facts — with no
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

## Generating the vocabulary, shredding a payload

Run `keryx gen` (the schema imports `keryx/options.proto` for `(keryx.set)`, which keryx
resolves from its embedded registry — no `-I` for it), then `keryx facts` over the committed
payload — [`batch.binpb`](batch.binpb) on the wire, the same message as text,
[`batch.txtpb`](batch.txtpb), or as canonical JSON, [`batch.json`](batch.json); the format is
named by the extension, and each shreds to the same facts:

```sh
keryx gen thermal.proto -I . -o gen/
keryx facts --root ReadingBatch=batch.binpb thermal.proto -I . > gen/thermal.v1.facts.lp
keryx facts --root ReadingBatch=batch.txtpb thermal.proto -I .   # the same seven facts
keryx facts --root ReadingBatch=batch.json thermal.proto -I .    # and again
```

`gen` writes one file set per package (spec §13). For `thermal.v1` that is the three files in
[`gen/`](gen/): `thermal.v1.core.lp`, `thermal.v1.views.lp`, and `thermal.v1.keryx-manifest`.
`facts` prints to stdout — the product, ready for `| clingo` — so the fourth file there,
`thermal.v1.facts.lp`, is that output captured.

## What it generates

### `thermal.v1.core.lp` — the honorary signature (spec §13.1)

The sorts and the base-fact (scalar) field predicates, one **`#defined`** declaration each; a
message-typed field has no base predicate here, so its functional signature rides on its
parent sort's declaration:

```prolog
%!An alert raised for an overheating reading.
%!sort alert/1
#defined alert/1.
%!A batch of readings — a sequence.
%!sort reading_batch/1
%!readings : reading_batch × index -> reading  (sequence)
#defined reading_batch/1.
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
the shared-field merge (spec §4.2). The repeated message fields `readings` and `alerts` have no
`#defined` of their own: they are message-typed, so keryx's canonical form is the occupant
access-path term (`readings(P, I)`, spec §4.1), and their signature rides on the parent sort as
shown above. The relational view a model author joins on lives in `views.lp`.

### `thermal.v1.views.lp` — the relational views (spec §13.2)

An additive module, and a client of `core.lp`: it opens by including the base — so it is
loadable on its own — then adds one access-path view rule per message-typed occupant, so a
downstream model can range over the elements of a sequence by sort:

```prolog
#include "thermal.v1.core.lp".
%!readings : reading_batch × index -> reading  (sequence)
readings(P, I, E) :- reading(E), E = readings(P, I).
```

`readings` and `alerts` each get a **sequence** view (their elements are messages —
`Reading`, `Alert`). The scalar fields `sensor` and `temp_c` need no view. A project that wants
only the functional canon can exclude this file (spec §13.2); `core.lp` stands on its own.

### `thermal.v1.keryx-manifest` — the evolution contract (spec §13.4)

The number↔name binding: every proto path and field number, and the emitted predicate,
arity, and shape it maps to.

```
thermal.v1.AlertSet.alerts #1 fam  alerts/2 -> alert  alert  seq ; view alerts/3
```

The record's own arity, `alerts/2`, is the occupant access-path term keryx keys facts on
(spec §4.1); `; view alerts/3` names the relational view in `views.lp`. This is the contract a
later revision of the schema is checked against — the record of what each element *became*, so
a rename, a renumber, or a treatment change is a visible, reviewable diff rather than a silent
break (schema-diff checking lands at Increment 5).

### `thermal.v1.facts.lp` — the ground facts (spec §11)

[`batch.binpb`](batch.binpb) is the spec's own payload (§28) on the wire: two readings,
`{sensor: "s-101", temp_c: 44}` and `{sensor: "s-107", temp_c: 21}`. Shredded as a
`ReadingBatch` — `--root` names the message type the payload is an instance of — from the root
constant `r0` that `facts` mints for the invocation, it is seven facts over the vocabulary above:

```prolog
reading(readings(r0, 0)).
reading(readings(r0, 1)).
reading_batch(r0).
sensor(readings(r0, 0), "s-101").
sensor(readings(r0, 1), "s-107").
temp_c(readings(r0, 0), 44).
temp_c(readings(r0, 1), 21).
```

Each reading is the access-path term `readings(r0, i)` — its index in the sequence, hanging
from the root (spec §4.1) — not a minted identity, and the very term the `readings/3` view in
`views.lp` joins on. `sensor` and `temp_c` are total, so both atoms exist for every reading.
The same payload always shreds to the same facts, in every form — [`batch.txtpb`](batch.txtpb)
is this batch in the protobuf text format, its `# proto-file:` / `# proto-message:` header naming
the schema and the root type, and [`batch.json`](batch.json) is the batch in the protobuf JSON
mapping; each shreds to this very file — so the file is golden-comparable like the three beside it.

## The solver-free path

`gen` and `facts` are the front half of the bridge. The whole round trip is:

1. **`keryx gen`** — schema → the ASP vocabulary above (this example).
2. **`keryx facts`** — a `ReadingBatch` payload → ground facts over that vocabulary (this
   example).
3. **your clingo** — the facts plus your own model (constraints, derivations) → an answer set.
   keryx invokes no solver; the solver is yours.
4. **reassemble** — an answer set → an outbound protobuf payload (Increment 4).

Everything keryx does here is a pure, deterministic function of the schema and the payload —
no solver, no network, golden-comparable. The end-to-end transient solve is wired together at
Increment 4; this example is the piece that is real today.

## Scope at this stage

- **Binary, textproto, and JSON payloads.** `facts` reads the binary wire format (`.binpb`), the
  protobuf text format (`.txtpb`), and the protobuf JSON mapping (`.json`) — every payload form
  spec §26 names — nesting in each bounded at the same ceiling: text ahead of its parser, JSON
  beneath its deserializer's own count.
- **`(keryx.set)` is inert at this stage.** keryx ingests the annotation (it appears as an `opt/3`
  descriptor fact) but does not yet read it for translation, so `AlertSet.alerts` is generated
  as a **sequence**, exactly like `ReadingBatch.readings` — `alerts/2` with a sequence view
  `alerts/3`.
  Set semantics — order- and multiplicity-insensitive membership — arrive with annotation
  reading at Increment 5, at which point `alerts` becomes a membership relation.
- **No `shape.lp` yet.** The serializability guard that constrains an answer set to a
  reassemblable shape (spec §13.3) is generated with the outbound codec at Increment 4.
