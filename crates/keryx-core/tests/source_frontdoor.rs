//! The `.proto` front door (spec §20, §31 M1): protox compiles source behind the bytes
//! seam and keryx ingests it; a compile failure is a `UncompilableSource` diagnostic, never
//! a panic (§6). Engine-direct fixtures resolved against the crate's fixtures/proto dirs.

use keryx_test_support as support;

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
            .any(|d| d.kind() == DiagnosticKind::UncompilableSource),
        "a compile failure composes UncompilableSource"
    );
}

// The editions front-door gate, verdict-aware (docs/proto-support.md; mirrors
// tests/editions_capability.rs). DEFERRED today → UncompilableSource; flips to Ok when
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
                .any(|d| d.kind() == DiagnosticKind::UncompilableSource),
            "editions: DEFERRED — protox cannot compile it (→ UncompilableSource)"
        ),
    }
}

// keryx's own option registry resolves from the embedded copy — no `-I` for the vendored
// `proto/` dir (architecture §11), the way `google/protobuf/*` does. Only the fixtures dir
// is on the include path, and it holds no `keryx/options.proto`, so the import can resolve only
// through the embedded registry.
#[test]
fn keryx_options_import_resolves_without_an_include() {
    let (fixtures, _) = dirs();
    let schema = compile(&["options.proto"], &[&fixtures])
        .expect("options.proto compiles against the embedded keryx/options.proto");
    assert!(
        schema
            .messages()
            .iter()
            .any(|m| m.path().as_str() == "keryx.opt.Sample"),
        "the schema importing keryx/options.proto ingested"
    );
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
