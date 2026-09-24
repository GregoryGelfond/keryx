# The evolution example — `keryx diff`

A worked example of keryx's evolution instrument (spec §13.4, §27): the thermal schema at two
versions, and what `keryx diff` says about the move from one to the other — which changes a
model written against v1 survives, which it does not, and the **bridge** keryx writes for the
one kind of change it can carry a model across. Two smaller cases follow: a pair of schemas that
are not two versions of anything, and a multi-file schema compared as descriptor sets, package
by package.

## Why a diff of the vocabulary, not of the wire

Protobuf's contract is that field numbers are identity and names are free to change; `buf
breaking` guards that contract on the wire. ASP's contract is the opposite: a predicate's
identity *is* its name. So a rename that is wire-compatible — no protobuf consumer notices —
silently forks the ASP vocabulary: a model written against `temp_c/2` sees no `temp_c` atom
once the field is called `celsius`. And an annotation dropped, which buf never sees, can change
a predicate's arity.

`keryx diff` regenerates the mapping from each version's `.proto` and compares the two by
protobuf identity — a field by its number, a message or enum by its path, a package by its name
with the version segment stripped — so a rename is reported as a rename, with a bridge, rather
than seen as a removal beside an addition, and every change is classed by what it does to a
model: **breaking**, **bridged**, or **additive**. The two checks are complementary: buf for the
wire, keryx for the model side.

## The schemas

[`thermal-v1.proto`](thermal-v1.proto) and [`thermal-v2.proto`](thermal-v2.proto), versioned as
buf recommends — package `thermal.v1` becomes `thermal.v2`, which is how keryx knows the two are
one schema:

```proto
syntax = "proto3";
package thermal.v1;

import "keryx/options.proto";

// A single sensor reading.
message Reading      { string sensor = 1; int32 temp_c = 2; }
// A batch of readings — a sequence.
message ReadingBatch { repeated Reading readings = 1; }

// An alert raised for a reading, graded by level.
message Alert    { string sensor = 1; Level level = 2; }
// The alerts raised for a batch — a set: membership, not order.
message AlertSet { repeated Alert alerts = 1 [(keryx.set) = true]; }

// A sensor's calibration offset.
message Calibration { string sensor = 1; int32 offset = 2; }

// How serious an alert is.
enum Level {
  LEVEL_UNSPECIFIED = 0;
  LEVEL_LOW = 1;
  LEVEL_HIGH = 2;
  LEVEL_TEST = 3;
}
```

```proto
syntax = "proto3";
package thermal.v2;

import "keryx/options.proto";

// A single sensor reading: `temp_c` renamed `celsius` at its number, `humidity` added.
message Reading      { string sensor = 1; int32 celsius = 2; int32 humidity = 3; }
// A batch of readings — unchanged.
message ReadingBatch { repeated Reading readings = 1; }

// An alert raised for a reading — unchanged.
message Alert    { string sensor = 1; Level level = 2; }
// The alerts raised for a batch — an ordered log now, no longer a set.
message AlertSet { repeated Alert alerts = 1; }

// Where a sensor is installed — new in v2. (`Calibration` is gone.)
message Site { string name = 1; string region = 2; }

// How serious an alert is: `LEVEL_HIGH` renamed `LEVEL_CRITICAL` at its number, `LEVEL_TEST`
// dropped (its number reserved), `LEVEL_ADVISORY` added — and an undeclared value is carried
// rather than refused.
enum Level {
  option (keryx.unknown) = PRESERVE;
  reserved 3;
  LEVEL_UNSPECIFIED = 0;
  LEVEL_LOW = 1;
  LEVEL_CRITICAL = 2;
  LEVEL_ADVISORY = 4;
}
```

Seven kinds of change between them, chosen because each says something different to a model:
a field renamed with its shape intact (`temp_c` → `celsius`), a field added (`humidity`), a
field whose *form* changed (`alerts`, a `(keryx.set)` membership relation in v1 and a plain
sequence in v2), a message removed (`Calibration`) and one added (`Site`), and an enum with a
value renamed, a value removed, a value added, and `(keryx.unknown) = PRESERVE` switched on
(spec §7.4). Two messages — `Alert`, `ReadingBatch` — are untouched, which the report says too.

## The report

```sh
keryx diff thermal-v1.proto thermal-v2.proto -I .
```

The `.proto` sources go through keryx's own compiler (`keryx/options.proto` resolves from the
embedded registry, so `-I .` is all the include path needed), each version is mapped as
`keryx gen` would map it, and the two mappings are compared. The report is coloured on a
terminal; [`diff.txt`](diff.txt) is the same report captured plain (`--color never`, or simply
redirected):

```
━━ keryx diff ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
   thermal.v1  →  thermal.v2                               keryx 1.0.0
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

 alert · thermal.Alert                                       unchanged
 ─────────────────────────────────────────────────────────────────────

 alert_set · thermal.AlertSet
 ─────────────────────────────────────────────────────────────────────
  #1  ! changed   alerts            set → seq, /2 → /3  no bridge

 calibration · thermal.Calibration               - removed · no bridge
 ─────────────────────────────────────────────────────────────────────
  #1  - removed   sensor/2          string,total  no bridge
  #2  - removed   offset/2          int32,total   no bridge

 reading · thermal.Reading
 ─────────────────────────────────────────────────────────────────────
  #2  ~ renamed   temp_c → celsius  /2            bridge available
  #3  + added     humidity/2        int32,total

 reading_batch · thermal.ReadingBatch                        unchanged
 ─────────────────────────────────────────────────────────────────────

 site · thermal.Site                                           + added
 ─────────────────────────────────────────────────────────────────────
  #1  + added     name/2            string,total
  #2  + added     region/2          string,total

 level · thermal.Level  (open) → (open, preserve)  ! changed · no bridge
 ─────────────────────────────────────────────────────────────────────
  #2  ~ renamed   high → critical                 no bridge · constant
  #3  - removed   test                            no bridge
  #4  + added     advisory

 ─────────────────────────────────────────────────────────────────────
  7 breaking   2 removed · 1 changed · 1 message removed · 1 value removed · 1 value renamed · 1 preserve change
  1 bridged    1 renamed (inbound-facing)
  5 additive   3 added · 1 message added · 1 value added
  → rerun with --bridge <path>  to write the 1 bridge view
```

### Reading it

One block per message and per enum, headed by its sort predicate and its version-free path
(`reading · thermal.Reading`), the messages first. Beneath a header, one row per field that
changed, in **field-number order** — the gutter `#2` is the number, the identity keryx matched
the field by. What did not change is shown too: `alert` and `reading_batch` are headers marked
`unchanged`, so a reader sees what stayed beside what moved.

- **`~ renamed   temp_c → celsius  /2   bridge available`.** The field kept its number, its
  type, its arity, and its presence; only its name — and so its predicate — changed. That is the
  one change a rule can alias, and keryx offers the alias: a *bridge view* (below). The change is
  **bridged**, not breaking.
- **`! changed   alerts   set → seq, /2 → /3   no bridge`.** Dropping `(keryx.set)` turns a
  membership relation `alerts(P, E)` into a sequence view `alerts(P, I, E)` (spec §7.1): a
  different arity, a different meaning, and no rule can pretend otherwise. **Breaking**, and buf
  would never have said a word — an option changed, the wire did not.
- **`- removed   sensor/2   …   no bridge`** under `calibration`. A removed message's fields
  ride as removed rows of their own. The predicate `sensor/2` still exists in v2 — `Reading`
  and `Alert` declare it — but the *calibration* sort's `sensor` atoms are gone, and that is
  what a model reading them loses.
- **`level · thermal.Level  (open) → (open, preserve)  ! changed`.** An enum's header carries its
  own change: switching on `(keryx.unknown) = PRESERVE` admits the escape term `unknown(N)` to
  the `level` sort (spec §7.4), a vocabulary a v1 model never saw — breaking, by the same rule
  that makes a removal breaking. Beneath it, the values: `high → critical` is a rename **without
  a bridge** (`no bridge · constant` — a constant is not a predicate, so no rule can alias it),
  `test` is removed, `advisory` is added. Values are matched by number, as fields are: v2 puts
  `LEVEL_ADVISORY` at 4 and reserves 3, so the report reads an addition and a removal; had it
  reused 3, the report would have read a *rename* of `test` — which is what the wire would have
  seen too.
- **The footer** tallies the three classes and points at the flag that writes the bridges.

The exit code is `0`: a breaking change is a finding, not a failure. In a pipeline that should
fail on one, `--exit-code` exits `9` when any change is breaking, with the report still on
stdout.

## The changeset — `--json`

```sh
keryx diff thermal-v1.proto thermal-v2.proto -I . --json
```

[`diff.json`](diff.json) is the same comparison as data: one array, one record per change, in
a stable order — by package, then by the message's or enum's path, then by field number — so
`level` sits third here, between `calibration` and `reading`, where the report, which lists
messages before enums, puts it last. For a review bot, a changelog generator, or a check of
your own. The rename's record, reformatted from the file's one line:

```json
{
  "breaking": false,
  "bridge": "%! temp_c/2 reads celsius/2  (inbound-facing bridge: thermal.v1.Reading.temp_c renamed thermal.v2.Reading.celsius)\ntemp_c(A, B) :- celsius(A, B).\n",
  "kind": "renamed",
  "new": "celsius/2 int32 total",
  "new_path": "thermal.v2.Reading.celsius",
  "number": 2,
  "old": "temp_c/2 int32 total",
  "old_path": "thermal.v1.Reading.temp_c",
  "package": "thermal"
}
```

`old` and `new` are the element's signature on each side, in the manifest's words; `kind` is the
change's slug (`renamed`, `changed`, `message_removed`, `value_renamed`, `preserve_changed`,
…); `number` is present only where the element has one; `bridge` only where there is one.

## The bridge — `--bridge`

```sh
keryx diff thermal-v1.proto thermal-v2.proto -I . --bridge thermal.bridge.lp
```

writes the report as before and then the bridge views — one rule per clean rename — to the
file named. Here that is [`thermal.bridge.lp`](thermal.bridge.lp):

```prolog
%! temp_c/2 reads celsius/2  (inbound-facing bridge: thermal.v1.Reading.temp_c renamed thermal.v2.Reading.celsius)
temp_c(A, B) :- celsius(A, B).
```

A generated module like every module keryx writes — a `%!` provenance line over a rule, valid
clingo (it grounds clean beside v2's `core.lp`, whose `#defined celsius/2` declares the body)
— and it says one thing: wherever v2 facts assert `celsius`, `temp_c` holds too. **A v1 model
reads v2 facts through it.** To see it work, shred a v2 batch and run a v1 rule over the facts.
[`batch.txtpb`](batch.txtpb) is a `ReadingBatch` as v2 writes one — two readings, `celsius` on
the wire — and [`hot.lp`](hot.lp) is a rule written against v1, `hot(R) :- temp_c(R, T), T > 40.`:

```sh
keryx facts --root ReadingBatch=batch.txtpb thermal-v2.proto -I . > facts.lp
clingo facts.lp hot.lp                     # the v1 rule alone
clingo facts.lp thermal.bridge.lp hot.lp   # the same rule, with the bridge
```

[`facts.lp`](facts.lp) is the batch shredded — `celsius(readings(r0, 0), 44).` and its
siblings, no `temp_c` among them — so alone, the rule finds nothing to fire on and derives
nothing. With the bridge loaded beside the facts, it derives `hot(readings(r0,0))`: the model
written against v1 is cut over to v2 without a line of it changing. When there is no clean
rename, keryx writes no file and says so.

Two limits, stated rather than papered over:

- **The bridge is inbound-facing.** It lets a v1 model *read* v2 facts. A model that *asserts*
  `temp_c` atoms for `keryx emit` to serialize is not shimmed: v2's theory obliges `celsius`, and
  the reassembler reads `celsius`.
- **A message-field bridge aliases the view, not the path term.** Rename `ReadingBatch.readings`
  to `samples` and the bridge is `readings(A, B, C) :- samples(A, B, C).` — the relational view
  a model joins on (spec §13.2). The occupant term itself, `samples(r0, 0)` inside
  `reading(samples(r0, 0))`, is renamed too, and no rule can alias a term; a model that spells
  the path term directly (`reading(readings(P, I))`) is not bridged. The `%!` line names such a
  bridge as *of the view*.

## Two schemas that are not two versions of anything

[`heartbeat.proto`](heartbeat.proto) (package `heartbeat.v1`) and [`invoice.proto`](invoice.proto)
(package `billing.v1`) have one thing in common: both import `google/protobuf/timestamp.proto`
and stamp a message with a `Timestamp`.

```sh
keryx diff heartbeat.proto invoice.proto -I .
```

```
keryx: no_comparable_schemas: the old schema (`heartbeat`) and the new schema (`billing`) have
no package in common once version segments are stripped (imported packages aside); keryx diff
compares two versions of one schema
```

Exit `2`, a usage error, and no report. The `Timestamp` each references becomes a sort in each
schema's vocabulary (spec §10), but it is *referent closure* — pulled in because a field names
it — not the schema's own subject vocabulary, and `keryx diff` compares subject vocabulary only.
So the shared closure counts for nothing, and the refusal names each side's own package, never
`google.protobuf`: a diff over two unrelated schemas is a mistake, not a page of well-known
types.

## A multi-file schema, package by package

[`station/`](station/) holds a schema at two revisions as a repository would, the same file
names under [`v1/`](station/v1/) and [`v2/`](station/v2/). `station.proto` imports a package of
its own on each side — `log.proto` (`station.log.v1`) in v1, `alarms.proto`
(`station.alarms.v2`) in v2 — so a version is more than one file. Compile each to a
**self-contained descriptor set** and diff the sets:

```sh
protoc -I station/v1 --include_imports --descriptor_set_out=station/v1.binpb station/v1/station.proto
protoc -I station/v2 --include_imports --descriptor_set_out=station/v2.binpb station/v2/station.proto
keryx diff station/v1.binpb station/v2.binpb
```

[`station/diff.txt`](station/diff.txt):

```
━━ keryx diff ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
   3 packages                                              keryx 1.0.0
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

 station.v1  →  station.v2
 ═════════════════════════════════════════════════════════════════════

 station · station.Station
 ─────────────────────────────────────────────────────────────────────
  #2  - removed   log/3             entry,seq     no bridge
  #3  + added     alarms/2          policy,partial

 station.alarms.v2                                             + added
 ═════════════════════════════════════════════════════════════════════

 policy · station.alarms.Policy                                + added
 ─────────────────────────────────────────────────────────────────────
  #1  + added     threshold_c/2     int32,total

 station.log.v1                                  - removed · no bridge
 ═════════════════════════════════════════════════════════════════════

 entry · station.log.Entry                       - removed · no bridge
 ─────────────────────────────────────────────────────────────────────
  #1  - removed   note/2            string,total  no bridge

 ─────────────────────────────────────────────────────────────────────
  4 breaking   2 removed · 1 message removed · 1 package removed
  4 additive   2 added · 1 message added · 1 package added
```

Three packages, three blocks under a `3 packages` banner: `station`, matched across the version
bump, with the field that named the log gone and the one naming the alarm policy new;
`station.alarms`, on the new side only, its message and field riding as additions; and
`station.log`, on the old side only, removed with everything in it. The changeset carries a
`package_added` and a `package_removed` record for the two.

### Two doors, two scopes

What counts as *the schema* on a side depends on the door it came through:

- **`.proto` source** — a side is the one file named on the command line. What it imports is
  closure: resolved, translated where a field references it, and *not compared*. The thermal
  pair went through this door; its `keryx/options.proto` import is closure.
- **`.binpb` descriptor set** — a side is every file in the set except the well-known types
  (`google/protobuf/*`) and keryx's option registry (`keryx/options.proto`). This is the route
  for a schema of several files: `--include_imports` makes the set carry them all, and the
  comparison spans them all.

Both sides go through one door. A `.proto` on one side and a `.binpb` on the other is refused
before either is read — "both sides go through one door", the refusal says, and names which
side is which.

## Scope at this stage

- **Bridges are for clean renames** — a field, a message, or an enum whose predicate changed
  with its identity and shape intact; one rule each, `old :- new`, inbound-facing. A changed
  form, a removed element, a renamed constant, and an enum flip have none, and the report says
  so on the row.
- **keryx runs no solver here either.** The bridge is a module for *your* clingo to load beside
  the new facts and the old model; `keryx diff` writes it and stops.
- **The regression suite runs everything above.** The report, the changeset, the bridge, and
  the shredded v2 facts are compared to the committed files on every commit (the report modulo
  its version line); the unrelated pair is checked to name no well-known type; the station sets
  are re-derived from their sources and diffed to the same report. The clingo step is yours to
  run — keryx's suite spawns no solver.
