//! themelios term and fact constructors for the descriptor facts. Proto-derived
//! text (names, paths, doc prose) becomes string terms — faithful, escaping-safe,
//! and never an identifier a CamelCase name or a dot would make illegal; keryx's
//! own closed vocabulary (predicate names, scalar kinds, presence, cardinality,
//! openness, the `msg`/`enum`/`map` type functors, and option keys) becomes
//! constant and function terms over validated identifiers. An option key
//! (`Annotation::key`) is a `String`, not a compile-time literal, but is still
//! keryx vocabulary, not proto text: `descriptor::options::read` admits an
//! option only by *extension identity* — declared in the vendored
//! `keryx/options.proto` (architecture §6) — so a key reaching here is always
//! one of that fixed, all-lowercase-initial registry, never a name a foreign
//! schema coined. No proto-derived text reaches `Name::new`, so its `expect`
//! is a discharged invariant (§6). Internal to `facts`.

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
pub(super) fn konst(name: &str) -> Term {
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

/// A validated keryx-vocabulary identifier — a fixed, all-legal set, so the
/// `expect` is discharged (§6); proto text never arrives here (it uses `text`).
fn vocabulary(name: &str) -> Name {
    Name::new(name).expect("keryx vocabulary is a valid identifier")
}
