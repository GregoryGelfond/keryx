//! themelios term and fact constructors — the one fact-construction vocabulary the
//! descriptor facts (`schema_facts`) and the payload facts (`codec`) share, so a fact,
//! and the applied term beneath it (`apply`), is built one way crate-wide. Proto-derived
//! text (names, paths, doc prose, payload strings) becomes string terms — faithful,
//! escaping-safe, and never an identifier a CamelCase name or a dot would make illegal.
//!
//! Two doors, two discharges (§6):
//!
//! - **The literal door** — `constant`, `function`, `fact`. keryx's own closed
//!   vocabulary — predicate names, scalar kinds, presence, cardinality, openness, and the
//!   `msg`/`enum`/`map` type functors — is always a fixed compile-time literal, so it
//!   lowers through `vocabulary`'s `expect`: a discharged invariant, not a live risk.
//! - **The `Name` door** — `apply`, `fact_named`, `atom_symbol`, `fact_of`. A predicate or
//!   functor that is a runtime value — a mapping name the policy derived from a schema —
//!   arrives as an already-validated [`Name`], whose text always re-lexes as an identifier
//!   (its one validation happened at the policy's `Name::new` door, where a failure is
//!   diagnosed), so nothing here re-validates or `expect`s it. A payload fact has one
//!   representation, its head [`Symbol`] (`atom_symbol` — the model a consuming tool is
//!   handed); the statement the `.lp` seam renders is derived from that symbol (`fact_of`)
//!   through the same statement door a descriptor fact is built at (`fact_named`), so the
//!   text is a view of the symbols — never a second structure that could disagree — and a
//!   derived statement and a directly built one are one spelling. `atom_symbol`'s doc
//!   states the one discharge its collapse rests on; `fact_of`'s the one its shape rests on.
//!
//! An option key (`Annotation::key`) is neither: a `String`, not a literal, and not yet
//! validated. `descriptor::options::read` admits an option by matching its extension's
//! *file name* against `keryx/options.proto` — a best-effort heuristic, not true
//! extension identity (see that function's doc) — so a key reaching here is never assumed
//! to be one of keryx's own registry. `try_constant` is the total counterpart to
//! `constant` for exactly this one runtime-derived string: it returns `Err` instead of
//! panicking, so `schema_facts::render` stays total over any input (§6).
//!
//! `emit::build` stays separate: it constructs *declaration* statements — `#defined`
//! signatures and view rules over variables — never a ground fact, so the two
//! construction sites share no door.

use themelios_program::prelude::*;
use themelios_program::term::TermParts;

/// A string term, e.g. `"dispatch.v1.Shipment"`.
pub(crate) fn text(value: &str) -> Term {
    Term::Symbolic(Symbol::String(value.to_owned()))
}

/// An integer term.
pub(crate) fn int(value: i32) -> Term {
    Term::from(value)
}

/// A constant (0-arity function) over a keryx-vocabulary identifier, e.g.
/// `implicit`, `singular`, `int32`.
pub(crate) fn constant(name: &str) -> Term {
    function(name, Vec::new())
}

/// A ground function term over a keryx-vocabulary functor, e.g. `msg("...")`,
/// `map(string, int32)` — the literal door over [`apply`].
pub(crate) fn function(name: &str, arguments: Vec<Term>) -> Term {
    apply(vocabulary(name), arguments)
}

/// An application term `name(args…)` (a constant when `arguments` is empty) over an
/// already-validated functor, canonicalized at the door — a ground constructor collapses
/// to a `Symbol`. The one spelling of the *ground* applied term across the
/// fact-construction doors (`function`, `try_constant`, [`atom_symbol`]'s head,
/// `Root::term`); the declaration-side `emit::build::apply`, applied to view variables,
/// stays separate — see the module doc.
pub(crate) fn apply(name: Name, arguments: Vec<Term>) -> Term {
    Term::Function { name, arguments }.canonicalize()
}

/// A fact statement `pred(args...).` over a keryx-vocabulary predicate, carrying a
/// constructed provenance — the literal door over [`fact_named`].
pub(crate) fn fact(predicate: &str, arguments: Vec<Term>) -> WithProvenance<Statement> {
    fact_named(vocabulary(predicate), arguments)
}

/// A fact statement `pred(args...).` over an already-validated predicate, carrying a
/// constructed provenance — the one statement door: a descriptor fact is built here
/// directly, and a payload fact's statement is derived here from its head symbol
/// ([`fact_of`]). The arguments canonicalize at themelios's atom door, so the head is the
/// same application [`atom_symbol`] builds from the same `(predicate, arguments)`.
pub(crate) fn fact_named(predicate: Name, arguments: Vec<Term>) -> WithProvenance<Statement> {
    WithProvenance::constructed(Statement::Rule(Rule::fact(Atom::new(predicate, arguments))))
}

/// The fact head `pred(args...)` as its ground [`Symbol`] — the value the library seam
/// hands a client, and the one representation a payload fact has: its `.lp` statement is
/// derived from this symbol by [`fact_of`], so nothing exists beside it to drift. The
/// applied head collapses to a symbol when every argument is a ground leaf — the one
/// discharge here (§6): every term a keryx door builds (`text`, `int`, `constant`,
/// `function`, `apply`, `try_constant`, `Root::term`) is already such a leaf, and a
/// payload's values reach a head only through those doors, so the uncollapsed arm is
/// unreachable — a keryx programming error, never a foreign input.
pub(crate) fn atom_symbol(predicate: Name, arguments: Vec<Term>) -> Symbol {
    let head = apply(predicate, arguments);
    match head.into_parts() {
        TermParts::Symbolic(symbol) => symbol,
        _ => unreachable!("a keryx-built fact head is ground and collapses to its symbol"),
    }
}

/// The fact statement `pred(args...).` of a ground head symbol — the `.lp` seam's door,
/// deriving a payload fact's statement from the one representation it has, through
/// [`fact_named`], so a derived statement is spelled exactly as a directly built one. A
/// keryx-built head is a positive function symbol: [`apply`] collapses a ground application
/// to one (a term-position function bears no strong sign), and every head reaches here
/// through [`atom_symbol`] over `apply` — so a symbol of any other shape (a number, a
/// string, a tuple, `#inf`/`#sup`, a strongly-negated function) is the one discharge here
/// (§6): a keryx programming error, never a foreign input.
pub(crate) fn fact_of(head: &Symbol) -> WithProvenance<Statement> {
    match head {
        Symbol::Function {
            name,
            arguments,
            sign: Sign::Positive,
        } => fact_named(
            name.clone(),
            arguments.iter().cloned().map(Term::from).collect(),
        ),
        Symbol::Function {
            sign: Sign::Negative,
            ..
        }
        | Symbol::Infimum
        | Symbol::Number(_)
        | Symbol::String(_)
        | Symbol::Tuple(_)
        | Symbol::Supremum => {
            unreachable!("a keryx-built fact head is a positive ground function symbol")
        }
    }
}

/// The `opt/3` key constant for `key`, or the reason it is not a themelios
/// identifier. Unlike `constant`, total: an option key is the one runtime-derived
/// string that can reach a `Name::new` door (see the module doc), so it is
/// checked here rather than `expect`ed — `annotation_facts` composes a
/// diagnostic on `Err` instead of panicking (§6).
pub(crate) fn try_constant(key: &str) -> Result<Term, NotAnIdentifier> {
    Ok(apply(Name::new(key)?, Vec::new()))
}

/// A validated keryx-vocabulary identifier — a fixed, all-legal set, so the
/// `expect` is discharged (§6); proto text never arrives here (it uses `text`),
/// and the one runtime-derived string that could (the option key) uses
/// `try_constant` instead.
fn vocabulary(name: &str) -> Name {
    Name::new(name).expect("keryx vocabulary is a valid identifier")
}

#[cfg(test)]
mod tests {
    use super::*;
    use themelios_program::render::render;

    fn name(text: &str) -> Name {
        Name::new(text).expect("a test name is a valid identifier")
    }

    // A payload fact's head symbol is its one representation: the statement `fact_of`
    // derives from it is exactly the statement `fact_named` builds from the same
    // `(predicate, arguments)` — one spelling for a derived fact and a directly built one —
    // and its head atom, re-applied as a term, canonicalizes back to the symbol, which is
    // the positive ground application the arguments denote, the fact rendering as it spells.
    #[test]
    fn a_fact_derived_from_its_head_symbol_is_the_fact_built_directly() {
        let predicate = name("reading");
        let arguments = vec![
            text("s-101"),
            int(44),
            constant("celsius"),
            function("msg", vec![text("x")]),
        ];
        let symbol = atom_symbol(predicate.clone(), arguments.clone());
        let statement = fact_named(predicate.clone(), arguments);
        assert_eq!(fact_of(&symbol), statement);

        let Statement::Rule(rule) = statement.get() else {
            panic!("a fact is a rule");
        };
        let Head::Literal(Literal {
            negation: DefaultNegation::None,
            inner: LiteralInner::Atom(atom),
        }) = rule.head().get()
        else {
            panic!("a fact's head is one positive literal");
        };
        let atom = atom.get();
        assert_eq!(atom.name, predicate);
        assert_eq!(atom.sign, Sign::Positive);
        assert!(!atom.is_pooled());
        let reapplied = Term::Function {
            name: atom.name.clone(),
            arguments: atom.argument_terms().cloned().collect(),
        }
        .canonicalize();
        assert_eq!(reapplied, Term::from(symbol.clone()));

        assert_eq!(
            symbol,
            Symbol::Function {
                name: predicate,
                arguments: vec![
                    Symbol::String("s-101".to_owned()),
                    Symbol::Number(44),
                    Symbol::Function {
                        name: name("celsius"),
                        arguments: Vec::new(),
                        sign: Sign::Positive,
                    },
                    Symbol::Function {
                        name: name("msg"),
                        arguments: vec![Symbol::String("x".to_owned())],
                        sign: Sign::Positive,
                    },
                ],
                sign: Sign::Positive,
            }
        );
        assert_eq!(
            render(&Program::of([statement]), Dialect::Clingo).expect("renders"),
            "reading(\"s-101\", 44, celsius, msg(\"x\")).\n"
        );
    }

    // The literal door is the named door over keryx's vocabulary — one construction, so
    // the descriptor facts and the payload facts cannot build a fact two ways.
    #[test]
    fn a_literal_fact_is_the_named_fact_over_the_vocabulary() {
        let arguments = vec![text("a.proto"), text("a")];
        assert_eq!(
            fact("file", arguments.clone()),
            fact_named(name("file"), arguments)
        );
    }

    // The premise of `fact_of`'s discharge, pinned from the other side: a symbol that is not
    // a positive function — never a head a keryx door builds — is a keryx error, loud, not a
    // statement quietly minted over the wrong shape.
    #[test]
    #[should_panic(expected = "a keryx-built fact head is a positive ground function symbol")]
    fn a_head_that_is_not_a_positive_function_symbol_is_a_keryx_error() {
        let _ = fact_of(&Symbol::Number(1));
    }

    // The premise of `atom_symbol`'s discharge, pinned: every term a keryx door builds —
    // the doors here and a payload's root — is a collapsed ground leaf, so an applied head
    // over them always canonicalizes to its symbol.
    #[test]
    fn every_door_builds_a_collapsed_ground_leaf() {
        let built = [
            text("a.B"),
            int(-1),
            constant("implicit"),
            function("map", vec![constant("string"), constant("int32")]),
            apply(name("readings"), vec![constant("r0"), int(0)]),
            try_constant("set").expect("`set` is an identifier"),
            crate::codec::Root::fresh(0).term(),
        ];
        assert!(built.iter().all(|term| matches!(term, Term::Symbolic(_))));
    }
}
