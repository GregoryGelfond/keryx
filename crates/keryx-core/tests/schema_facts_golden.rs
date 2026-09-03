//! Descriptor facts, pinned to golden `.lp` per fixture. The render is a pure,
//! deterministic function of the set (P3), so equality is the whole contract.
//! Goldens are generated once, verified by eye against Appendix C, and
//! committed; a diff here is a real change, intended or a regression.

use keryx_test_support as support;

use keryx_core::descriptor::ingest;
use keryx_core::schema_facts;

fn rendered(fixture: &str) -> String {
    schema_facts::render(&ingest(&support::compile_fixture(fixture)).expect("ingests"))
        .expect("renders")
}

macro_rules! golden {
    ($name:ident, $fixture:literal, $golden:literal) => {
        #[test]
        fn $name() {
            assert_eq!(rendered($fixture), include_str!($golden));
        }
    };
}

golden!(proto2_facts, "proto2.proto", "golden/proto2.lp");
golden!(proto3_facts, "proto3.proto", "golden/proto3.lp");
golden!(maps_facts, "maps.proto", "golden/maps.lp");
golden!(recursion_facts, "recursion.proto", "golden/recursion.lp");
golden!(options_facts, "options.proto", "golden/options.lp");
golden!(nested_facts, "nested.proto", "golden/nested.lp");
