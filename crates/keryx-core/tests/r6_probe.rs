//! R6 proven end to end at M0: protox produces the descriptor set, prost-reflect
//! reads it dynamically, keryx parses no protobuf. Engine-direct at the test level;
//! the production wrap `descriptor::ingest` will be exercised by the ingestion
//! tests at a later step.

use keryx_test_support as support;

use prost_reflect::DescriptorPool;

#[test]
fn protox_compiles_and_prost_reflect_reads() {
    let bytes = support::compile_fixture("smoke.proto");
    let pool = DescriptorPool::decode(&bytes[..]).expect("the set decodes");
    let message = pool
        .get_message_by_name("keryx.smoke.Ping")
        .expect("Ping is in the pool");
    let field = message.get_field_by_name("note").expect("note is a field");
    assert_eq!(field.number(), 1);
    assert!(matches!(field.kind(), prost_reflect::Kind::String));
}
