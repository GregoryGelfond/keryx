# Contributing to keryx

Start with the worked examples in [`examples/`](examples/) — they show the whole bridge end to end — then the architecture of record, [`docs/design/architecture.md`](docs/design/architecture.md), over the founding specification, [`docs/specification.md`](docs/specification.md). The manual in [`docs/book/`](docs/book/) is the reader's path; this file is the standard the work is held to.

keryx is the Protocol Buffers ↔ Answer Set Programming bridge, and only that: it compiles a schema into an ASP vocabulary, translates messages to ground facts, and reassembles answer sets back into messages. It never invokes a solver — a consuming tool runs the oracle and composes keryx's translation on both sides. It is a library first; the `keryx` command is a thin shell over `keryx-core`. The MIT license names Gregory Gelfond.

## Design from the translation's questions

Public operations speak in the domain's terms — schemas, sorts, predicates, occupant terms, facts, answer sets — not in the incidental shapes of the crates beneath them. **No external type appears in keryx's public surface.** The ASP value plane is themelios's `Symbol`, `Name`, and `Sign`, re-exported from `keryx_core` so a consumer names them through keryx alone, never through a direct rev-pinned dependency; the descriptor engine, the `.proto` compiler, and the JSON deserializer are read behind the `descriptor` and `codec` seams, and no `prost-reflect`, `protox`, or `serde_json` type reaches a public signature. A dependency is wrapped so it stays swappable — a boundary is not merely another box in a diagram, it is a type you could change the far side of without a consumer noticing.

The themelios dependency is arm's-length: keryx consumes it, never modifies it, and pins it by git revision so the build is reproducible. A surfaced gap is closed in themelios and adopted by a deliberate dependency bump, never worked around by editing it. When themelios publishes to crates.io, the pin becomes a semver dependency — a one-line change.

`Mapping` is the stable interface between the compiler and everything downstream. `gen` renders it, `explain` reports it, the codec shreds and reassembles against it, and `diff` compares two of them. It is a value, not rendered text: outputs that must cohere — a migration report, a JSON changeset, a rename bridge — are each a pure view of one mapping, so they cannot disagree.

## Make represented knowledge inspectable

A distinction in a type must have a producer and a consumer; a variant nothing constructs, or nothing reads, is a false claim about the design. Every foreign input crosses a `Result` boundary returning a typed diagnostic at a field-path locus — never a panic, never a bare string, never a partial shred left behind. One concept has one name: a message *shreds* to facts and an answer set *reassembles* to a message; a value at a field position is an *occupant*; a boundary that admits external input is a *door*. The book's [Vocabulary](docs/book/vocabulary.md) is that registry — two words for one thing is the same defect as one word for two.

Documentation is held to the same standard as code: it must be intelligible without access to private working records, and it can never lie. Keep development notes, session logs, and machine-local paths out of the tree. A worked example's generated artifacts (`examples/*/gen/`) are golden — regenerated and checked on every run — so the manual, which shows them, cannot drift from the tool.

## Carry the contract into the generated ASP

The generated vocabulary is itself a contract. `core.lp` declares each sort and base-fact predicate with `#defined` and a readable `%!` signature, so a model is written *against* a stated vocabulary and a consumed-only predicate draws no "atom does not occur" warning. The outbound theory `emit.lp` turns the wire's guarantees into obligations: strict, an unserializable answer set is UNSAT; diagnostic, it derives `violates(path, occupant)` for the reassembler to report. The grounding gate — the runner's clingo over the generated theory — is the arbiter that these hold; clingo is test infrastructure, never a keryx dependency.

**All generated ASP is constructed as syntax values and rendered, never string-templated** (spec §18). Emission builds terms, atoms, and rules through the ASP layer and prints them; no keryx output is assembled by hand. A change that would emit text by concatenation is wrong however plausible it looks.

## Verification and review

Every change is held to the gate, green, before it lands. The single entry point is `scripts/check.sh`:

| mode | scope |
|---|---|
| `portable` | `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo test --workspace --locked`; `cargo doc` with `RUSTDOCFLAGS=-D warnings` |
| `coverage` | line coverage, floor **85** (`cargo llvm-cov --workspace --locked --fail-under-lines 85`) |
| `ground` | the clingo grounding gate (`scripts/ground.sh`) |
| `book` | build the mdBook manual (mdBook 0.5.4) |
| `full` | all of the above |

The coverage floor follows themelios's method — measure line coverage, round down to a multiple of five, minus five — raised only deliberately and never lowered. The dependency pins `prost-reflect =0.16.5`, `protox =0.9.1`, and `serde_json =1.0.151` are exact because keryx's door hardening is a statement about those crates' internals at exactly those versions; the pre-emption catalogue (`docs/design/threat-model.md`) is re-owed on any deliberate bump. Never suppress `dead_code`, `unused`, or a warning, whether with `allow` or `expect`; a narrow `expect` is admissible only where an invariant the code discharges makes a foreign-API lint a false positive, with the reason beside it. One test name states one proposition — fifty characters is a review cue, not a limit. Cancellation and exhaustion must never masquerade as a translation result.

**Proposing a change.** Branch off `main`; `main` is protected and lands only by pull request, rebase-merged (`gh pr merge --rebase`) to keep history linear. Run `scripts/check.sh full` locally before you push — hosted CI is not running the gate on every push (the Actions quota is exhausted; `.github/workflows/checks.yml` is retained as documented intent, `workflow_dispatch`-only), so the local gate is the gate. Commit subjects are plain and imperative ("Add the keryx diff command", not "feat:"). Keep per-person tooling out of the tree; the repository does not list it.

## Repository presentation

The GitHub description, topics, badges, and release metadata state what is true and no more. Badges are static and honest: license, Rust version, and a coverage figure measured locally by `scripts/check.sh coverage` (its floor and method are under *Verification and review* above). There is **no CI badge** — a CI badge must identify an active workflow, and keryx's is paused. A release tag corresponds to what was released; historical or archival tags are not releases.
