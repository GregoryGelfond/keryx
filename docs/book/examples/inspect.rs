//! A checked example for the keryx Book (Getting started): compile a schema and
//! read the mapping it produced. Built and run by `cargo test` (this example is
//! registered `test = true` in keryx-core's manifest); the book embeds the
//! `example` anchor below via `{{#include}}`.

// ANCHOR: example
use keryx_core::codec::Codec;

fn main() {
    // Compile a `.proto` schema into a self-contained descriptor set. Here a test
    // fixture stands in for your own schema; a tool would read its own bytes.
    let descriptor_set = keryx_test_support::compile_fixture("proto3.proto");

    // Build a Codec — one per schema — which holds the compiled Mapping.
    let codec = Codec::new(&descriptor_set).expect("the schema compiles into a mapping");

    // The Mapping is keryx's account of how the schema lowers to ASP: the stable
    // interface `gen`, `facts`, `emit`, and `diff` all build on. Inspect it, render
    // it, shred a payload against it — whatever your tool needs.
    let _mapping = codec.mapping();
}
// ANCHOR_END: example
