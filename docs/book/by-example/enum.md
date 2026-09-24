# An enum in ASP

How keryx renders a Protocol Buffers **enum** into Answer Set Programming: a proto3 open enum, the sort and constants `keryx gen` mints from it, and a payload `keryx facts` shreds to atoms that name those constants.

The runnable example is in [`examples/enum/`](https://github.com/GregoryGelfond/keryx/tree/main/examples/enum).

## The schema

```proto
syntax = "proto3";
package signals.v1;

// A traffic signal's phase — a proto3 (open) enum: each value maps to a constant.
enum Phase { PHASE_UNSPECIFIED = 0; PHASE_GO = 1; PHASE_CAUTION = 2; PHASE_STOP = 3; }

message Light    { string intersection = 1; Phase phase = 2; }
message Corridor { repeated Light lights = 1; }
```

## The enum is a sort; each value is a constant

`gen` writes the enum as a sort of its own, and each field of that type as a total function into it:

```prolog
%! enum phase/1  (open)
#defined phase/1.
%! phase : light -> phase  (total)
#defined phase/2.
```

`phase/1` is the enum as an **open** sort — a proto3 enum is open, so the sort admits members beyond the declared ones, which a model may range over. `phase/2` is the field, a total function from a `light` to a `phase`.

Shredding a payload names the constants:

```prolog
phase(lights(r0, 0), go).
phase(lights(r0, 1), stop).
```

`PHASE_GO` lowers to `go` and `PHASE_STOP` to `stop` — the enum's name prefix is stripped and the rest lowercased. Not the integer `1`, not the string `"PHASE_GO"`: a **constant** a rule can pattern-match on. (Where a stripped remainder would open with a digit, keryx keeps the unstripped form for the whole enum, so every constant is a legal identifier.)

## The membership the theory keeps

Outbound, `emit.lp` declares the enum's members and holds an emitted value to one of them:

```prolog
ok_phase(caution).  ok_phase(go).  ok_phase(stop).  ok_phase(unspecified).
:- light(P), phase(P, V), reach(P), not ok_phase(V).
```

The manifest records the number↔name↔constant binding, so a renamed value or a renumber becomes a reviewable diff, not a silent break.

## An undeclared number is refused

A proto3 enum is open, so a payload may carry a number the schema never declared. keryx cannot name a constant for such a value, so it **refuses** at the field's path rather than inventing one — a light carrying phase `99` shreds to a single structured diagnostic:

```text
keryx: unknown_enum_value at signals.v1.Light.phase: the value 99 matches no declared
value of the enum `signals.v1.Phase`; an unknown number of an open enum is a translation
error by default (§7.4) — annotate the field `(keryx.unknown) = PRESERVE` to carry it as
`unknown(99)`
```

The refusal is the default; annotating the field `(keryx.unknown) = PRESERVE` carries an unknown as `unknown(99)` through translation instead, and re-encodes it outbound (see [Annotating the mapping](../guide/annotations.md)). Either way, a hand-rolled shim that maps the raw integer straight through would pass a value it cannot mean — keryx names the number, the enum, and the field.
