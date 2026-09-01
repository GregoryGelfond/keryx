//! keryx's typed error taxonomy (architecture §6): every foreign input crosses a
//! `Result` boundary returning [`Diagnostic`] *values* that name the offending
//! locus — a proto path, not an ASP atom (P1) — never a string or a log, and a
//! partial result is never delivered beside them. Errors are values in
//! themelios's idiom (spec §26): the CLI renders them at the boundary and the
//! library invents no error semantics. The shape mirrors the envelope's wire
//! `Diagnostic` (Appendix B), so the value produced here and the value pythia
//! reads over the wire are one shape. This is the seed the codec, admission, and
//! shape layers grow in later increments.

use std::fmt;

/// The proto locus a [`Diagnostic`] names: a fully-qualified path (a file,
/// message, field, enum, or option), or *whole-input* when a failure has no
/// finer locus — the descriptor set as a whole did not decode. A value, carried
/// not rendered (spec §26); the boundary renders it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Locus(String);

impl Locus {
    /// The whole-input locus — no finer path than the set itself.
    #[must_use]
    pub fn whole() -> Locus {
        Locus(String::new())
    }

    /// The locus at a fully-qualified proto path (e.g. `dispatch.v1.Shipment.tags`).
    #[must_use]
    pub fn at(path: impl Into<String>) -> Locus {
        Locus(path.into())
    }

    /// The path text; empty for the whole-input locus.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is the whole-input locus.
    #[must_use]
    pub fn is_whole(&self) -> bool {
        self.0.is_empty()
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
    /// The descriptor-facts program could not be rendered to ASP text — a
    /// themelios `Unspellable` composed here. Near-impossible for constructed
    /// facts; total rather than a panic (§6).
    UnrenderableFacts,
    /// A `.proto` source could not be compiled to a descriptor set by the front-door
    /// compiler (protox) — a parse, type, or import error, or a file whose edition the
    /// compiler does not yet cover (`docs/proto-support.md`). A front-door capability
    /// limit, not a translation error: keryx branches on resolved features, so an
    /// editions file is ingestible once a descriptor set for it is supplied by another
    /// producer. The compiler's own message is composed into the detail (§6), never
    /// exposed as its type.
    SourceCompile,
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
            DiagnosticKind::SourceCompile => "source_compile",
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

/// Human-readable rendering, for `stderr` at the CLI boundary (architecture §6).
/// `--format json` (a later increment) serializes the fields instead.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.locus.is_whole() {
            write!(f, "{}: {}", self.kind.as_str(), self.detail)
        } else {
            write!(
                f,
                "{} at {}: {}",
                self.kind.as_str(),
                self.locus.as_str(),
                self.detail
            )
        }
    }
}

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
}

/// One-cause construction from a bare [`Diagnostic`], for the common `Err` path.
impl From<Diagnostic> for Diagnostics {
    fn from(diagnostic: Diagnostic) -> Diagnostics {
        Diagnostics::one(diagnostic)
    }
}

#[cfg(test)]
mod tests {
    use super::DiagnosticKind;

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
        assert_eq!(DiagnosticKind::SourceCompile.as_str(), "source_compile");
        // A new kind must be added above: this exhaustive match (no wildcard,
        // allowed in-crate despite #[non_exhaustive]) fails to compile otherwise.
        match DiagnosticKind::UnreadableDescriptorSet {
            DiagnosticKind::UnreadableDescriptorSet
            | DiagnosticKind::MalformedDescriptor
            | DiagnosticKind::MalformedOption
            | DiagnosticKind::UnrenderableFacts
            | DiagnosticKind::SourceCompile => {}
        }
    }
}
