//! Stable, class-distinguishing process exit codes (architecture §6) — the single home of the
//! integers, and the top-level panic containment that maps an escaped panic to one. Variants
//! are added as commands need them (the values fixed); the later `Admission`/`Shape`
//! classes land with their increments.

use std::io::Write as _;
use std::process::{ExitCode, Termination};

/// The process exit code, by error class (architecture §6).
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Exit {
    /// Success — the product was produced; stderr quiet.
    Success = 0,
    /// An internal error — a bug or an escaped panic (mapped by the top-level hook).
    Internal = 1,
    /// A usage error — bad arguments (clap also exits here).
    Usage = 2,
    /// An input error — a file could not be read or written (file I/O, either direction).
    Input = 3,
    /// A schema error — the `.proto`/descriptor set did not compile, ingest, or map.
    Schema = 4,
}

impl Exit {
    /// The error class as a wire slug — the JSON `kind` for a CLI-adapter error (a file-I/O or
    /// usage failure) under `--format json`, where there is no library `DiagnosticKind` to name
    /// (§6). The lowercase class name.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            Exit::Success => "success",
            Exit::Internal => "internal",
            Exit::Usage => "usage",
            Exit::Input => "input",
            Exit::Schema => "schema",
        }
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> ExitCode {
        ExitCode::from(exit as u8)
    }
}

impl Termination for Exit {
    fn report(self) -> ExitCode {
        self.into()
    }
}

/// Run `run`, containing an escaped panic (architecture §6): a top-level hook prints a calm,
/// user-facing line — never a raw `panicked at …` backtrace — and any panic maps to `Internal`
/// (exit 1). A panic is a bug in keryx, not a fault in the user's input, and the line says so; the
/// panic detail is shown only when the user opts in with `RUST_BACKTRACE`, for a bug report. The
/// hook swallows a broken stderr pipe rather than re-panicking, so an escaped panic never
/// double-panics into an abort. The one place the process's panic posture is set, so `main` is a
/// shim over it.
#[must_use]
pub fn contain(run: impl FnOnce() -> Exit) -> Exit {
    std::panic::set_hook(Box::new(|info| {
        if std::env::var_os("RUST_BACKTRACE").is_some() {
            let _ = writeln!(std::io::stderr(), "keryx: internal error (bug): {info}");
        } else {
            let _ = writeln!(
                std::io::stderr(),
                "keryx: internal error — this is a bug in keryx, not a problem with your input; \
                 please report it (set RUST_BACKTRACE=1 for detail)"
            );
        }
    }));
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(exit) => exit,
        Err(_) => Exit::Internal, // the hook printed the friendly line; map to a clean exit 1
    }
}

#[cfg(test)]
mod tests {
    use super::{Exit, contain};

    #[test]
    fn contain_maps_a_panic_to_internal() {
        // The §6 top-level hook: a panic anywhere under `contain` becomes a clean exit 1, not a
        // raw backtrace or an abort — proven with a real panic (a command body cannot be driven
        // into one deterministically), the estate's containment-test posture. The hook prints an
        // internal-error line to stderr as it runs; that is the mechanism under test.
        assert_eq!(contain(|| panic!("boom")) as u8, Exit::Internal as u8);
    }

    #[test]
    fn contain_returns_a_clean_exit_unchanged() {
        assert_eq!(contain(|| Exit::Schema) as u8, Exit::Schema as u8);
    }
}
