//! The solver-free core of keryx: the protobuf-schema-to-ASP-vocabulary
//! compiler and the protobuf-message-to-ground-facts codec, over themelios's
//! `Symbol` algebra. Every foreign input crosses a `Result` boundary returning
//! the [`diagnostics`] taxonomy's typed values — never a panic or a bare string
//! (architecture §6); that taxonomy is the foundation the ingestion spine (the
//! schema model and descriptor facts) and, later, the codec build upon. The
//! themelios binding is proven in `tests/themelios_binding.rs`.
#![forbid(unsafe_code)]

pub mod diagnostics;
pub mod descriptor;
pub mod facts;
pub mod policy;
pub mod emit;
pub mod manifest;
