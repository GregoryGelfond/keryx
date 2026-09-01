//! The keryx command-line frontend — a thin satellite that composes the library
//! (architecture §3, §6): stdout is the product, stderr is diagnostics/progress, exit codes
//! are stable and class-distinguishing. The public surface (§25) — `gen` (schema → ASP
//! vocabulary) and `explain` (mapping verdicts) — with the internal `schema-facts` dump
//! kept. A top-level panic hook maps any escaped panic to a clean internal error (§6).
#![forbid(unsafe_code)]

mod exit;
mod render;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use keryx_core::descriptor::{Schema, compile, ingest};
use keryx_core::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use keryx_core::policy::Mapping;
use keryx_core::{emit, facts, manifest, policy};

use crate::exit::Exit;
use crate::render::{Format, note, product, report};

/// keryx — a bidirectional bridge between Protocol Buffers and Answer Set Programming.
#[derive(Parser)]
#[command(name = "keryx", version, about)]
struct Cli {
    /// Diagnostic format on stderr.
    #[arg(long, global = true, value_enum, default_value_t = Format::Auto)]
    format: Format,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate ASP vocabulary (`core.lp`, `views.lp`, manifest) from a schema.
    Gen(GenArgs),
    /// Explain the mapping — what each schema element becomes, and why (§21.3).
    Explain(ExplainArgs),
    /// Dump a descriptor set's stage-0 descriptor facts (internal).
    SchemaFacts(SchemaFactsArgs),
}

#[derive(clap::Args)]
struct GenArgs {
    /// Schema sources: `.proto` files, or a single serialized descriptor set (`.binpb`).
    #[arg(required = true)]
    protos: Vec<PathBuf>,
    /// Include directories for import resolution (repeatable).
    #[arg(short = 'I', long = "include")]
    includes: Vec<PathBuf>,
    /// Output directory for the generated files.
    #[arg(short, long, default_value = ".")]
    out: PathBuf,
}

#[derive(clap::Args)]
struct ExplainArgs {
    /// The schema: a `.proto` source, or a serialized descriptor set (`.binpb`).
    spec: PathBuf,
    /// Include directories for import resolution (repeatable).
    #[arg(short = 'I', long = "include")]
    includes: Vec<PathBuf>,
    /// Restrict the explanation to one element, by its fully-qualified proto path.
    fq_path: Option<String>,
}

#[derive(clap::Args)]
struct SchemaFactsArgs {
    /// A serialized `FileDescriptorSet`.
    set: PathBuf,
}

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|info| eprintln!("keryx: internal error: {info}")));
    let outcome = std::panic::catch_unwind(|| run(Cli::parse()));
    match outcome {
        Ok(exit) => exit.into(),
        Err(_) => Exit::Internal.into(), // the hook printed; map the panic to a clean exit 1
    }
}

fn run(cli: Cli) -> Exit {
    // A debug-only affordance to exercise the §6 top-level panic hook + `catch_unwind` from an
    // integration test (absent from release builds); never on any real command path. A
    // deliberate trigger, not an invariant assertion — so `manual_assert` does not apply.
    #[cfg(debug_assertions)]
    #[allow(clippy::manual_assert)]
    if std::env::var_os("KERYX_INTERNAL_PANIC").is_some() {
        panic!("deliberate panic to exercise the top-level hook");
    }
    match cli.command {
        Command::Gen(args) => generate(&args, cli.format),
        Command::Explain(args) => explain(&args, cli.format),
        Command::SchemaFacts(args) => schema_facts(&args, cli.format),
    }
}

/// Load, map, and write `<out>/<pkg>.core.lp`, `.views.lp`, and `.keryx-manifest` per package
/// (spec §13, §28). stdout stays clean; written paths are reported to stderr. The schema hash
/// is not computed at M1 (`-`); content hashing lands with `keryx diff` (Increment 5).
fn generate(args: &GenArgs, format: Format) -> Exit {
    let schema = match load_schema(&args.protos, &args.includes, format) {
        Ok(schema) => schema,
        Err(exit) => return exit,
    };
    let mapping = match policy::map(&schema) {
        Ok(mapping) => mapping,
        Err(diagnostics) => return report(format, Exit::Schema, &diagnostics),
    };
    for unit in mapping.units() {
        // keryx generates one file set per package (§13); a package-less source would write
        // hidden dotfiles (`.core.lp`, …). Reject it with a clear cause rather than silently.
        if unit.package().is_empty() {
            return note(
                Exit::Schema,
                "a package-less .proto is not supported — declare a `package` (keryx generates one file set per package, §13)",
            );
        }
        let core = match emit::core(unit) {
            Ok(text) => text,
            Err(diagnostics) => return report(format, Exit::Internal, &diagnostics),
        };
        let views = match emit::views(unit) {
            Ok(text) => text,
            Err(diagnostics) => return report(format, Exit::Internal, &diagnostics),
        };
        let manifest = manifest::write(unit, "-");
        for (suffix, text) in [
            ("core.lp", &core),
            ("views.lp", &views),
            ("keryx-manifest", &manifest),
        ] {
            let path = args.out.join(format!("{}.{suffix}", unit.package()));
            // A write failure (a bad `-o` directory, permissions, a full disk) is a file-I/O
            // error (§6 `Input`), not an internal bug.
            if let Err(error) = std::fs::write(&path, text) {
                return note(
                    Exit::Input,
                    &format!("cannot write {}: {error}", path.display()),
                );
            }
            eprintln!("keryx: wrote {}", path.display());
        }
    }
    Exit::Success
}

/// Render the mapping verdicts to stdout (spec §21.3): per package, the manifest *records*
/// (without the §13.4 header — an explanation is not the evolution contract) — what each
/// element became — plus a note for any recursive sort (§8). An optional `[fq.path]` restricts
/// the explanation to one element (§25). The product of `explain` is the explanation, so it
/// goes to stdout.
fn explain(args: &ExplainArgs, format: Format) -> Exit {
    let schema = match load_schema(std::slice::from_ref(&args.spec), &args.includes, format) {
        Ok(schema) => schema,
        Err(exit) => return exit,
    };
    let mapping = match policy::map(&schema) {
        Ok(mapping) => mapping,
        Err(diagnostics) => return report(format, Exit::Schema, &diagnostics),
    };
    // §25's `explain` also proposes a scalar treatment for an un-annotated field (the
    // annotation prompt); that half lands with the codec (Increment 5).
    if let Some(path) = args.fq_path.as_deref() {
        return explain_element(&mapping, path);
    }
    let mut out = String::new();
    for unit in mapping.units() {
        out.push_str(&manifest::records(unit));
        for sort in unit.sorts().iter().filter(|s| s.is_recursive()) {
            let _ = writeln!(
                out,
                "note: {} participates in a containment cycle — consider reified individuals (§8)",
                sort.proto().as_str(),
            );
        }
    }
    product(&out)
}

/// Render the verdict for the one schema element named by `path` — a field, sort, or enum's
/// fully-qualified proto path (§25's `[fq.path]`) — reusing the manifest's record vocabulary
/// (the first whitespace-delimited token of a record line is the element's proto path). A path
/// naming no element is a usage error — the argument resolves to nothing — not silence.
fn explain_element(mapping: &Mapping, path: &str) -> Exit {
    let mut out = String::new();
    for unit in mapping.units() {
        let records = manifest::records(unit);
        for line in records.lines() {
            if line.split_whitespace().next() == Some(path) {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    if out.is_empty() {
        return note(
            Exit::Usage,
            &format!("no schema element has the proto path `{path}`"),
        );
    }
    product(&out)
}

/// Read a serialized `FileDescriptorSet` and dump its descriptor facts (internal; Increment
/// 1's deliverable, kept). stdout = facts, stderr = diagnostics.
fn schema_facts(args: &SchemaFactsArgs, format: Format) -> Exit {
    let bytes = match std::fs::read(&args.set) {
        Ok(bytes) => bytes,
        Err(error) => {
            return note(
                Exit::Input,
                &format!("cannot read {}: {error}", args.set.display()),
            );
        }
    };
    let schema = match ingest(&bytes) {
        Ok(schema) => schema,
        Err(diagnostics) => return report(format, Exit::Schema, &diagnostics),
    };
    match facts::render(&schema) {
        Ok(text) => product(&text),
        Err(diagnostics) => report(format, Exit::Internal, &diagnostics),
    }
}

/// A `.binpb` spec is a serialized descriptor set; any other extension is `.proto` source.
fn is_descriptor_set(spec: &Path) -> bool {
    spec.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("binpb"))
}

/// Load the schema for `gen`/`explain` from a spec set (spec §25 `<spec.proto|spec.binpb>`): a
/// lone `.binpb` spec is read and `ingest`ed (a serialized descriptor set, as `schema-facts`
/// takes); any other spec set is `.proto` source compiled through the front door. Renders any
/// diagnostic itself and returns the classified [`Exit`] — a `.binpb` read failure is `Input`,
/// an ingest or compile failure is `Schema`.
fn load_schema(specs: &[PathBuf], includes: &[PathBuf], format: Format) -> Result<Schema, Exit> {
    if let [spec] = specs
        && is_descriptor_set(spec)
    {
        let bytes = std::fs::read(spec).map_err(|error| {
            note(
                Exit::Input,
                &format!("cannot read {}: {error}", spec.display()),
            )
        })?;
        return ingest(&bytes).map_err(|diagnostics| report(format, Exit::Schema, &diagnostics));
    }
    compile(specs, includes)
        .map_err(|diagnostics| report(format, Exit::Schema, &with_descriptor_set_hint(diagnostics)))
}

/// A front-door compile failure carries a fix-it hint (architecture §6: fix-it hints travel
/// with the error): a source keryx cannot compile — e.g. a Protobuf edition, which protox does
/// not yet cover (spec §31) — can be compiled to a descriptor set and supplied here as a
/// `.binpb`, which `gen`/`explain` now accept. Appended as a further diagnostic so it renders
/// in both the human and JSON forms; a non-`SourceCompile` failure is returned unchanged.
fn with_descriptor_set_hint(mut diagnostics: Diagnostics) -> Diagnostics {
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind() == DiagnosticKind::SourceCompile)
    {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::SourceCompile,
            Locus::whole(),
            "compile this source to a descriptor set and supply that instead — keryx accepts a <spec>.binpb",
        ));
    }
    diagnostics
}
