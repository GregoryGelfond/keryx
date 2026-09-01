//! The `.proto` front door (spec §20, §31 M1): protox compiles source behind the bytes
//! seam and keryx ingests it; a compile failure is a `SourceCompile` diagnostic, never
//! a panic (§6). Engine-direct fixtures resolved against the crate's fixtures/proto dirs.

mod support;

use std::path::{Path, PathBuf};

use keryx_core::descriptor::compile;
use keryx_core::diagnostics::DiagnosticKind;

fn dirs() -> (PathBuf, PathBuf) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    (manifest.join("tests/fixtures"), manifest.join("proto"))
}

#[test]
fn compiles_a_proto3_source_to_a_schema() {
    let (fixtures, vendored) = dirs();
    let schema = compile(&["proto3.proto"], &[&fixtures, &vendored]).expect("proto3 compiles");
    let via_bytes = keryx_core::descriptor::ingest(&support::compile_fixture("proto3.proto"))
        .expect("the equivalent bytes ingest");
    assert_eq!(
        schema, via_bytes,
        "the front door ingests the same Schema `ingest` does"
    );
}

#[test]
fn a_broken_source_is_a_diagnostic_not_a_panic() {
    let (fixtures, vendored) = dirs();
    let diagnostics = compile(&["broken.proto"], &[&fixtures, &vendored])
        .expect_err("a malformed .proto must not compile");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.kind() == DiagnosticKind::SourceCompile),
        "a compile failure composes SourceCompile"
    );
}

// The editions front-door gate, verdict-aware (docs/proto-support.md; mirrors
// tests/editions_capability.rs). DEFERRED today → SourceCompile; flips to Ok when
// protox gains editions, at which point the editions fixture/golden are added.
#[test]
fn editions_source_is_gated_by_the_compiler_verdict() {
    let (fixtures, vendored) = dirs();
    match compile(&["editions_probe.proto"], &[&fixtures, &vendored]) {
        Ok(schema) => assert!(
            !schema.messages().is_empty(),
            "editions: SUPPORTED via front door"
        ),
        Err(diagnostics) => assert!(
            diagnostics
                .iter()
                .any(|d| d.kind() == DiagnosticKind::SourceCompile),
            "editions: DEFERRED — the front door says supply a descriptor set"
        ),
    }
}

// A subject whose name matches `is_dependency_file`'s heuristic (a well-known type) is
// still ingested when it is the file opened — the subject-carry fix (the §21.2 self-
// application depends on it). protox bundles google/protobuf/descriptor.proto.
#[test]
fn a_well_known_named_subject_is_not_skipped() {
    let (fixtures, _) = dirs();
    let schema = compile(&["google/protobuf/descriptor.proto"], &[&fixtures])
        .expect("descriptor.proto compiles");
    assert!(
        !schema.messages().is_empty(),
        "the opened root file is a subject even though its name is a well-known one"
    );
}
