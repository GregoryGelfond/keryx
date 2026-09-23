//! The `.proto` front door (spec §20, §31 M1): protox compiles source behind the bytes
//! seam and keryx ingests it; a compile failure is a `UncompilableSource` diagnostic, never
//! a panic (§6). Engine-direct fixtures resolved against the crate's fixtures/proto dirs.

use keryx_test_support as support;

use std::path::{Path, PathBuf};

use keryx_core::descriptor::{Schema, compile, ingest};
use keryx_core::diagnostics::DiagnosticKind;
use keryx_core::policy::{self, Mapping, Unit};

fn dirs() -> (PathBuf, PathBuf) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    (manifest.join("tests/fixtures"), manifest.join("proto"))
}

/// Whether the message at `path` is a subject of `schema` (it must be present).
fn message_is_subject(schema: &Schema, path: &str) -> bool {
    schema
        .messages()
        .iter()
        .find(|m| m.path().as_str() == path)
        .expect("message present")
        .is_subject()
}

/// Whether the enum at `path` is a subject of `schema` (it must be present).
fn enum_is_subject(schema: &Schema, path: &str) -> bool {
    schema
        .enums()
        .iter()
        .find(|e| e.path().as_str() == path)
        .expect("enum present")
        .is_subject()
}

/// Whether the sort at `proto` is a subject of `mapping` (it must be present).
fn sort_is_subject(mapping: &Mapping, proto: &str) -> bool {
    mapping
        .units()
        .iter()
        .flat_map(Unit::sorts)
        .find(|s| s.proto().as_str() == proto)
        .expect("sort present")
        .is_subject()
}

/// Whether the enum mapping at `proto` is a subject of `mapping` (it must be present).
fn enum_mapping_is_subject(mapping: &Mapping, proto: &str) -> bool {
    mapping
        .units()
        .iter()
        .flat_map(Unit::enums)
        .find(|e| e.proto().as_str() == proto)
        .expect("enum present")
        .is_subject()
}

#[test]
fn compiles_a_proto3_source_to_a_schema() {
    let (fixtures, vendored) = dirs();
    let schema = compile(&["proto3.proto"], &[&fixtures, &vendored]).expect("proto3 compiles");
    let via_bytes =
        ingest(&support::compile_fixture("proto3.proto")).expect("the equivalent bytes ingest");
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

// The subject-versus-referent-closure mark (`is_subject`) is the one axis on which the two doors
// differ by design: the front door's subjects are the files *opened* (the §21.2 subject carry
// above), the bytes door's every file of the set that is not a dependency file. For a schema
// whose subject imports a *user* file, the imported types are referent closure through the front
// door and subjects through the bytes door — so, `Schema` and `Mapping` deriving `PartialEq`, the
// two doors' models are unequal for such a schema, where `compiles_a_proto3_source_to_a_schema`
// has them equal for one importing dependency files alone. `gmap.proto` imports the user file
// `gdep.proto` (a message); `preserve.proto` imports `preserve_dep.proto` (an enum).
#[test]
fn an_imported_user_file_is_closure_through_the_front_door_and_subject_through_the_bytes_door() {
    let (fixtures, vendored) = dirs();
    let front = compile(&["gmap.proto"], &[&fixtures, &vendored]).expect("gmap compiles");
    let bytes =
        ingest(&support::compile_fixture("gmap.proto")).expect("the equivalent bytes ingest");
    assert!(
        message_is_subject(&front, "keryx.gmap.Holder")
            && message_is_subject(&bytes, "keryx.gmap.Holder"),
        "the opened file's message is a subject through either door"
    );
    assert!(
        !message_is_subject(&front, "keryx.gdep.Thing"),
        "front door: the imported user file was not opened, so its message is closure"
    );
    assert!(
        message_is_subject(&bytes, "keryx.gdep.Thing"),
        "bytes door: the imported user file is not a dependency file, so its message is a subject"
    );
    assert_ne!(front, bytes, "the two doors' schemas differ on the mark");

    // The mark rides onto the mapping, where the evolution instrument reads it.
    let front = policy::map(&front).expect("gmap maps");
    let bytes = policy::map(&bytes).expect("gmap maps");
    assert!(sort_is_subject(&front, "keryx.gmap.Holder"));
    assert!(!sort_is_subject(&front, "keryx.gdep.Thing"));
    assert!(sort_is_subject(&bytes, "keryx.gdep.Thing"));
    assert_ne!(front, bytes, "the two doors' mappings differ on the mark");

    // An imported user file's enum, the same way, at both layers.
    let front = compile(&["preserve.proto"], &[&fixtures, &vendored]).expect("preserve compiles");
    let bytes =
        ingest(&support::compile_fixture("preserve.proto")).expect("the equivalent bytes ingest");
    assert!(!enum_is_subject(&front, "keryx.preserve.dep.Beacon"));
    assert!(enum_is_subject(&bytes, "keryx.preserve.dep.Beacon"));
    let front = policy::map(&front).expect("preserve maps");
    let bytes = policy::map(&bytes).expect("preserve maps");
    assert!(!enum_mapping_is_subject(
        &front,
        "keryx.preserve.dep.Beacon"
    ));
    assert!(enum_mapping_is_subject(&bytes, "keryx.preserve.dep.Beacon"));
}
