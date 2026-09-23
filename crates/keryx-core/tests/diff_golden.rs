//! The JSON changeset (`Comparison::to_json`), pinned to a golden for the fixture pair
//! `evolution_v1.proto` -> `evolution_v2.proto` (spec §13.4, §27). `to_json` is a pure,
//! deterministic function of the two mappings (P3), so equality is the whole contract. The golden
//! is the captured product — the one compact line `to_json` returns — stored newline-terminated
//! like every text file beside it (the product itself ends with no newline, which
//! `the_changeset_is_compact_and_the_comparison_is_breaking` pins), so the byte-exact assertion
//! is the product plus that one line terminator. Generated once through the public path
//! (`ingest` -> `policy::map` -> `diff::compare`), verified by eye against the record shape, and
//! committed; a diff here is a real change, intended or a regression. The bridge text in it is
//! themelios-rendered, so the golden is captured, never hand-written. The remaining tests state
//! the record contract legibly: each record is its row of `changes`, in that order, and the
//! optional keys are present exactly where the row has them.

use keryx_test_support as support;

use keryx_core::descriptor::ingest;
use keryx_core::diff::{self, Change, ChangeKind};
use keryx_core::policy::{self, Mapping};
use serde_json::Value;

fn mapping_of(fixture: &str) -> Mapping {
    let schema = ingest(&support::compile_fixture(fixture)).expect("ingests");
    policy::map(&schema).expect("maps")
}

/// The fixture pair's two mappings, old then new.
fn mappings() -> (Mapping, Mapping) {
    (
        mapping_of("evolution_v1.proto"),
        mapping_of("evolution_v2.proto"),
    )
}

/// The fixture pair's changeset text.
fn changeset() -> String {
    let (old, new) = mappings();
    diff::compare(&old, &new)
        .expect("two versions of one schema are comparable")
        .to_json()
}

/// The changeset parsed back: the records of the one JSON array the text is.
fn records() -> Vec<Value> {
    let text = changeset();
    let Value::Array(records) = serde_json::from_str(&text).expect("the changeset is JSON") else {
        panic!("the changeset is one array: {text}")
    };
    records
}

/// The one record of `kind` whose `new_path` (or, for a removal, `old_path`) is `path`.
fn record(records: &[Value], kind: &str, path: &str) -> Value {
    let matching: Vec<&Value> = records
        .iter()
        .filter(|record| {
            record["kind"] == kind && (record["new_path"] == path || record["old_path"] == path)
        })
        .collect();
    let [record] = matching.as_slice() else {
        panic!("one `{kind}` record at `{path}`: {matching:?}")
    };
    Value::clone(record)
}

#[test]
fn the_changeset_is_pinned_to_its_golden() {
    // The golden is newline-terminated text; the product is not, so the comparison adds the one
    // line terminator to the product rather than trimming the file.
    assert_eq!(
        format!("{}\n", changeset()),
        include_str!("golden/evolution.json")
    );
}

#[test]
fn each_record_is_its_row_of_changes_in_that_order() {
    // The view adds nothing and reorders nothing: record by record, the paths, signatures,
    // breaking bit, number, and bridge are the row's own, the bridge text verbatim through JSON's
    // escaping of its newlines.
    let records = records();
    let (old, new) = mappings();
    let comparison = diff::compare(&old, &new).expect("comparable");
    let changes = comparison.changes();
    assert_eq!(records.len(), changes.len());
    assert!(!changes.is_empty(), "the fixture pair changes");
    for (record, change) in records.iter().zip(&changes) {
        assert_eq!(record["package"], change.package(), "{record}");
        assert_eq!(
            record["old_path"],
            Value::from(change.old_path()),
            "{record}"
        );
        assert_eq!(
            record["new_path"],
            Value::from(change.new_path()),
            "{record}"
        );
        assert_eq!(
            record["old"],
            Value::from(change.old_signature()),
            "{record}"
        );
        assert_eq!(
            record["new"],
            Value::from(change.new_signature()),
            "{record}"
        );
        assert_eq!(
            record["breaking"],
            Value::from(change.is_breaking()),
            "{record}"
        );
        assert_eq!(
            record.get("number").cloned(),
            change.number().map(Value::from),
            "{record}"
        );
        assert_eq!(
            record.get("bridge").cloned(),
            change.bridge().map(Value::from),
            "{record}"
        );
    }
    // The slugs are the kinds' names in `snake_case` — the fixture pair's five of the seventeen,
    // in the rows' order: package, then relative path, then number, an element's own row first.
    let kinds: Vec<(ChangeKind, Value)> = changes
        .iter()
        .map(Change::kind)
        .zip(records.iter().map(|record| record["kind"].clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            (ChangeKind::Removed, Value::from("removed")),
            (ChangeKind::MessageAdded, Value::from("message_added")),
            (ChangeKind::Added, Value::from("added")),
            (ChangeKind::ValueAdded, Value::from("value_added")),
            (ChangeKind::Renamed, Value::from("renamed")),
            (ChangeKind::Added, Value::from("added")),
        ]
    );
}

#[test]
fn a_bridged_rename_record_carries_both_paths_and_the_bridge() {
    let records = records();
    let renamed = record(&records, "renamed", "thermal.v2.Reading.celsius");
    assert_eq!(renamed["old_path"], "thermal.v1.Reading.temp_c");
    assert_eq!(renamed["new_path"], "thermal.v2.Reading.celsius");
    assert_eq!(renamed["old"], "temp_c/2 int32 total");
    assert_eq!(renamed["new"], "celsius/2 int32 total");
    assert_eq!(renamed["number"], 2);
    assert_eq!(renamed["breaking"], Value::Bool(false));
    let bridge = renamed["bridge"]
        .as_str()
        .expect("a pure rename is bridged");
    assert!(
        bridge.starts_with("%! temp_c/2 reads celsius/2") && bridge.ends_with(".\n"),
        "{bridge}"
    );
    assert!(
        bridge.contains("temp_c(A, B) :- celsius(A, B)."),
        "{bridge}"
    );
}

#[test]
fn an_addition_record_has_no_old_side_and_no_bridge() {
    let records = records();
    let added = record(&records, "added", "thermal.v2.Reading.humidity");
    assert_eq!(added["old_path"], Value::Null);
    assert_eq!(added["old"], Value::Null);
    assert_eq!(added["new"], "humidity/2 int32 total");
    assert_eq!(added["number"], 3);
    assert_eq!(added["breaking"], Value::Bool(false));
    assert!(added.get("bridge").is_none(), "{added}");
}

#[test]
fn a_removal_record_has_no_new_side_and_is_breaking() {
    let records = records();
    let removed = record(&records, "removed", "thermal.v1.Alert.note");
    assert_eq!(removed["new_path"], Value::Null);
    assert_eq!(removed["new"], Value::Null);
    assert_eq!(removed["old"], "note/2 string total");
    assert_eq!(removed["breaking"], Value::Bool(true));
    assert!(removed.get("bridge").is_none(), "{removed}");
}

#[test]
fn an_element_record_omits_the_number() {
    // A message's own row has no field number, so the key is absent — not null — while its field
    // rides beneath it with one; a value row's number is the value's.
    let records = records();
    let batch = record(&records, "message_added", "thermal.v2.Batch");
    assert!(batch.get("number").is_none(), "{batch}");
    assert_eq!(batch["new"], "batch/1");
    let readings = record(&records, "added", "thermal.v2.Batch.readings");
    assert_eq!(readings["number"], 1);
    assert_eq!(readings["new"], "readings/3 reading seq");
    let critical = record(&records, "value_added", "thermal.v2.Level.LEVEL_CRITICAL");
    assert_eq!(critical["number"], 3);
    assert_eq!(critical["new"], "critical");
}

#[test]
fn every_record_carries_the_same_fixed_keys() {
    // The fixed keys are always present (a missing side is `null`); only `number` and `bridge`
    // come and go, and no other key exists.
    for record in records() {
        let Value::Object(fields) = &record else {
            panic!("a record is an object: {record}")
        };
        for key in [
            "package", "kind", "old_path", "new_path", "old", "new", "breaking",
        ] {
            assert!(fields.contains_key(key), "`{key}` in {record}");
        }
        assert!(fields["package"].is_string() && fields["kind"].is_string());
        assert!(fields["breaking"].is_boolean());
        for key in fields.keys() {
            assert!(
                matches!(
                    key.as_str(),
                    "package"
                        | "kind"
                        | "old_path"
                        | "new_path"
                        | "old"
                        | "new"
                        | "breaking"
                        | "number"
                        | "bridge"
                ),
                "an unknown key `{key}` in {record}"
            );
        }
    }
}

#[test]
fn the_changeset_is_compact_and_the_comparison_is_breaking() {
    // One line, no trailing newline: the text is the serialization and nothing beside it, so a
    // consumer adds its own line terminator once. The fixture pair's removal makes it breaking.
    let text = changeset();
    assert!(!text.contains('\n'), "compact: {text}");
    assert!(text.starts_with('[') && text.ends_with(']'));
    let (old, new) = mappings();
    assert!(diff::compare(&old, &new).expect("comparable").is_breaking());
}
