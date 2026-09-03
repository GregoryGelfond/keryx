//! Stable, class-distinguishing process exit codes (architecture §6) — the single home of the
//! integers, and the top-level panic containment that maps an escaped panic to one. Variants
//! are added as commands need them (the values fixed); the later `Admission`/`Shape`
//! classes land with their increments.

use std::io::Write as _;
use std::process::{ExitCode, Termination};

use keryx_core::diagnostics::{DiagnosticKind, Diagnostics, wire_object};

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
    /// A dependency fault — an unforeseen fault in foreign code (the descriptor engine, the source
    /// compiler) contained on a foreign-input path (the threat model's dependency boundary). Neither
    /// a keryx bug (`Internal`) nor a user's schema error (`Schema`): the input provoked an engine
    /// fault keryx caught and returned as a value. `5`/`6` are reserved for the `Admission`/`Shape`
    /// classes their increments land; the integer is tunable and named here alone.
    Dependency = 7,
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
            Exit::Dependency => "dependency",
        }
    }

    /// Classify a run by its diagnostics: a [`DiagnosticKind::DependencyFault`] anywhere in the
    /// collection classifies the run [`Exit::Dependency`]; otherwise the caller's `default`. The
    /// CLI's door sites route their door `Diagnostics` through this, so a contained engine fault
    /// reaches `Dependency` rather than `Schema` — while a keryx bug stays a panic mapped to
    /// `Internal`. The precedence (a dependency fault dominates) is stated for the general
    /// collection form, though a contained fault is a single cause.
    #[must_use]
    pub fn classify(default: Exit, diagnostics: &Diagnostics) -> Exit {
        if diagnostics.contains_kind(DiagnosticKind::DependencyFault) {
            Exit::Dependency
        } else {
            default
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

/// The panic report line the hook writes, or `None` when a `fault::contain` frame is live
/// (`containing`) and no backtrace was requested — a contained fault is keryx-core's to return as a
/// `DependencyFault` value, which the CLI renders, so by default the hook stays silent rather than
/// adding a false "bug in keryx" line (and, under `json`, a second array for one event). Under
/// `RUST_BACKTRACE` (`detailed`), a contained fault still emits its **location and backtrace** — which
/// the returned diagnostic's detail (the payload message alone) lacks and which the operator asked
/// for — framed as a **dependency** fault, never a keryx bug, a debugging aid opted into. A genuine
/// keryx bug — no contain frame — always yields the notice: a calm line by default, the panic's
/// location/backtrace under `RUST_BACKTRACE` (`panic_detail`), framed as a one-element wire array
/// (kind `internal`) under `json` so it does not break a consumer's parser. Pure — the wiring that
/// passes the live flag is held by an end-to-end test (`tests/`), not this function.
fn hook_report(
    json: bool,
    info: &dyn std::fmt::Display,
    detailed: bool,
    containing: bool,
) -> Option<String> {
    let (class, detail) = if containing {
        if !detailed {
            return None;
        }
        (
            Exit::Dependency,
            format!(
                "a contained dependency fault: {info}\n{}",
                std::backtrace::Backtrace::force_capture()
            ),
        )
    } else {
        (Exit::Internal, panic_detail(info, detailed))
    };
    Some(if json {
        format!("[{}]", wire_object("", class.slug(), &detail))
    } else {
        format!("keryx: {detail}")
    })
}

/// Run `run`, containing an escaped panic (architecture §6): a top-level hook reports the panic and
/// any panic maps to `Internal` (exit 1). A panic is a bug in keryx, not a fault in the user's input,
/// so the report says so; `hook_report` composes it. The hook consults `keryx_core::is_containing`
/// and stays **silent** for a fault keryx-core is containing to return as a value (a `DependencyFault`,
/// which the CLI renders as its diagnostic), so a contained engine fault reports **once** — the
/// diagnostic — not twice (under `RUST_BACKTRACE` it additionally emits the fault's location and
/// backtrace, framed as a dependency fault, for debugging). (A consumer with Rust's default hook
/// still sees `panicked at …` for a contained fault, and silences it only by installing a hook that
/// consults `is_containing`.) The
/// hook swallows a broken stderr pipe rather than re-panicking, so an escaped panic never
/// double-panics into an abort. The one place the process's panic posture is set, so `main` is a
/// shim over it.
#[must_use]
pub fn contain(json: bool, run: impl FnOnce() -> Exit) -> Exit {
    std::panic::set_hook(Box::new(move |info| {
        let detailed = std::env::var("RUST_BACKTRACE").is_ok_and(|value| value != "0");
        if let Some(report) = hook_report(json, info, detailed, keryx_core::is_containing()) {
            let _ = writeln!(std::io::stderr(), "{report}");
        }
    }));
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(exit) => exit,
        // The hook printed the report — a panic reaching here fired outside every `fault::contain`
        // frame (one inside a frame is caught by that frame, not here). Map to a clean exit 1.
        Err(_) => Exit::Internal,
    }
}

#[cfg(test)]
mod tests {
    use keryx_core::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

    use super::{Exit, contain, hook_report, panic_detail, wire_object};

    #[test]
    fn silent_while_containing_by_default() {
        // A live `fault::contain` frame with no backtrace requested: the hook stays silent —
        // keryx-core returns the fault as a value, which the CLI renders; a "bug in keryx" line
        // would be false.
        assert!(hook_report(false, &"panicked at x", false, true).is_none());
    }

    #[test]
    fn a_contained_fault_keeps_its_location_under_rust_backtrace() {
        // Silence is default-only: under `RUST_BACKTRACE` the operator gets the *location* of the
        // unforeseen fault (which the returned diagnostic's detail lacks), framed as a dependency
        // fault — never "bug in keryx".
        let report =
            hook_report(false, &"panicked at engine.rs:9:1", true, true).expect("a backtrace");
        assert!(
            report.contains("engine.rs:9:1"),
            "the location survives: {report}"
        );
        assert!(
            !report.contains("bug in keryx"),
            "not a keryx bug: {report}"
        );
        assert!(
            report.contains("dependency"),
            "framed as a dependency fault: {report}"
        );
    }

    #[test]
    fn the_contained_backtrace_is_a_dependency_array_under_json() {
        // Under `--format json` the debugging backtrace is a one-line `dependency` array, never
        // `internal`, so it does not masquerade as a keryx bug.
        let report = hook_report(true, &"x", true, true).expect("a report");
        assert!(report.contains(r#""kind":"dependency""#), "{report}");
        assert!(!report.contains("internal"), "{report}");
        assert!(!report.contains('\n'), "one line: {report}");
    }

    #[test]
    fn a_bug_notice_when_not_containing() {
        // A genuine keryx bug (no contain frame) yields the calm notice.
        let report = hook_report(false, &"panicked at x", false, false).expect("a report");
        assert!(report.contains("bug in keryx"), "{report}");
    }

    #[test]
    fn a_dependency_fault_classifies_dependency() {
        // A contained upstream fault reaching a door site classifies the run `Dependency` —
        // neither a keryx bug (`Internal`) nor a user's schema error (`Schema`).
        let diagnostics = Diagnostics::one(Diagnostic::new(
            DiagnosticKind::DependencyFault,
            Locus::whole(),
            "x",
        ));
        assert_eq!(Exit::classify(Exit::Schema, &diagnostics), Exit::Dependency);
    }

    #[test]
    fn without_a_dependency_fault_the_default_stands() {
        // No dependency fault in the collection: the caller's default class stands.
        let diagnostics = Diagnostics::one(Diagnostic::new(
            DiagnosticKind::MalformedDescriptor,
            Locus::whole(),
            "x",
        ));
        assert_eq!(Exit::classify(Exit::Schema, &diagnostics), Exit::Schema);
    }

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
