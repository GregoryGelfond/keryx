//! The enum example (§7.4): documentation by example and a regression suite in one, in the
//! `thermal_example` mould. Regenerate the vocabulary and the shred and assert they equal the
//! committed `examples/enum/gen/*`. A proto3 (open) enum becomes an open sort, its declared
//! values lower to constants, and a number the enum does not declare is a structured refusal at
//! the field's path — the check a hand-rolled shim skips.

use std::path::{Path, PathBuf};

use keryx_test_support::wire;

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::descriptor::compile;
use keryx_core::diagnostics::DiagnosticKind;
use keryx_core::emit::Shape;
use keryx_core::{emit, manifest, policy};

/// The example's directory (`examples/enum`).
fn example() -> PathBuf {
    // keryx-core/ -> crates/ -> repo root.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/enum")
}

/// The committed text of the `gen/` artifact `name`.
fn golden(name: &str) -> String {
    std::fs::read_to_string(example().join("gen").join(name)).expect("golden present")
}

#[test]
fn enum_gen_matches_the_committed_example() {
    let example = example();
    let schema = compile(
        &[example.join("signals.proto")],
        std::slice::from_ref(&example),
    )
    .expect("signals compiles");
    let mapping = policy::map(&schema).expect("maps");
    let unit = mapping.units().first().expect("signals.v1 unit");
    assert_eq!(
        emit::core(unit).expect("core"),
        golden("signals.v1.core.lp")
    );
    assert_eq!(
        emit::views(unit).expect("views"),
        golden("signals.v1.views.lp")
    );
    assert_eq!(
        emit::emit_strict(unit).expect("emit"),
        golden("signals.v1.emit.lp")
    );
    assert_eq!(
        manifest::write(unit, "-", Shape::Strict),
        golden("signals.v1.keryx-manifest")
    );
}

#[test]
fn enum_facts_match_the_committed_example() {
    // The committed textproto payload shreds to the committed fact module: what
    // `keryx facts --root Corridor=corridor.txtpb signals.proto -I .` prints, enum values as
    // constants (`PHASE_GO` -> `go`).
    let example = example();
    let payload = std::fs::read(example.join("corridor.txtpb")).expect("payload present");
    let codec = Codec::from_source(
        &[example.join("signals.proto")],
        std::slice::from_ref(&example),
    )
    .expect("the enum example builds a codec");
    let facts = codec
        .shred(
            "signals.v1.Corridor",
            &payload,
            PayloadFormat::Textproto,
            &Root::fresh(0),
        )
        .expect("the corridor shreds");
    assert_eq!(
        facts.render().expect("the facts render"),
        golden("signals.v1.facts.lp")
    );
}

#[test]
fn an_undeclared_enum_number_is_refused_at_the_path() {
    // A proto3 open enum admits unknown numbers on the wire; keryx cannot name a constant for one
    // the enum does not declare, so it refuses at the field's path (§7.4, §26) — where a
    // hand-rolled shim would silently pass a value it cannot mean.
    let example = example();
    let codec = Codec::from_source(
        &[example.join("signals.proto")],
        std::slice::from_ref(&example),
    )
    .expect("the enum example builds a codec");

    // A `Light { intersection: "5th & Main"; phase: 99 }` — 99 is a `Phase` number the enum does
    // not declare — inside a `Corridor`, written as bytes on the wire.
    let mut light = Vec::new();
    wire::delimited(1, b"5th & Main", &mut light);
    wire::int32(2, 99, &mut light);
    let mut corridor = Vec::new();
    wire::delimited(1, &light, &mut corridor);

    let diagnostics = codec
        .shred(
            "signals.v1.Corridor",
            &corridor,
            PayloadFormat::Binary,
            &Root::fresh(0),
        )
        .expect_err("an undeclared enum number is refused");
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.iter().next().expect("one diagnostic");
    assert_eq!(diagnostic.kind(), DiagnosticKind::UnknownEnumValue);
    assert_eq!(diagnostic.locus().path(), Some("signals.v1.Light.phase"));
    assert_eq!(
        diagnostic.detail(),
        "the value 99 matches no declared value of the enum `signals.v1.Phase`; an unknown number of an open enum is a translation error by default (§7.4) — annotate the field `(keryx.unknown) = PRESERVE` to carry it as `unknown(99)`",
        "the full diagnostic detail is pinned, so the README's verbatim quote cannot drift"
    );
}
