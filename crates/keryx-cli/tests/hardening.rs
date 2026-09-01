//! The §6 hardening paths, proven adversarially rather than only asserted in the happy path.
//! The broken-pipe and JSON-escaping arms are unit-tested in `src/render.rs` (deterministic,
//! no real pipe); the top-level panic hook is exercised here — any escaped panic must become a
//! clean `internal error` on stderr and exit 1, never a raw backtrace. The trigger is a
//! debug-only affordance in `run`, so this test builds only under `debug_assertions` (the dev
//! profile the gate uses); a release build carries neither the trigger nor this test.

#![cfg(debug_assertions)]

use std::process::Command;

#[test]
fn an_escaped_panic_is_a_clean_internal_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .env("KERYX_INTERNAL_PANIC", "1")
        .env_remove("RUST_BACKTRACE")
        .arg("explain")
        .arg("unused.proto") // parsing must succeed; the panic fires before any work
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "an escaped panic maps to exit 1"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("keryx: internal error:"),
        "a clean internal-error report on stderr: {stderr}"
    );
    assert!(
        !stderr.contains("stack backtrace"),
        "no raw backtrace leaks: {stderr}"
    );
    assert!(out.stdout.is_empty(), "no product on a panic");
}
