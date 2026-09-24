# Working on keryx

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) before making changes. It is the authoritative contributor guide; this file is a concise entry point for coding agents. Apply the same standards to implementation, tests, and documentation.

## Find the relevant boundary

Repository knowledge is in the manual and the design of record, not a directory tour:

- The bridge, end to end: [`examples/`](examples/) and the book's guided tour ([`docs/book/guide/tour.md`](docs/book/guide/tour.md)).
- The translation model, the payload forms, the outbound theory, annotations, and evolution: the book's Part I.
- The library surface — `Mapping`, `Codec`, diagnostics, the fault boundary: the book's Part II.
- The crate map and error posture: [`docs/design/architecture.md`](docs/design/architecture.md) §3 and §6.
- The input surfaces and what is defended: [`docs/design/threat-model.md`](docs/design/threat-model.md), "The doors".
- Version and format support: [`docs/proto-support.md`](docs/proto-support.md).

## Preserve the translation contract

- **keryx never solves** and defines no solver backend; a consuming tool runs the oracle and composes keryx's translation around it.
- **keryx parses no Protocol Buffers or ASP itself**, beyond two bounded, non-semantic nesting guards (a `.proto` and a textproto measure). A `FileDescriptorSet` is the schema interface; answer-set text is read through the ASP layer.
- The inbound and outbound models are symmetric, and **proto2 and proto3 reach parity on both ends** (a proto2 `required` field carries its outbound totality obligation).
- The uniform nesting ceiling is **99** message-typed levels below the root, reached the same way in every payload form.
- The themelios dependency is arm's-length: **never modify it**; close a gap in themelios and adopt it by a deliberate, reviewed pin bump.

## Make the correctness argument readable

- Library first, the CLI thin: a capability lives in `keryx-core` and the `keryx` command is a view over it.
- Every foreign input returns a typed diagnostic at a field-path locus, never a panic or a bare string. One concept, one name (the book's Vocabulary is the registry).
- Resolve every rustfmt, pedantic Clippy, and rustdoc diagnostic in the code. Never suppress `dead_code`, `unused`, or a warning with `allow` or `expect`, except a narrow, reasoned `expect` for a foreign-API false positive.
- One test name states one proposition. Generated ASP is constructed as syntax values and rendered, never string-templated.

## Validate the affected contract

Run the gate through the single entry point before pushing — hosted CI is not running it (the Actions quota is exhausted), so the local gate is the gate:

| mode | scope |
|---|---|
| `scripts/check.sh portable` | fmt, clippy (`-D warnings`), test, doc (`-D warnings`) |
| `scripts/check.sh coverage` | line coverage, floor 85 |
| `scripts/check.sh ground` | the clingo grounding gate |
| `scripts/check.sh book` | build the mdBook manual |
| `scripts/check.sh full` | all of the above |

Report the checks you actually ran. Never weaken a gate to accommodate a change.

## Keep public documentation current

Update the manual, the worked examples and their goldens (`examples/*/gen/`), the manifest, and `docs/proto-support.md` with the implementation they describe. Keep development diaries, session notes, and machine-local paths out of the tree — documentation must be intelligible without them.
