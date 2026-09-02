//! The manifest, pinned to golden `.manifest` per fixture (spec §13.4). `write` is a pure,
//! deterministic function of the `Mapping` and a caller-supplied schema hash (P3), so
//! equality is the whole contract. The header carries the keryx version and the schema hash
//! — both volatile — so the test normalizes it out (drops the `schema-hash ` line) and passes
//! a fixed placeholder hash before comparing. Goldens are generated once, verified by eye
//! against §13.4/Appendix B, and committed; a diff here is a real change, intended or a
//! regression.

use keryx_test_support as support;

use keryx_core::descriptor::ingest;
use keryx_core::{manifest, policy};

fn body(fixture: &str) -> String {
    let schema = ingest(&support::compile_fixture(fixture)).expect("ingests");
    let mapping = policy::map(&schema).expect("maps");
    let unit = mapping.units().first().expect("one unit");
    let text = manifest::write(unit, "sha256:PLACEHOLDER");
    // Drop the version-bearing header line; the golden pins the vocabulary body (P3), which
    // is what the evolution contract is about — the header's keryx version is not stable.
    text.lines()
        .filter(|line| !line.starts_with("schema-hash "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

macro_rules! golden {
    ($name:ident, $fixture:literal, $golden:literal) => {
        #[test]
        fn $name() {
            assert_eq!(body($fixture), include_str!($golden));
        }
    };
}

golden!(proto3_manifest, "proto3.proto", "golden/proto3.manifest");
golden!(maps_manifest, "maps.proto", "golden/maps.manifest");
// The `[escaped]` note on a reserved-word-escaped field, pinned where it is seen (§13.4): the
// `reach` -> `reach_` escape renders `... reach_/2  int32  total [escaped]`, the sort/enum form.
golden!(
    field_lowering_manifest,
    "field_lowering.proto",
    "golden/field_lowering.manifest"
);
