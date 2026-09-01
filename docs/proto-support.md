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
| edition 2023+ | DEFERRED: protox 0.9.1 does not compile editions — revisit when it does. `SchemaVersion` is `#[non_exhaustive]`, so a distinct `Edition` variant and the enum_type override are a later add, not a redesign |

Editions verification follows spec §31's (M1) gate: the front-loaded editions capability
test is the tripwire; when it flips to SUPPORTED, add the editions fixture and
golden and update this row.
