//! `keryx facts` end to end (spec §11, §25; architecture §6): a payload and its schema in — the
//! `--root Type=payload.binpb` pair and a `.proto` source or `.binpb` descriptor set — the ground
//! facts as a `.lp` fact module on stdout (`keryx facts … | clingo`), diagnostics on stderr, and
//! the §6 exit taxonomy with its translation class (8): a payload that does not translate is
//! distinct from a file that cannot be read (`Input`, 3), a schema that builds no codec (`Schema`,
//! 4), a root the schema lacks or a malformed `--root` (`Usage`, 2), and a contained engine fault
//! (`Dependency`, 7). The fixtures are the suite's own: a thermal-shaped source written and compiled
//! here (through `support`), its payloads written as bytes on the wire — no engine encoder, and no
//! protox type, in reach.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use keryx_test_support as support;
use keryx_test_support::wire::{self, batch, reading};

/// The suite's schema: the thermal story's `Reading`/`ReadingBatch` (spec §28), whose batch shreds
/// to the seven §28 facts, and a `Tally` carrying the one scalar the story lacks — a `uint32`,
/// whose value above `i32::MAX` is the §6 refusal the translation class carries.
const SCHEMA: &str = "\
syntax = \"proto3\";
package keryx.facts;

message Reading      { string sensor = 1; int32 temp_c = 2; }
message ReadingBatch { repeated Reading readings = 1; }
message Tally        { uint32 count = 1; }
";

/// The §28 facts of [`section_28_batch`], in the canonical statement order the codec renders.
const SECTION_28_FACTS: &str = "reading(readings(r0, 0)).\n\
    reading(readings(r0, 1)).\n\
    reading_batch(r0).\n\
    sensor(readings(r0, 0), \"s-101\").\n\
    sensor(readings(r0, 1), \"s-107\").\n\
    temp_c(readings(r0, 0), 44).\n\
    temp_c(readings(r0, 1), 21).\n";

/// The suite's fixtures under one fresh scratch directory (per test, so parallel tests never
/// collide): the schema as `.proto` source and as the descriptor set compiled from it, so both
/// spec doors are driven, and the directory the payloads are written into.
struct Fixture {
    dir: PathBuf,
    proto: PathBuf,
    set: PathBuf,
}

fn fixture(name: &str) -> Fixture {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let proto = dir.join("facts.proto");
    std::fs::write(&proto, SCHEMA).unwrap();
    let set = dir.join("facts.binpb");
    std::fs::write(
        &set,
        support::compile_in(std::slice::from_ref(&dir), "facts.proto"),
    )
    .unwrap();
    Fixture { dir, proto, set }
}

impl Fixture {
    /// Write `bytes` as the payload file `name` in the fixture directory.
    fn payload(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

/// A `Tally { count = 1 }` — this suite's own shape, written as bytes on the wire like the thermal
/// `reading`/`batch` that `support::wire` builds, never through the engine's encoder.
fn tally(count: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    wire::uint32(1, count, &mut buf);
    buf
}

/// The spec's own payload (§28): two readings.
fn section_28_batch() -> Vec<u8> {
    batch(&[reading("s-101", 44), reading("s-107", 21)])
}

/// The `--root` argument `Type=payload`, the path joined as the OS gives it.
fn root(root_type: &str, payload: &Path) -> OsString {
    let mut root = OsString::from(root_type);
    root.push("=");
    root.push(payload);
    root
}

/// Run `keryx [args] facts --root <root> <spec> [-I include]…`, the runner's `RUST_BACKTRACE`
/// cleared so the subprocess shows the default panic posture.
fn facts(root: impl AsRef<OsStr>, spec: &Path, includes: &[&Path], args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_keryx"));
    command
        .args(args)
        .arg("facts")
        .arg("--root")
        .arg(root)
        .arg(spec);
    for include in includes {
        command.arg("-I").arg(include);
    }
    command.env_remove("RUST_BACKTRACE").output().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn shreds_a_batch_to_the_section_28_facts() {
    // The spec's own payload (§28) through the descriptor-set door: the seven facts on stdout in
    // canonical order — the product, ready for `| clingo` — stderr quiet, exit 0.
    let fx = fixture("facts_batch");
    let payload = fx.payload("batch.binpb", &section_28_batch());
    let out = facts(root("ReadingBatch", &payload), &fx.set, &[], &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), SECTION_28_FACTS);
    assert!(out.stderr.is_empty(), "stderr is quiet on success");
}

#[test]
fn accepts_a_proto_source_spec() {
    // §25's `<spec>` is `.proto` source or a `.binpb` set alike: the source door, with `-I`,
    // builds the same codec and shreds to the same facts — here from the fully-qualified root,
    // which resolves as the short name does.
    let fx = fixture("facts_source");
    let payload = fx.payload("batch.binpb", &section_28_batch());
    let out = facts(
        root("keryx.facts.ReadingBatch", &payload),
        &fx.proto,
        &[&fx.dir],
        &[],
    );
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), SECTION_28_FACTS);
}

#[test]
fn an_out_of_range_value_is_a_translation_error_with_a_structured_diagnostic() {
    // §6: a `uint32` above `i32::MAX` is refused, never truncated — a translation error (8),
    // neither a schema nor an input one — and under `--format json` the diagnostic is the
    // Appendix B structure naming the field's path, so a consumer reads *why* as data (§26).
    let fx = fixture("facts_out_of_range");
    let over = u32::try_from(i32::MAX).unwrap() + 1;
    let payload = fx.payload("over.binpb", &tally(over));
    let out = facts(root("Tally", &payload), &fx.set, &[], &["--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(8),
        "exit Translation: {}",
        stderr(&out)
    );
    assert!(out.stdout.is_empty(), "no partial product on error");
    let diagnostics = stderr(&out);
    let diagnostics = diagnostics.trim();
    assert!(
        diagnostics.starts_with(r#"[{"field_path":"keryx.facts.Tally.count""#),
        "a structured JSON array naming the field's path: {diagnostics}"
    );
    assert!(
        diagnostics.contains(r#""kind":"value_out_of_range""#),
        "the value_out_of_range kind is structured: {diagnostics}"
    );
    assert!(
        diagnostics.ends_with("}]"),
        "a closed JSON array: {diagnostics}"
    );

    // The human form is the library `Display`: `keryx: <kind> at <path>: <detail>`.
    let out = facts(
        root("Tally", &payload),
        &fx.set,
        &[],
        &["--format", "human"],
    );
    assert_eq!(out.status.code(), Some(8));
    assert!(
        stderr(&out).contains("keryx: value_out_of_range at keryx.facts.Tally.count: "),
        "human prose, not JSON: {}",
        stderr(&out)
    );
}

#[test]
fn an_undecodable_payload_is_a_translation_error() {
    // Bytes that do not decode as the root type are a translation error too (8): the file read
    // fine (not `Input`) and the schema is sound (not `Schema`) — the payload is what failed.
    let fx = fixture("facts_undecodable");
    // A field-1 length prefix claiming far more bytes than follow.
    let payload = fx.payload("garbage.binpb", b"\x0a\xff\xff\xff\x0f");
    let out = facts(root("ReadingBatch", &payload), &fx.set, &[], &[]);
    assert_eq!(
        out.status.code(),
        Some(8),
        "exit Translation: {}",
        stderr(&out)
    );
    assert!(out.stdout.is_empty(), "no partial product on error");
    assert!(
        stderr(&out).contains("undecodable_payload"),
        "the diagnostic names the decode failure: {}",
        stderr(&out)
    );
}

#[test]
fn a_root_type_the_schema_lacks_is_a_usage_error() {
    // `--root Absent=…`: a type no message of the schema bears is the caller's argument at fault,
    // a usage error (2) — named as given, the payload never decoded.
    let fx = fixture("facts_absent");
    let payload = fx.payload("batch.binpb", &section_28_batch());
    let out = facts(root("Absent", &payload), &fx.set, &[], &[]);
    assert_eq!(out.status.code(), Some(2), "exit Usage: {}", stderr(&out));
    assert!(out.stdout.is_empty(), "no partial product on error");
    let diagnostics = stderr(&out);
    assert!(
        diagnostics.contains("unknown_root_type") && diagnostics.contains("Absent"),
        "the diagnostic names the type as given: {diagnostics}"
    );
}

#[test]
fn a_missing_payload_is_an_input_error() {
    // A payload file that cannot be read stays `Input` (3) — strictly file I/O, never a
    // translation error — distinct from one that reads but does not decode (`Translation`).
    let fx = fixture("facts_missing_payload");
    let out = facts(
        root("ReadingBatch", &fx.dir.join("missing.binpb")),
        &fx.set,
        &[],
        &[],
    );
    assert_eq!(out.status.code(), Some(3), "exit Input: {}", stderr(&out));
    assert!(out.stdout.is_empty(), "no partial product on error");
    assert!(
        stderr(&out).contains("cannot read"),
        "the read failure is named: {}",
        stderr(&out)
    );
}

#[test]
fn a_malformed_root_is_a_usage_error() {
    // `--root` takes `Type=payload`: no `=`, or an empty half, is a usage error (2) stating the
    // shape, before any file is touched.
    let fx = fixture("facts_malformed_root");
    for malformed in ["ReadingBatch", "=batch.binpb", "ReadingBatch="] {
        let out = facts(malformed, &fx.set, &[], &[]);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`--root {malformed}` exits Usage: {}",
            stderr(&out)
        );
        assert!(
            stderr(&out).contains("Type=payload"),
            "the shape is stated: {}",
            stderr(&out)
        );
    }
}

#[test]
fn a_payload_path_may_itself_contain_an_equals() {
    // `--root` splits at its first `=` — a type name never contains one — so a payload under a
    // `key=value` partition directory is read as given, not refused as a second `=`.
    let fx = fixture("facts_partitioned_payload");
    let partition = fx.dir.join("date=2026-09-04");
    std::fs::create_dir_all(&partition).unwrap();
    let payload = partition.join("batch.binpb");
    std::fs::write(&payload, section_28_batch()).unwrap();
    let out = facts(root("ReadingBatch", &payload), &fx.set, &[], &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), SECTION_28_FACTS);
}

#[test]
fn an_unsupported_payload_format_is_a_usage_error() {
    // The payload's format is its extension; one the codec does not read — a `.json` until that
    // format lands, or no extension at all — is a usage error (2), decided by the extension before
    // the file is read (the files exist).
    let fx = fixture("facts_unsupported_format");
    for name in ["batch.json", "batch"] {
        let payload = fx.payload(name, &section_28_batch());
        let out = facts(root("ReadingBatch", &payload), &fx.set, &[], &[]);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`{name}` exits Usage: {}",
            stderr(&out)
        );
        assert!(
            stderr(&out).contains("binpb"),
            "the format keryx reads is named: {}",
            stderr(&out)
        );
    }
}

#[test]
fn an_uncompilable_source_is_a_schema_error_with_the_descriptor_set_hint() {
    // A `.proto` spec the front door cannot compile — a syntax error: a message left unclosed —
    // is a schema error (4), and the fix-it hint `gen`/`explain` carry travels with it (§6):
    // supply a descriptor set instead.
    let fx = fixture("facts_uncompilable");
    let payload = fx.payload("batch.binpb", &section_28_batch());
    // Deliberately malformed (the message never closes): a parse failure, not a missing source.
    let broken = fx.dir.join("broken.proto");
    std::fs::write(
        &broken,
        "syntax = \"proto3\";\npackage keryx.facts;\nmessage Broken { string sensor = 1;\n",
    )
    .unwrap();
    let out = facts(root("ReadingBatch", &payload), &broken, &[&fx.dir], &[]);
    assert_eq!(out.status.code(), Some(4), "exit Schema: {}", stderr(&out));
    assert!(out.stdout.is_empty(), "no partial product on error");
    let diagnostics = stderr(&out);
    assert!(
        diagnostics.contains("uncompilable_source"),
        "the diagnostic names the front-door compile failure: {diagnostics}"
    );
    // A source never found composes `uncompilable_source` too; the pinned compiler's detail for
    // the parse failure names the unexpected end of file, so this pins the cause.
    assert!(
        diagnostics.contains("reached end of file"),
        "the failure is the syntax error, not a missing source: {diagnostics}"
    );
    assert!(
        diagnostics.contains(".binpb"),
        "a descriptor-set fix-it hint is present: {diagnostics}"
    );
}

#[test]
fn a_descriptor_set_spec_that_cannot_be_read_or_ingested_is_classed_as_for_gen() {
    // A `.binpb` spec that cannot be read is `Input` (3); one that reads but does not ingest is
    // `Schema` (4) — the two are not conflated (§6), exactly as `gen` classes them.
    let fx = fixture("facts_bad_set");
    let payload = fx.payload("batch.binpb", &section_28_batch());
    let out = facts(
        root("ReadingBatch", &payload),
        &fx.dir.join("missing.binpb"),
        &[],
        &[],
    );
    assert_eq!(out.status.code(), Some(3), "exit Input: {}", stderr(&out));

    let garbage = fx.payload("garbage.binpb", b"\x0a\xff\xff\xff\x0f");
    let out = facts(root("ReadingBatch", &payload), &garbage, &[], &[]);
    assert_eq!(out.status.code(), Some(4), "exit Schema: {}", stderr(&out));
    assert!(out.stdout.is_empty(), "no partial product on error");
    assert!(
        stderr(&out).contains("unreadable_descriptor_set"),
        "the diagnostic names the ingest failure: {}",
        stderr(&out)
    );
}

#[test]
fn a_contained_engine_fault_while_building_the_codec_is_a_dependency_error() {
    // The dependency boundary at the codec's construction: a descriptor set that drives a real
    // contained engine fault classifies `Dependency` (7) — neither `Schema` nor a keryx bug — with
    // one report and no false bug notice, as `schema-facts` holds for its door.
    let fx = fixture("facts_fault");
    let payload = fx.payload("batch.binpb", &section_28_batch());
    let set = fx.payload("fault.binpb", &support::decode_fault_set());
    let out = facts(root("M", &payload), &set, &[], &[]);
    assert_eq!(
        out.status.code(),
        Some(7),
        "exit Dependency: {}",
        stderr(&out)
    );
    assert!(out.stdout.is_empty(), "no partial product on error");
    let diagnostics = stderr(&out);
    assert!(
        diagnostics.contains("dependency_fault"),
        "the diagnostic is rendered: {diagnostics}"
    );
    assert!(
        !diagnostics.contains("bug in keryx"),
        "no false keryx-bug notice: {diagnostics}"
    );
}
