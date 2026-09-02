//! The `.proto` front door (architecture §5; spec §20, §31 M1): compile source files
//! to a descriptor set with protox — the pure-Rust compiler, no `protoc` — and ingest
//! it. The sole adapter over the `.proto` compiler; no `protox` type escapes this
//! module (the descriptor-engine boundary). Bytes are the seam: protox encodes the
//! resolved pool straight to a `FileDescriptorSet` and keryx decodes it through its own
//! prost-reflect in `ingest_subjects`, so the two crates' prost versions never couple.
//! The *subjects* are the explicitly-opened (root) files, carried across the seam by name
//! so a subject named like a well-known type (the §21.2 `descriptor.proto` self-application)
//! is ingested, not treated as a dependency. Editions gate (spec §31 M1): a file protox
//! cannot compile (editions is DEFERRED for protox 0.9.1, `docs/proto-support.md`) composes
//! a `UncompilableSource` diagnostic telling the caller to supply a descriptor set — a front-door
//! limit, not a translation one.

use std::path::Path;

use protox::Compiler;

use crate::descriptor::ingest_subjects;
use crate::descriptor::model::Schema;
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

/// Compile `files` (imports resolved against `includes` plus protox's bundled well-known
/// types, incl. `google/protobuf/descriptor.proto`) to a descriptor set and ingest it to a
/// [`Schema`], treating the opened files as the subjects. Built through
/// `encode_file_descriptor_set`, **not** `protox::compile` — the convenience re-encodes
/// options through prost-types' typed structs and drops keryx's custom-option bytes (the
/// §20 trap; see `tests/support/mod.rs`). Total (§6): any compile failure composes a
/// `UncompilableSource` diagnostic, never a panic.
///
/// # Errors
///
/// [`Diagnostics`] (`UncompilableSource`) when protox cannot compile the sources, or the
/// ingestion diagnostics when the resulting set does not ingest.
pub fn compile(
    files: &[impl AsRef<Path>],
    includes: &[impl AsRef<Path>],
) -> Result<Schema, Diagnostics> {
    let mut compiler =
        Compiler::new(includes).map_err(|error| source_error(&error, Locus::whole()))?;
    compiler.include_source_info(true).include_imports(true);
    for file in files {
        compiler
            .open_file(file)
            .map_err(|error| source_error(&error, Locus::at(file.as_ref().to_string_lossy())))?;
    }
    // The subjects are the opened (root) files: protox marks a file added by `open_file`
    // with `is_import() == false`, and one pulled in as a dependency with `true` (the field
    // is set that way in `Compiler::open_file`/`add_import`; note the accessor's own doc
    // comment is inverted, the behavior is not). Only the names — keryx-native `String`s —
    // cross to `ingest_subjects`; no protox type escapes this module.
    let subjects: Vec<String> = compiler
        .files()
        .filter(|file| !file.is_import())
        .map(|file| file.name().to_owned())
        .collect();
    ingest_subjects(&compiler.encode_file_descriptor_set(), &subjects)
}

/// Compose a protox compile error into a keryx `Diagnostic` (§6) at `locus`: the
/// compiler's message is preserved in the detail, its type never re-exported.
fn source_error(error: &protox::Error, locus: Locus) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::UncompilableSource,
        locus,
        format!("{error}"),
    ))
}
