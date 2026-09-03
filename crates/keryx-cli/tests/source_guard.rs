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
