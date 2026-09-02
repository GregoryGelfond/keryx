//! Coverage for stage-1 policy (spec §21.3, §4.2, §5, §6, §7): `policy::map` over the
//! fixture corpus proves the vocabulary — sort and field predicates, presence, emitted
//! form and value treatment, view selection, and the §7.4 enum-constant lowering
//! (including its loud within-enum collision report) — and, over the `collisions`
//! fixture, §4.2 qualification and injectivity: colliding message/message and
//! message/enum sorts qualify symmetrically to the shortest disambiguating path-suffix,
//! non-colliding sorts stay bare, shared enum-value constants are not cross-qualified,
//! and the emitted /1 sort namespace is injective and deterministic.

use keryx_test_support as support;

use keryx_core::descriptor::{MapKey, Openness, Scalar, Schema, ingest};
use keryx_core::diagnostics::DiagnosticKind;
use keryx_core::policy::{
    self, EmitForm, EnumMapping, EnumValueMapping, FieldMapping, Mapping, ScalarTreatment,
    SortMapping, Totality, Unit, ValueMapping, ViewKind,
};

fn schema(fixture: &str) -> Schema {
    ingest(&support::compile_fixture(fixture)).expect("the fixture ingests")
}

fn mapping(fixture: &str) -> Mapping {
    policy::map(&schema(fixture)).expect("the fixture maps")
}

fn sort<'a>(mapping: &'a Mapping, proto: &str) -> &'a SortMapping {
    mapping
        .units()
        .iter()
        .flat_map(Unit::sorts)
        .find(|sort| sort.proto().as_str() == proto)
        .expect("sort present")
}

fn enumeration<'a>(mapping: &'a Mapping, proto: &str) -> &'a EnumMapping {
    mapping
        .units()
        .iter()
        .flat_map(Unit::enums)
        .find(|e| e.proto().as_str() == proto)
        .expect("enum present")
}

fn field<'a>(sort: &'a SortMapping, short_name: &str) -> &'a FieldMapping {
    sort.fields()
        .iter()
        .find(|field| field.proto().as_str().rsplit('.').next() == Some(short_name))
        .expect("field present")
}

fn enum_value<'a>(enumeration: &'a EnumMapping, proto_name: &str) -> &'a EnumValueMapping {
    enumeration
        .values()
        .iter()
        .find(|value| value.proto_name() == proto_name)
        .expect("enum value present")
}

#[test]
fn reading_sort_and_implicit_scalar_field() {
    let mapping = mapping("proto3.proto");
    let reading = sort(&mapping, "keryx.p3.Reading");
    assert_eq!(reading.predicate().as_str(), "reading");

    let sensor = field(reading, "sensor");
    assert_eq!(sensor.predicate().as_str(), "sensor");
    assert_eq!(sensor.arity(), 2);
    assert_eq!(sensor.form(), &EmitForm::Function);
    assert_eq!(sensor.presence(), Totality::Total);
    assert_eq!(sensor.view(), None);
}

#[test]
fn reading_message_field_is_partial_with_a_singular_view() {
    let mapping = mapping("proto3.proto");
    let reading = sort(&mapping, "keryx.p3.Reading");

    let detail = field(reading, "detail");
    assert_eq!(detail.predicate().as_str(), "detail");
    assert_eq!(detail.arity(), 2);
    assert_eq!(detail.form(), &EmitForm::Function);
    assert_eq!(detail.presence(), Totality::Partial);
    assert_eq!(detail.view(), Some(ViewKind::Singular));
    match detail.value() {
        ValueMapping::Message(referent) => assert_eq!(referent.as_str(), "detail"),
        ValueMapping::Scalar { .. } | ValueMapping::Enum(_) => {
            panic!("expected `detail` to carry a message value")
        }
    }
}

#[test]
fn reading_oneof_arms_are_partial_oneof_arm_functions() {
    let mapping = mapping("proto3.proto");
    let reading = sort(&mapping, "keryx.p3.Reading");

    for arm in ["device", "gateway"] {
        let arm_field = field(reading, arm);
        assert_eq!(
            arm_field.form(),
            &EmitForm::OneofArm {
                oneof: "source".to_owned()
            }
        );
        assert_eq!(arm_field.presence(), Totality::Partial);
    }
}

#[test]
fn field_names_lower_and_reserved_escape() {
    let mapping = mapping("field_lowering.proto");
    let sample = sort(&mapping, "keryx.fieldlower.Sample");

    // camelCase / PascalCase field names lower like sorts and enums (§4.2).
    let camel = field(sample, "camelField");
    assert_eq!(camel.predicate().as_str(), "camel_field");
    assert!(!camel.escaped());
    assert_eq!(
        field(sample, "PascalField").predicate().as_str(),
        "pascal_field"
    );

    // A field colliding with a generated identifier is escaped, and the escape is recorded as
    // data (§13.4), not re-derived from the trailing underscore.
    let reach = field(sample, "reach");
    assert_eq!(reach.predicate().as_str(), "reach_");
    assert!(reach.escaped());
}

#[test]
fn maps_scalar_value_has_no_view() {
    let mapping = mapping("maps.proto");
    let inventory = sort(&mapping, "keryx.maps.Inventory");

    let counts = field(inventory, "counts");
    assert_eq!(counts.predicate().as_str(), "counts");
    assert_eq!(counts.arity(), 3);
    assert_eq!(
        counts.form(),
        &EmitForm::Map {
            key: MapKey::String
        }
    );
    assert_eq!(
        counts.value(),
        &ValueMapping::Scalar {
            kind: Scalar::Int32,
            treatment: ScalarTreatment::Native
        }
    );
    assert_eq!(counts.view(), None);
}

#[test]
fn maps_message_value_gets_a_map_view() {
    let mapping = mapping("maps.proto");
    let inventory = sort(&mapping, "keryx.maps.Inventory");

    let items = field(inventory, "items");
    assert_eq!(items.predicate().as_str(), "items");
    assert_eq!(items.arity(), 3);
    assert_eq!(items.form(), &EmitForm::Map { key: MapKey::Int64 });
    assert_eq!(items.view(), Some(ViewKind::Map));
    match items.value() {
        ValueMapping::Message(referent) => assert_eq!(referent.as_str(), "item"),
        ValueMapping::Scalar { .. } | ValueMapping::Enum(_) => {
            panic!("expected `items` to carry a message value")
        }
    }
}

#[test]
fn proto2_presence_across_the_three_labels() {
    let mapping = mapping("proto2.proto");
    let order = sort(&mapping, "keryx.p2.Order");

    assert_eq!(field(order, "id").presence(), Totality::Partial);
    assert_eq!(field(order, "quantity").presence(), Totality::Partial);

    let tags = field(order, "tags");
    assert_eq!(tags.predicate().as_str(), "tags");
    assert_eq!(tags.arity(), 3);
    assert_eq!(tags.form(), &EmitForm::Sequence);
    assert_eq!(tags.presence(), Totality::Total);
}

#[test]
fn level_enum_lowering_strips_the_shared_prefix() {
    let mapping = mapping("proto3.proto");
    let level = enumeration(&mapping, "keryx.p3.Level");
    assert_eq!(level.predicate().as_str(), "level");
    assert_eq!(level.openness(), Openness::Open);

    assert_eq!(
        enum_value(level, "LEVEL_UNSPECIFIED").constant().as_str(),
        "unspecified"
    );
    assert_eq!(enum_value(level, "LEVEL_LOW").constant().as_str(), "low");
    assert_eq!(enum_value(level, "LEVEL_HIGH").constant().as_str(), "high");
}

#[test]
fn enum_strip_falls_back_on_a_leading_digit() {
    // §7.4: stripping `Edition`'s `EDITION_` prefix would leave `2023`/`1_test_only`, neither a
    // legal ASP constant (an identifier cannot open with a digit), so the strip falls back to
    // the unstripped form for the *whole* enum. This is the shape descriptor.proto's own
    // `Edition` carries — the §21.2 self-application dogfood surfaced it, and this fixture pins
    // the behavior directly rather than only through that third-party schema.
    let mapping = mapping("enum_digit_strip.proto");

    let edition = enumeration(&mapping, "keryx.enumdigit.Edition");
    assert_eq!(
        enum_value(edition, "EDITION_UNKNOWN").constant().as_str(),
        "edition_unknown"
    );
    assert_eq!(
        enum_value(edition, "EDITION_2023").constant().as_str(),
        "edition_2023"
    );
    assert_eq!(
        enum_value(edition, "EDITION_1_TEST_ONLY")
            .constant()
            .as_str(),
        "edition_1_test_only"
    );

    // The guard is per-enum, not blanket: a sibling whose remainder still opens with a letter
    // strips as usual (`LEVEL_LOW` → `low`).
    let level = enumeration(&mapping, "keryx.enumdigit.Level");
    assert_eq!(enum_value(level, "LEVEL_LOW").constant().as_str(), "low");
}

#[test]
fn proto2_enum_is_closed() {
    let mapping = mapping("proto2.proto");
    let grade = enumeration(&mapping, "keryx.p2.Grade");
    assert_eq!(grade.openness(), Openness::Closed);
}

#[test]
fn recursive_sorts_are_flagged_and_others_are_not() {
    let recursive = mapping("recursion.proto");
    assert!(sort(&recursive, "keryx.rec.Tree").is_recursive());
    assert!(sort(&recursive, "keryx.rec.A").is_recursive());
    assert!(sort(&recursive, "keryx.rec.B").is_recursive());

    // A separate, non-recursive fixture is not flagged (guards against a mutation that
    // hardcodes `recursive` to always-true).
    let acyclic = mapping("maps.proto");
    assert!(!sort(&acyclic, "keryx.maps.Item").is_recursive());
}

#[test]
fn docs_carry_through_to_the_mapping() {
    let mapping = mapping("docs.proto");

    let note = sort(&mapping, "keryx.docs.Note");
    assert_eq!(note.doc(), Some("A leading comment on the message."));
    assert_eq!(
        field(note, "text").doc(),
        Some("A leading comment on the field.")
    );

    let status = enumeration(&mapping, "keryx.docs.Status");
    assert_eq!(status.doc(), Some("A leading comment on the enum."));
    assert_eq!(
        enum_value(status, "STATUS_ACTIVE").doc(),
        Some("A leading comment on the enum value.")
    );
}

#[test]
fn scalar_treatment_covers_every_class() {
    let mapping = mapping("scalar_treatment.proto");
    let sample = sort(&mapping, "keryx.scalars.Sample");

    // At least one field per §6 treatment family (two for DecimalString), so a mutation
    // in any single match arm of `scalar_treatment` (e.g. `Bool` mapping to `Native`, or
    // `Bytes` to `Text`) fails here instead of passing unnoticed.
    let cases = [
        ("count", Scalar::Int32, ScalarTreatment::Native),
        ("total", Scalar::Int64, ScalarTreatment::DecimalString),
        ("checksum", Scalar::Uint64, ScalarTreatment::DecimalString),
        ("ratio", Scalar::Float, ScalarTreatment::NeedsAnnotation),
        ("active", Scalar::Bool, ScalarTreatment::Bool),
        ("payload", Scalar::Bytes, ScalarTreatment::HexString),
        ("label", Scalar::String, ScalarTreatment::Text),
    ];
    for (name, kind, treatment) in cases {
        assert_eq!(
            field(sample, name).value(),
            &ValueMapping::Scalar { kind, treatment },
            "field `{name}`"
        );
    }
}

#[test]
fn singular_enum_field_has_no_view() {
    let mapping = mapping("scalar_treatment.proto");
    let sample = sort(&mapping, "keryx.scalars.Sample");

    let kind = field(sample, "kind");
    assert_eq!(kind.arity(), 2);
    assert_eq!(kind.form(), &EmitForm::Function);
    assert_eq!(kind.presence(), Totality::Total);
    assert_eq!(kind.view(), None);
    match kind.value() {
        ValueMapping::Enum(referent) => assert_eq!(referent.as_str(), "kind"),
        ValueMapping::Scalar { .. } | ValueMapping::Message(_) => {
            panic!("expected `kind` to carry an enum value")
        }
    }
}

#[test]
fn repeated_message_field_gets_a_sequence_view() {
    let mapping = mapping("scalar_treatment.proto");
    let sample = sort(&mapping, "keryx.scalars.Sample");

    let notes = field(sample, "notes");
    assert_eq!(notes.arity(), 3);
    assert_eq!(notes.form(), &EmitForm::Sequence);
    assert_eq!(notes.presence(), Totality::Total);
    assert_eq!(notes.view(), Some(ViewKind::Sequence));
    match notes.value() {
        ValueMapping::Message(referent) => assert_eq!(referent.as_str(), "note"),
        ValueMapping::Scalar { .. } | ValueMapping::Enum(_) => {
            panic!("expected `notes` to carry a message value")
        }
    }
}

#[test]
fn repeated_and_mapped_enum_values_have_no_view() {
    let mapping = mapping("scalar_treatment.proto");
    let sample = sort(&mapping, "keryx.scalars.Sample");

    let kinds = field(sample, "kinds");
    assert_eq!(kinds.form(), &EmitForm::Sequence);
    assert_eq!(kinds.view(), None);
    match kinds.value() {
        ValueMapping::Enum(referent) => assert_eq!(referent.as_str(), "kind"),
        ValueMapping::Scalar { .. } | ValueMapping::Message(_) => {
            panic!("expected `kinds` to carry an enum value")
        }
    }

    let tags = field(sample, "tags");
    assert_eq!(
        tags.form(),
        &EmitForm::Map {
            key: MapKey::String
        }
    );
    assert_eq!(tags.view(), None);
    match tags.value() {
        ValueMapping::Enum(referent) => assert_eq!(referent.as_str(), "kind"),
        ValueMapping::Scalar { .. } | ValueMapping::Message(_) => {
            panic!("expected `tags` to carry an enum value")
        }
    }
}

#[test]
fn within_enum_constant_collision_is_reported() {
    let schema = schema("enum_collision.proto");
    let error = policy::map(&schema).expect_err("a within-enum constant collision is an error");

    assert_eq!(error.len(), 1);
    let diagnostic = error.iter().next().expect("one diagnostic");
    assert_eq!(diagnostic.kind(), DiagnosticKind::AmbiguousConstant);
    assert_eq!(diagnostic.locus().path(), Some("keryx.enumcoll.Mixed"));
}

#[test]
fn map_is_pure_and_deterministically_ordered() {
    let schema = schema("proto3.proto");
    let first = policy::map(&schema).expect("the fixture maps");
    let second = policy::map(&schema).expect("the fixture maps");
    assert_eq!(first, second);

    assert_eq!(first.units().len(), 1);
    let unit = &first.units()[0];
    assert_eq!(unit.package(), "keryx.p3");

    let sort_paths: Vec<&str> = unit.sorts().iter().map(|s| s.proto().as_str()).collect();
    assert_eq!(sort_paths, vec!["keryx.p3.Detail", "keryx.p3.Reading"]);

    let reading = sort(&first, "keryx.p3.Reading");
    let numbers: Vec<i32> = reading.fields().iter().map(FieldMapping::number).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn colliding_sorts_qualify_to_the_shortest_suffix() {
    let mapping = mapping("collisions.proto");
    // Two distinct `Status` messages share the base sort `status`. The symmetric §4.2 rule
    // qualifies BOTH by one path segment each (`dispatch__status` *and* `logistics__status`)
    // — never leaving one bare, and never over-qualifying to the fully-qualified
    // `keryx__coll__…__status` form; one segment already restores injectivity.
    let dispatch = sort(&mapping, "keryx.coll.Dispatch.Status");
    let logistics = sort(&mapping, "keryx.coll.Logistics.Status");
    assert_eq!(dispatch.predicate().as_str(), "dispatch__status");
    assert_eq!(logistics.predicate().as_str(), "logistics__status");
    assert_ne!(
        dispatch.predicate().as_str(),
        logistics.predicate().as_str()
    );
}

#[test]
fn message_and_enum_sharing_a_base_name_both_qualify() {
    let mapping = mapping("collisions.proto");
    // `keryx.coll.Mode` (a message → a `SortMapping`) and `keryx.coll.Carrier.Mode` (an enum
    // → an `EnumMapping`) share the base sort `mode` in the one message+enum /1 namespace, so
    // BOTH qualify and differ. That the enum qualifies proves `build_enum` takes its predicate
    // from the qualified `sort_of` map (not `names::enum_name`, the pre-qualification base): an
    // enum bypassing qualification would collide with the message on `mode`.
    let message_mode = sort(&mapping, "keryx.coll.Mode");
    let enum_mode = enumeration(&mapping, "keryx.coll.Carrier.Mode");
    assert_eq!(message_mode.predicate().as_str(), "coll__mode");
    assert_eq!(enum_mode.predicate().as_str(), "carrier__mode");
    assert_ne!(
        message_mode.predicate().as_str(),
        enum_mode.predicate().as_str()
    );
}

#[test]
fn non_colliding_sorts_stay_bare() {
    let mapping = mapping("collisions.proto");
    // A base name is qualified only on an actual collision: these three have distinct bare
    // names and keep them (`Dispatch` → `dispatch`, not `coll__dispatch`).
    assert_eq!(
        sort(&mapping, "keryx.coll.Dispatch").predicate().as_str(),
        "dispatch"
    );
    assert_eq!(
        sort(&mapping, "keryx.coll.Logistics").predicate().as_str(),
        "logistics"
    );
    assert_eq!(
        sort(&mapping, "keryx.coll.Carrier").predicate().as_str(),
        "carrier"
    );
}

#[test]
fn enum_value_constants_are_not_cross_qualified() {
    let mapping = mapping("collisions.proto");
    // A value name shared across enums is intended polymorphism-by-sort (like a shared field
    // name, §4.2), NOT a sort collision — qualification operates on the sort namespace only.
    // The §7.4 strip keys on each enum's own name (`GRADE_`/`RATING_`), so both `*_UNSPECIFIED`
    // lower to the SAME unqualified `unspecified`, and both `*_LOW` to `low`.
    let grade = enumeration(&mapping, "keryx.coll.Grade");
    let rating = enumeration(&mapping, "keryx.coll.Rating");
    // The enum SORTS do not collide and stay bare.
    assert_eq!(grade.predicate().as_str(), "grade");
    assert_eq!(rating.predicate().as_str(), "rating");
    // The value CONSTANTS are shared, unqualified, and identical across the two enums.
    assert_eq!(
        enum_value(grade, "GRADE_UNSPECIFIED").constant().as_str(),
        "unspecified"
    );
    assert_eq!(
        enum_value(rating, "RATING_UNSPECIFIED").constant().as_str(),
        "unspecified"
    );
    assert_eq!(enum_value(grade, "GRADE_LOW").constant().as_str(), "low");
    assert_eq!(enum_value(rating, "RATING_LOW").constant().as_str(), "low");
}

#[test]
fn the_sort_namespace_is_injective() {
    let mapping = mapping("collisions.proto");
    // Collect EVERY /1 sort predicate — messages and enums share the one namespace — and
    // assert no two coincide. Injectivity is exactly what `qualify` guarantees; a duplicate
    // would be a keryx bug (the `duplicate` diagnostic path guards the case `ingest` cannot
    // produce). Nine sorts (six messages + three enums) are present, so this is non-vacuous.
    let predicates: Vec<&str> = mapping
        .units()
        .iter()
        .flat_map(|unit| {
            unit.sorts()
                .iter()
                .map(|sort| sort.predicate().as_str())
                .chain(unit.enums().iter().map(|e| e.predicate().as_str()))
        })
        .collect();
    let unique: std::collections::BTreeSet<&str> = predicates.iter().copied().collect();
    assert_eq!(
        predicates.len(),
        9,
        "all nine sorts present: {predicates:?}"
    );
    assert_eq!(
        predicates.len(),
        unique.len(),
        "sort predicates are not injective: {predicates:?}"
    );
}

#[test]
fn qualification_is_deterministic() {
    // `map` is a pure function (P3/R4): the choice-free, Ord-least-first advance makes the
    // qualified result unique, hence stable across runs. Two INDEPENDENT parses+ingests of
    // the fixture — not one shared `Schema` mapped twice — so this guards ingest→map order
    // stability, not merely a HashMap-iteration regression inside a single `map`.
    let first = policy::map(&schema("collisions.proto")).expect("the fixture maps");
    let second = policy::map(&schema("collisions.proto")).expect("the fixture maps");
    assert_eq!(first, second);
    // Non-vacuous: a real collision was resolved (identically) both times.
    assert_eq!(
        sort(&first, "keryx.coll.Dispatch.Status")
            .predicate()
            .as_str(),
        "dispatch__status"
    );
}

#[test]
fn a_collision_that_survives_depth_one_resolves_at_depth_two() {
    let mapping = mapping("deep_collisions.proto");
    // `P.M.X` and `Q.M.X` share base `x` AND the depth-1 qualifier `m__x` (the same
    // intermediate `M`); only the second segment (`P` vs `Q`) separates them. The pass
    // advances BOTH to depth 2 — never to the fully-qualified `keryx__deep__…` form, since
    // two segments already restore injectivity.
    let p = sort(&mapping, "keryx.deep.P.M.X");
    let q = sort(&mapping, "keryx.deep.Q.M.X");
    assert_eq!(p.predicate().as_str(), "p__m__x");
    assert_eq!(q.predicate().as_str(), "q__m__x");
    assert_ne!(p.predicate().as_str(), q.predicate().as_str());
}

#[test]
fn a_three_way_collision_qualifies_minimally() {
    let mapping = mapping("deep_collisions.proto");
    // `U.K.Y`, `V.K.Y`, `W.N.Y` all share base `y`. `W.N.Y` becomes unique at depth 1
    // (`n__y`) and STOPS there, while `{U.K.Y, V.K.Y}` still collide at `k__y` and advance to
    // depth 2 — asymmetric 2/2/1 depths. This is the per-member "only as deep as needed"
    // reading, which the symmetric-unique rule realizes by advancing only the still-clashing
    // subset each round (a whole-group-to-one-depth rule would over-qualify `W.N.Y`).
    let u = sort(&mapping, "keryx.deep.U.K.Y");
    let v = sort(&mapping, "keryx.deep.V.K.Y");
    let w = sort(&mapping, "keryx.deep.W.N.Y");
    assert_eq!(u.predicate().as_str(), "u__k__y");
    assert_eq!(v.predicate().as_str(), "v__k__y");
    assert_eq!(w.predicate().as_str(), "n__y");
    // All three distinct (injective), and `W.N.Y` stopped shallower than `U`/`V`.
    assert_ne!(u.predicate().as_str(), v.predicate().as_str());
    assert_ne!(u.predicate().as_str(), w.predicate().as_str());
}

#[test]
fn distinct_sorts_that_collide_are_diagnosed() {
    // `Bar` and `Bar_` are distinct proto messages, but `lower_snake` trims the trailing `_`
    // so both lower to base `bar`; as siblings they share every qualifier and never separate.
    // Qualification is the injectivity backstop: `map` returns an error rather than silently
    // conflating two types onto one `/1` predicate. This is the REACHABLE-from-valid-input
    // duplicate path (the white-box test in `qualify` covers only the identical-path variant).
    let schema = schema("collapsing_sorts.proto");
    let error = policy::map(&schema).expect_err("a non-injective sort collapse is diagnosed");
    assert_eq!(error.len(), 1);
    let diagnostic = error.iter().next().expect("one diagnostic");
    assert_eq!(diagnostic.kind(), DiagnosticKind::UnmappableName);
    assert_eq!(diagnostic.locus().path(), Some("keryx.collapse.Bar"));
}
