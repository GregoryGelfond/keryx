# keryx proto-version support

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
