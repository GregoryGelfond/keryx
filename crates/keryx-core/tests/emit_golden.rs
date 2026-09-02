//! Stage-2 emission (`core.lp`/`views.lp`), pinned to golden `.lp` per fixture (spec
//! §13.1, §13.2). Each render is a pure, deterministic function of the `Mapping` (P3), so
//! equality is the whole contract. Goldens are generated once (below), verified by eye
//! against §13.1/§13.2, and committed; a diff here is a real change, intended or a
//! regression.

use keryx_test_support as support;

use keryx_core::descriptor::ingest;
use keryx_core::{emit, policy};

fn unit_of(fixture: &str) -> keryx_core::policy::Unit {
    let schema = ingest(&support::compile_fixture(fixture)).expect("ingests");
    let mapping = policy::map(&schema).expect("maps");
    mapping.units().first().expect("one unit").clone()
}

macro_rules! golden {
    ($name:ident, $fixture:literal, $emit:path, $golden:literal) => {
        #[test]
        fn $name() {
            let unit = unit_of($fixture);
            assert_eq!($emit(&unit).expect("emits"), include_str!($golden));
        }
    };
}

golden!(
    proto3_core,
    "proto3.proto",
    emit::core,
    "golden/proto3.core.lp"
);
golden!(
    proto3_views,
    "proto3.proto",
    emit::views,
    "golden/proto3.views.lp"
);
golden!(maps_core, "maps.proto", emit::core, "golden/maps.core.lp");
golden!(
    maps_views,
    "maps.proto",
    emit::views,
    "golden/maps.views.lp"
);
golden!(
    shared_field_core,
    "shared_field.proto",
    emit::core,
    "golden/shared_field.core.lp"
);
// Field-name lowering and a reserved-word field escape (§4.2), pinned in emitted `core.lp`:
// `camelField`/`PascalField` lower, `reach` escapes to `reach_`, each with its signature doc.
golden!(
    field_lowering_core,
    "field_lowering.proto",
    emit::core,
    "golden/field_lowering.core.lp"
);
