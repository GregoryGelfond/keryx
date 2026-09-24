# The proto2 example — proto2 in ASP (`keryx gen`, `keryx facts`, §5, §7.4)

keryx ingests **proto2** as well as proto3. This example compiles a proto2 schema, shreds
(breaks a message down into flat, per-field ground facts) a payload over it, and shows how
proto2's presence labels and its **closed** enum render — the same vocabulary shape a proto3
schema produces, so a codebase mixing proto2 and proto3 reasons over one uniform surface.

## The schema

[`order.proto`](order.proto):

```proto
syntax = "proto2";
package orders.v1;

message Order {
  required string id = 1;      // presence: always
  optional int32 quantity = 2; // presence: maybe
  repeated string tags = 3;    // a sequence
  optional Grade grade = 4;    // a closed enum
}
enum Grade { GRADE_UNSPECIFIED = 0; GRADE_A = 1; GRADE_B = 2; }
message Ledger { repeated Order orders = 1; }
```

## Generate the vocabulary, shred a payload

```sh
keryx gen order.proto -I . -o gen/
keryx facts --root Ledger=ledger.txtpb order.proto -I .
```

## Presence, and a closed enum

The field declarations in [`gen/orders.v1.core.lp`](gen/orders.v1.core.lp):

```prolog
%! enum grade/1  (closed)
#defined grade/1.
%! grade : order -> grade  (partial)
#defined grade/2.
%! id : order -> string  (required)
#defined id/2.
%! quantity : order -> int32  (partial)
#defined quantity/2.
%! tags : order × index -> string  (sequence)
#defined tags/3.
```

- **A closed enum.** `grade/1` is marked `(closed)` — a proto2 enum is closed, so a number it
  does not declare is invalid (contrast the proto3 [`enum`](../enum/) example, whose `phase/1`
  is `(open)`). The membership `emit.lp` keeps is exact.
- **`repeated` is a sequence** (`tags/3`) and **`optional` is partial** (`quantity/2`), exactly
  as in proto3.

### `required` presence, and the outbound obligation

`id` is `required` in proto2 — spec §5 explicit presence, like an `optional` field, but with a
must-be-present contract. keryx marks that with a totality of its own: the `(required)` label,
distinct from both the `(partial)` of `optional` and the `(total)` of a proto3 implicit-presence
scalar. The contract is enforced **outbound**, in `emit.lp` (the mapping carries
`Totality::Required`): an answer set that omits a `required` field is refused — UNSAT under the
generated theory (`:- order(P), reach(P), not has_id(P).`) and rejected at reassembly
(`ShapeViolation`) alike — full proto2/proto3 outbound parity. This inbound example is
unaffected: a `required` field is always on the wire, so `id` is present for both orders above.

## The facts

Shredding [`ledger.txtpb`](ledger.txtpb) — two orders, the second omitting `quantity` and
`grade`:

```prolog
grade(orders(r0, 0), a).
id(orders(r0, 0), "PO-1001").
id(orders(r0, 1), "PO-1002").
ledger(r0).
order(orders(r0, 0)).
order(orders(r0, 1)).
quantity(orders(r0, 0), 5).
tags(orders(r0, 0), 0, "rush").
tags(orders(r0, 0), 1, "fragile").
tags(orders(r0, 1), 0, "bulk").
```

`id` is present for both orders; `quantity` only for the one that set it (partial presence);
`tags` is an indexed sequence; the closed enum value `GRADE_A` lowers to the constant `a`.

## Scope at this stage

This example demonstrates the inbound (proto→asp) direction. The asp→proto (outbound)
direction is complete, including the proto2 `required` totality obligation (full proto2/proto3
parity); the generated `emit.lp` above grounds clean under clingo (the repository's grounding
gate proves it), and the [thermal](../thermal/) and [config](../config/) examples close the
round trip end to end.
