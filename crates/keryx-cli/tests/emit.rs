//! `keryx emit` end to end (spec §12.3, §25; architecture §6): an answer set and its schema in —
//! a `.lp` of ground facts whose `emit_<sort>(root)` markers name the messages to rebuild, and a
//! `.proto` source or `.binpb` descriptor set — one reassembled message's wire bytes on stdout
//! (`keryx emit … | protoc --decode`), diagnostics on stderr, and the §6 exit taxonomy with its
//! shape class (6): an answer set that does not read, carries a term that does not lower to its
//! field, or is not one the theory admits is distinct from a file that cannot be read (`Input`, 3),
//! a schema that builds no codec (`Schema`, 4), a `--root` that does not resolve to one message
//! (`Usage`, 2), and a contained engine fault (`Dependency`, 7). One message reaches stdout:
//! `--root Type` narrows several roots to the one it carries. The outbound mirror of the `facts`
//! suite; its fixtures are its own — a thermal-shaped schema compiled here (through `support`), the
//! reassembling answer sets generated from a shred or hand-written over the schema's vocabulary.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use keryx_test_support as support;
use keryx_test_support::wire::{self, batch, reading};

/// The suite's schema — the thermal story's `Reading`/`ReadingBatch` (spec §28) and a `Tally`
/// carrying one scalar, so an answer set can name two roots of different types.
const SCHEMA: &str = "\
syntax = \"proto3\";
package keryx.facts;

message Reading      { string sensor = 1; int32 temp_c = 2; }
message ReadingBatch { repeated Reading readings = 1; }
message Tally        { uint32 count = 1; }
";

/// A `ReadingBatch` root `r0` with one reading, hand-written over the schema's vocabulary: the
/// `emit_reading_batch` marker exporting the root, the root and child occupancy atoms, and the
/// child's fields. Reassembles to `batch(&[reading("s-1", 1)])`.
const ONE_READING_BATCH: &str = "\
emit_reading_batch(r0).
reading_batch(r0).
reading(readings(r0, 0)).
sensor(readings(r0, 0), \"s-1\").
temp_c(readings(r0, 0), 1).
";

/// Two roots of different types — a `ReadingBatch` and a `Tally` — so `--root Type` has something
/// to narrow, and its absence is the selection ambiguity.
const BATCH_AND_TALLY: &str = "\
emit_reading_batch(r0).
reading_batch(r0).
reading(readings(r0, 0)).
sensor(readings(r0, 0), \"s-1\").
temp_c(readings(r0, 0), 1).
emit_tally(rt).
tally(rt).
count(rt, 5).
";

/// Two roots of the *same* type (`Reading`) — the case `--root Type` cannot separate, so it is a
/// usage error even with the filter given.
const TWO_READINGS: &str = "\
emit_reading(ra).
reading(ra).
sensor(ra, \"s-1\").
temp_c(ra, 1).
emit_reading(rb).
reading(rb).
sensor(rb, \"s-2\").
temp_c(rb, 2).
";

/// A `Reading` whose `temp_c` (an `int32`) carries a string — a term that does not lower to its
/// field's type, the outbound shape error.
const BAD_TEMP: &str = "\
emit_reading(r0).
reading(r0).
sensor(r0, \"s-1\").
temp_c(r0, \"hot\").
";

/// A `Tally { count = N }`, written as bytes on the wire like `facts`'s own fixtures — never
/// through the engine's encoder.
fn tally(count: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    wire::uint32(1, count, &mut buf);
    buf
}

/// The suite's fixtures under one fresh scratch directory (per test, so parallel tests never
/// collide): the schema as the descriptor set the emit door builds a codec from.
struct Fixture {
    dir: PathBuf,
    set: PathBuf,
}

fn fixture(name: &str) -> Fixture {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("emit.proto"), SCHEMA).unwrap();
    let set = dir.join("emit.binpb");
    std::fs::write(
        &set,
        support::compile_in(std::slice::from_ref(&dir), "emit.proto"),
    )
    .unwrap();
    Fixture { dir, set }
}

impl Fixture {
    /// Write `contents` as the file `name` in the fixture directory, returning its path — the
    /// answer set (`.lp`) or a re-shred round-trip payload alike.
    fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
}

/// Run `keryx [args] emit [--out OUT] [--root TYPE] <answer_set> <spec>`, the runner's
/// `RUST_BACKTRACE` cleared so the subprocess shows the default panic posture.
fn emit(
    answer_set: &Path,
    spec: &Path,
    out: Option<&str>,
    root: Option<&str>,
    args: &[&str],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_keryx"));
    command.args(args).arg("emit");
    if let Some(out) = out {
        command.arg("--out").arg(out);
    }
    if let Some(root) = root {
        command.arg("--root").arg(root);
    }
    command.arg(answer_set).arg(spec);
    command.env_remove("RUST_BACKTRACE").output().unwrap()
}

/// Shred `payload` (of type `root_type`) back to its `.lp` facts through `keryx facts` (asserting
/// exit 0): the reassembling answer set of the round-trip tests is generated from the shred, so the
/// round trip is exact rather than hand-transcribed.
fn facts_lp(spec: &Path, root_type: &str, payload: &Path) -> String {
    let mut root = OsString::from(root_type);
    root.push("=");
    root.push(payload);
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("facts")
        .arg("--root")
        .arg(root)
        .arg(spec)
        .env_remove("RUST_BACKTRACE")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "facts: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn emits_a_batch_to_the_canonical_binary() {
    // The round trip at the command: shred a batch to facts, mark its root, and emit — the bytes on
    // stdout are the canonical payload again, stderr quiet, exit 0.
    let fx = fixture("emit_binary");
    let payload = batch(&[reading("s-101", 44), reading("s-107", 21)]);
    let payload_path = fx.write("batch.binpb", &payload);
    let mut answer = facts_lp(&fx.set, "ReadingBatch", &payload_path);
    answer.push_str("emit_reading_batch(r0).\n");
    let path = fx.write("batch.lp", &answer);
    let out = emit(&path, &fx.set, Some("binpb"), None, &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(out.stdout, payload);
    assert!(out.stderr.is_empty(), "stderr is quiet on success");
}

#[test]
fn defaults_to_the_binary_form() {
    // No `--out`: the binary wire format, the canonical protobuf form. Also proves the hand-written
    // answer set the multi-root fixtures build on reassembles as intended.
    let fx = fixture("emit_default");
    let path = fx.write("one.lp", ONE_READING_BATCH);
    let out = emit(&path, &fx.set, None, None, &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(out.stdout, batch(&[reading("s-1", 1)]));
}

#[test]
fn each_out_form_reshreds_to_the_same_facts() {
    // `--out binpb|txtpb|json` each writes a faithful document: emitted and shredded back, it is
    // the same facts — three-way parity at the command (§26). The deep per-format instruments are
    // the library suite's; this proves the flag dispatches and each form round-trips.
    let fx = fixture("emit_all_forms");
    let path = fx.write("one.lp", ONE_READING_BATCH);
    // The reference facts: emit the binary form and shred it back.
    let binary = emit(&path, &fx.set, Some("binpb"), None, &[]);
    assert_eq!(binary.status.code(), Some(0), "stderr: {}", stderr(&binary));
    let reference = facts_lp(
        &fx.set,
        "ReadingBatch",
        &fx.write("round.binpb", &binary.stdout),
    );
    for (form, ext) in [("binpb", "binpb"), ("txtpb", "txtpb"), ("json", "json")] {
        let emitted = emit(&path, &fx.set, Some(form), None, &[]);
        assert_eq!(
            emitted.status.code(),
            Some(0),
            "{form} stderr: {}",
            stderr(&emitted)
        );
        let round = fx.write(&format!("round.{ext}"), &emitted.stdout);
        assert_eq!(
            facts_lp(&fx.set, "ReadingBatch", &round),
            reference,
            "the {form} form shreds back to the same facts"
        );
    }
}

#[test]
fn two_roots_without_a_filter_is_a_usage_error() {
    // The answer set names two roots of different types; with no `--root`, stdout cannot carry one
    // message (concatenating two binaries would merge them), so it is a usage error naming the
    // types found — never a merged product.
    let fx = fixture("emit_two_roots");
    let path = fx.write("both.lp", BATCH_AND_TALLY);
    let out = emit(&path, &fx.set, Some("binpb"), None, &[]);
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(
        out.stdout.is_empty(),
        "no product when the selection is not one message"
    );
    let err = stderr(&out);
    assert!(
        err.contains("keryx.facts.ReadingBatch") && err.contains("keryx.facts.Tally"),
        "the note names the roots found: {err}"
    );
}

#[test]
fn a_root_filter_selects_one_of_several() {
    // `--root Type` narrows several roots to one: the batch and the tally each emit alone, their
    // own canonical bytes.
    let fx = fixture("emit_filter");
    let path = fx.write("both.lp", BATCH_AND_TALLY);
    let batch_out = emit(&path, &fx.set, Some("binpb"), Some("ReadingBatch"), &[]);
    assert_eq!(
        batch_out.status.code(),
        Some(0),
        "stderr: {}",
        stderr(&batch_out)
    );
    assert_eq!(batch_out.stdout, batch(&[reading("s-1", 1)]));
    let tally_out = emit(&path, &fx.set, Some("binpb"), Some("Tally"), &[]);
    assert_eq!(
        tally_out.status.code(),
        Some(0),
        "stderr: {}",
        stderr(&tally_out)
    );
    assert_eq!(tally_out.stdout, tally(5));
}

#[test]
fn a_filter_matching_two_roots_of_one_type_is_a_usage_error() {
    // `--root Reading` matches both readings — a single type naming two roots, which `--root` alone
    // cannot separate: a usage error naming the count, never two concatenated messages.
    let fx = fixture("emit_ambiguous");
    let path = fx.write("two.lp", TWO_READINGS);
    let out = emit(&path, &fx.set, Some("binpb"), Some("Reading"), &[]);
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    assert!(
        out.stdout.is_empty(),
        "no product on an ambiguous selection"
    );
    assert!(
        stderr(&out).contains("2 roots"),
        "the note names how many roots the type has: {}",
        stderr(&out)
    );
}

#[test]
fn a_term_type_mismatch_is_a_shape_error() {
    // A term that does not lower to its field's type — a string where `temp_c` is `int32` — is the
    // outbound door's shape error (6), not the inbound translation class; the JSON diagnostic names
    // the kind, and no partial product is written.
    let fx = fixture("emit_mismatch");
    let path = fx.write("bad.lp", BAD_TEMP);
    let out = emit(&path, &fx.set, Some("binpb"), None, &["--format", "json"]);
    assert_eq!(out.status.code(), Some(6), "stderr: {}", stderr(&out));
    assert!(out.stdout.is_empty(), "no partial product on error");
    assert!(
        stderr(&out).contains("term_type_mismatch"),
        "the diagnostic names the kind: {}",
        stderr(&out)
    );
}

#[test]
fn a_missing_answer_set_is_an_input_error() {
    // A `.lp` that cannot be read is a file-I/O error (3), distinct from one that reads but does
    // not raise.
    let fx = fixture("emit_missing");
    let out = emit(&fx.dir.join("absent.lp"), &fx.set, Some("binpb"), None, &[]);
    assert_eq!(out.status.code(), Some(3), "stderr: {}", stderr(&out));
}

#[test]
fn a_malformed_answer_set_is_a_shape_error() {
    // An unterminated atom does not parse: the answer set is unreadable (6, the outbound door's
    // class), never a panic; the JSON diagnostic names the kind.
    let fx = fixture("emit_malformed");
    let path = fx.write("bad.lp", "emit_reading(r0");
    let out = emit(&path, &fx.set, Some("binpb"), None, &["--format", "json"]);
    assert_eq!(out.status.code(), Some(6), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("unreadable_answer_set"),
        "the diagnostic names the kind: {}",
        stderr(&out)
    );
}

#[test]
fn a_non_utf8_answer_set_is_a_shape_error() {
    // The `.lp` read fine (not `Input`), but its bytes are not text — the paradigm unreadable
    // answer set (6), as a non-UTF-8 payload is the inbound door's translation error, not `Input`.
    let fx = fixture("emit_not_utf8");
    let path = fx.write("latin1.lp", b"emit_reading(r\xe9).".as_slice());
    let out = emit(&path, &fx.set, Some("binpb"), None, &["--format", "json"]);
    assert_eq!(out.status.code(), Some(6), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("unreadable_answer_set"),
        "the diagnostic names the kind: {}",
        stderr(&out)
    );
}
