# keryx

κῆρυξ, *herald* — a bidirectional bridge between Protocol Buffers and Answer Set Programming.

keryx compiles a `.proto` schema into an ASP vocabulary and translates messages
into ground facts — and answer sets back into messages. The message side never
learns ASP; the model side never learns the wire.

keryx doesn't solve — bring your own solver (clingo today).

## Status

Founding. The compiler, codec, and CLI are under construction; a worked
`examples/` walkthrough lands with the first end-to-end path. See
[`docs/design/architecture.md`](docs/design/architecture.md) for the build plan
and [`docs/specification.md`](docs/specification.md) for the full design.

## Built on

[themelios](https://github.com/GregoryGelfond/themelios) — the ASP program
representation and, ahead, the solver backend. keryx is also the foundation for
pythia, a mission-critical ASP-solver-as-a-service.

## License

MIT. See [LICENSE](LICENSE).
