//! The `.proto` front door (architecture §5; spec §20, §31): compile source files to a descriptor
//! set with protox — the pure-Rust compiler, no `protoc` — and ingest it. The sole adapter over the
//! `.proto` compiler; no `protox` type escapes this module (the descriptor-engine boundary). Bytes are
//! the seam: protox encodes the resolved pool straight to a `FileDescriptorSet` and keryx decodes it
//! through its own prost-reflect in `ingest_subjects`, so the two crates' prost versions never couple.
//! The *subjects* are the explicitly-opened (root) files, carried across the seam by name so a subject
//! named like a well-known type (the §21.2 `descriptor.proto` self-application) is ingested, not
//! treated as a dependency.
//!
//! Three source-door hardenings live at the one seam where a `.proto` file is located, read, and
//! handed to the parser — keryx's own include-root resolver (`RootedResolver`):
//!
//! - **Bounded nesting (the source-door twin of the editions pre-read).** protox's recursive-descent
//!   parser is unbounded and *aborts* the process (an uncatchable stack overflow) on deeply-nested
//!   source, and prost-reflect's option-value text-format parser is unbounded too. keryx cannot bound
//!   either, so it scans each file's *lexical* bracket-nesting (`max_nesting_depth`) — a total,
//!   **dominant over-approximation** of both recursions — and refuses source past
//!   `SOURCE_NESTING_LIMIT` with a clean `SourceTooDeep` **before** protox parses it. This scan is a
//!   **bounded departure from R5** ("keryx parses no protobuf"): it reads lexical structure — brackets,
//!   strings, comments — but builds no tree and assigns no meaning, a *measure*, not a parse (named in
//!   the threat model's property 3 and architecture R5).
//! - **Bounded import graph.** protox resolves imports by recursion as well (`add_import` →
//!   `add_import`), a second abort axis the per-file brace scan cannot see; keryx's resolver counts the
//!   files it opens and refuses an import graph past `MAX_IMPORT_FILES` (`SourceImportGraphTooLarge`)
//!   before that recursion overflows.
//! - **Confinement.** protox rejects a `..` or absolute import *name* itself (`UncompilableSource`), but
//!   its `IncludeFileResolver` does not confine a resolved *path* — a **symlinked** import escapes the
//!   root. keryx's resolver canonicalises the resolved path and refuses one outside its root
//!   (`SourceOutsideRoot`), catching that escape and backstopping the rest. (A user file named like a
//!   well-known type *inside* a root still shadows it by chain precedence — a namespace-precedence
//!   question, not an escape.)
//!
//! Each bound is defense-in-depth — a sub-standard thread stack can abort below it, closed by the
//! consuming service's process isolation (the threat model's division of labor) — and interim, retired
//! when protox bounds its own recursion. An unforeseen protox *panic* (a catchable one — e.g. its own
//! `RecursionLimitReached` unwrap at moderate depth) is contained by `crate::fault::contain`, as at the
//! descriptor door; a stack-overflow abort is not catchable, which is what the bounds exist to avoid.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use protox::Compiler;
use protox::file::{
    ChainFileResolver, File, FileResolver, GoogleFileResolver, IncludeFileResolver,
};

use crate::descriptor::ingest_subjects;
use crate::descriptor::model::Schema;
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

/// keryx's source-nesting bound, derived from the one engine constant ([`super::RECURSION_LIMIT`]):
/// the deepest lexical nesting the descriptor pipeline admits is `RECURSION_LIMIT - 1`, so source
/// nesting deeper than this is refused (it would fail the descriptor door's decode, hit protox's own
/// re-encode limit, or — deeper — overflow protox's parser and abort). Counted as **bracket depth**
/// (`{ }` and `< >`), a dominant over-approximation of the message/group and option-aggregate
/// recursions ([`max_nesting_depth`]).
const SOURCE_NESTING_LIMIT: usize = super::RECURSION_LIMIT - 1;

/// keryx's bound on the number of `.proto` files one compile may open — the root files plus every
/// transitively-imported file. protox resolves imports by **recursion** (`add_import` → `add_import`,
/// `compile/mod.rs:281`), one live parser frame per level, so an import chain longer than the stack
/// admits overflows and *aborts* — the import-graph twin of the nesting overflow
/// [`SOURCE_NESTING_LIMIT`] pre-empts, which the brace scan cannot see (it bounds nesting *within* a
/// file). The chain length is bounded by the file count, so keryx's resolver counts the files it opens
/// and refuses past this. A *chosen* number (not derived from an engine constant), named once here;
/// retired when protox bounds its import recursion.
///
/// The value is set below the measured abort threshold down to the same ~0.8 MB stack floor the
/// nesting guard holds to: a chained import overflows an 8 MB debug main stack near ~1800 files
/// (release frames are smaller, so its threshold is higher — the debug figure is the conservative
/// one), which scales to ~180 on a ~0.8 MB worker stack, so 128 pre-empts the abort there with margin,
/// while far exceeding any real schema's transitive import count. Defense-in-depth, the same residual
/// as the nesting guard (a thread stack below that floor).
const MAX_IMPORT_FILES: usize = 128;

/// keryx's vendored option registry (`keryx/options.proto`, spec Appendix A), embedded at compile
/// time so a user's `import "keryx/options.proto"` resolves with no `-I` — the way protox resolves the
/// well-known types (architecture §11). This is the same file the file-name heuristic
/// (`descriptor::options::read`) recognizes to admit keryx's own options.
const OPTIONS_REGISTRY: &str = include_str!("../../proto/keryx/options.proto");

/// A protox resolver serving keryx's embedded option registry and nothing else. Chained after the user
/// includes (so a project's own `keryx/options.proto` still wins, if it has one) and before the
/// well-known types (so everything else falls through to `GoogleFileResolver`). Keryx-controlled, not
/// user input — a shallow constant — so it is exempt from the nesting scan and the confinement check.
struct OptionsRegistry;

impl FileResolver for OptionsRegistry {
    fn open_file(&self, name: &str) -> Result<File, protox::Error> {
        if name == "keryx/options.proto" {
            File::from_source(name, OPTIONS_REGISTRY)
        } else {
            Err(protox::Error::file_not_found(name))
        }
    }
}

/// The maximum lexical bracket-nesting depth of `.proto` source — a **total**, O(*n*), allocation-free
/// byte scan counting structural `{`/`}` **and** `<`/`>` outside string literals and comments. The
/// scanner's state transitions mirror protox-parse 0.9.0's lexer: a string starts at `'`/`"` and runs
/// to its matching quote or a bare newline (its `Unescaped` run is `[^\x00\n\\'"]`, `lex/mod.rs:222`,
/// so `\r` is string content and a newline ends the token); an escape keeps the string open — an
/// invalid one records an error and *continues* the token (`lex/mod.rs:310–333`), so the scanner's
/// `\`-skip matches; comments are `//`, `#` (protox-parse honours the shell-style `#` line comment,
/// `lex/mod.rs:62,350`, though it flags it), and `/* … */`. So wherever the lexer is in normal text, so
/// is the scanner, and it counts every bracket the parser recurses on.
///
/// **Dominance** (`scanner_depth ≥ parser_recursion_depth` for every input). Two parsers recurse, on
/// two bracket pairs, and the scan over-approximates both:
/// - protox-parse's message/group parser recurses on `{ }` bodies (`map<K,V>` does not self-recurse —
///   its `< >` is one balanced level).
/// - an option's aggregate value is captured as text and *re-parsed* at option resolution by
///   prost-reflect's text-format parser, which is **unbounded** and nests on `{ }` **or** `< >`
///   (`parse_message_value` → `parse_field` → `parse_value` → `parse_message_value`,
///   `dynamic/text_format/parse/mod.rs:47,314`) — the same abort axis the descriptor door pre-empts by
///   refusing `uninterpreted_option`, closed here on the source path by counting `< >` too.
///
/// Counting both pairs on one depth can only over-count (a `map<>` adds a transient level, an aggregate
/// its nesting), which strengthens `≥`. The one adversarial edge: a `\` immediately before a newline in
/// a string. The scanner exits a string at `\n` **unconditionally** — the `\`-escape does not consume a
/// newline, since a protobuf string cannot span a line — so it can never be fooled into treating
/// post-newline brackets as string content and under-counting behind a one-line prefix. An unterminated
/// `/*` runs to EOF (as the lexer's does), and there is nothing after EOF to parse, so no real bracket
/// is missed.
fn max_nesting_depth(source: &str) -> usize {
    enum State {
        Normal,
        LineComment,
        BlockComment,
        Str(u8),
    }

    let bytes = source.as_bytes();
    let mut state = State::Normal;
    let mut depth: usize = 0;
    let mut max: usize = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match state {
            State::Normal => match b {
                b'/' if bytes.get(i + 1) == Some(&b'/') => {
                    state = State::LineComment;
                    i += 1;
                }
                b'#' => state = State::LineComment,
                b'/' if bytes.get(i + 1) == Some(&b'*') => {
                    state = State::BlockComment;
                    i += 1;
                }
                b'"' | b'\'' => state = State::Str(b),
                // `{`/`<` open, `}`/`>` close, on one depth: the message parser recurses on `{ }`,
                // the option-aggregate text-format parser on `{ }` or `< >` — the scan dominates both.
                b'{' | b'<' => {
                    depth += 1;
                    max = max.max(depth);
                }
                b'}' | b'>' => depth = depth.saturating_sub(1),
                _ => {}
            },
            State::LineComment => {
                if b == b'\n' {
                    state = State::Normal;
                }
            }
            State::BlockComment => {
                if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    state = State::Normal;
                    i += 1;
                }
            }
            State::Str(delim) => match b {
                b'\n' => state = State::Normal, // a string cannot span a line — exit unconditionally
                b'\\' if bytes.get(i + 1) != Some(&b'\n') => i += 1, // skip the escaped byte, never a newline
                _ if b == delim => state = State::Normal,
                _ => {}
            },
        }
        i += 1;
    }
    max
}

/// The first file keryx's resolver refused, recovered by [`compile`] to emit a clean keryx diagnostic
/// — the sentinel `protox::Error` the resolver returns to halt the compile would otherwise surface as
/// a generic compile error.
enum Refusal {
    TooDeep { name: String, depth: usize },
    Outside { name: String },
    TooManyFiles { count: usize },
}

/// keryx's own include-root resolver: **locate** a file under one include root, **confine** it (reject
/// a `..`/absolute/symlink escape), **read** it once, **scan** its nesting, and only then hand the
/// already-read source to protox to parse (`File::from_source`) — so the scan runs strictly before the
/// parse, and no file is read twice (no scan-then-open TOCTOU). Replaces protox's `IncludeFileResolver`
/// (which does not confine); delegates `resolve_path` to one so a root path opened by
/// `Compiler::open_file` (`compile/mod.rs:119,137`) routes here like an import.
struct RootedResolver {
    root: PathBuf,
    canonical_root: Option<PathBuf>,
    names: IncludeFileResolver,
    limit: usize,
    /// The count of user files this compile has opened, shared across the chain's roots — a `.proto`
    /// import graph larger than [`MAX_IMPORT_FILES`] is refused before protox's recursive import
    /// resolution overflows.
    opened: Rc<Cell<usize>>,
    refusal: Rc<Cell<Option<Refusal>>>,
}

impl RootedResolver {
    fn new(
        root: PathBuf,
        limit: usize,
        opened: Rc<Cell<usize>>,
        refusal: Rc<Cell<Option<Refusal>>>,
    ) -> RootedResolver {
        RootedResolver {
            canonical_root: root.canonicalize().ok(),
            names: IncludeFileResolver::new(root.clone()),
            root,
            limit,
            opened,
            refusal,
        }
    }

    /// Record the **first** refusal (a refusal halts the compile; recording the first keeps the cause
    /// deterministic even when the chain falls through to a later root — halting is the safe choice
    /// for a name that escapes an earlier root). Panic-free: `Cell::take`/`set`, no borrow to corrupt.
    fn record(&self, refusal: Refusal) {
        let prior = self.refusal.take();
        self.refusal.set(prior.or(Some(refusal)));
    }
}

impl FileResolver for RootedResolver {
    fn resolve_path(&self, path: &Path) -> Option<String> {
        self.names.resolve_path(path)
    }

    fn open_file(&self, name: &str) -> Result<File, protox::Error> {
        let Some(canonical_root) = &self.canonical_root else {
            return Err(protox::Error::file_not_found(name)); // a non-existent root serves nothing
        };
        // Locate, and confine: canonicalise resolves `..`, absolute joins, and symlinks to the real
        // path; a path outside the canonical root is an escape. A missing/unreadable path is not an
        // escape — it falls through the chain to the next resolver (the WKTs, the option registry).
        let Ok(real) = self.root.join(name).canonicalize() else {
            return Err(protox::Error::file_not_found(name));
        };
        if !real.starts_with(canonical_root) {
            self.record(Refusal::Outside {
                name: name.to_owned(),
            });
            return Err(outside_root_error(name));
        }
        // Count every confined user file the compile opens (root or transitive import). protox
        // descends into a file's imports by recursion, so an over-long chain would overflow; the chain
        // length is bounded by the file count, refused here before the recursion reaches abort depth.
        // WKTs and the option registry fall through above (they never confine), so only user files
        // count, and each distinct file is opened once (protox skips an already-imported file).
        let opened = self.opened.get() + 1;
        self.opened.set(opened);
        if opened > MAX_IMPORT_FILES {
            self.record(Refusal::TooManyFiles { count: opened });
            return Err(too_many_files_error(opened));
        }
        let Ok(source) = std::fs::read_to_string(&real) else {
            return Err(protox::Error::file_not_found(name));
        };
        let depth = max_nesting_depth(&source);
        if depth > self.limit {
            self.record(Refusal::TooDeep {
                name: name.to_owned(),
                depth,
            });
            return Err(too_deep_error(name, depth));
        }
        // Parse the bytes keryx already read and scanned (never a second read of the path).
        File::from_source(name, &source)
    }
}

/// Compile `files` (imports resolved against `includes`, then keryx's embedded option registry
/// `keryx/options.proto`, then protox's bundled well-known types) to a descriptor set and ingest it to
/// a [`Schema`], treating the opened files as the subjects. Every user file protox parses — root and
/// import — is located, confined, read once, and depth-scanned by keryx's `RootedResolver` *before*
/// protox parses it. Built through `encode_file_descriptor_set`, **not** `protox::compile` — the
/// convenience re-encodes options through prost-types' typed structs and drops keryx's custom-option
/// bytes (the §20 trap). Total (§6): source too deeply nested is refused (`SourceTooDeep`), an import
/// graph too large is refused (`SourceImportGraphTooLarge`), an import escaping its root is refused
/// (`SourceOutsideRoot`), a protox compile failure composes `UncompilableSource`, and an unforeseen
/// protox panic is contained as a `DependencyFault` — never a panic, and (barring a sub-standard
/// stack) never an abort.
///
/// # Errors
///
/// [`Diagnostics`]: `SourceTooDeep` / `SourceImportGraphTooLarge` / `SourceOutsideRoot` for the guard's
/// refusals, `UncompilableSource` when protox cannot compile the sources, `DependencyFault` for a
/// contained protox panic, or the ingestion diagnostics when the resulting set does not ingest.
pub fn compile(
    files: &[impl AsRef<Path>],
    includes: &[impl AsRef<Path>],
) -> Result<Schema, Diagnostics> {
    let refusal: Rc<Cell<Option<Refusal>>> = Rc::new(Cell::new(None));
    let opened: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    // The closure captures only keryx's own `refusal`/`opened` cells (written in brief non-panicking
    // spans, read after the wrap) and the path slices; protox holds no process-global mutable state a
    // panic could leave inconsistent, and the resolver's own logic is total (no `unwrap`/`expect`, its
    // `Cell` writes panic-free) — so a fault here is protox's. The `AssertUnwindSafe` discharge for
    // this site, whose frame encloses all of `RootedResolver::open_file`.
    let compiled = crate::fault::contain(
        "protox",
        "compiling .proto source",
        || -> Result<(Vec<String>, Vec<u8>), Diagnostics> {
            let mut chain = ChainFileResolver::new();
            for include in includes {
                chain.add(RootedResolver::new(
                    include.as_ref().to_owned(),
                    SOURCE_NESTING_LIMIT,
                    opened.clone(),
                    refusal.clone(),
                ));
            }
            chain.add(OptionsRegistry);
            chain.add(GoogleFileResolver::new());
            let mut compiler = Compiler::with_file_resolver(chain);
            compiler.include_source_info(true).include_imports(true);
            for file in files {
                compiler.open_file(file).map_err(|error| {
                    source_error(&error, Locus::at(file.as_ref().to_string_lossy()))
                })?;
            }
            let subjects = compiler
                .files()
                .filter(|file| !file.is_import())
                .map(|file| file.name().to_owned())
                .collect();
            Ok((subjects, compiler.encode_file_descriptor_set()))
        },
    );
    // A guard refusal (too-deep, or an escaping import) is the true, more-specific cause — emit keryx's
    // own clean diagnostic, overriding the sentinel error the resolver returned to halt the compile.
    if let Some(refusal) = refusal.take() {
        return Err(match refusal {
            Refusal::TooDeep { name, depth } => source_too_deep(&name, depth),
            Refusal::Outside { name } => source_outside_root(&name),
            Refusal::TooManyFiles { count } => source_import_graph_too_large(count),
        });
    }
    // Outer `?`: a contained protox panic → `DependencyFault`. Inner `?`: protox's `UncompilableSource`.
    let (subjects, bytes) = compiled??;
    ingest_subjects(&bytes, &subjects)
}

/// Compose a protox compile error into a keryx `Diagnostic` (§6) at `locus`: the compiler's message is
/// preserved in the detail, its type never re-exported.
fn source_error(error: &protox::Error, locus: Locus) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::UncompilableSource,
        locus,
        format!("{error}"),
    ))
}

/// The clean `SourceTooDeep` diagnostic at the offending file's locus, naming the measured depth, the
/// admitted bound, and the only remedy that is always true (the scanner cannot tell message depth from
/// block depth, so it says "flatten," not "supply a descriptor set" — which fails for a deep chain too).
fn source_too_deep(name: &str, depth: usize) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::SourceTooDeep,
        Locus::at(name.to_owned()),
        format!(
            "source nests {depth} levels deep, beyond keryx's source-nesting limit of {SOURCE_NESTING_LIMIT}: \
             flatten the schema — nesting must stay at or below {SOURCE_NESTING_LIMIT}"
        ),
    ))
}

/// The clean `SourceOutsideRoot` diagnostic at the offending import's locus.
fn source_outside_root(name: &str) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::SourceOutsideRoot,
        Locus::at(name.to_owned()),
        format!(
            "import {name:?} resolves outside its include root; keryx reads only within the include \
             roots it is given"
        ),
    ))
}

/// The clean `SourceImportGraphTooLarge` diagnostic at the whole-input locus, naming the file-count
/// bound the import graph exceeded.
fn source_import_graph_too_large(count: usize) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::SourceImportGraphTooLarge,
        Locus::whole(),
        format!(
            "the source pulls in more than keryx's import limit of {MAX_IMPORT_FILES} files \
             (root plus transitive imports; reached {count}): reduce the import graph, or compile \
             it to a descriptor set and supply that instead"
        ),
    ))
}

/// The **truthful** `protox::Error` the resolver returns to halt a compile on an over-deep file — never
/// "file not found" for a file that was read, so it degrades to a truthful `UncompilableSource` if the
/// side-channel override is ever dropped; the user sees the clean `SourceTooDeep` instead.
fn too_deep_error(name: &str, depth: usize) -> protox::Error {
    protox::Error::new(format!(
        "{name}: nesting depth {depth} exceeds keryx's source-nesting limit"
    ))
}

/// The truthful `protox::Error` for an import that escapes its include root.
fn outside_root_error(name: &str) -> protox::Error {
    protox::Error::new(format!("{name}: resolves outside its include root"))
}

/// The truthful `protox::Error` the resolver returns to halt a compile that has opened too many files
/// — never "file not found" for a file that was found, so it degrades to a truthful
/// `UncompilableSource` if the side-channel override is ever dropped; the user sees the clean
/// `SourceImportGraphTooLarge` instead.
fn too_many_files_error(count: usize) -> protox::Error {
    protox::Error::new(format!(
        "import graph exceeds keryx's file limit ({count} > {MAX_IMPORT_FILES})"
    ))
}

#[cfg(test)]
mod tests {
    use super::{SOURCE_NESTING_LIMIT, max_nesting_depth};

    #[test]
    fn counts_structural_brace_depth() {
        assert_eq!(max_nesting_depth(""), 0);
        assert_eq!(max_nesting_depth("message A { message B { } }"), 2);
        assert_eq!(max_nesting_depth("a{b{c{}}} d{}"), 3);
    }

    #[test]
    fn ignores_braces_in_strings_and_comments() {
        assert_eq!(max_nesting_depth(r#"option x = "{{{{";"#), 0);
        assert_eq!(max_nesting_depth("// {{{{\nmessage A{}"), 1);
        assert_eq!(max_nesting_depth("# {{{{\nmessage A{}"), 1);
        assert_eq!(max_nesting_depth("/* {{{{ */ message A{}"), 1);
        assert_eq!(max_nesting_depth(r#"option x = "\"{{{";"#), 0);
    }

    #[test]
    fn a_backslash_before_a_newline_does_not_swallow_it() {
        // The adversarial edge: a `\` immediately before a newline must not keep the scanner in the
        // string — a protobuf string cannot span a line — so the braces after the newline are counted
        // (else a one-line prefix hides deep nesting from the guard and the parser aborts uncaught).
        assert_eq!(
            max_nesting_depth("option x = \"\\\nmessage A{message B{}}"),
            2
        );
    }

    #[test]
    fn refuses_past_the_limit_admits_at_it() {
        let past = "{".repeat(SOURCE_NESTING_LIMIT + 1);
        assert!(max_nesting_depth(&past) > SOURCE_NESTING_LIMIT);
        let at = "{".repeat(SOURCE_NESTING_LIMIT);
        assert!(max_nesting_depth(&at) <= SOURCE_NESTING_LIMIT);
    }

    #[test]
    fn counts_angle_bracket_nesting() {
        // The option-aggregate axis: prost-reflect's text-format parser nests on `< >` as well as
        // `{ }` and is unbounded, so the scanner counts both. A deep `< >` aggregate reaches the
        // limit even with shallow braces — without this it would slip past the guard and abort.
        assert_eq!(max_nesting_depth("option (r) = { f < g > };"), 2);
        assert_eq!(max_nesting_depth("f < f < f < g > > >"), 3);
        let deep = format!(
            "option (r) = {{ {} }};",
            "f < ".repeat(SOURCE_NESTING_LIMIT)
        );
        assert!(max_nesting_depth(&deep) > SOURCE_NESTING_LIMIT);
    }

    #[test]
    fn a_balanced_map_is_one_transient_level() {
        // `map<K,V>` is the common `< >` in real source; it nests one balanced level and returns to
        // its brace depth, so counting `< >` does not falsely refuse a map-bearing message.
        assert_eq!(
            max_nesting_depth("message M { map<string, int32> m = 1; }"),
            2
        );
    }
}
