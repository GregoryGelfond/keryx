//! The proto2 example (§5, §7.4): documentation by example and a regression suite in one, in the
//! `thermal_example` mould. proto2 is a first-class input — it compiles, shreds, and generates a
//! vocabulary. A proto2 enum is a **closed** sort (vs proto3's open); `optional` is partial,
//! `repeated` a sequence, and `required` is totality-obliged outbound (`Totality::Required`,
//! rendered `required`): the `emit.lp` totality obligation and the manifest word distinguish it
//! from `optional` (full proto2/proto3 parity), while its inbound presence is read like `optional`'s.

use std::path::{Path, PathBuf};

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::descriptor::compile;
use keryx_core::emit::Shape;
use keryx_core::{emit, manifest, policy};

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/proto2")
}

fn golden(name: &str) -> String {
    std::fs::read_to_string(example().join("gen").join(name)).expect("golden present")
}

#[test]
fn proto2_gen_matches_the_committed_example() {
    let example = example();
    let schema = compile(
        &[example.join("order.proto")],
        std::slice::from_ref(&example),
    )
    .expect("order compiles");
    let mapping = policy::map(&schema).expect("maps");
    let unit = mapping.units().first().expect("orders.v1 unit");
    assert_eq!(emit::core(unit).expect("core"), golden("orders.v1.core.lp"));
    assert_eq!(
        emit::views(unit).expect("views"),
        golden("orders.v1.views.lp")
    );
    assert_eq!(
        emit::emit_strict(unit).expect("emit"),
        golden("orders.v1.emit.lp")
    );
    assert_eq!(
        manifest::write(unit, "-", Shape::Strict),
        golden("orders.v1.keryx-manifest")
    );
}

#[test]
fn proto2_facts_match_the_committed_example_across_the_presence_labels() {
    let example = example();
    let payload = std::fs::read(example.join("ledger.txtpb")).expect("payload present");
    let codec = Codec::from_source(
        &[example.join("order.proto")],
        std::slice::from_ref(&example),
    )
    .expect("the proto2 example builds a codec");
    let facts = codec
        .shred(
            "orders.v1.Ledger",
            &payload,
            PayloadFormat::Textproto,
            &Root::fresh(0),
        )
        .expect("the ledger shreds");
    let rendered = facts.render().expect("the facts render");
    assert_eq!(rendered, golden("orders.v1.facts.lp"));

    // A `required` id is present for both orders; an `optional` quantity only for the one that set
    // it; `repeated` tags is a sequence; the closed enum lowers to a constant.
    assert!(rendered.contains(r#"id(orders(r0, 0), "PO-1001")"#));
    assert!(rendered.contains(r#"id(orders(r0, 1), "PO-1002")"#));
    assert!(rendered.contains("quantity(orders(r0, 0), 5)"));
    assert!(!rendered.contains("quantity(orders(r0, 1)"));
    assert!(rendered.contains(r#"tags(orders(r0, 0), 1, "fragile")"#));
    assert!(rendered.contains("grade(orders(r0, 0), a)"));
}
