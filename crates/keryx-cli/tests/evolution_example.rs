//! The evolution example (`examples/evolution`; spec §13.4, §27): documentation by example and a
//! regression suite in one, in the `thermal_example` mould — `keryx diff` run over the committed
//! sources, from the example's directory as the README's commands run, and held to the committed
//! products. The worked pair `thermal-v1.proto` -> `thermal-v2.proto` goes through the `.proto`
//! door: the migration report (`diff.txt`, compared with its keryx version masked), the JSON
//! changeset (`diff.json`), and the bridge file (`thermal.bridge.lp`) — each the command's own
//! output, captured and committed, never composed by hand. The unrelated pair `heartbeat.proto` /
//! `invoice.proto` shares nothing but a well-known import: not comparable, and the shared
//! `google.protobuf` closure is named nowhere. The station sets `station/v1.binpb` ->
//! `station/v2.binpb` go through the `.binpb` door: a package retired and one introduced beside the
//! one matched, each its own block of the report (`station/diff.txt`) — and the committed sets are
//! the committed sources compiled, since the same sources compiled here diff to the same report.
//! The bridge's demonstration — a v2 batch shredded (`batch.txtpb` -> `facts.lp`) for a v1 rule
//! (`hot.lp`) to read through the bridge — is pinned at the shred; the clingo run over the three
//! is the reader's, since the suite spawns no solver.

use keryx_test_support as support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The example's directory (`examples/evolution`).
fn example() -> PathBuf {
    // keryx-cli/ -> crates/ -> repo root.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/evolution")
}

/// The committed text of the example artifact `name`.
fn golden(name: &str) -> String {
    std::fs::read_to_string(example().join(name)).expect("golden present")
}

/// A fresh scratch directory per test (parallel-safe).
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `keryx <args>…` from the example's directory — the README's commands, verbatim — with the
/// runner's `RUST_BACKTRACE` cleared so the subprocess shows the default panic posture and its
/// `NO_COLOR` cleared so the colour decision is the command line's own.
fn keryx(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_keryx"))
        .current_dir(example())
        .args(args)
        .env_remove("RUST_BACKTRACE")
        .env_remove("NO_COLOR")
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The keryx version as the report's banner names it — this build's, since the suite and the
/// binary are one package.
fn version() -> String {
    format!("keryx {}", env!("CARGO_PKG_VERSION"))
}

/// Whether `text` is a version as the banner spells one (`0.1.0`, or a pre-release such as
/// `0.2.0-rc.1`): opens with a digit, then only what a Cargo version is made of.
fn is_version(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_digit())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

/// The report with its version masked: the banner line ending in `keryx <a version>` — the one
/// volatile text in a report — has its padding trimmed and the version replaced by `<version>`.
/// Applied to the command's output and to the committed report alike, so the two are compared
/// modulo that token: the committed report is the command's output exactly as it was captured,
/// version and all, and a version bump moves nothing the test holds it to. Panics unless exactly
/// one line carried a version.
fn masked(report: &str) -> String {
    let mut carried = 0;
    let text: String = report
        .lines()
        .map(|line| match line.rsplit_once("  keryx ") {
            Some((left, version)) if is_version(version) => {
                carried += 1;
                format!("{}  keryx <version>\n", left.trim_end())
            }
            _ => format!("{line}\n"),
        })
        .collect();
    assert_eq!(carried, 1, "one version line: {report}");
    text
}

/// The lines of `report` ending in `suffix`, for the header and row assertions.
fn lines_ending<'a>(report: &'a str, suffix: &str) -> Vec<&'a str> {
    report
        .lines()
        .filter(|line| line.ends_with(suffix))
        .collect()
}

#[test]
fn the_report_matches_the_committed_diff_txt_with_the_version_masked() {
    // `keryx diff thermal-v1.proto thermal-v2.proto -I . --color never`: exit 0 (a breaking
    // change is a finding, not an error), the plain report — no escape sequence — naming this
    // build's keryx version on its banner, and, with that version masked on both sides, byte for
    // byte the committed `diff.txt`. The rows the README walks through are then asserted by
    // name, so a change to the example fails here with the claim it breaks.
    let out = keryx(&[
        "diff",
        "thermal-v1.proto",
        "thermal-v2.proto",
        "-I",
        ".",
        "--color",
        "never",
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = stdout(&out);
    assert!(!report.contains('\x1b'), "no escape sequence: {report:?}");
    let banner = report.lines().nth(1).expect("a banner line");
    assert!(
        banner.starts_with("   thermal.v1  →  thermal.v2") && banner.ends_with(&version()),
        "the transition and this build's version: {banner}"
    );
    assert_eq!(masked(&report), masked(&golden("diff.txt")));

    // The rename bridges; the set-to-sequence change does not.
    assert!(
        report.contains("  #2  ~ renamed   temp_c → celsius  /2            bridge available"),
        "{report}"
    );
    assert!(
        report.contains("  #1  ! changed   alerts            set → seq, /2 → /3  no bridge"),
        "{report}"
    );
    // The two sorts v2 left alone are shown, marked so — what stayed beside what changed.
    let unchanged = lines_ending(&report, "unchanged");
    assert_eq!(unchanged.len(), 2, "{report}");
    assert!(
        unchanged[0].starts_with(" alert · thermal.Alert")
            && unchanged[1].starts_with(" reading_batch · thermal.ReadingBatch"),
        "{report}"
    );
    // The removed and the added message, each with its fields riding as rows.
    assert!(
        report.contains(" calibration · thermal.Calibration               - removed · no bridge")
            && report.contains("  #2  - removed   offset/2          int32,total   no bridge"),
        "{report}"
    );
    assert!(
        report.contains(" site · thermal.Site                                           + added")
            && report.contains("  #2  + added     region/2          string,total"),
        "{report}"
    );
    // The enum: its preserve flip on the header, a value renamed, removed, and added beneath.
    assert!(
        report.contains(" level · thermal.Level  (open) → (open, preserve)  ! changed · no bridge"),
        "{report}"
    );
    assert!(
        report.contains("  #2  ~ renamed   high → critical                 no bridge · constant")
            && report.contains("  #3  - removed   test                            no bridge")
            && report.contains("  #4  + added     advisory"),
        "{report}"
    );
    // The footer's tally and the bridge hint.
    assert!(
        report.contains("  7 breaking   2 removed · 1 changed · 1 message removed · 1 value removed · 1 value renamed · 1 preserve change")
            && report.contains("  1 bridged    1 renamed (inbound-facing)")
            && report.contains("  5 additive   3 added · 1 message added · 1 value added")
            && report.contains("  → rerun with --bridge <path>  to write the 1 bridge view"),
        "{report}"
    );
}

#[test]
fn the_changeset_matches_the_committed_diff_json() {
    // `keryx diff thermal-v1.proto thermal-v2.proto -I . --json`: the changeset, byte for byte the
    // committed `diff.json` — one compact array of thirteen records — with every kind of change
    // the pair makes named by its slug, the rename's record carrying its bridge, and nothing on
    // stderr but the two progress lines. Under `--exit-code` the same product precedes the
    // `Diverged` verdict (9): the comparison is breaking.
    let out = keryx(&[
        "diff",
        "thermal-v1.proto",
        "thermal-v2.proto",
        "-I",
        ".",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let changeset = stdout(&out);
    assert_eq!(changeset, golden("diff.json"));
    assert_eq!(changeset.matches(r#""kind":"#).count(), 13, "{changeset}");
    for kind in [
        "renamed",
        "added",
        "removed",
        "changed",
        "message_added",
        "message_removed",
        "value_renamed",
        "value_removed",
        "value_added",
        "preserve_changed",
    ] {
        assert!(
            changeset.contains(&format!(r#""kind":"{kind}""#)),
            "a `{kind}` record: {changeset}"
        );
    }
    assert!(
        changeset.contains(
            r#""bridge":"%! temp_c/2 reads celsius/2  (inbound-facing bridge: thermal.v1.Reading.temp_c renamed thermal.v2.Reading.celsius)\ntemp_c(A, B) :- celsius(A, B).\n""#
        ),
        "the rename's bridge, newlines escaped: {changeset}"
    );
    let progress = stderr(&out);
    assert_eq!(
        progress.lines().count(),
        2,
        "the two progress lines and nothing else: {progress}"
    );

    let out = keryx(&[
        "diff",
        "thermal-v1.proto",
        "thermal-v2.proto",
        "-I",
        ".",
        "--json",
        "--exit-code",
    ]);
    assert_eq!(
        out.status.code(),
        Some(9),
        "exit Diverged: {}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), changeset, "the product precedes the verdict");
}

#[test]
fn the_bridge_file_matches_the_committed_thermal_bridge_lp() {
    // `keryx diff … --bridge <path>`: the file is the committed `thermal.bridge.lp` — the one
    // clean rename's rule `temp_c(A, B) :- celsius(A, B).` under its `%!` provenance line, and
    // nothing else: a generated module a v1 model loads beside v2 facts. The changeset carries
    // the same text in the rename's record, and stderr says the file was written.
    let dir = scratch("evolution_bridge");
    let bridge = dir.join("thermal.bridge.lp");
    let out = keryx(&[
        "diff",
        "thermal-v1.proto",
        "thermal-v2.proto",
        "-I",
        ".",
        "--json",
        "--bridge",
        bridge.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let views = std::fs::read_to_string(&bridge).expect("the bridge file is written");
    assert_eq!(views, golden("thermal.bridge.lp"));
    assert_eq!(
        views.lines().collect::<Vec<_>>(),
        [
            "%! temp_c/2 reads celsius/2  (inbound-facing bridge: thermal.v1.Reading.temp_c renamed thermal.v2.Reading.celsius)",
            "temp_c(A, B) :- celsius(A, B).",
        ],
        "one rule under its provenance line: {views}"
    );
    let escaped = views.replace('\n', "\\n");
    assert!(
        stdout(&out).contains(&format!(r#""bridge":"{escaped}""#)),
        "the changeset carries the same view"
    );
    assert!(
        stderr(&out)
            .lines()
            .last()
            .is_some_and(|line| line == format!("keryx: wrote {}", bridge.display())),
        "the write is reported: {}",
        stderr(&out)
    );
}

#[test]
fn the_v2_batch_shreds_to_the_committed_facts() {
    // `keryx facts --root ReadingBatch=batch.txtpb thermal-v2.proto -I .`: the committed
    // `facts.lp` — v2's vocabulary, so `celsius` atoms and no `temp_c` at all, which is what the
    // v1 rule in `hot.lp` needs the bridge for.
    let out = keryx(&[
        "facts",
        "--root",
        "ReadingBatch=batch.txtpb",
        "thermal-v2.proto",
        "-I",
        ".",
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let facts = stdout(&out);
    assert_eq!(facts, golden("facts.lp"));
    assert!(
        facts.contains("celsius(readings(r0, 0), 44).") && !facts.contains("temp_c"),
        "v2's vocabulary: {facts}"
    );
}

/// The refusal's detail for the unrelated pair, as the README quotes it: naming each side's own
/// package and nothing of the well-known closure the two share.
const NOT_COMPARABLE: &str = "the old schema (`heartbeat`) and the new schema (`billing`) have no package in common once version segments are stripped (imported packages aside); keryx diff compares two versions of one schema";

#[test]
fn two_schemas_sharing_only_a_well_known_import_are_not_comparable() {
    // `keryx diff heartbeat.proto invoice.proto -I .`: both sides read (a progress line each —
    // the `google.protobuf.Timestamp` import resolved on both), then refused as not comparable —
    // a usage error (2), no product. stderr is pinned whole, in both forms — the structured one
    // a pipe gets, the prose one a terminal gets (`--format human`), which is the text the README
    // quotes — so the refusal names each schema's own package and never the well-known closure
    // the two share (an import is referent closure, not the schema's vocabulary), and a change
    // to its wording fails here rather than leaving the README's quote stale.
    let out = keryx(&["diff", "heartbeat.proto", "invoice.proto", "-I", "."]);
    assert_eq!(out.status.code(), Some(2), "exit Usage: {}", stderr(&out));
    assert!(out.stdout.is_empty(), "no product on a refusal");
    let progress = "keryx: reading the old schema heartbeat.proto\nkeryx: reading the new schema invoice.proto\n";
    assert_eq!(
        stderr(&out),
        format!(
            "{progress}[{{\"field_path\":\"\",\"kind\":\"no_comparable_schemas\",\"detail\":\"{NOT_COMPARABLE}\"}}]\n"
        ),
        "both sides read, then the structured refusal, and nothing else"
    );
    let out = keryx(&[
        "diff",
        "--format",
        "human",
        "heartbeat.proto",
        "invoice.proto",
        "-I",
        ".",
    ]);
    assert_eq!(out.status.code(), Some(2), "exit Usage: {}", stderr(&out));
    assert!(out.stdout.is_empty(), "no product on a refusal");
    let prose = stderr(&out);
    assert_eq!(
        prose,
        format!("{progress}keryx: no_comparable_schemas: {NOT_COMPARABLE}\n"),
        "the prose refusal the README quotes, whole"
    );
    assert!(
        !prose.contains("google.protobuf") && !prose.contains("Timestamp"),
        "the shared well-known type is not dumped: {prose}"
    );
}

#[test]
fn a_multi_package_descriptor_set_is_reported_package_by_package() {
    // `keryx diff station/v1.binpb station/v2.binpb --color never`: through the `.binpb` door a
    // side is every file of its set but the well-known and option-registry ones, so the
    // comparison spans three packages — `station`, matched across the version bump; `station.log`,
    // on the old side only; `station.alarms`, on the new side only — each its own block under
    // the `3 packages` banner, and the changeset carries the package rows. The report is the
    // committed `station/diff.txt`, version masked.
    let out = keryx(&[
        "diff",
        "station/v1.binpb",
        "station/v2.binpb",
        "--color",
        "never",
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = stdout(&out);
    assert!(
        report
            .lines()
            .nth(1)
            .is_some_and(|banner| banner.starts_with("   3 packages")),
        "{report}"
    );
    assert!(
        report.contains(" station.v1  →  station.v2\n ═")
            && report.contains(
                " station.alarms.v2                                             + added\n ═"
            )
            && report.contains(
                " station.log.v1                                  - removed · no bridge\n ═"
            ),
        "a block per package: {report}"
    );
    assert_eq!(masked(&report), masked(&golden("station/diff.txt")));

    let out = keryx(&["diff", "station/v1.binpb", "station/v2.binpb", "--json"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let changeset = stdout(&out);
    assert!(
        changeset.contains(r#""kind":"package_added","new":null,"new_path":"station.alarms.v2","old":null,"old_path":null,"package":"station.alarms""#)
            && changeset.contains(r#""kind":"package_removed","new":null,"new_path":null,"old":null,"old_path":"station.log.v1","package":"station.log""#),
        "the package rows: {changeset}"
    );
}

#[test]
fn the_committed_descriptor_sets_are_the_committed_sources_compiled() {
    // The two `.binpb` files were produced by protoc from `station/v1/` and `station/v2/`
    // (`--include_imports`, so each set carries the package it imports). Compiled here from the
    // same sources — through protox, whose bytes differ in layout, not in meaning — they diff to
    // the same report, so the committed sets and the committed sources cannot drift apart.
    let dir = scratch("evolution_station_sets");
    let mut sets = Vec::new();
    for side in ["v1", "v2"] {
        let root = example().join("station").join(side);
        let bytes = support::compile_in(std::slice::from_ref(&root), "station.proto");
        let path = dir.join(format!("{side}.binpb"));
        std::fs::write(&path, bytes).unwrap();
        sets.push(path);
    }
    let out = keryx(&[
        "diff",
        sets[0].to_str().unwrap(),
        sets[1].to_str().unwrap(),
        "--color",
        "never",
    ]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(masked(&stdout(&out)), masked(&golden("station/diff.txt")));
}
