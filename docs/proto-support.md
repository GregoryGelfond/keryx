# keryx proto support — versions and payload formats

keryx branches on *resolved features*, never on syntax era (spec §5, §20), so
supporting a proto version is a matter of the descriptor engine resolving its
features — not of keryx logic. keryx supports every version its engine
(prost-reflect) can ingest; new editions are a drop-in as the engine gains them.
This ledger states the proto-version support keryx *delivers* as of the gen
increment (Increment 2) — proto2 and proto3 golden-tested by the facts
renderer, editions per the front-loaded capability verdict — not the state of
any single commit along the way. The outbound direction is whole for both eras: a proto2 `required` field's
totality obligation is emitted and enforced (stated after the table) — full
proto2/proto3 parity.

| version       | status as of the gen increment (Increment 2)                                                                                                                |
|---------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------|
| proto2        | supported (golden-tested)                                                                                                                                       |
| proto3        | supported (golden-tested)                                                                                                                                       |
| edition 2023+ | DEFERRED, and refused cleanly. Neither engine handles editions at these versions: protox 0.9.1 does not *compile* an editions `.proto` (→ `UncompilableSource`), and prost-reflect 0.16.5 has no editions `Syntax` and *panics* decoding an editions descriptor set. keryx detects an editions `FileDescriptorSet` up front and refuses each editions file with a specific `UnsupportedEdition` diagnostic at that file's locus (§6 — total, no panic). `SchemaVersion` is `#[non_exhaustive]`, so a distinct `Edition` variant and the enum_type override are a later add, not a redesign |

**Both routes fail on editions today, and keryx says so precisely.** A measurement (protoc 36 →
an edition-2023 descriptor set → keryx) confirmed prost-reflect 0.16.5 panics building a pool from
an editions set — its `Syntax` carries only `Proto2`/`Proto3`. keryx therefore inspects a
serialized set for `syntax = "editions"` *before* handing it to the engine (`descriptor::decode`),
and returns a specific `UnsupportedEdition` diagnostic, one per editions file at that file's locus
("editions … are not supported yet: keryx's descriptor engine has no editions support, so neither
a .proto source nor a protoc-compiled descriptor set is accepted"), rather than provoking the
panic. This is §6 totality by construction: keryx pre-empts the editions panic rather than catching it. (An
*unforeseen* engine fault on a foreign-input path is the different case — *contained* as a typed
dependency fault at the descriptor door (its decode and its accessor walk), the threat model's
dependency boundary, not masked.) The
prost-reflect panic-on-editions is worth reporting upstream.

Editions support arrives when the engine does — prost-reflect gaining an editions syntax (a
deliberate dependency bump) — at which point keryx's own presence/`enum_type` logic, already
feature-based rather than era-based, resolves editions with no redesign. Spec §31's (M1)
capability test is the tripwire; when it flips to SUPPORTED, add the editions fixture and golden
and update this row.

**Outbound, a proto2 `required` field's totality obligation is emitted and enforced — full
proto2/proto3 parity.** The specification has the serializability theory oblige totality of every
IMPLICIT and `LEGACY_REQUIRED` singular field (spec §5, §12.2). The mapping's totality carries the
distinction: `Totality::Required` for a proto2 `required` field, `Total` for IMPLICIT, `Partial`
for EXPLICIT. So `emit.lp` emits the totality obligation for a `required` field exactly as for an
IMPLICIT one — strict: UNSAT on an answer set that omits it; diagnostic: `violates` derived at the
field's path — and the reassembler, reading the same mapping, refuses an answer set that omits a
`required` field with a `ShapeViolation`, so no incomplete proto2 message is produced: enforced at
solve time and at reassembly alike. A `required` field's presence is still read from the message
inbound (partial, like `optional`); only the outbound obligation distinguishes it. proto3 has no
`required`, and proto2's `required` is now enforced outbound, so both eras have whole outbound
support — for interconnect preservation and inbound↔outbound symmetry.

## Enum openness and unknown values

keryx branches on an enum's **resolved openness**, never on syntax era (§5, §7.4). A proto3 enum is **open** — a wire value naming no declared constant is legal — and `(keryx.unknown) = PRESERVE` carries such a value through translation rather than refusing it: inbound, an unknown number `N` shreds to `unknown(N)`; outbound, `unknown(N)` re-encodes to `N`; without the annotation an unknown value is a structured refusal (`UnknownEnumValue`). A proto2 enum is **closed** — an unknown wire value is an error — so `PRESERVE` is inapplicable there (a mis-target diagnostic at the policy door). The inverse override — declaring a proto3 enum *closed* through the editions `enum_type = CLOSED` feature — is **deferred** with the rest of editions support (above): a resolved-feature drop-in when the descriptor engine gains editions, not a redesign.

## Payload formats

The inbound codec (`Codec::shred`; `keryx facts`) accepts a payload in each wire form spec §26
names — binary, canonical JSON, textproto — through one `Codec`, one walk, and one admission
policy: the same §6 refusals at the same field paths, the same uniform nesting ceiling of **99**
message-typed levels below the root, and the same facts from the same message in any of the three
forms. Where the three differ is in what each *admits* on the way to the walk, and in how its
decoder's own bound meets the ceiling — stated per form below. This ledger states what keryx
*delivers* as of the inbound codec (Increment 3).

| format                  | status as of the inbound codec (Increment 3)                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
|-------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| binary (`.binpb`)       | supported (golden-tested on the thermal example). Compositional nesting is admitted to **99** message-typed levels below the root — spec §8's door-admission policy, one below the descriptor engine's decode recursion limit; a payload nesting deeper is refused whole — `PayloadTooDeep` from the walk at the 100th level, `UndecodablePayload` from the engine's own limit past it. Every §6 refusal is a diagnostic at its field's path; bytes that do not decode as the root type are `UndecodablePayload` |
| canonical JSON (`.json`) | supported (golden-tested on the thermal example: `batch.json` shreds to the facts `batch.binpb` does, three-way with `batch.txtpb`). The same `Codec`, walk, and §6 refusals, under the same ceiling of **99** — with no bound of keryx's ahead of the engine: the deserializer bounds its own nesting, counting arrays and objects (127 nested containers admitted, the 128th refused), and the walk's counter binds beneath that count for a chain of singular message fields, where a repeated or map-of-message chain meets the count first, at about 63 levels (below); either way a deeper payload is refused whole. A payload that is not one canonical JSON value of the root type — not UTF-8, a member the type does not declare, a value outside its kind, text after the value, or the empty text (the empty message is `{}`) — is `UndecodablePayload` |
| textproto (`.txtpb`)    | supported (golden-tested on the thermal example: `batch.txtpb` shreds to the facts `batch.binpb` does). The same `Codec`, walk, and §6 refusals, under the same ceiling of **99** — bound *ahead* of the engine's text parser by a pre-parse guard that counts message values, so a map entry or an expanded `Any` spends more of it than on the wire (below); a deeper payload is refused whole, `PayloadTooDeep`. Text that is not UTF-8, or does not parse as the root type, is `UndecodablePayload`                   |

**Textproto's ceiling is counted in message values, the wire's in occupants.** The engine's text
parser recurses natively on every nested message value and bounds nothing, so keryx bounds a text
payload *before* it: a pre-parse guard measures the text's `{ }`/`< >` nesting (outside string
literals and `#` comments) and refuses past 99 whole — `PayloadTooDeep`, naming the depth and the
ceiling and nothing of the text — before the parser sees a token; the parse then runs on a thread
keryx sizes for the deepest admitted payload (8 MiB), so no admitted payload overflows whatever
thread the caller decodes on. The measure is exact for a singular or repeated message field (one
opener, one occupant), so such a payload is admitted exactly as deep as its binary form; it is
conservative for a map entry (two openers per occupant — the entry's and its value's) and an
expanded `Any` (an opener the walk never enters, plus whatever it nests), which bind earlier in
text than on the wire — a map-of-message chain is admitted to 49 levels as text where its wire
form is admitted to 99 — and never later: the guard over-refuses, and never admits deeper than the
walk would. Settled, a documented consequence of bounding the text parser lexically.

**JSON's ceiling is counted in containers by its deserializer, the wire's in occupants.** The JSON
form has no guard: the deserializer keryx drives the engine's JSON mapping with bounds its own
nesting — its recursion limit, on by default and never lifted, counts arrays and objects and refuses
the 128th nested one — and the engine's mapping recurses natively beneath that count with no counter
of its own, so the whole admitted payload (127 nested containers at most) is deserialized on a thread
keryx sizes for that admit (8 MiB, `engine::JSON_DECODE_STACK`; the decode pays a thread spawn per
payload, as the text parse does, the stack reserved address space committed as the deserialization
descends), and the walk then applies the uniform ceiling. Which of the two binds goes by the
field's form. A chain of singular message fields spends one object a level and reaches the ceiling
inside the count: 99 levels shred whole, to the facts the same chain shreds to from the wire; 100
through 126 — every depth the deserializer admits past the ceiling — are the walk's
`PayloadTooDeep`; 127 is the deserializer's own refusal, `UndecodablePayload`. A chain of repeated
message fields spends an array and an object a level and meets the count first: 63 levels shred
whole, to the facts the same chain shreds to from the wire and from text, and 64 — which the wire
admits — is `UndecodablePayload`, 35 levels short of the ceiling. A map-of-message chain spends the
map's object and the value's object a level and so meets the count at about the same depth, 63, by
the same arithmetic; and a chain through `google.protobuf.Value`, `Struct`, or `ListValue` binds
earlier still, at about 50 levels, at the engine's own message-decode limit as it materialises the
dynamic value. Those two bindings are documented from the engine's measure of record, not
instrumented: no fixture of the suite nests a recursive chain of maps of messages — its one map of
messages, `maps.proto`'s `Inventory.items`, is a single level — or declares a `Value`. Every one is a
refusal in the safe direction — a shallower message depth than the ceiling, never admission past it
— and the mechanism is not the text form's: there a lexical guard over-counts openers; here keryx
scans nothing, and the deserializer counts containers. One asymmetry of reading, not of content: an
empty payload is the empty message on the wire and as text, and no JSON value at all —
`UndecodablePayload` for the JSON form, whose empty message is `{}`. Settled, a documented
consequence of the deserializer bounding itself by container rather than by occupant.

The codec has no proto-version branch of its own: presence is decided from the mapping's totality
(spec §5), which the descriptor door resolves from features, never from syntax era.

## Output formats

The outbound codec (`Codec::reassemble`; `keryx emit`) writes a reassembled message in each of the
same three wire forms — binary (`--out binpb`), the protobuf text format (`--out txtpb`), and the
canonical JSON mapping (`--out json`) — through one reassembly walk and one encode adapter, the
inverse of the inbound read, as of the outbound codec (Increment 4). The three-way round-trip parity
is exact and golden-tested: the thermal `ReadingBatch` reassembles to each form and shreds back to
one fact set, and the composite forms — scalar and message-valued maps, enums, singular and repeated
fields — round-trip in every form too (`tests/codec_roundtrip.rs`, `tests/codec_emit_shapes.rs`) — exact for every message all three forms can represent, the one class canonical JSON cannot documented below. A
map's entries are ordered by key in each form, so identical answer sets yield identical bytes (spec
§12.3): the binary wire re-sort (`codec::canonical`), the textproto sorter (`codec::canonical_text`),
and serde_json's key-ordered map. `keryx emit` writes exactly one message to stdout; `--root Type`
selects which when the answer set names more than one root (§25). The proto2 `required` totality obligation (above) is enforced at reassembly as at
solve time, and the three output forms carry it identically — no proto-version asymmetry remains.

**One documented limit of `--out json`: a well-known type it cannot represent.** Canonical JSON
mandates the resolved, range-validated form of a `Timestamp`, `Duration`, or `Any`, with no raw
fallback — where the binary and text forms carry the two structural fields as they stand. keryx keeps
well-known types opaque and structural on both ends (§10) and does not range-validate or resolve one
at reassembly (that would special-case the model), so a value the reassembler admits but canonical
JSON cannot represent — an out-of-range `Timestamp` or `Duration`, or an `Any` whose `type_url` the
schema's pool cannot resolve — is refused at the JSON encode with `UnrepresentableJson`, naming the
forms that carry it (`--out binpb`/`txtpb`), rather than emitted as a non-conforming document. The
model stays symmetric: each output form refuses only what it alone cannot represent (the outbound
counterpart of the inbound dialect's control-character refusal, `UnrepresentableText`), and the
round-trip parity above is exact for every message all three forms can represent
(`tests/codec_emit_wkt.rs`).

**The thermal example's outbound story closes both round trips.** The worked example closes the
`ReadingBatch` round trip solver-free — `examples/thermal/batch.binpb` → facts →
`examples/thermal/answer.lp` (its facts under the `emit_reading_batch(r0)` marker) → `keryx emit` →
`examples/thermal/batch.reassembled.binpb`, byte-for-byte the payload again (Increment 4). The §28
story's *alert* half — an `AlertSet` of alerts emitted from a batch — closes at Increment 5, when
`(keryx.set)` gains meaning: `AlertSet.alerts` is generated as a **membership relation**, not a
sequence, so a natural `overheating` model that names its alerts by their own provenance
(`alerts(out, al(R))`, not the dense indices a sequence needs) reassembles, and `keryx emit` closes
the `AlertSet` round trip byte-for-byte, its members in clingo's total symbol order (§7.1).

## Schema evolution

The evolution instrument (`keryx diff`; spec §13.4, §27) compares two versions of one schema on
the model side — what a model written against the old vocabulary survives, what it does not, and
the bridge that carries it across the one kind of change a rule can alias — as of the evolution
instrument (Increment 6). Both sides come in through the doors this ledger describes, so the
proto-version support above is the instrument's too: a proto2 side and a proto3 side compare
alike (presence is read from each mapping's totality, never from the syntax era), and an editions
file is refused on either side with the same `UnsupportedEdition` diagnostic.

**Both mappings are regenerated; no manifest is read.** `keryx diff <old> <new>` compiles each
side through the descriptor door `keryx gen` uses — a `.proto` source (with `-I` include roots,
one list shared by both sides) or a `protoc`-compiled descriptor set (`.binpb`) — maps each as
`gen` would, annotations included, and compares the two mappings. The manifest `gen` writes is a
per-version record, never an input: the comparison sees exactly the vocabulary each side generates
today, and nothing has to be kept in step with a stored file.

**Matching is by protobuf identity, with the version stripped.** Two versions of one schema are
two packages under buf's convention (`thermal.v1`, `thermal.v2`), so a package is matched by its
name with the trailing version segment removed, a message or enum by its path within that package,
and a field or enum value by its number. Names are free to change, so a renamed field is reported
as a rename rather than as a removal beside an addition; a renumbered field is the reverse — a
removal beside an addition, the wire-breaking act it is. Two schemas with no package in common
once normalized are refused as not comparable (`NoComparableSchemas`, a usage error, no report),
and a side that declares two versions of one package is refused as ambiguous
(`AmbiguousVersionPackages`).

**Only subject vocabulary is compared, and the door decides the subject.** What a side *is*
depends on how it came in:

- through the `.proto` door, a side is the one file named on the command line; what it imports is
  referent closure — resolved, and translated where a field references it, but not compared;
- through the `.binpb` door, a side is every file in the set except the well-known types
  (`google/protobuf/*`) and keryx's option registry (`keryx/options.proto`).

Both sides go through one door. A `.proto` on one side and a `.binpb` on the other would scope
the two sides differently — one file against a whole set — so the mix is refused before either
side is read, a usage error naming which side is which, rather than reported as a page of
spurious additions and removals.

**A schema of several files goes in as descriptor sets.** Compile each revision with
`--include_imports` to its own self-contained set, and diff the sets:

```sh
protoc -I v1 --include_imports --descriptor_set_out=v1.binpb v1/station.proto
protoc -I v2 --include_imports --descriptor_set_out=v2.binpb v2/station.proto
keryx diff v1.binpb v2.binpb
```

The comparison then spans every file of the schema, package by package; a package on one side
only rides as a whole addition or removal, its elements as rows of their own.

**Two revisions sharing a file name take that route, or distinct names.** The `.proto` door
reduces the path it is given to a name relative to an include root (`v1/station.proto` under
`-I v1` is `station.proto`) and opens that name through the include roots in order — protoc's
own rule. So `keryx diff v1/station.proto v2/station.proto -I v1 -I v2` reduces *both* sides to
`station.proto`, and both open under `v1`, the first root that holds it: the same file is loaded
twice, and the report is a version diffed against itself — `no changes`, exit `0`. The banner
exposes it: both sides show the same package (`station.v1  →  station.v1`), where two revisions
would show the bump. For two revisions of one file name, compile each to its own descriptor set
against its own root, as above, or name the files distinctly (`thermal-v1.proto`,
`thermal-v2.proto`), as the worked example does.

**The bridge is inbound-facing, and it aliases a predicate, never a term.** For a clean rename — a
field, message, or enum whose predicate changed with its identity and shape intact (the same
number or path, arity, type, presence, form, and oneof membership) — `--bridge <path>` writes one
rule, `old(…) :- new(…).`, under a `%!` provenance line: a generated module like any other, and
loaded beside the new facts it lets a model written against the old vocabulary read them without
a line of it changing. Two limits, stated on the row rather than papered over. The bridge lets an
old model *read* new facts; a model that *asserts* the old atoms for `keryx emit` is not shimmed —
the new theory obliges the new predicate, and the reassembler reads it. And a message-field bridge
aliases the field's relational view (`readings(P, I, E) :- samples(P, I, E).`, spec §13.2), not the
occupant term: `samples(r0, 0)` inside `reading(samples(r0, 0))` is renamed too, and no rule can
alias a term, so a model that spells the path term directly is not bridged. A renamed enum
constant has no bridge either — a constant is not a predicate — and neither has a changed form
(a `(keryx.set)` dropped turns `alerts/2` into `alerts/3`), a removal, or an enum whose openness or
`(keryx.unknown)` setting flipped: each is breaking, and the report says so on the row.

**The report, the changeset, and the exit.** The migration report on stdout groups the changes by
package, then by message or enum, then by field number, and shows what stayed beside what moved;
`--json` writes the changeset instead — one array of records in a stable order, each carrying the
change's kind, its element's path and rendered signature per side, and whether it breaks — for a
review bot or a check of your own. A breaking change is a finding, not a failure: the exit is `0`
whatever the comparison found, and `--exit-code` turns a breaking comparison into exit `9`
(diverged), the report still on stdout and the bridge file still written, for a pipeline that
should stop on one. An error on the way — a side that does not read, build, or map — keeps its
own exit class.

**Complementary to `buf breaking`.** buf guards the wire: a field renumbered, a type changed. The
protobuf contract lets a name change freely, so buf passes a rename — and on the ASP side a
predicate's identity *is* its name, so that rename silently forks the vocabulary; an annotation
dropped, which the wire never sees, changes an arity. `keryx diff` catches both. Run the two
together: buf for the wire, keryx for the model side.
