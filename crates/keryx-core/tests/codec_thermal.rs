//! The inbound codec (spec §11, §22) over the thermal story (§28) and the presence and
//! composite fixtures: a payload shredded, under the mapping and a root, to the ground facts
//! Part II names — the same content on both delivery seams (§11: the sorted `Symbol`s and the
//! canonical `.lp` text), presence decided from the mapping's totality (§5), sequences and maps as
//! indexed families (§7.1, §7.2), a `oneof` as partial arms (§7.3), enum values as constants
//! (§7.4), and every refusal a diagnostic at the field's path (§26).

use std::path::{Path, PathBuf};

use keryx_test_support as support;
use keryx_test_support::wire::delimited;
use prost::encoding;
use themelios_program::prelude::Sign;

use keryx_core::codec::{Codec, Facts, PayloadFormat, Root};
use keryx_core::diagnostics::{DiagnosticKind, Diagnostics};
use keryx_core::policy::Element;
use keryx_core::{Name, Symbol};

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

/// A binary payload shredded from the fresh root `r0`.
fn shred(codec: &Codec, root_type: &str, payload: &[u8]) -> Facts {
    codec
        .shred(root_type, payload, PayloadFormat::Binary, &Root::fresh(0))
        .expect("the payload shreds")
}

/// A binary payload refused from the fresh root `r0`.
fn refused(codec: &Codec, root_type: &str, payload: &[u8]) -> Diagnostics {
    codec
        .shred(root_type, payload, PayloadFormat::Binary, &Root::fresh(0))
        .expect_err("the payload is refused")
}

// Wire-format builders: the payloads are written as bytes on the wire, never through the
// engine's encoder, so the door is seen to read the wire.

/// A thermal `Reading { sensor = 1; temp_c = 2 }`.
fn reading(sensor: &str, temp_c: i32) -> Vec<u8> {
    let mut buf = Vec::new();
    delimited(1, sensor.as_bytes(), &mut buf);
    encoding::int32::encode(2, &temp_c, &mut buf);
    buf
}

/// A thermal `ReadingBatch { repeated Reading readings = 1 }`.
fn batch(readings: &[Vec<u8>]) -> Vec<u8> {
    let mut buf = Vec::new();
    for reading in readings {
        delimited(1, reading, &mut buf);
    }
    buf
}

// Expected symbols, built as a client of keryx builds them: through the re-exported `Symbol`
// and `Name` alone (R1) — the one themelios item named directly is the strong sign, which
// `Symbol::Function` carries.

fn name(text: &str) -> Name {
    Name::new(text).expect("an identifier")
}

fn function(functor: &str, arguments: Vec<Symbol>) -> Symbol {
    Symbol::Function {
        name: name(functor),
        arguments,
        sign: Sign::Positive,
    }
}

fn constant(text: &str) -> Symbol {
    function(text, Vec::new())
}

fn text(value: &str) -> Symbol {
    Symbol::String(value.to_owned())
}

fn number(value: i32) -> Symbol {
    Symbol::Number(value)
}

/// The occupant term of the `i`th reading of the batch at `r0` (§4.1): `readings(r0, i)`.
fn slot(i: i32) -> Symbol {
    function("readings", vec![constant("r0"), number(i)])
}

/// Whether `facts` carries `symbol` on the symbol seam.
fn has(facts: &Facts, symbol: &Symbol) -> bool {
    facts.symbols().contains(symbol)
}

/// Whether any fact on the symbol seam is over the predicate `predicate`.
fn mentions(facts: &Facts, predicate: &str) -> bool {
    facts
        .symbols()
        .iter()
        .any(|symbol| matches!(symbol, Symbol::Function { name, .. } if name.as_str() == predicate))
}

/// The two seams carry identical content (§11): `symbols()` in `Symbol::Ord`, and the rendering
/// with exactly one fact per symbol — the rendering's canonical program de-duplicates, so a count
/// equality proves no fact was emitted twice.
fn assert_seams_agree(facts: &Facts) -> String {
    assert!(
        facts.symbols().is_sorted(),
        "the symbol seam is in `Symbol::Ord`"
    );
    let rendered = facts.render().expect("the facts render");
    assert_eq!(
        rendered.lines().count(),
        facts.symbols().len(),
        "one rendered fact per symbol: {rendered}"
    );
    rendered
}

#[test]
fn the_thermal_batch_shreds_to_the_section_28_facts() {
    // The spec's own payload (§28): two readings, a sequence of messages over two total scalars.
    let codec = thermal_codec();
    let payload = batch(&[reading("s-101", 44), reading("s-107", 21)]);
    let facts = shred(&codec, "ReadingBatch", &payload);

    // The `.lp` seam: the §28 facts in themelios's canonical statement order.
    let rendered = assert_seams_agree(&facts);
    assert_eq!(
        rendered,
        "reading(readings(r0, 0)).\n\
         reading(readings(r0, 1)).\n\
         reading_batch(r0).\n\
         sensor(readings(r0, 0), \"s-101\").\n\
         sensor(readings(r0, 1), \"s-107\").\n\
         temp_c(readings(r0, 0), 44).\n\
         temp_c(readings(r0, 1), 21).\n"
    );

    // The symbol seam: exactly the §28 atoms, in `Symbol::Ord`.
    let mut expected = vec![
        function("reading_batch", vec![constant("r0")]),
        function("reading", vec![slot(0)]),
        function("sensor", vec![slot(0), text("s-101")]),
        function("temp_c", vec![slot(0), number(44)]),
        function("reading", vec![slot(1)]),
        function("sensor", vec![slot(1), text("s-107")]),
        function("temp_c", vec![slot(1), number(21)]),
    ];
    expected.sort();
    assert_eq!(facts.symbols(), expected.as_slice());
}

#[test]
fn an_implicit_zero_still_emits_its_atom_with_the_default_materialised() {
    // §5: an IMPLICIT field's atom always exists — a `temp_c` of 0 emits `temp_c(…, 0)` exactly
    // as 44 emits `temp_c(…, 44)`; the wire cannot distinguish zero from unset, and neither does
    // the shred.
    let codec = thermal_codec();
    let facts = shred(&codec, "ReadingBatch", &batch(&[reading("s-101", 0)]));
    assert!(has(&facts, &function("temp_c", vec![slot(0), number(0)])));
    assert!(
        assert_seams_agree(&facts).contains("temp_c(readings(r0, 0), 0).\n"),
        "the materialised zero renders"
    );

    // A reading carrying nothing at all materialises both defaults.
    let facts = shred(&codec, "ReadingBatch", &batch(&[Vec::new()]));
    assert!(has(&facts, &function("sensor", vec![slot(0), text("")])));
    assert!(has(&facts, &function("temp_c", vec![slot(0), number(0)])));

    // An empty batch: the root's sort atom alone — a total collection emits per element, and
    // there are none.
    let facts = shred(&codec, "ReadingBatch", &[]);
    assert_eq!(assert_seams_agree(&facts), "reading_batch(r0).\n");
}

#[test]
fn a_codec_from_source_shreds_identically() {
    // The two construction doors — a descriptor set and `.proto` source — build one codec: the
    // same mapping, and the same facts for the same payload.
    let from_set = thermal_codec();
    let from_source = Codec::from_source(
        &[thermal_dir().join("thermal.proto")],
        &[thermal_dir(), support::vendored()],
    )
    .expect("the thermal source builds a codec");
    assert_eq!(from_set.mapping(), from_source.mapping());

    let payload = batch(&[reading("s-101", 44), reading("s-107", 21)]);
    let a = shred(&from_set, "ReadingBatch", &payload);
    let b = shred(&from_source, "ReadingBatch", &payload);
    assert_eq!(a.symbols(), b.symbols());
    assert_eq!(a.render().expect("renders"), b.render().expect("renders"));
}

#[test]
fn an_unknown_or_ambiguous_root_type_is_refused_before_any_decoding() {
    // A name no message of the schema bears: `UnknownRootType` at the whole-payload locus,
    // naming the type as given — the payload is never decoded (a malformed one refuses the
    // same way).
    let codec = thermal_codec();
    for payload in [&[][..], &[0xff, 0xff, 0xff][..]] {
        let diagnostics = refused(&codec, "Absent", payload);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.iter().next().expect("one diagnostic");
        assert_eq!(diagnostic.kind(), DiagnosticKind::UnknownRootType);
        assert!(diagnostic.locus().is_whole());
        assert!(
            diagnostic.detail().contains("Absent"),
            "the detail names the type as given: {diagnostic}"
        );
    }

    // The fully-qualified name and the short name resolve alike; protoc's leading-dot spelling
    // is neither, and misses.
    assert!(
        codec
            .shred(
                "thermal.v1.ReadingBatch",
                &[],
                PayloadFormat::Binary,
                &Root::fresh(0)
            )
            .is_ok()
    );
    assert_eq!(
        refused(&codec, ".thermal.v1.ReadingBatch", &[])
            .iter()
            .next()
            .expect("one diagnostic")
            .kind(),
        DiagnosticKind::UnknownRootType
    );

    // A short name two messages share (`Dispatch.Status` and `Logistics.Status`) is ambiguous:
    // refused, the detail listing both, and only the fully-qualified name separates them.
    let codec = fixture_codec("collisions.proto");
    let diagnostics = refused(&codec, "Status", &[]);
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.iter().next().expect("one diagnostic");
    assert_eq!(diagnostic.kind(), DiagnosticKind::UnknownRootType);
    assert!(diagnostic.locus().is_whole());
    assert!(
        diagnostic.detail().contains("keryx.coll.Dispatch.Status")
            && diagnostic.detail().contains("keryx.coll.Logistics.Status"),
        "the detail lists the candidates: {diagnostic}"
    );
    let facts = shred(&codec, "keryx.coll.Dispatch.Status", &[]);
    let Some(Element::Sort(sort)) = codec.mapping().element("keryx.coll.Dispatch.Status") else {
        panic!("the resolved root is a sort of the mapping")
    };
    assert!(has(
        &facts,
        &function(sort.predicate().as_str(), vec![constant("r0")])
    ));
    assert!(has(
        &facts,
        &function("code", vec![constant("r0"), number(0)])
    ));
}

#[test]
fn an_explicit_field_emits_its_atom_only_when_set() {
    // §5, the partial branch: a proto3 `optional` scalar (`calibration`, #3) and a singular
    // message field (`detail`, #5) emit iff the wire carried them, while the IMPLICIT siblings
    // materialise their defaults whatever the wire carried.
    let codec = fixture_codec("proto3.proto");

    // Nothing on the wire: the total fields' atoms, and no others.
    let facts = shred(&codec, "Reading", &[]);
    assert_eq!(
        assert_seams_agree(&facts),
        "level(r0, unspecified).\n\
         reading(r0).\n\
         sensor(r0, \"\").\n\
         temp_c(r0, 0).\n"
    );
    assert!(!mentions(&facts, "calibration"));
    assert!(!mentions(&facts, "detail"));
    assert!(!mentions(&facts, "note"));

    // `calibration` carried as an explicit zero: present, so its atom exists — the very zero
    // the total `temp_c` materialises, here emitted because the wire carried it.
    let mut bytes = Vec::new();
    encoding::int32::encode(3, &0, &mut bytes);
    let facts = shred(&codec, "Reading", &bytes);
    assert!(has(
        &facts,
        &function("calibration", vec![constant("r0"), number(0)])
    ));
    assert_seams_agree(&facts);

    // `detail` carried: the occupant `detail(r0)` (§4.1 item 3), its occupancy atom
    // `detail(detail(r0))` (item 4), and the occupant's own field.
    let mut detail = Vec::new();
    delimited(1, b"calibrated", &mut detail);
    let mut bytes = Vec::new();
    delimited(5, &detail, &mut bytes);
    let facts = shred(&codec, "Reading", &bytes);
    let occupant = function("detail", vec![constant("r0")]);
    assert!(has(&facts, &function("detail", vec![occupant.clone()])));
    assert!(has(
        &facts,
        &function("note", vec![occupant, text("calibrated")])
    ));
    assert_eq!(
        facts.symbols().len(),
        6,
        "the four total atoms plus the occupancy atom and its field: {:?}",
        facts.symbols()
    );
    assert_seams_agree(&facts);
}

#[test]
fn a_oneof_shreds_as_partial_arms_the_present_one_alone() {
    // §7.3: each arm is an ordinary partial function on the parent sort — the carried arm's
    // atom exists, the other arm's does not, and no discriminator atom is minted.
    let codec = fixture_codec("proto3.proto");

    let mut bytes = Vec::new();
    delimited(6, b"dev", &mut bytes);
    let facts = shred(&codec, "Reading", &bytes);
    assert!(has(
        &facts,
        &function("device", vec![constant("r0"), text("dev")])
    ));
    assert!(!mentions(&facts, "gateway"));
    assert!(!mentions(&facts, "source"));

    let mut bytes = Vec::new();
    delimited(7, b"gw", &mut bytes);
    let facts = shred(&codec, "Reading", &bytes);
    assert!(has(
        &facts,
        &function("gateway", vec![constant("r0"), text("gw")])
    ));
    assert!(!mentions(&facts, "device"));

    let facts = shred(&codec, "Reading", &[]);
    assert!(!mentions(&facts, "device") && !mentions(&facts, "gateway"));
}

#[test]
fn an_enum_value_lowers_to_its_constant_and_an_unknown_number_is_refused() {
    // §7.4: a declared number lowers to the value's constant (prefix stripped); an IMPLICIT enum
    // left unset materialises its first value; a number the enum does not declare — legal on
    // the wire for a proto3 (open) enum — is a structured refusal at the field's path.
    let codec = fixture_codec("proto3.proto");

    let mut bytes = Vec::new();
    encoding::int32::encode(4, &2, &mut bytes);
    let facts = shred(&codec, "Reading", &bytes);
    assert!(has(
        &facts,
        &function("level", vec![constant("r0"), constant("high")])
    ));

    let mut bytes = Vec::new();
    encoding::int32::encode(4, &7, &mut bytes);
    let diagnostics = refused(&codec, "Reading", &bytes);
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.iter().next().expect("one diagnostic");
    assert_eq!(diagnostic.kind(), DiagnosticKind::UnknownEnumValue);
    assert_eq!(diagnostic.locus().path(), Some("keryx.p3.Reading.level"));
    assert!(
        diagnostic.detail().contains('7') && diagnostic.detail().contains("keryx.p3.Level"),
        "the detail names the number and the enum: {diagnostic}"
    );
}

#[test]
fn map_entries_shred_key_sorted_with_keys_lowered_per_section_6() {
    // §7.2: a scalar-valued map is `f(P, K, V)`, a message-valued one has occupants `f(P, K)`
    // with occupancy atoms; keys map per §6, so the `int64` keys of `items` travel as decimal
    // strings. The engine's map is unordered: the same entries in either wire order shred to the
    // same facts (the determinism the threat model requires).
    let codec = fixture_codec("maps.proto");
    let count = |key: &[u8], value: i32| {
        let mut entry = Vec::new();
        delimited(1, key, &mut entry);
        encoding::int32::encode(2, &value, &mut entry);
        entry
    };
    let item = |key: i64, sku: &[u8]| {
        let mut item = Vec::new();
        delimited(1, sku, &mut item);
        let mut entry = Vec::new();
        encoding::int64::encode(1, &key, &mut entry);
        delimited(2, &item, &mut entry);
        entry
    };
    let mut forward = Vec::new();
    delimited(1, &count(b"a", 1), &mut forward);
    delimited(1, &count(b"b", 2), &mut forward);
    delimited(2, &item(-1, b"y"), &mut forward);
    delimited(2, &item(20, b"x"), &mut forward);
    let mut backward = Vec::new();
    delimited(2, &item(20, b"x"), &mut backward);
    delimited(2, &item(-1, b"y"), &mut backward);
    delimited(1, &count(b"b", 2), &mut backward);
    delimited(1, &count(b"a", 1), &mut backward);

    let facts = shred(&codec, "Inventory", &forward);
    assert_eq!(
        assert_seams_agree(&facts),
        "counts(r0, \"a\", 1).\n\
         counts(r0, \"b\", 2).\n\
         inventory(r0).\n\
         item(items(r0, \"-1\")).\n\
         item(items(r0, \"20\")).\n\
         sku(items(r0, \"-1\"), \"y\").\n\
         sku(items(r0, \"20\"), \"x\").\n"
    );
    let reordered = shred(&codec, "Inventory", &backward);
    assert_eq!(facts.symbols(), reordered.symbols());
    assert_eq!(
        facts.render().expect("renders"),
        reordered.render().expect("renders")
    );
}

#[test]
fn a_caller_named_root_is_the_constant_every_fact_hangs_from() {
    // §4.1 item 6: the library seam's root is a caller-supplied constant — the only extrinsic
    // identity — and every occupant beneath it is derived from it.
    let codec = thermal_codec();
    let root = Root::named(name("batch7"));
    let facts = codec
        .shred(
            "ReadingBatch",
            &batch(&[reading("s-101", 44)]),
            PayloadFormat::Binary,
            &root,
        )
        .expect("shreds");
    assert_eq!(
        assert_seams_agree(&facts),
        "reading(readings(batch7, 0)).\n\
         reading_batch(batch7).\n\
         sensor(readings(batch7, 0), \"s-101\").\n\
         temp_c(readings(batch7, 0), 44).\n"
    );
}

#[test]
fn every_refusal_is_collected_and_no_partial_shred_is_delivered() {
    // §6/§26: the walk collects every diagnosis before returning, in field order, and delivers
    // no facts beside them. `Sample.ratio` is an unannotated `float`, refused whenever the walk
    // reaches it — its materialised zero included (§5) — so an empty `Sample` is already refused;
    // a `label` carrying a tab adds `UnrepresentableText` behind it.
    let codec = fixture_codec("scalar_treatment.proto");
    let diagnostics = refused(&codec, "Sample", &[]);
    assert_eq!(diagnostics.len(), 1);
    let ratio = diagnostics.iter().next().expect("one diagnostic");
    assert_eq!(ratio.kind(), DiagnosticKind::UnannotatedFloat);
    assert_eq!(ratio.locus().path(), Some("keryx.scalars.Sample.ratio"));

    let mut bytes = Vec::new();
    delimited(7, b"a\tb", &mut bytes);
    let diagnostics = refused(&codec, "Sample", &bytes);
    let kinds: Vec<(DiagnosticKind, Option<&str>)> = diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.kind(), diagnostic.locus().path()))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                DiagnosticKind::UnannotatedFloat,
                Some("keryx.scalars.Sample.ratio")
            ),
            (
                DiagnosticKind::UnrepresentableText,
                Some("keryx.scalars.Sample.label")
            ),
        ]
    );
}

#[test]
fn compositional_nesting_lives_inside_the_path_terms() {
    // §4.1/§8: a message-typed slot's occupant is its access path from the root, however deep —
    // `children(children(r0, 0), 0)` — and every level is a sort member.
    let codec = fixture_codec("recursion.proto");
    let leaf = |label: &[u8]| {
        let mut node = Vec::new();
        delimited(1, label, &mut node);
        node
    };
    let node = |label: &[u8], child: &[u8]| {
        let mut node = leaf(label);
        delimited(2, child, &mut node);
        node
    };
    let payload = node(b"a", &node(b"b", &leaf(b"c")));
    let facts = shred(&codec, "Tree", &payload);
    let r0 = constant("r0");
    let first = function("children", vec![r0.clone(), number(0)]);
    let second = function("children", vec![first.clone(), number(0)]);
    let mut expected = vec![
        function("tree", vec![r0.clone()]),
        function("label", vec![r0, text("a")]),
        function("tree", vec![first.clone()]),
        function("label", vec![first, text("b")]),
        function("tree", vec![second.clone()]),
        function("label", vec![second, text("c")]),
    ];
    expected.sort();
    assert_eq!(facts.symbols(), expected.as_slice());
    assert_seams_agree(&facts);
}
