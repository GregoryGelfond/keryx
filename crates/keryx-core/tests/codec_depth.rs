//! Bounded depth at the binary payload door (spec §8, §26; the threat model's property 3). A
//! payload's compositional nesting — message-typed fields below the root — meets two bounds on
//! this door, and the instrument pins where each sits, measured against the pinned engine. The
//! walk's uniform ceiling is 99 levels (`RECURSION_LIMIT − 1`, every format alike): a payload
//! nesting 99 shreds whole, and one nesting 100 is `PayloadTooDeep` — the walk's refusal, not the
//! engine's, because the engine's decode recursion limit (100) is spent one level per nested
//! message and trips only at the 101st, so the deepest binary payload it decodes nests exactly
//! 100 levels and the ceiling is the binding refusal for that one level. From 101 on the engine
//! refuses at decode (`UndecodablePayload`), and there the ceiling stands as defense-in-depth.
//! Either way the refusal is total — one diagnosis at the whole-payload locus, no facts beside it,
//! no panic — and the deepest admitted payload costs the walk heap, not call stack. The walk's
//! counter is exercised past the ceiling at a seeded depth where it lives (`codec::walk`'s
//! tests), the door being unable to deliver a decoded tree deeper than the engine admits.

use keryx_test_support as support;
use keryx_test_support::wire::delimited;

use keryx_core::codec::{Codec, Facts, PayloadFormat, Root};
use keryx_core::diagnostics::{DiagnosticKind, Diagnostics};

/// The uniform payload nesting ceiling (`codec::walk::NESTING_CEILING`): one below the engine's
/// decode recursion limit of 100.
const CEILING: usize = 99;

/// A chain of `depth` messages nested through the field numbered `tag`, each carrying nothing but
/// the next and the innermost nothing at all, built from the inside out.
fn chain(tag: u32, depth: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..depth {
        let mut outer = Vec::new();
        delimited(tag, &bytes, &mut outer);
        bytes = outer;
    }
    bytes
}

/// The recursion fixture's codec: `Tree` nests through a repeated field (`children`, #2), `A` and
/// `B` through singular fields (#1), alternating — a message costs one level either way.
fn codec() -> Codec {
    Codec::new(&support::compile_fixture("recursion.proto")).expect("the fixture builds a codec")
}

fn shred(codec: &Codec, root_type: &str, payload: &[u8]) -> Result<Facts, Diagnostics> {
    codec.shred(root_type, payload, PayloadFormat::Binary, &Root::fresh(0))
}

/// The one diagnosis a refused payload carries, at the whole-payload locus: its kind and detail.
fn the_one_refusal(diagnostics: &Diagnostics) -> (DiagnosticKind, &str) {
    assert_eq!(diagnostics.len(), 1, "one diagnosis: {diagnostics}");
    let diagnostic = diagnostics.iter().next().expect("one diagnostic");
    assert!(
        diagnostic.locus().is_whole(),
        "the whole-payload locus: {diagnostic}"
    );
    (diagnostic.kind(), diagnostic.detail())
}

#[test]
fn a_payload_at_the_ceiling_shreds_whole_and_one_past_it_is_refused_by_the_walk() {
    let codec = codec();

    // 99 levels: the root and 99 nested trees shred, each its sort atom and its materialised
    // label, the deepest occupant 99 `children` applications below `r0` — every fact once on
    // both seams.
    let facts = shred(&codec, "keryx.rec.Tree", &chain(2, CEILING))
        .expect("the deepest admitted payload shreds");
    assert_eq!(facts.symbols().len(), 2 * (CEILING + 1));
    let rendered = facts.render().expect("renders");
    assert_eq!(rendered.lines().count(), facts.symbols().len());
    assert_eq!(
        rendered
            .lines()
            .map(|line| line.matches("children(").count())
            .max(),
        Some(CEILING)
    );
    let facts = shred(&codec, "keryx.rec.A", &chain(1, CEILING)).expect("shreds");
    assert_eq!(facts.symbols().len(), CEILING + 1);

    // 100 levels: the engine decodes it — its limit is one level further — and the walk refuses
    // it: `PayloadTooDeep`, once, naming the over-deep sort, its depth, and the ceiling, with no
    // facts beside it.
    for (root_type, tag, over_deep) in [
        ("keryx.rec.Tree", 2, "keryx.rec.Tree"),
        ("keryx.rec.A", 1, "keryx.rec.A"),
    ] {
        let diagnostics =
            shred(&codec, root_type, &chain(tag, CEILING + 1)).expect_err("past the ceiling");
        let (kind, detail) = the_one_refusal(&diagnostics);
        assert_eq!(kind, DiagnosticKind::PayloadTooDeep, "{root_type}");
        assert!(
            detail.contains(over_deep) && detail.contains("100") && detail.contains("99"),
            "the detail names the sort, the depth, and the ceiling: {detail}"
        );
    }
}

#[test]
fn past_the_engine_s_limit_the_decode_refuses_before_any_walk() {
    // 101 levels and far beyond: the engine's recursion limit trips at the 101st nested message —
    // `UndecodablePayload` at the whole-payload locus, naming the root type — and nothing is
    // walked. A payload ten thousand levels deep (some 40 KiB) is refused the same way, with no
    // call stack to exhaust on the way.
    let codec = codec();
    for depth in [CEILING + 2, 1_000, 10_000] {
        for (root_type, tag) in [("keryx.rec.Tree", 2), ("keryx.rec.A", 1)] {
            let diagnostics =
                shred(&codec, root_type, &chain(tag, depth)).expect_err("past the engine's limit");
            let (kind, detail) = the_one_refusal(&diagnostics);
            assert_eq!(
                kind,
                DiagnosticKind::UndecodablePayload,
                "{root_type} at {depth}"
            );
            assert!(
                detail.contains(root_type),
                "the detail names the root type: {detail}"
            );
        }
    }
}
