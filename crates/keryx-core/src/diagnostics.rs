//! keryx's typed error taxonomy (architecture §6): every foreign input crosses a
//! `Result` boundary returning [`Diagnostic`] *values* that name the offending
//! locus — a proto path, not an ASP atom (P1) — never a string or a log, and a
//! partial result is never delivered beside them. Errors are values in
//! themelios's idiom (spec §26): they render themselves through `Display` and
//! compose as `std::error::Error`; the CLI renders them at the boundary and the
//! library invents no error semantics. The shape is the wire `Diagnostic`
//! (Appendix B) — the shape a consuming tool's result envelope may carry — and
//! [`Diagnostics::wire`] is that view, so the value produced here and the value a
//! consumer reads over the wire are one shape.
//! This is the seed the codec, admission, and shape layers grow in later increments.

use std::fmt::{self, Write as _};

/// The proto locus a [`Diagnostic`] names: a fully-qualified path (a file,
/// message, field, enum, or option), or *whole-input* when a failure has no
/// finer locus — the descriptor set as a whole did not decode. Absence is
/// represented as absence (a variant), not an empty-string sentinel inside the
/// space of valid paths; a value carried, not rendered (spec §26) — the boundary
/// renders it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Locus {
    /// The whole-input locus — no finer path than the descriptor set itself.
    #[default]
    Whole,
    /// A fully-qualified proto path (e.g. `dispatch.v1.Shipment.tags`), or a file's (or
    /// import's) name where the failure has no finer path than the file (a source compile,
    /// editions, a packageless file, a pre-read refusal, an over-deep or escaping source). The
    /// path may be empty — a caller that opened an empty spec path (`keryx gen ""`) locates the
    /// failure at that path; it is still a located diagnostic, distinct from `Whole`.
    At(String),
}

impl Locus {
    /// The whole-input locus — no finer path than the set itself.
    #[must_use]
    pub fn whole() -> Locus {
        Locus::Whole
    }

    /// The locus at a fully-qualified proto path (e.g. `dispatch.v1.Shipment.tags`).
    #[must_use]
    pub fn at(path: impl Into<String>) -> Locus {
        Locus::At(path.into())
    }

    /// The path text, or `None` for the whole-input locus.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match self {
            Locus::Whole => None,
            Locus::At(path) => Some(path),
        }
    }

    /// Whether this is the whole-input locus.
    #[must_use]
    pub fn is_whole(&self) -> bool {
        matches!(self, Locus::Whole)
    }
}

/// The class of a [`Diagnostic`]. Non-exhaustive: later increments add codec,
/// admission, and shape kinds without breaking a match written today (§6). The
/// wire `kind` string (Appendix B) is this variant's name at the boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// The descriptor-set bytes did not decode as a `FileDescriptorSet`, or an
    /// import was unresolved — the whole input is unusable (§20). The engine's
    /// own message is *composed into* the detail, never exposed as its type.
    UnreadableDescriptorSet,
    /// The descriptor set decoded, but a file declares a Protobuf edition (edition 2023+)
    /// that keryx's descriptor engine (prost-reflect 0.16.5) cannot yet read — a capability
    /// limit, not malformed input (the file's `syntax` is `"editions"`). Named at the
    /// offending file's locus (§6), one per editions file; the descriptor-set route opens
    /// when the engine gains editions (`docs/proto-support.md`).
    UnsupportedEdition,
    /// A decodable set carried a structurally-malformed element — e.g. a map entry
    /// missing its key or value field, or a map key of a non-key kind. Keeps
    /// ingestion total over an adversarial descriptor that decodes but violates a
    /// protobuf structural invariant (§6 — no panic on foreign input); names the
    /// element's locus.
    MalformedDescriptor,
    /// A keryx custom option carried a value keryx could not lower to a fact
    /// term (§15) — e.g. an integer outside the term range.
    MalformedOption,
    /// A custom option's key is not a valid ASP constant, so its `opt/3` descriptor fact cannot
    /// be rendered (§15, §6). Reachable only on a crafted descriptor set: option admission is a
    /// file-name heuristic (`descriptor::options::read`), not true extension identity, so a set
    /// can carry a non-identifier key that passes admission — a schema-input error, named at the
    /// annotated element's locus, distinct from `UnrenderableFacts` (a keryx bug in constructed
    /// output).
    UnmappableOptionKey,
    /// A keryx-constructed program — the descriptor facts (§21.1) or the generated
    /// vocabulary (§21.4) — could not be rendered to ASP text: a themelios `Unspellable`
    /// composed here. Near-impossible for constructed output; total rather than a panic (§6).
    UnrenderableFacts,
    /// A `.proto` source could not be compiled to a descriptor set by the front-door
    /// compiler (protox) — a parse, type, or import error, or a file whose edition the
    /// compiler does not yet cover (`docs/proto-support.md`). A front-door capability
    /// limit, not a translation error: for a source protox cannot parse but protoc can,
    /// a protoc-compiled descriptor set is the route in — except editions, which the
    /// descriptor engine also cannot yet read (`UnsupportedEdition`). The compiler's own
    /// message is composed into the detail (§6), never exposed as its type.
    UncompilableSource,
    /// A `.proto` schema has no package (a package-less file), which keryx does not
    /// emit for — the generated `.lp`/manifest files would be hidden dotfiles. Named
    /// at the offending file's locus (§6); a schema property, refused before any output.
    PackagelessFile,
    /// Raised in four cases (§6): (1) and (4) are reachable on a well-formed `Schema`, while (2)
    /// and (3) are can't-happen guards — checked rather than assumed. (1) A schema element's
    /// lowered name is not a themelios identifier — after §4.2/§7.4 lowering, a name that cannot be
    /// an ASP predicate/constant symbol; a real if rare live rejection (`message _2Foo` lowers to
    /// `2_foo`, which cannot open an identifier). (2) A message or enum path has no entry in the
    /// resolved sort table — a field's value-type referent absent from the schema, or an element's
    /// own entry (both unreachable from a well-formed `Schema`, the lookup checked not assumed).
    /// (3) An element's declaring file is absent from the schema's file list (`policy::missing_file`;
    /// `ingest` populates each element's declaring file first, so unreachable from it, but checked
    /// not assumed). (4) Two distinct sorts, or two distinct fields on one message, collapse to one
    /// predicate that qualification (§4.2) cannot separate — their base names and every proto-path
    /// qualifier `lower_snake`-collapse to the same string (e.g. sibling messages `Bar` and `Bar_`,
    /// both `bar`, since `lower_snake` trims a trailing `_` and collapses `_`-runs). Qualification
    /// is the injectivity backstop: rather than emit a non-injective map it diagnoses. Names the
    /// offending (or first offending) element's locus; for an absent field referent, the referent
    /// path's.
    UnmappableName,
    /// Two values of one enum lower to the same ASP constant (§7.4) — a within-enum
    /// collision that survives the prefix-strip fallback (e.g. names differing only in
    /// case, `X_FOO`/`X_Foo`, both `x_foo`; or names differing only in a separator run,
    /// `FOO__BAR`/`FOO_BAR`, both `foo_bar`, since `lower_snake` collapses `_` runs). §7.4
    /// resolves residual collisions by *qualification*, which is the codec increment's
    /// (Increment 5); at present the collision is reported (loud, §6) rather than silently
    /// producing a duplicate constant. Names the enum's locus.
    AmbiguousConstant,
    /// A dependency faulted on a foreign-input path and keryx contained the unwind (the threat
    /// model's dependency boundary): an unforeseen panic in foreign code keryx is a client of becomes
    /// this value at keryx's foreign-fault containment seam, rather than unwinding into keryx's caller
    /// — the offending dependency named in the `detail`. Distinct from a keryx bug,
    /// which stays a panic and is reported by the CLI as `Exit::Internal` ("a bug in keryx"):
    /// keryx-core stays total by construction and mints no library "internal" kind of its own (§6).
    /// The split is asymmetric — a keryx bug panics, an upstream fault is a value — and it carries
    /// its own exit class (`Exit::Dependency`), an engine fault being neither a keryx bug nor a
    /// user's schema error. The dependency and operation keryx knows with certainty ride in the
    /// `detail` prose; the wire shape (Appendix B `{field_path, kind, detail}`) is unchanged.
    DependencyFault,
    /// A `.proto` source file nests more deeply than keryx's source-nesting guard admits — refused
    /// *before* protox parses it, so protox's unbounded recursive-descent parser cannot overflow the
    /// stack and abort. Like `UnsupportedEdition`, a capability limit of the descriptor engine applied
    /// early (keryx bounds source nesting at the engine's own decode-recursion limit), not malformed
    /// input; named at the offending file's locus. The guard is defense-in-depth — a sub-standard
    /// thread stack can abort below its bound, closed by the consuming service's process isolation
    /// (the threat model's division of labor).
    SourceTooDeep,
    /// A `.proto` `import` resolves outside its include root — keryx's confining resolver canonicalises
    /// the resolved path and refuses one that escapes, notably a **symlinked** escape protox's own
    /// import-name validation does not catch (protox rejects a bare `..`/absolute import *name* itself,
    /// `UncompilableSource`). So protox reads only within the include roots the operator grants (the
    /// source door's confidentiality). Named at the offending import's locus.
    SourceOutsideRoot,
    /// A `.proto` compile pulls in more files (root plus transitive imports) than keryx's source door
    /// admits — refused *before* protox's **recursive** import resolution (`add_import` → `add_import`,
    /// one live parser frame per level) descends deep enough to overflow the stack and abort. Like
    /// `SourceTooDeep`, a bound keryx imposes because the compiler's recursion is unbounded; the
    /// import-chain length is bounded by the file count, so the count is what the resolver caps. Named
    /// at the whole-input locus. Defense-in-depth with the same residual as `SourceTooDeep` (a
    /// sub-standard thread stack), retired when protox bounds its own import recursion.
    SourceImportGraphTooLarge,
    /// A payload's bytes did not decode as the root message type the caller named — malformed,
    /// truncated, or nested at or beyond the engine's decode recursion limit — so the payload as a
    /// whole is untranslatable (§26): the whole-payload locus, no finer field path. The engine's own
    /// message is composed into the detail, never exposed as its type. Distinct from a payload that
    /// decodes but carries a value the §6 policy refuses, which is diagnosed at the field's locus.
    UndecodablePayload,
}

impl DiagnosticKind {
    /// The stable wire name of this kind (Appendix B `kind`), in `snake_case`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticKind::UnreadableDescriptorSet => "unreadable_descriptor_set",
            DiagnosticKind::UnsupportedEdition => "unsupported_edition",
            DiagnosticKind::MalformedDescriptor => "malformed_descriptor",
            DiagnosticKind::MalformedOption => "malformed_option",
            DiagnosticKind::UnmappableOptionKey => "unmappable_option_key",
            DiagnosticKind::UnrenderableFacts => "unrenderable_facts",
            DiagnosticKind::UncompilableSource => "uncompilable_source",
            DiagnosticKind::PackagelessFile => "packageless_file",
            DiagnosticKind::UnmappableName => "unmappable_name",
            DiagnosticKind::AmbiguousConstant => "ambiguous_constant",
            DiagnosticKind::DependencyFault => "dependency_fault",
            DiagnosticKind::SourceTooDeep => "source_too_deep",
            DiagnosticKind::SourceOutsideRoot => "source_outside_root",
            DiagnosticKind::SourceImportGraphTooLarge => "source_import_graph_too_large",
            DiagnosticKind::UndecodablePayload => "undecodable_payload",
        }
    }
}

/// A single structured diagnostic (architecture §6): its [`DiagnosticKind`], the
/// [`Locus`] it names, and a human-readable `detail`. Comparable, cloneable data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    kind: DiagnosticKind,
    locus: Locus,
    detail: String,
}

impl Diagnostic {
    /// A diagnostic of the given kind at the given locus, with detail prose.
    pub fn new(kind: DiagnosticKind, locus: Locus, detail: impl Into<String>) -> Diagnostic {
        Diagnostic {
            kind,
            locus,
            detail: detail.into(),
        }
    }

    /// The kind.
    #[must_use]
    pub fn kind(&self) -> DiagnosticKind {
        self.kind
    }

    /// The locus.
    #[must_use]
    pub fn locus(&self) -> &Locus {
        &self.locus
    }

    /// The detail prose.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Human-readable rendering (architecture §6): `kind at locus: detail`, or
/// `kind: detail` for the whole-input locus. `--format json` serializes the
/// fields instead ([`Diagnostics::wire`]).
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.locus.path() {
            None => write!(f, "{}: {}", self.kind.as_str(), human(&self.detail)),
            Some(path) => write!(
                f,
                "{} at {}: {}",
                self.kind.as_str(),
                human(path),
                human(&self.detail)
            ),
        }
    }
}

/// A diagnostic composes as a standard error (themelios's posture, spec §26), so a
/// Rust consumer can `?` and box it; `Display` states the fixable question.
impl std::error::Error for Diagnostic {}

/// A non-empty collection of diagnostics — the error half of an ingestion or
/// render `Result`. Non-empty by construction: a failure names at least one
/// cause. Totality (§6): a caller collects every diagnosis it can before
/// returning, never only the first, and never a partial success beside them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    /// The diagnostics carrying a single cause.
    #[must_use]
    pub fn one(diagnostic: Diagnostic) -> Diagnostics {
        Diagnostics(vec![diagnostic])
    }

    /// The diagnostics collecting a possibly-empty sequence of causes: `None` when the sequence is
    /// empty (nothing to report), else a non-empty `Diagnostics`. The one home of the "take the first,
    /// then push the rest" idiom every multi-refusal site would otherwise copy — so the non-empty
    /// invariant is upheld here, not re-argued at each caller (`descriptor::pre_validate`,
    /// `policy::reject_packageless`).
    #[must_use]
    pub fn collect(diagnostics: impl IntoIterator<Item = Diagnostic>) -> Option<Diagnostics> {
        let mut iter = diagnostics.into_iter();
        let mut out = Diagnostics::one(iter.next()?);
        out.0.extend(iter);
        Some(out)
    }

    /// Add a further cause — the collection only grows, so the non-empty
    /// invariant holds.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.0.push(diagnostic);
    }

    /// The diagnostics, in the order they were collected.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Diagnostic> {
        self.0.iter()
    }

    /// Whether any diagnostic in the collection is of `kind` — the "does this collection carry kind
    /// K" query a boundary asks to classify a run (the CLI routes an exit on `DependencyFault`, and
    /// distinguishes a schema-input `UnmappableOptionKey` from a keryx bug), kept on the collection
    /// that owns it rather than re-derived as a hand-rolled closure at each site.
    #[must_use]
    pub fn contains_kind(&self, kind: DiagnosticKind) -> bool {
        self.0.iter().any(|diagnostic| diagnostic.kind == kind)
    }

    /// The number of diagnoses — at least one.
    #[must_use]
    // `Diagnostics` is non-empty by construction, so there is no `is_empty` to pair with `len`.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The wire view: the diagnostics as a JSON array (Appendix B `Diagnostic`:
    /// `field_path`, `kind`, `detail`), hand-rolled so keryx-core takes no serde
    /// dependency — a JSON array of three strings does not earn one. The whole-input
    /// locus flattens to an empty `field_path`, as Appendix B's proto3 string forces
    /// the wire to. This is the view a library consumer (the CLI today, a service over
    /// the wire) reads.
    #[must_use]
    pub fn wire(&self) -> String {
        let mut out = String::from("[");
        for (i, diagnostic) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&wire_object(
                diagnostic.locus.path().unwrap_or(""),
                diagnostic.kind.as_str(),
                &diagnostic.detail,
            ));
        }
        out.push(']');
        out
    }
}

/// One diagnostic per line — the human view; the CLI adds its `keryx:` prefix per
/// line, other consumers print this directly.
impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, diagnostic) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{diagnostic}")?;
        }
        Ok(())
    }
}

/// The error half of every keryx-core `Result` composes as a standard error.
impl std::error::Error for Diagnostics {}

/// One-cause construction from a bare [`Diagnostic`], for the common `Err` path.
impl From<Diagnostic> for Diagnostics {
    fn from(diagnostic: Diagnostic) -> Diagnostics {
        Diagnostics::one(diagnostic)
    }
}

/// One wire `Diagnostic` object — `{"field_path":..,"kind":..,"detail":..}` (Appendix B), each
/// value JSON-escaped. The single home of the object's shape: [`Diagnostics::wire`] frames a JSON
/// array of these for library diagnostics, and the CLI renders a one-element array of one for an
/// adapter error (a file-I/O or usage failure, with no library `DiagnosticKind`), so the two
/// structured-stderr forms (§26) cannot spell the object differently.
#[must_use]
pub fn wire_object(field_path: &str, kind: &str, detail: &str) -> String {
    format!(
        r#"{{"field_path":"{}","kind":"{}","detail":"{}"}}"#,
        escape(field_path),
        escape(kind),
        escape(detail),
    )
}

/// Escape a string for a JSON string literal: `"`, `\`, and the C0 controls (`\n`/`\t`/`\r` by
/// name, the rest as `\u00NN`). Non-UTF-8 is impossible in a proto path or a composed detail, so
/// no byte fallback is needed. Internal to the wire serializers ([`wire_object`], and through it
/// [`Diagnostics::wire`]).
#[must_use]
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// The human-view length cap, in **characters** (not bytes): a diagnostic is a line for a person, not
/// a payload dump, and adversary-influenced text must not flood a terminal. Applied *after* escaping,
/// so the cap counts what is shown.
const DETAIL_HUMAN_CHARS: usize = 2_048;

/// The terminal view of a composed value — sibling to `escape`, the JSON view — **escaped and
/// bounded**. Escapes control characters (C0/C1 and DEL) to a visible `\u{…}` form so an adversary's
/// control bytes cannot move a terminal's cursor or inject an escape sequence, then takes the first
/// `DETAIL_HUMAN_CHARS` characters, marking truncation with `…`. On a `char` boundary throughout: a
/// byte-index slice of adversary multibyte text would panic, and keryx's own renderers stay total (§6).
/// Applied to the `detail` and the locus path — the two adversary-influenced fields — not the `kind`,
/// which is keryx's own name. Public so a boundary rendering adversary-influenced text outside a
/// `Diagnostic` (the CLI's panic hook, which prints a contained fault's payload under `RUST_BACKTRACE`)
/// bounds and escapes it through the one function, not by hand.
#[must_use]
pub fn human(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() {
            let code = ch as u32;
            let _ = write!(escaped, "\\u{{{code:04x}}}");
        } else {
            escaped.push(ch);
        }
    }
    let char_count = escaped.chars().count();
    let mut out: String = escaped.chars().take(DETAIL_HUMAN_CHARS).collect();
    if char_count > DETAIL_HUMAN_CHARS {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{DETAIL_HUMAN_CHARS, Diagnostic, DiagnosticKind, Diagnostics, Locus, escape};

    #[test]
    fn escape_encodes_quote_backslash_and_controls() {
        // `"`, `\`, the named C0s (`\n`/`\t`/`\r`), and a bare C0 control (`\u{01}`) — the
        // exact risk in the hand-rolled wire serializer.
        assert_eq!(
            escape("a\"b\\c\nd\te\rf\u{01}g"),
            "a\\\"b\\\\c\\nd\\te\\rf\\u0001g"
        );
    }

    #[test]
    fn the_human_view_escapes_controls_and_bounds_on_a_char_boundary() {
        // Adversary-influenced detail: control bytes (ESC, BEL) and multibyte text past the cap. The
        // human view escapes the controls to a visible form and truncates on a `char` boundary — a
        // byte-index slice of the multibyte text would panic, which is the whole point.
        let noisy = format!("\u{1b}[31m\u{7}{}", "é".repeat(DETAIL_HUMAN_CHARS + 50));
        let shown =
            Diagnostic::new(DiagnosticKind::DependencyFault, Locus::whole(), noisy).to_string();
        assert!(
            !shown.contains('\u{1b}') && !shown.contains('\u{7}'),
            "controls escaped: {shown:?}"
        );
        assert!(shown.contains('…'), "truncation marked");

        // The locus *path* is the other adversary-influenced field (a declared name); `Display` routes
        // it through `human` too, so it is escaped and bounded alike — a mutant dropping that survives
        // without this case (the field an attacker controls most directly).
        let noisy_path = format!("\u{1b}\u{7}{}", "é".repeat(DETAIL_HUMAN_CHARS + 50));
        let located = Diagnostic::new(
            DiagnosticKind::UnmappableName,
            Locus::at(noisy_path),
            "boom",
        )
        .to_string();
        assert!(
            !located.contains('\u{1b}') && !located.contains('\u{7}'),
            "path controls escaped: {located:?}"
        );
        assert!(located.contains('…'), "path truncation marked");
    }

    #[test]
    fn kind_wire_names_are_stable() {
        // Stable wire names (Appendix B `kind`), asserted independently.
        assert_eq!(
            DiagnosticKind::UnreadableDescriptorSet.as_str(),
            "unreadable_descriptor_set"
        );
        assert_eq!(
            DiagnosticKind::UnsupportedEdition.as_str(),
            "unsupported_edition"
        );
        assert_eq!(
            DiagnosticKind::MalformedDescriptor.as_str(),
            "malformed_descriptor"
        );
        assert_eq!(DiagnosticKind::MalformedOption.as_str(), "malformed_option");
        assert_eq!(
            DiagnosticKind::UnmappableOptionKey.as_str(),
            "unmappable_option_key"
        );
        assert_eq!(
            DiagnosticKind::UnrenderableFacts.as_str(),
            "unrenderable_facts"
        );
        assert_eq!(
            DiagnosticKind::UncompilableSource.as_str(),
            "uncompilable_source"
        );
        assert_eq!(DiagnosticKind::PackagelessFile.as_str(), "packageless_file");
        assert_eq!(DiagnosticKind::UnmappableName.as_str(), "unmappable_name");
        assert_eq!(
            DiagnosticKind::AmbiguousConstant.as_str(),
            "ambiguous_constant"
        );
        assert_eq!(DiagnosticKind::DependencyFault.as_str(), "dependency_fault");
        assert_eq!(DiagnosticKind::SourceTooDeep.as_str(), "source_too_deep");
        assert_eq!(
            DiagnosticKind::SourceOutsideRoot.as_str(),
            "source_outside_root"
        );
        assert_eq!(
            DiagnosticKind::SourceImportGraphTooLarge.as_str(),
            "source_import_graph_too_large"
        );
        assert_eq!(
            DiagnosticKind::UndecodablePayload.as_str(),
            "undecodable_payload"
        );
        // A new kind must be added above: this exhaustive match (no wildcard,
        // allowed in-crate despite #[non_exhaustive]) fails to compile otherwise.
        match DiagnosticKind::UnreadableDescriptorSet {
            DiagnosticKind::UnreadableDescriptorSet
            | DiagnosticKind::UnsupportedEdition
            | DiagnosticKind::MalformedDescriptor
            | DiagnosticKind::MalformedOption
            | DiagnosticKind::UnmappableOptionKey
            | DiagnosticKind::UnrenderableFacts
            | DiagnosticKind::UncompilableSource
            | DiagnosticKind::PackagelessFile
            | DiagnosticKind::UnmappableName
            | DiagnosticKind::AmbiguousConstant
            | DiagnosticKind::DependencyFault
            | DiagnosticKind::SourceTooDeep
            | DiagnosticKind::SourceOutsideRoot
            | DiagnosticKind::SourceImportGraphTooLarge
            | DiagnosticKind::UndecodablePayload => {}
        }
    }

    #[test]
    fn the_whole_input_locus_is_an_absence_not_a_sentinel() {
        assert!(Locus::whole().is_whole());
        assert_eq!(Locus::whole().path(), None);
        assert_eq!(Locus::at("x").path(), Some("x"));
        assert!(!Locus::at("x").is_whole());
    }

    #[test]
    fn diagnostics_compose_as_a_standard_error() {
        fn takes_error(_: &dyn std::error::Error) {}
        let diagnostics = Diagnostics::one(Diagnostic::new(
            DiagnosticKind::UnmappableName,
            Locus::at("p.Q"),
            "boom",
        ));
        takes_error(&diagnostics);
        // `Display` is one diagnostic per line, no `keryx:` prefix (the CLI's).
        assert_eq!(diagnostics.to_string(), "unmappable_name at p.Q: boom");
    }

    #[test]
    fn wire_is_the_appendix_b_array_with_a_flat_whole_locus() {
        let mut diagnostics = Diagnostics::one(Diagnostic::new(
            DiagnosticKind::UncompilableSource,
            Locus::whole(),
            "one",
        ));
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::UnmappableName,
            Locus::at("p.Q"),
            "two\"x",
        ));
        assert_eq!(
            diagnostics.wire(),
            r#"[{"field_path":"","kind":"uncompilable_source","detail":"one"},{"field_path":"p.Q","kind":"unmappable_name","detail":"two\"x"}]"#
        );
    }

    #[test]
    fn contains_kind_asks_the_collection_whether_it_carries_a_kind() {
        let mut diagnostics = Diagnostics::one(Diagnostic::new(
            DiagnosticKind::UncompilableSource,
            Locus::whole(),
            "one",
        ));
        assert!(diagnostics.contains_kind(DiagnosticKind::UncompilableSource));
        assert!(!diagnostics.contains_kind(DiagnosticKind::DependencyFault));
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::DependencyFault,
            Locus::whole(),
            "two",
        ));
        assert!(diagnostics.contains_kind(DiagnosticKind::DependencyFault));
    }
}
