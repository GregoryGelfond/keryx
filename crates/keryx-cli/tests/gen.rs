//! `keryx gen` end to end (spec §25; architecture §6): a schema in — `.proto` source or a
//! serialized `.binpb` descriptor set — the per-package `core.lp`/`views.lp`/`.keryx-manifest`
//! written to `-o DIR`, stdout quiet, diagnostics on stderr, and the §6 exit taxonomy. A
//! source that cannot compile is a `Schema`(4) error carrying a `.binpb` fix-it hint; a bad
//! `-o` is an `Input`(3) write error; a package-less source is rejected. No protox type is in
//! reach here — `gen` composes the library (the fixtures are compiled in `support`).

use keryx_test_support as support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{fixtures, vendored};

// A fresh output directory under the target tmp dir, so parallel tests never collide.
fn out_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)
}

#[test]
fn writes_the_three_files_per_package() {
    let out = out_dir("gen_proto3");
    let status = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("gen")
        .arg("proto3.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-I".as_ref(), vendored().as_os_str()])
        .args(["-o".as_ref(), out.as_os_str()])
        .output()
        .unwrap();
    assert!(status.status.success(), "gen exits 0 on a good source");
    assert!(
        status.stdout.is_empty(),
        "stdout stays clean; the product is the files"
    );

    let core = std::fs::read_to_string(out.join("keryx.p3.core.lp")).expect("core.lp written");
    let views = std::fs::read_to_string(out.join("keryx.p3.views.lp")).expect("views.lp written");
    let manifest =
        std::fs::read_to_string(out.join("keryx.p3.keryx-manifest")).expect("manifest written");

    // The §13.1 honorary signature rides as a `%!` doc on the sort's `#defined`.
    assert!(
        core.contains("#defined reading/1."),
        "core declares the sort"
    );
    assert!(
        core.contains("%!sort reading/1"),
        "core carries the sort signature"
    );
    // The §13.2 relational view for the singular message field.
    assert!(
        views.contains("detail(P, A) :- detail(A), A = detail(P)."),
        "views carries the singular-message view"
    );
    assert!(
        manifest.starts_with("keryx-manifest v0\n"),
        "the manifest v0 header"
    );
}

#[test]
fn accepts_a_binpb_descriptor_set() {
    // §25 gives the arg as `<spec.proto|spec.binpb>`: a serialized descriptor set is read and
    // ingested, not fed to protox. This is what the editions fix-it hint points the user to.
    let set = tmp("gen_proto3.binpb");
    std::fs::write(&set, support::compile_fixture("proto3.proto")).unwrap();
    let out = out_dir("gen_binpb");
    let status = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("gen")
        .arg(&set)
        .args(["-o".as_ref(), out.as_os_str()])
        .output()
        .unwrap();
    assert!(status.status.success(), "gen ingests a .binpb set");
    let core = std::fs::read_to_string(out.join("keryx.p3.core.lp")).expect("core.lp written");
    assert!(core.contains("#defined reading/1."), "the same vocabulary");
}

#[test]
fn a_missing_source_is_a_schema_error_with_a_fix_it_hint() {
    let out = out_dir("gen_missing");
    let status = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("gen")
        .arg("no_such_file.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-o".as_ref(), out.as_os_str()])
        .output()
        .unwrap();
    // A source the front door cannot compile is a schema error (§6), not an input one.
    assert_eq!(status.status.code(), Some(4));
    assert!(status.stdout.is_empty(), "no partial product on error");
    let stderr = String::from_utf8(status.stderr).unwrap();
    assert!(
        stderr.contains("uncompilable_source"),
        "the diagnostic names the front-door compile failure: {stderr}"
    );
    // The fix-it hint travels with the error (§6): supply a descriptor set instead.
    assert!(
        stderr.contains(".binpb"),
        "a descriptor-set fix-it hint is present: {stderr}"
    );
}

#[test]
fn the_error_format_is_json_on_request() {
    let out = out_dir("gen_missing_json");
    let status = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .args(["--format", "json"])
        .arg("gen")
        .arg("no_such_file.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-o".as_ref(), out.as_os_str()])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(4));
    let stderr = String::from_utf8(status.stderr).unwrap();
    let stderr = stderr.trim();
    // Appendix B key order: field_path, kind, detail.
    assert!(
        stderr.starts_with(r#"[{"field_path":"#),
        "stderr is a structured JSON array (Appendix B): {stderr}"
    );
    assert!(
        stderr.contains(r#""kind":"uncompilable_source""#),
        "the uncompilable_source kind is structured: {stderr}"
    );
    assert!(stderr.ends_with("}]"), "a closed JSON array: {stderr}");
}

#[test]
fn the_error_format_is_human_on_request() {
    let out = out_dir("gen_missing_human");
    let status = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .args(["--format", "human"])
        .arg("gen")
        .arg("no_such_file.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-o".as_ref(), out.as_os_str()])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(4));
    let stderr = String::from_utf8(status.stderr).unwrap();
    // Human prose is the library `Display`: `keryx: uncompilable_source at <locus>: <detail>`.
    assert!(
        stderr.contains("keryx: uncompilable_source at "),
        "human prose, not JSON: {stderr}"
    );
}

#[test]
fn rejects_a_package_less_source() {
    // keryx generates one file set per package (§13); a package-less source would write hidden
    // dotfiles. It is rejected with a clear cause instead.
    let dir = out_dir("gen_nopkg_src");
    std::fs::write(
        dir.join("bare.proto"),
        "syntax = \"proto3\";\nmessage Bare { string x = 1; }\n",
    )
    .unwrap();
    let out = out_dir("gen_nopkg_out");
    let status = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("gen")
        .arg("bare.proto")
        .args(["-I".as_ref(), dir.as_os_str()])
        .args(["-o".as_ref(), out.as_os_str()])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(4), "a schema error");
    let stderr = String::from_utf8(status.stderr).unwrap();
    assert!(
        stderr.contains("package-less"),
        "the cause is named: {stderr}"
    );
    assert!(
        !out.join(".core.lp").exists(),
        "no hidden dotfile is written"
    );
}

#[test]
fn a_write_failure_is_an_input_error() {
    // A bad `-o` directory (its parent does not exist) is a file-I/O error (§6 `Input`), not
    // an internal bug.
    let out = tmp("gen_writefail_nonexistent/deeper");
    let status = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("gen")
        .arg("proto3.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-o".as_ref(), out.as_os_str()])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(3), "a file-I/O (Input) error");
    let stderr = String::from_utf8(status.stderr).unwrap();
    assert!(
        stderr.contains("cannot write"),
        "the write failure is named: {stderr}"
    );
}
