# keryx documentation

The manual lives in [`book/`](book/) and is built with [mdBook](https://rust-lang.github.io/mdBook/); `book.toml` at the repository root configures the build. Read it as rendered pages on the documentation site, or browse the Markdown sources here.

The design of record — the dated, load-bearing documents the manual is built over — is kept alongside it rather than inside the book:

- [`specification.md`](specification.md) — the founding specification.
- [`design/architecture.md`](design/architecture.md) — the architecture of record, which reconciles the specification to what is built.
- [`design/threat-model.md`](design/threat-model.md) — the threat model of record.
- [`proto-support.md`](proto-support.md) — the Protocol Buffer support matrix.

To build the book locally, see [`book/building.md`](book/building.md).
