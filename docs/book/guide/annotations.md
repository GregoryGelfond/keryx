# Annotating the mapping

Most fields need no annotation — keryx's defaults lower a schema faithfully. Where a field or enum needs a *precise* ASP treatment, an annotation gives it one. Each is a custom Protocol Buffers option under `keryx/options.proto` (which keryx resolves from an embedded registry — no `-I` for it), and each is validated at the policy door: a mis-targeted or malformed annotation is a structured diagnostic, never a silent mis-lowering.

## `(keryx.set)` — membership, not sequence

A repeated field whose order is incidental is a *set*, not a sequence. `(keryx.set) = true` generates it as a membership relation — an arity-`n+1` base relation a model asserts, order- and multiplicity-insensitive — rather than the indexed sequence a plain `repeated` field becomes. It gets no `views.lp` projection (there is no occupancy to project), and on the outbound side a set's members serialize in the solver's total symbol order, so an identical answer set yields identical bytes.

```proto
message AlertSet { repeated Alert alerts = 1 [(keryx.set) = true]; }
```

## `(keryx.numeric)` — integer width

Protocol Buffers has 64-bit integers that ASP's numbers do not natively carry. `(keryx.numeric)` names how a field's integer is represented: `NATIVE_CHECKED` keeps it a native ASP number with its range checked, `DECIMAL_STRING` carries it as a decimal string where the range would overflow.

## `(keryx.scale)` and `(keryx.opaque)` — floats

Floating-point fields need an explicit treatment. `(keryx.scale)` fixes a decimal scale (a value off the scale is refused, `ValueNotOnScale`; a non-finite value, `NonFiniteFloat`); `(keryx.opaque)` carries a `float`/`double` opaquely. A float field with neither is a mandatory-annotation error with a fix-it — keryx will not silently guess a float's ASP form.

## `(keryx.unknown)` — open-enum unknown values

A proto3 enum is *open*: a wire value naming no declared constant is legal. `(keryx.unknown) = PRESERVE` carries such a value through translation — inbound, an unknown number `N` shreds to `unknown(N)`; outbound, `unknown(N)` re-encodes to `N` — rather than refusing it (`UnknownEnumValue`) as the default does. A proto2 enum is closed, so `PRESERVE` is inapplicable there (a mis-target diagnostic at the policy door).

## Presence across the eras

A proto2 `required` field carries its outbound totality obligation: `emit.lp` obliges it exactly as an implicit field, and the reassembler refuses an answer set that omits it. proto3 has no `required`; proto2's is enforced outbound — full proto2/proto3 parity, for interconnect preservation and inbound↔outbound symmetry.

See the [`enum`](https://github.com/GregoryGelfond/keryx/tree/main/examples/enum) and [`oneof`](https://github.com/GregoryGelfond/keryx/tree/main/examples/oneof) examples for enums and oneofs end to end.
