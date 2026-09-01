//! The thermal `gen` example is a gate fixture (spec §27): regenerate it and assert it
//! equals the committed `examples/thermal/gen/*`. A drift here is a real change to the
//! generated vocabulary, intended or a regression.

use std::path::{Path, PathBuf};

use keryx_core::descriptor::compile;
use keryx_core::{emit, manifest, policy};

fn workspace() -> PathBuf {
    // keryx-core/ -> crates/ -> repo root.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn thermal_gen_matches_the_committed_example() {
    let root = workspace();
    let example = root.join("examples/thermal");
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR")).join("proto");
    let schema = compile(
        &[example.join("thermal.proto")],
        &[example.clone(), vendored],
    )
    .expect("thermal compiles");
    let mapping = policy::map(&schema).expect("maps");
    let unit = mapping.units().first().expect("thermal.v1 unit");
    let read = |name: &str| {
        std::fs::read_to_string(example.join("gen").join(name)).expect("golden present")
    };
    assert_eq!(emit::core(unit).expect("core"), read("thermal.v1.core.lp"));
    assert_eq!(
        emit::views(unit).expect("views"),
        read("thermal.v1.views.lp")
    );
    assert_eq!(
        manifest::write(unit, "-"),
        read("thermal.v1.keryx-manifest")
    );
}
