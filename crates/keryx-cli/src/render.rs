//! Rendering the library's typed diagnostics at the CLI boundary (architecture §6): human
//! prose (default when stderr is a terminal) or structured JSON (`--format json`, or when
//! stderr is not a terminal). stdout carries the product; a broken pipe on stdout exits
//! cleanly rather than panicking with EPIPE. `NO_COLOR` is honored by construction — the
//! human form is uncolored at M1. The JSON view is the library's own
//! ([`keryx_core::diagnostics::Diagnostics::wire`]), rendered here, not reinvented.

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
        Err(error) => note(Exit::Internal, &format!("write failed: {error}")),
    }
}

/// Render diagnostics to stderr in the resolved format, and return `exit` (the caller's
/// error class). `Auto` resolves by whether stderr is a terminal — the stream the
/// diagnostics travel on, so a `| jq` consumer of stdout still sees human errors on its
/// terminal (architecture §6).
#[must_use]
pub fn report(format: Format, exit: Exit, diagnostics: &Diagnostics) -> Exit {
    let json = match format {
        Format::Json => true,
        Format::Human => false,
        Format::Auto => !std::io::stderr().is_terminal(),
    };
    if json {
        eprintln!("{}", diagnostics.wire());
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
