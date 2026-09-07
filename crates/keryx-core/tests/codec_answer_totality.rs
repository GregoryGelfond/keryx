//! Totality of the `.lp` answer-set door (spec §12.3; the threat model's property 3, branch (a)) —
//! the outbound door's *input* side, the mirror of the payload door's decode totality. Over any
//! text, `raise_answer_set` returns a `Vec<Symbol>` or typed `Diagnostics`; it never panics,
//! aborts, or hangs. themelios's parser bounds its own nesting and its `raise` is total, so no
//! pre-parse guard of keryx's precedes them; a fault in either is contained (`DependencyFault`,
//! defense-in-depth, no known trigger). Every other refusal is `UnreadableAnswerSet` — a parse
//! error, a rule with a body, a disjunctive or choice head, a non-ground atom — never a partial
//! read, never another door's kind. The branch-(a) fuzz instrument.

use keryx_core::codec::raise_answer_set;
use keryx_core::diagnostics::DiagnosticKind;
use proptest::prelude::*;

/// Whether `kind` is one the `.lp` door's contract names — the unreadable-answer-set refusal, or
/// (defense-in-depth) a contained themelios fault — never a kind of another door.
fn is_a_door_kind(kind: DiagnosticKind) -> bool {
    matches!(
        kind,
        DiagnosticKind::UnreadableAnswerSet | DiagnosticKind::DependencyFault
    )
}

/// Text with a clingo flavour: fact-ish atoms with argument lists, clingo punctuation, and
/// arbitrary characters — enough to reach the parser's own error recovery and `raise`'s groundness
/// check, not only noise the lexer rejects at the first byte.
fn arb_clingo_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z_][a-z0-9_]{0,7}(\\([a-z0-9_, \"()]{0,24}\\))?\\.?",
        "[a-zA-Z0-9_(), .:%|~\\-\"]{0,64}",
        ".{0,64}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Over any text, `raise_answer_set` returns — never panics — and every diagnosis is a door
    /// kind. Several fragments joined, to reach the per-statement and whole-program paths alike. A
    /// `DependencyFault` here would be a found trigger of the themelios containment frame (branch
    /// (a)), to be recorded, not hidden; none is expected.
    #[test]
    fn raise_answer_set_returns_over_any_text(
        parts in prop::collection::vec(arb_clingo_text(), 0..6)
    ) {
        let text = parts.join(" ");
        match raise_answer_set(&text) {
            Ok(_) => {}
            Err(diagnostics) => {
                for diagnostic in diagnostics.iter() {
                    prop_assert!(
                        is_a_door_kind(diagnostic.kind()),
                        "an unexpected kind reached the .lp door: {:?}",
                        diagnostic.kind()
                    );
                }
            }
        }
    }
}
