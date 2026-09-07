//! The oneof example (§7.3): documentation by example and a regression suite in one, in the
//! `thermal_example` mould. Regenerate the vocabulary and the shred and assert they equal the
//! committed `examples/oneof/gen/*`. A `oneof`'s arms are partial functions; only the arm the
//! payload sets shreds — absence is honest absence, where a hand-rolled shim often emits both
//! arms or a null.

use std::path::{Path, PathBuf};

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::descriptor::compile;
use keryx_core::emit::Shape;
use keryx_core::{emit, manifest, policy};

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/oneof")
}

fn golden(name: &str) -> String {
    std::fs::read_to_string(example().join("gen").join(name)).expect("golden present")
}

#[test]
fn oneof_gen_matches_the_committed_example() {
    let example = example();
    let schema = compile(
        &[example.join("dispatch.proto")],
        std::slice::from_ref(&example),
    )
    .expect("dispatch compiles");
    let mapping = policy::map(&schema).expect("maps");
    let unit = mapping.units().first().expect("dispatch.v1 unit");
    assert_eq!(
        emit::core(unit).expect("core"),
        golden("dispatch.v1.core.lp")
    );
    assert_eq!(
        emit::views(unit).expect("views"),
        golden("dispatch.v1.views.lp")
    );
    assert_eq!(
        emit::emit_strict(unit).expect("emit"),
        golden("dispatch.v1.emit.lp")
    );
    assert_eq!(
        manifest::write(unit, "-", Shape::Strict),
        golden("dispatch.v1.keryx-manifest")
    );
}

/// The committed outbox payload shredded to its rendered facts.
fn rendered_outbox() -> String {
    let example = example();
    let payload = std::fs::read(example.join("outbox.txtpb")).expect("payload present");
    let codec = Codec::from_source(
        &[example.join("dispatch.proto")],
        std::slice::from_ref(&example),
    )
    .expect("the oneof example builds a codec");
    let facts = codec
        .shred(
            "dispatch.v1.Outbox",
            &payload,
            PayloadFormat::Textproto,
            &Root::fresh(0),
        )
        .expect("the outbox shreds");
    facts.render().expect("the facts render")
}

#[test]
fn oneof_facts_match_the_committed_example() {
    assert_eq!(rendered_outbox(), golden("dispatch.v1.facts.lp"));
}

#[test]
fn oneof_shreds_only_the_present_arm() {
    // Notice 0 set `email`, notice 1 set `sms`: only the present arm of each shreds — the absent
    // arm has no atom (not a null, not both).
    let rendered = rendered_outbox();
    assert!(rendered.contains(r#"email(notices(r0, 0), "ops@example.com")"#));
    assert!(rendered.contains(r#"sms(notices(r0, 1), "+15551234")"#));
    assert!(!rendered.contains("email(notices(r0, 1)"));
    assert!(!rendered.contains("sms(notices(r0, 0)"));
}
