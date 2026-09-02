//! Unit coverage for the diagnostics taxonomy (architecture §6, spec §26):
//! the [`Diagnostic`] rendering at the whole-input and located loci, the
//! non-empty-by-construction [`Diagnostics`] collection, and the [`Locus`]
//! accessors.

use keryx_core::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

#[test]
fn whole_input_diagnostic_renders_without_a_locus() {
    let diagnostic = Diagnostic::new(
        DiagnosticKind::UnreadableDescriptorSet,
        Locus::whole(),
        "unexpected end of buffer",
    );

    assert_eq!(
        diagnostic.to_string(),
        "unreadable_descriptor_set: unexpected end of buffer"
    );
    assert!(!diagnostic.to_string().contains(" at "));
}

#[test]
fn located_diagnostic_renders_its_locus() {
    let diagnostic = Diagnostic::new(
        DiagnosticKind::MalformedOption,
        Locus::at("dispatch.v1.Shipment.tags"),
        "value out of range",
    );

    assert_eq!(
        diagnostic.to_string(),
        "malformed_option at dispatch.v1.Shipment.tags: value out of range"
    );
}

#[test]
fn diagnostics_collect_in_order() {
    let first = Diagnostic::new(DiagnosticKind::MalformedDescriptor, Locus::whole(), "first");
    let second = Diagnostic::new(
        DiagnosticKind::UnrenderableFacts,
        Locus::at("dispatch.v1.Shipment"),
        "second",
    );

    let mut diagnostics = Diagnostics::one(first.clone());
    diagnostics.push(second.clone());

    assert_eq!(diagnostics.len(), 2);
    let collected: Vec<&Diagnostic> = diagnostics.iter().collect();
    assert_eq!(collected, vec![&first, &second]);
}

#[test]
fn locus_whole_and_at() {
    assert!(Locus::whole().is_whole());
    assert_eq!(Locus::whole().path(), None);

    let at = Locus::at("x");
    assert!(!at.is_whole());
    assert_eq!(at.path(), Some("x"));
}
