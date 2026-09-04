//! Test-only helpers shared across keryx's suites (a dev-dependency library, so its helpers
//! are public API — no suite pays a `dead_code` allow for the ones it does not use). Compile a
//! fixture `.proto` to a serialized `FileDescriptorSet` (bytes) through protox, the pure-Rust
//! compiler: bytes are the *only* thing that crosses to a suite — keryx decodes them through
//! its own prost-reflect, so the two crates' prost versions never couple (the descriptor-engine
//! boundary). Built through `encode_file_descriptor_set`, not `protox::compile`: that
//! convenience re-encodes every options submessage through prost-types' typed structs and
//! silently drops the custom-option extension bytes (the same §20 trap keryx's own ingestion
//! avoids); encoding straight from the pool keeps them. `include_imports`/`include_source_info`
//! are on, so the set is self-contained and carries doc comments.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use protox::Compiler;

/// keryx-core's fixtures directory (`crates/keryx-core/tests/fixtures`) — the one fixture
/// corpus, read by both suites through this crate.
#[must_use]
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../keryx-core/tests/fixtures")
}

/// keryx-core's vendored proto directory (`crates/keryx-core/proto`), for `keryx/options.proto`.
#[must_use]
pub fn vendored() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../keryx-core/proto")
}

/// Compile a fixture `<name>` — imports resolved against the fixtures dir, the vendored
/// `proto/` dir, and protox's bundled well-known types — to a serialized `FileDescriptorSet`.
///
/// # Panics
/// If the fixture does not compile: a broken fixture is a test bug, surfaced loudly. Use
/// [`try_compile_fixture`] where a compile failure is an expected outcome to record.
#[must_use]
pub fn compile_fixture(name: &str) -> Vec<u8> {
    try_compile_fixture(name).unwrap_or_else(|error| panic!("fixture `{name}` compiles: {error}"))
}

/// As [`compile_fixture`], but returns the compiler's result so a probe can record whether a
/// version (e.g. an edition) is supported rather than panic.
///
/// # Errors
/// The compiler's message (as a `String`) when the sources do not compile — a parse, type,
/// import, or unsupported-edition error.
pub fn try_compile_fixture(name: &str) -> Result<Vec<u8>, String> {
    try_compile_in(&[fixtures(), vendored()], name)
}

/// Compile the `.proto` named `file` relative to one of `includes` — imports resolved against
/// those roots and protox's bundled well-known types — to a serialized `FileDescriptorSet`, as
/// [`compile_fixture`] does for the fixture corpus. For a source outside that corpus, such as a
/// worked example under `examples/`.
///
/// # Panics
/// If the source does not compile: a broken example is a test bug, surfaced loudly.
#[must_use]
pub fn compile_in(includes: &[PathBuf], file: &str) -> Vec<u8> {
    try_compile_in(includes, file).unwrap_or_else(|error| panic!("`{file}` compiles: {error}"))
}

/// As [`compile_in`], returning the compiler's result.
///
/// # Errors
/// As [`try_compile_fixture`].
pub fn try_compile_in(includes: &[PathBuf], file: &str) -> Result<Vec<u8>, String> {
    let mut compiler = Compiler::new(includes).map_err(|error| error.to_string())?;
    compiler.include_source_info(true).include_imports(true);
    compiler
        .open_file(file)
        .map_err(|error| error.to_string())?;
    Ok(compiler.encode_file_descriptor_set())
}

/// Wire-format builders over prost's encoding primitives, so a suite writes a payload as bytes
/// on the wire — never through the engine's own encoder — and the payload door is seen to read
/// the wire. The scalar primitives (`prost::encoding::int32::encode` and kin) are used directly;
/// this module carries only what they lack.
pub mod wire {
    use prost::encoding::{self, WireType};

    /// Append a length-delimited field — a sub-message, a `string`, or `bytes` — numbered `tag`
    /// with `payload` as its content.
    pub fn delimited(tag: u32, payload: &[u8], buf: &mut Vec<u8>) {
        encoding::encode_key(tag, WireType::LengthDelimited, buf);
        encoding::encode_varint(
            u64::try_from(payload.len()).expect("a test payload fits"),
            buf,
        );
        buf.extend_from_slice(payload);
    }
}

/// A hand-built `FileDescriptorSet` that carries an **`uninterpreted_option`** — one message with a
/// single scalar-valued uninterpreted option. keryx refuses *any* uninterpreted option at the door
/// (`descriptor::pre_validate`): a compiled set has none (protoc/protox interpret and clear them),
/// and an unresolved one could carry a deep text-format aggregate value the descriptor engine parses
/// with an *unbounded* recursion — a stack-overflow **abort** containment cannot hold — so keryx
/// pre-empts every such option as a clean `MalformedDescriptor`, not only one bearing a deep
/// aggregate. protoc/protox would never emit this, so it is built by hand. For the tests that
/// exercise the door's uninterpreted-option refusal.
#[must_use]
pub fn uninterpreted_option_set() -> Vec<u8> {
    use prost::Message as _;
    use prost_types::uninterpreted_option::NamePart;
    use prost_types::{
        DescriptorProto, FileDescriptorProto, FileDescriptorSet, MessageOptions,
        UninterpretedOption,
    };

    FileDescriptorSet {
        file: vec![FileDescriptorProto {
            name: Some("m.proto".to_owned()),
            package: Some("my".to_owned()),
            syntax: Some("proto3".to_owned()),
            message_type: vec![DescriptorProto {
                name: Some("M".to_owned()),
                options: Some(MessageOptions {
                    uninterpreted_option: vec![UninterpretedOption {
                        name: vec![NamePart {
                            name_part: "x".to_owned(),
                            is_extension: true,
                        }],
                        positive_int_value: Some(1),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
    .encode_to_vec()
}

/// A hand-built `FileDescriptorSet` that drives a **real contained engine fault** through keryx's
/// descriptor door — for the tests that exercise the containment seam end to end, not a synthetic
/// `panic!`. It redefines `descriptor.proto`'s `MessageOptions`, retyping field 1
/// (`message_set_wire_format`) as a repeated `int32`, and sets that option to a scalar `true` on one
/// message: the descriptor engine **panics decoding the options** during the pool build — a fault
/// keryx contains at the *decode* (`DependencyFault`). It carries no `uninterpreted_option` and no
/// non-identifier name, so it passes the door's pre-emption and reaches the engine, where the real
/// fault occurs. protoc/protox would never emit this, so it is built by hand.
#[must_use]
pub fn decode_fault_set() -> Vec<u8> {
    use prost::Message as _;
    use prost_types::{
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
        MessageOptions,
    };

    FileDescriptorSet {
        file: vec![
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
        ],
    }
    .encode_to_vec()
}
