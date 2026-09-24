# The enum example — an enum in ASP (`keryx gen`, `keryx facts`, §7.4)

How keryx renders a Protocol Buffers **enum** into Answer Set Programming: a proto3 open
enum, the sort and constants `keryx gen` mints from it, and a payload `keryx facts` shreds
to atoms that name those constants — *shredding* is keryx's word for breaking a message
down into flat, per-field ground facts. This is the proto→asp direction — the half a
hand-rolled shim usually gets *almost* right.

## The schema

[`signals.proto`](signals.proto):

```proto
syntax = "proto3";
package signals.v1;

// A traffic signal's phase — a proto3 (open) enum: §7.4 maps each value to a constant.
enum Phase { PHASE_UNSPECIFIED = 0; PHASE_GO = 1; PHASE_CAUTION = 2; PHASE_STOP = 3; }

message Light    { string intersection = 1; Phase phase = 2; }
message Corridor { repeated Light lights = 1; }
```

## Generate the vocabulary, shred a payload

```sh
keryx gen signals.proto -I . -o gen/
keryx facts --root Corridor=corridor.txtpb signals.proto -I .
```

## The enum is a sort; each value is a constant

`gen` writes the enum into [`gen/signals.v1.core.lp`](gen/signals.v1.core.lp) as a sort of
its own, and each field of that type as a total function into it:

```prolog
%! enum phase/1  (open)
#defined phase/1.
%! phase : light -> phase  (total)
#defined phase/2.
```

`phase/1` is the enum as an **open** sort — a proto3 enum is open, so the sort admits
members beyond the declared ones, which a model may range over. `phase/2` is the field, a
total function from a `light` to a `phase`. One name at two arities, each unambiguous.

Shredding [`corridor.txtpb`](corridor.txtpb) names the constants:

```prolog
phase(lights(r0, 0), go).
phase(lights(r0, 1), stop).
```

`PHASE_GO` lowers to `go` and `PHASE_STOP` to `stop` — §7.4 strips the enum's name prefix
and lowercases the rest. Not the integer `1`, not the string `"PHASE_GO"`: a **constant** a
rule can pattern-match on. (Where a stripped remainder would open with a digit, §7.4 keeps
the unstripped form for the whole enum, so every constant is a legal identifier.)

## The membership the theory keeps, and the manifest that records it

The generated [`emit.lp`](gen/signals.v1.emit.lp) declares the enum's members and, outbound,
holds an emitted value to one of them:

```prolog
ok_phase(caution).  ok_phase(go).  ok_phase(stop).  ok_phase(unspecified).
:- light(P), phase(P, V), reach(P), not ok_phase(V).
```

And the [manifest](gen/signals.v1.keryx-manifest) records the number↔name↔constant binding
— the evolution contract a later schema revision is diffed against:

```
signals.v1.Phase  enum  phase/1  (open)
PHASE_GO  #1  value  go
```

A renamed value or a renumber becomes a reviewable diff, not a silent break.

## An undeclared number is refused

A proto3 enum is open, so a payload may carry a number the schema never declared. keryx
cannot name a constant for such a value, so it **refuses** at the field's path rather than
inventing one — a light carrying phase `99` shreds to a single structured diagnostic:

```
keryx: unknown_enum_value at signals.v1.Light.phase: the value 99 matches no declared
value of the enum `signals.v1.Phase`; an unknown number of an open enum is a translation
error by default (§7.4) — annotate the field `(keryx.unknown) = PRESERVE` to carry it as
`unknown(99)`
```

The refusal is the default; annotating the field `(keryx.unknown) = PRESERVE` carries an
unknown as `unknown(99)` through translation instead, and re-encodes it outbound. Either way, a
hand-rolled shim that maps the raw integer straight through would pass a value it cannot mean —
keryx names the number, the enum, and the field. The regression suite pins both the constant
lowering and this refusal.

## Scope at this stage

This example demonstrates the inbound (proto→asp) direction. The asp→proto (outbound)
direction is complete — the generated `emit.lp` above grounds clean under clingo (the
repository's grounding gate proves it), and the [thermal](../thermal/) and
[config](../config/) examples close the round trip end to end.
