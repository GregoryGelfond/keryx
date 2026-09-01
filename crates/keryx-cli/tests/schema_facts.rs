//! The `schema-facts` command end to end: descriptor set in, facts on stdout,
//! diagnostics on stderr, stable exit codes (§6). The set is produced by protox,
//! reusing keryx-core's fixtures.

use std::path::{Path, PathBuf};
use std::process::Command;

// `protox::compile`'s typed `FileDescriptorSet` drops custom-option bytes
// (§20) — harmless here since every fixture this helper compiles is option-free.
fn compile(name: &str) -> Vec<u8> {
    use protox::prost::Message;
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../keryx-core");
    protox::compile([name], [core.join("tests/fixtures"), core.join("proto")])
        .expect("fixture compiles")
        .encode_to_vec()
}

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)
}

#[test]
fn writes_facts_to_stdout() {
    let path = tmp("proto3.binpb");
    std::fs::write(&path, compile("proto3.proto")).unwrap();
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
fn unreadable_set_is_input_error() {
    let path = tmp("garbage.binpb");
    // A field-1 length prefix claiming far more bytes than follow — decode fails.
    std::fs::write(&path, b"\x0a\xff\xff\xff\x0f").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("schema-facts")
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty(), "no partial product on error");
    assert!(!out.stderr.is_empty(), "the diagnostic is on stderr");
}
