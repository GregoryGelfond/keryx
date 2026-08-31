# themelios gaps surfaced by keryx

keryx consumes themelios arm's-length and **never modifies it** (founding
design §4). When keryx's use surfaces a gap in themelios's surface, it is
recorded here with the failing keryx use case, then closed in a *themelios*
session and adopted by a deliberate dependency-rev bump — never worked around
by editing themelios.

Format per entry: the need, the failing use case, the proposed themelios
change, and the status (open / fixed-in `<rev>` / adopted).

## Candidates

- **`raise` convenience.** keryx-core touches `themelios_syntax::parse`
  directly only to hand a `Parse` to `themelios_program::raise`. If that
  direct dependency reads as a leak, a `themelios_program::raise_source(&Source)
  -> Raised` convenience would let consumers touch only the program crate.
  Status: open (raised at founding; not yet needed — the `admit` increment
  decides).

- **Free-standing / inline `%` comment emission.** themelios's `render` emits
  comments only as statement-attached `%!` doc lines (from provenance); there is
  no free-standing `%` block or inline `%` prose, and canonical `render` is
  provenance-blind. keryx's §13.1 honorary signature is a free-standing comment
  block, and the worked stories show inline `%` prose in generated `.lp`. Needed:
  a rendering path for plain `%` comments. Status: open (surfaced at the founding
  design review; the emit shape is decided at the `emit` increment).
