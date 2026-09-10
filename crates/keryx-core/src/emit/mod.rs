//! Stage 2 — emission (architecture §3, R2; spec §21.4, §13): the `Mapping` rendered to
//! `core.lp`, `views.lp`, and `emit.lp` directly over themelios `construct` +
//! `render_documented` — no builder/printer trait. A pure function of the `Mapping` (P3 →
//! golden-comparable). The honorary signature (§13.1) rides as `%!` docs on the statement
//! carrying each line — a base-fact field's line on its own `#defined`, a message-typed field's
//! on its parent sort's `#defined` in `core.lp` (its `views.lp` rule carries the same line for
//! a standalone reader; architecture §4 gap #2 — themelios has no free-standing `%` block at
//! `86c7dfb`). This module emits `core.lp` (§13.1), `views.lp` (§13.2), and `emit.lp` (§13.3);
//! the manifest (§13.4) is generated elsewhere.
//! Submodules: `build` (themelios constructors), `signature` (the §13.1 lines), `core`,
//! `views`, `emit_lp`.

mod build;
mod core;
mod emit_lp;
mod signature;
mod views;

pub use core::core;
pub use emit_lp::{emit_diagnostic, emit_strict};
pub use views::views;

use themelios_program::prelude::*;

/// Which variant(s) of the serializability theory a generation writes (spec §13.3). The two
/// modes of §12.2 are emitted as two files rather than switched by a `#const`, so a project
/// loads the one it means — the strict theory, under which an unserializable answer set is
/// UNSAT (the production default), or the diagnostic one, under which the model survives and
/// `violates(path, occupant)` names what the reassembler reports — and the manifest records
/// which stand beside it (§13.4), so the manifest reads as the record of the whole generated
/// set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The strict theory alone ([`emit_strict`]).
    Strict,
    /// The diagnostic theory alone ([`emit_diagnostic`]).
    Diagnostic,
    /// Both, each under its own name.
    Both,
}

impl Shape {
    /// The manifest's word for the choice (spec §13.4; Appendix B's `shape both`): `strict`,
    /// `diagnostic`, or `both`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Shape::Strict => "strict",
            Shape::Diagnostic => "diagnostic",
            Shape::Both => "both",
        }
    }
}
use themelios_program::render::render_documented;

use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::policy::model::Unit;

/// Combine a proto doc (if any) and a signature line into one `%!` doc string (spec §13.1,
/// §13.2): the proto prose first, then the signature line, joined by `\n` so
/// `render_documented` emits them in that order — one string, never two `with_doc` calls
/// (the doc set is Ord-rendered).
pub(super) fn doc_line(proto: Option<&str>, signature: &str) -> String {
    match proto {
        Some(text) => format!("{text}\n{signature}"),
        None => signature.to_owned(),
    }
}

/// Render a statement list to documented clingo text (spec §21.4): `Program::of` puts the
/// statements in canonical Ord order and de-duplicates (P3); `render_documented` prepends
/// each statement's `%!` docs. Total (§6): a themelios `Unspellable` composes an
/// `UnrenderableFacts` diagnostic. `Unspellable` fires only when the renderer spells a string
/// — a `Symbol::String`, or an `#include` path — that bears a control character other than
/// newline, which the clingo dialect has no escape for. Two strings reach it from this module:
/// the `#include` path — a validated `Package` under a literal suffix
/// ([`render_client_of_core`]), which carries no control character — and, in the diagnostic
/// `emit.lp`, the fully-qualified proto path a `violates(path, occupant)` head names
/// (`build::text`; the one ground string `build` constructs — every other term `core`/
/// `views`/`emit_lp` build is a variable, a `Function` over variables, or an integer). A
/// path's package and message segments passed the descriptor and policy doors as
/// identifiers, and a field's own name lowered into a validated `Name` before any `Unit`
/// formed; a oneof's name, which the exclusivity obligation's path ends in, is the
/// descriptor's own string, so a hand-built descriptor set could carry one the dialect cannot
/// spell — and it lands here as a diagnostic, never a panic. So the mapping is a live path
/// with no known trigger from a compiled `.proto`, not a witnessed-impossible one. The doc
/// text (proto prose, signature lines) rides as `%!` comment lines, which `render_docs`
/// writes verbatim and never passes through `spell_string` either.
pub(super) fn render(statements: Vec<WithProvenance<Statement>>) -> Result<String, Diagnostics> {
    let program = Program::of_nodes(statements);
    render_documented(&program, Dialect::Clingo).map_err(|unspellable| {
        Diagnostics::from(Diagnostic::new(
            DiagnosticKind::UnrenderableFacts,
            Locus::whole(),
            format!("{unspellable}"),
        ))
    })
}

/// Render a module that opens as a client of `core.lp` (spec §13.2, §13.3) — `views.lp`,
/// `emit.lp` — as `#include "<pkg>.core.lp".` then `statements`: the include resolves the
/// sorts and access-path terms the module's rules join on, so the module is loadable on its
/// own. The directive heads the file, where a reader expects a dependency stated before its
/// use; themelios's canonical statement order would sort an `Include` after every rule and
/// `#defined`, so it is rendered as its own one-statement program ahead of the body. The one
/// place the directive is spelled, so both clients spell it one way. Its operand is the unit's
/// `Package` — validated at the descriptor door as dotted proto identifiers, no `"` or control
/// byte — under the literal `core.lp` suffix, so the path is spellable and cannot break out of
/// its quotes; the door represents that shape rather than this site re-checking it (the
/// threat model's descriptor-door package boundary).
pub(super) fn render_client_of_core(
    unit: &Unit,
    statements: Vec<WithProvenance<Statement>>,
) -> Result<String, Diagnostics> {
    let mut text = render(vec![build::include(format!(
        "{}.core.lp",
        unit.package().as_str()
    ))])?;
    text.push_str(&render(statements)?);
    Ok(text)
}
