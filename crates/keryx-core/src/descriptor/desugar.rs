//! The engine-side de-sugaring (§20): prost-reflect descriptors → keryx model
//! fragments. Every function here is private to `descriptor` — this is the inside
//! of the descriptor-engine boundary. keryx reads *resolved features*, never the
//! syntax era, for translation-bearing facts: presence rides prost-reflect's
//! `supports_presence`/`is_required` (§5); the era is read only to *resolve* enum
//! openness, which prost-reflect exposes no accessor for (§7.4).

use prost_reflect::{FieldDescriptor, FileDescriptor, Kind, Syntax};

use super::model::{FqName, MapKey, Openness, Presence, Scalar, SchemaVersion, ValueType};

/// A descriptor-set file keryx treats as a dependency, not a subject: the
/// well-known types and the vendored option registry. Its messages and enums are
/// in the pool (so references and options resolve) but do not become schema
/// elements or facts.
pub(super) fn is_dependency_file(name: &str) -> bool {
    name.starts_with("google/protobuf/") || name == "keryx/options.proto"
}

/// The file's declared version (§5, §20). prost-reflect's `Syntax` distinguishes
/// proto2 from proto3; an editions file never reaches here — `decode` refuses it up
/// front (`UnsupportedEdition`), since prost-reflect has no editions `Syntax`. A
/// distinct `Edition` version and the `enum_type` override arrive with the editions
/// increment.
pub(super) fn version(file: &FileDescriptor) -> SchemaVersion {
    match file.syntax() {
        Syntax::Proto2 => SchemaVersion::Proto2,
        Syntax::Proto3 => SchemaVersion::Proto3,
    }
}

/// Enum openness (§7.4), resolved from the version: proto2 is closed, proto3 (and
/// the editions default) open. prost-reflect exposes no openness accessor, and the
/// explicit editions `enum_type = CLOSED` override is deferred to the increment
/// that consumes open-enum semantics — recorded in `docs/proto-support.md`. Exact
/// for every proto2/proto3 fixture. Same-crate, so the match needs no wildcard; adding an
/// `Edition` version later makes this a compile error, forcing the decision.
pub(super) fn openness(version: SchemaVersion) -> Openness {
    match version {
        SchemaVersion::Proto2 => Openness::Closed,
        SchemaVersion::Proto3 => Openness::Open,
    }
}

/// Resolved presence for a *singular* field (§5): required is legacy-required,
/// else presence-tracking is explicit, else implicit. Uniform across eras —
/// prost-reflect resolved the feature.
pub(super) fn presence(field: &FieldDescriptor) -> Presence {
    if field.is_required() {
        Presence::LegacyRequired
    } else if field.supports_presence() {
        Presence::Explicit
    } else {
        Presence::Implicit
    }
}

/// A field's value type (§6): the scalar kind, or a message/enum reference by
/// fully-qualified name. A group (delimited-encoded message, `is_group`) reaches
/// here as `Kind::Message` and de-sugars to a message reference with no special
/// case (§20).
pub(super) fn value_type(kind: &Kind) -> ValueType {
    match kind {
        Kind::Double => ValueType::Scalar(Scalar::Double),
        Kind::Float => ValueType::Scalar(Scalar::Float),
        Kind::Int32 => ValueType::Scalar(Scalar::Int32),
        Kind::Int64 => ValueType::Scalar(Scalar::Int64),
        Kind::Uint32 => ValueType::Scalar(Scalar::Uint32),
        Kind::Uint64 => ValueType::Scalar(Scalar::Uint64),
        Kind::Sint32 => ValueType::Scalar(Scalar::Sint32),
        Kind::Sint64 => ValueType::Scalar(Scalar::Sint64),
        Kind::Fixed32 => ValueType::Scalar(Scalar::Fixed32),
        Kind::Fixed64 => ValueType::Scalar(Scalar::Fixed64),
        Kind::Sfixed32 => ValueType::Scalar(Scalar::Sfixed32),
        Kind::Sfixed64 => ValueType::Scalar(Scalar::Sfixed64),
        Kind::Bool => ValueType::Scalar(Scalar::Bool),
        Kind::String => ValueType::Scalar(Scalar::String),
        Kind::Bytes => ValueType::Scalar(Scalar::Bytes),
        Kind::Message(message) => ValueType::Message(FqName::new(message.full_name())),
        Kind::Enum(enumeration) => ValueType::Enum(FqName::new(enumeration.full_name())),
    }
}

/// A map key kind (§7.2): protobuf restricts map keys to integral, bool, or string.
/// `None` for any other kind — unreachable for a compiler-produced set, but returned
/// rather than panicked so an adversarial set that decodes stays total (§6); the
/// caller composes a `MalformedDescriptor` diagnostic.
pub(super) fn map_key(kind: &Kind) -> Option<MapKey> {
    Some(match kind {
        Kind::Int32 => MapKey::Int32,
        Kind::Int64 => MapKey::Int64,
        Kind::Uint32 => MapKey::Uint32,
        Kind::Uint64 => MapKey::Uint64,
        Kind::Sint32 => MapKey::Sint32,
        Kind::Sint64 => MapKey::Sint64,
        Kind::Fixed32 => MapKey::Fixed32,
        Kind::Fixed64 => MapKey::Fixed64,
        Kind::Sfixed32 => MapKey::Sfixed32,
        Kind::Sfixed64 => MapKey::Sfixed64,
        Kind::Bool => MapKey::Bool,
        Kind::String => MapKey::String,
        _ => return None,
    })
}
