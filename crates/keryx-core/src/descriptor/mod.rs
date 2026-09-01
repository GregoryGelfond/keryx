//! Descriptor ingestion (architecture §3, §5; spec §20): the sole adapter over the
//! descriptor engine. It will expose `ingest` — descriptor-set *bytes* in, a
//! de-sugared [`Schema`] or [`Diagnostics`] out — with no `prost_reflect` type
//! escaping this module (the descriptor-engine boundary); the ingest pass and its
//! de-sugaring submodules land with the ingestion step. This step establishes the
//! [`Schema`] model (`model`), the module's stable interface.
//!
//! [`Schema`]: model::Schema
//! [`Diagnostics`]: crate::diagnostics::Diagnostics

pub mod model;

pub use model::{
    Annotation, AnnotationValue, Enum, EnumValue, Field, FieldShape, File, FqName, MapKey, Message,
    Oneof, Openness, Presence, Scalar, Schema, SchemaVersion, ValueType,
};
