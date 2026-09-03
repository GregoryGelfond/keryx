//! The source-nesting guard end to end (§6; the threat model's bounded-depth-walks property for the
//! source door): a deeply-nested `.proto` is refused (`SourceTooDeep`, exit `Schema`) **before** protox
//! parses it, so protox's unbounded recursive-descent parser cannot overflow the stack and abort.
//! Without the guard this input aborts the process (killed by a signal, no exit code); with it, keryx
//! exits cleanly. Run as a subprocess because an abort would kill the test binary — so this test fails
//! (no `Some(4)`) if the guard is removed or mis-placed.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_deeply_nested_source_is_refused_not_aborted() {
    let dir = scratch("deep_source");
    // Well above the ~900 abort threshold on the binary's 8 MB main thread.
    let depth = 2_000;
    let mut source = String::from("syntax = \"proto3\";\npackage deep;\n");
    for i in 0..depth {
        let _ = writeln!(source, "message M{i} {{");
    }
    for _ in 0..depth {
        source.push_str("}\n");
    }
    std::fs::write(dir.join("deep.proto"), source).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("gen")
        .arg("deep.proto")
        .arg("-I")
        .arg(&dir)
        .arg("-o")
        .arg(&dir)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    // A clean refusal exits `Schema` (4); an abort is killed by a signal and yields no exit code.
    assert_eq!(
        out.status.code(),
        Some(4),
        "the guard pre-empts the abort and exits Schema; stderr: {stderr}"
    );
    assert!(
        stderr.contains("source_too_deep"),
        "the clean diagnostic: {stderr}"
    );
}

#[test]
fn a_deep_option_aggregate_is_refused_not_aborted() {
    // The second parser: an option's aggregate value is re-parsed by prost-reflect's *unbounded*
    // text-format parser, which nests on `< >` as well as `{ }`. A deep `< >` aggregate has shallow
    // brace depth, so only counting `< >` too keeps it from slipping past the guard and aborting the
    // process inside that parser. The scanner counts it and refuses before protox parses.
    let dir = scratch("deep_aggregate");
    let depth = 2_000;
    let mut source = String::from(
        "syntax = \"proto2\";\npackage agg;\nimport \"google/protobuf/descriptor.proto\";\n\
         message Rec { optional Rec f = 1; }\n\
         extend google.protobuf.MessageOptions { optional Rec r = 50000; }\n\
         message M {\n  option (r) = { ",
    );
    for _ in 0..depth {
        source.push_str("f < ");
    }
    for _ in 0..depth {
        source.push('>');
    }
    source.push_str(" };\n}\n");
    std::fs::write(dir.join("agg.proto"), source).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .args(["gen", "agg.proto", "-I"])
        .arg(&dir)
        .arg("-o")
        .arg(&dir)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(4),
        "the `< >` count pre-empts the text-format abort and exits Schema; stderr: {stderr}"
    );
    assert!(
        stderr.contains("source_too_deep"),
        "the clean diagnostic: {stderr}"
    );
}

#[test]
fn a_long_import_chain_is_refused_not_aborted() {
    // protox resolves imports by recursion (`add_import` → `add_import`), an abort axis the per-file
    // brace scan cannot see. keryx's resolver counts the files it opens and refuses an over-large
    // import graph before that recursion overflows. A 200-file chain is brace-flat — the nesting
    // guard admits every file — but exceeds the import budget, so keryx refuses it cleanly.
    let dir = scratch("import_chain");
    let files = 200;
    for i in 0..files {
        let source = format!(
            "syntax = \"proto3\";\npackage c;\nimport \"f{}.proto\";\n",
            i + 1
        );
        std::fs::write(dir.join(format!("f{i}.proto")), source).unwrap();
    }
    std::fs::write(
        dir.join(format!("f{files}.proto")),
        "syntax = \"proto3\";\npackage c;\nmessage Leaf { int32 x = 1; }\n",
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .args(["gen", "f0.proto", "-I"])
        .arg(&dir)
        .arg("-o")
        .arg(&dir)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(4),
        "the import budget pre-empts the recursion abort and exits Schema; stderr: {stderr}"
    );
    assert!(
        stderr.contains("source_import_graph_too_large"),
        "the clean diagnostic: {stderr}"
    );
}
