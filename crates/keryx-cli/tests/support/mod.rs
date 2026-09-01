//! Test support: compile a keryx-core fixture `.proto` to a serialized `FileDescriptorSet`
//! (bytes) with protox, the pure-Rust compiler — the only path to a valid descriptor set for
//! the `schema-facts` and `.binpb`-`gen` tests. Bytes are the only thing that crosses back;
//! keryx decodes them through its own prost-reflect, so no engine type reaches the CLI product
//! (the descriptor-engine boundary — the shipped `keryx` binary composes only keryx-core).
//! keryx-core's fixtures live one crate over; a shared `tests/support/` module, not a binary.

// Shared across integration-test binaries via `mod support;`; each uses a subset, so the
// unused-in-this-binary helpers are dead there — the standard shared-helper pattern.
#![allow(dead_code)]

use std::path::Path;

use protox::Compiler;

/// The keryx-core fixtures directory (`crates/keryx-core/tests/fixtures`), one crate over.
pub fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../keryx-core/tests/fixtures")
}

/// The keryx-core vendored proto directory (`crates/keryx-core/proto`), for `keryx/options.proto`.
pub fn vendored() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../keryx-core/proto")
}

/// Compile a keryx-core fixture `<name>` — imports resolved against the fixtures dir, the
/// vendored `proto/` dir, and protox's bundled well-known types — to a serialized
/// `FileDescriptorSet`. Built through `encode_file_descriptor_set` (not `protox::compile`,
/// which re-encodes options through prost-types' typed structs and drops custom-option bytes,
/// the §20 trap); `include_imports`/`include_source_info` on, so the set is self-contained.
/// Panics on failure: a broken fixture is a test bug, surfaced loudly.
pub fn compile_fixture(name: &str) -> Vec<u8> {
    let mut compiler = Compiler::new([fixtures(), vendored()]).expect("compiler initializes");
    compiler.include_source_info(true).include_imports(true);
    compiler.open_file(name).expect("fixture compiles");
    compiler.encode_file_descriptor_set()
}
