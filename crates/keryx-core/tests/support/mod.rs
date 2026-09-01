//! Test support: compile a fixture `.proto` to a serialized `FileDescriptorSet`
//! (bytes) with protox, the pure-Rust compiler. Bytes are the *only* thing that
//! crosses back — keryx decodes them through its own prost-reflect, so the two
//! crates' prost versions never couple (the descriptor-engine boundary). A
//! subdirectory module (`tests/support/`) is shared, not a test binary itself.

// Shared across integration-test binaries via `mod support;`; each binary uses a
// subset of these helpers, so the unused-in-this-binary ones are dead there — the
// standard shared-test-helper pattern, allowed here, not a defect.
#![allow(dead_code)]

use std::path::Path;

use protox::Compiler;

/// Compile `tests/fixtures/<name>` — with its imports resolved against the
/// fixtures dir, the crate's vendored `proto/` dir (for `keryx/options.proto`,
/// vendored with the fixture corpus at a later step), and protox's bundled
/// well-known types (`google/protobuf/*`) — to a serialized `FileDescriptorSet`.
/// Built through `Compiler::encode_file_descriptor_set`, not the `protox::compile`
/// convenience function: that function returns a *typed* `FileDescriptorSet`, and
/// going through it re-encodes every options submessage through prost-types'
/// typed `*Options` structs, which — the same trap §20 names for keryx's own
/// ingestion — silently drops the custom-option extension bytes protox already
/// resolved. `encode_file_descriptor_set` encodes straight from the pool and keeps
/// them. `include_imports`/`include_source_info` are on, so the set is
/// self-contained and carries doc comments.
/// Panics on failure: a broken fixture is a test bug, surfaced loudly (this is
/// test support, not library code).
pub fn compile_fixture(name: &str) -> Vec<u8> {
    try_compile_fixture(name).unwrap_or_else(|error| panic!("fixture `{name}` compiles: {error}"))
}

/// As [`compile_fixture`], but returns the compiler's result so a probe can
/// record whether a version (e.g. an edition) is supported rather than panic.
pub fn try_compile_fixture(name: &str) -> Result<Vec<u8>, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = manifest.join("tests/fixtures");
    let vendored = manifest.join("proto");
    let mut compiler = Compiler::new([&fixtures, &vendored]).map_err(|error| error.to_string())?;
    compiler.include_source_info(true).include_imports(true);
    compiler
        .open_file(name)
        .map_err(|error| error.to_string())?;
    Ok(compiler.encode_file_descriptor_set())
}
