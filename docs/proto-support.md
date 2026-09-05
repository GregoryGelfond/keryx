# keryx proto support — versions and payload formats

keryx branches on *resolved features*, never on syntax era (spec §5, §20), so
supporting a proto version is a matter of the descriptor engine resolving its
features — not of keryx logic. keryx supports every version its engine
(prost-reflect) can ingest; new editions are a drop-in as the engine gains them.
This ledger states the proto-version support keryx *delivers* as of the gen
increment (Increment 2) — proto2 and proto3 golden-tested by the facts
renderer, editions per the front-loaded capability verdict — not the state of
any single commit along the way.

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

## Payload formats

The inbound codec (`Codec::shred`; `keryx facts`) is to accept a payload in each wire form spec
§26 names — binary, canonical JSON, textproto — through one `Codec`, one walk, and one admission
policy. This ledger states what keryx *delivers* as of the inbound codec (Increment 3).

| format                  | status as of the inbound codec (Increment 3)                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
|-------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| binary (`.binpb`)       | supported (golden-tested on the thermal example). Compositional nesting is admitted to **99** message-typed levels below the root — spec §8's door-admission policy, one below the descriptor engine's decode recursion limit; a payload nesting deeper is refused whole — `PayloadTooDeep` from the walk at the 100th level, `UndecodablePayload` from the engine's own limit past it. Every §6 refusal is a diagnostic at its field's path; bytes that do not decode as the root type are `UndecodablePayload` |
| canonical JSON (`.json`) | following — the same `Codec`, the same walk, the same ceiling                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
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

The codec has no proto-version branch of its own: presence is decided from the mapping's totality
(spec §5), which the descriptor door resolves from features, never from syntax era.
