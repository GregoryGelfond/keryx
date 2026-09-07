//! The map + cross-package example (§7.2, §4.1): documentation by example and a regression suite
//! in one, in the `thermal_example` mould. Regenerate the vocabulary for both packages and the
//! shred and assert they equal the committed `examples/map/gen/*`. A `map<k, v>` becomes a family
//! keyed by the map key — a first-class access-path term — and a cross-package message field keeps
//! its identity across the boundary, its sort and field predicates the other package's.

use std::path::{Path, PathBuf};

use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::descriptor::compile;
use keryx_core::emit::Shape;
use keryx_core::policy::{self, Mapping, Unit};
use keryx_core::{emit, manifest};

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/map")
}

fn golden(name: &str) -> String {
    std::fs::read_to_string(example().join("gen").join(name)).expect("golden present")
}

/// The unit of `package` in the mapping — the example spans two, so units are selected by name.
fn unit<'m>(mapping: &'m Mapping, package: &str) -> &'m Unit {
    mapping
        .units()
        .iter()
        .find(|unit| unit.package().as_str() == package)
        .unwrap_or_else(|| panic!("{package} unit"))
}

/// A unit's four generated artifacts equal the committed `<package>.*` goldens.
fn assert_unit_goldens(unit: &Unit, package: &str) {
    assert_eq!(
        emit::core(unit).expect("core"),
        golden(&format!("{package}.core.lp"))
    );
    assert_eq!(
        emit::views(unit).expect("views"),
        golden(&format!("{package}.views.lp"))
    );
    assert_eq!(
        emit::emit_strict(unit).expect("emit"),
        golden(&format!("{package}.emit.lp"))
    );
    assert_eq!(
        manifest::write(unit, "-", Shape::Strict),
        golden(&format!("{package}.keryx-manifest"))
    );
}

#[test]
fn map_gen_matches_the_committed_example_for_both_packages() {
    let example = example();
    let schema = compile(
        &[example.join("inventory.proto")],
        std::slice::from_ref(&example),
    )
    .expect("inventory compiles");
    let mapping = policy::map(&schema).expect("maps");
    assert_unit_goldens(unit(&mapping, "inventory.v1"), "inventory.v1");
    assert_unit_goldens(unit(&mapping, "catalog.v1"), "catalog.v1");
}

#[test]
fn map_facts_match_the_committed_example_with_keyed_and_cross_package_identity() {
    let example = example();
    let payload = std::fs::read(example.join("warehouse.txtpb")).expect("payload present");
    let codec = Codec::from_source(
        &[example.join("inventory.proto")],
        std::slice::from_ref(&example),
    )
    .expect("the map example builds a codec");
    let facts = codec
        .shred(
            "inventory.v1.Warehouse",
            &payload,
            PayloadFormat::Textproto,
            &Root::fresh(0),
        )
        .expect("the warehouse shreds");
    let rendered = facts.render().expect("the facts render");
    assert_eq!(rendered, golden("inventory.v1.facts.lp"));

    // The map key is a first-class access-path term (`bins(r0, "a-1")`), and the cross-package
    // Sku keeps its identity: `featured(r0)` carries catalog.v1's `sku`/`code` predicates.
    assert!(rendered.contains(r#"bin(bins(r0, "a-1"))"#));
    assert!(rendered.contains(r#"quantity(bins(r0, "a-1"), 12)"#));
    assert!(rendered.contains("sku(featured(r0))"));
    assert!(rendered.contains("code(featured(r0), 4090)"));
}
