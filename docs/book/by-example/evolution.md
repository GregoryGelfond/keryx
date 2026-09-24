# Schema evolution with `keryx diff`

A worked run of the evolution instrument: the thermal schema at two versions, and what `keryx diff` says about the move from one to the other — which changes a model written against v1 survives, which it does not, and the **bridge** keryx writes for the one kind of change it can carry a model across.

The runnable example — both schemas, the goldens, and a live bridge demonstration — is in [`examples/evolution/`](https://github.com/GregoryGelfond/keryx/tree/main/examples/evolution). The concepts are in [Schema evolution](../guide/evolution.md); this is the worked case.

## The two versions

`thermal-v1.proto` and `thermal-v2.proto`, versioned as buf recommends — package `thermal.v1` becomes `thermal.v2`, which is how keryx knows the two are one schema. Between them, seven kinds of change, each chosen because it says something different to a model:

- `temp_c` → `celsius` — a field **renamed** at its number, shape intact.
- `humidity` — a field **added**.
- `alerts` — its **form changed**: a `(keryx.set)` membership relation in v1, a plain sequence in v2.
- `Calibration` **removed**, `Site` **added**.
- `Level` — an enum with a value renamed (`LEVEL_HIGH` → `LEVEL_CRITICAL`), one removed (`LEVEL_TEST`), one added (`LEVEL_ADVISORY`), and `(keryx.unknown) = PRESERVE` switched on.

## The report

```sh
keryx diff thermal-v1.proto thermal-v2.proto -I .
```

Each version goes through keryx's compiler, is mapped as `keryx gen` would map it, and the two mappings are compared. Captured plain (`--color never`):

```text
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

One block per message and per enum, headed by its sort predicate and its version-free path, the messages first. Beneath a header, one row per changed field in **field-number order** — the gutter `#2` is the number, the identity keryx matched by. What did not change is shown too (`alert`, `reading_batch` are marked `unchanged`). Each change is classed by what it does to a model:

- **`temp_c → celsius` is bridged, not breaking** — the field kept its number, type, arity, and presence; only its name, and so its predicate, changed. That is the one change a rule can alias.
- **`alerts` `set → seq` is breaking** — dropping `(keryx.set)` turns a membership relation `alerts(P, E)` into a sequence view `alerts(P, I, E)`: a different arity and meaning, and buf would never have said a word (an option changed, the wire did not).
- **The `level` enum header is itself a change** — switching on `(keryx.unknown) = PRESERVE` admits the escape term `unknown(N)` to the sort, a vocabulary v1 never saw; and `high → critical` is a rename **without** a bridge (a constant is not a predicate, so no rule can alias it).

The exit code is `0` — a breaking change is a finding, not a failure. `--exit-code` exits `9` when any change is breaking, for a pipeline that should stop on one. `--json` writes the same comparison as a flat changeset for a review bot.

## The bridge

```sh
keryx diff thermal-v1.proto thermal-v2.proto -I . --bridge thermal.bridge.lp
```

writes the report, then one rule per clean rename:

```prolog
%! temp_c/2 reads celsius/2  (inbound-facing bridge: thermal.v1.Reading.temp_c renamed thermal.v2.Reading.celsius)
temp_c(A, B) :- celsius(A, B).
```

A generated module like any other keryx writes — a `%!` provenance line over a rule, valid clingo (it grounds clean beside v2's `core.lp`) — saying: wherever v2 facts assert `celsius`, `temp_c` holds too. **A v1 model reads v2 facts through it** — load it beside the shredded v2 facts and a v1 rule (`hot(R) :- temp_c(R, T), T > 40.`) fires again, without a line of the model changing.

Two limits, stated plainly: the bridge is **inbound-facing** (it lets a v1 model *read* v2 facts; a model that *asserts* `temp_c` for `keryx emit` is not shimmed — v2's theory obliges `celsius`), and a message-field bridge **aliases the view, not the occupant term** (a model spelling the path term directly is not bridged).

## Two doors, two scopes

What counts as *the schema* on a side depends on how it came in. A **`.proto` source** side is the one file named; what it imports is closure — resolved and translated, not compared (the thermal pair's `keryx/options.proto` is closure). A **`.binpb` descriptor set** side is every file in the set except the well-known types and keryx's option registry — the route for a schema of several files (`protoc --include_imports`), compared package by package. Both sides go through one door; a mix is refused before either is read.

The example also shows two smaller cases: two schemas with no package in common (refused `no_comparable_schemas`, naming each side's own package, never `google.protobuf`), and a multi-file station schema compared as descriptor sets, package by package (`package_added` / `package_removed`).

## Complementary to `buf breaking`

buf guards the wire: a field renumbered, a type changed. The protobuf contract lets a name change freely, so buf passes a rename — and on the ASP side a predicate's identity *is* its name, so that rename silently forks the vocabulary; an annotation dropped, which the wire never sees, changes an arity. `keryx diff` catches both. Run the two together: buf for the wire, keryx for the model side.
