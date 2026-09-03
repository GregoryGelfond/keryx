# keryx — Founding Architecture

**Date:** 2026-08-31 (founding; revised through the gen stage)
**Status:** The architecture of record for keryx; the keryx specification (`docs/specification.md`) is the design of record beneath it.
**Design of record:** `docs/specification.md`; this architecture governs where the two differ (§2, the deltas-from-the-spec table).
**Dependency:** themelios @ `86c7dfb` (public: `https://github.com/GregoryGelfond/themelios`) — program + analysis tiers built, validated, and consolidated for keryx.
**North star:** enabling a consuming tool — a mission-critical ASP-solver service — over keryx's translation.

This document fixes the founding **architecture** and the **build spine**. It is not a per-milestone design; each increment gets its own implementation plan.

---

## 1. Identity

**keryx is a bidirectional bridge between Protocol Buffers and Answer Set Programming** — a schema→vocabulary compiler and a message⇄facts codec.

Protocol Buffers is the **data-interchange / wire format** by which networked systems describe and exchange structured data. Answer Set Programming is a **declarative reasoning** formalism whose solvers compute answer sets (stable models). keryx compiles a `.proto` schema into an ASP vocabulary and its supporting theory, translates messages into ground facts over that vocabulary, and translates answer sets back into messages — so data that crosses the wire as protobuf can be reasoned over by an ASP solver and the results returned in the same form. The message side never learns ASP; the model side never learns the wire.

This is **the mandate**, and it is the whole reason keryx does not invoke a solver: the message side ↔ the model side is keryx's entire concern; solving is the solver's, not the bridge's. Not-solving is a consequence of the mandate, not an incidental design choice.

**What keryx is:** the translation layer — schema→vocabulary compilation (the ASP predicate specification model-writers reason over), the message→facts ingest, and the answer-set→message reassembly, over one shared vocabulary (spec P2).

**What keryx is not:**
- **keryx does not solve — by mandate, not omission.** Solving is not the bridge's concern. keryx links no `libclingo`, spawns no `clingo`, and invokes no concrete solver — at run time or compile time. It is the **translation** between protobuf and ASP, in both directions; a consuming tool (in any language) invokes the solver over keryx's vocabulary and composes keryx's `facts`/reassembly around it. keryx therefore defines no solver backend and never depends on themelios-solve.
- **keryx does not do stateful composition.** Sessions, episodes, profiles (ring / horizon), selection, retention, blame — the runtime orchestration of a served oracle — belong to the consuming tool, built *on* keryx.

keryx is designed and justified as a **standalone tool** (spec §1) and, in the same stroke, as the **enabler for the consuming tool** and the **first-consumer checkpoint** for themelios's program + analysis tiers.

---

## 2. Reconciliation ledger — the spec, brought to today

The keryx spec was written when aspis (not themelios-solve) was the backend and themelios's surface was not yet consolidated or blessed as the sole provider. This design reconciles it with three facts now true: **themelios is ready and blessed as the sole emission/analysis provider**, **keryx is translation-only — solving belongs to the consuming tool, not keryx** (so no solver backend, no themelios-solve dependency), and **the library/service split** (a consuming tool runs the oracle; keryx translates). The deltas from the spec, each justified:

| # | Delta | Supersedes | Why |
|---|---|---|---|
| R1 | **`Sym = themelios::Symbol`** — no keryx `Sym` enum | spec §22 (own `Sym` value type) | themelios's `Symbol` *is* the value type; its `ToSymbol`/`FromSymbol` coverage (`{i8,i16,i32,u8,u16,str,String}`, excluding `u32,i64,u64,bool,f64`) **is** the §6 scalar policy, type-enforced. `Symbol::Ord` = gringo order = the canonical-bytes anchor. |
| R2 | **Emission is a keryx `emit` module directly over themelios `construct`/`render`** | spec §18 (swappable builder/printer trait + internal fallback AST) | Both motivations for the trait are resolved: ship-before-themelios (themelios is ready) and provider independence vs. a rival syntax effort (OQ-9 — themelios is the blessed sole provider). One provider, no speculative indirection. |
| R3 | **Stage-1 mapping policy is computed in Rust** | spec §21.3 (ASP program as the production mechanism) | keryx invokes no solver (R4); an ASP-as-production policy would need one. The ASP formulation survives as an *optional inspectable / cross-check co-artifact* (renderable by `explain`, checkable via ASP contracts under the user's clingo, plus the §21.2 self-application) — the inspection/verification intent preserved without keryx solving. If the co-artifact ships, the Rust policy is sole authority and the ASP form is a *view* generated from the same mapping model (so it cannot disagree) or held to agreement by a gate — never an independently-authored second model. |
| R4 | **keryx invokes no solver and defines no solver backend** — hence no `keryx-driver`; the workspace is `keryx-core` (library), `keryx-cli` (tool), `keryx-protoc` (plugin). A consuming tool (in any language) invokes the solver and composes keryx's translation — `facts` (message → `.lp`), reassembly (answer-set → message) — around it. | **Supersedes:** §17's backend-injection production path; §19's `keryx-driver` row, `protoc-gen-keryx`-crate row, and `PolicyEval` paragraph; §23 entire; §25's `keryx solve`; §26's solve-path envelope and domain-UNSAT exit class; §27's driver-run fixture harness; §31's M2 solve half and M5; §33's lazy-grounding note; Appendix B's `SolveResponse`/`Model`; Appendix D's episode. (§18's aspis and §22's driver mapping are already R2's and R1's.) | Sessions, episodes, ring/horizon, the domain model, and the solver call are how a tool *runs* an oracle over keryx's vocabulary; keryx provides only the bidirectional translation. |
| R5 | **keryx parses no protobuf** — protox produces `FileDescriptorSet`, prost-reflect reads it | (affirms spec P9, §20) | `FileDescriptorSet` is the interface; the dynamic layer (prost-reflect) is mandatory because keryx's annotations are custom options that typed `prost` structs silently drop (§20, load-bearing). Symmetry: keryx parses nothing — protobuf via protox/prost-reflect, ASP text via themelios `raise`. (prost/prost-types decode the set once to read its `syntax` for editions before the engine — the decoded struct is discarded, so no typed struct feeds the schema, §18/§20.) |
| R6 | **G7's "no text on the production path" is the library seam's, not the CLI's** | spec §17, G7, P10 | The library delivers inbound facts as a `Vec<Symbol>` — no text, no parse, no grounding pass — which a consuming tool feeds its solver directly; the CLI delivers them as a `.lp` text module, which the tool's own solver parses and grounds. G7 was written for a solve path keryx no longer owns: a consumer needing the text-free path links `keryx-core` and passes symbols; a CLI consumer accepts the text seam. keryx realises "ground by construction" (P10) on both; the "no text" half (G7) is the library seam's. |

---

## 3. Crate architecture

A Cargo workspace at `~/Projects/keryx`, centered on *translation, not solving*:

```
keryx/                     bidirectional bridge: Protocol Buffers ⇄ ASP ; solving is external
├─ keryx-core     solver-free LIBRARY — the translation: compile + codec + admission
│     ├─ themelios-program    construct · render/render_documented · raise · Symbol/To|FromSymbol · provenance
│     ├─ themelios-syntax     parse: text → AST  (front half of raise; admission; text-facts)
│     ├─ themelios-analysis   Analysis/Constructs scan · Atom::signatures · dependency facets
│     ├─ prost-reflect, protox   descriptor ingestion (dynamic options — the §20 rule)
│     └─ prost, prost-types    editions inspection only — decode a set's `syntax`; the struct is discarded, never feeds the schema (§18/§20)
├─ keryx-cli      the TOOL — the `keryx` command that composes the library; the language-agnostic
│                 interface a consuming tool (in any language) drives keryx through
└─ keryx-protoc   the protoc/buf plugin — the `protoc-gen-keryx` binary, a bytes→bytes shim over keryx-core
```

`keryx-test-support` is a dev-only crate (fixture compilation shared by the suites) — not shipped, and not part of the crate architecture above.

**keryx-core modules:**

| module | job | themelios surface |
|---|---|---|
| `descriptor` | ingest `FileDescriptorSet` → de-sugar → **schema model** (stable interface; carries fq-path + field number, presence, type, annotations, doc comment, field-path location) | — (prost-reflect dynamic pool; protox front door) |
| `facts` | schema model → **descriptor** facts (Appendix C) — a rendered artifact for `explain` + §21.2 self-application (not a policy input under Rust policy); surfaced by `keryx schema-facts`, distinct from the Increment-3 `keryx facts` command that renders a *payload's* ground facts to `facts.lp` | via `emit` |
| `policy` | stage 1 (**Rust**): names · qualification · presence classification · treatment · reserved-word escapes → **mapping model** (stable interface) | — |
| `emit` | stage 2: `core`/`views`/`shape` modules, manifest, scaffolds | **`construct` + `render`/`render_documented`** — direct |
| `codec` | payload ⇄ `themelios::Symbol` ground facts; validation | **`Symbol` + `ToSymbol`/`FromSymbol`**; the Symbol→`Atom`→`Rule::fact`→`render` bridge |
| `manifest` | read / write / diff (name↔number authority + evolution contract) | — (keryx-owned text) |
| `admit` | `.lp` admission, collision check, text-facts lowering | **`syntax::parse` → `program::raise` → `Analysis`/`Constructs` + `Atom::signatures`** |
| `analysis` | schema-derived, solverless metadata a consuming tool's profiles can use: keyed elements, finite domains, bounds, inert-field report | (schema inspection; solverless) |

Everything past the translation — invoking the solver, the domain model, the result envelope, and all stateful serving (sessions, episodes, ring/horizon profiles, selection, retention, blame, compaction) — belongs to the **consuming tool**, which composes keryx-core's translation and drives its own solver (R4).

---

## 4. The themelios binding

- **Value plane — `Sym = themelios::Symbol`.** The codec's value type end to end. The `ToSymbol`/`FromSymbol` exclusions *force* keryx to make the §6 decisions explicitly (range-check `u32`→`i32::MAX`; decimal-string for `i64`/`u64`; the `floor/ceil/round/trunc(f64)->Result` adapters are the `(keryx.scale)` primitive; bool/enum → constant `Function`). Interior NUL in a `Symbol::String` is refused at decode with a field-path error (spec §6; the codec marshalling constraint).
- **Emission — direct over `construct`/`render`.** keryx owns the *shapes* (path-term atoms, occupancy, shape obligations, `emit_t` markers, the module set); themelios owns the AST and the printer. Vocabulary modules render through **`render_documented`** (proto doc comments as verbatim `%!` lines); canonical `render` stays available for hash-stable text (`keryx diff`, content hashes).
- **Doc comments — proto prose → ASP doc comments (P1, §13.1).** `descriptor` reads each element's doc from `SourceCodeInfo` (precondition: the descriptor set carries source info; absent → no docs ride, still valid); `emit` attaches it via `Provenance::with_doc`; `render_documented` prints it. The honorary signature block (§13.1) and the inline `%` prose the worked stories show are a **real seam gap**: themelios's `render` emits comments *only* as statement-attached `%!` doc lines — there is no free-standing or inline plain-`%` emission, and §18 forbids string-templating it in. This is logged now as themelios gap #2 (`docs/themelios-gaps.md`), per the arm's-length rule (surface before depending). keryx `.lp` carries commentary as statement-attached `%!` docs, and the §13.1 signature block ships that way — settled at the `gen` increment (see §11 and `docs/themelios-gaps.md`), with free-standing `%` emission left as the narrowed themelios gap.
- **Admission — `parse` → `raise` → `Analysis`.** `.lp` / text-facts admission and `keryx check` lint: total, diagnostics-as-values; the `Constructs` allow-list scan, `Atom::signatures` for generated↔program collision.
- **The arm's-length rule (load-bearing).** keryx consumes themelios; it **never modifies it**. A surfaced gap is recorded in `docs/themelios-gaps.md` and closed in a *themelios* session, then adopted here by a deliberate dependency bump — the same arm's-length discipline a consuming tool applies to keryx, one layer down. Candidate gap #1: if touching `themelios_syntax::parse` directly (to hand a `Parse` to `raise`) reads as a leak, a `themelios_program::raise_source(&Source) -> Raised` convenience is the first entry — logged from keryx, fixed in themelios.

---

## 5. Data flow

**Compile pipeline:** `FileDescriptorSet → [descriptor] schema model → [policy, Rust] mapping model → [emit] core.lp · views.lp · shape.lp · manifest · scaffolds`. The schema model and mapping model are the two stable interfaces; stage-2 emission is a pure function of the mapping model (deterministic, P3 → golden-comparable). Inline options + TOML overlay merge into the schema model's annotations (overlay wins; unmatched key = error, §16).

**Inbound (message → facts):** payload (binary | JSON | textproto) → prost-reflect dynamic decode → tree-walk guided by the mapping model + a root constant → `Vec<Symbol>` + validation report. Delivered as a `.lp` fact module (Symbol→`Atom`→`Rule::fact`→`render`). Ground by construction (P10) — no grounder. Validation names field paths; partial shreds never delivered.

**Outbound (answer set → message):** answer-set `Vec<Symbol>` — from a `.lp` fixture (themelios `raise`) or the consuming tool's solver — → reachable subgraph from `emit_t`/`reach` → rebuild trees by joining on occupant terms → order sequences by index, sets by `Symbol::Ord` (canonical bytes) → `FromSymbol` scalars + term-type conformance → dynamic message → bytes. `shape.lp` in strict (UNSAT on unserializable) or diagnostic (`violates(field-path, occupant)`) mode.

**Solver-free in both directions:** outbound consumes answer sets as *input* (fixtures / the consuming tool's solver), so the entire bridge — `gen` → `facts` → *the tool's solver* → reassemble → message — is buildable and usable with no solver in keryx (R4).

**The two seams (R6).** Inbound crosses to the consuming tool as either a `Vec<Symbol>` (the library seam — fed to the tool's solver directly, no text and no grounding pass) or a `.lp` fact module (the CLI seam — the tool's solver parses and grounds it); either way the facts are ground by construction (P10). Outbound crosses the other way: an answer-set `Vec<Symbol>` in, a reassembled message out. A consumer that needs the text-free production path links `keryx-core`; the cost of the CLI seam is a parse and a grounding pass on the request path (flagged for measurement, not ruled on here).

---

## 6. Error handling & CLI posture

**Library (mission-critical Rust).** Every foreign input (payloads, descriptor sets, `.lp` text, answer sets) crosses a `Result` boundary. Errors are typed **values**, not strings/logs — a `Diagnostic { field_path, kind, detail }`-shaped taxonomy naming **field paths, not atoms** (P1); partial shreds never delivered. keryx *composes* the underlying libraries' structured errors (themelios `LowerError`/`Unspellable`/`FromSymbolError`; protox compile; prost-reflect decode) alongside its own (range, open-enum, interior-NUL, unannotated-float with the two-choice fix-it, overlay-key, shape, admission). Total functions, no panics on foreign input — themelios's posture, matched here: `unsafe_code` denied workspace-wide with `#![forbid(unsafe_code)]` per crate; `clippy::pedantic` denied; `missing_docs` / `unused` / `dead_code` denied; and totality *by construction* with `expect`s only where an invariant discharges them, documented in prose — **not** restriction lints (`unwrap_used` / `expect_used` / …), which themelios does not use (its own tests call `.expect`). keryx may add a targeted restriction lint on a specific path later if it earns one; the founding lints mirror themelios.

**CLI (Unix least-surprise).** A thin adapter that renders the library's typed errors and never invents error semantics. **stdout = the product, stderr = diagnostics/progress** (so `keryx facts … | clingo` and `… | jq` work; success is quiet). Exit codes are stable, documented, class-distinguishing — an `Exit` enum is the single home of the integers:

```rust
#[derive(Clone, Copy)] #[repr(u8)]
enum Exit { Success=0, Internal=1, Usage=2, Input=3, Schema=4, Admission=5, Shape=6 }
impl From<Exit> for std::process::ExitCode { /* … */ }   // integers live ONLY here (exact values tunable)
```

Human-readable by default; `--format json` (or when stderr — the stream diagnostics travel on — isn't a TTY) emits structured `Diagnostic`s; respect `NO_COLOR` and TTY detection. Broken pipe / `SIGPIPE` exits cleanly (no `EPIPE` panic); a top-level panic hook maps any escaped panic to a clean internal-error report + exit `1`, never a raw backtrace. Fix-it hints travel with the error.

**No magic numbers.** Semantic named constants/enums throughout: exit codes in `Exit`, error kinds in `DiagnosticKind` (rendered to the wire string only at the boundary), domain bounds named (`i32::MAX`, not `2147483647`), custom-option field-numbers resolved by *extension identity* from the vendored `keryx/options.proto` (the number lives in the `.proto` alone), infrastructure names (`reach`, `violates`, `emit_*`, `ep`, reserved words) as named tables. Enforcement is structural + review (there is no true magic-number lint).

---

## 7. Testing

The founding arc is testable end to end **without a solver**:

- **Golden compile tests:** descriptor set → golden `core/views/shape` + manifest (deterministic, P3); the plugin's bytes→bytes is trivially golden.
- **Codec round-trip properties:** payload → facts → payload identity on canonical forms; reassembly driven from **fixture answer sets parsed by `raise`**.
- **Self-application cross-check (§21.2):** hand-written stage-0 vs. `keryx(descriptor.proto)`.
- **Fixture harness (§27):** `examples/<name>/{request, expect}` (see §8); the reassembly path runs solver-free.
- **The gate** (§8) enforced per change.
- **The honest boundary:** keryx's *runtime* invokes no solver; keryx's *test harness / examples* may drive the user's clingo in CI to validate that the generated programs behave (the ASP-policy co-artifact and its fixture contracts). That is test infrastructure, not keryx code — the guard-rail holds.

The **clingo-run examples** exercise the generated theory end to end — the worked scenarios run clingo over the vocabulary, facts, and a model in CI, checking the fixtures behave. They test the translation, not the theory's semantics: the correctness argument for the emitted theory (§12.2's serializability, §12.3's inverse) is owed by the increment plan that emits it. Test infrastructure, not keryx code — §7's honest boundary.

---

## 8. Standing practices & repo

- **Arm's-length** (§4) — never modify themelios; gaps → `docs/themelios-gaps.md` → a themelios session.
- **License:** MIT `LICENSE` at founding, mirroring themelios's (holder/year form replicated at scaffolding).
- **themelios dependency:** by **git, pinned to `rev = 86c7dfb`** for the three crates (`program`, `syntax`, `analysis`) — which makes arm's-length *structural* (keryx builds against a fixed, published themelios, never a working copy). Local co-development of a themelios fix uses an *uncommitted* `[patch]`/path override; the committed manifest always names the rev. `Cargo.lock` committed. **Transition:** when themelios publishes to crates.io, keryx swaps to a semver dependency (a one-line change; crates.io is the strongest arm's-length form). themelios is a **public** repo, so the git dependency fetches over HTTPS with no credentials — locally and in CI; there is no keryx-specific dependency-access delta.
- **Repo:** the GitHub remote is `github.com:GregoryGelfond/keryx`. Shipped docs: `docs/specification.md`, `docs/design/architecture.md` (this document), `docs/themelios-gaps.md`, `docs/proto-support.md`. Vendored proto assets: `keryx/options.proto` (Appendix A).
- **CI** mirrors themelios's `checks` workflow (jobs and toolchain pin — replicated faithfully at scaffolding; themelios pins no caching); themelios is public, so no dependency-access step is needed.
- **The gate**, green per change, locally and in CI:
  ```
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings      # incl. the §6 no-panic / forbid-unsafe set
  cargo test --workspace --locked
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
  coverage floor                                                       # tool matched to themelios's, at scaffolding
  ```
- **Commit discipline:** imperative mood; the message describes the change, not the process.
- **`examples/`** — workspace-root worked scenarios (not cargo `.rs` examples), seeded from the spec's three stories, each self-contained and runnable on the solver-free standalone path (`gen` → `facts` → *user's clingo* → reassemble). They double as fixtures the gate runs (documentation-by-example = regression suite, §27) and as user onboarding, with a "plug it in" baseline-integration section. **thermal** first (headline solver-free E2E); **dispatch** and **diagnosis (translation)** with annotations.
- **`README.md`** — the user/client-facing front door, kept **pithy**: the naming blurb, a two-line *what*, one high-level example (`thermal`), getting-started, and a one-line *how it fits*. It states keryx's value and boundary crisply ("the message side never learns ASP; the model side never learns the wire"; "keryx doesn't solve — bring your own solver, clingo today") but keeps **rationale and internals out** — those live in `docs/design/`, which ship in the repo. It opens with the naming blurb, matching θεμέλιος / μορφή:
  > `# keryx`
  > `κῆρυξ, *herald* — a bidirectional bridge between Protocol Buffers and Answer Set Programming.`

  A starter README stands up with the repo at Increment 0; its "quick look" is cemented against the real thermal artifacts when that example goes green at Increment 4.

---

## 9. Enabler for the consuming tool

The needs a consuming tool places on keryx are wired in, not bolted on:

- **Ring inputs**, split at the library/service seam: keryx provides the schema-derived inputs (keyed elements, finite domains, bounds, inert-field report) via `keryx-core::analysis`; the consuming tool ingests them and binds its own profile (generating a ring program, grounding slots, managing guards). The ring program's generation is the tool's — it needs the slot budget, a publication-time decision — so keryx's obligation is the schema-derived inputs only.
- **Generated↔program collision** — detected in `keryx-core::admit` via `Atom::signatures`.
- **Value plane + totality** — `Sym = themelios::Symbol`; total functions, structured errors, no panics on foreign input.
- **Text-facts admission** — `keryx-core::admit`.

keryx enables the consuming tool by providing the **translation** (schema → vocabulary, message ⇄ facts), the **schema-derived analysis** its profiles can use, and **admission** (`.lp` / text-facts lint). The tool composes these with its own solver and stateful serving (R4). The former solve-side machinery — the fact-to-solver path, the result envelope, the backend trait — is the tool's, not keryx's.

---

## 10. Build spine & the first increment

Each increment leaves the workspace green and demonstrable and lands its worked example. keryx is **translation, end to end** — it never invokes a solver (R4); a consuming tool (in any language) invokes the solver and composes keryx's `facts`/reassembly around it. So there is no keryx `solve` and no solver-backend increment: keryx finishes *above* the solve line entirely.

| # | increment | delivers | example |
|---|---|---|---|
| **0** | **Walking skeleton** | workspace + crates + themelios git-deps + gate + CI + smoke test (construct→render + `Symbol` round-trip); LICENSE, spec + design import, gap-log stub | — |
| 1 | Ingestion | descriptor → schema model → descriptor facts; the §20 dynamic-options rule; golden tests | — |
| 2 | gen | Rust policy → mapping model; emit `core`/`views`/manifest via `render_documented`; `explain`; self-application | thermal *(gen)* |
| 3 | Inbound codec | payload → `Symbol` facts → `facts.lp`; `keryx facts`; round-trip properties | thermal *(facts)* |
| 4 | Outbound + shape | `shape.lp` (strict/diagnostic); reassemble from answer sets; `--emit`; field-path diagnostics | **thermal *(complete E2E)*** |
| 5 | Annotations + overlays | Appendix A vocabulary; TOML overlays; scalar policies; `keryx diff`; `scaffold` | dispatch; diagnosis *(translation)* |
| 6 | Admit + plugin | `.lp` admission/lint (`keryx check`); `keryx-protoc` (the `protoc-gen-keryx` plugin; editions handshake) | — |
| 7 | Targets | `--profile clingcon`; `--target <typed-dialect>` + degradation report (emission only) | — |

keryx's boundary is the translation: schema → vocabulary, message → facts, answer-set → message, plus the shape contract and the manifest. Everything past it — the solver, the domain model, stateful serving — is the consuming tool's (R4).

---

## 11. Open questions & decided-at-increment

- **Tuned at scaffolding (Increment 0):** exact exit-code integers; the coverage tool + floor (matched to themelios's); the MIT holder/year line; the CI workflow specifics; the themelios-repo CI credential mechanism.
- **Settled at the `gen` increment (2), emission:** the §13.1 honorary signature ships as `%!` docs on `#defined` declarations (themelios has no free-standing `%` block — `docs/themelios-gaps.md`); a message-typed field's functional signature (its occupant access-path term, §4.1) rides on its parent sort's `#defined`, keeping `core.lp` the complete functional canon, while the relational view is an additive `views.lp` layer that opens with `#include "<pkg>.core.lp".`.
- **Settled at the `gen` increment (2):** the shape of the Rust mapping-policy module; the manifest names a message field by its occupant term with the view noted (`readings/2 ; view readings/3`); `keryx/options.proto` resolves from keryx's embedded registry (like the well-known types), so importing it needs no `-I`. Whether/when to add the ASP policy co-artifact + its cross-check stays open.
- **Carried from the spec (§32), unchanged:** `(keryx.reify)`, `(keryx.mirror)`, the oneof discriminator view, Timestamp/Duration conveniences, `Any` registry ergonomics, manifest wire format, static per-spec codec codegen — all additive, none founding-blocking.
- **Family-level:** themelios crates.io publication (triggers the dependency-form transition, §8).
- **Candidate gap-log entry #1:** `themelios_program::raise_source(&Source) -> Raised`.

---

## 12. Status

Increments 0–2 are built: the walking skeleton, `.proto` ingestion, and the `gen` stage
(schema → ASP vocabulary, `keryx gen` / `keryx explain`). The codec — inbound facts (Increment
3) and outbound reassembly (Increment 4) — and the annotation semantics with `keryx diff`
(Increment 5) follow, per the increment ledger (§10). (The specification's §31 uses
M-milestones, offset by one — Increment 1 = M0, Increment 2 = M1 — and its M5 "Episodic" is
retired under the translation-only scope.)
