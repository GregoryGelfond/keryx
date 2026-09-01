//! The keryx command-line frontend — a thin satellite that composes the library
//! (architecture §3, §6): stdout is the product, stderr is diagnostics, exit codes
//! are stable and class-distinguishing. Increment 1 adds one internal command,
//! `schema-facts` — the M0 deliverable — dumping a descriptor set's Appendix C
//! facts; the full CLI surface (§25) lands from Increment 2. Argument handling is
//! hand-rolled for this single command; clap arrives with the multi-command surface.
#![forbid(unsafe_code)]

use std::process::ExitCode;

use keryx_core::descriptor::ingest;
use keryx_core::diagnostics::Diagnostics;
use keryx_core::facts;

/// Stable, class-distinguishing process exit codes (architecture §6) — the single
/// home of the integers. The §6 taxonomy (`Schema`, `Admission`, `Shape`,
/// `DomainUnsat`, …) grows here as later commands need it, these values fixed.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Exit {
    Success = 0,
    Internal = 1,
    Usage = 2,
    Input = 3,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> ExitCode {
        ExitCode::from(exit as u8)
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("schema-facts") => match args.next() {
            Some(path) => schema_facts(&path).into(),
            None => usage("schema-facts <descriptor-set.binpb>"),
        },
        Some(other) => usage(&format!("unknown command `{other}`")),
        None => usage("<command> [args]   (commands: schema-facts)"),
    }
}

/// Read a serialized `FileDescriptorSet` and write its descriptor facts to stdout;
/// diagnostics go to stderr, so stdout stays clean for `keryx schema-facts s | clingo`.
fn schema_facts(path: &str) -> Exit {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return note(Exit::Input, &format!("cannot read {path}: {error}")),
    };
    let schema = match ingest(&bytes) {
        Ok(schema) => schema,
        Err(diagnostics) => return report(Exit::Input, &diagnostics),
    };
    match facts::render(&schema) {
        Ok(text) => {
            print!("{text}");
            Exit::Success
        }
        Err(diagnostics) => report(Exit::Internal, &diagnostics),
    }
}

fn report(exit: Exit, diagnostics: &Diagnostics) -> Exit {
    for diagnostic in diagnostics.iter() {
        eprintln!("keryx: {diagnostic}");
    }
    exit
}

fn note(exit: Exit, message: &str) -> Exit {
    eprintln!("keryx: {message}");
    exit
}

fn usage(message: &str) -> ExitCode {
    eprintln!("keryx: usage: {message}");
    Exit::Usage.into()
}
