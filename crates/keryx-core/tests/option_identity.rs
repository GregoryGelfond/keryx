//! The option file-name heuristic (architecture §6): `descriptor::options::read` admits a
//! custom option only when its extension is declared in a file *named* `keryx/options.proto`,
//! not merely because the extension's full name starts with `keryx.` — a best-effort stand-in
//! for true extension identity, which waits for the real registry (Increment 5). A foreign
//! schema can coin its own extension under `package keryx` in a differently-named file; this
//! fixture (`foreign_option.proto`) does exactly that, and the heuristic excludes it at
//! ingestion, so its non-identifier field name never reaches an `Annotation`. The render check
//! is a backstop on §6 totality: were such a key ever to reach `facts`, it lowers through
//! `terms::try_constant` (not the vocabulary `expect`), which diagnoses a non-identifier rather
//! than panicking — so `schema_facts::render` stays total either way.

use keryx_test_support as support;

use keryx_core::descriptor::ingest;
use keryx_core::schema_facts;

#[test]
fn a_foreign_keryx_prefixed_extension_is_excluded() {
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

    // The foreign `(keryx.Evil)` option is declared in this fixture's own file, not in
    // `keryx/options.proto`, so the file-name heuristic excludes it — never carried into an
    // `Annotation` at all, not merely admitted as an unrecognized key.
    assert!(
        note.options().is_empty(),
        "a foreign keryx.-prefixed extension must not be admitted as an annotation"
    );

    // A backstop: even had a foreign key reached `facts`, it lowers through `terms::try_constant`,
    // which diagnoses a non-identifier rather than panicking, so render stays total (§6).
    schema_facts::render(&schema).expect("renders");
}
