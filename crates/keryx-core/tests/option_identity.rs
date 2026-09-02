//! Extension-identity resolution for keryx options (architecture §6): an
//! option is admitted only when its extension is declared in the vendored
//! `keryx/options.proto`, never merely because its full name happens to start
//! with `keryx.`. A foreign schema can coin its own extension under
//! `package keryx` with a field name that is not a themelios identifier;
//! admitting it by name alone would carry that foreign text into
//! `facts::render`'s vocabulary-only `Name::new` door and panic — a §6
//! totality violation on foreign input. This proves the fix: the foreign
//! option is excluded at ingestion, and `facts::render` stays total.

use keryx_test_support as support;

use keryx_core::descriptor::ingest;
use keryx_core::facts;

#[test]
fn a_foreign_keryx_prefixed_extension_is_excluded_and_render_stays_total() {
    let schema = ingest(&support::compile_fixture("foreign_option.proto")).expect("ingests");
    let foreign = schema
        .messages()
        .iter()
        .find(|m| m.path().as_str() == "keryx.Foreign")
        .expect("Foreign present");
    let note = foreign
        .fields()
        .iter()
        .find(|f| f.name() == "note")
        .expect("note present");

    // The foreign `(keryx.Evil)` option is not declared in the vendored
    // registry, so it is excluded by extension identity — not merely an
    // unrecognized key, but never carried into an `Annotation` at all.
    assert!(
        note.options().is_empty(),
        "a foreign keryx.-prefixed extension must not be admitted as an annotation"
    );

    // Had `Evil` been admitted, `facts::render` would have panicked lowering
    // its key through `Name::new` (not a themelios identifier) — the §6
    // totality bug the identity fix closes. This must not panic.
    facts::render(&schema).expect("renders");
}
