//! Stable, class-distinguishing process exit codes (architecture §6) — the single home of the
//! integers, and the top-level panic containment that maps an escaped panic to one. Variants
//! are added as commands need them (the values fixed); the later `Admission`/`Shape`
//! classes land with their increments.

use std::io::Write as _;
use std::process::{ExitCode, Termination};

use keryx_core::diagnostics::wire_object;

/// The process exit code, by error class (architecture §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// The calm bug notice keryx prints when it panics (architecture §6) — no raw `panicked at …`
/// backtrace by default. The one moment more detail is wanted is a bug report, so it says how to
/// get it.
const PANIC_BUG_NOTICE: &str = "internal error — this is a bug in keryx, not a problem with your \
     input; please report it (set RUST_BACKTRACE=1 for a backtrace)";

/// The panic report's detail text, composed as a pure value so the "no raw `panicked at` by
/// default" property is a unit test rather than a prose claim (architecture §6). Calm by default;
/// under `RUST_BACKTRACE` it is the panic's own message, location, and a real backtrace, for a bug
/// report. [`contain`] frames this as a `keryx:` line (human) or a wire `detail` (JSON) — one text
/// either way, so both forms honor the notice's `RUST_BACKTRACE=1` cue.
fn panic_detail(info: &dyn std::fmt::Display, detailed: bool) -> String {
    if detailed {
        format!(
            "internal error (bug): {info}\n{}",
            std::backtrace::Backtrace::force_capture()
        )
    } else {
        PANIC_BUG_NOTICE.to_owned()
    }
}

/// Run `run`, containing an escaped panic (architecture §6): a top-level hook reports the panic and
/// any panic maps to `Internal` (exit 1). A panic is a bug in keryx, not a fault in the user's
/// input; by default the report is a calm line saying so — never a raw `panicked at …` backtrace —
/// and under `RUST_BACKTRACE` it is the panic's message, location, and a real backtrace, for a bug
/// report (`panic_detail` composes that text). `json` (the resolved `--format`) frames it as a
/// one-element wire array (kind `internal`) instead of a `keryx:` line, so a keryx bug does not
/// break a JSON consumer's parser — the backtrace rides in the escaped `detail`, valid on one line.
/// The hook swallows a broken stderr pipe rather than re-panicking, so an escaped panic never
/// double-panics into an abort. The one place the process's panic posture is set, so `main` is a
/// shim over it.
#[must_use]
pub fn contain(json: bool, run: impl FnOnce() -> Exit) -> Exit {
    std::panic::set_hook(Box::new(move |info| {
        let detailed = std::env::var("RUST_BACKTRACE").is_ok_and(|value| value != "0");
        let detail = panic_detail(info, detailed);
        let report = if json {
            format!("[{}]", wire_object("", Exit::Internal.slug(), &detail))
        } else {
            format!("keryx: {detail}")
        };
        let _ = writeln!(std::io::stderr(), "{report}");
    }));
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(exit) => exit,
        Err(_) => Exit::Internal, // the hook printed the report; map to a clean exit 1
    }
}

#[cfg(test)]
mod tests {
    use super::{Exit, contain, panic_detail, wire_object};

    #[test]
    fn a_calm_report_is_a_bug_notice_without_the_raw_panic_text() {
        // The §6 "no scary line" property, made executable: the default (no RUST_BACKTRACE) report
        // never echoes the panic's `panicked at …` text, and names it a keryx bug. A regression
        // that printed the panic info by default fails here.
        let info = "panicked at src/foo.rs:12:5:\nboom";
        let calm = panic_detail(&info, false);
        assert!(
            !calm.contains("panicked at"),
            "no raw panic text in the calm report: {calm}"
        );
        assert!(
            calm.contains("bug in keryx"),
            "the calm report names it a keryx bug: {calm}"
        );
    }

    #[test]
    fn a_detailed_report_carries_the_panic_location() {
        // Under RUST_BACKTRACE the report echoes the panic's own message and location.
        let info = "panicked at src/foo.rs:12:5";
        let detailed = panic_detail(&info, true);
        assert!(
            detailed.contains("src/foo.rs:12:5"),
            "the detailed report carries the location: {detailed}"
        );
    }

    #[test]
    fn a_detailed_report_carries_a_backtrace() {
        // The detailed report adds a real backtrace, not just the location. `info` is single-line
        // on purpose: without the backtrace the detail is one line, so this fails if
        // `panic_detail`'s detailed branch ever dropped `force_capture`.
        let info = "panicked at src/foo.rs:12:5";
        let detailed = panic_detail(&info, true);
        assert!(
            detailed.lines().count() > 1,
            "the detailed report includes a backtrace, not just the location: {detailed}"
        );
    }

    #[test]
    fn the_json_panic_report_is_one_valid_line_even_with_a_multiline_detail() {
        // The JSON `Internal` path must not break a consumer's parser: a multi-line detail (the
        // detailed report carries a backtrace) is escaped into a single-line one-element wire array.
        let info = "panicked at src/foo.rs:12:5:\nboom";
        let json = format!(
            "[{}]",
            wire_object("", Exit::Internal.slug(), &panic_detail(&info, true))
        );
        assert!(
            !json.contains('\n'),
            "the JSON panic report is a single line: {json}"
        );
        assert!(
            json.contains(r#""kind":"internal""#),
            "the JSON panic report names the internal class: {json}"
        );
    }

    #[test]
    fn contain_maps_a_panic_to_internal_and_passes_clean_exits() {
        // `contain` installs a process-global panic hook (main's one-time setup). This one test
        // drives both a real panic and a clean exit through it and checks only the mapping — the
        // report wording is `panic_detail`'s to prove. Merged into one test, the hook saved and
        // restored once, so the global install does not bleed into sibling tests in this binary.
        let saved = std::panic::take_hook();
        let panicked = contain(false, || panic!("boom"));
        let clean = contain(false, || Exit::Schema);
        std::panic::set_hook(saved);
        assert_eq!(panicked, Exit::Internal);
        assert_eq!(clean, Exit::Schema);
    }
}
