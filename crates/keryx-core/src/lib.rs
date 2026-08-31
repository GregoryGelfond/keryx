//! The solver-free core of keryx: the protobuf-schema-to-ASP-vocabulary
//! compiler and the protobuf-message-to-ground-facts codec, over themelios's
//! `Symbol` algebra. This crate holds no logic yet — Increment 0 is the
//! walking skeleton; the binding to themelios is proven in
//! `tests/themelios_binding.rs`.
#![forbid(unsafe_code)]
