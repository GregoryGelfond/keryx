# Answer sets to messages

The outbound half of the bridge runs an answer set back to a message. It has two pieces: the theory `gen` emits so that only a *serializable* answer set is admitted, and `keryx emit`, which reassembles the message.

## `emit.lp` — the serializability theory

Inbound, the wire guaranteed a message's invariants (a singular field appears once; a sequence is dense from zero). Outbound, those become *obligations* an answer set must meet before it can be a message. `gen` writes them into `emit.lp`, a client of `core.lp` that declares a root marker per message sort, closes `reach/1` over the message-typed slots beneath each asserted marker, and guards every obligation by `reach` and by sort:

```prolog
#include "thermal.v1.core.lp".
reach(X) :- emit_reading_batch(X).
%! functionality of sensor : reading -> string  (total)
:- reach(P), reading(P), sensor(P, V1), sensor(P, V2), V1 != V2.
```

A model asserts `emit_reading_batch(r0)` to export the tree under `r0`; every reading it reaches then owes one `sensor` and one `temp_c`, and a sequence owes indices dense from zero. Nothing unreached is constrained, so a model's own working predicates stay free.

**Strict or diagnostic.** By default (`--shape strict`) each obligation is an integrity constraint: an answer set that would not serialize is simply not an answer set — UNSAT, never garbage bytes. `--shape diagnostic` writes `emit-diagnostic.lp` instead, where each obligation derives `violates(path, occupant)` and the model survives for the reassembler to report the field path; `--shape both` writes both. The manifest records the choice.

## `keryx emit` — reassembly

`keryx emit` turns an answer set satisfying the theory back into a message:

```sh
keryx emit --root ReadingBatch answer.lp thermal.proto -I . --out binpb > out.binpb
```

The reassembler keys only on the sort, field, and marker atoms (and reads `violates` where the diagnostic theory left it); a model's working predicates and the theory's scaffolding pass through untouched. The result is **byte-for-byte** the payload the facts came from — the round trip closes. `--root Type` names which message to write when the answer set could name several, since exactly one reaches stdout.

## Output forms, and one JSON limit

`emit` writes the reassembled message in the same three forms `facts` reads: `--out binpb`, `--out txtpb`, `--out json`. A map's entries are ordered by key in every form, so an identical answer set yields identical bytes.

Canonical JSON is the one form that cannot represent everything the binary and text forms can: it mandates the resolved, range-validated form of a `Timestamp`, `Duration`, or `Any`, with no raw fallback. keryx keeps well-known types opaque and structural on both ends and does not resolve one at reassembly, so a value canonical JSON cannot represent — an out-of-range timestamp, an unresolvable `Any` — is refused at the JSON encode with `UnrepresentableJson`, naming the forms that carry it (`--out binpb`/`txtpb`), rather than emitted as a non-conforming document. Each form refuses only what it alone cannot represent; the round-trip parity is exact for every message all three forms can carry.
