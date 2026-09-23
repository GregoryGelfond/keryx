//! `keryx diff` end to end (spec §13.4, §27; architecture §6): the old and the new schema in — each
//! `.proto` source or a `.binpb` descriptor set, through the shipped descriptor doors, one door for
//! both sides — the migration report on stdout by default and the JSON changeset under `--json`,
//! per-side progress and diagnostics on stderr, and the exit contract: `0` on any successful
//! comparison, `Diverged` (9) only under `--exit-code` when a change is breaking, `Usage` (2) for
//! mixed doors or two schemas with no package in common, `Schema` (4) for a side that does not
//! build — the last progress line naming which side. The thermal pair is keryx-core's evolution
//! fixture (`evolution_v1.proto` -> `evolution_v2.proto`); the rest are written here.

use keryx_test_support as support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use support::fixtures;

/// The changeset keryx-core pins for the thermal pair — the CLI's `--json` product is that text
/// plus one line terminator, which the golden file (newline-terminated like every text file
/// beside it) already is.
const THERMAL_CHANGESET: &str = include_str!("../../keryx-core/tests/golden/evolution.json");

/// A fresh scratch directory per test (parallel-safe), for the sources a test writes itself.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write `source` as `name` under `dir` and return its path.
fn write(dir: &Path, name: &str, source: impl AsRef<[u8]>) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, source).unwrap();
    path
}

/// The thermal pair: keryx-core's evolution fixture, old then new.
fn thermal_pair() -> (PathBuf, PathBuf) {
    (
        fixtures().join("evolution_v1.proto"),
        fixtures().join("evolution_v2.proto"),
    )
}

/// Run `keryx diff <old> <new> [-I include]… [args]…`, the runner's `RUST_BACKTRACE` cleared so
/// the subprocess shows the default panic posture.
fn diff(old: &Path, new: &Path, includes: &[&Path], args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_keryx"));
    command.arg("diff").arg(old).arg(new);
    for include in includes {
        command.arg("-I").arg(include);
    }
    command
        .args(args)
        .env_remove("RUST_BACKTRACE")
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The per-side progress lines on stderr (`keryx: reading the old schema …`), in order.
fn progress_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("keryx: reading the "))
        .collect()
}

#[test]
fn the_json_changeset_is_the_product_under_json() {
    // The thermal pair under `--json`: the changeset keryx-core pins, byte for byte plus the one
    // line terminator a stdout product ends with — the rename record in it with both paths and
    // its bridge. Exit 0: a breaking change is not an error, and `--exit-code` was not asked.
    // stderr carries the two progress lines and nothing else.
    let (old, new) = thermal_pair();
    let out = diff(&old, &new, &[&fixtures()], &["--json"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let product = stdout(&out);
    assert!(
        product.contains(r#""kind":"renamed""#)
            && product.contains(r#""old_path":"thermal.v1.Reading.temp_c""#)
            && product.contains(r#""new_path":"thermal.v2.Reading.celsius""#)
            && product.contains(r#""bridge":"%! temp_c/2 reads celsius/2"#),
        "the rename record with its paths and bridge: {product}"
    );
    assert_eq!(product, THERMAL_CHANGESET, "the changeset plus one newline");
    let stderr = stderr(&out);
    let progress = progress_lines(&stderr);
    assert_eq!(progress.len(), 2, "one progress line per side: {stderr}");
    assert!(
        progress[0].contains("old schema") && progress[1].contains("new schema"),
        "old then new: {stderr}"
    );
    assert_eq!(
        stderr.lines().count(),
        2,
        "nothing but progress on stderr: {stderr}"
    );
}

#[test]
fn the_human_report_is_the_product_by_default() {
    // Without `--json` the product is the migration report: prose naming the two versions and
    // the renamed field's two names, not the JSON array; newline-terminated. Exit 0.
    let (old, new) = thermal_pair();
    let out = diff(&old, &new, &[&fixtures()], &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = stdout(&out);
    assert!(
        !report.trim_start().starts_with('['),
        "a report, not the JSON changeset: {report}"
    );
    assert!(
        report.contains("thermal.v1") && report.contains("thermal.v2"),
        "the two versions are named: {report}"
    );
    assert!(
        report.contains("temp_c") && report.contains("celsius"),
        "the rename's two names: {report}"
    );
    assert!(report.ends_with('\n'), "newline-terminated: {report:?}");
}

#[test]
fn the_last_progress_line_names_the_side_that_failed() {
    // A side that does not compile is a schema error (4) — and stderr says which side: the
    // progress line for a side precedes its read, so the last one names the failing side. An old
    // side failing leaves no `new` progress line at all; a new side failing follows the old's.
    let dir = scratch("diff_failing_side");
    let broken = write(
        &dir,
        "broken.proto",
        "syntax = \"proto3\";\npackage thermal.v1;\nmessage Broken { string sensor = 1;\n",
    );
    let (old, new) = thermal_pair();

    let out = diff(&broken, &new, &[&dir, &fixtures()], &[]);
    assert_eq!(out.status.code(), Some(4), "exit Schema: {}", stderr(&out));
    assert!(out.stdout.is_empty(), "no product on error");
    let stderr_old = stderr(&out);
    let progress = progress_lines(&stderr_old);
    let last = progress.last().expect("a progress line precedes the read");
    assert!(
        last.contains("old schema") && last.contains("broken.proto"),
        "the last progress line names the old side: {stderr_old}"
    );
    assert!(
        !stderr_old.contains("new schema"),
        "the new side was never read: {stderr_old}"
    );
    assert!(
        stderr_old.contains("uncompilable_source"),
        "the compile failure is the diagnostic: {stderr_old}"
    );

    let out = diff(&old, &broken, &[&dir, &fixtures()], &[]);
    assert_eq!(out.status.code(), Some(4), "exit Schema: {}", stderr(&out));
    let stderr_new = stderr(&out);
    let progress = progress_lines(&stderr_new);
    assert_eq!(progress.len(), 2, "old read, then new: {stderr_new}");
    assert!(
        progress[1].contains("new schema") && progress[1].contains("broken.proto"),
        "the last progress line names the new side: {stderr_new}"
    );
}

#[test]
fn exit_code_diverges_on_a_breaking_change_and_only_then() {
    // `--exit-code` turns a breaking comparison into `Diverged` (9) — the product still on
    // stdout, since the verdict is not an error — and leaves a comparison with nothing breaking
    // (a rename beside an addition) at 0.
    let (old, new) = thermal_pair();
    let out = diff(&old, &new, &[&fixtures()], &["--exit-code", "--json"]);
    assert_eq!(
        out.status.code(),
        Some(9),
        "exit Diverged: {}",
        stderr(&out)
    );
    assert_eq!(
        stdout(&out),
        THERMAL_CHANGESET,
        "the product precedes the verdict"
    );

    let dir = scratch("diff_calm_pair");
    let calm_old = write(
        &dir,
        "calm_v1.proto",
        "syntax = \"proto3\";\npackage calm.v1;\nmessage Reading { string sensor = 1; int32 temp_c = 2; }\n",
    );
    let calm_new = write(
        &dir,
        "calm_v2.proto",
        "syntax = \"proto3\";\npackage calm.v2;\nmessage Reading { string sensor = 1; int32 celsius = 2; int32 humidity = 3; }\n",
    );
    let out = diff(&calm_old, &calm_new, &[&dir], &["--exit-code", "--json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "nothing breaking: {}",
        stderr(&out)
    );
    let product = stdout(&out);
    assert!(
        product.contains(r#""kind":"renamed""#) && product.contains(r#""kind":"added""#),
        "the rename and the addition are reported: {product}"
    );
    assert!(
        !product.contains(r#""breaking":true"#),
        "no record is breaking: {product}"
    );
}

#[test]
fn mixed_doors_are_refused_before_either_side_is_read() {
    // One side a `.binpb` descriptor set and the other `.proto` source is a usage error (2)
    // naming the rule — both sides go through one door — decided before either side is read, so
    // no progress line precedes it; in either order.
    let dir = scratch("diff_mixed_doors");
    let set = write(
        &dir,
        "old.binpb",
        support::compile_fixture("evolution_v1.proto"),
    );
    let (_, new) = thermal_pair();
    for (old_side, new_side) in [(&set, &new), (&new, &set)] {
        let out = diff(old_side, new_side, &[&fixtures()], &[]);
        assert_eq!(out.status.code(), Some(2), "exit Usage: {}", stderr(&out));
        assert!(out.stdout.is_empty(), "no product on error");
        let stderr = stderr(&out);
        assert!(
            stderr.contains("one door") && stderr.contains(".proto") && stderr.contains(".binpb"),
            "the rule is named: {stderr}"
        );
        assert!(
            progress_lines(&stderr).is_empty(),
            "refused before either side is read: {stderr}"
        );
    }
}

#[test]
fn two_schemas_sharing_only_a_well_known_import_are_not_comparable() {
    // Two real schemas that each import a well-known type but have no package in common: the
    // import is referent closure, not subject vocabulary, so the two are not comparable — a
    // usage error (2) naming each side's own packages, never the `google.protobuf` closure.
    let dir = scratch("diff_wkt_only");
    let alpha = write(
        &dir,
        "alpha.proto",
        "syntax = \"proto3\";\npackage alpha.v1;\nimport \"google/protobuf/timestamp.proto\";\nmessage Ping { google.protobuf.Timestamp at = 1; }\n",
    );
    let beta = write(
        &dir,
        "beta.proto",
        "syntax = \"proto3\";\npackage beta.v1;\nimport \"google/protobuf/duration.proto\";\nmessage Pong { google.protobuf.Duration wait = 1; }\n",
    );
    let out = diff(&alpha, &beta, &[&dir], &[]);
    assert_eq!(out.status.code(), Some(2), "exit Usage: {}", stderr(&out));
    assert!(out.stdout.is_empty(), "no product on error");
    let stderr = stderr(&out);
    assert!(
        stderr.contains("no_comparable_schemas"),
        "the diagnostic names the cause: {stderr}"
    );
    assert!(
        stderr.contains("`alpha`") && stderr.contains("`beta`"),
        "each side's own package is named: {stderr}"
    );
    assert!(
        !stderr.contains("google.protobuf"),
        "the well-known closure is not dumped: {stderr}"
    );
    assert_eq!(
        progress_lines(&stderr).len(),
        2,
        "both sides were read: {stderr}"
    );
}
