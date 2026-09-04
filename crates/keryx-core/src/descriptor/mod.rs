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
    Oneof, Openness, Package, Presence, Scalar, Schema, SchemaVersion, ValueType,
};
pub use source::compile;

mod desugar;
mod docs;
mod options;
mod recursion;

use std::collections::BTreeSet;

use prost::Message as _;
use prost_reflect::{
    DescriptorPool, EnumDescriptor, EnumValueDescriptor, FieldDescriptor, FileDescriptor, Kind,
    MessageDescriptor, OneofDescriptor,
};

use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::fault::Dependency;

/// Ingest a serialized `FileDescriptorSet` into a de-sugared [`Schema`] (§20). The
/// sole public entry of the descriptor-engine boundary: bytes in, keryx types out,
/// total on foreign input — a set that does not decode, or that decodes but carries
/// a structurally-malformed element, yields diagnostics, never a panic and never a
/// partial schema; the first structural diagnosis short-circuits the walk. Custom
/// options are read only through the dynamic layer, so no annotation is ever
/// silently dropped (§20). The model is assembled from the set in two passes — a
/// subject pass over its files and a referent closure that pulls in every well-known
/// or dependency type a subject field references (§10) — resolving options and doc
/// comments per element, then ordered deterministically and analysed for containment
/// cycles.
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
    crate::fault::contain(
        Dependency::ProstReflect,
        "walking the descriptor set",
        || build_schema(pool, is_subject),
    )?
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

/// prost's decode recursion limit (`prost::RECURSION_LIMIT`, not public API): `DescriptorPool::decode`
/// and the prost-types pre-read refuse a lexical message chain this deep or deeper as
/// `UnreadableDescriptorSet`, so the deepest chain the descriptor door admits is `RECURSION_LIMIT - 1`.
/// The one engine constant both the descriptor door's boundary test and the source door's nesting
/// guard (`source::SOURCE_NESTING_LIMIT`) derive from — named once, not re-derived per door.
pub(crate) const RECURSION_LIMIT: usize = 100;

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
    if let Some(diagnostics) = crate::fault::contain(
        Dependency::ProstTypes,
        "inspecting the descriptor set",
        || pre_validate(bytes),
    )? {
        return Err(diagnostics);
    }
    // The pool decode: the closure borrows only `bytes`. prost-reflect touches its global
    // well-known-type pool — here and at accessor time — only through `DescriptorPool::global()`, which
    // locks the pool's `Mutex` solely across an infallible `Arc` clone and drops the guard
    // (prost-reflect-0.16.5 `src/descriptor/global.rs:10-27`); the only holders across fallible work are
    // the `*_global_*` mutators (`global.rs:32-48`), which keryx never calls (a grep of `crates/` finds
    // none). So a contained panic cannot poison the global lock, and `AssertUnwindSafe` is sound on that
    // basis. (A same-process consumer that *does* call those mutators with a panicking set is the
    // residual, recorded in the threat model's dependency boundary — not an Open item.)
    crate::fault::contain(
        Dependency::ProstReflect,
        "decoding the descriptor set",
        || DescriptorPool::decode(bytes),
    )?
    .map_err(|error| unreadable_set(error.to_string()))
}

/// Refuse — before the descriptor engine builds a pool — the shapes prost-reflect cannot represent
/// and **panics or aborts** on rather than rejecting, and the shapes keryx's own sinks cannot carry
/// safely, so the engine only ever sees input it can represent:
///
/// - a file whose `syntax` is not proto2/proto3 — editions (an engine capability limit,
///   `UnsupportedEdition`), or any unrecognised value such as `proto4` or an empty string (a
///   `MalformedDescriptor`); `syntax` is read raw, since the `syntax()` getter cannot tell `None`
///   from `""`;
/// - a `package` that is not a dotted sequence of proto identifiers within the segment bound
///   ([`Package::parse`]) — the leading-`.` package the engine panics on is one such shape, and the
///   check additionally confines the package to the identifier shape the emitted `#include` operand
///   and the CLI's per-package output path assume (the threat model's descriptor-door package
///   boundary) and bounds the qualifier depth (`policy::qualify`, bounded work);
/// - a declared message/enum/field/value/… name that is not a proto identifier
///   ([`model::is_proto_ident`]) — the leading-`.` top-level name the engine panics on among them; and
/// - any `uninterpreted_option` the set carries: a *compiled* set has none (protoc and protox
///   interpret custom options and clear the field), but an unresolved one drives prost-reflect's
///   **unbounded** recursive text-format parse of its `aggregate_value` at pool build
///   (`option_to_message` → `DynamicMessage::parse_text_format`), a stack-overflow *abort*
///   `fault::contain` cannot hold — so refusing it pre-empts that abort at the door, with no
///   legitimate input lost.
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
        let name = file.name();
        match file.syntax.as_deref() {
            None | Some("proto2" | "proto3") => {}
            Some("editions") => refusals.push(Diagnostic::new(
                DiagnosticKind::UnsupportedEdition,
                Locus::at(name.to_owned()),
                EDITIONS_UNSUPPORTED,
            )),
            Some(other) => refusals.push(malformed(
                name,
                format!("unrecognised syntax {other:?}: keryx's descriptor engine reads proto2 and proto3"),
            )),
        }
        if let Err(problem) = Package::parse(file.package()) {
            refusals.push(malformed(name, problem.detail()));
        }
        check_structure(file, &mut refusals);
    }
    Diagnostics::collect(refusals)
}

/// A `MalformedDescriptor` at a file's locus — the refusal shape of the pre-read and of the walk's
/// package re-derivation.
fn malformed(file: &str, detail: impl Into<String>) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::MalformedDescriptor,
        Locus::at(file.to_owned()),
        detail,
    )
}

/// Refuse a declared name that is not a proto identifier (the leading-`.` name the engine panics on
/// among them) — the shape the schema's own name lowering (§4.2) and the door assume.
fn check_ident(refusals: &mut Vec<Diagnostic>, file: &str, what: &str, decl: &str) {
    if !model::is_proto_ident(decl) {
        refusals.push(malformed(
            file,
            format!("{what} name {decl:?} is not a proto identifier"),
        ));
    }
}

/// Refuse any `uninterpreted_option` (see [`pre_validate`]) — the text-format abort axis.
fn check_uninterpreted(
    refusals: &mut Vec<Diagnostic>,
    file: &str,
    uninterpreted: &[prost_types::UninterpretedOption],
) {
    if !uninterpreted.is_empty() {
        refusals.push(malformed(
            file,
            "the set carries an uninterpreted option — supply a compiled descriptor set (keryx does \
             not evaluate custom option values, and an unresolved option's text-format value drives \
             the descriptor engine's unbounded parser)",
        ));
    }
}

/// Walk a file's declarations, refusing every non-identifier name and every `uninterpreted_option`
/// (see [`pre_validate`]). The message walk is an explicit managed stack — heap, not the call stack —
/// so a deeply-nested file cannot exhaust the stack here (the same posture as [`subject_messages`]);
/// the pre-read decode already bounds the nesting at `RECURSION_LIMIT` regardless.
fn check_structure(file: &prost_types::FileDescriptorProto, refusals: &mut Vec<Diagnostic>) {
    let at = file.name();
    if let Some(options) = &file.options {
        check_uninterpreted(refusals, at, &options.uninterpreted_option);
    }
    for extension in &file.extension {
        check_ident(refusals, at, "extension", extension.name());
        if let Some(options) = &extension.options {
            check_uninterpreted(refusals, at, &options.uninterpreted_option);
        }
    }
    for service in &file.service {
        check_ident(refusals, at, "service", service.name());
        if let Some(options) = &service.options {
            check_uninterpreted(refusals, at, &options.uninterpreted_option);
        }
        for method in &service.method {
            check_ident(refusals, at, "method", method.name());
            if let Some(options) = &method.options {
                check_uninterpreted(refusals, at, &options.uninterpreted_option);
            }
        }
    }
    for enumeration in &file.enum_type {
        check_enum(refusals, at, enumeration);
    }
    let mut stack: Vec<&prost_types::DescriptorProto> = file.message_type.iter().collect();
    while let Some(message) = stack.pop() {
        check_ident(refusals, at, "message", message.name());
        if let Some(options) = &message.options {
            check_uninterpreted(refusals, at, &options.uninterpreted_option);
        }
        for field in &message.field {
            check_ident(refusals, at, "field", field.name());
            if let Some(options) = &field.options {
                check_uninterpreted(refusals, at, &options.uninterpreted_option);
            }
        }
        for oneof in &message.oneof_decl {
            check_ident(refusals, at, "oneof", oneof.name());
            if let Some(options) = &oneof.options {
                check_uninterpreted(refusals, at, &options.uninterpreted_option);
            }
        }
        for extension in &message.extension {
            check_ident(refusals, at, "extension", extension.name());
            if let Some(options) = &extension.options {
                check_uninterpreted(refusals, at, &options.uninterpreted_option);
            }
        }
        for range in &message.extension_range {
            if let Some(options) = &range.options {
                check_uninterpreted(refusals, at, &options.uninterpreted_option);
            }
        }
        for enumeration in &message.enum_type {
            check_enum(refusals, at, enumeration);
        }
        stack.extend(message.nested_type.iter());
    }
}

/// Refuse an enum's and its values' non-identifier names and any `uninterpreted_option` on them.
fn check_enum(
    refusals: &mut Vec<Diagnostic>,
    file: &str,
    enumeration: &prost_types::EnumDescriptorProto,
) {
    check_ident(refusals, file, "enum", enumeration.name());
    if let Some(options) = &enumeration.options {
        check_uninterpreted(refusals, file, &options.uninterpreted_option);
    }
    for value in &enumeration.value {
        check_ident(refusals, file, "enum value", value.name());
        if let Some(options) = &value.options {
            check_uninterpreted(refusals, file, &options.uninterpreted_option);
        }
    }
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

/// Assemble the schema from the pool. Two passes: the **subject** files `is_subject` names, and then
/// the **referent closure** — every message or enum a subject field names, transitively, translated as
/// a sort even when its file is not a subject (spec §10/§20: well-known types are ordinary messages
/// and translate structurally). `is_subject` decides the direct subjects — the bytes-only `ingest`
/// path uses the well-known-name heuristic (`desugar::is_dependency_file`), so a well-known-type or
/// option-registry file is not a *direct* subject; the front door (`ingest_subjects`) carries the
/// real, explicitly-opened set across the `compile → ingest` seam (§21.2). A well-known type reaches
/// the schema only when a subject field references it — so `Timestamp` becomes a sort in a schema that
/// uses it, while `descriptor.proto`'s option messages, referenced by no field, never do. The closure
/// makes every `ValueType::Message`/`Enum` referent an element — and the lexical parent of any nested
/// one, so an `outer` never names a non-element either — so neither a reference nor a `nested` outer
/// ever dangles. Deterministically ordered (P3).
fn build_schema(
    pool: &DescriptorPool,
    is_subject: impl Fn(&str) -> bool,
) -> Result<Schema, Diagnostics> {
    let mut files = Vec::new();
    let mut file_names = BTreeSet::new();
    let mut messages = Vec::new();
    let mut enums = Vec::new();

    // Pass one: the direct subjects.
    for file in pool.files() {
        if !is_subject(file.name()) {
            continue;
        }
        add_file(&mut files, &mut file_names, &file)?;
        let version = desugar::version(&file);
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

    // Pass two: the referent closure (§10). A worklist over the referents of every built message;
    // each new referent is looked up in the pool, built as a sort, and its own referents — and its
    // lexical parent, so a pulled-in nested type's `outer` always names a declared element — enqueued.
    // `included` is the set already built (subjects first), so each type is built once and the walk
    // terminates on the pool's finite type set — bounded work, no recursion on the call stack.
    let mut included: BTreeSet<String> = messages
        .iter()
        .map(|message| message.path.as_str().to_owned())
        .chain(
            enums
                .iter()
                .map(|enumeration| enumeration.path.as_str().to_owned()),
        )
        .collect();
    let mut queue: Vec<FqName> = messages.iter().flat_map(message_referents).collect();
    while let Some(referent) = queue.pop() {
        if !included.insert(referent.as_str().to_owned()) {
            continue;
        }
        if let Some(message) = pool.get_message_by_name(referent.as_str()) {
            if message.is_map_entry() {
                continue;
            }
            let file = message.parent_file();
            add_file(&mut files, &mut file_names, &file)?;
            let built = build_message(&message, file.name())?;
            queue.extend(message_referents(&built));
            queue.extend(built.outer.clone()); // the container of a nested referent is an element too
            messages.push(built);
        } else if let Some(enumeration) = pool.get_enum_by_name(referent.as_str()) {
            let file = enumeration.parent_file();
            add_file(&mut files, &mut file_names, &file)?;
            let built = build_enum(&enumeration, file.name(), desugar::version(&file))?;
            queue.extend(built.outer.clone()); // a nested enum's container is an element too
            enums.push(built);
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

/// Add a `file` to the schema's file list at most once (subject pass and referent closure share it),
/// validating its package through the one door (`Package::parse`) — a proof of shape, and a `?` that
/// keeps the walk total; `pre_validate` already refused any non-identifier package, so it cannot fail
/// on a set that reached the walk.
fn add_file(
    files: &mut Vec<File>,
    seen: &mut BTreeSet<String>,
    file: &FileDescriptor,
) -> Result<(), Diagnostics> {
    if !seen.insert(file.name().to_owned()) {
        return Ok(());
    }
    let package = Package::parse(file.package_name())
        .map_err(|problem| Diagnostics::from(malformed(file.name(), problem.detail())))?;
    files.push(File {
        name: file.name().to_owned(),
        package,
    });
    Ok(())
}

/// The message and enum types a built message's fields name — its structural referents, for the §10
/// closure. Maps are already de-sugared to their value type (`build_field`); a scalar names nothing.
fn message_referents(message: &Message) -> Vec<FqName> {
    message
        .fields
        .iter()
        .filter_map(|field| {
            let value = match &field.shape {
                FieldShape::Singular { value, .. }
                | FieldShape::Repeated { value }
                | FieldShape::Map { value, .. } => value,
            };
            match value {
                ValueType::Message(referent) | ValueType::Enum(referent) => Some(referent.clone()),
                ValueType::Scalar(_) => None,
            }
        })
        .collect()
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
    use keryx_test_support::{decode_fault_set, uninterpreted_option_set};
    use prost::Message as _;
    use prost_types::{
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
        MessageOptions,
    };

    use super::{DescriptorPool, RECURSION_LIMIT, ingest, subject_messages};
    use crate::diagnostics::DiagnosticKind;

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
    fn an_uninterpreted_option_is_refused_before_the_engine() {
        // A set carrying an uninterpreted option: keryx refuses *any* uninterpreted option at the
        // door (a compiled set has none) with a clean `MalformedDescriptor`, pre-empting the engine's
        // unbounded recursive text-format parse of an option's aggregate value (`option_to_message` →
        // `parse_text_format`), a stack-overflow *abort* containment cannot hold — for every such
        // option, not only one that carries a deep aggregate. Nothing legitimate is refused (§6).
        let diagnostics = ingest(&uninterpreted_option_set()).expect_err("a refusal, not a schema");
        assert_eq!(
            diagnostics.iter().next().unwrap().kind(),
            DiagnosticKind::MalformedDescriptor
        );
    }

    #[test]
    fn an_unforeseen_engine_fault_at_decode_is_a_dependency_fault() {
        // A set retyping `MessageOptions` field 1 as repeated int32, then setting it: the engine
        // panics decoding the options during the pool build — a *real* engine fault (not a synthetic
        // `panic!`) contained at the decode as a `DependencyFault` (§6, the dependency boundary). It
        // carries no uninterpreted option and no non-identifier name, so the door's pre-emption lets
        // it through to the engine, where the fault occurs; the accessor-walk `contain` frame is the
        // same seam applied a second time, defense-in-depth behind this and the pre-emptions.
        let diagnostics = ingest(&decode_fault_set()).expect_err("a fault, not a schema");
        assert_eq!(
            diagnostics.iter().next().unwrap().kind(),
            DiagnosticKind::DependencyFault
        );
    }

    #[test]
    fn a_contained_fault_does_not_poison_a_later_decode() {
        // The global-pool argument, made executable (not just asserted against the source): a contained
        // decode fault must leave the process's shared well-known-type pool usable, so a clean decode in
        // the *same* process still succeeds — prost-reflect holds the global lock only across an
        // infallible clone, and keryx never mutates it, so one fault cannot turn into a persistent
        // denial for a long-lived service (the dependency boundary).
        let _ = ingest(&decode_fault_set()).expect_err("the crafted set faults");
        let good = encode(vec![FileDescriptorProto {
            name: Some("ok.proto".to_owned()),
            package: Some("ok".to_owned()),
            syntax: Some("proto3".to_owned()),
            message_type: vec![DescriptorProto {
                name: Some("Ok".to_owned()),
                ..Default::default()
            }],
            ..Default::default()
        }]);
        ingest(&good).expect("a clean decode after a contained fault still succeeds");
    }

    #[test]
    fn a_non_identifier_package_is_refused_at_the_door() {
        // The `package` reaches a filesystem path (`gen -o`) and a raw `#include` operand
        // (`emit::views`); the door refuses any shape but a dotted identifier, so neither sink ever
        // sees a `..`, a quote, whitespace, or an empty segment (the threat model's package boundary).
        for package in [
            "../../etc/x",
            "x\"y",
            "a b",
            "a..b",
            ".leading",
            "trailing.",
        ] {
            let set = encode(vec![FileDescriptorProto {
                name: Some("m.proto".to_owned()),
                package: Some((*package).to_owned()),
                syntax: Some("proto3".to_owned()),
                message_type: vec![DescriptorProto {
                    name: Some("M".to_owned()),
                    ..Default::default()
                }],
                ..Default::default()
            }]);
            let diagnostics = ingest(&set).expect_err("a non-identifier package is refused");
            assert_eq!(
                diagnostics.iter().next().unwrap().kind(),
                DiagnosticKind::MalformedDescriptor,
                "package {package:?}"
            );
        }
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
        let deepest = RECURSION_LIMIT - 1;
        let schema = ingest(&lexical_chain_typed(deepest).encode_to_vec())
            .expect("at the deepest admitted depth, the door admits and the walk runs");
        assert_eq!(schema.messages.len(), deepest);
        let refused = ingest(&lexical_chain_typed(RECURSION_LIMIT).encode_to_vec())
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

    #[test]
    fn a_malformed_map_entry_is_a_diagnostic_not_a_panic() {
        // The totality instrument that reaches keryx's *own* walk (arbitrary bytes never do): a
        // valid-encoding but structurally-invalid map. The field is a repeated message whose entry
        // carries the `map_entry` option — so prost-reflect reads it as a map — but the entry is
        // missing its value field (#2). `map_shape` reaches it and composes `MalformedDescriptor`, not
        // a panic (§6).
        let set = encode(vec![FileDescriptorProto {
            name: Some("m.proto".to_owned()),
            package: Some("p".to_owned()),
            syntax: Some("proto3".to_owned()),
            message_type: vec![DescriptorProto {
                name: Some("M".to_owned()),
                field: vec![FieldDescriptorProto {
                    name: Some("m".to_owned()),
                    number: Some(1),
                    label: Some(3),   // repeated
                    r#type: Some(11), // message
                    type_name: Some(".p.M.MEntry".to_owned()),
                    ..Default::default()
                }],
                nested_type: vec![DescriptorProto {
                    name: Some("MEntry".to_owned()),
                    field: vec![FieldDescriptorProto {
                        name: Some("key".to_owned()),
                        number: Some(1),
                        label: Some(1),
                        r#type: Some(9), // string
                        ..Default::default()
                    }],
                    options: Some(MessageOptions {
                        map_entry: Some(true), // read as a map, but the value field (#2) is absent
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }]);
        let diagnostics = ingest(&set).expect_err("a malformed map entry is refused");
        assert_eq!(
            diagnostics.iter().next().unwrap().kind(),
            DiagnosticKind::MalformedDescriptor
        );
    }

    #[test]
    fn a_non_key_map_key_is_a_diagnostic_not_a_panic() {
        // A map entry with both fields present, but a key of a non-key kind (float): `map_shape`
        // reaches past the missing-field check to `map_key`, which refuses a float/double/message/enum
        // key with a clean `MalformedDescriptor`, not a panic (§6). protoc rejects this source; a
        // directly-supplied descriptor set can carry it.
        let set = encode(vec![FileDescriptorProto {
            name: Some("m.proto".to_owned()),
            package: Some("p".to_owned()),
            syntax: Some("proto3".to_owned()),
            message_type: vec![DescriptorProto {
                name: Some("M".to_owned()),
                field: vec![FieldDescriptorProto {
                    name: Some("m".to_owned()),
                    number: Some(1),
                    label: Some(3),   // repeated
                    r#type: Some(11), // message
                    type_name: Some(".p.M.MEntry".to_owned()),
                    ..Default::default()
                }],
                nested_type: vec![DescriptorProto {
                    name: Some("MEntry".to_owned()),
                    field: vec![
                        FieldDescriptorProto {
                            name: Some("key".to_owned()),
                            number: Some(1),
                            label: Some(1),
                            r#type: Some(2), // float — not a valid map key
                            ..Default::default()
                        },
                        FieldDescriptorProto {
                            name: Some("value".to_owned()),
                            number: Some(2),
                            label: Some(1),
                            r#type: Some(9), // string
                            ..Default::default()
                        },
                    ],
                    options: Some(MessageOptions {
                        map_entry: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }]);
        let diagnostics = ingest(&set).expect_err("a non-key map key is refused");
        assert_eq!(
            diagnostics.iter().next().unwrap().kind(),
            DiagnosticKind::MalformedDescriptor
        );
    }
}
