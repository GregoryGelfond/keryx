//! keryx's typed error taxonomy (architecture §6): every foreign input crosses a
//! `Result` boundary returning [`Diagnostic`] *values* that name the offending
//! locus — a proto path, not an ASP atom (P1) — never a string or a log, and a
//! partial result is never delivered beside them. Errors are values in
//! themelios's idiom (spec §26): they render themselves through `Display` and
//! compose as `std::error::Error`; the CLI renders them at the boundary and the
//! library invents no error semantics. The shape mirrors the envelope's wire
//! `Diagnostic` (Appendix B), and [`Diagnostics::wire`] is that wire view — so the
//! value produced here and the value pythia reads over the wire are one shape.
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
    /// A fully-qualified proto path (e.g. `dispatch.v1.Shipment.tags`), or a file
    /// path for a source-compile failure. The path may be empty — a caller that
    /// opened an empty spec path (`keryx gen ""`) locates the failure at that path;
    /// it is still a located diagnostic, distinct from `Whole`.
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
    /// A decodable set carried a structurally-malformed element — e.g. a map entry
    /// missing its key or value field, or a map key of a non-key kind. Keeps
    /// ingestion total over an adversarial descriptor that decodes but violates a
    /// protobuf structural invariant (§6 — no panic on foreign input); names the
    /// element's locus.
    MalformedDescriptor,
    /// A keryx custom option carried a value keryx could not lower to a fact
    /// term (§15) — e.g. an integer outside the term range.
    MalformedOption,
    /// A keryx-constructed program — the descriptor facts (§21.1) or the generated
    /// vocabulary (§21.4) — could not be rendered to ASP text: a themelios `Unspellable`
    /// composed here. Near-impossible for constructed output; total rather than a panic (§6).
    UnrenderableFacts,
    /// A `.proto` source could not be compiled to a descriptor set by the front-door
    /// compiler (protox) — a parse, type, or import error, or a file whose edition the
    /// compiler does not yet cover (`docs/proto-support.md`). A front-door capability
    /// limit, not a translation error: keryx branches on resolved features, so an
    /// editions file is ingestible once a descriptor set for it is supplied by another
    /// producer. The compiler's own message is composed into the detail (§6), never
    /// exposed as its type.
    UncompilableSource,
    /// A `.proto` schema has no package (a package-less file), which keryx does not
    /// emit for — the generated `.lp`/manifest files would be hidden dotfiles. Named
    /// at the offending file's locus (§6); a schema property, refused before any output.
    PackagelessFile,
    /// Raised in three cases (§6). Two are near-impossible on a well-formed `Schema` but
    /// checked rather than assumed: (1) a schema element's lowered name is not a themelios
    /// identifier — after §4.2/§7.4 lowering, a name that cannot be an ASP predicate/constant
    /// symbol; and (2) a field's value type references a message or enum path absent from the
    /// schema (unreachable from `ingest`, which never leaves a dangling reference, but the
    /// lookup is checked, not assumed). The third is genuinely reachable: (3) two distinct
    /// sorts, or two distinct fields on one message, collapse to one predicate that
    /// qualification (§4.2) cannot separate — their base names and every proto-path qualifier
    /// `lower_snake`-collapse to the same string (e.g. sibling messages `Bar` and `Bar_`, both
    /// `bar`, since `lower_snake` trims a trailing `_` and collapses `_`-runs). Qualification is
    /// the injectivity backstop: rather than emit a non-injective map it diagnoses. Names the
    /// offending (or first offending) element's, or the referencing field's, locus.
    UnmappableName,
    /// Two values of one enum lower to the same ASP constant (§7.4) — a within-enum
    /// collision that survives the prefix-strip fallback (e.g. names differing only in
    /// case, `X_FOO`/`X_Foo`, both `x_foo`; or names differing only in a separator run,
    /// `FOO__BAR`/`FOO_BAR`, both `foo_bar`, since `lower_snake` collapses `_` runs). §7.4
    /// resolves residual collisions by *qualification*, which is the codec increment's
    /// (Increment 5); at present the collision is reported (loud, §6) rather than silently
    /// producing a duplicate constant. Names the enum's locus.
    AmbiguousConstant,
}

impl DiagnosticKind {
    /// The stable wire name of this kind (Appendix B `kind`), in `snake_case`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticKind::UnreadableDescriptorSet => "unreadable_descriptor_set",
            DiagnosticKind::MalformedDescriptor => "malformed_descriptor",
            DiagnosticKind::MalformedOption => "malformed_option",
            DiagnosticKind::UnrenderableFacts => "unrenderable_facts",
            DiagnosticKind::UncompilableSource => "uncompilable_source",
            DiagnosticKind::PackagelessFile => "packageless_file",
            DiagnosticKind::UnmappableName => "unmappable_name",
            DiagnosticKind::AmbiguousConstant => "ambiguous_constant",
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
            None => write!(f, "{}: {}", self.kind.as_str(), self.detail),
            Some(path) => write!(f, "{} at {}: {}", self.kind.as_str(), path, self.detail),
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

    /// The number of diagnoses — at least one.
    #[must_use]
    // `Diagnostics` is non-empty by construction, so there is no `is_empty` to pair with `len`.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The wire view: the diagnostics as a JSON array (Appendix B `Diagnostic`:
    /// `field_path`, `kind`, `detail`), hand-rolled so keryx-core stays serde-free
    /// (the estate's minimal-closure posture). The whole-input locus flattens to an
    /// empty `field_path`, as Appendix B's proto3 string forces the wire to. This is
    /// the view a library consumer (the CLI today, pythia over the wire) reads.
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

#[cfg(test)]
mod tests {
    use super::{Diagnostic, DiagnosticKind, Diagnostics, Locus, escape};

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
    fn kind_wire_names_are_stable() {
        // Stable wire names (Appendix B `kind`), asserted independently.
        assert_eq!(
            DiagnosticKind::UnreadableDescriptorSet.as_str(),
            "unreadable_descriptor_set"
        );
        assert_eq!(
            DiagnosticKind::MalformedDescriptor.as_str(),
            "malformed_descriptor"
        );
        assert_eq!(DiagnosticKind::MalformedOption.as_str(), "malformed_option");
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
        // A new kind must be added above: this exhaustive match (no wildcard,
        // allowed in-crate despite #[non_exhaustive]) fails to compile otherwise.
        match DiagnosticKind::UnreadableDescriptorSet {
            DiagnosticKind::UnreadableDescriptorSet
            | DiagnosticKind::MalformedDescriptor
            | DiagnosticKind::MalformedOption
            | DiagnosticKind::UnrenderableFacts
            | DiagnosticKind::UncompilableSource
            | DiagnosticKind::PackagelessFile
            | DiagnosticKind::UnmappableName
            | DiagnosticKind::AmbiguousConstant => {}
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
}
