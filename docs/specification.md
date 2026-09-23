# keryx — Founding Specification

**Version:** 0.1 (preliminary design, for local refinement)
**Date:** 2026-08-28
**Status:** The founding specification — the design of record *beneath* `docs/design/architecture.md`, which is the architecture of record and reconciles this document via its "deltas from the spec" table. Where the two differ, the architecture governs. In particular, keryx is **translation-only** — it invokes no solver and defines no solver backend (architecture R4/R5) — so this document's aspis / `keryx-driver` / `keryx solve` material (§18, §23, §25; §22 is reconciled in place) is superseded: the consuming tool invokes the solver and composes keryx's translation.

**Name:** *keryx* (κῆρυξ — the herald who carries messages between parties) is provisional and the maintainer's to change. Candidate alternates in the house style: *angelia*, *hermeneus*. All names in this document (crates, CLI, options namespace) follow the provisional name and rename mechanically with it.

---

## Part I — Purpose and Principles

### 1. Problem statement

keryx is a bidirectional bridge between Protocol Buffers and clingo-family Answer Set Programming. Given a protobuf schema, it derives an ASP vocabulary and its supporting theory; given protobuf payloads, it produces ground facts in that vocabulary; given answer sets over that vocabulary, it produces protobuf payloads. It exists so that:

- **Inbound:** protobuf messages can provide facts to an ASP model — "the world talks to the model."
- **Outbound:** an ASP model can communicate results to the world as protobuf messages — "the model talks back."

The tool is general-purpose. A likely eventual consumer is a remote solver service (out of scope here), but keryx is designed and justified as a standalone tool: any environment where structured data crosses into and out of ASP reasoning.

A protobuf schema, correctly read, yields **three artifacts**, and keryx generates all three:

1. A **signature** — sorts and field-function declarations (the *types* of a system description, in Gelfond–Kahl terms).
2. **Axioms** — invariants the protobuf type system enforces structurally, made explicit once flattened: single-valuedness of scalar fields, oneof exclusivity, index contiguity, enum membership, numeric ranges.
3. An **instance translator** (codec) — the runtime mapping wire bytes ⇄ ground atoms.

The spec contributes types and pre-discharged constraints; definitions and defaults remain the model author's. Generators produce regularity; meaning is authored.

### 2. Goals and non-goals

**Goals**

- G1. Bidirectional translation with one shared vocabulary (§P2).
- G2. Minimal friction on both sides of the fence (§P1): the message side never needs ASP concepts; the model side never needs wire concepts.
- G3. Clingo-family orientation: clingo is the baseline target, clingcon an optional profile, a typed ASP dialect a first-class future target. The canonical data plane is the clingo symbol algebra.
- G4. Zero-annotation totality: every valid `.proto` translates without any keryx-specific markup. Annotations refine semantics; they are never required to make translation work — with one deliberate exception (floating-point fields, §6), where silence would mean guessing.
- G5. Deterministic, spec-computable vocabulary: a model writer can derive the generated names and shapes in their head from the `.proto` alone.
- G6. Evolution safety: schema changes surface as compiler guidance (migration notes, bridge views), never as silent vocabulary forks.
- G7. Production path free of text: inbound facts enter the solver as constructed symbols through the backend, with no parse and no grounding pass on the request path.

**Non-goals**

- N1. Networking, transport, RPC, service deployment (a downstream consumer's territory). keryx defines library-level codecs and solve profiles; framing them over a wire is a consumer concern.
- N2. Function-free targets (Datalog engines, term-depth-limited tools). The design commits to function symbols. A function-free compilation target would be a distinct target with a distinct contract, not a style option; it is explicitly out of scope for v0 and may never be built.
- N3. Other IDLs (JSON Schema, Avro, Thrift). The descriptor-facts layer (§21) would admit them later; nothing in v0 accommodates them.
- N4. gRPC service definitions in `.proto` files are ignored in v0 (messages only).
- N5. A general ASP IDE story. keryx emits and analyzes `.lp` artifacts but is not an editor or LSP.

### 3. Design principles

These are named so that later decisions can be audited against them. When a future choice is unclear, it should be argued in these terms.

- **P1 — Friction criterion.** Friction is every moment a person on one side is forced to think in the other side's formalism. The bridge must be *invisible leftward* (the message side sees protobuf in, protobuf out; ASP never mentioned) and *legible rightward* (the model side can inspect exactly what the machinery did; confidence through transparency, not concealment).
- **P2 — One vocabulary, direction by role.** Inbound and outbound share a single generated vocabulary. Direction is expressed by role (facts vs. emit markers), never by namespace (`in_name`/`out_name` is forbidden).
- **P3 — Vocabulary determinism.** Generated names and shapes are a pure function of the schema's *semantics*. Wire-invisible reorganizations of the `.proto` (moving a nested type to top level, reordering declarations) must not change what generated atoms mean. Lexical structure may affect *name qualification* only, and only via the deterministic rules of §4.2.
- **P4 — Monotone revisability.** Every configuration knob *adds* vocabulary; none renames or removes it. Turning on an annotation or including a view must never break an existing rule. (Corollary: exclusive either/or generation modes are forbidden; alternatives are additive modules selected by inclusion, policed by lint, not by codegen.)
- **P5 — Type-directed, never value-directed.** A field's term shape is computable from the spec alone. No "integer if it fits, string if it doesn't." Values that don't fit the declared shape are translation-time errors, not shape changes.
- **P6 — Presence principle.** Emit an atom iff the field has a value under protobuf's own presence semantics. Implicit-presence fields always have a value (defaults materialized ⇒ total functions). Explicit-presence fields (message-typed, `optional`, oneof arms) have a value only when set (absence of atom = unset ⇒ partial functions). Presence is read from *resolved editions features* (`field_presence`), never inferred from syntax era.
- **P7 — Wire fidelity.** The wire format is a tree; the canonical encoding is a tree (path terms, §4.1). Graph semantics — one individual referenced from many places — is expressed the same way protobuf users express it: explicit ID fields at the domain level. keryx does not invent identity.
- **P8 — Physical schema vs. authored ontology.** The generated vocabulary is the *physical schema* — the interchange representation of somebody's wire format. KR-grade meaning is a thin authored layer of definitions over it (one-liners lifting inbound, lowering outbound), written only where the mechanical vocabulary is awkward.
- **P9 — `FileDescriptorSet` is the interface.** keryx consumes serialized descriptor sets. protoc is one producer among several (protox, buf). No keryx component parses `.proto` text itself.
- **P10 — Ground by construction.** Inbound translation only ever builds concrete terms over concrete payloads. The grounder is never needed on the data path; the base program grounds once, data enters through the backend.

---

## Part II — The Translation Model (normative)

This part defines the mapping from schema to vocabulary. Throughout, `Msg` ranges over message types, `f` over fields, `s` over the sort generated for a message type.

### 4. Vocabulary derivation

#### 4.1 Sorts, field functions, path terms, occupancy

The uniform rule, with no exceptions keyed to spec layout:

1. **Every message type is a sort** — a unary predicate `s/1` whose extension is the set of *occupant terms* of that type present in the data. Sorts are extensional (data-defined), never grammar-defined; this keeps grounding finite even for recursive schemas.
2. **Every field is a function on its parent sort.** Singular fields are unary functions (total or partial per P6); repeated fields are indexed families; map fields are key-indexed families.
3. **Slot occupants are path terms.** The occupant of a message-typed slot is named by its access path from a root:
   - singular message field `f` on parent `P` → occupant term `f(P)`
   - repeated message field `f`, element `i` → occupant term `f(P, i)` (0-based)
   - map message-valued field `f`, key `k` → occupant term `f(P, k)`
4. **Occupancy is sort membership.** For each occupied message-typed slot, one atom `s(occupant)` is emitted, e.g. `address(dest(shipments(req,0)))`.
5. **Scalar-valued fields are atoms, not occupants:**
   - singular scalar `f` → `f(P, V)`
   - repeated scalar (sequence) → `f(P, I, V)`
   - repeated scalar (set-annotated, §7.1) → `f(P, V)`
   - map with scalar value → `f(P, K, V)`
6. **Roots.** The root of a decoded payload is a caller-supplied constant (library API) or a generated fresh constant (CLI: `r0`, `r1`, … per invocation). Root identity is the *only* extrinsic identity in the system; everything beneath is derived.

Consequences worth stating because they carry the design:

- **No minted identity.** Neither the translator (below the root) nor the model (constructing output) ever invents opaque handles. Output construction names slots functionally — `alert(al(R))`, `assignments(p, A)` — so choice-rules-over-handle-sorts, dangling references, and accidental sharing are impossible by construction.
- **"Inlining" is revealed as cosmetic** — flattening `city(home(P), C)` into a predicate name `home_city(P, C)` is the same relation spelled worse (it saves one occupancy atom and forecloses quantification). It is therefore not offered. Every message type gets its sort; the cost is one occupancy atom per occupied slot.
- **Nesting lives inside the terms.** Models quantify sort-wise (`truck(T), capacity_kg(T,C)`); deep paths surface only when deliberately reaching through structure; iteration over containment uses the generated relational views (§13.2).

#### 4.2 Naming and qualification

- **Base names.** Field, message sort, and enum sort alike → the proto short name in `lower_snake`; a `camelCase` or `PascalCase` name is lowered (`camelField` → `camel_field`, `PascalField` → `pascal_field`), so the emitted vocabulary is uniformly snake-case regardless of the proto author's casing (protobuf convention is usually already `lower_snake`, for which the lowering is the identity). Two fields of one message that lower to the same predicate collide and are diagnosed (§6), never silently merged. Enum values → §7.4.
- **Shared field names are a feature.** `name` on both `Person` and `Company` yields one predicate `name/2` used at both sorts. In the typed target this is overloading resolved statically by the subject's sort. In the raw clingo target it is conventional ASP polymorphism-by-disjoint-sorts: legal, idiomatic, and unchecked — the lint tool (§25) can warn on cross-sort joins.
- **Qualification only on collision.** When two *different* constructs would map to the same name/arity with incompatible meaning (e.g., a scalar field `status` on one message and a message type `Status`), the stage-1 policy (§21.3) assigns qualifiers: prefix segments drawn from the fully-qualified proto path, joined with `__`, using the **shortest suffix that restores injectivity** (e.g. `dispatch__status` before `acme__dispatch__status`). Qualifier assignment is computed as an optimization (minimize total qualifier segments subject to injectivity) and recorded in the manifest.
- **Reserved words.** Clingo reserved tokens (`not`, `#…` forms cannot occur as identifiers anyway) and generated-infrastructure names (`reach`, `violates`, `emit_*`, `ep`) are avoided by suffixing `_` and recording in the manifest. The `emit.lp` obligation auxiliaries `has_<field>` and `ok_<enum>` (§12.2) are **not** suffixed this way — those prefixes are common enough that a wholesale escape would rename innocent fields (`has_permission`) — so a schema element that would collide with one at its arity is instead refused with a `GeneratedPredicateCollision` diagnostic at mapping time, never silently sharing the auxiliary's extension.
- **The manifest is the authority** (§14). Humans read names; the machine-checked identity of every vocabulary element is its fully-qualified proto path plus field *number*.

### 5. Presence, absence, defaults

- Presence is read per-field from the resolved editions feature `field_presence` (`IMPLICIT`, `EXPLICIT`, `LEGACY_REQUIRED`). Legacy files are handled uniformly: modern protobuf models proto2/proto3 as fixed feature bundles, so keryx never branches on syntax era.
- **IMPLICIT** (classic proto3 scalars): the field always has a value; the atom is always emitted with the default materialized; the function is total on its sort. The correspondence is exact: proto3 cannot distinguish the zero value from unset, and CWA cannot distinguish false from unstated — the encoding tells no lies the wire doesn't already tell.
- **EXPLICIT** (`optional`, all message-typed fields, oneof arms, proto2 optional): atom emitted iff set; the function is partial; `not f(P, _)` means *unset*, and the standard default idiom applies. Where the schema declares a default (editions restores proto2-style `default = …`; `(keryx.default)` covers the rest, Appendix A), the generator emits the totalized view `f_or_default/…` in `views.lp` (§13.2) so model authors never hand-write the two-rule default pattern.
- **LEGACY_REQUIRED**: treated as EXPLICIT for translation; `emit.lp` adds an outbound totality obligation.
- **Zero-as-absent.** Protobuf convention often uses the zero value (`0`, `""`, `false`, `FOO_UNSPECIFIED`) as a pseudo-null on IMPLICIT fields. The wire cannot distinguish this intent; the author can. The annotation `(keryx.zero) = ABSENT` converts the convention into honest partiality: the zero value emits no atom, and the field is treated as EXPLICIT-partial in the signature. Default is `(keryx.zero) = VALUE` (presence principle verbatim). For `bool` under `ABSENT`, the binary form collapses to the unary presence predicate: `active(P)` iff true — the idiomatic KR encoding, available as the opt-in.

### 6. Scalar mapping

Term shapes are type-directed (P5). Range violations are structured translation-time errors, never shape changes.

| proto type | default mapping | notes / annotations |
|---|---|---|
| `int32`, `sint32`, `sfixed32` | native clingo integer | always fits; clingo integers are machine-bounded (32-bit signed) |
| `uint32`, `fixed32` | native, **range-checked** (must fit in `i32` — clingo's integer width — i.e. ≤ 2³¹−1 = `i32::MAX`) | `(keryx.numeric) = DECIMAL_STRING` for fields that genuinely use the top bit |
| `int64`, `uint64`, `fixed64`, `sfixed64`, `sint64` | **decimal-string constant** (opaque; e.g. `"9007199254740993"`) | `(keryx.numeric) = NATIVE_CHECKED` (small-count fields) or `= CLINGCON` (constraint-participating fields; clingcon profile only, lowered to `&dom`/`&sum` variables) |
| `float`, `double` | **no default — annotation required** | `(keryx.scale) = n` → fixed-point integer (value × 10ⁿ, range-checked), or `(keryx.opaque) = true` → decimal-string constant. Unannotated float fields are a translation error with a two-choice fix-it message. |
| `bool` | constants `true` / `false` as term | under `(keryx.zero)=ABSENT`: unary predicate, atom iff true (§5) |
| `string` | clingo string constant `"…"` | escaping: `\"`, `\\`, `\n`; other control characters escaped `\xNN`-free via decimal fallback is **not** attempted — non-UTF-8 is impossible in proto strings; embedded NUL is a translation error (`InteriorNul`), and so is **any other control character but newline** — a tab, a carriage return (so a CRLF line ending), an escape, DEL, a C1 control — which the dialect's escape set (the three named) cannot spell: `UnrepresentableText`, refused before the value becomes a fact, so both delivery forms of §11 carry identical content |
| `bytes` | lowercase-hex string constant | `(keryx.value) = true` → content-hash constant (§9); base64 rejected as canonical form (case/padding ambiguity) |
| enum | symbolic constant (§7.4) | open-enum policy in §7.4 |

`NATIVE_CHECKED` semantics everywhere: the translator verifies the value fits clingo's integer range at decode time (inbound) and `emit.lp` and the reassembler verify at emit time (outbound); violations are structured errors naming the field path.

**Float lowering (`(keryx.scale)` / `(keryx.opaque)`) is byte-for-byte or a refusal.** A `float`/`double` field carries no default term — one of the two annotations is required (an unannotated float field is `UnannotatedFloat`, a translation error with a two-choice fix-it). `(keryx.scale) = n` lowers a finite value `f` to the fixed-point integer `m = round(f · 10ⁿ)` **only if** re-dividing reproduces the exact input double — `(m as f64) / 10ⁿ == f` — else `f` is *off the declared scale* and refused (`ValueNotOnScale`, whose fix-it names `(keryx.opaque)` as the exact alternative); `|f · 10ⁿ| > i32::MAX` is `ValueOutOfRange` (the scaled integer is a native clingo `i32`). The exponent `n` is capped `SCALE_MAX = 9` (so `10ⁿ ≤ i32::MAX`) and validated **before** any `10ⁿ` is formed — an `n` past the cap is a malformed annotation at the policy door, never an overflow. `(keryx.opaque) = true` lowers `f` to its **shortest round-trippable decimal** string (float/double-only). A **non-finite** value (NaN, ±Inf) has no fixed-point integer nor single decimal that round-trips to a fixed bit pattern, so it is refused under **both** treatments (`NonFiniteFloat`). The governing rule is the byte-for-byte round trip (§26): a value admitted inbound reproduces its exact wire bytes outbound, so an off-grid or non-finite float is refused, never silently rounded.

### 7. Composite constructs

#### 7.1 `repeated` — sequences and sets

- **Default: sequence.** Order and multiplicity are meaningful; elements are indexed (§4.1). The invariant *indices are contiguous from 0* is a wire-guaranteed theorem inbound and a shape obligation outbound.
- **`(keryx.set) = true`: set semantics.** The annotation drops the sequence's index — and with it the contiguity obligation — and makes the field a **membership relation** `f(P, …)`, first-class and model-assertable, serialized in a canonical order (below). Its shape follows the element kind:
  - **A scalar (or enum) set is the binary membership relation `f(P, V)`** — the sequence atom `f(P, I, V)` with its index projected away (§4.1). Identical values are one atom, so a scalar set collapses duplicates inherently; it carries no index, no contiguity, and no functionality (a set holds many values at one `P` by design). Its only obligation is the value's own over `f(P, V)` — an enum value's membership `not ok_e(V)`, an unsigned native integer's non-negative range (§12.2). A model asserts it directly (`tags(S, "fragile")`, §29).
  - **A message set is the binary membership relation `f(P, E)`** relating a parent to a **member occupant** `E` of the element sort. Per §4.1, a model constructing a set names its members functionally, by their own provenance — `alert(al(R))` … `alerts(out, al(R))`, `assignment(asg(S))` … `assignments(p, asg(S))` (§28, §29) — never by a positional index it would have to invent (§4.1's *no minted identity*). The member's identity and fields ride on `E`'s own occupancy atom `<child>(E)` and field atoms; the membership atom `f(P, E)` is the sole link from the parent to `E`.
- **Reached and reassembled through membership (§12.1, §12.3).** Both directions land on the membership relation, so one account serves them. `emit.lp` closes reachability through it — `reach(A) :- reach(X), <sort>(X), f(X, A).` (§12.1, the membership-join form, distinct from a sequence's occupancy-join) — reaching each member occupant `A` whatever its provenance, so the element sort's own field obligations fire on it. A member's well-sortedness is a **membership-keyed occupancy obligation**, distinct from §12.2's parent-over-slot occupancy (which a set does not carry, its members not being access-path slots): strict `:- reach(X), <sort>(X), f(X, E), not <child>(E).`, the diagnostic form deriving `violates(<field path>, E)` (`E` is bound positively by `f(X, E)`, so the negation is safe). This obligation is `emit.lp`'s strict/diagnostic half; the reassembler is its mode-free half — reading a member from `f(P, E)`, it requires `<child>(E)` present and refuses one that lacks it (`ShapeViolation` at the field path) — so a set member's well-sortedness has the two guardians every structural condition has (§12.2, §12.3), not `emit.lp` alone (the reassembler's first-argument-spine orphan check does not by itself reach a member named by its own provenance, so this re-check is explicit). A member that is not an occupant of the element sort is thus refused on every path — the strict theory's UNSAT, a `violates` diagnostic, or the reassembler's `ShapeViolation` — never silently admitted as an empty message. There is no contiguity obligation. Membership is a base relation of the model's own vocabulary — a `#defined` predicate `core.lp` carries and a model populates, **not** a `views.lp` view — so `emit.lp` stays a client of `core.lp` alone (a project that excludes `views.lp` loses none of a set's obligations). The reassembler reads the membership atoms `f(P, E)` — the member is the second argument `E`, its identity and fields read from `E`'s own occupancy — so it finds a set's members the same way whether a model computed them or the shred produced them. Its grounding cost is a sequence's — one instantiation per member. Reassembly differs in one way a sequence's cannot: a member named by provenance may be **shared** across parents (an access-path occupant cannot be), and protobuf messages are copies (§9), so a shared member instantiates once per reference. The reassembler bounds the total expansions by the answer set's atom count — a faithful reassembly instantiates at most one message occupant per occupancy atom — and refuses a provenance-shared DAG that would exceed it (`ReassembledTooLarge`) rather than re-expand a shared occupant per path (exponentially), so the cost stays bounded, no unbounded class.
- **Inbound shred.** For each wire element the shred mints the positional occupant `f(P, i)` (§4.1), emits its occupancy atom `<child>(f(P, i))` and field atoms **and** the membership atom `f(P, f(P, i))` linking it into the set — so a shredded set lands in the same membership representation a model-computed one does, and the round trip reads back exactly what was shredded. (A scalar set shreds directly to its `f(P, V)` membership atoms.)
- **Canonical serialization (§26).** A set's members serialize in clingo's total order on symbols — a scalar set by its value `V`, a message set by its member occupant `E` (a provenance term for a model-computed set, the positional `f(P, i)` for a shredded one) — so an **identical answer set yields identical bytes**, deterministically. This is order *within* a given answer set, not order-normalization *across* representations: a default message set is a multiset, so two answer sets naming the same messages under different member occupants serialize differently. Content-canonical order — order- and duplicate-independent — is what `(keryx.value)` buys (§9).
- **Occupant and membership are distinct roles.** The **occupant** `E` is a *term* — a member's structural name, carried by its sort atom `<child>(E)`, with its own provenance (a functional term outbound, the positional `f(P, i)` inbound). The **membership** `f(P, E)` is the *atom* that links a parent to a member. For a message set the predicate `f` names the membership relation (arity 2), and its second argument is an occupant of another functor; the shred's positional occupant `f(P, i)` shares the functor `f` but stands in argument position, told apart from the membership atom by that argument — a member occupant, never a bare index. A scalar set has no occupant, so there `f(P, V)` is the membership relation over values directly.
- **Set honesty clause.** The set annotation changes relation shape and serialization order; it does *not* by itself collapse duplicate *messages* into one individual. A message set's members are distinct for equal payloads — positionally inbound, by distinct provenance outbound — so a message set is a **multiset**. If collapse-by-content is wanted, additionally mark the element type `(keryx.value) = true` (§9); the translator then uses content-hash occupants and duplicates merge. Scalar sets collapse duplicates inherently (identical terms are one term). Set semantics are identical across proto2 and proto3: the annotation reads the same feature-resolved `repeated` label, and neither the relation shape nor the canonical order depends on the syntax era.

#### 7.2 `map<K, V>`

The best-behaved construct in the language: an unordered, key-unique association.

- Scalar `V`: `f(P, K, V)`; message `V`: occupants `f(P, K)` with occupancy atoms.
- Keys are proto-restricted to integral/bool/string; they map per §6 (a map keyed by `int64` therefore has string keys by default — annotate the key side via `(keryx.numeric)` on the map field if native keys are needed and safe).
- Invariant: functional in `K` — theorem inbound, obligation outbound. No contiguity, no order.

#### 7.3 `oneof`

Arms are EXPLICIT-presence fields on the parent sort — ordinary partial functions, one per arm — plus the generated exclusivity axiom (pairwise `:- armᵢ(P,_), armⱼ(P,_).` in `emit.lp`). Inbound this is a theorem; outbound an obligation. In the typed target, a oneof lowers to a variant type instead (§24). Which-arm interrogation in raw clingo is by arm-atom presence; no discriminator atom is generated (it would violate P4's spirit by duplicating derivable information — a `views.lp` discriminator view may be added later if practice demands, recorded as an open question).

#### 7.4 Enums

- Each enum type is a **sort of symbolic constants**, closed or open per its resolved `enum_type` feature (a proto2-era enum is closed; a proto3-era enum is open — see *Open enums* below). Value names lower as follows: if all values share the conventional `ENUM_NAME_` prefix, strip it; lowercase the remainder (`SIGNAL_LOW` → `low`). Collisions after stripping fall back to unstripped names; residual collisions qualify per §4.2. All recorded in the manifest.
- **The value domain is a populated sort.** `core.lp` (the always-loaded module) carries one fact `e(c)` per declared constant, so a model may quantify over an enum's values directly — a conditional literal `f(P, V) : e(V)` in a choice or count, a constraint ranging over every value — exactly as it quantifies over a message sort's occupants (§4). This model-facing sort is distinct from the outbound **membership table** `ok_e` (§12.2), the serializability obligation's own copy of the admissible values carried in `emit.lp`. The two hold the same values (they cannot diverge — identical facts and identical admission rules) but are kept separate deliberately: `ok_e` is one of the uniform generated auxiliaries (`has_<field>`/`ok_<enum>`, §12.2), with a reserved prefix and its own collision handling (§4.2), whereas `e/1` is a schema-derived sort subject to qualification — different namespaces, and `core.lp` populates `e/1` even when `emit.lp` is not loaded (reasoning without emission). Collapsing both onto a single `e/1` the obligation reads directly is a possible later simplification, not a rename.
- **Zero value.** The `*_UNSPECIFIED = 0` convention is the enum instance of §5's zero-as-absent: default maps it as an ordinary constant (`unspecified`); `(keryx.zero) = ABSENT` makes zero mean *unset* (no atom, field partial).
- **Open enums.** Editions expose openness as the resolved `enum_type` feature (proto3-era enums are open: unknown integers are legal on the wire). Policy for an unknown numeric value arriving inbound: the default is a **structured translation error** (loud, honest). `(keryx.unknown) = PRESERVE` opts into the escape hatch, making `unknown(N)` a first-class value of the enum in both directions.
  - **Inbound**, an unknown wire number `N` maps to the term `unknown(N)`, carried in its field atom's value position exactly as a declared constant is (a declared number still maps to its constant, so `unknown(N)` arises only for a genuinely undeclared `N`).
  - **In the theory**, `unknown(N)` is admitted to both the value sort and the membership table, one rule per enum-valued field `f` of the enum, with `unknown(N)` in the field predicate's **value argument** (the last) and every other position anonymous: for a singular or set field, `e(unknown(N)) :- f(_, unknown(N)).` (in `core.lp`) and `ok_e(unknown(N)) :- f(_, unknown(N)).` (in `emit.lp`); for a `repeated` or `map` enum field, the same with the extra index or key position also anonymous (`f(_, _, unknown(N))`). The `ok_e` rule satisfies the membership obligation `:- s(P), f(P, V), reach(P), not ok_e(V).` for `V = unknown(N)`. Both rules bind `N` **only** through the field atom that carries it — a positive body literal over the `#defined` field predicate — never as a free variable, so grounding instantiates them over exactly the finite set of `unknown(N)` terms the answer set contains; a bare `e(unknown(N)).` or `ok_e(unknown(N)).` fact with an unbound `N` would range over clingo's integers and be unsafe.
  - **Outbound**, the reassembler raises `unknown(N)` back to the wire enum value `N` — a native clingo `i32`, which an open enum's wire value may legitimately be (negatives included) — the declared inverse of the inbound lowering (§12.3). An `unknown(N)` whose `N` *is* a declared value's number is refused as a shape violation (the model must name that value by its constant), so each wire value has a single outbound term and the round trip stays a clean inverse of the inbound mapping.

  A non-`PRESERVE` enum gains neither theory rule, and its reassembler refuses `unknown(N)` as before, so any `unknown(N)` on it is refused outbound — UNSAT under the strict theory, a `violates` diagnostic under the diagnostic one (§12.2), or a shape violation at reassembly — the same loud refusal the default gives inbound. Outbound, then, only declared constants (plus `unknown(N)` under `PRESERVE`) are serializable; anything else is a shape violation.

### 8. Nesting, recursion, reification

- **Lexical nesting** (`message A { message B {…} }`) is namespacing only. It influences qualifier *choice* (§4.2) and nothing else. Moving a type between nesting levels is wire-invisible and must be vocabulary-semantics-invisible (P3).
- **Compositional nesting** (message-typed fields) is fully handled by path terms (§4.1). No depth limit in the translation — path-term construction imposes no ceiling of its own — and no special cases. What bounds a *payload's* nesting is a **door-admission policy**, stated once for every payload format (§26): the inbound walk admits at most 99 message-typed levels below the root (one below the descriptor engine's decode recursion limit, from which it is derived) and refuses a deeper payload totally — one `PayloadTooDeep` at the whole-payload locus, no facts beside it, no panic, the walk running on a managed stack. Where the ceiling binds is the format's: for the binary wire format the walk's counter binds, and the engine's decoder adds an outer bound of its own beyond it — it decodes a payload nesting exactly 100 levels and refuses the 101st and deeper (`UndecodablePayload`), so the walk refuses the 100th level and the engine everything past it; for textproto, whose parser bounds nothing of its own, a pre-parse bound binds at the same ceiling before the parser sees a token — counted in message values, so map-of-message and expanded-`Any` nesting binds earlier there than on the wire (§26) — and the walk's counter stands beneath it; for canonical JSON, whose deserializer bounds its own nesting — it counts arrays and objects and refuses the 128th nested one — no bound precedes the engine, and the walk's counter binds for a chain of singular message fields (one object a level: 99 admitted, the 100th refused, the deserializer itself admitting to the 126th), while the deserializer's own count binds first for a repeated or map-of-message chain (two containers a level: about 63), a shallower message depth than the ceiling and never a deeper one (§26). (All three formats as of the inbound codec.)
- **Recursion.** If a message type participates in a cycle of the schema's *containment graph* (reachability through message-typed fields, including via repeated/map), path terms remain sound for any finite payload — but the compile-time analysis flags the cycle, because recursive schemas are precisely where authors typically *want* reified, quantifiable individuals rather than ever-deepening paths. v0 behavior: emit a prominent note in `keryx explain` and the manifest; translate with path terms regardless. A `(keryx.reify)` annotation reserving content-addressed or key-addressed occupants for such types is sketched in Appendix A and marked open (§32) — it must be designed against P4 (additive, never renaming) before it ships.
- **Reification triggers** (design guidance, for `explain` output and documentation): reify when (a) the type is recursive, (b) it is a genuine domain concept whose extension the model quantifies over *independently of containment* — though note occupancy sorts already give quantification, so this trigger is rarer than it sounds — or (c) deliberate graph semantics is wanted, which per P7 is expressed with explicit ID fields in the schema, not by keryx machinery.

### 9. Identity and equality

- **Slot identity is the path** — always, unconditionally (§4.1). This is the wire's own notion: embedded messages are copies; sharing does not exist in protobuf values.
- **Content equality is opt-in and additive.** `(keryx.value) = true` on a message type declares it value-like. Effects: (a) the generator emits a derived structural-equivalence view `same_t(X, Y)` in `views.lp` (recursive structural comparison over declared fields), (b) `bytes` fields of the type may hash (§6), (c) set-of-message fields may collapse duplicates (§7.1) — in that case occupant terms become `t(h)` where `h` is the canonical-form content hash (lowercase hex string constant), and this is the *only* circumstance in which occupant terms are not pure access paths. Switching a type's slot terms wholesale from paths to hashes is forbidden (it would rename vocabulary, violating P4); collapse applies only where set-ness makes position meaningless.
- **Cross-message identity** (the same real-world entity appearing in two payloads or two slots) is a domain-level concept: an explicit key field, `(keryx.key) = "field_name"` documenting it, and authored ontology rules joining on it. keryx records the key in the manifest and generates nothing magical.

### 10. Well-known types and `Any`

- **Uniformity first:** `google.protobuf.Timestamp`, `Duration`, and the wrapper types (`Int32Value`, …) are ordinary messages and translate structurally with zero special cases — `Timestamp.seconds` is an `int64` and gets the decimal-string default, which is not pedantry: epoch seconds exceed 2³¹ in 2038, inside this tool's design life. Fields wanting arithmetic opt into `(keryx.numeric) = NATIVE_CHECKED` (with eyes open) or `= CLINGCON`. Convenience annotations (e.g. a first-class epoch lowering) are an open question (§32), to be added only additively.
- **Wrappers** exist to add presence to scalars; under editions they are legacy. Structural translation (a one-field occupant) is correct if noisy; `explain` suggests migrating the schema to `optional`.
- **`Any`** is nesting with no static structure. Default: opaque — `type_url(P, "…")` and `payload(P, hexbytes)`. Opt-in: `(keryx.any_types) = ["fq.Type", …]` declares a closed registry; the translator dispatches on `type_url`, compiles each registered type, and uses type-tagged occupancy on the same path term. Unregistered types arriving under a registry are structured errors. Marked semi-open (§32) pending a real use case.

---

## Part III — Directionality (normative)

The two directions share one vocabulary (P2) and one theory, but stand in opposite modalities to it: **inbound, the invariants are theorems** (the wire format guarantees them; the generated axioms are checkable documentation); **outbound, the same invariants are obligations** (nothing in stable-model semantics prevents an answer set from containing two `name` atoms for one occupant). One compiler core emits both artifacts from one schema; only the modality differs.

### 11. Inbound: shredding

- Decode wire bytes (or canonical JSON, or textproto — §26) against the descriptor; walk the tree; build symbols per Part II. Ground by construction (P10): no gringo on this path. (Realised for all three by `Codec::shred` and `keryx facts` as of the inbound codec.)
- Root supplied by caller (§4.1.6). All facts of one payload are attributable to its root by term structure alone.
- Delivery has two forms with identical content: (a) **text** — a `.lp` fact module (the human-readable, diffable, archival serialization of the data plane), and (b) **symbols** — a `Vec<Symbol>` the consuming tool feeds its solver directly, with no text and no grounding pass. The `.lp` form exists for inspection, fixtures, and archival; it is never required.
- Inbound validation: range checks (`NATIVE_CHECKED`), open-enum policy, `Any` registry, UTF-8/NUL and the other control characters the dialect cannot spell (§6), the unannotated-float refusal (§6), the nesting ceiling (§8). Failures are structured errors naming the field path (§26) — an undecodable or over-deep payload, the whole payload; partial shreds are never delivered: every fact or every diagnosis.

### 12. Outbound: emission

#### 12.1 Roots and reachability

For every message type `T` exported as a response root, the generator emits a **root marker** predicate `emit_t/1`. The model asserts `emit_t(term)` to export a tree. The response schema is, in effect, a *typed, structured `#show`*: it declares the output vocabulary and its nesting, where `#show` gives a flat untyped atom list. Everything reachable from an emitted root through message-typed slots is serialized; everything else in the answer set is the model's private business.

```prolog
reach(X) :- emit_plan(X).
reach(A) :- reach(X), plan(X), assignments(X, A).   % one rule per message-typed field
```

The closure step's shape follows the field's form. The second rule above is a **set**'s membership-join (`assignments` is `(keryx.set)`-annotated, §29): the membership atom `f(X, A)` binds the member `A` directly, whatever its provenance. A singular, sequence, or map message field instead uses an **occupancy-join** — `reach(A) :- reach(X), <sort>(X), <child>(A), A = f(X[, I | K]).` — binding the occupant `A` through its child sort atom before the equality deconstructs its access-path term (§7.1, §13.3).

#### 12.2 The serializability theory (`emit.lp`)

Generated obligations, each guarded by `reach/1` so working predicates stay unconstrained:

- functionality (and, for IMPLICIT/LEGACY_REQUIRED fields, totality) of singular fields;
- index contiguity from 0 for sequences;
- key functionality for maps;
- oneof pairwise exclusivity;
- enum membership; numeric range for `NATIVE_CHECKED`;
- occupancy consistency (an occupant term's sort atom is present if any of its field atoms are — the field-atom ⇒ sort-atom direction; the reverse is not enforced, so a message with no fields set keeps its sort atom via its parent's occupancy rather than being refused, correcting an "iff" that would refuse valid empty messages);
- set-member occupancy — for a `(keryx.set)` message field, that each member the membership relation names is an occupant of the element sort (the membership-atom ⇒ sort-atom direction, §7.1): a set member is named by its own provenance and reached through membership, not an access-path slot, so its well-sortedness is held by this obligation of its own rather than by the slot occupancy above; a set carries neither functionality nor contiguity.

Two modes, one flag apart, both generated:

- **strict** (production default): obligations are integrity constraints. Every answer set is **serializable by construction**; the failure mode is UNSAT — "no expressible answer exists" — never garbage bytes. The known cost: domain-UNSAT and schema-too-narrow are conflated, which is what diagnostic mode is for.
- **diagnostic** (development default): each obligation instead derives `violates(FieldPathDescriptor, Occupant)`, models survive, and the reassembler reports violations as structured diagnostics naming *field paths, not atoms* (P1). An optional `optimize` variant demoting obligations to weak constraints ("nearest serializable model") is reserved but not v0.

#### 12.3 Reassembly and canonical serialization

The reassembler inverts the shred: collect the reachable subgraph, rebuild trees by joining on occupant terms, order sequences by index and sets by clingo's total symbol order (canonical bytes: identical answer sets ⇒ identical payloads), map terms back to scalars per §6. Term-type conformance (a string term where an integer is declared, etc.) is validated here in both modes with structured errors — some type discipline is not economically expressible as ASP constraints in the raw clingo target; the typed target discharges it statically (§24). For a `PRESERVE` open enum (§7.4) this conformance also admits an `unknown(N)` term, raising it to the wire value `N` (refusing an `N` that names a declared value, whose constant the model must use instead); for every other enum a non-constant term is refused as before.

Path-term construction cannot create cycles, but a `(keryx.set)` member named by its own provenance (§7.1) can be named a member of many parents — a DAG, or an ancestor — which path-terms alone could not form. So reassembly termination is **enforced**, not merely structural: the depth ceiling bounds a chain's depth (`ReassembledTooDeep`), and an expansion budget of the answer set's atom count bounds the total occupants built (`ReassembledTooLarge`), refusing a provenance-shared set-member DAG rather than re-expanding a shared occupant once per path (exponentially). A shared member within both bounds is built as an independent instance per reference (§9's copies), so the walk terminates and never double-consumes an occupant.

(Realised as of Increment 4's reassembler (`Codec::reassemble`): a slot index over the reachable subgraph joins field atoms to their occupants; a managed stack reconstructs each `emit_<sort>` root's tree under the uniform nesting ceiling (99, the inbound constant) — no native recursion — top-down to discover and validate, then bottom-up children-first to build; terms raise back to scalars by the inverse §6 policy with term-type conformance checked; and the message serializes canonically on a keryx-sized thread to the binary wire form, the protobuf text format, or the canonical JSON mapping, a map field's entries ordered by key in each form (the engine holds a map unordered — the binary wire re-sort `codec::canonical`, the textproto sorter `codec::canonical_text`, serde_json's ordered map). One message per root, roots ordered by their marker's total symbol order; every message or every diagnosis, the diagnostic theory's `violates` atoms reported mode-free beside the reassembler's own refusals (the reassembler runs no solver, so it takes no mode; R4). An orphan field atom within an exported tree — its parent occupant descends from a marker root but its occupancy atom is absent — is refused (`ShapeViolation`), while an atom reachable from no marker is the model's private business (§12.1), ignored. The answer set is read from `.lp` through themelios `raise` (`codec::answer`), and `keryx emit` writes one message to stdout in the form `--out` names; a set's members are ordered by symbol order (§7.1), and a member shared across parents is built once per reference (a plan instance apiece), the reassembly bounded by the answer set's atom count.)

#### 12.4 Envelope, multiplicity, consequence modes

An answer to a solve is not one message: a solve yields zero or more models, each yielding zero or more emitted roots. keryx generates a standard **envelope** per schema package (shape sketched in Appendix B):

```proto
message SolveResponse {
  Result result = 1;            // SAT | UNSAT | OPTIMAL | error; stats; diagnostics
  repeated Model models = 2;    // one per enumerated model
}
message Model { repeated Plan plans = 1; /* one repeated field per exported root type */ }
```

Projection is per-model; therefore **brave and cautious consequences are envelope-level set operations** (union/intersection over `models[]`), computed by keryx without touching the solver. Consumers that want exactly one payload use single-model solves or take `models[0]` under a documented policy; the envelope never lies about multiplicity.

---

## Part IV — Generated Artifacts

### 13. The module set

Per generation unit (proto package), four files. Assembly is automatic in `keryx solve` (§25): the model file contains domain content only; include lines return only when someone opts *out* of a module.

#### 13.1 `<pkg>.core.lp`

Sorts, occupancy, and the signature. In the raw clingo target the signature is an honorary one — a structured comment block, since clingo has nowhere to put declarations (the standing argument for the typed target, where these become checked):

```prolog
% dispatch.v1.core — signature
%   sort truck/1, shipment/1, address/1, item/1
%   capacity_kg : truck -> int32            (total)
%   dest        : shipment -> address       (partial)
%   items       : shipment × index -> item  (sequence)
%   tags        : shipment -> string        (set)
%   priority    : shipment -> int32         (partial)
%   dock | locker : oneof handoff           (partial, exclusive)
```

The presence word reads: **(total)** an IMPLICIT field, always present; **(partial)** an EXPLICIT one, present only when set; **(required)** a proto2 `required` field — read as partial inbound (its presence taken from the message), but totality-obliged outbound, so `emit.lp` obliges its presence exactly as a `(total)` field's (§5, §13.3). So `(required)` reads unambiguously against `(total)`/`(partial)`: it is the outbound obligation, not a change of inbound presence.

Doc comments from the `.proto` (`SourceCodeInfo`, §20) ride along verbatim above the entries they document — the spec author's prose becomes the model writer's documentation.

Beyond the signature, `core.lp` materializes each enum's value domain (§7.4): one ground fact `e(c)` per declared constant and, for a `PRESERVE` open enum, the admission rule `e(unknown(N)) :- f(…, unknown(N)).` per enum-valued field — the sort a model quantifies over. These are the module's ground content; the enum's honorary `#defined e/1`, which carries its signature line, sits redundant-but-harmless beside the facts that now define the same predicate.

#### 13.2 `<pkg>.views.lp` (additive; included by default)

- **Relational views** over the functional canon, for join-style iteration (one line each, term matching):
  ```prolog
  dest(S, A)     :- address(A), A = dest(S).
  items(S, I, E) :- item(E),    E = items(S, I).
  ```
- **Totalized defaults** `f_or_default/…` for every EXPLICIT field with a declared default (§5).
- **Structural equivalence** `same_t/2` for `(keryx.value)` types (§9).

Views are generated, never hand-edited; they are additive vocabulary (P4) and any project may exclude the file and lint against its use.

#### 13.3 `<pkg>.emit.lp`

The serializability theory (§12.2), generated as two variants rather than parameterized by a `#const`: `<pkg>.emit.lp` is the strict theory and `<pkg>.emit-diagnostic.lp` the diagnostic one — each a client of `core.lp`, opening with `#include "<pkg>.core.lp".` as `views.lp` does — so a project loads the one it means by name. `keryx gen --shape strict|diagnostic|both` selects which is written (`strict`, the production default, when unstated), and the manifest's header records the choice in its `shape` column (§13.4; Appendix B). (Realised by `keryx gen` as of Increment 4's theory generation, with its reassembler (§12.3): the root markers and reachability closure of §12.1 and the obligations of §12.2 in both modes, golden-tested and grounder-checked — the totality of a `LEGACY_REQUIRED` (proto2 `required`) field among the obligations, the `Mapping` carrying it as `Totality::Required`, emitted and enforced (the strict theory's UNSAT, the reassembler's `ShapeViolation`) — full proto2/proto3 outbound parity (`docs/proto-support.md`).)

#### 13.4 The manifest — `<pkg>.keryx-manifest`

The evolution contract and naming authority (P3, G6). Format sketched in Appendix B. Contents per entry: fully-qualified proto path, field **number**, resolved presence, declared type, applied annotations (with provenance: inline option vs. overlay), emitted name/arity/shape, qualifier decisions, reserved-word escapes. Plus: schema content hash, keryx version, target, profile.

Protobuf's contract is that field numbers are identity and names are free to change; a predicate's identity is its name. The manifest records the binding of the two for each generated version, and `keryx diff` compares the binding across versions by regenerating both — it diffs the two generated mappings, each from its own `.proto`, matched by protobuf identity (field number; sort and enum path), and reads no manifest — so that regeneration after a rename **diffs into a migration notice** — and optionally a generated *bridge view* (`old_name(…) :- new_name(…).` plus a lint deprecation) — instead of a silent vocabulary fork.

### 14. Scaffolds

`keryx scaffold --emit T` writes the outbound lowering skeleton: heads fixed by the schema, bodies holed. This is the response plumbing with the mechanical parts pre-written:

```prolog
% keryx scaffold --emit Plan          (▢ = model-side content)
assignment(asg(▢)) :- ▢.              % provenance term + producing condition
shipment_id(asg(S), V) :- assignment(asg(S)), V = ▢.
truck_id(asg(S), V)    :- assignment(asg(S)), V = ▢.
plan(p).  assignments(p, A) :- assignment(A).  emit_plan(p).
```

A correspondence annotation `(keryx.mirror) = "Shipment.id"` letting the generator fill echo-bodies outright is sugar under consideration, not v0 (§32).

---

## Part V — Annotations and Overlays

### 15. The option vocabulary

Annotations are protobuf **custom options** in the `keryx` namespace (`keryx/options.proto`, full draft in Appendix A; field numbers from the 50000–99999 org-internal range). Governing rules:

- **Phrasing rule (P1).** Options are phrased in the *spec author's* ontology — is order meaningful? is zero a real value? what is the unit scale? which field is the key? — never in ASP's. A spec author must be able to answer every annotation question without knowing what a predicate is.
- **Never load-bearing (G4).** Every schema translates unannotated; annotations refine. The single exception: `float`/`double` (§6), where any silent default would be a guess about semantics.
- **Additive only (P4).** No annotation renames existing vocabulary; each adds or refines. (The reserved `reify` annotation must clear this bar before it ships.)
- Under Edition 2024, consumers can pull the vocabulary with `import option "keryx/options.proto";` — options only, no symbol pollution. Earlier files use a plain import.

### 16. Overlays for borrowed specs

The common case: the `.proto` belongs to another team and cannot be edited. The **overlay** is a TOML file mapping fully-qualified field/type paths to the same options:

```toml
# dispatch.keryx.toml
["dispatch.v1.Shipment.tags"]
set = true

["dispatch.v1.Shipment.priority"]
default = 3

["acme.common.Money.units"]
numeric = "CLINGCON"
```

Inline options and overlay entries lower to **identical policy facts** (§21.3); owned and borrowed specs behave the same. Precedence: overlay wins over inline (the local decision is the deliberate one); every applied source is recorded in the manifest. Overlay keys that match nothing in the schema are errors (typo protection).

---

## Part VI — Architecture

### 17. The data plane

The canonical data plane is the **clingo symbol algebra** — numbers, strings, and function symbols as constructed values — not ASP text. The `.lp` fact module is one *serialization* of that plane (human-readable, diffable, archival); the production path is `protobuf bytes → decode → symbol construction → backend injection`, with no text and no grounding pass (P10). This contract holds unchanged over clingo today and over an alternative grounder/solver line later, since both speak the same term algebra by design.

### 18. Ecosystem position and dependencies

Because this spec must be self-contained, the neighboring projects are characterized here to the depth keryx needs; keryx must remain buildable if any of them lags, per the posture noted with each.

- **aspis** — the maintainer's Rust API over clingo and clingcon (richer, Rust-idiomatic layer above libclingo; minimal FFI; supports solving, optimization, multi-shot). keryx's solve profiles (§23) are written against aspis: symbol construction, `Backend` access (adding rules/externals programmatically), assumptions, model iteration, unsat cores. *Posture:* hard dependency of `keryx-driver`; `keryx-core` (compile + codec-to-symbols-as-data) must not depend on it, so the compiler is usable solverless.
- **themelios** — the maintainer's foundation library providing ASP syntax parsing and AST generation for an alternative toolchain. *Posture:* preferred provider behind an emission boundary, not a hard dependency (see below).
- **ASP contract testing** — a declarative ASP testing approach: contracts as `@`-annotations inside `.lp` files (sat/unsat, model counts, brave/cautious consequences, costs, optimality, clingcon assignments, three-valued `@query`); verdicts PASS/FAIL/UNDECIDED; solver declared in the contract. keryx's fixture harness (§27) emits contract-consumable artifacts.
- **Rust crates:** `prost-reflect` (dynamic descriptor pool + dynamic messages; §20 explains why the dynamic layer is mandatory), `protox` (pure-Rust protobuf compiler producing `FileDescriptorSet`, enabling the no-protoc single-binary story), `prost`/`prost-types` (one typed decode on ingestion — the editions-`syntax` inspection, whose decoded struct is discarded; no typed struct ever feeds the schema, §20). External binaries `protoc` and `buf` are *optional producers*, never build dependencies.

**The emission boundary.** All generated ASP is constructed as syntax values and pretty-printed — never string-templated. `keryx-core` defines a minimal internal representation sufficient for its own output shapes (terms; classical atoms; `not`; rules; integrity constraints; `#count`/`#sum` aggregate atoms in bodies; comparison literals; `#minimize`; `#show`; `#const`; comments; includes) behind a small builder/printer trait. The **themelios backend implements that trait** when themelios is ready (and is the intended eventual sole implementation, unifying the toolchain's AST); the internal fallback keeps keryx shippable meanwhile. Model-file *analysis* (lint §25, scaffold hole-checking) parses `.lp`, which is themelios's job; until it lands, analysis features are gated off rather than half-built on regex.

### 19. Crate layout

```
keryx/                          (Cargo workspace)
  keryx-core/                   compile pipeline + codec, solver-free
    descriptor/                 ingestion, de-sugaring → schema model (§20)
    facts/                      stage 0: schema model → descriptor facts (§21.1)
    policy/                     stage 1: mapping policy (.lp assets + evaluation) (§21.3)
    emit/                       stage 2: modules, manifest, scaffolds; emission trait (§18)
    codec/                      payload ⇄ symbolic-value data model (Sym enum), validation
    manifest/                   write (the per-version record; never read back)
    diff/                       two generated mappings → migration report, JSON changeset, bridge views (§13.4, §27)
  keryx-driver/                 aspis-backed runtimes: one-shot + episodic solve,
                                envelope assembly, brave/cautious, fixture harness
  keryx-cli/                    the `keryx` binary (§25)
  protoc-gen-keryx/             thin plugin shim (§20): stdin CodeGeneratorRequest →
                                keryx-core → stdout CodeGeneratorResponse
```

Stage-1 policy programs ship as embedded `.lp` assets of `keryx-core`. Their evaluation needs a solver: `keryx-core` exposes policy *facts* and expects a `PolicyEval` callback; `keryx-driver` supplies the aspis-backed evaluator; a vendored fallback (shelling to a user-provided clingo) is acceptable for `keryx-core`-only consumers but not required in v0.

### 20. Descriptor ingestion

Background, condensed to what the implementation needs:

- **protoc** is protobuf's reference *front end*: it parses `.proto`, resolves imports, type-checks, and produces **descriptors** — the AST, itself defined in protobuf (`descriptor.proto`: `FileDescriptorSet ⊃ FileDescriptorProto ⊃ DescriptorProto/EnumDescriptorProto`, fields as `FieldDescriptorProto` with name, number, label, type or fully-qualified `type_name` string reference, and options). Cross-references are qualified-name strings, so the descriptor graph is already relational. `SourceCodeInfo` carries doc comments keyed by path into the tree.
- **Producers.** `FileDescriptorSet` is keryx's interface (P9); its producers are: (a) embedded **protox** — the default UX: `keryx gen foo.proto` works with no protoc installed; (b) a serialized set from `protoc --descriptor_set_out=… --include_imports --include_source_info` or from **buf** (whose compiler emits the same descriptors and whose breaking-change detection complements the manifest); (c) the **plugin protocol**: `protoc --keryx_out=… ` spawns `protoc-gen-keryx`, writing a `CodeGeneratorRequest` (files to generate, parameter string, full transitive descriptor closure) to stdin and expecting a `CodeGeneratorResponse` (files or error) on stdout — a pure bytes→bytes function, trivially golden-testable.
- **Editions.** Modern protobuf replaces the proto2/proto3 syntax split with *editions* (2023, 2024 released; more coming): per-file/message/field **features** with per-edition defaults. keryx branches on **resolved features** only — `field_presence` (§5), `enum_type` (§7.4) — never on syntax era; legacy files arrive as fixed feature bundles. The plugin shim must advertise editions support (`FEATURE_SUPPORTS_EDITIONS` + `minimum_edition`/`maximum_edition` in its response) or protoc rejects editions inputs. protox's editions coverage is a **verification gate** (§31). Until both protox and the descriptor engine (prost-reflect) gain editions, keryx refuses an editions schema with a specific diagnostic — both the `.proto` front door and a protoc-compiled descriptor set fail, since prost-reflect has no editions `Syntax` (see `docs/proto-support.md`).
- **The dynamic-layer rule.** Custom options surface inside descriptors as *extension fields* of the options submessages. `prost`-generated typed structs do not retain unknown fields, so routing descriptors through `prost-types` silently **drops the bytes carrying keryx's annotations**. Therefore: descriptor ingestion is `prost_reflect::DescriptorPool::decode` over the raw serialized set, options read dynamically; typed structs only downstream of the schema model — no typed `prost` struct ever feeds the schema (the one typed decode, an editions-`syntax` inspection, is read and discarded, §18). This rule is load-bearing; violating it produces a tool that appears to work and ignores every annotation.
- **De-sugaring.** Descriptors are a compiler IR with historical warts. Ingestion normalizes them into a clean **schema model** so nothing downstream reasons about encoding artifacts: `map<K,V>`'s synthetic `*Entry` nested message (flagged `map_entry`) → a map field; proto3 `optional`'s synthetic single-field oneof → an EXPLICIT singular field; delimited/group encoding → ordinary message field; feature resolution applied; well-known types passed through structurally (§10).

### 21. Compilation stages

```
.proto ─(protox | protoc | buf)→ FileDescriptorSet
   → [stage 0] schema model → descriptor facts
   → [stage 1] mapping policy (ASP over descriptor facts) → mapping model
   → [stage 2] emit: core/views/emit modules, manifest, scaffolds, envelope types
                codec tables (manifest-driven generic codec)
```

#### 21.1 Stage 0 — descriptor facts

The de-sugared schema model lowers to a flat fact base: the idealized schema language, not the encoding of it. Sketch (full vocabulary in Appendix C): `message/2`, `field/6` (parent, number, name, type, presence, cardinality), `nested/2`, `enum_value/3`, `oneof/3`, `opt/3` (path, key, value — from inline options and overlays alike), `doc/2`.

#### 21.2 Self-application

`descriptor.proto` describes itself. The stage-0 fact vocabulary is therefore *definable* as keryx applied to `descriptor.proto` — the tool's input language, run through the tool. Bootstrapping order: implement stage 0 by hand first (M0); once `gen` works, add the cross-check test that hand-written stage 0 agrees with self-applied keryx on `descriptor.proto`. A pleasing invariant and a real regression net.

#### 21.3 Stage 1 — policy as ASP

The mapping *policy* — name assignment and qualification, presence classification, scalar/composite treatment selection, annotation application, reserved-word escapes — is an ASP program over descriptor facts whose stable model **is** the mapping. Qualifier assignment is literally an optimization: minimize total qualifier segments subject to name/arity injectivity (`#minimize`). Benefits: the policy is inspectable (`keryx explain` renders the model), overridable in principle, and **testable with ASP contracts as ordinary ASP** — mapping bugs become failing `@`-contracts over descriptor-fact fixtures. Stage 1 must be deterministic: the policy program is written to have a unique optimal model (tie-breaks by lexicographic cost terms); multiplicity here is a keryx bug, asserted in tests.

#### 21.4 Stage 2 — emission

Pure function of the mapping model: the module set (§13), manifest, scaffolds, envelope `.proto` types (§12.4), and the codec's dispatch tables. The generic manifest-driven codec is v0; static per-spec Rust codegen is a later optimization, not a founding requirement.

### 22. The codec

*Reconciled in place to the architecture's R1/R4/R6 at the inbound codec: the value type is themelios's `Symbol` (R1), there is no driver and no backend injection (R4), and the two delivery seams are the library's symbols and the CLI's `.lp` text (R6).*

- **Data model:** the clingo symbol algebra as themelios's `Symbol` — no keryx `Sym` type (R1); `keryx-core` stays solver-free by never invoking a solver at all (R4).
- **Inbound:** a `Codec` built once per schema — the descriptor set (or `.proto` source) ingested and retained, its mapping model computed and indexed — shredding any number of payloads: dynamic decode against the retained pool (the binary wire format, textproto, and canonical JSON as of the inbound codec — a text payload bounded by a pre-parse nesting guard and parsed on a thread keryx sizes; a JSON payload deserialized whole under its deserializer's own nesting count, on a thread keryx sizes likewise — §26) → the managed-stack tree walk under the mapping model, the §6 scalar policy, and the §8 nesting ceiling → `Facts`: the ground `Vec<Symbol>` of the library seam — one head symbol per fact, built from one `(predicate, arguments)`, the one model of the payload's facts — and the `.lp` fact module of the CLI seam, rendered from those symbols (§17), so they cannot disagree (§11). The root *type* is named by proto name and resolved by the library, the one type-resolution site; the root *constant* is the caller's, or the CLI's fresh `r0` (§4.1).
- **Outbound (built at Increment 4):** answer-set symbols — the consuming tool's solver's output, or an `.lp` answer set parsed through `raise` (`codec::answer`) — → a slot index over the reachable subgraph → validation (§12.3) → managed-stack reconstruction → the engine's validating setter → canonical serialization to the binary wire form, the protobuf text format, or the canonical JSON mapping. One message per `emit_<sort>` root, deterministic (a map ordered by key in each form); every message or every diagnosis (§6, property 4).
- Both directions are total functions with structured error results; no panics on foreign input — an unforeseen fault in the decode or encode engine is contained as a typed dependency fault (the threat model's dependency boundary).

### 23. Solve lifecycles

Two profiles, one vocabulary; profile choice never changes generated artifacts.

- **One-shot** (`keryx solve`): assemble modules + model, ground once, inject the payload's facts, solve, project models, build the envelope. For M2 simplicity the fact path may temporarily route as text include; the backend path replaces it at M5 without observable change (P10 realized).
- **Episodic** (library API in `keryx-driver`; CLI exposure minimal in v0): the base program grounds **once** at startup; thereafter, per episode *k*:
  1. decode payload → symbols (no grounding);
  2. mint a fresh guard atom `ep(k)` and register it via the solver **backend as an external**;
  3. inject each fact `a` as the backend rule `a ← ep(k)`;
  4. solve with assumptions selecting the active episode set — sliding window (last *n* guards true, rest false), cumulative (all true), or arbitrary what-if subsets;
  5. **retire** an episode permanently by releasing its external (the solver may then simplify its rules away). The vocabulary never changes; only the assumption set does.

  An equivalent encoding uses backend choice atoms as guards with explicit negative assumptions; externals-with-release is normative here because release enables simplification and matches multi-shot idiom. New terms per episode are unproblematic precisely because guards are backend-registered atoms, not grounded `#external` directives (which require grounding-time domains).
- **Diagnosis of the pipeline itself:** guards double as assumption-labeled suspects — on UNSAT, the unsat core names *which episodes* jointly broke consistency, and the envelope reports it as such.
- Envelope assembly, `-n N` enumeration, and brave/cautious set operations (§12.4) live in the driver, uniform across profiles.

### 24. Targets and profiles

- **`--target clingo`** (v0 baseline): everything above; signature as structured comments; polymorphic shared names allowed and lint-watched.
- **`--profile clingcon`** (orthogonal flag): enables `(keryx.numeric) = CLINGCON` lowering of designated integer fields to `&dom`-declared variables usable in `&sum` constraints; forbidden without the profile (structured error).
- **`--target <typed-dialect>`** (a typed ASP dialect, early design): same vocabulary, but the honorary signature becomes checked declarations. This importer is deliberately a **conformance suite** for the dialect — each construct it must express faithfully:
  1. sorts with *extensional* (data-defined) semantics;
  2. **partial vs. total functions** with declared presence (§5);
  3. **variants** for oneof (§7.3);
  4. **bounded integer sorts** for the checked-native numeric family (§6) — protobuf is honest about integer width, and grounding cares; let the type system say so;
  5. **overloading resolved by subject sort** for shared field names (§4.2);
  6. **set-valued fields** for `(keryx.set)` repeateds — where the dialect's first-class sets earn their keep;
  7. **relational accessor sugar**, bidirectional: `home(P, A)` in heads desugaring to occupancy-plus-path, resolving the read/write spelling asymmetry at the language level rather than in codegen.
  Where the dialect lacks a row, the target degrades to the clingo encoding for that construct and says so; the gap list is the point.

---

## Part VII — Toolchain

### 25. CLI surface

```
keryx gen       [--target clingo|<typed-dialect>] [--profile clingcon]
                [--overlay X.keryx.toml]... [--shape strict|diagnostic|both]
                <spec.proto|spec.binpb> -o DIR
keryx explain   <spec> [fq.path.or.Field]        # per-field verdicts + annotation prompts
keryx facts     --root Type=payload.(binpb|json|txtpb) <spec>        # shred to .lp, stdout
keryx emit      [--out binpb|json|txtpb] [--root Type] <answer.lp> <spec>   # reassemble, stdout
keryx scaffold  --emit Type <spec>                                    # lowering skeleton
# keryx solve … --emit …  — retired (R4): keryx invokes no solver; keryx emit is the outbound
#                           command, run over the consuming tool's own solver output
keryx diff      <old.proto> <new.proto>          # migration notes; optional bridge views (--bridge)
keryx check     <model.lp> <gen-dir>             # lint: signature conformance, house style
                                                 # (gated on the parsing provider, §18)
protoc-gen-keryx                                  # plugin shim (same gen pipeline)
```

`explain` is keryx lore delivered at point of use instead of documentation: what each field maps to, why, and where an annotation would change semantics ("`tags` is repeated: treated as ordered; if order is incidental, mark it `set = true`"). `facts` is the single fastest way to learn a generated vocabulary — watching real data become atoms — and makes the decode inspectable rather than trusted (P1, legible rightward). As of the inbound codec, `facts` reads `.binpb`, `.txtpb`, and `.json` payloads, the format named by the extension from one table (§26). `emit` is the outbound command — the answer-set → message half — that replaces the retired `keryx solve --emit` (R4): it reassembles a `.lp` answer set to exactly one message on stdout, `--out` naming the wire form (binary, text, or JSON) and `--root Type` selecting which message when the answer set names more than one root — distinct from `facts`'s `--root Type=payload`, which names the payload to shred. keryx runs no solver; the answer set is the consuming tool's own solver's output (§18, R4). `diff` is the evolution instrument (§13.4, §27): it regenerates both mappings from the old and new `.proto` and compares them by protobuf identity — the migration report on stdout, the changeset as JSON under `--json`, and the bridge views written to the file `--bridge` names; no manifest is read.

### 26. Interchange and structured failure

- Every payload entry point accepts and emits **binary, canonical JSON, and textproto** interchangeably (the dynamic descriptor layer provides all three), so services are curl-able and fixtures are human-writable. Interchangeably under one admission policy, too: the §8 nesting ceiling is uniform across the three — for textproto bound ahead of the engine's parser, which bounds nothing of its own; for JSON beneath the deserializer's own nesting count, which needs no bound ahead of it. (Inbound, all three are built, and the three-way parity is exact on this specification's own payload: the committed example's binary, text, and JSON forms shred to one golden, and a chain of singular message fields is admitted to the same 99 levels in every form. Two documented departures in what is *admitted*, each an over-refusal and never the reverse: map-of-message and expanded-`Any` nesting binds conservatively earlier in textproto, since the pre-parse guard counts message-value openers and those two forms spend openers the walk does not count — about 49 map-of-message levels against the wire's 99 — a settled consequence of bounding the text parser lexically; and repeated-message and map-of-message nesting binds earlier in JSON, since the deserializer counts arrays and objects and those forms spend two a level — about 63 levels against 99 — a settled consequence of the deserializer bounding itself by container rather than by occupant. And one asymmetry of reading, not of content: an empty payload is the empty message on the wire and as text, and no JSON value at all — refused; the JSON form's empty message is `{}`. Outbound, all three encoders are built as of Increment 4, and the three-way round-trip parity is exact: the thermal batch reassembles to each form and shreds back to one golden, and every map's entries are ordered by key in each form — the binary wire re-sort, the textproto sorter, and serde_json's ordered map. Exact for every message all three forms can represent; one documented departure outbound, an over-refusal never the reverse: a well-known-type value canonical JSON cannot represent — an out-of-range `Timestamp`/`Duration`, or an `Any` whose `type_url` the pool cannot resolve — is refused in the JSON form alone (`UnrepresentableJson`, §10), where the binary and text forms carry it structurally; keryx keeps well-known types opaque and does not range-validate or resolve one at reassembly, so the model stays symmetric across the forms.)
- Failures are **structure, not logs**: translation errors, range violations, open-enum rejections, and shape diagnostics travel as data — in the envelope's `Result` for solve paths, as JSON diagnostics for CLI paths — always naming **field paths, not atoms** on the message-facing side. A non-ASP consumer gets machine-readable *why*; exit codes distinguish a schema violation from a translation error (the CLI's `Exit::Schema` and `Exit::Translation`; domain-UNSAT is the consuming tool's, architecture R4).
- Determinism guarantees restated as testable properties: identical payload ⇒ identical facts; identical answer set ⇒ identical bytes (canonical set ordering, canonical map ordering, sequence indices).

### 27. Evolution and testing

- **Evolution flow is one-directional through the fence:** spec-side churn surfaces as compiler guidance. A `deprecated = true` field option becomes a model-side lint warning; a rename diffs (`keryx diff`, which regenerates both mappings from the old and new `.proto` and compares the two generated mappings by protobuf identity — no manifest read) into a migration note plus optional generated bridge view; a field-number change is flagged as the wire-breaking act it is (and buf users get the same from `buf breaking` — the two checks are complementary, `keryx diff` guarding the ASP side, buf the wire side).
- **Fixture harness.** A directory convention: `fixtures/<name>/{request.txtpb, expect.txtpb | expect.lp | contract.lp}`. The driver shreds the request, solves against the model, and checks the expectation — either exact envelope comparison or an **ASP contract** over the shared vocabulary (`@sat`, `@model`, `@cautious`, cost/optimality tags; solver declared in the contract per the contract's rule). Scenario corpora thus double as regression suites and as documentation-by-example.
- **Compiler self-tests:** golden descriptor sets → golden module sets and manifests; stage-1 policy under ASP contracts (§21.3); the §21.2 self-application cross-check; property tests for codec round-trips — payload → facts → payload identity on canonical forms, and answer set → payload → facts identity, in all three output forms — with the reassembler's totality (over arbitrary answer sets in every form and arbitrary `.lp` text), depth, determinism, and shape-refusal suites.

---

## Part VIII — Worked Stories

Three end-to-end narratives, in increasing depth. They are normative illustrations: where a story and Parts II–VII disagree, the parts win and the story is a bug.

### 28. Story 1 — thermal watch (flat messages, one-shot, local files)

```proto
// thermal.proto            syntax = "proto3"; package thermal.v1;
import "keryx/options.proto";
message Reading      { string sensor = 1; int32 temp_c = 2; }
message ReadingBatch { repeated Reading readings = 1; }

message Alert    { string sensor = 1; int32 temp_c = 2; }
message AlertSet { repeated Alert alerts = 1 [(keryx.set) = true]; }
```

```
$ keryx gen thermal.proto -o gen/
  gen/thermal.v1.core.lp   gen/thermal.v1.views.lp
  gen/thermal.v1.emit.lp   gen/thermal.v1.keryx-manifest
```

Core signature (comments in the clingo target):

```prolog
% thermal.v1.core — signature
%   sort reading_batch/1, reading/1, alert_set/1, alert/1
%   readings : reading_batch × index -> reading   (sequence)
%   sensor   : reading -> string   (total)    temp_c : reading -> int32 (total)
%   alerts   : alert_set -> alert  (set)
```

A payload `{readings:[{sensor:"s-101",temp_c:44},{sensor:"s-107",temp_c:21}]}` shreds to (root from the CLI):

```prolog
reading_batch(r0).
reading(readings(r0,0)).  sensor(readings(r0,0),"s-101").  temp_c(readings(r0,0),44).
reading(readings(r0,1)).  sensor(readings(r0,1),"s-107").  temp_c(readings(r0,1),21).
```

Both fields are IMPLICIT-presence: atoms always exist, the model treats them as total. The entire authored model:

```prolog
overheating(R) :- reading(R), temp_c(R,T), T >= 40.

alert(al(R))     :- overheating(R).          % provenance term — no minted identity
sensor(al(R),S)  :- alert(al(R)), sensor(R,S).
temp_c(al(R),T)  :- alert(al(R)), temp_c(R,T).
alert_set(out).  alerts(out,A) :- alert(A).  emit_alert_set(out).
```

Emit module (excerpt) guarding outbound obligations behind reachability:

```prolog
reach(X) :- emit_alert_set(X).
reach(A) :- reach(X), alert_set(X), alerts(X,A).
:- reach(A), alert(A), #count{ S : sensor(A,S) } != 1.
```

```
$ keryx solve --root ReadingBatch=batch.binpb --emit AlertSet model.lp gen/
← SolveResponse{ result: SAT, models: [ { alert_sets: [
     AlertSet{ alerts: [ {sensor:"s-101", temp_c:44} ] } ] } ] }
```

`alerts` is set-annotated, so elements serialize in clingo symbol order: identical answer sets ⇒ identical bytes. Two manifest lines, because this is the evolution contract:

```
thermal.v1.Reading   #2  temp_c  -> temp_c/2   int32  implicit  total
thermal.v1.AlertSet  #1  alerts  -> alerts/2   set<alert>
```

Rename `temp_c` in the `.proto` and `keryx diff` reports a migration (optionally emitting the bridge view) instead of a silent fork.

### 29. Story 2 — dispatch planning (nesting, sets vs. sequences, oneof, absence, aggregates, choice)

```proto
// dispatch.proto           syntax = "proto3"; package dispatch.v1;
import "keryx/options.proto";
message PlanRequest { Fleet fleet = 1; repeated Shipment shipments = 2; }
message Fleet   { repeated Truck trucks = 1; }
message Truck   { string id = 1; int32 capacity_kg = 2; }
message Address { string city = 1; }
message Item    { string sku = 1; int32 weight_kg = 2; }
message Shipment {
  string id = 1;
  Address dest = 2;                                  // message field: explicit presence
  repeated Item items = 3;                           // sequence: indexed
  repeated string tags = 4 [(keryx.set) = true];     // set semantics
  optional int32 priority = 5 [(keryx.default) = 3]; // explicit presence, default
  oneof handoff { string dock = 6; string locker = 7; }
}
```

One shipment's shred:

```prolog
plan_request(r0).
fleet(fleet(r0)).
truck(trucks(fleet(r0),0)).      id(trucks(fleet(r0),0),"t-1").
capacity_kg(trucks(fleet(r0),0), 800).
shipment(shipments(r0,0)).       id(shipments(r0,0),"s-17").
address(dest(shipments(r0,0))).  city(dest(shipments(r0,0)),"omaha").
item(items(shipments(r0,0),0)).  sku(items(shipments(r0,0),0),"K-9").
weight_kg(items(shipments(r0,0),0), 12).
tags(shipments(r0,0), "fragile").      % set: binary, no index
dock(shipments(r0,0), "d-4").          % oneof arm present
                                       % priority unset -> no atom
```

Nesting lives inside the terms; the model quantifies sort-wise and never spells a path unless deliberately reaching through structure. Iteration over containment uses the generated views (`items(S,I,E) :- item(E), E = items(S,I).`), and the declared default has already produced `priority_or_default/2` in `views.lp` — the hand-written two-rule default idiom does not appear in the model:

```prolog
load(S,W) :- shipment(S), W = #sum{ Wk,I : items(S,I,E), weight_kg(E,Wk) }.

1 { assigned(S,T) : truck(T) } 1 :- shipment(S).
:- truck(T), capacity_kg(T,C), #sum{ W,S : assigned(S,T), load(S,W) } > C.
:- shipment(S), tags(S,"fragile"), locker(S,_).   % oneof arm = ordinary partial fn
rush(S) :- shipment(S), priority_or_default(S,P), P <= 1.
```

Response side (`Plan { repeated Assignment assignments = 1 [(keryx.set)=true]; }`, `Assignment { string shipment_id = 1; string truck_id = 2; }`) — five mechanical lines, which is why `keryx scaffold` exists (§14):

```prolog
assignment(asg(S))     :- assigned(S,_).
shipment_id(asg(S),I)  :- assignment(asg(S)), id(S,I).
truck_id(asg(S),I)     :- assigned(S,T), id(T,I).
plan(p).  assignments(p,A) :- assignment(A).  emit_plan(p).
```

The shape guard earning its keep: drop the `1{…}1` bounds to `{…}` during a refactor and a model can assign one shipment two trucks; `truck_id(asg(S),·)` doubles; in **strict** mode the functionality constraint eliminates the candidate — a malformed wire message becomes "no such answer," never garbage bytes; in **diagnostic** mode the model survives and the reassembler reports `violates` at the field path `dispatch.v1.Assignment.truck_id`. Run `-n 3` and the envelope simply carries three `Plan`s.

### 30. Story 3 — diagnosis service (episodic, networked, streaming facts)

```proto
// diag.proto — request side          syntax = "proto3"; package diag.v1;
import "keryx/options.proto";
message ObservationBatch {
  int64 tick = 1 [(keryx.numeric) = NATIVE_CHECKED];   // default would be decimal string
  repeated Observation obs = 2 [(keryx.set) = true];
}
message Observation { string point = 1; Signal value = 2; }
enum Signal {
  option (keryx.zero) = ABSENT;        // wire's pseudo-null becomes honest partiality
  SIGNAL_UNSPECIFIED = 0; SIGNAL_LOW = 1; SIGNAL_HIGH = 2;
}
// response side
message Diagnosis { repeated string abnormal = 1 [(keryx.set) = true]; }
```

(Enum constants emit prefix-stripped: `low`, `high`. `value = SIGNAL_UNSPECIFIED` on the wire now shreds to *no* `value` atom — the author declared zero to mean unset; the tool could not have guessed.)

Resident model — classic consistency-based diagnosis; system description elided:

```prolog
{ ab(C) : component(C) }.
expected(P,V) :- ...                       % system description
:- value(O,V), point(O,P), expected(P,W), V != W, not excused(P).
excused(P) :- observes(C,P), ab(C).
#minimize { 1,C : ab(C) }.

diagnosis(d).  abnormal(d,Id) :- ab(C), id(C,Id).  emit_diagnosis(d).
```

The service loop, per batch *k* — the pipeline is the point:

```
-> ObservationBatch{ tick:7, obs:[{point:"p2", value:SIGNAL_HIGH}] }     (31 bytes)
   decode -> symbols                  % ground by construction: no grounder runs here
   backend: register external ep(7); inject  fact <- ep(7)  per atom
   solve with assumptions { ep(7)=T, ep(6)=F, ... }        % window policy
<- SolveResponse{ result: OPTIMAL, models: [ { diagnoses:[{abnormal:["c4"]}] },
                                             { diagnoses:[{abnormal:["c9"]}] } ] }
```

The base program grounded once at startup; each batch enters through aspis straight into the backend — no text, no parse, no grounding on the request path. Episode policy is assumption policy and nothing else: sliding window, cumulative scenario, or what-if subsets; permanent retirement releases the external and the solver may simplify. Brave/cautious diagnoses are envelope-level union/intersection over `models[]`, computed without touching the solver. On UNSAT, the unsat core over episode guards names *which batches* jointly broke consistency — diagnosis of the diagnosis service, free.

---

## Part IX — Plan

### 31. Milestones

Ordered for local development; each milestone leaves the workspace green and demonstrable.

- **M0 — Ingestion + facts.** `keryx-core::descriptor` over `prost-reflect` (dynamic-layer rule enforced by construction); de-sugaring; schema model; hand-written stage 0; golden tests on fixture descriptor sets (maps, proto3-optional, oneofs, recursion, custom options via a vendored `keryx/options.proto`; editions carry a refusal test, not a golden — deferred per M1). Deliverable: internal schema-facts dump command.
- **M1 — gen.** Stage-1 policy `.lp` + evaluator (aspis via driver); stage-2 emission of `core/views/manifest` for the clingo target through the internal emission backend; embedded protox front door (`keryx gen foo.proto`) with the **editions verification gate**: while the descriptor engine has no editions support, `gen` refuses editions files — both the `.proto` and descriptor-set routes — with a specific diagnostic and says so. `keryx explain` (mapping verdicts). Self-application cross-check (§21.2).
- **M2 — Inbound + one-shot solve.** Codec inbound (binary/JSON/textproto → `Sym` atoms → `.lp`); `keryx facts`; `keryx solve` one-shot with text-include fact path (temporary, flagged); envelope with SAT/UNSAT/stats.
- **M3 — Outbound.** `emit.lp` generation (strict + diagnostic); reachable-subgraph reassembler; canonical serialization; `--emit`; envelope models; structured shape diagnostics at field paths.
- **M4 — Annotations + overlays.** Full Appendix A vocabulary; TOML overlays with precedence + typo errors; scalar policies enforced end-to-end (float mandatory-annotation error with fix-it; NATIVE_CHECKED ranges; open-enum policy; zero-as-absent incl. unary bool); `keryx diff` migration notes + bridge views.
- **M5 — Episodic.** Driver episodic API on aspis backend (externals, assumptions, release); the P10 fact path replaces M2's text include everywhere; brave/cautious envelope ops; unsat-core episode blame; minimal CLI exposure (scripted episode files for fixtures).
- **M6 — Ring.** `keryx scaffold`; fixture harness with ASP contracts; `keryx check` lint *if* the parsing provider (themelios or successor decision) is available — otherwise explicitly deferred, not faked.
- **M7 — Targets + plugins.** `--profile clingcon` (`&dom`/`&sum` lowering); `protoc-gen-keryx` shim with editions handshake, verified against protoc and buf; `--target <typed-dialect>` to the extent the dialect exists, emitting the degradation report (§24) for missing rows.

### 32. Open questions

Tracked here so refinement sessions burn them down deliberately.

1. `(keryx.reify)` design (§8) — must clear P4 (additive) before shipping; interim answer is "explicit ID fields."
2. `(keryx.mirror)` echo sugar (§14) — worth its concept count?
3. Oneof discriminator view (§7.3) — wait for demand.
4. Timestamp/Duration convenience lowerings (§10) — additive-only if ever.
5. `Any` registry ergonomics (§10) — needs a motivating consumer.
6. Envelope customization (§12.4): user-supplied envelopes vs. generated-only; single-payload bypass flag semantics.
7. Manifest wire format (Appendix B): stay text, or dual text+binary (it is itself describable as a proto — pleasingly, by keryx).
8. Totalized-default view naming: `f_or_default` (current) vs. suffix conventions — bikeshed, decide once, record in manifest.
9. Emission/parsing provider timeline: themelios vs. the aspis-adjacent syntax effort — keryx tracks the family decision; the trait boundary (§18) exists so this never blocks.
10. Stage-1 policy evaluation without aspis (vendored clingo shell-out) — needed by any solverless `keryx-core` consumer?
11. Static per-spec codec codegen (§21.4) — profile first, generate later.
12. The name.

### 33. Deferred and out of scope (restated)

Networking/transport/service (a downstream consumer's concern); function-free targets; non-protobuf IDLs; gRPC service sections; editor/LSP duties. Lazy-grounding interactions (the non-CDCL solver line) are horizon-only: if that solver ever hosts keryx episodes, its design enters this spec explicitly at that time.

### 34. Editorial notes — consolidations made in drafting

Flagged for first-pass review, since these regularize or pin choices the design conversation left as sketches:

1. **Option namespace and consolidation.** All annotations live under `(keryx.*)`; the sketched `(asp.int64)=CHECKED_NATIVE` and `(asp.enum_zero)=ABSENT` are consolidated into the general `(keryx.numeric)` (all integral types, three policies) and `(keryx.zero)` (all zero-defaultable IMPLICIT fields incl. bool and enums). Protobuf's flat extension-symbol namespace cannot carry two `keryx.zero` extensions, so the *field*-targeted extension is encoded `zero_field` (FieldOptions, 50105) and the *enum*-targeted one stays `zero` (EnumOptions, 50141); the conceptual annotation and the overlay-TOML key remain `zero`, and the field-site inline spelling `(keryx.zero_field)` and its semantics are settled with annotation reading at M4. The unary-bool encoding is recovered as `(keryx.zero)=ABSENT` on a bool.
2. **Float policy pinned** to annotation-mandatory with a two-choice fix-it error (scale vs. opaque) — the one deliberate exception to G4.
3. **Set honesty clause** (§7.1): `(keryx.set)` changes relation shape and serialization order only; identity collapse for message elements additionally requires `(keryx.value)`. The conversation's stories implied but never stated this.
4. **Episodic guards** (backend-registered externals, release-to-retire) — retired under the translation-only scope; episodic serving is the consuming tool's, not keryx's.
5. **Emission boundary** (§18): all ASP output constructed as syntax values behind a builder/printer trait; themelios is the preferred and intended-final provider, with a minimal internal fallback so keryx ships on its own schedule.
6. **Overlay format pinned** to TOML keyed by fully-qualified paths, overlay-wins precedence, unmatched-key errors.
7. **Bytes canonical form pinned** to lowercase hex (base64 rejected for case/padding ambiguity).
8. **Milestone order** (§31) is a proposal, shaped so every milestone is demonstrable and the P10 principle lands at M5 without vocabulary change.
9. **Qualifier rule pinned** (§4.2). "Minimize total qualifier segments" is realized as the unique, symmetric rule §4.2's own example shows — every member of a name collision is qualified to the shortest common path-suffix depth that separates them — because leaving one member bare is non-unique and would violate P3 (a deterministic vocabulary). The prose objective and the example are reconciled in favour of the example.

---

## Appendix A — `keryx/options.proto` (draft)

```proto
syntax = "proto2";
package keryx;
import "google/protobuf/descriptor.proto";

enum NumericPolicy { NUMERIC_POLICY_UNSPECIFIED = 0; NATIVE_CHECKED = 1;
                     DECIMAL_STRING = 2; CLINGCON = 3; }
enum ZeroPolicy    { ZERO_POLICY_UNSPECIFIED = 0; VALUE = 1; ABSENT = 2; }
enum UnknownPolicy { UNKNOWN_POLICY_UNSPECIFIED = 0; REJECT = 1; PRESERVE = 2; }

extend google.protobuf.FieldOptions {
  bool          set      = 50101;   // repeated: order/multiplicity not meaningful
  NumericPolicy numeric  = 50102;   // integral fields & map keys (§6, §7.2)
  int32         scale    = 50103;   // float/double: fixed-point ×10^scale
  bool          opaque   = 50104;   // float/double: decimal-string constant
  ZeroPolicy    zero_field = 50105; // IMPLICIT fields: is the zero value a value? (proto symbol-namespace forces the field-level rename from `zero`; enum-level `zero` = 50141 below is kept)
  string        default  = 50106;   // rendered per field type; editions `default` preferred
  string        mirror   = 50107;   // reserved (§14, open)
}
extend google.protobuf.MessageOptions {
  bool          value    = 50121;   // value-like: same_t view; hashing where §9 allows
  string        key      = 50122;   // documents the domain key field (§9)
  repeated string any_types = 50123; // Any registry (§10)
  bool          reify    = 50124;   // reserved (§8, open — must clear P4)
}
extend google.protobuf.EnumOptions {
  ZeroPolicy    zero     = 50141;
  UnknownPolicy unknown  = 50142;
}
```

Overlay TOML keys mirror these names exactly (`set`, `numeric`, `scale`, `opaque`, `zero`, `default`, `value`, `key`, `any_types`, `unknown`), applied by fully-qualified path (§16) (`zero` is the overlay/conceptual key for both the field- and enum-level extensions; see §34.1).

## Appendix B — Manifest and envelope sketches

Manifest (line-oriented text, one record per vocabulary element; final grammar open per §32.7):

```
keryx-manifest v0
schema-hash  sha256:...        target clingo   profile -        shape both
dispatch.v1.Shipment           sort  shipment/1
dispatch.v1.Shipment.id     #1 fn    id/2         string  implicit total
dispatch.v1.Shipment.dest   #2 fn    dest/1->address       explicit partial
dispatch.v1.Shipment.items  #3 fam   items/2->item  seq    ; view items/3
dispatch.v1.Shipment.tags   #4 rel   tags/2       string  set        [inline set=true]
dispatch.v1.Shipment.priority #5 fn  priority/2   int32   explicit partial default=3
                                     ; view priority_or_default/2   [overlay default]
dispatch.v1.Shipment.handoff   oneof dock/2|locker/2  exclusive
```

Envelope (generated per package; `Result` shared from a vendored `keryx/envelope.proto`):

```proto
message Result { Status status = 1;  Stats stats = 2;  repeated Diagnostic diagnostics = 3; }
message Diagnostic { string field_path = 1; string kind = 2; string detail = 3; }
// per-package:
message Model         { repeated <Root> <roots> = 1; /* one field per exported root */ }
message SolveResponse { Result result = 1; repeated Model models = 2; }
```

## Appendix C — Stage-0 descriptor-fact vocabulary (sketch)

Definable as keryx(descriptor.proto) as of M1 (§21.2); hand-written form for M0:

```prolog
file(File, Package).                 message(Msg, File).      nested(Inner, Outer).
field(Msg, Num, Name, Type, Presence, Card).   % Type: scalar kind | msg(T) | enum(T) | map(K,V)
oneof(Msg, OneofName, Num).          enum_t(E, File, Openness).
enum_value(E, Name, Num).            opt(Path, Key, Value).   % inline + overlay, provenance-tagged
doc(Path, Text).                     recursive(T).            % containment-cycle analysis
```

## Appendix D — Terminology

- **occupant / occupancy** — the term filling a message-typed slot / the sort-membership atom asserting it (§4.1).
- **path term** — occupant named by its access path from a root; the identity scheme (§4.1, P7).
- **shred / reassemble** — inbound tree→relations / outbound relations→tree (§11, §12.3).
- **emit root** — an atom `emit_t(X)` marking a tree for export; the typed `#show` (§12.1).
- **shape** — the serializability theory; **strict/diagnostic** its two modes (§12.2).
- **episode** — one payload's facts under one backend-registered guard external (§23).
- **manifest** — the number↔name binding and evolution contract (§13.4).
- **overlay** — out-of-band annotations for borrowed specs (§16).
- **descriptor facts / mapping model** — stage-0 output / stage-1 stable model (§21).
- **physical schema vs. authored ontology** — generated vocabulary vs. the model writer's definition layer over it (P8).
