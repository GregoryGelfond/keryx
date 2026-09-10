//! Read an answer set from `.lp` text (the outbound door's CLI-seam input) through the contained
//! themelios door — the inverse of the `.lp` fact rendering (spec §17, the `.lp` seam). An answer
//! set is ground facts; this parses and raises the text, then collects each fact's head as a
//! `Symbol` — the `Vec<Symbol>` the reassembler ([`super::Codec::reassemble`]) reads, the same shape
//! a shred produces and a consuming tool's solver emits.
//!
//! **Branch (a) at the crossing (the threat model's property 3).** themelios's parser bounds its own
//! nesting and its `raise` is total, so no pre-parse guard of keryx's precedes them; the parse and
//! raise — reached through the one `themelios_program::raise_source` door — are contained
//! (`Dependency::Themelios`) as defense-in-depth, with no known trigger. The `Source` is built on
//! keryx's side, its size bound refused as `UnreadableAnswerSet`, not a fault. The reader returns
//! `Vec<Symbol>`, a value the surface already speaks, and keryx touches only the program crate — no
//! themelios-syntax type is named here or crosses keryx's boundary.

use themelios_program::prelude::*;
use themelios_program::raise::raise_source;
use themelios_program::term::TermParts;

use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::fault::{Dependency, contain};

/// Read an answer set from `.lp` text: parse and raise it (contained), then collect each ground
/// positive-atom fact's head as a `Symbol`. Every statement must be such a fact — an answer set is
/// facts; a parse or raise diagnostic, a rule with a body, a constraint or directive, a disjunctive
/// or choice head, or a non-ground atom is `UnreadableAnswerSet`, never a partial read.
///
/// # Errors
///
/// `UnreadableAnswerSet` when the text does not parse and raise to a set of ground facts;
/// `DependencyFault` for a contained themelios fault (branch (a), defense-in-depth).
pub fn raise_answer_set(text: &str) -> Result<Vec<Symbol>, Diagnostics> {
    let source = Source::new(SourceId::new(0), text.to_owned())
        .map_err(|_| unreadable("the answer set is larger than keryx reads"))?;
    // The closure borrows only `source` (keryx-built) and returns the `RaisedSource`; a fault drops
    // it with the unwind, so nothing keryx observes survives it. `raise_source` parses then raises,
    // and neither holds process-global state a fault could leave inconsistent: the program tier
    // "does no I/O, holds no global state, interns nothing" and hands out only owned
    // `Send + Sync + 'static` data (themelios-program `lib.rs`, its crate contract), and the syntax
    // parse interns only within the one `GreenNodeBuilder` minted per call (rowan's per-builder
    // `NodeCache`; themelios-syntax keeps no `static`/`thread_local`/locked cache) and reads no
    // filesystem — so the `AssertUnwindSafe` is sound. (themelios `653ca5b`.)
    let raised = contain(Dependency::Themelios, "reading an answer set", || {
        raise_source(&source, Dialect::Clingo)
    })?;
    // The parser is error-tolerant (it recovers a tree from malformed input), so its syntax
    // diagnostics are the first refusal, then the lowering (raise) diagnostics — neither surfaces
    // the other, so both are checked.
    if !raised.syntax_diagnostics().is_empty() {
        return Err(unreadable("the answer set does not parse as clingo facts"));
    }
    if !raised.lowering_diagnostics().is_empty() {
        return Err(unreadable("the answer set does not raise to ground facts"));
    }
    let mut symbols = Vec::new();
    for statement in raised.program().statements() {
        symbols.push(fact_symbol(statement.get())?);
    }
    Ok(symbols)
}

/// The head `Symbol` of one statement, which must be a ground positive-atom fact.
fn fact_symbol(statement: &Statement) -> Result<Symbol, Diagnostics> {
    let Statement::Rule(rule) = statement else {
        return Err(unreadable(
            "an answer set is facts; a statement is not a rule",
        ));
    };
    if !rule.is_fact() {
        return Err(unreadable("an answer set is facts; a rule carries a body"));
    }
    let Head::Literal(literal) = rule.head().get() else {
        return Err(unreadable(
            "a fact's head is a single atom, not a disjunction or choice",
        ));
    };
    let LiteralInner::Atom(atom) = &literal.inner else {
        return Err(unreadable("a fact's head is an atom"));
    };
    let atom = atom.get();
    let Arguments::Single(terms) = &atom.arguments else {
        return Err(unreadable(
            "a fact's head is one atom, not an argument pool",
        ));
    };
    let mut arguments = Vec::with_capacity(terms.len());
    for term in terms {
        match term.clone().into_parts() {
            TermParts::Symbolic(symbol) => arguments.push(symbol),
            _ => return Err(unreadable("a fact is ground; this one carries a variable")),
        }
    }
    Ok(Symbol::Function {
        name: atom.name.clone(),
        arguments,
        sign: atom.sign,
    })
}

/// `UnreadableAnswerSet` at the whole-text locus (the `.lp` is the unit read, so no finer locus).
fn unreadable(detail: &str) -> Diagnostics {
    Diagnostic::new(
        DiagnosticKind::UnreadableAnswerSet,
        Locus::whole(),
        detail.to_owned(),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::raise_answer_set;
    use crate::diagnostics::DiagnosticKind;
    use themelios_program::prelude::*;

    /// A positive atom `pred(args…)`.
    fn atom(pred: &str, arguments: Vec<Symbol>) -> Symbol {
        Symbol::Function {
            name: Name::new(pred).expect("an identifier"),
            arguments,
            sign: Sign::Positive,
        }
    }

    /// A constant `name` — a positive zero-argument function.
    fn constant(name: &str) -> Symbol {
        atom(name, Vec::new())
    }

    #[test]
    fn a_ground_fact_module_raises_to_its_symbols() {
        // The `.lp` an inbound shred renders — occupancy, field, and marker facts, with a nested
        // occupant term, a string, and a number — reads back to exactly those head symbols.
        let text = "emit_reading_batch(b0). reading_batch(b0). \
                    reading(readings(b0, 0)). sensor(readings(b0, 0), \"s-1\"). \
                    temp_c(readings(b0, 0), 21).";
        let mut symbols = raise_answer_set(text).expect("the answer set reads");
        symbols.sort();
        let element = atom("readings", vec![constant("b0"), Symbol::Number(0)]);
        let mut expected = vec![
            atom("emit_reading_batch", vec![constant("b0")]),
            atom("reading_batch", vec![constant("b0")]),
            atom("reading", vec![element.clone()]),
            atom(
                "sensor",
                vec![element.clone(), Symbol::String("s-1".to_owned())],
            ),
            atom("temp_c", vec![element, Symbol::Number(21)]),
        ];
        expected.sort();
        assert_eq!(symbols, expected);
    }

    #[test]
    fn a_syntactically_bad_answer_set_is_unreadable_never_a_panic() {
        let error =
            raise_answer_set("emit_reading(r0").expect_err("an unterminated atom does not read");
        assert!(
            error
                .iter()
                .any(|d| d.kind() == DiagnosticKind::UnreadableAnswerSet)
        );
    }

    #[test]
    fn a_rule_with_a_body_is_not_an_answer_set() {
        // An answer set is facts; a rule with a body is not one — refused, never read as its head.
        let error = raise_answer_set("reading(r0) :- emit_reading(r0).")
            .expect_err("a rule with a body does not read");
        assert!(
            error
                .iter()
                .any(|d| d.kind() == DiagnosticKind::UnreadableAnswerSet)
        );
    }

    #[test]
    fn a_non_ground_fact_is_not_an_answer_set() {
        // A variable in a "fact" makes it non-ground — an answer set is ground, so this is refused.
        let error =
            raise_answer_set("sensor(r0, X).").expect_err("a non-ground fact does not read");
        assert!(
            error
                .iter()
                .any(|d| d.kind() == DiagnosticKind::UnreadableAnswerSet)
        );
    }

    // themelios's parse and raise are total with no known fault trigger (unlike the payload door's
    // engine, which a crafted descriptor can panic), so a forced-panic poison test as at the
    // descriptor door is not constructible here. The observable guarantee the containment's
    // discharge protects — a refused read leaves nothing that disturbs a later one — is pinned by
    // refusing a malformed answer set, then reading a well-formed one cleanly.
    #[test]
    fn a_refused_read_does_not_disturb_a_later_read() {
        raise_answer_set("emit_reading(r0").expect_err("the malformed read is refused");
        let symbols = raise_answer_set("reading_batch(b0).").expect("the later read is clean");
        assert_eq!(symbols, vec![atom("reading_batch", vec![constant("b0")])]);
    }
}
