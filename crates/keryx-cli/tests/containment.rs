//! The dependency-fault path end to end (§6, the dependency boundary): a descriptor set that drives
//! a real contained engine fault reaches the CLI as its `DependencyFault` diagnostic and exit
//! `Dependency` (7) — **once**, with no false "bug in keryx" line and no raw `panicked at …`. This
//! holds the wiring the `exit::hook_report` unit tests cannot reach: the panic hook consulting the
//! live `keryx_core::is_containing()` flag during an actual contained panic.

use std::path::{Path, PathBuf};
use std::process::Command;

use keryx_test_support as support;

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)
}

#[test]
fn a_contained_engine_fault_exits_dependency_with_no_bug_notice() {
    let path = tmp("fault.binpb");
    std::fs::write(&path, support::decode_fault_set()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("schema-facts")
        .arg(&path)
        // The "no raw panic line by default" claim is about the process's *default* hook; clear the
        // runner's `RUST_BACKTRACE` so the subprocess sees that default, not the shell's (with it set,
        // the hook prints the fault's location and this assertion, unrelated to it, would fail).
        .env_remove("RUST_BACKTRACE")
        .output()
        .unwrap();

    // A contained dependency fault classifies `Dependency` (exit 7) — neither a keryx bug (`Internal`,
    // 1) nor a user's schema error (`Schema`, 4).
    assert_eq!(out.status.code(), Some(7), "exit Dependency");
    assert!(out.stdout.is_empty(), "no partial product on stdout");

    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("dependency_fault"),
        "the diagnostic is rendered: {stderr}"
    );
    // The reconciliation: the panic hook stayed silent (it consulted `is_containing()`), so there is
    // no false keryx-bug notice and no raw panic line beside the diagnostic — one report, not two.
    assert!(
        !stderr.contains("bug in keryx"),
        "no false keryx-bug notice: {stderr}"
    );
    assert!(
        !stderr.contains("panicked at"),
        "no raw panic line: {stderr}"
    );
}
