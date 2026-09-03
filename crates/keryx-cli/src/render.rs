//! Rendering the library's typed diagnostics at the CLI boundary (architecture §6): human
//! prose (default when stderr is a terminal) or structured JSON (`--format json`, or when
//! stderr — the stream diagnostics travel on — is not a terminal). stdout carries the product;
//! a broken pipe on either stream exits/continues cleanly rather than panicking with EPIPE.
//! `NO_COLOR` is honored by construction — the human form is uncolored at present. The JSON view of a
//! library `Diagnostic` is the library's own ([`keryx_core::diagnostics::Diagnostics::wire`]); a
//! CLI-adapter error (file I/O, usage) renders in the same shape with the exit class as its kind.

use std::io::{ErrorKind, IsTerminal, Write as _};

use clap::ValueEnum;
use keryx_core::diagnostics::{Diagnostics, wire_object};

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
    pub(crate) fn is_json(self) -> bool {
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
/// cleanly (§6 — no EPIPE panic), any other write error is internal (rendered in `format`).
#[must_use]
pub fn product(format: Format, text: &str) -> Exit {
    write_product(format, &mut std::io::stdout().lock(), text)
}

/// Write `text` to `out`, mapping a broken pipe (a closed downstream) to a clean success
/// (§6 — no EPIPE panic) and any other write error to an internal error rendered through [`note`]
/// in `format` — so the `Internal` class is structured under `--format json` like every other
/// (§6, §26). Split from [`product`] so the pipe/error handling is unit-testable over an arbitrary
/// writer, rather than only against a real closed pipe.
fn write_product<W: std::io::Write>(format: Format, out: &mut W, text: &str) -> Exit {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => Exit::Success,
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Exit::Success,
        Err(error) => note(format, Exit::Internal, &format!("write failed: {error}")),
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

/// Render one CLI-adapter error to stderr (a file-I/O, usage, or internal failure — not a library
/// `Diagnostic`), returning `exit`. Under `--format json` it renders as a one-element wire array
/// with the exit class as its `kind`, so structured stderr is uniform across keryx's own error
/// classes (§6, §26); clap's own usage errors, which precede `--format` parsing, are the one
/// inherent exception. Otherwise a `keryx:` line.
#[must_use]
pub fn note(format: Format, exit: Exit, message: &str) -> Exit {
    line(&note_line(format, exit, message));
    exit
}

/// The one line [`note`] renders: under `--format json` a one-element wire array (the same
/// `wire_object` serializer the library uses — the exit class is the `kind`, a CLI-adapter error's
/// field path empty, so it is byte-shaped exactly like a library diagnostic), else a `keryx:` prose
/// line. Pure, so the JSON form is unit-testable.
fn note_line(format: Format, exit: Exit, message: &str) -> String {
    if format.is_json() {
        format!("[{}]", wire_object("", exit.slug(), message))
    } else {
        format!("keryx: {message}")
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::{Format, note_line, write_product};
    use crate::exit::Exit;

    #[test]
    fn a_json_adapter_error_is_a_one_line_wire_array_naming_the_class() {
        // The `--format json` `note` path (the `Internal` write-failure travels it): a single-line
        // one-element wire array carrying the exit class as `kind` and the message as `detail`.
        let json = note_line(Format::Json, Exit::Internal, "write failed");
        assert!(!json.contains('\n'), "one line: {json}");
        assert!(
            json.contains(r#""kind":"internal""#),
            "carries the exit class: {json}"
        );
        assert!(
            json.contains(r#""detail":"write failed""#),
            "carries the message: {json}"
        );
        assert_eq!(note_line(Format::Human, Exit::Internal, "x"), "keryx: x");
    }

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
        let exit = write_product(
            Format::Human,
            &mut FailingWriter(io::ErrorKind::BrokenPipe),
            "product",
        );
        assert_eq!(exit, Exit::Success);
    }

    #[test]
    fn any_other_write_error_is_internal() {
        let exit = write_product(
            Format::Human,
            &mut FailingWriter(io::ErrorKind::PermissionDenied),
            "product",
        );
        assert_eq!(exit, Exit::Internal);
    }

    #[test]
    fn a_good_write_delivers_the_product() {
        let mut buffer = Vec::new();
        let exit = write_product(Format::Human, &mut buffer, "hello");
        assert_eq!(exit, Exit::Success);
        assert_eq!(buffer, b"hello");
    }
}
