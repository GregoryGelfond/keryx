//! The `.proto`-source door's confinement (the threat model's source-door confidentiality): keryx's
//! include-root resolver reads only within the include roots the operator grants. protox's own
//! import-name validation already rejects a traversing (`..`) or absolute import *name*
//! (`UncompilableSource`); keryx's resolver canonicalises the resolved path and refuses one that
//! escapes its root (`SourceOutsideRoot`), catching the **symlinked** escape protox does not — and
//! backstopping the rest. An in-root import resolves.
//!
//! (WKT/registry *shadowing* — a user file named like a well-known type *inside* a root — is a
//! namespace-precedence question, not an escape: the threat model's source door records why it is
//! closed for confidentiality — a shadow substitutes in-root content only — and is not tested here.)

use std::path::{Path, PathBuf};

use keryx_core::descriptor::compile;
use keryx_core::diagnostics::DiagnosticKind;

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// The kind a refused escape composes — protox's `UncompilableSource` (a rejected `..`/absolute import
/// name) or keryx's `SourceOutsideRoot` (a resolved path outside the root). Either means: refused, no
/// escape.
fn refusal_kind(root: &Path) -> DiagnosticKind {
    let diagnostics =
        compile(&["main.proto"], &[root]).expect_err("the escaping import is refused");
    let kind = diagnostics.iter().next().expect("one diagnostic").kind();
    assert!(
        matches!(
            kind,
            DiagnosticKind::UncompilableSource | DiagnosticKind::SourceOutsideRoot
        ),
        "an escaping import is refused, not admitted: {kind:?}"
    );
    kind
}

#[test]
fn an_in_root_import_resolves() {
    let root = scratch("confine_ok");
    write(
        &root.join("dep.proto"),
        "syntax=\"proto3\";package dep;message D{}",
    );
    write(
        &root.join("main.proto"),
        "syntax=\"proto3\";package main;import \"dep.proto\";message M{}",
    );
    compile(&["main.proto"], &[&root]).expect("an in-root import resolves and compiles");
}

#[test]
fn a_symlinked_escape_is_refused_by_keryx() {
    // The escape protox does not catch (a legitimate-looking import name resolving through a symlink
    // out of the root): keryx's canonicalise + root-check refuses it with `SourceOutsideRoot`.
    let base = scratch("confine_symlink");
    let root = base.join("root");
    let secret = base.join("secret.proto");
    write(&secret, "syntax=\"proto3\";package secret;message S{}");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&secret, root.join("link.proto")).unwrap();
    write(
        &root.join("main.proto"),
        "syntax=\"proto3\";package main;import \"link.proto\";message M{}",
    );
    assert_eq!(refusal_kind(&root), DiagnosticKind::SourceOutsideRoot);
}

#[test]
fn a_traversing_import_does_not_escape() {
    let base = scratch("confine_traverse");
    let root = base.join("root");
    write(
        &base.join("secret.proto"),
        "syntax=\"proto3\";package secret;message S{}",
    );
    write(
        &root.join("main.proto"),
        "syntax=\"proto3\";package main;import \"../secret.proto\";message M{}",
    );
    let _ = refusal_kind(&root); // refused (by protox's import-name validation, or keryx's root-check)
}

#[test]
fn an_absolute_import_does_not_escape() {
    let base = scratch("confine_absolute");
    let root = base.join("root");
    let secret = base.join("secret.proto");
    write(&secret, "syntax=\"proto3\";package secret;message S{}");
    write(
        &root.join("main.proto"),
        &format!(
            "syntax=\"proto3\";package main;import \"{}\";message M{{}}",
            secret.display()
        ),
    );
    let _ = refusal_kind(&root);
}
