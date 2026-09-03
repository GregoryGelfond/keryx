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
    let mut compiler =
        Compiler::new([fixtures(), vendored()]).map_err(|error| error.to_string())?;
    compiler.include_source_info(true).include_imports(true);
    compiler
        .open_file(name)
        .map_err(|error| error.to_string())?;
    Ok(compiler.encode_file_descriptor_set())
}

/// A hand-built `FileDescriptorSet` that drives a **real contained engine fault** through keryx's
/// descriptor door — for the tests that exercise the containment seam end to end, not a synthetic
/// `panic!`. It redefines `descriptor.proto`'s `MessageOptions` with a self-referential message
/// field, and gives one message a 120-deep uninterpreted-option name path: the set decodes and the
/// pool builds (the nesting is created by option navigation and encoded with no limit), but reading
/// `options()` on the message re-decodes past prost's recursion limit and unwraps — a fault keryx
/// contains at the accessor walk (`DependencyFault`). protoc/protox would never emit this, so it is
/// built by hand rather than compiled from a fixture.
#[must_use]
pub fn fault_provoking_set() -> Vec<u8> {
    use prost::Message as _;
    use prost_types::uninterpreted_option::NamePart;
    use prost_types::{
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
        MessageOptions, UninterpretedOption,
    };

    const DEPTH: usize = 120;
    let mut name: Vec<NamePart> = (0..DEPTH)
        .map(|_| NamePart {
            name_part: "self_".to_owned(),
            is_extension: false,
        })
        .collect();
    name.push(NamePart {
        name_part: "x".to_owned(),
        is_extension: false,
    });
    FileDescriptorSet {
        file: vec![
            FileDescriptorProto {
                name: Some("google/protobuf/descriptor.proto".to_owned()),
                package: Some("google.protobuf".to_owned()),
                message_type: vec![DescriptorProto {
                    name: Some("MessageOptions".to_owned()),
                    field: vec![
                        FieldDescriptorProto {
                            name: Some("self_".to_owned()),
                            number: Some(1000),
                            label: Some(1),   // optional
                            r#type: Some(11), // message
                            type_name: Some(".google.protobuf.MessageOptions".to_owned()),
                            ..Default::default()
                        },
                        FieldDescriptorProto {
                            name: Some("x".to_owned()),
                            number: Some(1001),
                            label: Some(1),
                            r#type: Some(5), // int32
                            ..Default::default()
                        },
                    ],
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
                        uninterpreted_option: vec![UninterpretedOption {
                            name,
                            positive_int_value: Some(1),
                            ..Default::default()
                        }],
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
