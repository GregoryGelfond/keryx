# keryx proto-version support

keryx branches on *resolved features*, never on syntax era (spec §5, §20), so
supporting a proto version is a matter of the descriptor engine resolving its
features — not of keryx logic. keryx supports every version its engine
(prost-reflect) can ingest; new editions are a drop-in as the engine gains them.
This ledger states the proto-version support keryx *delivers* at completion of
Increment 1 (M0 — ingestion) — proto2 and proto3 golden-tested by the facts
renderer, editions per the front-loaded capability verdict — not the state of
any single commit along the way.

| version       | status on completion of Increment 1 (M0 — ingestion)                                                                                                        |
|---------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------|
| proto2        | supported (golden-tested)                                                                                                                                       |
| proto3        | supported (golden-tested)                                                                                                                                       |
| edition 2023+ | DEFERRED, and refused cleanly. Neither engine handles editions at these versions: protox 0.9.1 does not *compile* an editions `.proto` (→ `UncompilableSource`), and prost-reflect 0.16.5 has no editions `Syntax` and *panics* decoding an editions descriptor set. keryx detects an editions `FileDescriptorSet` up front and refuses it with a specific `UnreadableDescriptorSet` diagnostic (§6 — total, no panic). `SchemaVersion` is `#[non_exhaustive]`, so a distinct `Edition` variant and the enum_type override are a later add, not a redesign |

**Both routes fail on editions today, and keryx says so precisely.** A measurement (protoc 36 →
an edition-2023 descriptor set → keryx) confirmed prost-reflect 0.16.5 panics building a pool from
an editions set — its `Syntax` carries only `Proto2`/`Proto3`. keryx therefore inspects a
serialized set for `syntax = "editions"` *before* handing it to the engine (`descriptor::decode`),
and returns a specific diagnostic ("editions descriptor sets … are not supported yet …
transliterate to proto3") rather than provoking the panic. This is §6 totality by construction,
not by catching a panic; the prost-reflect panic-on-editions is worth reporting upstream.

Editions support arrives when the engine does — prost-reflect gaining an editions syntax (a
deliberate dependency bump) — at which point keryx's own presence/`enum_type` logic, already
feature-based rather than era-based, resolves editions with no redesign. Spec §31's (M1)
capability test is the tripwire; when it flips to SUPPORTED, add the editions fixture and golden
and update this row.
