//! Determinism at the payload door (the threat model's property 5 — an auditability property):
//! the same payload yields byte-identical facts, on both delivery seams (spec §11), from any codec
//! built over the same schema, whatever the wire's field order and whatever order the engine's map
//! table hands entries back in; and every fact is delivered exactly once on each seam. The
//! mechanism is ordering by canonical bytes — `Symbol::Ord` on the symbol seam, themelios's
//! statement order on the `.lp` seam — over the one fact set the walk emits, so no hidden state (a
//! hash seed, an iteration order) can vary the output; the instruments assert the output. The
//! de-duplication guard rests on the rendering's canonical program spelling each fact once: a
//! rendered count equal to the symbol count proves no fact was emitted twice, and a wire that
//! repeats a field or a map key still yields one fact for it.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use keryx_test_support as support;
use keryx_test_support::wire::{self, batch, delimited, reading};
use proptest::prelude::*;

use keryx_core::codec::{Codec, Facts, PayloadFormat, Root};
use keryx_core::diagnostics::Diagnostics;

/// The thermal example's directory (`examples/thermal`), the subject of spec §28.
fn thermal_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/thermal")
}

/// The thermal codec, through the descriptor-set door.
fn thermal_codec() -> Codec {
    let set = support::compile_in(&[thermal_dir(), support::vendored()], "thermal.proto");
    Codec::new(&set).expect("the thermal example builds a codec")
}

/// A fixture's codec, through the descriptor-set door.
fn fixture_codec(name: &str) -> Codec {
    Codec::new(&support::compile_fixture(name)).expect("the fixture builds a codec")
}

/// Shred `payload` as `root_type` under `codec`, from the fresh root `r0`.
fn shred(codec: &Codec, root_type: &str, payload: &[u8]) -> Result<Facts, Diagnostics> {
    codec.shred(root_type, payload, PayloadFormat::Binary, &Root::fresh(0))
}

/// Every fact once on each seam: the symbols sorted and pairwise distinct, and the rendering one
/// line per symbol — the rendering's canonical program spells each fact once, so a count equality
/// proves no fact was emitted twice. The rendering, for the caller's own comparison.
fn assert_once_each(facts: &Facts) -> String {
    let symbols = facts.symbols();
    assert!(
        symbols.windows(2).all(|pair| pair[0] < pair[1]),
        "the symbol seam is sorted with no fact twice: {symbols:?}"
    );
    let rendered = facts.render().expect("the facts render");
    assert_eq!(
        rendered.lines().count(),
        symbols.len(),
        "one rendered fact per symbol: {rendered}"
    );
    rendered
}

/// A `counts` entry of the maps fixture's `Inventory`: `map<string, int32>`.
fn count(key: &str, value: i32) -> Vec<u8> {
    let mut entry = Vec::new();
    delimited(1, key.as_bytes(), &mut entry);
    wire::int32(2, value, &mut entry);
    entry
}

/// An `items` entry of the maps fixture's `Inventory`: `map<int64, Item>`, the item its `sku`.
fn item(key: i64, sku: &str) -> Vec<u8> {
    let mut item = Vec::new();
    delimited(1, sku.as_bytes(), &mut item);
    let mut entry = Vec::new();
    wire::int64(1, key, &mut entry);
    delimited(2, &item, &mut entry);
    entry
}

#[test]
fn the_same_payload_shreds_to_identical_facts_however_often_and_from_whichever_codec() {
    // One payload; the same codec twice, a second codec over the same set, and a codec from the
    // source the set was compiled from: one symbol sequence and one rendering, every time.
    let payload = batch(&[reading("s-107", 21), reading("s-101", 44), reading("", 0)]);
    let codec = thermal_codec();
    let first = shred(&codec, "ReadingBatch", &payload).expect("shreds");
    let again = shred(&codec, "ReadingBatch", &payload).expect("shreds");
    let rebuilt = shred(&thermal_codec(), "ReadingBatch", &payload).expect("shreds");
    let from_source = Codec::from_source(
        &[thermal_dir().join("thermal.proto")],
        &[thermal_dir(), support::vendored()],
    )
    .expect("the thermal source builds a codec");
    let from_source = shred(&from_source, "ReadingBatch", &payload).expect("shreds");

    let rendered = assert_once_each(&first);
    assert_eq!(
        rendered.lines().count(),
        10,
        "the batch atom and three readings' three facts"
    );
    for other in [&again, &rebuilt, &from_source] {
        assert_eq!(first.symbols(), other.symbols());
        assert_eq!(rendered, assert_once_each(other));
    }
}

#[test]
fn a_map_shreds_the_same_whatever_the_wire_order() {
    // `Inventory`'s two maps: three scalar-valued entries and two message-valued ones, on the wire
    // in four orders. The engine's map is unordered; keryx orders the entries once by key, so
    // every order shreds to the one key-sorted fact set, on both seams.
    let codec = fixture_codec("maps.proto");
    let entries: [(u32, Vec<u8>); 5] = [
        (1, count("b", 2)),
        (1, count("a", 1)),
        (1, count("c", 3)),
        (2, item(20, "x")),
        (2, item(-1, "y")),
    ];
    let orders: [[usize; 5]; 4] = [
        [0, 1, 2, 3, 4],
        [4, 3, 2, 1, 0],
        [2, 0, 4, 1, 3],
        [3, 1, 4, 0, 2],
    ];
    let shreds: Vec<Facts> = orders
        .iter()
        .map(|order| {
            let mut bytes = Vec::new();
            for &at in order {
                let (tag, entry) = &entries[at];
                delimited(*tag, entry, &mut bytes);
            }
            shred(&codec, "Inventory", &bytes).expect("shreds")
        })
        .collect();
    let rendered = assert_once_each(&shreds[0]);
    assert_eq!(
        rendered,
        "counts(r0, \"a\", 1).\n\
         counts(r0, \"b\", 2).\n\
         counts(r0, \"c\", 3).\n\
         inventory(r0).\n\
         item(items(r0, \"-1\")).\n\
         item(items(r0, \"20\")).\n\
         sku(items(r0, \"-1\"), \"y\").\n\
         sku(items(r0, \"20\"), \"x\").\n"
    );
    for other in &shreds[1..] {
        assert_eq!(shreds[0].symbols(), other.symbols());
        assert_eq!(rendered, assert_once_each(other));
    }
}

#[test]
fn every_fact_is_delivered_once_on_each_seam_even_from_a_wire_that_repeats_itself() {
    // A wire that repeats a singular field, or a map key, is legal protobuf — the last occurrence
    // wins — and yields one fact for it: there is no duplicate in the fact set for either seam to
    // de-duplicate. And identical *values* at distinct occupants are distinct facts, not
    // duplicates: three children with one label are three label atoms.
    let thermal = thermal_codec();
    let mut repeated = Vec::new();
    delimited(1, b"s-101", &mut repeated);
    wire::int32(2, 44, &mut repeated);
    delimited(1, b"s-202", &mut repeated);
    wire::int32(2, 45, &mut repeated);
    let facts = shred(&thermal, "Reading", &repeated).expect("shreds");
    assert_eq!(
        assert_once_each(&facts),
        "reading(r0).\n\
         sensor(r0, \"s-202\").\n\
         temp_c(r0, 45).\n"
    );

    let maps = fixture_codec("maps.proto");
    let mut bytes = Vec::new();
    for (key, value) in [("a", 1), ("a", 2), ("b", 3), ("a", 4)] {
        delimited(1, &count(key, value), &mut bytes);
    }
    let facts = shred(&maps, "Inventory", &bytes).expect("shreds");
    assert_eq!(
        assert_once_each(&facts),
        "counts(r0, \"a\", 4).\n\
         counts(r0, \"b\", 3).\n\
         inventory(r0).\n"
    );

    let trees = fixture_codec("recursion.proto");
    let mut leaf = Vec::new();
    delimited(1, b"x", &mut leaf);
    let mut bytes = Vec::new();
    for _ in 0..3 {
        delimited(2, &leaf, &mut bytes);
    }
    let facts = shred(&trees, "Tree", &bytes).expect("shreds");
    let rendered = assert_once_each(&facts);
    assert_eq!(
        facts.symbols().len(),
        8,
        "four tree atoms, four label atoms"
    );
    assert_eq!(rendered.matches(", \"x\").").count(), 3);
}

/// The codecs the arbitrary-bytes generator shreds against, each with its root; built once.
static CODECS: LazyLock<Vec<(Codec, &'static str)>> = LazyLock::new(|| {
    vec![
        (thermal_codec(), "thermal.v1.ReadingBatch"),
        (fixture_codec("refusals.proto"), "keryx.refusals.Probe"),
        (fixture_codec("maps.proto"), "keryx.maps.Inventory"),
    ]
});

proptest! {
    #[test]
    fn arbitrary_bytes_shred_identically_twice(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        // Whatever the bytes — facts or a refusal — two shreds agree on every seam: the symbols and
        // the rendering, each fact once; or the diagnostics, kind for kind and locus for locus.
        for (codec, root_type) in CODECS.iter() {
            match (shred(codec, root_type, &bytes), shred(codec, root_type, &bytes)) {
                (Ok(first), Ok(again)) => {
                    prop_assert_eq!(first.symbols(), again.symbols());
                    let rendered = first.render().expect("renders");
                    prop_assert_eq!(&rendered, &again.render().expect("renders"));
                    prop_assert_eq!(rendered.lines().count(), first.symbols().len());
                }
                (Err(first), Err(again)) => prop_assert_eq!(first, again),
                (first, again) => {
                    return Err(TestCaseError::fail(format!(
                        "one shred succeeded and the other did not: {first:?} / {again:?}"
                    )));
                }
            }
        }
    }
}
