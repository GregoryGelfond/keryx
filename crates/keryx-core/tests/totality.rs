//! Totality of the descriptor door (§6; the threat model's totality property): `ingest` returns a
//! value or a typed `Diagnostics` over *any* input — it never panics, aborts, or hangs. Arbitrary
//! bytes overwhelmingly fail at the decoder; the structurally-invalid-but-*decodable* refusals keryx's
//! own walk determines (a malformed map entry, an unrepresentable syntax or name, a contained engine
//! fault) are exercised by the unit tests in the `descriptor` module, which hand-build the sets.

use proptest::prelude::*;

proptest! {
    #[test]
    fn ingest_is_total_over_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        // A value or `Diagnostics`, never a panic — this exercises the decoder's totality across the
        // whole byte space (arbitrary bytes overwhelmingly fail at `DescriptorPool::decode`, which is
        // exactly why the module's hand-built sets carry the refusals keryx's own logic determines).
        let _ = keryx_core::descriptor::ingest(&bytes);
    }
}
