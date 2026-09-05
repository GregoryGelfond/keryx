# keryx

κῆρυξ, *herald* — a bidirectional bridge between Protocol Buffers and Answer Set Programming.

keryx compiles a `.proto` schema into an ASP vocabulary and translates messages
into ground facts — and answer sets back into messages. The message side never
learns ASP; the model side never learns the wire.

keryx doesn't solve — bring your own solver (clingo today).

## What it looks like

A message becomes a **sort**; its scalar fields become **predicates** over that
sort; a message-typed field becomes an occupant term with a relational **view**
to join on.

```proto
message Reading { string sensor = 1; int32 temp_c = 2; }
message Batch   { repeated Reading readings = 1; }
```

**`keryx gen`** compiles the schema into the vocabulary a model is written against.
`core.lp` declares the sorts and base-fact field predicates with `#defined`,
carrying the readable signature as a `%!` doc comment; `views.lp` adds a
relational view rule per message-typed field:

```prolog
%!sort batch/1
%!readings : batch × index -> reading  (sequence)
#defined batch/1.
%!sort reading/1
#defined reading/1.
%!sensor : reading -> string  (total)
#defined sensor/2.
%!temp_c : reading -> int32  (total)
#defined temp_c/2.

readings(P, I, E) :- reading(E), E = readings(P, I).   % views.lp
```

Then the bridge runs. A **`Batch` message becomes ground facts** over that
vocabulary — each nested reading is the access-path term `readings(b, i)`, not a
minted handle:

```prolog
batch(b).
reading(readings(b,0)).  sensor(readings(b,0),"s1").  temp_c(readings(b,0),20).
reading(readings(b,1)).  sensor(readings(b,1),"s2").  temp_c(readings(b,1),105).
```

Those are the base facts. Loaded together with `views.lp`, the view rule *derives*
the `readings/3` relation, which a client's own model joins on — e.g. flagging an
overheating reading:

```prolog
hot(B, R) :- readings(B, _, R), temp_c(R, T), T > 100.   % ⊢ hot(b, readings(b,1))
```

Solve that with **your own clingo**, and keryx turns the answer set **back into a
`Batch`**. keryx never runs the solver. `gen` and `facts` — the schema-to-vocabulary and
payload-to-facts halves above — are real today; the reassembly that turns the answer set
back into a `Batch` arrives with the first end-to-end path.

## Status

Under construction. The compiler (`keryx gen`, `keryx explain`) and the inbound codec
(`keryx facts`, binary, textproto, and JSON payloads) are built; outbound reassembly,
annotations, and `.lp` admission follow. The worked [`examples/thermal`](examples/thermal/)
walkthrough shows the built half — schema to vocabulary, payload to facts — and completes
with the first end-to-end path. See
[`docs/design/architecture.md`](docs/design/architecture.md) for the build plan
and [`docs/specification.md`](docs/specification.md) for the full design.

## Built on

[themelios](https://github.com/GregoryGelfond/themelios) — the ASP program
representation keryx builds on. keryx is a translation library: a consuming tool
composes it around its own solver, using keryx for the protobuf↔ASP bridge on
both sides.

## License

MIT. See [LICENSE](LICENSE).
