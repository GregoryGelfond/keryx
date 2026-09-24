# Vocabulary

One concept, one name. This page is the registry the rest of the book and the source hold to.

| Concept | keryx term |
|---|---|
| A message type, as an ASP domain | **sort** — a unary predicate `sort_name/1` |
| A scalar or enum field, as a fact | **base-fact predicate** — `field/2` over its sort |
| A message-typed field's occupant, addressed by where it sits | **occupant term** — the access-path term `field(parent, …)`, never a minted handle |
| The relation a message-typed field derives, for a model to join on | **view** — a rule in `views.lp` |
| Turning a message into ground facts | **shred** |
| Turning an answer set back into a message | **reassemble** |
| A value occupying a field position in the reassembled tree | **occupant** |
| A boundary that admits external input (a schema, a payload, an answer set) | **door** |
| The atom that marks a reassembly root | **marker** |
| The per-version record `gen` writes and never reads back | **manifest** |
| A field or enum's precise ASP treatment, set by option | **annotation** — `(keryx.set)`, `(keryx.numeric)`, `(keryx.scale)`, `(keryx.opaque)`, `(keryx.unknown)` |
| A structured refusal reported at a field-path locus | **diagnostic** |
| An insulated failure on a dependency's input path | **fault** |
