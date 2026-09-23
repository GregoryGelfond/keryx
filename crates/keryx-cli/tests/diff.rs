//! `keryx diff` end to end (spec §13.4, §27; architecture §6): the old and the new schema in — each
//! `.proto` source or a `.binpb` descriptor set, through the shipped descriptor doors, one door for
//! both sides — the migration report on stdout by default and the JSON changeset under `--json`,
//! per-side progress and diagnostics on stderr, and the exit contract: `0` on any successful
//! comparison, `Diverged` (9) only under `--exit-code` when a change is breaking, `Usage` (2) for
//! mixed doors or two schemas with no package in common, `Schema` (4) for a side that does not
//! build — the last progress line naming which side. Under `--bridge <path>` the bridge views for
//! the clean renames go to that file after the product — every rename's rendered rule under its
//! `%!` provenance line, in the changeset's order, as the renderer spelled it (`golden/*.bridge.lp`)
//! — with a note and no file when there is no clean rename, and `Input` (3) over the verdict when
//! the file cannot be written. The thermal pair is keryx-core's evolution fixture
//! (`evolution_v1.proto` -> `evolution_v2.proto`); the climate pair — two subject packages a side
//! and a third on the new side, every kind of field change, an unchanged message and enum — is its
//! report fixture (`report_v1.proto` -> `report_v2.proto`, each with its imports as one descriptor
//! set); the rest are written here. The report is pinned to goldens (`golden/*.report`) with the
//! banner's keryx version masked ([`masked`]); coloured under `--color always`, it is the same
//! text with escape sequences around its roles ([`unstyled`] strips them), and plain on a pipe
//! under the default `auto`.

use keryx_test_support as support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use support::fixtures;

/// The changeset keryx-core pins for the thermal pair — the CLI's `--json` product is that text
/// plus one line terminator, which the golden file (newline-terminated like every text file
/// beside it) already is.
const THERMAL_CHANGESET: &str = include_str!("../../keryx-core/tests/golden/evolution.json");

/// The plain report for the climate pair — three packages, every kind of field change, the
/// unchanged shown — with its version line masked.
const CLIMATE_REPORT: &str = include_str!("golden/climate.report");

/// The plain report for the thermal pair — one package, so the banner names the transition — with
/// its version line masked.
const EVOLUTION_REPORT: &str = include_str!("golden/evolution.report");

/// The plain report for the telemetry pair — proto2 to proto3: an enum's openness flipped, a
/// `required` field and the `optional` scalars implicit now, a field into a oneof, maps and
/// scalars of several kinds — with its version line masked.
const TELEMETRY_REPORT: &str = include_str!("golden/telemetry.report");

/// The old side of the panel pair: one message, three scalar fields.
const PANEL_V1: &str = "syntax = \"proto3\";\npackage thermal.v1;\nmessage Reading { string sensor = 1; int32 temp_c = 2; int32 rh = 3; }\n";

/// The new side of the panel pair: two of `Reading`'s fields renamed at their numbers (`temp_c`
/// → `celsius`, `rh` → `humidity`), and a nested `Panel.Reading` added whose collision re-spells
/// `Reading`'s predicate `v2__reading` — a sort renamed beside two fields renamed, three clean
/// renames of two arities.
const PANEL_V2: &str = "syntax = \"proto3\";\npackage thermal.v2;\nmessage Reading { string sensor = 1; int32 celsius = 2; int32 humidity = 3; }\nmessage Panel { message Reading { int32 n = 1; } Reading inner = 1; }\n";

/// The bridge file for the panel pair: the three views in the changeset's order — the sort's,
/// then the fields' by number — each its `%!` provenance line and the one rule `old :- new.` at
/// the shared arity, as the renderer spells them and laid out as a generated module lays out its
/// rules, one after the other.
const PANEL_BRIDGES: &str = include_str!("golden/panel.bridge.lp");

/// The keryx version as the report's banner names it — this build's, since the suite and the
/// binary are one package.
fn version() -> String {
    format!("keryx {}", env!("CARGO_PKG_VERSION"))
}

/// The report with its version masked: the banner line ending in `keryx <this version>` — the one
/// volatile text in a report, as the manifest goldens' header is — has its padding trimmed and the
/// version replaced by `<version>`, so the golden pins the transition the line names and nothing a
/// version bump would move. Newline-terminated as the report is. Panics unless exactly one line
/// carried the version.
fn masked(report: &str) -> String {
    let version = version();
    let mut carried = 0;
    let text: String = report
        .lines()
        .map(|line| match line.strip_suffix(version.as_str()) {
            Some(left) => {
                carried += 1;
                format!("{}  keryx <version>\n", left.trim_end())
            }
            None => format!("{line}\n"),
        })
        .collect();
    assert_eq!(carried, 1, "one version line: {report}");
    text
}

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

/// The climate pair: keryx-core's report fixture, old then new, each compiled with its imports to
/// one descriptor set — the multi-package route, since a `.proto` side scopes its subject
/// vocabulary to the one opened file while a `.binpb` side carries every non-dependency file it
/// holds.
fn climate_pair() -> (PathBuf, PathBuf) {
    let dir = scratch("diff_climate_pair");
    (
        write(
            &dir,
            "climate_v1.binpb",
            support::compile_fixture("report_v1.proto"),
        ),
        write(
            &dir,
            "climate_v2.binpb",
            support::compile_fixture("report_v2.proto"),
        ),
    )
}

/// The command `keryx diff <old> <new> [-I include]… [args]…`, the runner's `RUST_BACKTRACE`
/// cleared so the subprocess shows the default panic posture, and its `NO_COLOR` cleared so the
/// colour decision is the test's own.
fn command(old: &Path, new: &Path, includes: &[&Path], args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_keryx"));
    command.arg("diff").arg(old).arg(new);
    for include in includes {
        command.arg("-I").arg(include);
    }
    command
        .args(args)
        .env_remove("RUST_BACKTRACE")
        .env_remove("NO_COLOR");
    command
}

/// Run [`command`].
fn diff(old: &Path, new: &Path, includes: &[&Path], args: &[&str]) -> Output {
    command(old, new, includes, args).output().unwrap()
}

/// Run [`command`] with `--bridge <bridge>`.
fn diff_bridged(
    old: &Path,
    new: &Path,
    includes: &[&Path],
    args: &[&str],
    bridge: &Path,
) -> Output {
    command(old, new, includes, args)
        .arg("--bridge")
        .arg(bridge)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `text` with every escape sequence removed: the plain text a coloured report is over.
fn unstyled(text: &str) -> String {
    let mut plain = String::new();
    let mut rest = text;
    while let Some((before, sequence)) = rest.split_once('\x1b') {
        plain.push_str(before);
        assert!(
            sequence.starts_with('['),
            "a control sequence: {sequence:?}"
        );
        rest = sequence.split_once('m').map_or("", |(_, after)| after);
    }
    plain.push_str(rest);
    plain
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
fn the_report_is_pinned_to_its_golden_with_the_version_masked() {
    // The climate pair under `--color never`: the plain report, byte for byte the golden once the
    // version line is masked, and no escape sequence in it. The alerts package's message and
    // enum, untouched by the new side, are present as headers marked `unchanged` — shown only
    // because the renderer reads the comparison's tree, where the unchanged nodes are, and not
    // its flat change rows, where they are not.
    let (old, new) = climate_pair();
    let out = diff(&old, &new, &[], &["--color", "never"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = stdout(&out);
    assert!(!report.contains('\x1b'), "no escape sequence: {report:?}");
    let unchanged: Vec<&str> = report
        .lines()
        .filter(|line| line.ends_with("unchanged"))
        .collect();
    assert_eq!(
        unchanged.len(),
        2,
        "the alerts package's message and enum: {report}"
    );
    assert!(
        unchanged[0].starts_with(" alert · climate.alerts.Alert")
            && unchanged[1].starts_with(" level · climate.alerts.Level"),
        "each shown under its predicate and version-free path: {report}"
    );
    // `Grade`'s predicate is re-spelled `v2__grade` by a collision on the new side: the enum's own
    // header says so, once, with its bridge — and `Reading.grade`, which names the enum on both
    // sides, is no row at all, since a referent renamed is that enum's change and not the field's.
    let respelled: Vec<&str> = report
        .lines()
        .filter(|line| line.contains("v2__grade"))
        .collect();
    assert_eq!(
        respelled.len(),
        1,
        "the rename on the enum's header only: {report}"
    );
    assert!(
        respelled[0].starts_with(" grade → v2__grade · climate.Grade")
            && respelled[0].ends_with("~ renamed · bridge available"),
        "{report}"
    );
    assert_eq!(masked(&report), CLIMATE_REPORT);
}

#[test]
fn a_syntax_migration_shows_the_enum_flip_and_the_presence_deltas() {
    // The telemetry pair through the `.proto` door — proto2 old, proto3 new: the enum whose
    // openness flipped carries its descriptors old → new on its header, the once-`required` field
    // and the `optional` scalars show their presence old → new as rows, the field that joined a
    // oneof shows its form and its cell, and the maps and scalars spell their types in the
    // manifest's words. No rename, so no bridge line and no hint.
    let out = diff(
        &fixtures().join("telemetry_v1.proto"),
        &fixtures().join("telemetry_v2.proto"),
        &[&fixtures()],
        &["--color", "never"],
    );
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = stdout(&out);
    assert!(!report.contains('\x1b'), "no escape sequence: {report:?}");
    let flipped = report
        .lines()
        .find(|line| line.starts_with(" mode · telemetry.Mode"))
        .expect("the enum's header");
    assert!(
        flipped.contains("(closed) → (open)") && flipped.ends_with("! changed · no bridge"),
        "the openness flip on the header: {flipped}"
    );
    assert!(
        report.contains("required → total") && report.contains("no oneof → oneof{4}"),
        "the presence and the cell deltas: {report}"
    );
    assert!(
        !report.contains("bridge") || report.contains("no bridge"),
        "nothing bridged: {report}"
    );
    assert_eq!(masked(&report), TELEMETRY_REPORT);
}

#[test]
fn a_single_package_report_names_the_transition_in_its_banner() {
    // The thermal pair through the `.proto` door: one package, so the banner's second line is
    // the version transition itself beside this build's keryx version; masked, the golden keeps
    // the transition and drops the version.
    let (old, new) = thermal_pair();
    let out = diff(&old, &new, &[&fixtures()], &["--color", "never"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = stdout(&out);
    let banner = report.lines().nth(1).expect("a banner line");
    assert!(
        banner.starts_with("   thermal.v1  →  thermal.v2") && banner.ends_with(&version()),
        "the transition and the version: {banner}"
    );
    assert_eq!(masked(&report), EVOLUTION_REPORT);
}

#[test]
fn the_report_is_coloured_under_always_and_plain_on_a_pipe() {
    // The thermal pair under `--color always`: the report carries escape sequences by role — on
    // the rename row, the gutter dim, `~ renamed` yellow, the two names bold around a dim arrow,
    // the shared arity dim, the annotation unpainted — and stripped of them it is the `--color
    // never` report byte for byte, so the plain golden pins the coloured layout too. `always`
    // decides alone: `NO_COLOR=1` in the environment moves it no more than the pipe does. Under
    // the default `auto`, stdout here is a pipe and not a terminal, so the report is plain.
    let (old, new) = thermal_pair();
    let out = diff(&old, &new, &[&fixtures()], &["--color", "always"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let colored = stdout(&out);
    let plain = stdout(&diff(&old, &new, &[&fixtures()], &["--color", "never"]));
    assert!(!plain.contains('\x1b'), "no escape sequence: {plain:?}");
    assert_ne!(colored, plain, "the coloured report carries sequences");
    assert_eq!(unstyled(&colored), plain, "the same text under the colour");
    let row = colored
        .lines()
        .find(|line| unstyled(line).starts_with("  #2  ~ renamed   temp_c → celsius"))
        .expect("the rename row");
    assert!(
        row.starts_with("\x1b[2m  #2\x1b[0m  \x1b[33m~ renamed\x1b[0m   ")
            && row.contains(
                "\x1b[1mtemp_c\x1b[0m\x1b[2m → \x1b[0m\x1b[1mcelsius\x1b[0m  \x1b[2m/2\x1b[0m"
            )
            && row.ends_with("  bridge available"),
        "the roles' sequences: {row:?}"
    );
    let forced = command(&old, &new, &[&fixtures()], &["--color", "always"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(stdout(&forced), colored, "`always` overrides `NO_COLOR`");
    let auto = stdout(&diff(&old, &new, &[&fixtures()], &[]));
    assert_eq!(auto, plain, "`auto` on a pipe is the plain report");
}

#[test]
fn a_referent_renamed_is_the_sorts_change_not_the_fields_emphasis() {
    // `Batch.readings` names `Reading` on both sides and goes from a sequence to a singular field
    // (whose presence, explicit for a proto3 message field, is partial where the sequence's was
    // total); on the new side a nested `Panel.Reading` collides with `Reading`, whose predicate
    // the collision re-spells `v2__reading`. The field's signature strings therefore differ in
    // the type too (`reading` → `v2__reading`), but the comparison records the form, the arity,
    // and the presence as the field's own change and not the type — the referent is the same
    // sort on both sides — and the emphasis is read from that record: the row bolds those three
    // deltas and names the re-spelling nowhere; it appears once in the report, bold on the
    // sort's own header as its rename.
    let dir = scratch("diff_referent_renamed");
    let old = write(
        &dir,
        "referent_v1.proto",
        "syntax = \"proto3\";\npackage thermal.v1;\nmessage Reading { string sensor = 1; }\nmessage Batch { repeated Reading readings = 1; }\n",
    );
    let new = write(
        &dir,
        "referent_v2.proto",
        "syntax = \"proto3\";\npackage thermal.v2;\nmessage Reading { string sensor = 1; }\nmessage Batch { Reading readings = 1; }\nmessage Panel { message Reading { int32 n = 1; } Reading inner = 1; }\n",
    );
    let out = diff(&old, &new, &[&dir], &["--color", "always"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = stdout(&out);
    let row = report
        .lines()
        .find(|line| unstyled(line).starts_with("  #1  ! changed   readings"))
        .expect("the changed field's row");
    assert!(
        unstyled(row).contains("seq → singular, /3 → /2, total → partial  no bridge"),
        "the form, the arity, and the presence, and no type delta: {row:?}"
    );
    assert!(
        row.contains("\x1b[1mseq\x1b[0m\x1b[2m → \x1b[0m\x1b[1msingular\x1b[0m")
            && row.contains("\x1b[1m/3\x1b[0m\x1b[2m → \x1b[0m\x1b[1m/2\x1b[0m")
            && row.contains("\x1b[1mtotal\x1b[0m\x1b[2m → \x1b[0m\x1b[1mpartial\x1b[0m")
            && !row.contains("v2__reading"),
        "the aspect's dimensions bold, the re-spelling not on the row: {row:?}"
    );
    let header = report
        .lines()
        .find(|line| unstyled(line).starts_with(" reading → v2__reading · thermal.Reading"))
        .expect("the sort's header");
    assert!(
        header.contains("\x1b[1mreading\x1b[0m\x1b[2m → \x1b[0m\x1b[1mv2__reading\x1b[0m")
            && unstyled(header).ends_with("~ renamed · bridge available"),
        "the sort's own rename, bold on its header: {header:?}"
    );
    assert_eq!(
        report.matches("v2__reading").count(),
        1,
        "the re-spelling is on the header alone: {report}"
    );
}

#[test]
fn an_identical_pair_reports_nothing_changed() {
    // A schema against itself: every message and enum is still shown, each marked `unchanged`,
    // no change row at all, and the footer says so — the report shows what stayed, not only what
    // changed.
    let (old, _) = thermal_pair();
    let out = diff(&old, &old, &[&fixtures()], &["--color", "never"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let report = stdout(&out);
    assert_eq!(
        report
            .lines()
            .filter(|line| line.ends_with("unchanged"))
            .count(),
        3,
        "two messages and one enum: {report}"
    );
    assert!(
        !report.lines().any(|line| line.starts_with("  #")),
        "no change row: {report}"
    );
    assert!(
        report.lines().any(|line| line == "  no changes"),
        "the footer: {report}"
    );
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
fn the_bridge_file_carries_every_clean_renames_rule_in_the_changesets_order() {
    // The panel pair under `--bridge`: the file is the three bridge views in the changeset's
    // order — the sort's (`reading → v2__reading`, arity 1), then the fields' by number (`temp_c
    // → celsius`, `rh → humidity`, arity 2) — each the renderer's own text, a `%!` line naming
    // the rename over the one rule `old :- new.`, one after the other as a generated module lays
    // its rules out: no free-standing `%`, nothing of the command's own; byte for byte the
    // golden. The product is stdout's first and whole — the changeset, whose three records carry
    // the same three views — and stderr is the two progress lines and then `wrote <path>`.
    let dir = scratch("diff_bridge_file");
    let old = write(&dir, "panel_v1.proto", PANEL_V1);
    let new = write(&dir, "panel_v2.proto", PANEL_V2);
    let bridge = dir.join("panel.bridge.lp");
    let out = diff_bridged(&old, &new, &[&dir], &["--json"], &bridge);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let views = std::fs::read_to_string(&bridge).expect("the bridge file is written");
    assert_eq!(views, PANEL_BRIDGES, "the golden, byte for byte");
    let rules: Vec<&str> = views
        .lines()
        .filter(|line| !line.starts_with("%! "))
        .collect();
    assert_eq!(
        rules,
        [
            "reading(A) :- v2__reading(A).",
            "temp_c(A, B) :- celsius(A, B).",
            "rh(A, B) :- humidity(A, B).",
        ],
        "every rename's rule, the sort's first, then the fields' by number: {views}"
    );
    assert_eq!(
        views.lines().filter(|line| line.starts_with("%! ")).count(),
        3,
        "each rule under its own provenance line: {views}"
    );
    assert!(
        !views
            .lines()
            .any(|line| line.starts_with('%') && !line.starts_with("%! ")),
        "no free-standing `%`: {views}"
    );
    let product = stdout(&out);
    assert!(
        product.starts_with('[') && product.matches(r#""bridge":"#).count() == 3,
        "the changeset, with the three views in its records: {product}"
    );
    let stderr = stderr(&out);
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(
        progress_lines(&stderr).len(),
        2,
        "one progress line per side: {stderr}"
    );
    assert_eq!(
        lines.last().copied(),
        Some(format!("keryx: wrote {}", bridge.display()).as_str()),
        "the write reported after the sides: {stderr}"
    );
    assert_eq!(lines.len(), 3, "nothing else on stderr: {stderr}");
}

#[test]
fn a_bridge_that_cannot_be_written_is_input_over_a_delivered_report() {
    // `--bridge` into a directory that is not there: the report is still the product, on stdout
    // first and whole (the golden, masked); then the write fails — `Input` (3), the file-I/O
    // class, naming the path, structured on a pipe like every adapter error — with no `wrote`
    // line and no file. The product is delivered before the file is attempted, so a bad path
    // never costs the report.
    let dir = scratch("diff_bridge_unwritable");
    let bridge = dir.join("missing").join("thermal.bridge.lp");
    let (old, new) = thermal_pair();
    let out = diff_bridged(&old, &new, &[&fixtures()], &["--color", "never"], &bridge);
    assert_eq!(out.status.code(), Some(3), "exit Input: {}", stderr(&out));
    assert_eq!(
        masked(&stdout(&out)),
        EVOLUTION_REPORT,
        "the report is delivered first"
    );
    let stderr = stderr(&out);
    assert!(
        stderr.contains("cannot write") && stderr.contains(&bridge.display().to_string()),
        "the failure names the path: {stderr}"
    );
    assert!(
        stderr.contains(r#""kind":"input""#),
        "the class, structured on a pipe: {stderr}"
    );
    assert!(!stderr.contains("wrote"), "nothing written: {stderr}");
    assert!(!bridge.exists(), "no file: {}", bridge.display());
}

#[test]
fn a_failed_bridge_write_dominates_the_verdict_and_a_written_one_leaves_it() {
    // `--exit-code` beside `--bridge` on the breaking thermal pair. Written, the file carries the
    // rename's view and the exit is the verdict, `Diverged` (9): the bridge is an output, not a
    // judgement, and delivering it changes none. Not written, the exit is `Input` (3): the
    // requested output did not arrive, a class the verdict must not mask — the changeset on
    // stdout either way, first.
    let dir = scratch("diff_bridge_verdict");
    let (old, new) = thermal_pair();
    let written = dir.join("thermal.bridge.lp");
    let out = diff_bridged(
        &old,
        &new,
        &[&fixtures()],
        &["--exit-code", "--json"],
        &written,
    );
    assert_eq!(
        out.status.code(),
        Some(9),
        "exit Diverged: {}",
        stderr(&out)
    );
    assert_eq!(
        stdout(&out),
        THERMAL_CHANGESET,
        "the product, then the verdict"
    );
    let views = std::fs::read_to_string(&written).expect("the bridge file is written");
    assert!(
        views.starts_with("%! temp_c/2 reads celsius/2")
            && views.ends_with("temp_c(A, B) :- celsius(A, B).\n"),
        "the rename's view: {views}"
    );
    let unwritable = dir.join("missing").join("thermal.bridge.lp");
    let out = diff_bridged(
        &old,
        &new,
        &[&fixtures()],
        &["--exit-code", "--json"],
        &unwritable,
    );
    assert_eq!(
        out.status.code(),
        Some(3),
        "exit Input over Diverged: {}",
        stderr(&out)
    );
    assert_eq!(
        stdout(&out),
        THERMAL_CHANGESET,
        "the product is delivered before the file is attempted"
    );
    assert!(!unwritable.exists(), "no file: {}", unwritable.display());
}

#[test]
fn no_clean_rename_is_a_note_and_no_bridge_file() {
    // `--bridge` over a comparison with nothing to bridge — the telemetry pair, every kind of
    // change but no rename, and a schema against itself — is a note on stderr, `no bridge views
    // (no clean renames)`, and no file: not an empty one, not a comment-only one. The product is
    // delivered as ever, and the exit is 0.
    let dir = scratch("diff_bridge_none");
    let (thermal, _) = thermal_pair();
    let pairs = [
        (
            "telemetry",
            fixtures().join("telemetry_v1.proto"),
            fixtures().join("telemetry_v2.proto"),
        ),
        ("identical", thermal.clone(), thermal),
    ];
    for (name, old, new) in &pairs {
        let bridge = dir.join(format!("{name}.bridge.lp"));
        let out = diff_bridged(old, new, &[&fixtures()], &["--color", "never"], &bridge);
        assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
        assert!(
            !bridge.exists(),
            "no file for nothing: {}",
            bridge.display()
        );
        let stderr = stderr(&out);
        assert!(
            stderr
                .lines()
                .any(|line| line == "keryx: no bridge views (no clean renames)"),
            "the note: {stderr}"
        );
        assert!(!stderr.contains("wrote"), "nothing written: {stderr}");
        assert!(!stdout(&out).is_empty(), "the report is delivered: {name}");
    }
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
