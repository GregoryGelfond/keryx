//! Foreign-fault containment (threat model, *The dependency boundary*): the one seam where an
//! unforeseen unwinding fault in foreign code on a foreign-input path becomes a typed
//! [`DiagnosticKind::DependencyFault`] value rather than unwinding into keryx's caller. keryx's own
//! logic stays total by construction (architecture §6); this seam holds only what crosses into code
//! keryx does not own — today, prost-reflect decoding and walking a descriptor set (the source door
//! extends it as it lands). The split
//! is asymmetric: a keryx bug stays a panic (the CLI maps it to `Internal`), an upstream fault
//! becomes a value. A thread-local flag ([`is_containing`]) records whether a containment frame is
//! live, so a consumer's panic hook may stay quiet for a fault keryx will return as a value.
//!
//! [`DiagnosticKind::DependencyFault`]: crate::diagnostics::DiagnosticKind::DependencyFault

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

thread_local! {
    static CONTAINING: Cell<bool> = const { Cell::new(false) };
}

/// Whether a `contain` frame is live on this thread's stack — true iff at least one, held under
/// nesting. A consumer's panic hook may consult it to stay quiet for a fault keryx returns as a
/// value; a consumer with Rust's default hook still sees `panicked at …` and silences it only by
/// consulting this.
#[must_use]
pub fn is_containing() -> bool {
    CONTAINING.with(Cell::get)
}

/// Run `call`, containing an unwinding panic in it as a [`DiagnosticKind::DependencyFault`] value
/// naming `dependency` and `operation`, rather than letting it unwind into keryx's caller. The one
/// seam that crosses into foreign code on a foreign-input path; keryx's own logic never panics on
/// foreign input, so this is not a general panic net. Save/restore of the thread-local flag holds
/// its invariant under nesting: it is set for the duration of `call` and restored to its prior
/// value afterwards, on the clean and the panicking path alike.
///
/// # Precondition (discharged per call site)
/// `call` captures no state observed after a panic, **and** the dependency it calls holds no
/// process-global mutable state a panic could leave inconsistent — only then is the
/// `AssertUnwindSafe` sound. And `call`'s own keryx-side logic must itself be **total**: a keryx
/// panic inside the frame is attributed to `dependency` (and a hook consulting [`is_containing`]
/// stays silent for it), so a frame enclosing keryx logic states that it holds no `unwrap`/`expect`
/// of its own. Each call site states its discharge in one line.
pub(crate) fn contain<T>(
    dependency: &str,
    operation: &str,
    call: impl FnOnce() -> T,
) -> Result<T, Diagnostics> {
    let prior = CONTAINING.replace(true);
    let result = catch_unwind(AssertUnwindSafe(call));
    CONTAINING.set(prior);
    result.map_err(|payload| fault(dependency, operation, message(&*payload)))
}

/// The panic payload's message, when it is the usual `&str` or `String` that `panic!` carried; an
/// exotic payload yields `None`, and the fault still names the dependency and operation.
fn message(payload: &(dyn std::any::Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
}

/// Compose the [`DiagnosticKind::DependencyFault`] naming the dependency, the operation, and the
/// panic message when there is one (§6). The whole-input locus: an engine fault is the input's as a
/// whole, with no finer proto path.
fn fault(dependency: &str, operation: &str, message: Option<&str>) -> Diagnostics {
    let detail = match message {
        Some(message) => {
            format!("the {dependency} dependency faulted while {operation}: {message}")
        }
        None => format!("the {dependency} dependency faulted while {operation}"),
    };
    Diagnostic::new(DiagnosticKind::DependencyFault, Locus::whole(), detail).into()
}

#[cfg(test)]
mod tests {
    use crate::diagnostics::DiagnosticKind;

    use super::{contain, is_containing};

    #[test]
    fn a_panicking_call_becomes_a_dependency_fault() {
        let diagnostics =
            contain("test-engine", "probing", || -> u8 { panic!("boom") }).unwrap_err();
        let diagnostic = diagnostics.iter().next().unwrap();
        assert_eq!(diagnostic.kind(), DiagnosticKind::DependencyFault);
        assert!(
            diagnostic.detail().contains("test-engine")
                && diagnostic.detail().contains("probing")
                && diagnostic.detail().contains("boom"),
            "the fault names the dependency, the operation, and the payload: {diagnostic}"
        );
        assert!(
            !is_containing(),
            "the flag is cleared once the frame returns"
        );
    }

    #[test]
    fn a_clean_call_passes_through() {
        assert_eq!(contain("e", "op", || 42u8).unwrap(), 42);
    }

    #[test]
    fn the_flag_is_set_during_the_call() {
        assert!(contain("e", "op", is_containing).unwrap());
    }

    #[test]
    fn nesting_preserves_the_flag() {
        // A contain inside a contain: the flag is still set after the inner returns, and cleared
        // only after the outer — the save/restore invariant, held under nesting.
        let after_inner = contain("outer", "o", || {
            let _ = contain("inner", "i", || ());
            is_containing()
        })
        .unwrap();
        assert!(
            after_inner,
            "the outer frame keeps the flag set after the inner clears"
        );
        assert!(!is_containing());
    }
}
