//! themelios constructors for emitted vocabulary (architecture R2): a view variable, an
//! occupant application term over view variables, the documented `#defined` signature
//! statement, and the relational view rule. The one place emit touches themelios's
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
    let comparison = Literal {
        negation: DefaultNegation::None,
        inner: LiteralInner::Comparison(WithProvenance::constructed(Comparison::new(
            element,
            Relation::Eq,
            occupant,
        ))),
    };
    let rule = head.into_head().when(vec![
        BodyElement::from(referent),
        BodyElement::from(comparison),
    ]);
    WithProvenance::new(Statement::Rule(rule), Provenance::empty().with_doc(doc))
}
