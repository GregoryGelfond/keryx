//! The reassembler's depth posture (the threat model's property 3, branch (b)): an occupant chain
//! deeper than the reconstruction ceiling is refused (`ReassembledTooDeep`) *before* any message is
//! built or encoded, and a chain at the ceiling reassembles and encodes on keryx's own sized thread
//! — so a sub-standard caller thread, on which the engine's unbounded serializer would overflow,
//! does not. Over `recursion.proto`'s `Tree { string label = 1; repeated Tree children = 2; }`.

use keryx_core::codec::Codec;
use keryx_core::diagnostics::DiagnosticKind;
use keryx_test_support as support;
use themelios_program::{Name, Sign, Symbol};

fn rec_codec() -> Codec {
    Codec::new(&support::compile_fixture("recursion.proto"))
        .expect("recursion.proto builds a codec")
}

fn constant(name: &str) -> Symbol {
    Symbol::Function {
        name: Name::new(name).expect("an identifier"),
        arguments: Vec::new(),
        sign: Sign::Positive,
    }
}

fn atom(pred: &str, arguments: Vec<Symbol>) -> Symbol {
    Symbol::Function {
        name: Name::new(pred).expect("an identifier"),
        arguments,
        sign: Sign::Positive,
    }
}

/// An answer set for a single-child `Tree` chain `depth` occupants deep below the root `t0`: each
/// occupant `O_i` is `children(O_{i-1}, 0)`, declared by its `tree` occupancy atom and carrying its
/// total `label`. The root `t0` sits at depth 0, so a chain built with `depth` children reaches an
/// occupant at depth `depth`.
fn tree_chain(depth: usize) -> Vec<Symbol> {
    let mut answer = vec![atom("emit_tree", vec![constant("t0")])];
    let mut occupant = constant("t0");
    for _ in 0..=depth {
        answer.push(atom("tree", vec![occupant.clone()]));
        answer.push(atom(
            "label",
            vec![occupant.clone(), Symbol::String("x".to_owned())],
        ));
        occupant = atom("children", vec![occupant, Symbol::Number(0)]);
    }
    answer
}

#[test]
fn a_chain_past_the_ceiling_is_reassembled_too_deep() {
    // A chain 100 message-fields below the root sits one past the ceiling of 99 — refused before any
    // message is built.
    let codec = rec_codec();
    let error = codec
        .reassemble(&tree_chain(100))
        .expect_err("a chain past the ceiling is refused");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == DiagnosticKind::ReassembledTooDeep),
        "the chain is refused as too deep"
    );
}

#[test]
fn a_chain_at_the_ceiling_encodes_on_keryx_s_thread_not_the_caller_s() {
    // A chain at the ceiling (99 levels below the root) reassembles and encodes; the encode recurses
    // that deep natively in the engine's serializer, which would overflow the small caller thread
    // below — but it runs on keryx's own sized `ENCODE_STACK` thread, so this succeeds. The caller
    // thread is sized well under the engine's ~2.5 MB debug need at the ceiling; the walk itself is a
    // heap stack, so it does not spend it.
    let codec = rec_codec();
    let answer = tree_chain(99);
    let reassembled = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("substandard-caller".to_owned())
            .stack_size(512 * 1024)
            .spawn_scoped(scope, || codec.reassemble(&answer))
            .expect("the host can spawn the caller thread")
            .join()
            .expect("the caller thread does not overflow")
    });
    let out = reassembled.expect("a chain at the ceiling reassembles");
    assert_eq!(out.messages().len(), 1);
    assert_eq!(out.messages()[0].type_name(), "keryx.rec.Tree");
    assert!(
        !out.messages()[0].bytes().is_empty(),
        "the ceiling-deep tree encodes to bytes"
    );
}

#[test]
fn a_huge_sequence_index_is_refused_without_sizing_an_allocation() {
    // A `children` element at index `i32::MAX` — an index sized by the input (property 2, bounded
    // work) — is refused as non-dense (`ShapeViolation`) without sizing a list of that length: the
    // walk keys the slot's entries by their count, never by an index read from the answer set, so
    // this returns at once rather than trying to reserve two billion slots.
    let codec = rec_codec();
    let huge = atom("children", vec![constant("t0"), Symbol::Number(i32::MAX)]);
    let answer = vec![
        atom("emit_tree", vec![constant("t0")]),
        atom("tree", vec![constant("t0")]),
        atom(
            "label",
            vec![constant("t0"), Symbol::String("x".to_owned())],
        ),
        atom("tree", vec![huge]),
    ];
    let error = codec
        .reassemble(&answer)
        .expect_err("a non-dense sequence is refused");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == DiagnosticKind::ShapeViolation),
        "the huge index is a shape violation, refused at once"
    );
}
