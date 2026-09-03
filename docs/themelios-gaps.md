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

  **Resolution in `gen` (`emit::core`/`emit::views`).** The §13.1 honorary
  signature ships as documented `#defined name/arity.` declarations, one per
  sort and per base-fact (scalar/enum) field predicate: the signature line (and
  the proto doc, if any) rides as a single `%!` doc string on the declaration,
  through `render_documented`. A message-typed field has no base predicate, so
  its functional signature rides on its parent sort's `#defined` — keeping
  `core.lp` the complete functional canon — and its relational view (carrying
  the same signature) lives in `views.lp`, which opens with `#include
  "<pkg>.core.lp".`. This is the accepted divergence from §13.1's literal
  free-standing `%` comment block — themelios `86c7dfb` has no comment-only
  `Statement` variant, so the block form stays unemittable — and it renders in
  themelios's canonical `Ord` order (alphabetical by `name/arity`), not §28's
  sort-grouped block. It sidesteps this gap for the one shape `core.lp` needed,
  rather than closing it: what remains genuinely unemittable, unchanged, is (a)
  the exact free-standing `%` block spelling of §13.1's own worked example (the
  `% dispatch.v1.core — signature` block), and (b) the inline `%` prose the
  worked stories show inside generated `.lp` (e.g. §14's `keryx scaffold`
  example). **morphe** (the estate ASP-formatting library over
  themelios-syntax) is a candidate provider to *evaluate* if/when that inline-
  prose need is pursued — not adopted at M1 (R2's one-provider posture; its
  capability for this is unverified). No dependency taken on morphe or on any
  themelios change this increment. Status: open, narrowed (the `core.lp`
  signature need is met a different way; free-standing/inline `%` emission
  itself is still absent from themelios).

- **`From<Comparison> for BodyElement` (or `Literal`).** Composing a comparison
  into a rule body (`emit::build::view_rule`, the relational-view `E = f(P, I)`)
  takes the full ladder — `BodyElement::from(Literal { negation:
  DefaultNegation::None, inner: LiteralInner::Comparison(
  WithProvenance::constructed(Comparison::new(…))) })` — where an atom composes in
  one step (`BodyElement::from(atom)`). A themelios `From<Comparison> for
  BodyElement` (or `for Literal`) would make the comparison seam as smooth as the
  atom seam. Status: open (surfaced at the `emit` increment; cosmetic — the ladder
  works, it is only bumpy). The one composition seam this increment found rough.
