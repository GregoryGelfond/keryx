//! Foreign-fault containment (threat model, *The dependency boundary*): the one seam where an
//! unforeseen unwinding fault in foreign code on a foreign-input path becomes a typed
//! [`DiagnosticKind::DependencyFault`] value rather than unwinding into keryx's caller. keryx's own
//! logic stays total by construction (architecture §6); this seam holds only what crosses into code
//! keryx does not own — the closed set of foreign dependencies enumerated as [`Dependency`], each
//! variant naming the doors it is contained at (a door over a new dependency adds a variant as it
//! lands). The split is asymmetric: a keryx bug stays a panic (the CLI maps it to `Internal`), an
//! upstream fault becomes a value. A thread-local flag ([`is_containing`]) records whether a
//! containment frame is live, so a consumer's panic hook may stay quiet for a fault keryx will return
//! as a value.
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

/// A foreign dependency keryx contains at [`contain`] — the closed set of code keryx is a client of on
/// a foreign-input path. Enumerated as a type, not spread as string literals across the call sites, so
/// the seam's coverage is readable from one place, each crate name is spelled once (a typo cannot
/// split one dependency into two on the wire), and the module doc names one referent rather than
/// restating the list; a new door adds a variant here. Each variant's doc names the door it is
/// contained at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dependency {
    /// prost-types — the descriptor door's pre-read (`descriptor::pre_validate`, a plain decode).
    ProstTypes,
    /// prost-reflect — the descriptor engine, at both the pool decode and the accessor walk; and
    /// the payload engine, at the payload door's binary decode (`codec::engine::decode_binary`)
    /// and its textproto parse (`codec::engine::decode_textproto`).
    ProstReflect,
    /// protox — the `.proto` source compiler, at the source door's compile.
    Protox,
}

impl Dependency {
    /// The crate's name, as it reads in a `DependencyFault` detail — spelled once, here.
    fn crate_name(self) -> &'static str {
        match self {
            Dependency::ProstTypes => "prost-types",
            Dependency::ProstReflect => "prost-reflect",
            Dependency::Protox => "protox",
        }
    }
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
    dependency: Dependency,
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
fn fault(dependency: Dependency, operation: &str, message: Option<&str>) -> Diagnostics {
    let name = dependency.crate_name();
    let detail = match message {
        Some(message) => {
            format!("the {name} dependency faulted while {operation}: {message}")
        }
        None => format!("the {name} dependency faulted while {operation}"),
    };
    Diagnostic::new(DiagnosticKind::DependencyFault, Locus::whole(), detail).into()
}

#[cfg(test)]
mod tests {
    use crate::diagnostics::DiagnosticKind;

    use super::{Dependency, contain, is_containing};

    #[test]
    fn a_panicking_call_becomes_a_dependency_fault() {
        let diagnostics = contain(Dependency::ProstReflect, "probing", || -> u8 {
            panic!("boom")
        })
        .unwrap_err();
        let diagnostic = diagnostics.iter().next().unwrap();
        assert_eq!(diagnostic.kind(), DiagnosticKind::DependencyFault);
        assert!(
            diagnostic.detail().contains("prost-reflect")
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
        assert_eq!(contain(Dependency::ProstTypes, "op", || 42u8).unwrap(), 42);
    }

    #[test]
    fn the_flag_is_set_during_the_call() {
        assert!(contain(Dependency::ProstTypes, "op", is_containing).unwrap());
    }

    #[test]
    fn nesting_preserves_the_flag() {
        // A contain inside a contain: the flag is still set after the inner returns, and cleared
        // only after the outer — the save/restore invariant, held under nesting.
        let after_inner = contain(Dependency::ProstReflect, "o", || {
            let _ = contain(Dependency::ProstTypes, "i", || ());
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
