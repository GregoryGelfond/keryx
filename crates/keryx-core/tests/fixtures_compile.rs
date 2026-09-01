//! Every fixture compiles under protox and decodes under prost-reflect — the
//! hermetic corpus is sound before the ingestion pass asserts on it.

mod support;

use prost_reflect::DescriptorPool;

#[test]
fn every_fixture_compiles_and_decodes() {
    for fixture in [
        "proto2.proto",
        "proto3.proto",
        "maps.proto",
        "recursion.proto",
        "options.proto",
        "nested.proto",
    ] {
        let bytes = support::compile_fixture(fixture);
        DescriptorPool::decode(&bytes[..])
            .unwrap_or_else(|error| panic!("`{fixture}` decodes: {error}"));
    }
}
