# keryx — Founding Architecture

**Date:** 2026-08-31
**Status:** The architecture of record for keryx; the keryx specification (`docs/specification.md`) is the design of record beneath it.
**Design of record:** `docs/specification.md` — revised alongside the gen stage (§6 `fixed32`; §20 editions-gate citation; Appendix A `zero`→`zero_field`; §34 qualifier-rule note; §4.2/§7.4 identifier lowering).
**Dependency:** themelios @ `86c7dfb` (public: `https://github.com/GregoryGelfond/themelios`) — program + analysis tiers built, validated, and consolidated for keryx.
**North star:** pythia requirements v0.3 (`~/Desktop/pythia-requirements-v0.3.md`).

This document fixes the founding **architecture** and the **build spine**. It is not a per-milestone design; each increment gets its own implementation plan.

---

## 1. Identity

**keryx is a bidirectional bridge between Protocol Buffers and Answer Set Programming** — a schema→vocabulary compiler and a message⇄facts codec.

Protocol Buffers is the **data-interchange / wire format** by which networked systems describe and exchange structured data. Answer Set Programming is a **declarative reasoning** formalism whose solvers compute answer sets (stable models). keryx compiles a `.proto` schema into an ASP vocabulary and its supporting theory, translates messages into ground facts over that vocabulary, and translates answer sets back into messages — so data that crosses the wire as protobuf can be reasoned over by an ASP solver and the results returned in the same form. The message side never learns ASP; the model side never learns the wire.

This is **the mandate**, and it is the whole reason keryx does not invoke a solver: the message side ↔ the model side is keryx's entire concern; solving is the solver's, not the bridge's. Not-solving is a consequence of the mandate, not an incidental design choice.

**What keryx is:** the translation layer — schema→vocabulary compilation (the ASP predicate specification model-writers reason over), the message→facts ingest, and the answer-set→message reassembly, over one shared vocabulary (spec P2).

**What keryx is not:**
- **keryx does not solve — by mandate, not omission.** Solving is not the bridge's concern. So keryx links no `libclingo`, spawns no `clingo`, and invokes no concrete solver — at run time or compile time. Solving is external: the user's own clingo today, themelios-solve (behind a trait) tomorrow.
- **keryx does not do stateful composition.** Sessions, episodes, profiles (ring / horizon), selection, retention, blame — the runtime orchestration of a served oracle — are pythia's, built *on* keryx.

keryx is designed and justified as a **standalone tool** (spec §1) and, in the same stroke, as the **enabler for pythia** (pythia §3) and the **first-consumer checkpoint** for themelios's program + analysis tiers.

---

## 2. Reconciliation ledger — the spec, brought to today

The keryx spec was written when aspis (not themelios-solve) was the backend and themelios's surface was not yet consolidated or blessed as the sole provider. This design reconciles it with three facts now true: **themelios is ready and blessed as the sole emission/analysis provider**, **themelios-solve replaces aspis but is not yet built**, and **pythia's propagated split** (pythia §0.3, §3, §7). The deltas from the spec, each justified:

| # | Delta | Supersedes | Why |
|---|---|---|---|
| R1 | **`Sym = themelios::Symbol`** — no keryx `Sym` enum | spec §22 (own `Sym` value type) | themelios's `Symbol` *is* the value type; its `ToSymbol`/`FromSymbol` coverage (`{i8,i16,i32,u8,u16,str,String}`, excluding `u32,i64,u64,bool,f64`) **is** the §6 scalar policy, type-enforced. `Symbol::Ord` = gringo order = the canonical-bytes anchor. |
| R2 | **Emission is a keryx `emit` module directly over themelios `construct`/`render`** | spec §18 (swappable builder/printer trait + internal fallback AST) | Both motivations for the trait are resolved: ship-before-themelios (themelios is ready) and provider independence vs. a rival syntax effort (OQ-9 — themelios is the blessed sole provider). One provider, no speculative indirection. |
| R3 | **Stage-1 mapping policy is computed in Rust** | spec §21.3 (ASP program as the production mechanism) | keryx invokes no solver (R4); an ASP-as-production policy would need one. The ASP formulation survives as an *optional inspectable / cross-check co-artifact* (renderable by `explain`, checkable in elenctic under the user's clingo, plus the §21.2 self-application) — the inspection/verification intent preserved without keryx solving. If the co-artifact ships, the Rust policy is sole authority and the ASP form is a *view* generated from the same mapping model (so it cannot disagree) or held to agreement by a gate — never an independently-authored second model. |
| R4 | **keryx invokes no concrete solver, ever** (compile-time included) | spec §18–§23 (aspis-driven) | keryx is translation glue; solving is themelios-solve's / the user's. This makes keryx-core solver-free and the whole founding arc solver-free. |
| R5 | **Thinner keryx-driver:** one-shot lifecycle + backend trait + envelope + §7.2 fact-delivery; **all stateful composition is pythia's** | spec §23 ("episodic library API in keryx-driver"); pythia §3/§7.4 framing | Profiles/sessions/episodes are how pythia *runs* an oracle. keryx provides the mechanism (the backend trait) and the translation; pythia composes them. The episodic *mechanism* lives in the trait; the episodic *session orchestration* is pythia (as pythia already corrected the §23 fact-path in C-K1). |
| R6 | **keryx parses no protobuf** — protox produces `FileDescriptorSet`, prost-reflect reads it | (affirms spec P9, §20) | `FileDescriptorSet` is the interface; the dynamic layer (prost-reflect) is mandatory because keryx's annotations are custom options that typed `prost` structs silently drop (§20, load-bearing). Symmetry: keryx parses nothing — protobuf via protox/prost-reflect, ASP text via themelios `raise`. |

---

## 3. Crate architecture

A Cargo workspace at `~/Projects/keryx`, centered on *translation, not solving*:

```
keryx/                     bidirectional bridge: Protocol Buffers ⇄ ASP ; solving is external
├─ keryx-core     solver-free LIBRARY — compile + codec + admission
│     ├─ themelios-program    construct · render/render_documented · raise · Symbol/To|FromSymbol · provenance
│     ├─ themelios-syntax     parse: text → AST  (front half of raise; admission; text-facts)
│     ├─ themelios-analysis   Analysis/Constructs scan · Atom::signatures · dependency facets
│     └─ prost-reflect, protox   descriptor ingestion (dynamic options — the §20 rule)
├─ keryx-driver   transient-solve glue + the backend abstraction pythia targets
│     ├─ keryx-core
│     └─ «backend trait»  concrete impl = themelios-solve when it lands (test double until)
├─ keryx-cli      thin `keryx` frontend that COMPOSES the library (a satellite, never the primary artifact)
└─ protoc-gen-keryx   bytes→bytes plugin shim over keryx-core (trivially golden-testable)
```

**keryx-core modules:**

| module | job | themelios surface |
|---|---|---|
| `descriptor` | ingest `FileDescriptorSet` → de-sugar → **schema model** (stable interface; carries fq-path + field number, presence, type, annotations, doc comment, field-path location) | — (prost-reflect dynamic pool; protox front door) |
| `facts` | schema model → descriptor facts (Appendix C) — a rendered artifact for `explain` + §21.2 self-application (not a policy input under Rust policy) | via `emit` |
| `policy` | stage 1 (**Rust**): names · qualification · presence classification · treatment · reserved-word escapes → **mapping model** (stable interface) | — |
| `emit` | stage 2: `core`/`views`/`shape` modules, manifest, scaffolds | **`construct` + `render`/`render_documented`** — direct |
| `codec` | payload ⇄ `themelios::Symbol` ground facts; validation | **`Symbol` + `ToSymbol`/`FromSymbol`**; the Symbol→`Atom`→`Rule::fact`→`render` bridge |
| `manifest` | read / write / diff (name↔number authority + evolution contract) | — (keryx-owned text) |
| `admit` | `.lp` admission, collision check, text-facts lowering | **`syntax::parse` → `program::raise` → `Analysis`/`Constructs` + `Atom::signatures`** |
| `analysis` (profile inputs) | ring's schema-derived inputs for pythia: keyed elements, finite domains, bounds, inert-field report | (schema inspection; solverless) |

**keryx-driver modules:**

| module | job | notes |
|---|---|---|
| `backend` | the backend **trait** (pythia §8, B-1…B-11): ground-once / ground-parameterized-module / build-facts-from-AST; externals declare·assign·release·assume; guard atoms; enumeration·optimization·brave/cautious·cores; interruptibility; statistics; deterministic mode; isolation-friendly; clingcon | designed now, bound to themelios-solve later; a test double stands in |
| `lifecycle` | the **one-shot** transient solve: assemble modules → ground once → solve → reassemble → envelope | drives the *abstract* injected backend; the §7.2 fact delivery lives here |
| `envelope` | envelope assembly; brave/cautious as set-ops over `models[]`; consequence scope (C-K3) | pure, solverless |

Everything stateful — sessions, episodes, ring/horizon profiles, selection, retention, blame, compaction — is **pythia**, ingesting keryx-driver's mechanism and keryx-core's translation + profile-input analysis.

---

## 4. The themelios binding

- **Value plane — `Sym = themelios::Symbol`.** The codec's value type end to end. The `ToSymbol`/`FromSymbol` exclusions *force* keryx to make the §6 decisions explicitly (range-check `u32`→`i32::MAX`; decimal-string for `i64`/`u64`; the `floor/ceil/round/trunc(f64)->Result` adapters are the `(keryx.scale)` primitive; bool/enum → constant `Function`). Interior NUL in a `Symbol::String` is refused at decode with a field-path error (spec §6; the solve-seam marshalling constraint).
- **Emission — direct over `construct`/`render`.** keryx owns the *shapes* (path-term atoms, occupancy, shape obligations, `emit_t` markers, the module set); themelios owns the AST and the printer. Vocabulary modules render through **`render_documented`** (proto doc comments as verbatim `%!` lines); canonical `render` stays available for hash-stable text (`keryx diff`, content hashes).
- **Doc comments — proto prose → ASP doc comments (P1, §13.1).** `descriptor` reads each element's doc from `SourceCodeInfo` (precondition: the descriptor set carries source info; absent → no docs ride, still valid); `emit` attaches it via `Provenance::with_doc`; `render_documented` prints it. The honorary signature block (§13.1) and the inline `%` prose the worked stories show are a **real seam gap**: themelios's `render` emits comments *only* as statement-attached `%!` doc lines — there is no free-standing or inline plain-`%` emission, and §18 forbids string-templating it in. This is logged now as themelios gap #2 (`docs/themelios-gaps.md`), per the arm's-length rule (surface before depending). Founding stance: keryx `.lp` carries commentary as statement-attached `%!` docs; whether the whole §13.1 signature block fits that shape, or needs free-standing `%` emission from themelios, is decided at the `emit` increment against the logged gap.
- **Admission — `parse` → `raise` → `Analysis`.** `.lp` / text-facts admission (pythia §11.3, §7.1) and `keryx check` lint: total, diagnostics-as-values; the `Constructs` allow-list scan (SEC-1…3, SEC-22), `Atom::signatures` for generated↔program collision (C-K4).
- **The backend trait — the arm's-length seam to solving.** keryx-driver reaches the solver only through this trait (pythia §7.6); the concrete solver is always injected. It honors the grounding constraint (§7.2): payload facts enter by pre-declared externals or by grounding an AST-built parameterized module — backend injection is for guards only (C-K1).
- **The arm's-length rule (load-bearing).** keryx consumes themelios; it **never modifies it**. A surfaced gap is recorded in `docs/themelios-gaps.md` and closed in a *themelios* session, then adopted here by a deliberate dependency bump (pythia OQ-1, one layer down). Candidate gap #1: if touching `themelios_syntax::parse` directly (to hand a `Parse` to `raise`) reads as a leak, a `themelios_program::raise_source(&Source) -> Raised` convenience is the first entry — logged from keryx, fixed in themelios.

---

## 5. Data flow

**Compile pipeline:** `FileDescriptorSet → [descriptor] schema model → [policy, Rust] mapping model → [emit] core.lp · views.lp · shape.lp · manifest · scaffolds · envelope types`. The schema model and mapping model are the two stable interfaces; stage-2 emission is a pure function of the mapping model (deterministic, P3 → golden-comparable). Inline options + TOML overlay merge into the schema model's annotations (overlay wins; unmatched key = error, §16). The per-package **envelope `.proto` types** in the pipeline are the one non-ASP `emit` output — generated protobuf via a distinct protobuf-codegen mechanism, not themelios-rendered ASP, so they do not pass through the `construct`/`render` seam.

**Inbound (message → facts):** payload (binary | JSON | textproto) → prost-reflect dynamic decode → tree-walk guided by the mapping model + a root constant → `Vec<Symbol>` + validation report. Delivered as a `.lp` fact module (Symbol→`Atom`→`Rule::fact`→`render`) or, in the driver, through the §7.2 mechanism. Ground by construction (P10) — no grounder. Validation names field paths; partial shreds never delivered.

**Outbound (answer set → message):** answer-set `Vec<Symbol>` — from a `.lp` fixture (themelios `raise`), the user's clingo, or a backend model — → reachable subgraph from `emit_t`/`reach` → rebuild trees by joining on occupant terms → order sequences by index, sets by `Symbol::Ord` (canonical bytes) → `FromSymbol` scalars + term-type conformance → dynamic message → bytes. `shape.lp` in strict (UNSAT on unserializable) or diagnostic (`violates(field-path, occupant)`) mode.

**The backend seam (deferred impl):** keryx-driver's one-shot lifecycle drives the *abstract, injected* backend; the concrete solver is never linked or spawned by keryx.

**Solver-free in both directions:** outbound consumes answer sets as *input* (fixtures / the user's clingo), so the entire bridge — `gen` → `facts` → *user's clingo* → reassemble → message — is buildable and usable with no solver. Only `keryx solve`'s backend-driven loop is deferred, and it only ever drives an injected abstract backend.

---

## 6. Error handling & CLI posture

**Library (mission-critical Rust).** Every foreign input (payloads, descriptor sets, `.lp` text, answer sets) crosses a `Result` boundary. Errors are typed **values**, not strings/logs — a `Diagnostic { field_path, kind, detail }`-shaped taxonomy naming **field paths, not atoms** (P1); partial shreds never delivered. keryx *composes* the underlying libraries' structured errors (themelios `LowerError`/`Unspellable`/`FromSymbolError`; protox compile; prost-reflect decode) alongside its own (range, open-enum, interior-NUL, unannotated-float with the two-choice fix-it, overlay-key, shape, admission). Total functions, no panics on foreign input — the estate's posture, enforced as themelios enforces it: `unsafe_code` denied workspace-wide with `#![forbid(unsafe_code)]` per crate; `clippy::pedantic` denied; `missing_docs` / `unused` / `dead_code` denied; and totality *by construction* with `expect`s only where an invariant discharges them, documented in prose — **not** restriction lints (`unwrap_used` / `expect_used` / …), which the estate does not use (themelios's own tests call `.expect`). keryx may add a targeted restriction lint on a specific path later if it earns one; the founding lints mirror themelios.

**CLI (Unix least-surprise).** A thin adapter that renders the library's typed errors and never invents error semantics. **stdout = the product, stderr = diagnostics/progress** (so `keryx facts … | clingo` and `… | jq` work; success is quiet). Exit codes are stable, documented, class-distinguishing — an `Exit` enum is the single home of the integers:

```rust
#[derive(Clone, Copy)] #[repr(u8)]
enum Exit { Success=0, Internal=1, Usage=2, Input=3, Schema=4, Admission=5, Shape=6, DomainUnsat=7 }
impl From<Exit> for std::process::ExitCode { /* … */ }   // integers live ONLY here (exact values tunable)
```

Human-readable by default; `--format json` (or when stdout isn't a TTY) emits structured `Diagnostic`s; respect `NO_COLOR` and TTY detection. Broken pipe / `SIGPIPE` exits cleanly (no `EPIPE` panic); a top-level panic hook maps any escaped panic to a clean internal-error report + exit `1`, never a raw backtrace. Fix-it hints travel with the error.

**No magic numbers.** Semantic named constants/enums throughout: exit codes in `Exit`, error kinds in `DiagnosticKind` (rendered to the wire string only at the boundary), domain bounds named (`i32::MAX`, not `2147483647`), custom-option field-numbers resolved by *extension identity* from the vendored `keryx/options.proto` (the number lives in the `.proto` alone), infrastructure names (`reach`, `violates`, `emit_*`, `ep`, reserved words) as named tables. Enforcement is structural + review (there is no true magic-number lint).

---

## 7. Testing

The founding arc is testable end to end **without a solver**:

- **Golden compile tests:** descriptor set → golden `core/views/shape` + manifest (deterministic, P3); the plugin's bytes→bytes is trivially golden.
- **Codec round-trip properties:** payload → facts → payload identity on canonical forms; reassembly driven from **fixture answer sets parsed by `raise`**.
- **Self-application cross-check (§21.2):** hand-written stage-0 vs. `keryx(descriptor.proto)`.
- **Fixture harness (§27):** `examples/<name>/{request, expect}` (see §8); the reassembly path runs solver-free.
- **The gate** (§8) enforced per change.
- **The honest boundary:** keryx's *runtime* invokes no solver; keryx's *test harness / examples* may drive the user's clingo in CI to validate that the generated programs behave (the ASP-policy co-artifact and elenctic fixture contracts). That is test infrastructure, not keryx code — the guard-rail holds.

Two complementary lenses cover the deferred solve without themelios-solve: the **test double** proves keryx's *orchestration* (right backend calls, §7.2-shaped fact delivery, envelope assembly); the **clingo-run examples** prove the *generated ASP's semantics*.

---

## 8. Standing practices & repo

- **Arm's-length** (§4) — never modify themelios; gaps → `docs/themelios-gaps.md` → a themelios session.
- **License:** MIT `LICENSE` at founding, mirroring themelios's (holder/year form replicated at scaffolding).
- **themelios dependency:** by **git, pinned to `rev = 86c7dfb`** for the three crates (`program`, `syntax`, `analysis`) — which makes arm's-length *structural* (keryx builds against a fixed, published themelios, never a working copy). Local co-development of a themelios fix uses an *uncommitted* `[patch]`/path override; the committed manifest always names the rev. `Cargo.lock` committed. **Transition:** when themelios publishes to crates.io, keryx swaps to a semver dependency (a one-line change; crates.io is the strongest arm's-length form). themelios is a **public** repo, so the git dependency fetches over HTTPS with no credentials — locally and in CI; there is no keryx-specific dependency-access delta.
- **Repo:** local `git init` now; the GitHub remote (`github.com:GregoryGelfond/keryx`) is created and pushed **only on the principal's explicit word**. `docs/specification.md` (spec import); `docs/design/architecture.md` (this document, imported); `docs/themelios-gaps.md`. Vendored proto assets: `keryx/options.proto` (Appendix A), `keryx/envelope.proto` (Appendix B).
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
- **`README.md`** — the user/client-facing front door, kept **pithy**: the naming blurb, a two-line *what*, one high-level example (`thermal`), getting-started, and a one-line *how it fits*. It states keryx's value and boundary crisply ("the message side never learns ASP; the model side never learns the wire"; "keryx doesn't solve — bring your own solver, clingo today") but keeps **rationale and internals out** — those live in `docs/design/`, which ship in the repo. It opens with the estate-standard naming blurb, matching θεμέλιος / μορφή:
  > `# keryx`
  > `κῆρυξ, *herald* — a bidirectional bridge between Protocol Buffers and Answer Set Programming.`

  A starter README stands up with the repo at Increment 0; its "quick look" is cemented against the real thermal artifacts when that example goes green at Increment 4.

---

## 9. Enabler for pythia

The propagated needs (pythia §0.3, §7, §8, §11.3) are wired in, not bolted on:

- **C-K1** — the §7.2 fact path (backend injection for guards only; facts by pre-declared externals or AST-into-module) → keryx-driver's `lifecycle` + the backend trait.
- **C-K2** — ring support, **split at the library/service seam**: keryx provides the schema-derived inputs (keyed elements, finite domains, bounds, inert-field report) via `keryx-core::analysis`; pythia *ingests* them and binds the profile (generates `ring.lp`, grounds slots, manages guards). This refines pythia §0.3's shorthand, which lists a generated `ring.lp` under keryx: the `ring.lp` generation is pythia's because it needs the slot budget — a pythia publication-time decision — so keryx's C-K2 obligation is the schema-derived inputs only.
- **C-K3** — consequence scope in the envelope operations → keryx-driver's `envelope`.
- **C-K4** — generated↔program predicate collision → `keryx-core::admit` via `Atom::signatures`.
- **§7.1** — `Sym = themelios::Symbol`; total functions, structured errors, no panics on foreign input.
- **§8** — the backend trait *is* the B-1…B-11 surface.
- **§11.3 / §7.1 text-facts** — `keryx-core::admit`.

The backend trait is designed to pythia §8 from the start (defined + tested against a double at Increment 8), so pythia opens onto a stable surface; only the concrete themelios-solve binding remains (D1).

---

## 10. Build spine & the first increment

Each increment leaves the workspace green and demonstrable and lands its worked example. **Rust policy makes `gen` solver-free**, and **the full driver *surface* is buildable now** — only the real backend binding sits below the line.

| # | increment | delivers | example |
|---|---|---|---|
| **0** | **Walking skeleton** | workspace + 4 crates + themelios git-deps + gate + CI + smoke test (construct→render + `Symbol` round-trip); LICENSE, spec + design import, gap-log stub | — |
| 1 | Ingestion | descriptor → schema model → descriptor facts; the §20 dynamic-options rule; golden tests | — |
| 2 | gen | Rust policy → mapping model; emit `core`/`views`/manifest via `render_documented`; `explain`; self-application | thermal *(gen)* |
| 3 | Inbound codec | payload → `Symbol` facts → `facts.lp`; `keryx facts`; round-trip properties | thermal *(facts)* |
| 4 | Outbound + shape | `shape.lp` (strict/diagnostic); reassemble from answer sets; `--emit`; field-path diagnostics | **thermal *(complete E2E)*** |
| 5 | Annotations + overlays | Appendix A vocabulary; TOML overlays; scalar policies; `keryx diff`; `scaffold` | dispatch; diagnosis *(translation)* |
| 6 | Admit + plugin | `.lp` admission/lint (`keryx check`, C-K4); `protoc-gen-keryx` (editions handshake) | — |
| 7 | Targets | `--profile clingcon`; `--target flint` + degradation report (emission only) | — |
| 8 | Driver surface | backend trait (§8) + test double; envelope + brave/cautious; the one-shot choreography tested vs. the double | — |
| | **═══ solve-deferral boundary — below needs themelios-solve ═══** | | |
| D1 | Real backend + solve | implement the trait on themelios-solve; one-shot `keryx solve`; the P10 fact path | — |

Ring / horizon and all stateful serving are **pythia's**, not keryx increments (R5, C-K2).

**The first increment to plan is Increment 0 — the walking skeleton.** It front-loads the founding arc's real integration risk: that the arm's-length git-deps resolve, the gate/CI go green fetching the public themelios dependency with no credentials, and the themelios API is usable from keryx exactly as recorded — proven by a smoke test that construct→renders a trivial `Program` and round-trips a `Symbol`, before any feature.

---

## 11. Open questions & decided-at-increment

- **Tuned at scaffolding (Increment 0):** exact exit-code integers; the coverage tool + floor (matched to themelios's); the MIT holder/year line; the CI workflow specifics; the themelios-repo CI credential mechanism.
- **Settled at the `emit` increment (2/4):** the honorary-signature-block emission (leaning `%!` doc annotations on sort/field statements); whether any free-standing-comment need becomes a themelios gap.
- **Settled at the `gen` increment (2):** the exact shape of the Rust mapping-policy module and whether/when to add the ASP policy co-artifact + elenctic cross-check.
- **Carried from the spec (§32), unchanged:** `(keryx.reify)`, `(keryx.mirror)`, the oneof discriminator view, Timestamp/Duration conveniences, `Any` registry ergonomics, manifest wire format, static per-spec codec codegen — all additive, none founding-blocking.
- **Family-level:** themelios crates.io publication (triggers the dependency-form transition, §8); themelios-solve timeline (gates D1).
- **Candidate gap-log entry #1:** `themelios_program::raise_source(&Source) -> Raised`.

---

## 12. The path from here

1. **This document** — written to `~/Projects/archeion/keryx/2026-08-31-founding-design/` (session artifact; archeion is not a git repo, matching the themelios convention). Imported into `keryx/docs/design/architecture.md` at Increment 0.
2. **Principal review** of this document.
3. On approval → **the implementation plan** for Increment 0 (the walking skeleton).
4. Increment 0's build stands up the repo and imports both the specification and this design.

No repo scaffolding or code precedes step 3 (the gated flow).
