//! themelios constructors for emitted vocabulary (architecture R2): a view variable, an
//! occupant application term over view variables, the documented `#defined` signature
//! statement, the relational view rule, the `#include` that opens a client module, and — for
//! `emit.lp`'s reach rules and obligations (spec §12) — the documented rule, fact, and
//! integrity constraint, an atom's positive and default-negated body occurrences, the
//! comparison, and the few terms an obligation spells beside a variable: the anonymous `_`,
//! an integer, the difference `I - 1`, and the one string constant `emit` builds, a diagnostic
//! head's field path. The one place emit touches themelios's construction surface, so the
//! binding is confined and greppable. Emitted predicate names arrive pre-validated as `Name`s
//! from the `Mapping` (policy), so nothing here re-validates or `expect`s a runtime string;
//! the only `expect` is on the fixed compile-time set of view-variable letters (a discharged
//! invariant, §6).

use themelios_program::construct::not;
use themelios_program::prelude::*;

/// A view variable, `A`/`E`/`I`/`K`/`P`/`V`/`V1`/`V2`/`X` — a fixed compile-time set of valid
/// `VARIABLE`s, so the `expect` is a discharged invariant (§6); no runtime string reaches here.
/// keryx writes `P` (parent) where §13.2's own example writes `S` (subject) — the same role,
/// this module's own letter — `X` for the reached parent in `emit.lp`'s closure, §12.1's own
/// letter, and `V`, `V1`, `V2` for the values an obligation holds or compares.
pub(super) fn var(letter: &str) -> Term {
    Term::Variable(Variable::Named(
        VarName::new(letter).expect("view variables are valid variable names"),
    ))
}

/// The anonymous variable `_` — the argument position an obligation projects away, `f(P, _)`:
/// each `_` is its own fresh variable, so the atom asks only that some value stand there.
pub(super) fn anonymous() -> Term {
    Term::Variable(Variable::Anonymous)
}

/// An integer term — the `0` an index or a range obligation compares against.
pub(super) fn int(value: i32) -> Term {
    Term::from(value)
}

/// A string constant — the one ground string `emit` builds: a diagnostic obligation's field
/// path, `violates("pkg.Msg.field", P)` (spec §12.2's field-path descriptor, P1). The renderer
/// spells it under the dialect, the one place a render can refuse; `emit::render` owns the
/// argument for the paths keryx passes here.
pub(super) fn text(value: &str) -> Term {
    Term::Symbolic(Symbol::String(value.to_owned()))
}

/// The difference `term - amount` — the `I - 1` a contiguity obligation looks back to. Built
/// through themelios's operator sugar, which canonicalizes at the door: an operator over a
/// variable never folds, so the term renders as written, parenthesized.
pub(super) fn minus(term: Term, amount: i32) -> Term {
    term - amount
}

/// An application term `name(args…)` over view variables — an occupant access-path term (§4.1)
/// as a view rule spells it, `readings(P, I)`, so not ground: the rule deconstructs it against
/// the bound element. The `Name` is pre-validated (from the `Mapping`); the term canonicalizes
/// at the door.
pub(super) fn apply(name: Name, args: Vec<Term>) -> Term {
    Term::Function {
        name,
        arguments: args,
    }
    .canonicalize()
}

/// An atom `name(args…)` (a constant when `args` is empty) — a view rule's head or referent
/// (§13.2). The `Name` is pre-validated (from the `Mapping`). The one place `emit` builds a
/// themelios [`Atom`], so the construction binding stays confined to this module.
pub(super) fn atom(name: Name, args: impl IntoIterator<Item = Term>) -> Atom {
    Atom::new(name, args)
}

/// A `#defined name/arity.` statement (spec §13.1) carrying `doc` as one `%!` doc string.
/// `#defined` is clingo's declaration directive — inert on answer sets, and it suppresses
/// the grounder's "atom does not occur in a rule head" note for keryx's data-defined sorts.
pub(super) fn defined(name: Name, arity: u32, doc: String) -> WithProvenance<Statement> {
    WithProvenance::new(
        Statement::Defined(Defined {
            signature: Signature {
                sign: Sign::Positive,
                name,
                arity,
            },
        }),
        Provenance::empty().with_doc(doc),
    )
}

/// An `#include "path".` directive — the loader meta-statement a client module opens with
/// (spec §13.2, §13.3). themelios carries it as a statement and never resolves it (no I/O in
/// its program tier); the path is spelled as a string under the dialect, so the quoting is the
/// renderer's, not a format string's. Undocumented: no `%!` line rides above it.
pub(super) fn include(path: String) -> WithProvenance<Statement> {
    WithProvenance::new(
        Statement::Include(Include::new(IncludeTarget::Path(path))),
        Provenance::empty(),
    )
}

/// A relational view rule `head :- referent, element = occupant.` (spec §13.2), carrying
/// `doc` as one `%!` doc string. `referent` is the sort atom binding `element`; `occupant`
/// is the access-path term the comparison deconstructs (the spec's own idiom,
/// `items(S,I,E) :- item(E), E = items(S,I).`).
pub(super) fn view_rule(
    head: Atom,
    referent: Atom,
    element: Term,
    occupant: Term,
    doc: String,
) -> WithProvenance<Statement> {
    rule(
        head,
        vec![positive(referent), compare(element, Relation::Eq, occupant)],
        doc,
    )
}

/// An integrity constraint `:- body.` carrying `doc` as one `%!` doc string — a strict
/// obligation (spec §12.2), a falsum-headed rule. The body is themelios's set: de-duplicated
/// and rendered in its `Ord` order, not the caller's, so no emitter orders one.
pub(super) fn constraint(body: impl IntoBody, doc: String) -> WithProvenance<Statement> {
    WithProvenance::new(
        Statement::Rule(Rule::constraint(body)),
        Provenance::empty().with_doc(doc),
    )
}

/// A rule `head :- body.` carrying `doc` as one `%!` doc string — a reach rule (spec §12.1),
/// a witness an obligation reads, or a diagnostic obligation deriving `violates` (§12.2). The
/// body is themelios's set, as [`constraint`]'s.
pub(super) fn rule(head: Atom, body: impl IntoBody, doc: String) -> WithProvenance<Statement> {
    WithProvenance::new(
        Statement::Rule(head.into_head().when(body)),
        Provenance::empty().with_doc(doc),
    )
}

/// A fact `head.` carrying `doc` as one `%!` doc string — an enum's membership table entry,
/// `ok_e(c).` (spec §12.2).
pub(super) fn fact(head: Atom, doc: String) -> WithProvenance<Statement> {
    WithProvenance::new(
        Statement::Rule(Rule::fact(head)),
        Provenance::empty().with_doc(doc),
    )
}

/// The atom under default negation, `not p(…)`. Default negation is a property of a body
/// occurrence, so the result is a [`BodyElement`] and never reaches a head.
pub(super) fn not_atom(atom: Atom) -> BodyElement {
    not(atom)
}

/// An atom's positive body occurrence, `p(…)` — the element a mixed body (atoms beside a
/// comparison) is assembled from, since a body's elements are one type. The lift is
/// themelios's own coercion, spelled here so no emitter reaches its construction surface for it.
pub(super) fn positive(atom: Atom) -> BodyElement {
    BodyElement::from(atom)
}

/// The comparison `first R second` as a positive body element, in the written direction —
/// themelios's own `From<Comparison> for BodyElement` coercion (a positive body literal, no
/// default negation), spelled here so no emitter reaches its construction surface for it.
/// [`view_rule`]'s `element = occupant` goes through it too.
pub(super) fn compare(first: Term, relation: Relation, second: Term) -> BodyElement {
    BodyElement::from(Comparison::new(first, relation, second))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::render;

    fn name(text: &str) -> Name {
        Name::new(text).expect("a test name is a valid identifier")
    }

    // A rule with a body renders `head :- body.` under its doc — the reach rule's shape: the
    // child sort atom binds the occupant the comparison deconstructs, the body in themelios's
    // order (atoms by name, the comparison last) however the caller ordered it.
    #[test]
    fn a_rule_renders_its_head_over_its_body() {
        let statement = rule(
            atom(name("reach"), [var("A")]),
            [
                compare(
                    var("A"),
                    Relation::Eq,
                    apply(name("f"), vec![var("P"), var("I")]),
                ),
                positive(atom(name("u"), [var("A")])),
                positive(atom(name("t"), [var("P")])),
                positive(atom(name("reach"), [var("P")])),
            ],
            "reach : f".to_owned(),
        );
        assert_eq!(
            render(vec![statement]).expect("renders"),
            "%! reach : f\nreach(A) :- reach(P), t(P), u(A), A = f(P, I).\n"
        );
    }

    // A comparison is a positive body element spelling its relation in the written direction
    // (`A != E`, never flipped), and the body renders in themelios's `Ord` order — the atom
    // before the comparison — whatever order the caller passed, so no constructor here is
    // asked to keep one.
    #[test]
    fn a_comparison_renders_in_its_written_direction() {
        let statement = rule(
            atom(name("distinct"), [var("A"), var("E")]),
            [
                compare(var("A"), Relation::Neq, var("E")),
                positive(atom(name("p"), [var("A"), var("E")])),
            ],
            "the pairs p tells apart".to_owned(),
        );
        assert_eq!(
            render(vec![statement]).expect("renders"),
            "%! the pairs p tells apart\ndistinct(A, E) :- p(A, E), A != E.\n"
        );
    }

    // An integrity constraint is the falsum-headed rule over its body, its doc riding as the
    // `%!` line above it, the body in themelios's `Ord` order whatever order the caller passed.
    #[test]
    fn a_constraint_is_a_falsum_headed_rule_over_its_body() {
        let statement = constraint(
            [
                compare(var("A"), Relation::Neq, var("E")),
                positive(atom(name("p"), [var("A"), var("E")])),
            ],
            "no p pairs a value with itself".to_owned(),
        );
        assert_eq!(
            render(vec![statement]).expect("renders"),
            "%! no p pairs a value with itself\n:- p(A, E), A != E.\n"
        );
    }

    // Default negation is a property of a body occurrence: `not_atom` yields a body element
    // spelled `not p(…)`, which sorts after the positive elements in themelios's order, so it
    // renders last however it was inserted.
    #[test]
    fn a_negated_atom_renders_under_default_negation_in_body_position() {
        let statement = constraint(
            [
                not_atom(atom(name("p"), [var("P")])),
                positive(atom(name("t"), [var("P")])),
            ],
            "every t is a p".to_owned(),
        );
        assert_eq!(
            render(vec![statement]).expect("renders"),
            "%! every t is a p\n:- t(P), not p(P).\n"
        );
    }

    // A fact is a head over the empty body, rendered head-and-dot under its doc — the
    // membership table's shape, its constant a ground application collapsed to a symbol.
    #[test]
    fn a_fact_renders_its_head_alone() {
        let statement = fact(
            atom(name("ok_level"), [apply(name("low"), Vec::new())]),
            "membership of enum level/1  (open)".to_owned(),
        );
        assert_eq!(
            render(vec![statement]).expect("renders"),
            "%! membership of enum level/1  (open)\nok_level(low).\n"
        );
    }

    // The obligation terms render as written: the anonymous variable as `_`, an integer as its
    // digits, the difference parenthesized, and the string constant quoted — the diagnostic
    // head's field path, the one string `emit` spells.
    #[test]
    fn the_obligation_terms_render_as_written() {
        let statement = rule(
            atom(name("violates"), [text("a.B.c"), var("P")]),
            vec![
                positive(atom(name("f"), [var("P"), anonymous()])),
                positive(atom(name("has"), [var("P"), var("I")])),
                compare(var("I"), Relation::Gt, int(0)),
                not_atom(atom(name("has"), [var("P"), minus(var("I"), 1)])),
            ],
            "contiguity of c".to_owned(),
        );
        assert_eq!(
            render(vec![statement]).expect("renders"),
            "%! contiguity of c\nviolates(\"a.B.c\", P) :- f(P, _), has(P, I), I > 0, not has(P, (I - 1)).\n"
        );
    }
}
