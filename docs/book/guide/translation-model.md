# The translation model

keryx maps Protocol Buffers to Answer Set Programming by a few consistent rules. Knowing them, you can predict the vocabulary any schema generates.

## A message is a sort

Each message type becomes a **sort** — a unary membership predicate. `message Reading` becomes `reading/1`; `reading(x)` says *x is a reading*. `gen` declares it:

```prolog
%! sort reading/1
#defined reading/1.
```

## A scalar or enum field is a base-fact predicate

A scalar or enum field becomes a binary predicate over its sort: `string sensor` in `Reading` becomes `sensor/2`, a function from a `reading` to a string.

```prolog
%! sensor : reading -> string  (total)
#defined sensor/2.
```

**Polymorphism-by-sort.** When a field name is shared across messages — `sensor` in both `Reading` and `Alert` — keryx declares the predicate *once*, its meaning fixed by the sort of the first argument. `sensor/2` carries both signatures; it is not mangled into `reading_sensor` and `alert_sensor`. Shared names cohere instead of colliding.

## A message-typed field is an occupant term with a view

A message-typed field has no base predicate. Its occupant is addressed by *where it sits* — a canonical **access-path term**, `readings(parent, index)`, not an identity minted per run. A nested reading is `readings(r0, 0)`, and `reading(readings(r0, 0))` says that occupant is a reading.

So that a model can range over such occupants relationally, `views.lp` derives a **view** — one rule per message-typed field:

```prolog
readings(P, I, E) :- reading(E), E = readings(P, I).
```

`views.lp` opens by including `core.lp`, so it is loadable on its own; a project that wants only the functional canon can exclude it.

## Canonical identity, not minted handles

Because occupants are access-path terms, two runs over the same message produce the same facts, and a model joins on stable, meaningful terms — the position in the tree — rather than opaque ids it must first correlate. This is what makes an answer set reassemblable: the term *is* the address.

## Presence and totality come from resolved features

Whether a field is total (always present), partial (optional), or required is read from the schema's *resolved features*, never from the syntax era — so proto2 and proto3 sit on one uniform surface. A field's totality drives both the `%!` signature and the outbound obligation ([Answer sets to messages](outbound.md)).

The [`enum`](https://github.com/GregoryGelfond/keryx/tree/main/examples/enum), [`oneof`](https://github.com/GregoryGelfond/keryx/tree/main/examples/oneof), [`map`](https://github.com/GregoryGelfond/keryx/tree/main/examples/map), and [`proto2`](https://github.com/GregoryGelfond/keryx/tree/main/examples/proto2) examples each show one construct end to end.
