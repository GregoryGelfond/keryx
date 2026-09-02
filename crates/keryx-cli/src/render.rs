//! Rendering the library's typed diagnostics at the CLI boundary (architecture §6): human
//! prose (default when stderr is a terminal) or structured JSON (`--format json`, or when
//! stderr — the stream diagnostics travel on — is not a terminal). stdout carries the product;
//! a broken pipe on either stream exits/continues cleanly rather than panicking with EPIPE.
//! `NO_COLOR` is honored by construction — the human form is uncolored at M1. The JSON view of a
//! library `Diagnostic` is the library's own ([`keryx_core::diagnostics::Diagnostics::wire`]); a
//! CLI-adapter error (file I/O, usage) renders in the same shape with the exit class as its kind.

use std::io::{ErrorKind, IsTerminal, Write as _};

use clap::ValueEnum;
use keryx_core::diagnostics::{Diagnostics, escape};

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

impl Format {
    /// Whether diagnostics render as JSON: forced by `json`/`human`, else resolved by whether
    /// stderr — the stream the diagnostics travel on, so a `| jq` consumer of stdout still sees
    /// human errors on its terminal — is not a terminal (architecture §6).
    fn is_json(self) -> bool {
        match self {
            Format::Json => true,
            Format::Human => false,
            Format::Auto => !std::io::stderr().is_terminal(),
        }
    }
}

/// Write one line to stderr, swallowing a broken pipe (a closed `2>&1 | head`) and any other
/// stderr-write failure — there is nowhere left to report it, and a diagnostic must never
/// double-panic into an abort (§6, the stderr counterpart of [`write_product`]'s stdout guard).
fn line(text: &str) {
    let _ = writeln!(std::io::stderr(), "{text}");
}

/// A progress line to stderr (`keryx: <message>`); stdout stays the product (§6).
pub fn progress(message: &str) {
    line(&format!("keryx: {message}"));
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
            line(&format!("keryx: write failed: {error}"));
            Exit::Internal
        }
    }
}

/// Render library diagnostics to stderr in the resolved format, and return `exit` (the caller's
/// error class).
#[must_use]
pub fn report(format: Format, exit: Exit, diagnostics: &Diagnostics) -> Exit {
    if format.is_json() {
        line(&diagnostics.wire());
    } else {
        for diagnostic in diagnostics.iter() {
            line(&format!("keryx: {diagnostic}"));
        }
    }
    exit
}

/// Render one CLI-adapter error to stderr (a file-I/O or usage failure — not a library
/// `Diagnostic`), returning `exit`. Under `--format json` it renders as a one-element wire array
/// with the exit class as its `kind`, so structured stderr is uniform across every error class
/// (§6, §26); otherwise a `keryx:` line.
#[must_use]
pub fn note(format: Format, exit: Exit, message: &str) -> Exit {
    if format.is_json() {
        line(&format!(
            r#"[{{"field_path":"","kind":"{}","detail":"{}"}}]"#,
            exit.slug(),
            escape(message),
        ));
    } else {
        line(&format!("keryx: {message}"));
    }
    exit
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::write_product;
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
    fn a_good_write_delivers_the_product() {
        let mut buffer = Vec::new();
        let exit = write_product(&mut buffer, "hello");
        assert_eq!(exit as u8, Exit::Success as u8);
        assert_eq!(buffer, b"hello");
    }
}
