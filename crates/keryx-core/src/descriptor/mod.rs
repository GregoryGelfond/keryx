//! Descriptor ingestion (architecture §3, §5; spec §20): the sole adapter over the
//! descriptor engine. `ingest` takes descriptor-set *bytes* and returns a de-sugared
//! [`Schema`] or [`Diagnostics`] — total on foreign input, with no `prost_reflect`
//! type escaping this module (the descriptor-engine boundary). The [`Schema`] model
//! is `model`; the engine-side de-sugaring lives in private submodules.
//!
//! [`Schema`]: model::Schema
//! [`Diagnostics`]: crate::diagnostics::Diagnostics

pub mod model;
pub mod source;

pub use model::{
    Annotation, AnnotationValue, Enum, EnumValue, Field, FieldShape, File, FqName, MapKey, Message,
    Oneof, Openness, Presence, Scalar, Schema, SchemaVersion, ValueType,
};
pub use source::compile;

mod desugar;
mod docs;
mod options;
mod recursion;

use prost_reflect::{
    DescriptorPool, EnumDescriptor, EnumValueDescriptor, FieldDescriptor, FileDescriptor, Kind,
    MessageDescriptor, OneofDescriptor,
};

use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

/// Ingest a serialized `FileDescriptorSet` into a de-sugared [`Schema`] (§20). The
/// sole public entry of the descriptor-engine boundary: bytes in, keryx types out,
/// total on foreign input — a set that does not decode, or that decodes but carries
/// a structurally-malformed element, yields diagnostics, never a panic and never a
/// partial schema; the first structural diagnosis short-circuits the walk. Custom
/// options are read only through the dynamic layer, so no annotation is ever
/// silently dropped (§20). The model is assembled in one walk over the set —
/// resolving options and doc comments per element — then ordered deterministically
/// and analysed for containment cycles.
///
/// # Errors
///
/// Returns [`Diagnostics`] when the bytes do not decode as a `FileDescriptorSet` or
/// an import is unresolved (`UnreadableDescriptorSet`), when a decodable descriptor
/// violates a protobuf structural invariant (`MalformedDescriptor`), or when a
/// custom option carries an unlowerable value (`MalformedOption`).
///
/// [`Schema`]: model::Schema
/// [`Diagnostics`]: crate::diagnostics::Diagnostics
pub fn ingest(bytes: &[u8]) -> Result<Schema, Diagnostics> {
    let pool = decode(bytes)?;
    build_schema(&pool, |name| !desugar::is_dependency_file(name))
}

/// Ingest a serialized `FileDescriptorSet`, treating exactly the files named in
/// `subjects` (by their descriptor `name`) as subjects — the front door's entry, where the
/// opened (root) files are known, so a subject *named* like a well-known type (the §21.2
/// self-application on `google/protobuf/descriptor.proto`) is ingested, not skipped. Total
/// (§6), as [`ingest`].
///
/// # Errors
///
/// As [`ingest`].
pub(crate) fn ingest_subjects(bytes: &[u8], subjects: &[String]) -> Result<Schema, Diagnostics> {
    let pool = decode(bytes)?;
    build_schema(&pool, |name| subjects.iter().any(|subject| subject == name))
}

/// Decode a serialized `FileDescriptorSet` into a `DescriptorPool`, or the typed reason it did
/// not (`UnreadableDescriptorSet`, §6) — the one decode door both `ingest` paths share.
fn decode(bytes: &[u8]) -> Result<DescriptorPool, Diagnostics> {
    DescriptorPool::decode(bytes).map_err(|error| {
        Diagnostic::new(
            DiagnosticKind::UnreadableDescriptorSet,
            Locus::whole(),
            error.to_string(),
        )
        .into()
    })
}

/// Assemble the schema from the pool, over the subject files only (dependencies —
/// well-known types, the option registry — stay in the pool but are not subjects).
/// `is_subject` decides which pool files are subjects — the bytes-only `ingest` path
/// uses the well-known-name heuristic (`desugar::is_dependency_file`); the front door
/// (`ingest_subjects`) instead carries the real, explicitly-opened subject set across
/// the `compile → ingest` seam, so a subject named like a well-known type is not
/// silently dropped (§21.2). The per-file message walk is computed once and shared by
/// the message and enum passes. Deterministically ordered (P3).
fn build_schema(
    pool: &DescriptorPool,
    is_subject: impl Fn(&str) -> bool,
) -> Result<Schema, Diagnostics> {
    let mut files = Vec::new();
    let mut messages = Vec::new();
    let mut enums = Vec::new();

    for file in pool.files() {
        if !is_subject(file.name()) {
            continue;
        }
        let version = desugar::version(&file);
        files.push(File {
            name: file.name().to_owned(),
            package: file.package_name().to_owned(),
        });
        let file_messages = subject_messages(&file);
        for message in &file_messages {
            if message.is_map_entry() {
                continue;
            }
            messages.push(build_message(message, file.name())?);
        }
        for enumeration in subject_enums(&file, &file_messages) {
            enums.push(build_enum(&enumeration, file.name(), version)?);
        }
    }

    files.sort_by(|a, b| a.name.cmp(&b.name));
    messages.sort_by(|a, b| a.path.cmp(&b.path));
    enums.sort_by(|a, b| a.path.cmp(&b.path));
    recursion::mark(&mut messages);
    Ok(Schema {
        files,
        messages,
        enums,
    })
}

/// Every message under a file — top level and nested, in one flat list (order is
/// fixed by the caller's sort). Map-entry synthetics are included here and filtered
/// at the call site.
fn subject_messages(file: &FileDescriptor) -> Vec<MessageDescriptor> {
    let mut out = Vec::new();
    for message in file.messages() {
        collect_messages(message, &mut out);
    }
    out
}

fn collect_messages(message: MessageDescriptor, out: &mut Vec<MessageDescriptor>) {
    for child in message.child_messages() {
        collect_messages(child, out);
    }
    out.push(message);
}

/// Every enum under a file — top level and nested within any message; the message
/// walk is passed in rather than recomputed.
fn subject_enums(file: &FileDescriptor, messages: &[MessageDescriptor]) -> Vec<EnumDescriptor> {
    let mut out: Vec<EnumDescriptor> = file.enums().collect();
    for message in messages {
        out.extend(message.child_enums());
    }
    out
}

fn build_message(message: &MessageDescriptor, file: &str) -> Result<Message, Diagnostics> {
    let mut fields = Vec::new();
    for field in message.fields() {
        fields.push(build_field(&field)?);
    }
    fields.sort_by_key(|field| field.number);

    let mut oneofs: Vec<Oneof> = message
        .oneofs()
        .filter(|oneof| !oneof.is_synthetic())
        .map(|oneof| build_oneof(&oneof))
        .collect::<Result<Vec<_>, Diagnostics>>()?;
    oneofs.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Message {
        path: FqName::new(message.full_name()),
        file: file.to_owned(),
        outer: message.parent_message().map(|p| FqName::new(p.full_name())),
        fields,
        oneofs,
        options: options::read(&message.options(), message.full_name())?,
        doc: docs::for_path(&message.parent_file(), message.path()),
        recursive: false,
    })
}

fn build_field(field: &FieldDescriptor) -> Result<Field, Diagnostics> {
    let shape = if field.is_map() {
        map_shape(field)?
    } else if field.is_list() {
        FieldShape::Repeated {
            value: desugar::value_type(&field.kind()),
        }
    } else {
        FieldShape::Singular {
            value: desugar::value_type(&field.kind()),
            presence: desugar::presence(field),
        }
    };
    Ok(Field {
        number: field_number(field)?,
        name: field.name().to_owned(),
        path: FqName::new(field.full_name()),
        shape,
        options: options::read(&field.options(), field.full_name())?,
        doc: docs::for_path(&field.parent_file(), field.path()),
    })
}

/// The de-sugared map shape (§7.2), total over an adversarial descriptor: a map
/// field whose kind is not its synthetic entry message, an entry missing key #1 or
/// value #2, or a non-key key kind, each composes a `MalformedDescriptor` at the
/// field rather than panicking (§6 — no panic on foreign input).
fn map_shape(field: &FieldDescriptor) -> Result<FieldShape, Diagnostics> {
    let malformed = |detail: &str| {
        Diagnostics::from(Diagnostic::new(
            DiagnosticKind::MalformedDescriptor,
            Locus::at(field.full_name()),
            detail,
        ))
    };
    let Kind::Message(entry) = field.kind() else {
        return Err(malformed("a map field's kind is not its entry message"));
    };
    let (Some(key_field), Some(value_field)) = (entry.get_field(1), entry.get_field(2)) else {
        return Err(malformed("a map entry is missing its key or value field"));
    };
    let Some(key) = desugar::map_key(&key_field.kind()) else {
        return Err(malformed(
            "a map key is not an integral, bool, or string kind",
        ));
    };
    Ok(FieldShape::Map {
        key,
        value: desugar::value_type(&value_field.kind()),
    })
}

/// A proto field number as `i32` (the descriptor stores it as `int32`, so a
/// well-formed number fits); an out-of-range number from an adversarial set
/// composes `MalformedDescriptor` rather than panicking (§6).
fn field_number(field: &FieldDescriptor) -> Result<i32, Diagnostics> {
    i32::try_from(field.number()).map_err(|_| {
        Diagnostics::from(Diagnostic::new(
            DiagnosticKind::MalformedDescriptor,
            Locus::at(field.full_name()),
            "field number out of range",
        ))
    })
}

fn build_oneof(oneof: &OneofDescriptor) -> Result<Oneof, Diagnostics> {
    let mut arms = oneof
        .fields()
        .map(|field| field_number(&field))
        .collect::<Result<Vec<i32>, Diagnostics>>()?;
    arms.sort_unstable();
    Ok(Oneof {
        name: oneof.name().to_owned(),
        path: FqName::new(oneof.full_name()),
        arms,
        doc: docs::for_path(&oneof.parent_file(), oneof.path()),
    })
}

fn build_enum(
    enumeration: &EnumDescriptor,
    file: &str,
    version: SchemaVersion,
) -> Result<Enum, Diagnostics> {
    let mut values = enumeration
        .values()
        .map(|value| build_enum_value(&value))
        .collect::<Result<Vec<EnumValue>, Diagnostics>>()?;
    values.sort_by_key(|value| value.number);
    Ok(Enum {
        path: FqName::new(enumeration.full_name()),
        file: file.to_owned(),
        outer: enumeration
            .parent_message()
            .map(|p| FqName::new(p.full_name())),
        openness: desugar::openness(version),
        values,
        options: options::read(&enumeration.options(), enumeration.full_name())?,
        doc: docs::for_path(&enumeration.parent_file(), enumeration.path()),
    })
}

fn build_enum_value(value: &EnumValueDescriptor) -> Result<EnumValue, Diagnostics> {
    Ok(EnumValue {
        name: value.name().to_owned(),
        number: value.number(),
        path: FqName::new(value.full_name()),
        options: options::read(&value.options(), value.full_name())?,
        doc: docs::for_path(&value.parent_file(), value.path()),
    })
}
