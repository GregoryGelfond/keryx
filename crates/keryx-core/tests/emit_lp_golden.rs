//! Stage-2 emission of `emit.lp` (spec §13.3) — the response-root markers, the reachability
//! closure (§12.1), and the serializability obligations (§12.2) — pinned to golden `.lp` per
//! fixture in each mode. Each render is a pure, deterministic function of the `Mapping` (P3), so
//! equality is the whole contract. Goldens are generated once, verified by eye against §12, and
//! committed; a diff here is a real change, intended or a regression.

use keryx_test_support as support;

use keryx_core::descriptor::ingest;
use keryx_core::{emit, policy};

fn unit_of(fixture: &str) -> policy::Unit {
    let schema = ingest(&support::compile_fixture(fixture)).expect("ingests");
    let mapping = policy::map(&schema).expect("maps");
    mapping.units().first().expect("one unit").clone()
}

/// The unit of `fixture`'s named `package` — for a multi-package fixture, where `unit_of`'s
/// first-unit choice is not the one under test.
fn unit_of_package(fixture: &str, package: &str) -> policy::Unit {
    let schema = ingest(&support::compile_fixture(fixture)).expect("ingests");
    let mapping = policy::map(&schema).expect("maps");
    mapping
        .units()
        .iter()
        .find(|unit| unit.package().as_str() == package)
        .expect("the named package's unit")
        .clone()
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

// Every scalar-valued form on one sort, each obligation `reach`-guarded and relative to its
// sort (§12.2): an IMPLICIT field gets functionality *and* totality where an EXPLICIT one gets
// functionality alone; a sequence gets contiguity from 0 (the index witness, the gap rule, the
// non-negative rule); a map gets key functionality; a two-arm oneof gets its pairwise
// exclusivity; an enum field gets membership against the enum's table, and a uint32 the
// non-negative range — each in its singular, sequence, map-value, and map-key instance. The
// predicate `sensor` is shared by `Gauge` and `Alarm` (§4.2): every obligation carries its own
// sort atom, so neither sort's obligations fire on the other's occupants. Strict writes each
// obligation as an integrity constraint; diagnostic derives `violates(path, occupant)` with the
// field's fully-qualified proto path (the oneof's own path for exclusivity, the sort's for the
// root instance of occupancy). Canonical order in both modes: the rules by head, the
// constraints by body, the `#defined`s after.
golden!(
    obligations_strict,
    "obligations.proto",
    emit::emit_strict,
    "golden/obligations.emit.lp"
);
golden!(
    obligations_diagnostic,
    "obligations.proto",
    emit::emit_diagnostic,
    "golden/obligations.emit-diagnostic.lp"
);

// A proto2 `required` field is totality-obliged outbound (E1): it gets functionality *and*
// totality, where a proto2 `optional` (EXPLICIT) field gets functionality alone — the mapping
// carries the distinction (`Totality::Required`), full proto2/proto3 parity; its inbound signature
// stays partial. The unreferenced enum still gets its membership table: the table is the enum's
// own, so a field in another package can hold its values to it.
golden!(
    required_strict,
    "proto2.proto",
    emit::emit_strict,
    "golden/proto2.emit.lp"
);
golden!(
    required_diagnostic,
    "proto2.proto",
    emit::emit_diagnostic,
    "golden/proto2.emit-diagnostic.lp"
);

// E1 (#1): a proto2 `required` field is totality-obliged outbound — it mints the presence
// witness and carries the totality constraint an IMPLICIT (`Total`) field does, which an EXPLICIT
// (`optional`) one does not. Its inbound signature stays partial (presence read from the message);
// only the outbound obligation is added.
#[test]
fn a_proto2_required_field_carries_the_outbound_totality_obligation() {
    let strict = emit::emit_strict(&unit_of("proto2.proto")).expect("emits");
    assert!(
        strict.contains("has_id(P) :- id(P, _)."),
        "the required `id` field mints its presence witness:\n{strict}"
    );
    assert!(
        strict.contains("not has_id(P)."),
        "the required `id` field carries the totality obligation:\n{strict}"
    );
    assert!(
        !strict.contains("not has_quantity(P)."),
        "the optional `quantity` field carries no totality obligation:\n{strict}"
    );
}

// The closure over every message-typed form on one parent sort — a singular field, a sequence,
// a map, and a message-typed oneof arm — each reaching its occupant through the safe idiom: the
// child sort atom binds the occupant, and the equality deconstructs it to bind the index or key
// (§12.1). Every message sort gets a marker, and the marker carries its signature line alone —
// the sort's proto prose stays on `core.lp`, which the module includes. A message-typed slot has
// no field atom of its own (§4.1), so its obligations join on occupancy: the sequence's index
// witness reads `step(steps(P, I))`, the message arm's presence in the exclusivity pair is
// `step(pick(P))`, and functionality of a singular slot is structural (one term), so none is
// written. Occupancy consistency is emitted from the parent over each slot — a field atom of the
// child sort (the base atom of `note`, the occupancy atom of `next`'s slot) on the occupant
// obliges the occupant's sort atom — plus the root instance under each marker. The statements
// render in themelios's canonical order — rules by head then body, the `#defined`s after — not
// the emitter's, in both modes alike.
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

// A holder with a `map<uint32, message>` and a message field into another package — two `emit.lp`
// shapes the single-package fixtures do not reach. The map's key range is read through the
// occupant's sort atom `inner(by_id(P, K))`, not a field atom, since the value is a message; and
// the cross-package field `thing` gets the reach step and the root instance of occupancy but *no*
// `slot_occupancy` obligation over the child's fields — the child sort lives in the other package's
// unit, so occupancy there is that unit's (the cross-unit early return). `scripts/ground.sh`
// grounds this theory together with the dependency package's `core.lp`, so the cross-package sort
// atom resolves — the way the two packages' files load together. The `keryx.gmap` unit is not the
// first (its package sorts after `keryx.gdep`), so it is selected by name.
#[test]
fn crossmap_strict() {
    let unit = unit_of_package("gmap.proto", "keryx.gmap");
    assert_eq!(
        emit::emit_strict(&unit).expect("emits"),
        include_str!("golden/gmap.emit.lp")
    );
}
#[test]
fn crossmap_diagnostic() {
    let unit = unit_of_package("gmap.proto", "keryx.gmap");
    assert_eq!(
        emit::emit_diagnostic(&unit).expect("emits"),
        include_str!("golden/gmap.emit-diagnostic.lp")
    );
}
