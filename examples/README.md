# keryx by example

κῆρυξ, *herald* — a bidirectional bridge between Protocol Buffers and Answer Set Programming.
These worked examples are the fastest way to see what that means. Each is self-contained and
runnable, and each is also part of keryx's test suite: the vocabulary and facts shown below are
regenerated and checked on every commit, and the generated theories are grounded under clingo.

## The motion

keryx turns a **schema** into an ASP *vocabulary*, a **message** into ground *facts* over that
vocabulary — and an *answer set* back into a message:

```
message ──keryx facts──▶ facts (proto→asp) ──your solver──▶ answer set ──keryx emit──▶ message
                                            (keryx runs none)
```

- **`keryx gen`** compiles a `.proto` into the vocabulary a model is written against.
- **`keryx facts`** *shreds* a payload — breaks it down into flat, per-field ground facts.
- **`keryx emit`** *reassembles* an answer set back into a payload.

**keryx never runs a solver.** The solve between `facts` and `emit` is yours (clingo today), over
the vocabulary keryx generated. keryx is the communication glue on both ends — nothing more.

## Why not a hand-rolled shim?

Turning a message into atoms is easy to do badly. keryx does the parts a one-off converter usually
gets wrong:

| It gives you | so that | shown in |
|---|---|---|
| **Canonical identity** — nested values are access-path terms (`readings(r0, i)`), not opaque generated ids | a model joins on stable, meaningful terms | [thermal](thermal/), [map](map/) |
| **Polymorphism-by-sort** — one predicate serves every sort that declares the field | shared field names don't collide or get mangled | [thermal](thermal/) |
| **A declared vocabulary** — `#defined` with readable signatures | a model is written *against* a contract, with no "atom does not occur" warnings | every example |
| **Faithful type coverage** — enum, oneof, map, nested, proto2/proto3, three wire formats | your whole schema translates, not the easy 80% | [enum](enum/), [oneof](oneof/), [map](map/), [proto2](proto2/) |
| **A bidirectional round trip** — answer sets back to canonical bytes | you get protobuf *out*, not just in | [thermal](thermal/), [config](config/) |
| **Checked serializability** — an answer set that isn't a message is UNSAT, or diagnosed at the field path | you never emit garbage bytes | [thermal](thermal/), [config](config/) |
| **Evolution as a contract** — a manifest binds every number, name, and predicate | a rename or renumber is a reviewable diff, not a silent break | every example |

## The examples, in reading order

1. **[thermal](thermal/)** — the on-ramp. The whole motion end to end: a batch of sensor readings
   → facts → back to bytes. Polymorphism-by-sort and canonical identity in one place.
2. **[config](config/)** — *solve a problem, get protobuf back.* A `Deployment` in, your clingo
   validator in the middle, a `Report` out — a different message, computed. The point of the bridge.
3. **[enum](enum/)** — a proto3 enum → constants (§7.4), with the undeclared-value refusal a shim
   skips.
4. **[oneof](oneof/)** — a `oneof` → partial arms; only the present one shreds (§7.3).
5. **[map](map/)** — a `map<k, v>` → a family keyed by the map key, plus cross-package identity
   (§7.2, §4.1).
6. **[proto2](proto2/)** — proto2 alongside proto3: a closed enum and the presence labels, on one
   uniform surface (§5).

## What's built, and what's landing

- **Built:** the compiler (`keryx gen`, `keryx explain`) and the inbound codec (`keryx facts` —
  binary, textproto, and JSON payloads). Every example's proto→asp half runs today.
- **Being finalized:** the outbound codec (`keryx emit`, asp→proto). The thermal and config
  examples demonstrate the round trip; these docs will be extended as it lands.
- **Not yet:** set-valued fields (`(keryx.set)`) and Protocol Buffers editions.

See the [top-level README](../README.md) for the project overview and
[`docs/specification.md`](../docs/specification.md) for the full design.
