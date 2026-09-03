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

use prost::Message as _;
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
/// an import is unresolved (`UnreadableDescriptorSet`), when the set declares a Protobuf
/// edition the descriptor engine cannot yet read (`UnsupportedEdition`), when a decodable
/// descriptor violates a protobuf structural invariant (`MalformedDescriptor`), or when a
/// custom option carries an unlowerable value (`MalformedOption`).
///
/// [`Schema`]: model::Schema
/// [`Diagnostics`]: crate::diagnostics::Diagnostics
pub fn ingest(bytes: &[u8]) -> Result<Schema, Diagnostics> {
    let pool = decode(bytes)?;
    walk(&pool, |name| !desugar::is_dependency_file(name))
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
    walk(&pool, |name| subjects.iter().any(|subject| subject == name))
}

/// Walk the built pool into a [`Schema`], containing an unforeseen accessor fault as a
/// `DependencyFault` (the threat model's dependency boundary). The post-decode walk reads the engine's
/// descriptor through its accessors — `messages()`, `field.kind()`, and `options()`, which in
/// prost-reflect is a **lazy decode** — any of which can fault on a decodable-but-adversarial set.
/// keryx holds **no `unwrap`/`expect` of its own** across the walk (`build_schema`, `desugar`, `docs`,
/// `options`, `recursion`), so a fault here is the engine's accessor faulting — correctly a
/// `DependencyFault`, not a misattributed keryx bug. The closure borrows only the pool (nothing keryx
/// observes after a fault), and prost-reflect reads its global well-known-type pool only across an
/// infallible `Arc` clone (as at decode), so the `AssertUnwindSafe` is sound.
fn walk(pool: &DescriptorPool, is_subject: impl Fn(&str) -> bool) -> Result<Schema, Diagnostics> {
    crate::fault::contain("prost-reflect", "walking the descriptor set", || {
        build_schema(pool, is_subject)
    })?
}

/// keryx's editions refusal message, in one place — every `UnsupportedEdition` diagnostic (one per
/// editions file) carries it. Both routes fail today: protox does not parse an editions `.proto`,
/// and the descriptor engine (prost-reflect 0.16.5) has no editions `Syntax`, so a protoc-compiled
/// descriptor set is refused too. The descriptor-set route opens when the engine gains editions
/// (`docs/proto-support.md`). The front-door compile hint (`keryx-cli`) gives a brief pointer to
/// the same story; keep them consistent.
pub(crate) const EDITIONS_UNSUPPORTED: &str = "editions (edition 2023+) are not supported yet: keryx's descriptor engine has no editions \
     support, so neither a .proto source nor a protoc-compiled descriptor set is accepted. \
     Transliterate the schema to proto3, or track editions support in docs/proto-support.md";

/// Decode a serialized `FileDescriptorSet` into a `DescriptorPool`, or the typed reason it did not
/// — the one decode door both `ingest` paths share.
///
/// The descriptor engine (prost-reflect 0.16.5) **panics**, rather than returning an error, on input
/// it cannot represent: a `syntax` it has no `Syntax` for (editions, or any value that is not
/// proto2/proto3), and a package or top-level name the engine's table cannot look up (one beginning
/// with `.`). keryx does not catch *those* panics — it pre-empts the shapes it can foresee:
/// [`pre_validate`] inspects the set with a plain decode (which cannot panic) and refuses each such
/// file at its locus, so the engine only ever sees a shape it can represent. What remains is an
/// *unforeseen* engine fault — on this decode, or on the later accessor walk ([`walk`]) — which
/// crosses into code keryx does not own on a foreign-input path: each is wrapped in
/// [`crate::fault::contain`], so it becomes a typed `DependencyFault` rather than unwinding into
/// keryx's caller (the threat model's dependency boundary). Ingestion is thus total (§6) — the
/// foreseeable shapes by pre-emption, the unforeseen by containment — not by masking a panic. A set
/// that does not decode at all is the engine's own `UnreadableDescriptorSet`.
fn decode(bytes: &[u8]) -> Result<DescriptorPool, Diagnostics> {
    // The pre-read: the closure borrows only `bytes`; prost-types' decode holds no process-global
    // mutable state a panic could leave inconsistent, and `pre_validate`'s own logic is total (no
    // `unwrap`/`expect`), so a fault here is prost-types', not keryx's — the `AssertUnwindSafe` is sound.
    if let Some(diagnostics) =
        crate::fault::contain("prost-types", "inspecting the descriptor set", || {
            pre_validate(bytes)
        })?
    {
        return Err(diagnostics);
    }
    // The pool decode: the closure borrows only `bytes`. prost-reflect reads its global well-known-type
    // pool — here and at accessor time — only across an infallible `Arc` clone, never across panic-prone
    // work, and keryx never calls the pool's global mutators, so a contained panic cannot poison it.
    // (A same-process consumer that *does* call those mutators with a panicking set is the residual,
    // recorded under the threat model's Open items.) Verified against the pinned prost-reflect.
    crate::fault::contain("prost-reflect", "decoding the descriptor set", || {
        DescriptorPool::decode(bytes)
    })?
    .map_err(|error| unreadable_set(error.to_string()))
}

/// Refuse — before the descriptor engine builds a pool — the shapes prost-reflect cannot represent
/// and **panics** on rather than rejecting, so the engine only ever sees input it can represent:
///
/// - a file whose `syntax` is not proto2/proto3 — editions (an engine capability limit,
///   `UnsupportedEdition`), or any unrecognised value such as `proto4` or an empty string (a
///   `MalformedDescriptor`); `syntax` is read raw, since the `syntax()` getter cannot tell `None`
///   from `""`; and
/// - a package or **top-level** message/enum name beginning with `.` — the engine stores the
///   scope-joined name verbatim but looks it up with one leading dot stripped, so it cannot find it
///   and panics (`MalformedDescriptor`). Nested names carry a non-empty namespace and are safe.
///
/// A plain prost-types decode (no feature resolution, so it cannot panic the way the pool build can)
/// whose typed result is discarded — a pre-read that gates the engine and feeds nothing to the schema
/// (§18/§20), as the editions check it grew from did. Returns the refusals for every offending file,
/// or `None` when the set has nothing to refuse or does not decode (the engine's own decode then
/// composes `UnreadableDescriptorSet`).
fn pre_validate(bytes: &[u8]) -> Option<Diagnostics> {
    let set = prost_types::FileDescriptorSet::decode(bytes).ok()?;
    let mut refusals: Vec<Diagnostic> = Vec::new();
    for file in &set.file {
        let malformed = |detail: String| {
            Diagnostic::new(
                DiagnosticKind::MalformedDescriptor,
                Locus::at(file.name().to_owned()),
                detail,
            )
        };
        match file.syntax.as_deref() {
            None | Some("proto2" | "proto3") => {}
            Some("editions") => refusals.push(Diagnostic::new(
                DiagnosticKind::UnsupportedEdition,
                Locus::at(file.name().to_owned()),
                EDITIONS_UNSUPPORTED,
            )),
            Some(other) => refusals.push(malformed(format!(
                "unrecognised syntax {other:?}: keryx's descriptor engine reads proto2 and proto3"
            ))),
        }
        if file.package().starts_with('.') {
            refusals.push(malformed("package name begins with '.'".to_owned()));
        }
        for name in file
            .message_type
            .iter()
            .map(prost_types::DescriptorProto::name)
            .chain(
                file.enum_type
                    .iter()
                    .map(prost_types::EnumDescriptorProto::name),
            )
        {
            if name.starts_with('.') {
                refusals.push(malformed(format!(
                    "a top-level type name begins with '.': {name:?}"
                )));
            }
        }
    }
    let mut refusals = refusals.into_iter();
    let mut diagnostics = Diagnostics::one(refusals.next()?);
    for diagnostic in refusals {
        diagnostics.push(diagnostic);
    }
    Some(diagnostics)
}

/// An `UnreadableDescriptorSet` diagnostic at the whole-input locus (§6) — the set as a whole did
/// not decode, the engine's own decode error composed into the detail.
fn unreadable_set(detail: String) -> Diagnostics {
    Diagnostic::new(
        DiagnosticKind::UnreadableDescriptorSet,
        Locus::whole(),
        detail,
    )
    .into()
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
/// at the call site. Walked with an **explicit managed stack** — heap, not the call
/// stack — so nesting depth cannot exhaust the stack (the threat model's bounded-depth
/// walks; the same idiom as `recursion::reaches_self`). Bounded by the file's message
/// count: every message reachable through `child_messages()` is pushed once.
fn subject_messages(file: &FileDescriptor) -> Vec<MessageDescriptor> {
    let mut out = Vec::new();
    let mut stack: Vec<MessageDescriptor> = file.messages().collect();
    while let Some(message) = stack.pop() {
        stack.extend(message.child_messages());
        out.push(message);
    }
    out
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

#[cfg(test)]
mod tests {
    use keryx_test_support::fault_provoking_set;
    use prost::Message as _;
    use prost_types::{
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
        MessageOptions,
    };

    use super::{DescriptorPool, ingest, subject_messages};
    use crate::diagnostics::DiagnosticKind;

    /// prost's decode recursion limit (`prost::RECURSION_LIMIT`, not public API): the descriptor door
    /// admits a lexical message chain one shallower, and refuses a `DECODE_RECURSION_LIMIT`-deep one as
    /// `UnreadableDescriptorSet`. The source door's nesting guard derives from the same limit.
    const DECODE_RECURSION_LIMIT: usize = 100;

    fn encode(files: Vec<FileDescriptorProto>) -> Vec<u8> {
        FileDescriptorSet { file: files }.encode_to_vec()
    }

    /// A file with one lexically-nested message chain `M0 { M1 { … M{depth-1} } }` — the `nested_type`
    /// chain the walk descends. Built inside-out; typed, so `from_file_descriptor_set` can build a pool
    /// past the byte-decode cap.
    fn lexical_chain_typed(depth: usize) -> FileDescriptorSet {
        let mut inner: Option<DescriptorProto> = None;
        for i in (0..depth).rev() {
            let mut message = DescriptorProto {
                name: Some(format!("M{i}")),
                ..Default::default()
            };
            if let Some(child) = inner.take() {
                message.nested_type.push(child);
            }
            inner = Some(message);
        }
        FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("chain.proto".to_owned()),
                package: Some("chain".to_owned()),
                syntax: Some("proto3".to_owned()),
                message_type: vec![inner.expect("depth >= 1")],
                ..Default::default()
            }],
        }
    }

    #[test]
    fn an_unforeseen_engine_fault_at_the_walk_is_a_dependency_fault() {
        // A real engine fault through `ingest`, at the accessor walk (`options()` is a lazy decode
        // past prost's recursion limit): contained as a `DependencyFault`, not a panic escaping as a
        // keryx bug — totality across the walk (§6, the dependency boundary).
        let diagnostics = ingest(&fault_provoking_set()).expect_err("a fault, not a schema");
        let diagnostic = diagnostics.iter().next().expect("one diagnostic");
        assert_eq!(diagnostic.kind(), DiagnosticKind::DependencyFault);
        assert!(
            diagnostic.detail().contains("prost-reflect")
                && diagnostic.detail().contains("walking"),
            "the fault names the dependency and the walk: {diagnostic}"
        );
    }

    #[test]
    fn an_unforeseen_engine_fault_at_decode_is_a_dependency_fault() {
        // A set retyping `MessageOptions` field 1 as repeated int32, then setting it: the engine
        // panics decoding the options during the pool build — contained at the decode (§6).
        let set = encode(vec![
            FileDescriptorProto {
                name: Some("google/protobuf/descriptor.proto".to_owned()),
                package: Some("google.protobuf".to_owned()),
                syntax: Some("proto3".to_owned()),
                message_type: vec![DescriptorProto {
                    name: Some("MessageOptions".to_owned()),
                    field: vec![FieldDescriptorProto {
                        name: Some("message_set_wire_format".to_owned()),
                        number: Some(1),
                        label: Some(3),  // repeated
                        r#type: Some(5), // int32
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
            FileDescriptorProto {
                name: Some("m.proto".to_owned()),
                package: Some("my".to_owned()),
                dependency: vec!["google/protobuf/descriptor.proto".to_owned()],
                message_type: vec![DescriptorProto {
                    name: Some("M".to_owned()),
                    options: Some(MessageOptions {
                        message_set_wire_format: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ]);
        let diagnostics = ingest(&set).expect_err("a fault, not a schema");
        assert_eq!(
            diagnostics.iter().next().unwrap().kind(),
            DiagnosticKind::DependencyFault
        );
    }

    #[test]
    fn an_unrecognised_syntax_is_refused_before_the_engine() {
        // prost-reflect panics on any syntax that is not proto2/proto3; keryx pre-empts it with a
        // clean `MalformedDescriptor` refusal (for a non-editions value), never letting it reach the
        // engine — where it would panic.
        for syntax in ["proto4", ""] {
            let set = encode(vec![FileDescriptorProto {
                name: Some("m.proto".to_owned()),
                syntax: Some(syntax.to_owned()),
                message_type: vec![DescriptorProto {
                    name: Some("M".to_owned()),
                    ..Default::default()
                }],
                ..Default::default()
            }]);
            let diagnostics = ingest(&set).expect_err("an unrecognised syntax is refused");
            assert_eq!(
                diagnostics.iter().next().unwrap().kind(),
                DiagnosticKind::MalformedDescriptor,
                "syntax {syntax:?}"
            );
        }
    }

    #[test]
    fn a_leading_dot_name_is_refused_before_the_engine() {
        // A top-level name beginning with `.` makes prost-reflect's name lookup panic; keryx pre-empts
        // it with a `MalformedDescriptor` at the file's locus.
        let set = encode(vec![FileDescriptorProto {
            name: Some("m.proto".to_owned()),
            message_type: vec![DescriptorProto {
                name: Some(".Foo".to_owned()),
                ..Default::default()
            }],
            ..Default::default()
        }]);
        let diagnostics = ingest(&set).expect_err("a leading-dot name is refused");
        assert_eq!(
            diagnostics.iter().next().unwrap().kind(),
            DiagnosticKind::MalformedDescriptor
        );
    }

    #[test]
    fn the_door_admits_the_deepest_lexical_nesting_and_refuses_past_it() {
        // The deepest lexical message chain the door admits is one shallower than the engine's decode
        // recursion limit; one deeper is refused as `UnreadableDescriptorSet` (the engine's own
        // limit), not a panic — the managed walk is defense-in-depth behind this.
        let deepest = DECODE_RECURSION_LIMIT - 1;
        let schema = ingest(&lexical_chain_typed(deepest).encode_to_vec())
            .expect("at the deepest admitted depth, the door admits and the walk runs");
        assert_eq!(schema.messages.len(), deepest);
        let refused = ingest(&lexical_chain_typed(DECODE_RECURSION_LIMIT).encode_to_vec())
            .expect_err("one deeper is refused");
        assert_eq!(
            refused.iter().next().unwrap().kind(),
            DiagnosticKind::UnreadableDescriptorSet
        );
    }

    #[test]
    fn the_managed_walk_survives_nesting_past_the_decode_cap() {
        // A typed set built in memory bypasses the byte-decode cap, so the walk itself is exercised at
        // a depth a naive recursion could not survive; the managed stack walks it without exhausting
        // the call stack.
        let depth = 500;
        let pool = DescriptorPool::from_file_descriptor_set(lexical_chain_typed(depth))
            .expect("the pool builds past the byte-decode cap");
        let file = pool.files().next().expect("one file");
        assert_eq!(subject_messages(&file).len(), depth);
    }
}
