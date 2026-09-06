//! Stage-2 emission of `emit.lp` (spec §13.3) — the response-root markers and the
//! reachability closure (§12.1) — pinned to golden `.lp` per fixture in each mode. Each render
//! is a pure, deterministic function of the `Mapping` (P3), so equality is the whole contract.
//! Goldens are generated once, verified by eye against §12.1, and committed; a diff here is a
//! real change, intended or a regression.

use keryx_test_support as support;

use keryx_core::descriptor::ingest;
use keryx_core::{emit, policy};

fn unit_of(fixture: &str) -> policy::Unit {
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

// The closure over every message-typed form on one parent sort — a singular field, a sequence,
// a map, and a message-typed oneof arm — each reaching its occupant through the safe idiom: the
// child sort atom binds the occupant, and the equality deconstructs it to bind the index or key
// (§12.1). Every message sort gets a marker, and the marker carries its signature line alone —
// the sort's proto prose stays on `core.lp`, which the module includes. The statements render
// in themelios's canonical order — rules by head then body, the `#defined`s after — not the
// emitter's, in both modes alike.
golden!(
    reach_strict,
    "reach.proto",
    emit::emit_strict,
    "golden/reach.emit.lp"
);
golden!(
    reach_diagnostic,
    "reach.proto",
    emit::emit_diagnostic,
    "golden/reach.emit-diagnostic.lp"
);
