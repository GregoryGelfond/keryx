# The thermal example — `keryx gen`, `keryx facts`, and `keryx emit`

A worked example of keryx's solver-free bridge (spec §28): a small Protocol Buffers schema
of sensor readings and overheating alerts, the Answer Set Programming *vocabulary* keryx
generates from it, a batch of readings *shredded* to ground *facts* over that vocabulary (to
**shred** is to break a structured message down into flat, per-field facts), and an answer set
reassembled back to the payload it came from. Both halves of the bridge — schema
to vocabulary, payload to facts, and facts back to payload — with no solver in the loop; where
the solver would sit is sketched at the end.

## Why this shape, not a hand-rolled shim

A one-off converter can turn a message into atoms; the value is in *how*. Four choices
this example makes that a hand-rolled proto→ASP shim typically does not:

- **Canonical identity.** Each nested reading is the access-path term `readings(r0, i)`
  — stable, meaningful, and joinable — not an opaque handle minted per run.
- **Polymorphism-by-sort.** `sensor` and `temp_c` are declared once and serve both
  `reading` and `alert`, their meaning fixed by the sort of the first argument (§4.2),
  not mangled into per-message names.
- **A declared vocabulary.** `core.lp` states each sort and field with `#defined` and a
  readable `%!` signature, so a model is written *against* a vocabulary — not handed a
  bare dump of facts that draws an "atom does not occur in any rule" warning.
- **Checked serializability.** `emit.lp` makes an answer set that could not be a message
  UNSAT (or, under the diagnostic theory, reportable), so the round trip cannot emit
  garbage bytes.

The rest of this walkthrough shows each of these in the generated files.

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
  (spec Appendix A). See the scope note at the bottom for what it means.

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

`gen` writes one file set per package (spec §13). For `thermal.v1` that is the four files in
[`gen/`](gen/): `thermal.v1.core.lp`, `thermal.v1.views.lp`, `thermal.v1.emit.lp`, and
`thermal.v1.keryx-manifest`. `facts` prints to stdout — the product, ready for `| clingo` — so
the fifth file there, `thermal.v1.facts.lp`, is that output captured.

## What it generates

### `thermal.v1.core.lp` — the honorary signature (spec §13.1)

The sorts and the base-fact (scalar) field predicates, one **`#defined`** declaration each; a
message-typed field has no base predicate here, so its functional signature rides on its
parent sort's declaration:

```prolog
%! An alert raised for an overheating reading.
%! sort alert/1
#defined alert/1.
%! A batch of readings — a sequence.
%! sort reading_batch/1
%! readings : reading_batch × index -> reading  (sequence)
#defined reading_batch/1.
…
%! sensor : alert -> string  (total)
%! sensor : reading -> string  (total)
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
%! readings : reading_batch × index -> reading  (sequence)
readings(P, I, E) :- reading(E), E = readings(P, I).
```

`readings` gets a **sequence** view (its elements are indexed messages). `alerts`, a
`(keryx.set)` membership relation, gets **no** view — its membership `alerts/2` is a base
relation a model asserts, not a projection over occupancy (spec §7.1). The scalar fields
`sensor` and `temp_c` need no view. A project that wants only the functional canon can exclude
this file (spec §13.2); `core.lp` stands on its own.

### `thermal.v1.emit.lp` — the serializability theory (spec §13.3)

Outbound, the invariants the wire guaranteed inbound become obligations an answer set must
meet before it can be a message. A client of `core.lp` like `views.lp`, the module declares a
root marker per message sort, closes `reach/1` over the message-typed slots beneath every
asserted marker, and guards each obligation by `reach` and by its sort:

```prolog
#include "thermal.v1.core.lp".
%! readings : reading_batch × index -> reading  (sequence)
reach(A) :- reach(X), reading(A), reading_batch(X), A = readings(X, I).
%! emit_reading_batch : reading_batch  (root)
reach(X) :- emit_reading_batch(X).
%! functionality of sensor : reading -> string  (total)
:- reach(P), reading(P), sensor(P, V1), sensor(P, V2), V1 != V2.
%! contiguity of readings : reading_batch × index -> reading  (sequence)
:- has_readings(P, I), reach(P), reading_batch(P), I > 0, not has_readings(P, (I - 1)).
```

A model asserts `emit_reading_batch(r0)` to export the tree under `r0`; every reading it
reaches then owes one `sensor` and one `temp_c`, and the sequence owes indices dense from 0.
Nothing unreached is constrained, so a model's working predicates stay free, and the guard
carries the sort — `sensor` is shared with `alert`, and each sort's obligations are its own.
This is the **strict** theory, `gen`'s default: an obligation is an integrity constraint, so an
answer set that would not serialize is simply not an answer set — UNSAT, never garbage bytes.
`keryx gen --shape diagnostic` writes `thermal.v1.emit-diagnostic.lp` instead, where each
obligation derives `violates(path, occupant)` and the model survives for the reassembler to
report the field path; `--shape both` writes both. The manifest's header records the choice
(`shape strict`).

### `thermal.v1.keryx-manifest` — the evolution contract (spec §13.4)

The number↔name binding: every proto path and field number, and the emitted predicate,
arity, and shape it maps to.

```
thermal.v1.AlertSet.alerts #1 rel  alerts/2 -> alert  alert  set
```

The `rel` kind and `set` descriptor mark `alerts/2` a `(keryx.set)` membership relation (spec
§7.1) — arity 2, and no `; view` clause, a set having no `views.lp` projection. This is the
contract a later revision of the schema is checked against — the record of what each element
*became*, so a rename, a renumber, or a treatment change (a sequence becoming a set, say) is a
visible, reviewable diff rather than a silent break — the check `keryx diff` performs (see the [evolution](../evolution/) example).

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

## Reassembling a payload — `keryx emit`

The outbound half runs an answer set back to a payload. [`answer.lp`](answer.lp) is the seven
facts above plus the one marker a model asserts to export a tree — `emit_reading_batch(r0)` —
and `keryx emit` reassembles the message it names:

```sh
keryx emit --root ReadingBatch answer.lp thermal.proto -I . > batch.reassembled.binpb
```

The bytes are [`batch.reassembled.binpb`](batch.reassembled.binpb) — byte-for-byte
[`batch.binpb`](batch.binpb) again. The round trip closes: the payload shreds to the seven
facts, the marker exports them, and the reassembler rebuilds the very payload, canonical (a
zero implicit scalar is omitted, as proto3 encodes it). `--out txtpb` and `--out json` write the
same message in the other two forms; `--root ReadingBatch` names which message to write when the
answer set could name several, since exactly one reaches stdout.

A real clingo answer set carries emit.lp's derived atoms too — `reach/1`, the `has_` witnesses,
and, under the diagnostic theory, `violates(path, occupant)`. The reassembler keys only on the
sort, field, and marker atoms, reading `violates` where the diagnostic theory left it; a model's
own working predicates and the theory's scaffolding pass through untouched.

## The solver-free path

`gen`, `facts`, and `emit` are the bridge itself. The whole round trip is:

1. **`keryx gen`** — schema → the ASP vocabulary above (this example).
2. **`keryx facts`** — a `ReadingBatch` payload → ground facts over that vocabulary (this
   example).
3. **your clingo** — the facts plus your own model (constraints, derivations) → an answer set.
   keryx invokes no solver; the solver is yours.
4. **`keryx emit`** — an answer set → an outbound protobuf payload (this example — the
   `ReadingBatch` round trip above).

Everything keryx does here is a pure, deterministic function of its input — no solver, no
network, golden-comparable. keryx wires steps 1, 2, and 4; step 3 is your solver's, over the
vocabulary keryx generates. Only the transient *solve* is external — keryx translates, never
solves — and this example runs the whole of keryx's bridge around it.

## Scope at this stage

- **Binary, textproto, and JSON payloads.** `facts` reads the binary wire format (`.binpb`), the
  protobuf text format (`.txtpb`), and the protobuf JSON mapping (`.json`) — every payload form
  spec §26 names — nesting in each bounded at the same ceiling: text ahead of its parser, JSON
  beneath its deserializer's own count.
- **`(keryx.set)` is honored.** keryx reads the annotation and generates `AlertSet.alerts` as a
  **membership relation** `alerts/2` — no sequence view, order- and multiplicity-insensitive (spec
  §7.1) — not the sequence `ReadingBatch.readings` is. A model computing a set names its members
  functionally by their own provenance (`alert(al(R))` occupancy, `alerts(out, al(R))` membership —
  the natural `overheating` shape, not the dense indices a sequence needs), and `emit` closes the
  `AlertSet` round trip byte-for-byte, its members serialized in clingo's total symbol order
  (identical answer set ⇒ identical bytes). A message set keeps distinct occupants for equal
  payloads — a multiset — until element-content collapse under `(keryx.value)`.
- **`emit.lp` is generated and reassembly is real.** The theory above is what `gen` writes today,
  and the repository's grounding gate runs it under clingo — the facts of `batch.binpb`, exported
  under `emit_reading_batch(r0)`, satisfy it, and a second `sensor` on a reading refutes it.
  `keryx emit` turns an answer set satisfying it back into a message — the `ReadingBatch` round
  trip above closes byte-for-byte, in the binary form and, the same way, the text and JSON forms.
