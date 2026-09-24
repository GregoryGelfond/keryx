# Diagnostics and exit codes

Every foreign input keryx reads crosses a `Result` boundary that returns a typed diagnostic — never a panic, never a bare string. A tool embedding keryx can therefore handle every refusal as a value, and the `keryx` command maps those values to distinct exit codes.

## A diagnostic is a typed value at a locus

A `Diagnostic` carries a *kind*, a *locus* (the field path or element where the refusal occurred), and a human-readable message. Refusals come back as `Diagnostics` — a collection, because one input can fail in more than one place, each named at its own path. A tool reads the kind to branch, the locus to point a user at the problem, and the message to show.

The taxonomy spans the doors:

- **Payload** — bytes that do not decode as the root type (`UndecodablePayload`), or nest past the ceiling (`PayloadTooDeep`).
- **Value** — a scalar keryx cannot represent in the dialect (`UnrepresentableText`), a float off its scale (`ValueNotOnScale`) or non-finite (`NonFiniteFloat`), an unknown enum value without `PRESERVE` (`UnknownEnumValue`).
- **Schema** — an editions file (`UnsupportedEdition`), an option keryx cannot map (`UnmappableOptionKey`).
- **Shape** — an answer set that cannot be a message (`ShapeViolation`), or one the reassembler cannot read (`UnreadableAnswerSet`); on the JSON output form, a well-known value canonical JSON cannot carry (`UnrepresentableJson`).
- **Diff** — two schemas with no comparable package (`NoComparableSchemas`), or a side with two versions of one package (`AmbiguousVersionPackages`).

## Exit codes

The `keryx` command turns those classes into exit codes, so a script can branch on the *kind* of failure: success, an internal error, a usage error, an unreadable input, a schema rejection, a shape violation, a translation refusal, a dependency fault, and — under `keryx diff --exit-code` — a breaking divergence are each their own class. A breaking diff is a *finding*, not an error: it only becomes a non-zero exit under `--exit-code`. The exact values are listed in the architecture (`docs/design/architecture.md`).
