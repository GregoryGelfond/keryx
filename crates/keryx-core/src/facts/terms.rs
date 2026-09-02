//! themelios term and fact constructors for the descriptor facts. Proto-derived
//! text (names, paths, doc prose) becomes string terms — faithful, escaping-safe,
//! and never an identifier a CamelCase name or a dot would make illegal.
//!
//! keryx's own closed vocabulary — predicate names, scalar kinds, presence,
//! cardinality, openness, and the `msg`/`enum`/`map` type functors — is always a
//! fixed compile-time literal, so `constant`/`function`/`fact` lower it through
//! `vocabulary`'s `expect`: a discharged invariant, not a live risk (§6).
//!
//! An option key (`Annotation::key`) is different: a `String`, not a literal.
//! `descriptor::options::read` admits an option by matching its extension's
//! *file name* against `keryx/options.proto` — a best-effort heuristic, not true
//! extension identity (see that function's doc) — so a key reaching here is
//! never assumed to be one of keryx's own registry. `try_constant` is the total
//! counterpart to `constant` for exactly this one runtime-derived string: it
//! returns `Err` instead of panicking, so `facts::render` stays total over any
//! input (§6). Internal to `facts`.

use themelios_program::prelude::*;

/// A string term, e.g. `"dispatch.v1.Shipment"`.
pub(super) fn text(value: &str) -> Term {
    Term::Symbolic(Symbol::String(value.to_owned()))
}

/// An integer term.
pub(super) fn int(value: i32) -> Term {
    Term::from(value)
}

/// A constant (0-arity function) over a keryx-vocabulary identifier, e.g.
/// `implicit`, `singular`, `int32`.
pub(super) fn constant(name: &str) -> Term {
    function(name, Vec::new())
}

/// A ground function term over a keryx-vocabulary functor, e.g. `msg("...")`,
/// `map(string, int32)`; a ground constructor canonicalizes to a `Symbol`.
pub(super) fn function(name: &str, arguments: Vec<Term>) -> Term {
    Term::Function {
        name: vocabulary(name),
        arguments,
    }
    .canonicalize()
}

/// A fact statement `pred(args...).`, carrying a constructed provenance.
pub(super) fn fact(predicate: &str, arguments: Vec<Term>) -> WithProvenance<Statement> {
    WithProvenance::constructed(Statement::Rule(Rule::fact(Atom::new(
        vocabulary(predicate),
        arguments,
    ))))
}

/// The `opt/3` key constant for `key`, or the reason it is not a themelios
/// identifier. Unlike `constant`, total: an option key is the one runtime-derived
/// string that can reach a `Name::new` door (see the module doc), so it is
/// checked here rather than `expect`ed — `annotation_facts` composes a
/// diagnostic on `Err` instead of panicking (§6).
pub(super) fn try_constant(key: &str) -> Result<Term, NotAnIdentifier> {
    Ok(Term::Function {
        name: Name::new(key)?,
        arguments: Vec::new(),
    }
    .canonicalize())
}

/// A validated keryx-vocabulary identifier — a fixed, all-legal set, so the
/// `expect` is discharged (§6); proto text never arrives here (it uses `text`),
/// and the one runtime-derived string that could (the option key) uses
/// `try_constant` instead.
fn vocabulary(name: &str) -> Name {
    Name::new(name).expect("keryx vocabulary is a valid identifier")
}
