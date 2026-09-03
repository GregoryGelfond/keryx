//! `keryx explain` end to end (spec §21.3, §25; architecture §6): a schema in, the mapping
//! verdicts on stdout — the manifest *records* (no §13.4 header: an explanation is not the
//! evolution contract) plus a §8 note for any recursive sort — and an optional `[fq.path]`
//! that restricts the explanation to one element. The explanation is the product, so it is on
//! stdout. Reuses keryx-core's fixtures off the include path.

use keryx_test_support as support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{fixtures, vendored};

// A fresh dir under the target tmp dir (parallel-safe).
fn tmp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn rejects_a_package_less_source() {
    // The package-less refusal lives in the library (`policy::map`), so `explain` refuses it too —
    // not only `gen`. A package-less file has no valid file set to name, so its mapping is not
    // shown; the cause is named at the file's locus.
    let dir = tmp_dir("explain_nopkg_src");
    std::fs::write(
        dir.join("bare.proto"),
        "syntax = \"proto3\";\nmessage Bare { string x = 1; }\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("explain")
        .arg("bare.proto")
        .args(["-I".as_ref(), dir.as_os_str()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4), "a schema error");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("package-less"),
        "the cause is named: {stderr}"
    );
    assert!(
        out.stdout.is_empty(),
        "no verdicts are printed for a rejected schema"
    );
}

#[test]
fn renders_the_records_without_the_manifest_header() {
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("explain")
        .arg("proto3.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-I".as_ref(), vendored().as_os_str()])
        .output()
        .unwrap();
    assert!(out.status.success(), "explain exits 0 on a good source");
    let stdout = String::from_utf8(out.stdout).unwrap();
    // A verdict line — what `Reading` became — but NOT the §13.4 manifest header (which would
    // carry a misleading null `schema-hash -`).
    assert!(
        stdout.contains("keryx.p3.Reading  sort  reading/1"),
        "the Reading sort verdict: {stdout}"
    );
    assert!(
        !stdout.contains("keryx-manifest v0"),
        "no manifest header in an explanation: {stdout}"
    );
    assert!(
        !stdout.contains("schema-hash"),
        "no null schema hash in an explanation: {stdout}"
    );
    assert!(out.stderr.is_empty(), "stderr is quiet on success");
}

#[test]
fn notes_a_recursive_sort() {
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("explain")
        .arg("recursion.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .output()
        .unwrap();
    assert!(out.status.success(), "explain exits 0");
    let stdout = String::from_utf8(out.stdout).unwrap();
    // The §8 note fires for a sort in a containment cycle (recursion.proto's `Tree`).
    assert!(
        stdout.contains("keryx.rec.Tree participates in a containment cycle"),
        "the recursive sort carries the §8 note: {stdout}"
    );
}

#[test]
fn a_fq_path_restricts_the_explanation_to_one_element() {
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("explain")
        .arg("proto3.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-I".as_ref(), vendored().as_os_str()])
        .arg("keryx.p3.Reading.sensor")
        .output()
        .unwrap();
    assert!(out.status.success(), "explain exits 0 for a matching path");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("keryx.p3.Reading.sensor #1 fn  sensor/2  string  total"),
        "the one field's verdict: {stdout}"
    );
    // Only that element — a sibling field and the sort line are absent.
    assert!(
        !stdout.contains("temp_c"),
        "the filter excludes other fields: {stdout}"
    );
    assert!(
        !stdout.contains("  sort  "),
        "the filter excludes the sort line: {stdout}"
    );
}

#[test]
fn an_enum_value_is_addressable_by_its_full_path() {
    // §25: an enum value's manifest line begins with its short name (`LEVEL_LOW  #1  value
    // low`), so a text search for its fully-qualified path found nothing. The element is now
    // looked up on the mapping model, so `pkg.Enum.VALUE` addresses it like any other element.
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("explain")
        .arg("proto3.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-I".as_ref(), vendored().as_os_str()])
        .arg("keryx.p3.Level.LEVEL_LOW")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "explain exits 0 for a matching enum value"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("LEVEL_LOW  #1  value  low"),
        "the one enum value's verdict: {stdout}"
    );
    // Only that value — its siblings and the enum line itself are absent.
    assert!(
        !stdout.contains("LEVEL_HIGH") && !stdout.contains("  enum  "),
        "the filter excludes sibling values and the enum line: {stdout}"
    );
}

#[test]
fn a_message_path_explains_the_sort_and_its_fields() {
    // A message path names the sort; its record is the `sort` line and every field of it (the
    // full per-message record), distinct from a single field path (which is just that field).
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("explain")
        .arg("proto3.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-I".as_ref(), vendored().as_os_str()])
        .arg("keryx.p3.Detail")
        .output()
        .unwrap();
    assert!(out.status.success(), "explain exits 0 for a message path");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("keryx.p3.Detail  sort  detail/1")
            && stdout.contains("keryx.p3.Detail.note #1 fn  note/2  string  total"),
        "the sort and its field: {stdout}"
    );
    // Not a different message's record.
    assert!(
        !stdout.contains("keryx.p3.Reading"),
        "the filter excludes other messages: {stdout}"
    );
}

#[test]
fn an_unknown_fq_path_is_a_usage_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_keryx"))
        .arg("explain")
        .arg("proto3.proto")
        .args(["-I".as_ref(), fixtures().as_os_str()])
        .args(["-I".as_ref(), vendored().as_os_str()])
        .arg("keryx.p3.Nope.missing")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "a path naming nothing is usage");
    assert!(out.stdout.is_empty(), "no product for a bad path");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("no schema element has the proto path"),
        "the cause is named: {stderr}"
    );
}
