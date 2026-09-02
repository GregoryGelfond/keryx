//! Stage 2 — emission (architecture §3, R2; spec §21.4, §13): the `Mapping` rendered to
//! `core.lp` and `views.lp` directly over themelios `construct` + `render_documented` — no
//! builder/printer trait. A pure function of the `Mapping` (P3 → golden-comparable). The
//! honorary signature (§13.1) rides as `%!` docs on the statement carrying each line — a
//! base-fact field's line on its own `#defined`, a message-typed field's on its parent sort's
//! `#defined` in `core.lp` (its `views.lp` rule carries the same line for a standalone reader;
//! architecture §4 gap #2 — themelios has no free-standing `%` block at `86c7dfb`). This
//! module emits
//! `core.lp` (§13.1) and `views.lp` (§13.2); the other §13 outputs — `shape.lp` (§13.3,
//! Increment 4) and the manifest (§13.4) — are generated elsewhere.
//! Submodules: `build` (themelios constructors), `signature` (the §13.1 lines), `core`,
//! `views`.

mod build;
mod core;
mod signature;
mod views;

pub use core::core;
pub use views::views;

use themelios_program::prelude::*;
use themelios_program::render::render_documented;

use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

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
/// `UnrenderableFacts` diagnostic — belt-and-suspenders over a render failure that is
/// witnessed-impossible for `core`/`views`'s own output: `Unspellable` fires only when the
/// symbol walk spells a `Symbol::String`, and `build` never constructs one — every term
/// `core`/`views` build is a bare `Variable` or a `Function` applied to variables (`build::var`,
/// `build::apply`, `build::atom`), never a ground `Term::Symbolic`. The doc text (proto
/// prose, signature lines) rides as `%!` comment lines, which `render_docs` writes verbatim
/// and never passes through `spell_string` either.
pub(super) fn render(statements: Vec<WithProvenance<Statement>>) -> Result<String, Diagnostics> {
    let program = Program::of(statements);
    render_documented(&program, Dialect::Clingo).map_err(|unspellable| {
        Diagnostics::from(Diagnostic::new(
            DiagnosticKind::UnrenderableFacts,
            Locus::whole(),
            format!("{unspellable}"),
        ))
    })
}
