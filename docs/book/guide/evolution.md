# Schema evolution

`keryx diff` compares two versions of a schema at the level of the ASP vocabulary: what a model written against the old vocabulary survives, what it does not, and the bridge that carries it across the one kind of change a rule can alias. It complements `buf breaking` — buf guards the wire; `keryx diff` guards the model side.

```sh
keryx diff old.proto new.proto
keryx diff --json old.binpb new.binpb
```

## Both mappings are regenerated; no manifest is read

`keryx diff` compiles each side through the same descriptor door `keryx gen` uses, maps each as `gen` would (annotations included), and compares the two mappings. The manifest `gen` writes is a per-version record, never an input: the comparison sees exactly the vocabulary each side generates today, so nothing has to be kept in step with a stored file. The instrument opens no new input door.

## Matching is by protobuf identity, version stripped

Two versions of one schema are two packages under buf's convention (`thermal.v1`, `thermal.v2`), so a package is matched by name with the trailing version segment removed, a message or enum by its path, and a field or value by its number. Names are free to change: a renamed field is reported as a **rename**, not a removal beside an addition; a renumbered field is the reverse — a removal beside an addition, the wire-breaking act it is.

Two schemas with no package in common once normalized are refused as not comparable (`NoComparableSchemas`); a side declaring two versions of one package is refused as ambiguous (`AmbiguousVersionPackages`).

## Two doors, and no mixing

A side's subject depends on how it came in: through the `.proto` door, the side is the file named on the command line (its imports are referent closure — resolved and translated, but not compared); through the `.binpb` door, the side is every file in the set except the well-known types and keryx's option registry. Both sides must come through *one* door — a `.proto` against a `.binpb` would scope the two sides differently, so the mix is refused before either side is read. A multi-file schema goes in as descriptor sets (`protoc --include_imports`), compared package by package.

## The bridge aliases a predicate, never a term

For a clean rename — a field, message, or enum whose predicate changed with its identity and shape intact — `--bridge <path>` writes one rule, `old(…) :- new(…).`, under a `%!` provenance line: a generated module loaded beside the new facts, through which a model written against the old vocabulary reads them without a line of it changing. It is themelios-constructed, never string-templated, and round-trips as valid ASP.

Two limits, stated plainly. The bridge lets an old model *read* new facts; a model that *asserts* the old atoms for `keryx emit` is not shimmed (the new theory obliges the new predicate). And a message-field bridge aliases the field's relational *view*, not the occupant term — a model that spells the path term directly is not bridged. A renamed enum constant, a changed form (a dropped `(keryx.set)` changes an arity), a removal, or a flipped openness has no bridge: each is breaking, and the report says so on the row.

## The report, the changeset, and the exit

The migration report on stdout groups changes by package, then message or enum, then field number, showing what stayed beside what moved. `--json` writes a flat changeset instead — one array of records in a stable order — for a review bot. A breaking change is a *finding*, not a failure: the exit is `0` whatever the comparison found, and `--exit-code` turns a breaking comparison into a distinct exit (with the report still on stdout and the bridge still written) for a pipeline that should stop on one.

The [`evolution`](https://github.com/GregoryGelfond/keryx/tree/main/examples/evolution) example walks the thermal schema at v1 and v2 end to end.
