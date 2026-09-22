//! `Codec::reassemble` (spec §12.3) — the outbound door, over the public surface: an answer set of
//! `Symbol`s in the generated vocabulary reassembles to the messages its `emit_<sort>` markers name,
//! each carrying its root and encoded to the binary wire form. The round-trip property proper is in
//! `codec_roundtrip.rs`; this pins the door's shape — one message per marker, the root carried, the
//! results marker-ordered.

use std::path::Path;

use keryx_core::codec::{Codec, PayloadFormat};
use themelios_program::{Name, Sign, Symbol};

/// The thermal example's codec (spec §28), through the source door.
fn thermal_codec() -> Codec {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    Codec::from_source(&[example.join("thermal.proto")], &[example, vendored])
        .expect("the thermal example compiles")
}

/// A constant symbol — a positive zero-argument function.
fn constant(name: &str) -> Symbol {
    atom(name, Vec::new())
}

/// A positive atom `pred(args…)`.
fn atom(pred: &str, arguments: Vec<Symbol>) -> Symbol {
    Symbol::Function {
        name: Name::new(pred).expect("an identifier"),
        arguments,
        sign: Sign::Positive,
    }
}

/// The facts of one `Reading` under the root `root`, exported by its marker.
fn reading(root: &str, sensor: &str, temp_c: i32) -> Vec<Symbol> {
    vec![
        atom("emit_reading", vec![constant(root)]),
        atom("reading", vec![constant(root)]),
        atom(
            "sensor",
            vec![constant(root), Symbol::String(sensor.to_owned())],
        ),
        atom("temp_c", vec![constant(root), Symbol::Number(temp_c)]),
    ]
}

#[test]
fn an_answer_set_reassembles_one_message_carrying_its_type_root_and_bytes() {
    let codec = thermal_codec();
    let out = codec
        .reassemble(&reading("r0", "s-1", 21), PayloadFormat::Binary)
        .expect("the answer set reassembles");
    assert_eq!(out.messages().len(), 1);
    let message = &out.messages()[0];
    assert_eq!(message.type_name(), "thermal.v1.Reading");
    assert_eq!(
        message.bytes(),
        keryx_test_support::wire::reading("s-1", 21)
    );
    assert_eq!(message.root(), &constant("r0"));
}

#[test]
fn two_roots_of_one_type_are_distinct_and_marker_ordered() {
    // Two `Reading` roots in one answer set, given out of order: the results are distinguished by
    // their root and ordered by the marker atom's `Symbol::Ord` — `emit_reading(r0)` before
    // `emit_reading(r1)` — regardless of the answer set's order, so the door is deterministic.
    let codec = thermal_codec();
    let mut answer = reading("r1", "b", 2);
    answer.extend(reading("r0", "a", 1));
    let out = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("both roots reassemble");
    assert_eq!(out.messages().len(), 2);
    assert_eq!(out.messages()[0].root(), &constant("r0"));
    assert_eq!(out.messages()[1].root(), &constant("r1"));
    assert_eq!(
        out.messages()[0].bytes(),
        keryx_test_support::wire::reading("a", 1)
    );
    assert_eq!(
        out.messages()[1].bytes(),
        keryx_test_support::wire::reading("b", 2)
    );
}

/// The proto2 example's codec (§5, §7.4), through the source door.
fn proto2_codec() -> Codec {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/proto2");
    Codec::from_source(&[example.join("order.proto")], &[example])
        .expect("the proto2 example compiles")
}

/// The sets fixture's codec (spec §7.1), through the source door.
fn sets_codec() -> Codec {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    Codec::from_source(&[fixtures.join("sets.proto")], &[&fixtures, &vendored])
        .expect("the sets fixture compiles")
}

#[test]
fn a_set_member_of_a_different_sort_is_refused_not_built_as_the_element_sort() {
    // Integrity (threat model property 4, spec §7.1): the reassembler's set-member occupancy re-check
    // is keyed to the *element* sort, not "an occupant of some sort". `Holder.items` is a set of
    // `Empty` (a message with no field); this answer set names a member the answer set declares an
    // occupant of a *different* sort — `tag(foo)`, not `empty(foo)`. Since `Empty` has no field, a
    // member misfiled under any sort would reassemble to a clean invented empty message if the
    // re-check accepted an occupant of some sort; keyed to the element sort, it is refused. Mirrors
    // `emit.lp`'s `:- reach(P), holder(P), items(P, E), not empty(E).` on the adversarial-input path
    // (an answer set / `.lp` fixture, no serializability theory in the loop).
    let codec = sets_codec();
    let answer = vec![
        atom("emit_holder", vec![constant("h")]),
        atom("holder", vec![constant("h")]),
        atom("items", vec![constant("h"), constant("foo")]),
        atom("tag", vec![constant("foo")]),
    ];
    let error = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("a set member declared a different sort is refused");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == keryx_core::diagnostics::DiagnosticKind::ShapeViolation),
        "a wrong-sort set member is a shape violation, not a silent empty message"
    );
}

#[test]
fn a_provenance_shared_set_member_dag_is_refused_in_bounded_work() {
    // Bounded work (threat model property 2; property 1's "never hangs"): a message-set member is
    // named by its own provenance, so an adversarial answer set can name one occupant a member of
    // many parents — a DAG the un-budgeted walk would re-expand once per path, exponentially in
    // depth. keryx bounds the reassembly by the answer set's own atom count and refuses a walk that
    // would exceed it. This lattice — both `Node`s of each level are members of both `Node`s of the
    // level above — expands to 2^depth occupants from O(depth) atoms, so a naive walk hangs; the
    // budget refuses it after atom-count expansions. Depth is well under the nesting ceiling, so it
    // is the width budget (not the depth ceiling) that fires.
    let codec = sets_codec();
    let depth = 12; // ~8k expansions unbounded; refused after ~6*depth atoms with the budget.
    let node = |name: &str| atom("node", vec![constant(name)]);
    let kid = |parent: &str, child: &str| atom("kids", vec![constant(parent), constant(child)]);
    let mut answer = vec![atom("emit_node", vec![constant("p0")]), node("p0")];
    for level in 0..depth {
        let (pn, qn) = (format!("p{}", level + 1), format!("q{}", level + 1));
        answer.push(node(&pn));
        answer.push(node(&qn));
        for parent in [format!("p{level}"), format!("q{level}")] {
            answer.push(kid(&parent, &pn));
            answer.push(kid(&parent, &qn));
        }
    }
    let error = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("a provenance-shared set-member DAG is refused, not expanded exponentially");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == keryx_core::diagnostics::DiagnosticKind::ReassembledTooLarge),
        "the DAG is refused with ReassembledTooLarge (bounded work), never expanded"
    );
}

#[test]
fn a_proto2_required_field_omitted_is_a_shape_violation_outbound() {
    // A proto2 `required` field is totality-obliged outbound (#1), so an answer set naming an
    // `Order` root but omitting its `required` id is refused at reassembly (`ShapeViolation`) — the
    // reassembly-side enforcement paired with `emit.lp`'s totality obligation, full proto2/proto3
    // parity, exactly as an IMPLICIT (`Total`) field's absence is refused. An `Order` carrying its
    // id reassembles.
    let codec = proto2_codec();
    let with_id = vec![
        atom("emit_order", vec![constant("o0")]),
        atom("order", vec![constant("o0")]),
        atom(
            "id",
            vec![constant("o0"), Symbol::String("PO-1".to_owned())],
        ),
    ];
    codec
        .reassemble(&with_id, PayloadFormat::Binary)
        .expect("an order carrying its required id reassembles");
    let without_id = vec![
        atom("emit_order", vec![constant("o0")]),
        atom("order", vec![constant("o0")]),
    ];
    let error = codec
        .reassemble(&without_id, PayloadFormat::Binary)
        .expect_err("an order omitting its required id is refused");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == keryx_core::diagnostics::DiagnosticKind::ShapeViolation),
        "the missing required field is a shape violation"
    );
}

#[test]
fn a_broken_answer_set_is_every_diagnosis_never_a_partial_reassembly() {
    // One good root and one with a duplicate singular: the whole call fails (property 4), no
    // messages beside the diagnosis.
    let codec = thermal_codec();
    let mut answer = reading("r0", "a", 1);
    answer.extend(reading("r1", "b", 2));
    answer.push(atom(
        "sensor",
        vec![constant("r1"), Symbol::String("dup".to_owned())],
    ));
    let error = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("a duplicate singular fails the whole reassembly");
    assert!(
        error
            .iter()
            .any(|d| d.kind() == keryx_core::diagnostics::DiagnosticKind::ShapeViolation),
        "the duplicate is a shape violation"
    );
}
#[test]
fn a_set_member_shared_across_two_fields_reassembles_totally_never_panics() {
    // Totality (threat model property 1, "never panics"): a message-set member is named by its own
    // provenance, so one occupant can be a member of two set fields at once (spec §7.1, "may be shared
    // across parents … instantiates once per reference"). The build draws each reference by its own
    // plan-instance id, so a shared occupant is two independent built messages — never one removed
    // twice. `Pair.xs` and `Pair.ys` both name the same `Node` `c`; it reassembles to a `Pair` holding
    // `c` in each field, and the door does not panic on this adversary-reachable shape. (A single-set
    // shared member — a diamond — already reassembled; this pins the two-field case, where the old
    // occupant-term keying overwrote and double-removed.)
    let codec = sets_codec();
    let answer = vec![
        atom("emit_pair", vec![constant("r")]),
        atom("pair", vec![constant("r")]),
        atom("node", vec![constant("c")]),
        atom("xs", vec![constant("r"), constant("c")]),
        atom("ys", vec![constant("r"), constant("c")]),
    ];
    let out = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect("a member shared across two set fields reassembles, never panics");
    assert_eq!(out.messages().len(), 1);
    assert_eq!(out.messages()[0].type_name(), "keryx.sets.Pair");
    // xs (field 1) and ys (field 2) each carry one empty `Node` — the shared member copied per field.
    assert_eq!(out.messages()[0].bytes(), vec![0x0A, 0x00, 0x12, 0x00]);
}

#[test]
fn a_self_cycle_set_member_is_refused_by_a_bounded_limit_never_looped() {
    // Termination (spec §12.3): a message-set member named by provenance can be an ancestor — here a
    // `Node` that is its own kid, a cycle a path-term occupant could never form. Reassembly is
    // bounded, not structural: a tight cycle exceeds the atom-count budget (`ReassembledTooLarge`),
    // a deep one the nesting ceiling (`ReassembledTooDeep`) — either way a bounded refusal, never a
    // loop or a hang.
    let codec = sets_codec();
    let answer = vec![
        atom("emit_node", vec![constant("r")]),
        atom("node", vec![constant("r")]),
        atom("kids", vec![constant("r"), constant("r")]),
    ];
    let error = codec
        .reassemble(&answer, PayloadFormat::Binary)
        .expect_err("a self-cycle set member is refused, never looped");
    assert!(
        error.iter().any(|d| matches!(
            d.kind(),
            keryx_core::diagnostics::DiagnosticKind::ReassembledTooLarge
                | keryx_core::diagnostics::DiagnosticKind::ReassembledTooDeep
        )),
        "a cycle is refused by a bounded limit, never looped or panicked"
    );
}
