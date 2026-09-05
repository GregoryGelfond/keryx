//! The keryx command-line frontend as a library — the command logic lives here so the suite can
//! name the exit contract ([`exit::Exit`]) rather than a raw process code (architecture §3: the
//! CLI is a satellite that composes the library; §6: stdout is the product, stderr is
//! diagnostics/progress, exit codes are stable and class-distinguishing). The public surface
//! (§25) is `gen` (schema → ASP vocabulary), `explain` (mapping verdicts), and `facts` (payload →
//! ground facts), with the internal `schema-facts` dump kept. The `keryx` binary is a shim over
//! [`run`].
//!
//! Design of record: `docs/design/architecture.md` (the architecture) over
//! `docs/specification.md` (the spec).
#![forbid(unsafe_code)]

pub mod exit;
pub mod render;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use keryx_core::codec::{Codec, PayloadFormat, Root};
use keryx_core::descriptor::{Schema, compile, ingest};
use keryx_core::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use keryx_core::policy::Mapping;
use keryx_core::{emit, manifest, policy, schema_facts};

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
    /// Shred a payload to ground facts over the schema's vocabulary (`.lp` on stdout, §11).
    Facts(FactsArgs),
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
struct FactsArgs {
    /// The root: `Type=payload` — the message type the payload is an instance of (a
    /// fully-qualified proto path, or a short name one message bears) and the payload file, its
    /// format named by its extension.
    #[arg(long, value_name = "TYPE=PAYLOAD")]
    root: String,
    /// The schema: a `.proto` source, or a serialized descriptor set (`.binpb`).
    spec: PathBuf,
    /// Include directories for import resolution (repeatable).
    #[arg(short = 'I', long = "include")]
    includes: Vec<PathBuf>,
}

#[derive(clap::Args)]
struct SchemaFactsArgs {
    /// A serialized `FileDescriptorSet`.
    set: PathBuf,
}

/// Parse the command line and run keryx, containing any escaped panic (§6). The `keryx` binary
/// is a shim over this; the exit class it returns is the process's exit code.
#[must_use]
pub fn run() -> Exit {
    let cli = Cli::parse();
    let json = cli.format.is_json();
    exit::contain(json, move || dispatch(cli))
}

fn dispatch(cli: Cli) -> Exit {
    match cli.command {
        Command::Gen(args) => generate(&args, cli.format),
        Command::Explain(args) => explain(&args, cli.format),
        Command::Facts(args) => facts(&args, cli.format),
        Command::SchemaFacts(args) => dump_schema_facts(&args, cli.format),
    }
}

/// Load, map, and write `<out>/<pkg>.core.lp`, `.views.lp`, and `.keryx-manifest` per package
/// (spec §13, §28). stdout stays clean; written paths are reported to stderr. The schema hash
/// is not computed at present (`-`); content hashing lands with `keryx diff` (Increment 5).
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
            // `unit.package()` is a validated `Package` (a dotted identifier — no `/`, `..`, or NUL),
            // so it names one file directly under `-o`, not a path that could traverse out of it (the
            // threat model's descriptor-door package boundary; the door represents that shape).
            let path = args
                .out
                .join(format!("{}.{suffix}", unit.package().as_str()));
            // A write failure (a bad `-o` directory, permissions, a full disk) is a file-I/O
            // error (§6 `Input`), not an internal bug.
            if let Err(error) = std::fs::write(&path, text) {
                return note(
                    format,
                    Exit::Input,
                    &format!("cannot write {}: {error}", path.display()),
                );
            }
            render::progress(&format!("wrote {}", path.display()));
        }
    }
    Exit::Success
}

/// Render the mapping verdicts to stdout (spec §21.3): per package, what each element became
/// (from the mapping model), plus a note for any recursive sort (§8). An optional `[fq.path]`
/// restricts the explanation to one element (§25). The product of `explain` is the explanation,
/// so it goes to stdout.
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
    // annotation prompt); that half lands with the annotation vocabulary (Increment 5).
    if let Some(path) = args.fq_path.as_deref() {
        return explain_element(&mapping, path, format);
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
    product(format, &out)
}

/// Render the verdict for the one schema element named by `path` — a sort, field, enum, or enum
/// value's fully-qualified proto path (§25's `[fq.path]`) — looked up on the mapping model
/// itself (not by re-tokenizing rendered text), so an enum value is addressable by its full name
/// like every other element. A path naming no element is a usage error (§6), not silence.
fn explain_element(mapping: &Mapping, path: &str, format: Format) -> Exit {
    let Some(element) = mapping.element(path) else {
        return note(
            format,
            Exit::Usage,
            &format!("no schema element has the proto path `{path}`"),
        );
    };
    product(format, &manifest::element_record(&element))
}

/// Shred one payload to its ground facts (spec §11, §25): the schema — `.proto` source or a
/// `.binpb` descriptor set, as `gen`/`explain` take it — built once into a [`Codec`], the payload
/// `--root Type=payload` names read and shredded as an instance of `Type` from the fresh root `r0`
/// (spec §4.1 item 6, the CLI's choice), and the `.lp` fact module written to stdout
/// (`keryx facts … | clingo`). The exit classes (§6): a `--root` not of the shape `Type=payload`,
/// a payload format keryx does not read, or a type the schema lacks are `Usage`; a file that
/// cannot be read is `Input`; a schema that builds no codec is `Schema`; a payload that does not
/// translate — undecodable, a §6 refusal, an enum number the schema lacks, nesting past the
/// ceiling — is `Translation`; a contained engine fault on either door is `Dependency`; a fact
/// themelios cannot spell is a keryx bug, `Internal`.
fn facts(args: &FactsArgs, format: Format) -> Exit {
    let (root_type, payload_path) = match parse_root(&args.root) {
        Ok(root) => root,
        Err(message) => return note(format, Exit::Usage, &message),
    };
    let Some(payload_format) = payload_format(payload_path) else {
        return note(
            format,
            Exit::Usage,
            &format!(
                "the payload `{}` has no extension naming a format keryx reads — {}",
                payload_path.display(),
                admitted_payload_formats()
            ),
        );
    };
    let codec = match load(
        std::slice::from_ref(&args.spec),
        &args.includes,
        format,
        Codec::new,
        Codec::from_source,
    ) {
        Ok(codec) => codec,
        Err(exit) => return exit,
    };
    let payload = match read(payload_path, format) {
        Ok(payload) => payload,
        Err(exit) => return exit,
    };
    match codec.shred(root_type, &payload, payload_format, &Root::fresh(0)) {
        Ok(facts) => match facts.render() {
            Ok(text) => product(format, &text),
            // The §6 policy refused every value the dialect cannot spell before it became a fact,
            // so an unspellable fact is a keryx bug, not the payload's translation error.
            Err(diagnostics) => report(format, Exit::Internal, &diagnostics),
        },
        Err(diagnostics) => {
            // The root type is the caller's own argument, so a name the schema lacks is a usage
            // error; every other refusal is the payload's — a translation error. Either is the
            // base class `classify` ranks beneath a contained engine fault (the dependency
            // boundary) — the one precedence every door keeps.
            let base = if diagnostics.contains_kind(DiagnosticKind::UnknownRootType) {
                Exit::Usage
            } else {
                Exit::Translation
            };
            report(format, Exit::classify(base, &diagnostics), &diagnostics)
        }
    }
}

/// Split a `--root Type=payload` argument (spec §25) into the root type and the payload path at
/// its first `=`: a proto type name never contains one, so the split is unambiguous, and the
/// payload path may contain one itself (a `date=2026-09-04/` partition directory). No `=`, or an
/// empty half, is a usage error, the shape stated.
fn parse_root(root: &str) -> Result<(&str, &Path), String> {
    let malformed = || {
        format!(
            "`--root` takes `Type=payload` — the message type and the payload file, joined by `=` — not `{root}`"
        )
    };
    let Some((root_type, payload)) = root.split_once('=') else {
        return Err(malformed());
    };
    if root_type.is_empty() || payload.is_empty() {
        return Err(malformed());
    }
    Ok((root_type, Path::new(payload)))
}

/// The payload formats keryx reads, by extension (spec §25 `payload.(binpb|json|txtpb)`): the
/// extension (matched ASCII-case-insensitively, as a spec's `.binpb` is), the codec's format, and
/// the description the usage note gives it. The one statement both [`payload_format`] and the note
/// naming the admitted formats derive from, so a format the codec comes to admit joins here alone.
const PAYLOAD_FORMATS: &[(&str, PayloadFormat, &str)] = &[
    ("binpb", PayloadFormat::Binary, "the binary wire format"),
    (
        "txtpb",
        PayloadFormat::Textproto,
        "the protobuf text format",
    ),
    ("json", PayloadFormat::Json, "the protobuf JSON mapping"),
];

/// The wire form a payload file is in, by its extension — its entry in [`PAYLOAD_FORMATS`]. A
/// payload in no form the codec reads (no extension, or one the table lacks) is `None`: a usage
/// error at the command, decided before the file is read.
fn payload_format(payload: &Path) -> Option<PayloadFormat> {
    let extension = payload.extension()?.to_str()?;
    PAYLOAD_FORMATS
        .iter()
        .find(|(admitted, _, _)| admitted.eq_ignore_ascii_case(extension))
        .map(|(_, format, _)| *format)
}

/// The admitted formats as the usage note names them — "`.binpb` (the binary wire format)", one
/// per [`PAYLOAD_FORMATS`] entry, comma-separated.
fn admitted_payload_formats() -> String {
    PAYLOAD_FORMATS
        .iter()
        .map(|(extension, _, description)| format!("`.{extension}` ({description})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Read a serialized `FileDescriptorSet` and dump its descriptor facts (internal; Increment
/// 1's deliverable, kept). stdout = facts, stderr = diagnostics.
fn dump_schema_facts(args: &SchemaFactsArgs, format: Format) -> Exit {
    let bytes = match read(&args.set, format) {
        Ok(bytes) => bytes,
        Err(exit) => return exit,
    };
    let schema = match ingest(&bytes) {
        Ok(schema) => schema,
        Err(diagnostics) => {
            return report(
                format,
                Exit::classify(Exit::Schema, &diagnostics),
                &diagnostics,
            );
        }
    };
    match schema_facts::render(&schema) {
        Ok(text) => product(format, &text),
        // This render is interior — over a constructed schema, not foreign input — so no contained
        // dependency fault can arise here (unlike the door above, routed through `Exit::classify`).
        // A non-identifier option key on a crafted set is a schema-input error (§6); a themelios
        // spell failure on constructed output is a keryx bug. Class by the kind the render produced.
        Err(diagnostics) => {
            let class = if diagnostics.contains_kind(DiagnosticKind::UnmappableOptionKey) {
                Exit::Schema
            } else {
                Exit::Internal
            };
            report(format, class, &diagnostics)
        }
    }
}

/// A `.binpb` spec is a serialized descriptor set; any other extension is `.proto` source.
fn is_descriptor_set(spec: &Path) -> bool {
    spec.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("binpb"))
}

/// Read a file at the CLI boundary: its bytes, or the `Input` class rendered (§6 — a file that
/// cannot be read, distinct from one that reads but does not parse).
fn read(path: &Path, format: Format) -> Result<Vec<u8>, Exit> {
    std::fs::read(path).map_err(|error| {
        note(
            format,
            Exit::Input,
            &format!("cannot read {}: {error}", path.display()),
        )
    })
}

/// Load the schema for `gen`/`explain` from a spec set: [`load`] through the descriptor doors.
fn load_schema(specs: &[PathBuf], includes: &[PathBuf], format: Format) -> Result<Schema, Exit> {
    load(specs, includes, format, ingest, compile)
}

/// Load what a command derives from its spec set (spec §25 `<spec.proto|spec.binpb>`) through
/// the two doors: a lone `.binpb` spec is read and handed to `from_set` (a serialized descriptor
/// set, as `schema-facts` takes); any other spec set is `.proto` source handed to `from_source`
/// (compiled through the front door). The doors are the schema's (`ingest`/`compile`) for
/// `gen`/`explain` and the codec's for `facts`, classified alike. Renders any diagnostic itself
/// and returns the classified [`Exit`]: a `.binpb` read failure is `Input`; a door failure is
/// `Schema` — a contained engine fault `Dependency` — and a compile failure carries the
/// descriptor-set hint.
fn load<T>(
    specs: &[PathBuf],
    includes: &[PathBuf],
    format: Format,
    from_set: impl FnOnce(&[u8]) -> Result<T, Diagnostics>,
    from_source: impl FnOnce(&[PathBuf], &[PathBuf]) -> Result<T, Diagnostics>,
) -> Result<T, Exit> {
    if let [spec] = specs
        && is_descriptor_set(spec)
    {
        let bytes = read(spec, format)?;
        return from_set(&bytes).map_err(|diagnostics| {
            report(
                format,
                Exit::classify(Exit::Schema, &diagnostics),
                &diagnostics,
            )
        });
    }
    from_source(specs, includes).map_err(|diagnostics| {
        let hinted = with_descriptor_set_hint(diagnostics);
        report(format, Exit::classify(Exit::Schema, &hinted), &hinted)
    })
}

/// A front-door compile failure carries a fix-it hint (architecture §6: fix-it hints travel
/// with the error): a source protox cannot parse but protoc can may be compiled to a descriptor
/// set and supplied here as a `.binpb`, which `gen`/`explain` accept. Editions are the exception —
/// the descriptor engine cannot read them either (`UnsupportedEdition`), so the hint says so
/// rather than promising a route that also fails. Appended as a further diagnostic so it renders
/// in both the human and JSON forms; a non-`UncompilableSource` failure is returned unchanged.
fn with_descriptor_set_hint(mut diagnostics: Diagnostics) -> Diagnostics {
    if diagnostics.contains_kind(DiagnosticKind::UncompilableSource) {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::UncompilableSource,
            Locus::whole(),
            "if protoc can compile this source, supply the descriptor set instead — keryx accepts a <spec>.binpb (editions are not yet supported by either route; see docs/proto-support.md)",
        ));
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use keryx_core::codec::PayloadFormat;

    use super::{PAYLOAD_FORMATS, admitted_payload_formats, parse_root, payload_format};

    #[test]
    fn a_root_is_a_type_and_a_payload_split_at_the_first_equals() {
        // `--root Type=payload`: split at the first `=`, the payload a path as given — a further
        // `=` is the path's own, since a type name never carries one. No `=`, or an empty half,
        // is malformed, the shape stated.
        assert_eq!(
            parse_root("ReadingBatch=batch.binpb").expect("well-formed"),
            ("ReadingBatch", Path::new("batch.binpb"))
        );
        assert_eq!(
            parse_root("a=b=c").expect("well-formed"),
            ("a", Path::new("b=c"))
        );
        for malformed in ["ReadingBatch", "=batch.binpb", "ReadingBatch="] {
            let message = parse_root(malformed).expect_err("malformed");
            assert!(message.contains("Type=payload"), "{message}");
        }
    }

    #[test]
    fn a_payload_format_is_named_by_its_extension() {
        // `.binpb`, in any case, is the binary wire format, `.txtpb` the text format, and `.json`
        // the JSON mapping; any other extension, or none, is a form the codec does not read. The
        // table is the one source: every entry resolves to its format, and the usage note names
        // every entry's extension.
        assert_eq!(
            payload_format(Path::new("batch.binpb")),
            Some(PayloadFormat::Binary)
        );
        assert_eq!(
            payload_format(Path::new("batch.BINPB")),
            Some(PayloadFormat::Binary)
        );
        assert_eq!(
            payload_format(Path::new("batch.txtpb")),
            Some(PayloadFormat::Textproto)
        );
        assert_eq!(
            payload_format(Path::new("batch.TXTPB")),
            Some(PayloadFormat::Textproto)
        );
        assert_eq!(
            payload_format(Path::new("batch.json")),
            Some(PayloadFormat::Json)
        );
        assert_eq!(
            payload_format(Path::new("batch.JSON")),
            Some(PayloadFormat::Json)
        );
        assert_eq!(payload_format(Path::new("batch.yaml")), None);
        assert_eq!(payload_format(Path::new("batch")), None);
        let admitted = admitted_payload_formats();
        for (extension, format, _) in PAYLOAD_FORMATS {
            assert_eq!(
                payload_format(&Path::new("payload").with_extension(extension)),
                Some(*format)
            );
            assert!(admitted.contains(&format!("`.{extension}`")), "{admitted}");
        }
    }
}
