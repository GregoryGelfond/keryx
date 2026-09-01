//! Stable, class-distinguishing process exit codes (architecture §6) — the single home of
//! the integers. Variants are added as commands need them (the values fixed); the later
//! `Admission`/`Shape`/`DomainUnsat` classes land with their increments.

use std::process::ExitCode;

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

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> ExitCode {
        ExitCode::from(exit as u8)
    }
}
