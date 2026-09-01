//! The `schema-facts` command end to end: descriptor set in, facts on stdout,
//! diagnostics on stderr, stable exit codes (§6). The set is produced by protox,
//! reusing keryx-core's fixtures.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)
}

#[test]
fn writes_facts_to_stdout() {
    let path = tmp("proto3.binpb");
    std::fs::write(&path, support::compile_fixture("proto3.proto")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("schema-facts")
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains(r#"message("keryx.p3.Reading", "proto3.proto")."#));
    assert!(out.stderr.is_empty(), "stderr is quiet on success");
}

#[test]
fn missing_argument_is_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("schema-facts")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn unreadable_set_is_schema_error() {
    let path = tmp("garbage.binpb");
    // A field-1 length prefix claiming far more bytes than follow — decode fails.
    std::fs::write(&path, b"\x0a\xff\xff\xff\x0f").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("schema-facts")
        .arg(&path)
        .output()
        .unwrap();
    // The set read fine but is not a valid schema: a schema error (§6), not an input
    // (file-read) error — matching `gen`/`explain` and the `exit.rs` taxonomy.
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stdout.is_empty(), "no partial product on error");
    assert!(!out.stderr.is_empty(), "the diagnostic is on stderr");
}

#[test]
fn missing_file_is_an_input_error() {
    // A file that cannot be read stays `Input`(3) — distinct from a set that reads but
    // does not decode (`Schema`, 4): the two failures are not conflated (§6).
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("schema-facts")
        .arg(tmp("does-not-exist.binpb"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty(), "no partial product on error");
    assert!(!out.stderr.is_empty(), "the diagnostic is on stderr");
}
