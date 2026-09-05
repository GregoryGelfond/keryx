//! The solver-free core of keryx: the protobuf-schema-to-ASP-vocabulary
//! compiler and the protobuf-message-to-ground-facts codec, over themelios's
//! `Symbol` algebra. Every foreign input crosses a `Result` boundary returning
//! the [`diagnostics`] taxonomy's typed values — never a panic or a bare string
//! (architecture §6); that taxonomy is the foundation the ingestion spine (the
//! schema model and descriptor facts) and, later, the codec build upon. The
//! themelios binding is proven in `tests/themelios_binding.rs`.
//!
//! Design of record: `docs/design/architecture.md` (the architecture) over
//! `docs/specification.md` (the spec).
#![forbid(unsafe_code)]

pub mod diagnostics;
mod fault;
pub(crate) mod terms;
pub mod descriptor;
pub mod schema_facts;
pub mod policy;
pub mod emit;
pub mod manifest;
pub mod codec;

/// The one public path to the foreign-fault containment flag: a consumer's panic hook may consult
/// `keryx_core::is_containing()` to stay quiet for a fault keryx returns as a value rather than a
/// panic (the threat model's dependency boundary).
pub use fault::is_containing;

/// themelios's identifier type, re-exported so keryx's public surface is self-contained: a
/// `Mapping`'s predicates and constants *are* `Name`s (R1), and a client names them through
/// keryx alone, never a direct rev-pinned dependency on `themelios-program`.
pub use themelios_program::Name;

/// themelios's ground value, re-exported for the same reason: a payload's facts *are*
/// `Symbol`s (R1) — the library seam hands a client the codec's facts as `Symbol`s directly,
/// no text between (R6) — and a client reads them through keryx alone.
pub use themelios_program::Symbol;

/// themelios's strong sign, re-exported to complete the value plane (R1): a `Symbol::Function`
/// carries its sign, so a client destructuring or building one names `Sign` — through keryx
/// alone, as it names `Symbol` and `Name`.
pub use themelios_program::Sign;
