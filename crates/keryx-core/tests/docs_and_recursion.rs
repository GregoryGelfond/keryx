//! Doc comments read from `SourceCodeInfo` (§13.1) and the containment-cycle
//! `recursive` flag (§8), proven end to end through `ingest`.

mod support;

use keryx_core::descriptor::ingest;

#[test]
fn recursive_messages_are_flagged_and_others_are_not() {
    let schema = ingest(&support::compile_fixture("recursion.proto")).expect("ingests");
    let is_recursive = |path: &str| {
        schema
            .messages()
            .iter()
            .find(|m| m.path().as_str() == path)
            .unwrap_or_else(|| panic!("{path} present"))
            .is_recursive()
    };
    assert!(is_recursive("keryx.rec.Tree"));
    assert!(is_recursive("keryx.rec.A"));
    assert!(is_recursive("keryx.rec.B"));

    // A separate ingest of a non-recursive fixture is not flagged.
    let maps = ingest(&support::compile_fixture("maps.proto")).expect("ingests");
    let item = maps
        .messages()
        .iter()
        .find(|m| m.path().as_str() == "keryx.maps.Item")
        .expect("Item present");
    assert!(!item.is_recursive());
}

#[test]
fn doc_comments_ride_from_source_info() {
    let schema = ingest(&support::compile_fixture("docs.proto")).expect("ingests");
    let note = schema
        .messages()
        .iter()
        .find(|m| m.path().as_str() == "keryx.docs.Note")
        .expect("Note present");
    assert_eq!(note.doc(), Some("A leading comment on the message."));

    let field = note
        .fields()
        .iter()
        .find(|f| f.name() == "text")
        .expect("text present");
    assert_eq!(field.doc(), Some("A leading comment on the field."));

    let oneof = note
        .oneofs()
        .iter()
        .find(|o| o.name() == "pick")
        .expect("pick present");
    assert_eq!(oneof.doc(), Some("A leading comment on the oneof."));

    let status = schema
        .enums()
        .iter()
        .find(|e| e.path().as_str() == "keryx.docs.Status")
        .expect("Status present");
    assert_eq!(status.doc(), Some("A leading comment on the enum."));

    let active = status
        .values()
        .iter()
        .find(|v| v.name() == "STATUS_ACTIVE")
        .expect("STATUS_ACTIVE present");
    assert_eq!(active.doc(), Some("A leading comment on the enum value."));
}
