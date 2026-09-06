//! themelios constructors for emitted vocabulary (architecture R2): a view variable, an
//! occupant application term over view variables, the documented `#defined` signature
//! statement, the relational view rule, and — for `emit.lp`'s reach rules (spec §12.1) —
//! the documented rule and the comparison. The one place emit touches themelios's
//! construction surface, so the binding is confined and greppable. Emitted predicate names
//! arrive pre-validated as `Name`s from the `Mapping` (policy), so nothing here re-validates
//! or `expect`s a runtime string; the only `expect` is on the fixed compile-time set of
//! view-variable letters (a discharged invariant, §6).

use themelios_program::prelude::*;

/// A view variable, `A`/`E`/`I`/`K`/`P` — a fixed compile-time set of valid `VARIABLE`s, so
/// the `expect` is a discharged invariant (§6); no runtime string reaches here. keryx writes
/// `P` (parent) where §13.2's own example writes `S` (subject) — the same role, this
/// module's own letter.
pub(super) fn var(letter: &str) -> Term {
    Term::Variable(Variable::Named(
        VarName::new(letter).expect("view variables are valid variable names"),
    ))
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
        vec![
            BodyElement::from(referent),
            compare(element, Relation::Eq, occupant),
        ],
        doc,
    )
}

/// A rule `head :- body.` carrying `doc` as one `%!` doc string — a reach rule (spec §12.1)
/// or a diagnostic obligation deriving `violates` (§12.2). The body is themelios's set:
/// de-duplicated and rendered in its `Ord` order, not the caller's, so no emitter orders one.
pub(super) fn rule(head: Atom, body: impl IntoBody, doc: String) -> WithProvenance<Statement> {
    WithProvenance::new(
        Statement::Rule(head.into_head().when(body)),
        Provenance::empty().with_doc(doc),
    )
}

/// The comparison `first R second` as a positive body element, in the written direction. A
/// [`Comparison`] has no body coercion of its own, so this is its one wrapping into a
/// [`Literal`] — [`view_rule`]'s `element = occupant` goes through it too.
pub(super) fn compare(first: Term, relation: Relation, second: Term) -> BodyElement {
    BodyElement::from(Literal {
        negation: DefaultNegation::None,
        inner: LiteralInner::Comparison(WithProvenance::constructed(Comparison::new(
            first, relation, second,
        ))),
    })
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
                BodyElement::from(atom(name("u"), [var("A")])),
                BodyElement::from(atom(name("t"), [var("P")])),
                BodyElement::from(atom(name("reach"), [var("P")])),
            ],
            "reach : f".to_owned(),
        );
        assert_eq!(
            render(vec![statement]).expect("renders"),
            "%!reach : f\nreach(A) :- reach(P), t(P), u(A), A = f(P, I).\n"
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
                BodyElement::from(atom(name("p"), [var("A"), var("E")])),
            ],
            "the pairs p tells apart".to_owned(),
        );
        assert_eq!(
            render(vec![statement]).expect("renders"),
            "%!the pairs p tells apart\ndistinct(A, E) :- p(A, E), A != E.\n"
        );
    }
}
