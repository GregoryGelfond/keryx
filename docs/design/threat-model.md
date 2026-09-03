# keryx — Threat model of record

**Date:** 2026-09-03
**Status:** The threat model of record for keryx. It answers to the architecture (`docs/design/architecture.md`) — §5 (the surfaces), §6 (the error and totality posture), R4 (translation-only: keryx translates; a consuming service runs the solver and owns deployment) — and to the specification (`docs/specification.md`) beneath it. It commits the **security properties** the architecture's surfaces must hold. Where a property refines the architecture's prose it says so in place (see *The dependency boundary*); otherwise the architecture remains the architecture of record.

keryx ingests descriptor sets — and, as later increments land, message payloads, ASP text, and overlays — supplied by callers it does not control, and hands back an ASP vocabulary and ground facts a caller reads without re-checking. It is built to a **mission-critical footing**: it may sit inside a service where the input it translates is wholly adversarial and where an availability or integrity failure is costly.

A **property** here is a *commitment* — it binds every keryx surface, present and future. What *holds* a property at a given revision is a *status*, recorded per door below. The two are kept separate: a reviewer reading keryx's code against this model must not read a commitment as a claim that the code already meets it.

## Readers

Three readers hold this document, and their fluencies differ:

- **The implementer** of a not-yet-built door (the codec, admission, overlays) — assumed fluent in Rust (unwinding, `catch_unwind`) and in protobuf descriptors; needs the walk sites, the generators, the per-door status.
- **The deploying operator** — assumed fluent in operating a service; needs the trust gradient, the *prefer-descriptor-sets* recommendation, the isolation requirement, and the `panic = abort` residual.
- **A reviewer reading keryx's code against this model** — needs each door's commitment and its honest status.

Where a passage serves one reader, the door sections below place it under that door rather than interleaving the three.

## The shared frame

**Adversary.** A caller who controls the entire input to a keryx door and may repeat the call. In a served deployment the concrete adversary is the **payload** sender — per request, the hot path. Out of scope: a caller who already holds a constructed `Schema` or `Mapping` (past the door — see *What is assumed trusted*), and one who controls the host process.

**Boundary vocabulary.** A *door* is a surface untrusted input crosses. The *interior* is everything reachable only from a constructed `Schema` or `Mapping`. A *walk* is any traversal over a schema or a decoded message. *Foreign code* is a dependency keryx is a client of — prost-reflect, protox, themelios.

**The trust gradient.** The *schema* — a descriptor set, or `.proto` source — is loaded once from the operator's control plane; it is configuration, operator-trusted. The *payload* streams per request; it is adversary-controlled. keryx-core defends **both** to the bar **regardless of embedding** — it is one library and cannot know which drives it — so a deployment that accepts an untrusted schema is still defended. The gradient decides where the sharpest scrutiny falls (the payload door) and carries the *prefer-descriptor-sets* recommendation (the source door, below).

## What is defended

Each property is stated once here, generically. Its *status* per door is in *The doors*.

1. **Totality.** Every door returns a value or a typed `Diagnostic`; it never panics, aborts, or hangs. keryx's own logic is total *by construction* — no partial result beside a diagnosis, no panic reachable from foreign input (`expect`s only where an invariant discharges them), known foreign-code panic triggers pre-empted (the descriptor pre-read refuses editions, an unrepresentable `syntax`, and a leading-dot package or top-level name before prost-reflect would panic on them; and the source-nesting guard refuses over-deep source before protox's unbounded parser would overflow the stack). Where totality crosses into foreign code that can fault unforeseeably, *The dependency boundary* holds it.
   *Instrument.* Two generators, because one does not reach keryx: **arbitrary bytes** exercise the decoder's totality (they overwhelmingly fail at `DescriptorPool::decode`), and **valid encodings of structurally-invalid descriptors** exercise keryx's own refusals (`MalformedDescriptor`, `MalformedOption`, the option-key path), which arbitrary bytes never reach. The second is the one that tests keryx.

2. **Bounded work — allocation and time.** No count or length read from the input sizes an allocation before it is checked against what the input carries — the "small message declaring a huge length" attack buys no memory. That is the committed allocation property, and it is what the allocation instrument holds. keryx's *own* allocation is **not** linear in the input: every schema element owns its fully-qualified name (`FqName::new(…full_name())`), Θ(*d*) long at nesting depth *d*, and the containment analysis clones full names per node and edge — so the schema is Θ(*n*·*d*). Time is likewise not linear everywhere: `recursion::mark` is quadratic in the message count by its own account. What bounds *d* is the depth property below; whether Θ(*n*·*d*) and the cycle analysis are *acceptable* against a deployment's limits is measurement, not commitment (recorded under *Open*).
   *Instrument.* A boundary test that a small input declaring a large length or count does not pre-allocate to it. A complexity bound would need its own instrument; none is committed, and the profile above is stated so a deployment sizes its limits to the truth, not to a false linear claim.

3. **Bounded-depth walks.** No walk over a schema or a decoded message may exhaust the stack. At each walk site where recursion depth is adversary-controlled, keryx's own walk carries an **explicit managed stack** — heap, not call stack — bounded by the input's nesting depth, itself bounded by the input length: structural, not a chosen number, so keryx's *own* translation imposes no depth ceiling and the specification's §8 (compositional nesting; no depth limit) holds for it. Foreign code decodes *before* keryx walks, though, and its decode recurses on the same nesting, so keeping that decode on a bounded-depth path is part of this property — and here the branches diverge: **(a)** rely on and verify the decoder's own recursion limit, which keeps *no ceiling of keryx's own* true and leaves §8's "no limit" a question about the engine; or **(b)** bound input depth before the engine, which *is* a keryx-imposed ceiling and so revises the §8 promise for payloads. Which branch is load-bearing — and, on (b), the §8 reconciliation it owes — is a decision the implementation pass settles, informed by the escalated question of what prost-reflect's and protox's decode limits actually are. **Settled for the shipped doors:** the descriptor decode takes **(a)** — prost's `RECURSION_LIMIT` (100, verified; the deepest admitted lexical chain is one shallower), so *no ceiling of keryx's own* holds and §8's "no limit" is a question about the engine, not keryx. The source door takes **(b)** on the *lexical* axis — keryx's source-nesting guard imposes a repo-local ceiling on brace depth (deriving from that same `RECURSION_LIMIT`) because protox's parser is unbounded and aborts. Its **§8 discharge:** §8's "no depth limit" is about *compositional* nesting (message-typed fields, lexically flat — the payload door's, Increment 3), so a lexical cap on source does not revise the §8 promise; and keryx's *own* walk still imposes no ceiling (the managed stack). The keryx-side guard is interim — retired when protox bounds its own parser recursion.
   *Instrument.* A deep-nesting test that would exhaust a naive recursive walk. It attaches to the **walk** (not the door) where foreign code caps decode depth below the level the walk must survive — it cannot reach the walk through the cap — so the pass states, per door, where it attaches and what it must reach past.

4. **Integrity — no silent misrepresentation.** keryx emits facts that faithfully represent the input, or it refuses; it never emits facts that misrepresent it, and a partial result is never delivered beside a diagnosis. A scalar that does not fit its term is refused, not truncated; an interior NUL, an open-enum value, a name collision are named. A silently wrong translation is worse than a refusal here.
   *Instrument.* The codec's round-trip property (payload → facts → payload, Increment 3) and the refusal tests for the cases integrity turns on.

5. **Determinism.** The same input yields byte-identical vocabulary and facts. This is an **auditability** property: a translation can be reproduced and checked. Its mechanism is ordering by `Symbol::Ord` (canonical bytes), which fixes *output order* against the hidden state — a hash seed, an iteration order — that would otherwise vary it. It does not bound *time*, so it is not by itself an availability guarantee.
   *Instrument.* The golden tests in CI.

6. **Confidentiality.** Out of scope at the descriptor and payload doors: they carry no filesystem or network reach, so nothing an adversary places in them names a resource to read back. It is **in question at the source door**, whose imports name files the resolver reads and folds into the descriptor set, whence names and doc comments render (via `render_documented`, architecture §4) into the `.lp` a caller receives. The recommendation below places that door behind operator trust — the mitigation; whether an adversary-directed import can exfiltrate file contents through the `.lp` is for the security review of the code (*Open*).

## What is assumed trusted

- **A holder of a constructed `Schema` or `Mapping`.** It is constructible only at a door (its lists are `pub(crate)`), so possession proves it crossed one; the interior reads it without re-validating. This is the value of the typed model.
- **The dependencies, on their supported profile.** themelios, prost-reflect, and protox are foreign code. keryx trusts them on the input it admits and does **not** trust them total on arbitrary input (prost-reflect panics on an editions set; that is pre-empted). Their misbehavior is insulated on every axis: their types never appear in keryx's surface, their structured errors are composed into keryx `Diagnostic`s, and an unforeseen *fault* on a foreign-input path is contained at *The dependency boundary*. themelios is pinned by git revision; prost-reflect and protox are semver dependencies held to an exact version by the committed lockfile — so under `--locked`, as CI builds it, the code keryx trusts is the code that builds.
- **The schema source, in the typical deployment** — operator configuration, not adversary input. keryx defends the schema doors regardless; the trust is the deployment's to grant, recorded not relied upon.

## The dependency boundary

Foreign code sits on more than one path, and only some carry foreign input. Containment sits at every foreign-code-meets-foreign-input crossing, and only there — by argument, not omission:

- **Descriptor-set door:** prost-reflect decodes bytes, and its accessors lazily decode as keryx's walk reads them; prost-types pre-reads for the shapes the engine cannot represent. Foreign input — *contained at both the decode and the walk.*
- **Source door:** protox compiles source (and reads the filesystem). Foreign input — *contained here* (an unforeseen protox panic), *and pre-empted*: over-deep source is refused before protox's parser overflows, and imports are confined to their include roots.
- **Payload door (Increment 3):** prost-reflect decodes the payload; the codec's `ToSymbol`/`FromSymbol` convert values. Foreign input — *contained as the door lands.*
- **`.lp` / answer-set doors (Increments 4, 6):** themelios `parse`/`raise`. Foreign input — *contained as the doors land.*
- **Emission:** themelios `construct`/`render` runs over keryx-**constructed** values, never foreign input — interior code, trusted by the interior's own trust, needing no containment.

**The containment, and its honest ground.** At each such crossing keryx wraps the foreign call in one named `catch_unwind`, so an unforeseen unwinding fault becomes a typed `Diagnostic` rather than unwinding into the caller. Its ground is **architecture §6's totality posture — "no panics on foreign input"** — not a claim that a `Result` type forbids panics (it does not). On that ground the boundary is a **security mechanism with a named residual**, whose *motivation* is contract-honesty and the keryx user's experience; calling it "not a security guarantee" would be too modest for a reviewer deciding whether to count on it.

**The fault's diagnosis.** The contained fault surfaces as a new `DiagnosticKind`, **`DependencyFault`** (the enum is `#[non_exhaustive]`; a clean add). The case-split it serves is **asymmetric**: a *keryx* bug is a panic — keryx-core stays total by construction and mints no library "internal" kind — reported by the CLI as `Exit::Internal` ("a bug in keryx"); an *upstream* fault is `DependencyFault`, a value. It carries its own CLI exit class (a new `Exit` variant — an engine fault is neither a keryx bug nor a user's schema error). The dependency and operation keryx knows with certainty ride in the `detail` prose; the wire shape (`{field_path, kind, detail}`, specification Appendix B) is **unchanged** — the `kind` distinguishes the fault, and structured fields are not yet earned.

**The untrusted text this composes** is not unique to this boundary. Every diagnostic `detail` and `Locus` composed from input or foreign code is untrusted text — `unreadable_set` carries prost-reflect's message, `source_error` carries protox's (which quotes source), `MalformedOption` embeds a `Debug` of an adversary value, every `Locus::at(…full_name())` embeds an adversary-chosen name — and all of it renders raw through `Display` to a terminal today, unbounded. So the property (composed text is length-bounded and escaped for the format it renders in) belongs at the **diagnostics level**, held by the human and wire renderers — the wire view's `escape` already covers the C0 set; the human view's bounding and escaping are this pass's — not at this boundary alone. *Instrument (this pass):* a test that a `detail` or `Locus` composed from input or a dependency, carrying control bytes and excess length, renders escaped and length-bounded in the human view, as `escape`'s test already assures for the wire.

**The panic-hook consequence, reconciled.** A panic hook runs at the panic site, *before* any `catch_unwind` unwinds. The CLI installs one (`exit::contain`) whose notice reads "this is a bug in keryx." A `DependencyFault` contained at a door would trip that hook — the user seeing "a bug in keryx" (false — the fault is upstream's, provoked by the input) *and* the `DependencyFault` diagnostic (under `--format json`, two arrays for one event). The reconciliation chosen (over swapping the global hook — process-global mutation that races other threads' panics — or neutralizing the notice — which loses the keryx-bug signal): the hook **consults `keryx_core::is_containing()`**, a thread-local set for the duration of a `fault::contain` frame, and stays silent while containing, so a contained fault reports once, as its diagnostic. Its **named residual costs**: (i) a library consumer with Rust's *default* hook still sees `panicked at …` for a contained fault, and silences it only by installing a hook that consults `is_containing`; (ii) the silence is default-only — under `RUST_BACKTRACE` the hook still emits the fault's location and backtrace, framed as a *dependency* fault (a debugging aid the operator opts into), since the returned diagnostic carries the payload message but not the location; (iii) a *non-unwinding* panic inside a frame (e.g. a `Drop` panicking during unwind) fires the hook while containing and then aborts with only the runtime's line — an accepted residual inside *The residual*'s abort case, with no stable-Rust guard (`PanicHookInfo::can_unwind` is unstable). It retired three standing statements of the old posture, updated as it landed: architecture §6, the `descriptor::decode` note, and `docs/proto-support.md`.

**The residual.** `catch_unwind` holds only an *unwinding* panic; under `panic = abort`, and for a fault that aborts, hangs, or corrupts rather than unwinds, there is nothing to catch. That residual is closed by *The division of labor*.

## The division of labor

keryx is the translation; a consuming service runs the solver and owns deployment (R4). Fault-containment divides on that line.

- **keryx guarantees:** totality (by construction, plus the contained boundary under unwinding), bounded allocation and bounded-depth walks, integrity, determinism, and the full insulation of its dependencies from its user.
- **The consuming service provides** what only a process boundary can: isolation of the translation under resource limits, so that an unforeseen fault keryx's in-process containment cannot catch — an abort, a hang, a corruption — is contained by the operating system. A mission-critical embedding runs keryx's translation isolated; keryx names the requirement and does not over-claim a total in-process guarantee. The service also owns the trust placement (a trusted schema source) and the build profile (unwinding, to keep the contained boundary live).

## The doors

Each door: its input, its trust in the typical deployment, the foreign code it invokes, its walks, its per-property **status** (*held at `b093008`* / *this pass* / *Increment N*), its instruments, and any operator recommendation.

### Descriptor-set door — `descriptor::ingest(&[u8])` — shipped

- **Input.** A serialized `FileDescriptorSet`. **Trust.** Schema (operator, typically). **Foreign code.** prost-reflect (`DescriptorPool::decode`, and the lazily-decoding accessors the walk reads); prost-types (the pre-read). **Walks.** `collect_messages` (gathers nested message types); `recursion::reaches_self` (the containment-cycle walk); the per-element `build_*` pass.
- **Status.**
  - *Totality:* the structural refusals `MalformedDescriptor`/`MalformedOption` hold — *held at `b093008`*; *this pass* pre-empts every shape the engine panics on rather than rejects (editions, an unrecognised `syntax`, a leading-dot package or top-level name), and **contains an unforeseen engine fault at both the decode and the accessor walk** — the accessors lazily decode, and keryx's walk holds no `unwrap`/`expect` of its own, so a walk fault is the engine's (`DependencyFault`), not a keryx bug (containing the decode alone was found to leave the walk exposed). The two totality generators are *this pass* too.
  - *Bounded depth:* `recursion::reaches_self` is an explicit managed stack already — *held at `b093008`*; `collect_messages` is native recursion today, and its conversion to a managed stack is *this pass*.
  - *Bounded work:* the Θ(*n*·*d*) profile is the truth today; the allocation-budget instrument is *this pass*.
  - *Integrity, determinism:* *held at `b093008`* (the refusal set; the golden tests).
- **A named vector at this door.** Option admission is a **file-name heuristic** (`descriptor::options::read`: an extension is keryx's iff its declaring file is *named* `keryx/options.proto`), not true extension identity — a crafted set can self-declare that file name (`options.rs` documents this). *Totality* survives it: a non-identifier option key is diagnosed downstream (`UnmappableOptionKey`), not panicked. Whether *integrity* survives a crafted `keryx/options.proto` that redefines an annotation is for the security review of the code (*Open*). Replacing the file-name heuristic with true extension identity is an additive follow-up.
- **Instruments.** The two totality generators; the deep-nesting test (attaching per the depth property); the allocation-budget test.

### `.proto`-source door — `descriptor::source::compile(files, includes)` — shipped

- **Input.** `.proto` file paths and include roots. **Trust.** Schema; **not** recommended at an untrusted per-request boundary. **Foreign code.** protox (compiler and resolver chain), which **reads the filesystem**. **Walks.** the descriptor door's, downstream of the compiled set.
- **Status.** Totality, integrity, and determinism ride the descriptor door's walk over the compiled set — *held at `b093008`*. *This pass:* an unforeseen protox panic is contained (`DependencyFault`); **over-deep source is pre-empted** — keryx's source-nesting guard refuses it (`SourceTooDeep`) *before* protox's unbounded recursive-descent parser overflows the stack and aborts (a second pre-emption instance beside the descriptor pre-read, property 1); the walk over the compiled set is bounded (the managed stack) and its work bounded, as for the descriptor door; **confinement** is now *held for escapes* (below). Bounded depth takes property 3's branch **(b)** on the *lexical* axis — a defense-in-depth guard with a **residual**: a sub-standard thread stack (below ~0.8 MB; the abort scales ~125 nesting levels/MB, the bound is 100) can abort below the bound, closed by the consuming service's process isolation (*The division of labor*).
- **Resource selection / confinement.** This door **selects files by design**: an `import` in adversary-supplied source names a file the resolver reads and folds into the set — so its defended property is **confinement**: the resolver reads only within the include roots, not a traversing (`..`), absolute, or symlinked import out of them. **Status: held for escapes.** protox's own import-name validation rejects a `..`/absolute import *name* (`UncompilableSource`); keryx's include-root resolver canonicalises the resolved path and refuses one that escapes its root (`SourceOutsideRoot`), catching the **symlinked** escape protox does not and backstopping the rest. **WKT/registry *shadowing*** — a user file named like a well-known type or the option registry placed *inside* a root, which resolves first by chain precedence — is **not** an escape and is **not** closed by this mechanism (mechanising it would fight the deliberate "a project's own `keryx/options.proto` wins" order, `source.rs`); it is carried *still-open* to the security review, parallel to the descriptor door's registry-shadowing integrity vector. **Confidentiality** (property 6): source-import exfiltration is closed *as a consequence* of escape confinement, subject to the security review confirming the shadowing half leaves no path.
- **Recommendation.** At an untrusted boundary, accept a **pre-compiled descriptor set** from a trusted source; treat `.proto`-source compilation as an operator or CLI convenience. keryx-core keeps the door total and bounded regardless; the recommendation steers where the compiler-on-hostile-input surface is exposed.
- **Instruments.** The confinement tests above — *this pass* to the extent confinement is established from protox's behavior; where it cannot be established, the door's status is recorded honestly and its establishment carries to the security review.

### Payload door — Increment 3

- **Input.** A message payload (binary / canonical JSON / textproto). **Foreign code.** prost-reflect (dynamic decode); the codec's `ToSymbol`/`FromSymbol`. **Walks.** the codec tree-walk, built with a managed stack from the start. **Status.** *Increment 3*, built to every property here — this is the hot adversarial door, and the reason the model precedes the increment. **Instruments.** round-trip, arbitrary-bytes totality, deep-nesting — Increment 3.

### `.lp` and answer-set doors — Increments 4, 6

- **Input.** ASP text — an answer set to reassemble; an `.lp` module to admit. **Foreign code.** themelios `parse`/`raise`. **Status.** *Increments 4, 6*, built to this model; the `DependencyFault` containment extends to the themelios crossing as these land.

### Overlay door — Increment 5

- **Input.** A TOML overlay merged into the schema model's annotations. **Status.** *Increment 5*, built to this model.

### Protoc-plugin door — Increment 6

- **Input.** A `CodeGeneratorRequest` on stdin from `protoc`/`buf`, decoded in `keryx-protoc` before any keryx-core door — untrusted bytes carrying the full descriptor closure and a parameter string. **Status.** *Increment 6*; a door in this model's own vocabulary, built to it when the plugin lands.

### Manifest-read door — Increment 5

- **Input.** A manifest read from disk (`keryx diff <old-manifest>`) — keryx-owned text, but under *regardless of embedding* every input is defended. **Status.** *Increment 5*, built to this model.

*Not doors.* The CLI's own `std::fs` reads of the paths it is given are adapter-level plumbing, not surfaces that admit adversarial structure; they are named here so the omission is deliberate.

### Summary

| door | input | shipped | hot / adversarial | note |
|---|---|---|---|---|
| descriptor set | bytes | yes | — | option-admission vector |
| `.proto` source | files + roots | yes | — | filesystem; depth pre-empted; confinement held (escapes), shadowing open; prefer descriptor sets |
| payload | binary / JSON / textproto | Increment 3 | **yes** | — |
| `.lp` / answer set | text | Increments 4, 6 | — | themelios crossing |
| overlay | TOML | Increment 5 | — | — |
| protoc plugin | `CodeGeneratorRequest` | Increment 6 | — | separate crate |
| manifest read | manifest text | Increment 5 | — | keryx-owned |

## Open — named here, settled elsewhere

By the implementation pass, by measurement, or by the security review of keryx's code:

- **WKT/registry shadowing** (source door, confidentiality/integrity) — escape confinement is *held* (protox rejects `..`/absolute import names; keryx's resolver refuses a symlinked escape). Open: a user file placed *inside* an include root under a reserved name (`google/protobuf/*`, or `keryx/options.proto`) resolves first by chain precedence — a namespace-precedence question, not an escape, parallel to the descriptor door's registry-shadowing integrity vector, not closed by this pass (mechanising it would fight the deliberate registry-override order).
- **Source-import exfiltration** — closed *as a consequence* of escape confinement (an outside-root import is refused before it is read); the security review confirms the shadowing half above leaves no residual path.
- **Option-admission integrity** — a crafted `keryx/options.proto` redefining an annotation (descriptor door).
- **Decode limits** — *settled for the shipped doors:* prost's `RECURSION_LIMIT` is 100 (deepest admitted lexical chain 99); the descriptor door takes branch (a), the source door branch (b) on the lexical axis (property 3). The payload door's compositional-depth branch remains Increment 3's.
- **`catch_unwind` soundness** — whether the foreign call is `UnwindSafe` given the dependency's state after a panic (the CLI already asserts unwind-safety at its top level), and the `panic = abort` residual.
- **Cost acceptability** — whether the Θ(*n*·*d*) allocation and the quadratic cycle analysis are acceptable against a deployment's limits (measurement).
- **Denial-of-service classes** the property set does not yet name — hash-seed steering if any input-reachable `HashMap` exists; algorithmic complexity in qualification.
