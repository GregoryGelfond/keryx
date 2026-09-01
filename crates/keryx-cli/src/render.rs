//! Rendering the library's typed diagnostics at the CLI boundary (architecture §6): human
//! prose (default when stderr is a terminal) or structured JSON (`--format json`, or when
//! stderr is not a terminal). stdout carries the product; a broken pipe on stdout exits
//! cleanly rather than panicking with EPIPE. `NO_COLOR` is honored by construction — the
//! human form is uncolored at M1.

use std::fmt::Write as _;
use std::io::{ErrorKind, IsTerminal};

use clap::ValueEnum;
use keryx_core::diagnostics::Diagnostics;

use crate::exit::Exit;

/// The diagnostic output format (architecture §6).
#[derive(Clone, Copy, ValueEnum)]
pub enum Format {
    /// Human prose if stderr is a terminal, else JSON.
    Auto,
    /// Human prose.
    Human,
    /// Structured JSON (Appendix B `Diagnostic`: `field_path`, `kind`, `detail`).
    Json,
}

/// Write the product to stdout; a broken pipe (a closed downstream, e.g. `| head`) exits
/// cleanly (§6 — no EPIPE panic), any other write error is internal.
#[must_use]
pub fn product(text: &str) -> Exit {
    write_product(&mut std::io::stdout().lock(), text)
}

/// Write `text` to `out`, mapping a broken pipe (a closed downstream) to a clean success
/// (§6 — no EPIPE panic) and any other write error to an internal error. Split from
/// [`product`] so the pipe/error handling is unit-testable over an arbitrary writer, rather
/// than only against a real closed pipe.
fn write_product<W: std::io::Write>(out: &mut W, text: &str) -> Exit {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => Exit::Success,
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Exit::Success,
        Err(error) => {
            eprintln!("keryx: write failed: {error}");
            Exit::Internal
        }
    }
}

/// Render diagnostics to stderr in the resolved format, and return `exit` (the caller's
/// error class). `Auto` resolves by whether stderr is a terminal.
#[must_use]
pub fn report(format: Format, exit: Exit, diagnostics: &Diagnostics) -> Exit {
    let json = match format {
        Format::Json => true,
        Format::Human => false,
        Format::Auto => !std::io::stderr().is_terminal(),
    };
    if json {
        eprintln!("{}", to_json(diagnostics));
    } else {
        for diagnostic in diagnostics.iter() {
            eprintln!("keryx: {diagnostic}");
        }
    }
    exit
}

/// A single-line message to stderr (progress or a non-diagnostic error), returning `exit`.
#[must_use]
pub fn note(exit: Exit, message: &str) -> Exit {
    eprintln!("keryx: {message}");
    exit
}

/// The diagnostics as a JSON array (Appendix B), hand-rolled so keryx-core stays serde-free
/// (the estate's minimal-closure posture).
fn to_json(diagnostics: &Diagnostics) -> String {
    let mut out = String::from("[");
    for (i, diagnostic) in diagnostics.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            r#"{{"field_path":"{}","kind":"{}","detail":"{}"}}"#,
            escape(diagnostic.locus().as_str()),
            diagnostic.kind().as_str(),
            escape(diagnostic.detail()),
        );
    }
    out.push(']');
    out
}

/// Escape a string for a JSON string literal: `"`, `\`, and the C0 controls (§6's string
/// escaping — `\n`/`\t`/`\r` by name, the rest as `\u00NN`).
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
    use std::io::{self, Write};

    use keryx_core::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

    use super::{escape, to_json, write_product};
    use crate::exit::Exit;

    /// A writer whose every write fails with a chosen kind — drives `write_product`'s
    /// broken-pipe and internal-error arms deterministically, with no real pipe.
    struct FailingWriter(io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::from(self.0))
        }
    }

    #[test]
    fn a_broken_pipe_writes_as_a_clean_success() {
        // The headline §6 hardening: `keryx … | head` closing early must exit 0, never
        // an EPIPE panic. A mutant deleting the `BrokenPipe => Success` arm fails here.
        let exit = write_product(&mut FailingWriter(io::ErrorKind::BrokenPipe), "product");
        assert_eq!(exit as u8, Exit::Success as u8);
    }

    #[test]
    fn any_other_write_error_is_internal() {
        let exit = write_product(
            &mut FailingWriter(io::ErrorKind::PermissionDenied),
            "product",
        );
        assert_eq!(exit as u8, Exit::Internal as u8);
    }

    #[test]
    fn a_good_write_delivers_the_product_and_succeeds() {
        let mut buffer = Vec::new();
        let exit = write_product(&mut buffer, "hello");
        assert_eq!(exit as u8, Exit::Success as u8);
        assert_eq!(buffer, b"hello");
    }

    #[test]
    fn escape_encodes_quote_backslash_and_controls() {
        // `"`, `\`, the named C0s (`\n`/`\t`/`\r`), and a bare C0 control (``) — the
        // exact risk in a hand-rolled serializer.
        assert_eq!(
            escape("a\"b\\c\nd\te\rf\u{01}g"),
            "a\\\"b\\\\c\\nd\\te\\rf\\u0001g"
        );
    }

    #[test]
    fn to_json_emits_appendix_b_field_order() {
        // Keys in Appendix B order: field_path, kind, detail.
        let diagnostics = Diagnostics::one(Diagnostic::new(
            DiagnosticKind::SourceCompile,
            Locus::at("a.b.C"),
            "boom",
        ));
        assert_eq!(
            to_json(&diagnostics),
            r#"[{"field_path":"a.b.C","kind":"source_compile","detail":"boom"}]"#
        );
    }

    #[test]
    fn to_json_joins_multiple_diagnostics_into_one_array() {
        let mut diagnostics = Diagnostics::one(Diagnostic::new(
            DiagnosticKind::SourceCompile,
            Locus::whole(),
            "one",
        ));
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::UnmappableName,
            Locus::at("p.Q"),
            "two",
        ));
        assert_eq!(
            to_json(&diagnostics),
            r#"[{"field_path":"","kind":"source_compile","detail":"one"},{"field_path":"p.Q","kind":"unmappable_name","detail":"two"}]"#
        );
    }
}
