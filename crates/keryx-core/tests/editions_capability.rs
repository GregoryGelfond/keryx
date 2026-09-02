//! Editions capability, verified up front (spec §31's (M1) editions gate). Green either
//! way: if the toolchain compiles edition 2023, prost-reflect must resolve its
//! presence features correctly; if not, editions is a documented, crate-gated
//! upgrade — keryx branches on resolved features, so it is a fixture add, not a
//! redesign. Run with `--nocapture` to read the verdict for `docs/proto-support.md`.

use keryx_test_support as support;

use prost_reflect::DescriptorPool;

#[test]
fn editions_resolve_correctly_when_supported() {
    assert!(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/editions_probe.proto")
            .exists(),
        "editions probe fixture present"
    );
    match support::try_compile_fixture("editions_probe.proto") {
        Ok(bytes) => {
            let pool = DescriptorPool::decode(&bytes[..]).expect("the editions set decodes");
            let sample = pool
                .get_message_by_name("keryx.editions_probe.Sample")
                .expect("Sample is present");
            let field = |name: &str| sample.get_field_by_name(name).expect("field present");
            assert!(
                field("explicit_field").supports_presence(),
                "EXPLICIT feature resolved"
            );
            assert!(
                !field("implicit_field").supports_presence(),
                "implicit default resolved"
            );
            println!("editions: SUPPORTED (add the editions fixture + golden)");
        }
        Err(error) => {
            println!("editions: DEFERRED (protox does not compile edition 2023: {error})");
        }
    }
}
