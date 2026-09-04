//! The manifest — the number↔name binding and evolution contract (spec §13.4, Appendix B;
//! architecture §3). A line-oriented text: a header (schema hash, keryx version, target,
//! profile, shape), then per package one `sort` line per message/enum and one record per
//! field/value, binding the fully-qualified proto path and field number to the emitted
//! name/arity/shape and recording qualifier/escape divergence. A pure, deterministic
//! function of the [`Mapping`] (P3); *write* only at present — read/diff (`keryx diff`) is
//! Increment 5. The final grammar is open (spec §32 item 7); this is the v0 form.
//!
//! [`Mapping`]: crate::policy::model::Mapping

use std::fmt::Write as _;

use crate::descriptor::model::{Openness, Scalar};
use crate::policy::model::{
    Element, EmitForm, EnumMapping, EnumValueMapping, FieldMapping, SortMapping, Totality, Unit,
    ValueMapping,
};

/// The manifest text for one generation unit — a package (spec §13.4, `<pkg>.keryx-manifest`).
/// `schema_hash` is the caller's content hash of the descriptor set (e.g. `sha256:…`); keryx
/// does not hash bytes it was handed a `Mapping` for. Every other string that reaches the
/// output is a themelios `Name`/`FqName`, newline-free by construction; `schema_hash` is the
/// one caller-supplied opaque string with no such guarantee, so it is a precondition that it
/// must not itself contain a newline — the manifest's line-oriented format assumes it
/// doesn't, and a caller that passed one would silently split the header across lines. A
/// pure, deterministic function of the unit and hash (P3).
#[must_use]
pub fn write(unit: &Unit, schema_hash: &str) -> String {
    let mut out = String::new();
    out.push_str("keryx-manifest v0\n");
    let _ = writeln!(
        out,
        "schema-hash {schema_hash}  package {}  target clingo  profile -  shape -  keryx {}",
        unit.package().as_str(),
        env!("CARGO_PKG_VERSION"),
    );
    out.push_str(&records(unit));
    out
}

/// The manifest's record lines for one unit — the per-sort/field and per-enum/value bindings
/// (spec §13.4) *without* the header. [`write()`] is the header (schema hash, target, profile,
/// shape, keryx version) followed by these records; `explain` (spec §21.3, human lore) renders
/// the records alone, since the §13.4 manifest — an evolution contract, header included — is a
/// distinct artifact, and a null `schema-hash -` in an explanation would misrepresent it as
/// computed. A pure, deterministic function of the unit (P3).
#[must_use]
pub fn records(unit: &Unit) -> String {
    let mut out = String::new();
    for sort in unit.sorts() {
        sort_lines(&mut out, sort);
    }
    for enumeration in unit.enums() {
        enum_lines(&mut out, enumeration);
    }
    out
}

/// The manifest record for one schema element (spec §25's `explain [fq.path]`): a message's
/// `sort` line and its fields, one field's line, an enum's line and its values, or one enum
/// value's line — rendered through the very writers [`records`] uses, so a one-element
/// explanation is a byte-for-byte slice of the full manifest, never a divergent second
/// rendering. A pure, deterministic function of the element (P3).
#[must_use]
pub fn element_record(element: &Element) -> String {
    let mut out = String::new();
    match element {
        Element::Sort(sort) => sort_lines(&mut out, sort),
        Element::Field(field) => field_line(&mut out, field),
        Element::Enum(enumeration) => enum_lines(&mut out, enumeration),
        Element::Value(value) => value_line(&mut out, value),
    }
    out
}

/// One message's manifest record (spec §13.4): a `sort <predicate>/1` line naming the
/// message's identity (`proto`) and emitted predicate, a `(recursive)` mark when the sort
/// participates in a containment cycle (§8), and any carried qualifier/escape decision
/// (`decision_note`) — followed by one `field_line` per field, in the field-number order the
/// mapping already carries.
fn sort_lines(out: &mut String, sort: &SortMapping) {
    let recursive = if sort.is_recursive() {
        "  (recursive)"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "{}  sort  {}/1{}{}",
        sort.proto().as_str(),
        sort.predicate().as_str(),
        recursive,
        decision_note(sort.qualifier(), sort.escaped()),
    );
    for field in sort.fields() {
        field_line(out, field);
    }
}

/// One field's manifest record (spec §13.4, §4.1, §7):
/// `<path> #<number> <kind>  <name>/<arity>[ -> <target>]  <declared>  <descriptor>`. `kind`
/// and `target` are two independent axes, each a pure function of one dimension of the field —
/// never conflated, so a field's oneof-arm-ness and its value's message-ness compose freely
/// instead of one silently overriding the other:
/// - `kind` is a function of the field's `EmitForm` alone: `fn` (singular), `fam` (repeated
///   or map), `oneof` (an oneof arm, regardless of what its value is — a message-typed arm is
///   still `oneof`, never demoted to `fn`), `rel` (a `(keryx.set)` membership relation,
///   Appendix B's shape; `EmitForm::Set` is reserved and never produced at present, so this arm is
///   presently unreachable but correctly labeled).
/// - `target` is a function of the field's `ValueMapping` alone: `-> <target>` names the
///   referent sort only for a message-typed occupant (an enum referent shows only in
///   `<declared>` — §13.4's occupant-vs-declared distinction).
///
/// `<name>/<arity>` for a message field is its occupant access-path term (§4.1) — the
/// functional constructor, arity one less than its relational view — with ` ; view <name>/<v>`
/// noting the view predicate (`views.lp`, §13.2) a model author joins on. `<declared>` is the
/// proto-declared type regardless of `kind`/target
/// (`declared`). `<descriptor>` is the family's shape — `seq` (sequence), `map<key>` (map), or
/// `set` (a `(keryx.set)` membership relation, reserved at present) — or, for a singular field or
/// oneof arm, its `Totality` (§5), not the finer presence (the gen-stage fidelity the `Mapping`
/// carries; `LEGACY_REQUIRED`'s distinct outbound obligation is a shape concern, Increment 4).
/// A map's `<key>` is the *declared* key type (§13.4): the codec lowers a key under its §6
/// default treatment (an `int64` key travels as a decimal string, §7.2), but the manifest records
/// the declaration, so `map<int64>` names the declared key, never the emitted term's shape.
fn field_line(out: &mut String, field: &FieldMapping) {
    let kind = match field.form() {
        EmitForm::Function => "fn",
        EmitForm::Sequence | EmitForm::Map { .. } => "fam",
        EmitForm::OneofArm { .. } => "oneof",
        EmitForm::Set => "rel",
    };
    let target = match field.value() {
        ValueMapping::Message(name) => format!(" -> {}", name.as_str()),
        ValueMapping::Scalar { .. } | ValueMapping::Enum(_) => String::new(),
    };
    // The trailing descriptor: a family names its shape — a sequence's contiguous 0-based index,
    // or a map's typed key, the KR distinction §4.1 draws (and which two message families would
    // otherwise be indistinguishable by); a singular field or oneof arm names its presence (§5).
    let descriptor = match field.form() {
        EmitForm::Sequence => "seq".to_owned(),
        EmitForm::Map { key, .. } => format!("map<{}>", Scalar::from(*key).as_str()),
        EmitForm::Set => "set".to_owned(),
        EmitForm::Function | EmitForm::OneofArm { .. } => {
            totality_word(field.presence()).to_owned()
        }
    };
    // A message field is named by its occupant access-path term — the functional constructor,
    // arity one less than the relational view it carries (`FieldMapping::arity` is the view
    // arity) — with the view predicate noted (`; view <name>/<v>`), the additive join surface in
    // `views.lp`. A base-fact field (scalar/enum) is named directly, with no view.
    let (name_arity, view) = if field.view().is_some() {
        (
            format!("{}/{}", field.predicate().as_str(), field.arity() - 1),
            format!(" ; view {}/{}", field.predicate().as_str(), field.arity()),
        )
    } else {
        (
            format!("{}/{}", field.predicate().as_str(), field.arity()),
            String::new(),
        )
    };
    let _ = writeln!(
        out,
        "{} #{} {}  {}{}  {}  {}{}{}",
        field.proto().as_str(),
        field.number(),
        kind,
        name_arity,
        target,
        declared(field.value()),
        descriptor,
        view,
        decision_note(&[], field.escaped()),
    );
}

/// One enum's manifest record (spec §13.4, §7.4): an `enum <predicate>/1 (open|closed)` line
/// naming the resolved `enum_type` feature, then a `#<number>  value  <constant>` line per
/// value in number order, each with its own carried escape decision (`decision_note`).
fn enum_lines(out: &mut String, e: &EnumMapping) {
    let openness = if matches!(e.openness(), Openness::Open) {
        "open"
    } else {
        "closed"
    };
    let _ = writeln!(
        out,
        "{}  enum  {}/1  ({openness}){}",
        e.proto().as_str(),
        e.predicate().as_str(),
        decision_note(e.qualifier(), e.escaped()),
    );
    for value in e.values() {
        value_line(out, value);
    }
}

/// One enum value's manifest record (spec §13.4): `<proto_name>  #<number>  value  <constant>`,
/// with its own carried escape decision (`decision_note`). Split from [`enum_lines`] so a single
/// value renders through the same writer for `explain <enum>.<VALUE>` (spec §25).
fn value_line(out: &mut String, value: &EnumValueMapping) {
    let _ = writeln!(
        out,
        "{}  #{}  value  {}{}",
        value.proto_name(),
        value.number(),
        value.constant().as_str(),
        decision_note(&[], value.escaped()),
    );
}

/// The proto-declared type of a field's value (spec §13.4's `<declared>` column), regardless
/// of the record's `kind`/target columns: a scalar's proto type name (its `Scalar` kind), or
/// a message/enum's referent sort predicate.
fn declared(value: &ValueMapping) -> String {
    match value {
        ValueMapping::Scalar { kind, .. } => kind.as_str().to_owned(),
        ValueMapping::Message(name) | ValueMapping::Enum(name) => name.as_str().to_owned(),
    }
}

/// The manifest's totality word (spec §13.4, §5): `total` for `Totality::Total`, `partial`
/// for `Totality::Partial` — the fidelity the `Mapping` carries; `LEGACY_REQUIRED`'s distinct
/// outbound totality obligation is a shape concern (Increment 4), not recorded here.
fn totality_word(totality: Totality) -> &'static str {
    match totality {
        Totality::Total => "total",
        Totality::Partial => "partial",
    }
}

/// The manifest note for a name's carried qualifier/escape decisions (spec §13.4): `
/// [qualified <segments>]` when the base name collided and qualification prefixed one or more
/// path segments (each already `lower_snake`d, joined by `__` — the emitted join), and `
/// [escaped]` when the base was reserved-word escaped. The decisions are read as *data* from
/// the mapping — recorded where they were made (`policy`), never re-derived from the emitted
/// symbol — so a change to the lowering or the join cannot silently falsify the note. Empty
/// (no note) when the name is bare.
fn decision_note(qualifier: &[String], escaped: bool) -> String {
    let mut note = String::new();
    if !qualifier.is_empty() {
        let _ = write!(note, " [qualified {}]", qualifier.join("__"));
    }
    if escaped {
        note.push_str(" [escaped]");
    }
    note
}

#[cfg(test)]
mod tests {
    use themelios_program::Name;

    use super::{decision_note, declared, field_line, totality_word, write};
    use crate::descriptor::model::{FqName, Openness, Package, Scalar};
    use crate::policy::model::{
        EmitForm, EnumMapping, EnumValueMapping, FieldMapping, ScalarTreatment, SortMapping,
        Totality, Unit, ValueMapping,
    };

    fn name(text: &str) -> Name {
        Name::new(text).expect("test name is a valid identifier")
    }

    #[test]
    fn decision_note_is_empty_on_a_bare_name() {
        assert_eq!(decision_note(&[], false), "");
    }

    #[test]
    fn decision_note_reports_escape_only() {
        assert_eq!(decision_note(&[], true), " [escaped]");
    }

    #[test]
    fn decision_note_reports_qualifier_only() {
        assert_eq!(
            decision_note(&["dispatch".to_owned()], false),
            " [qualified dispatch]"
        );
    }

    #[test]
    fn decision_note_composes_both_and_joins_segments() {
        // Order is qualified, then escaped (spec §13.4); a multi-segment qualifier joins with __.
        assert_eq!(
            decision_note(&["acme".to_owned(), "dispatch".to_owned()], true),
            " [qualified acme__dispatch] [escaped]"
        );
    }

    #[test]
    fn declared_names_the_scalar_kind() {
        let value = ValueMapping::Scalar {
            kind: Scalar::Int32,
            treatment: ScalarTreatment::Native,
        };
        assert_eq!(declared(&value), "int32");
    }

    #[test]
    fn declared_names_the_message_and_enum_referent() {
        assert_eq!(declared(&ValueMapping::Message(name("detail"))), "detail");
        assert_eq!(declared(&ValueMapping::Enum(name("level"))), "level");
    }

    #[test]
    fn totality_word_matches_total_and_partial() {
        assert_eq!(totality_word(Totality::Total), "total");
        assert_eq!(totality_word(Totality::Partial), "partial");
    }

    #[test]
    fn write_includes_the_normalized_header() {
        let unit = Unit {
            package: Package::parse("keryx.t").expect("valid package"),
            sorts: vec![],
            enums: vec![],
        };

        let text = write(&unit, "sha256:PLACEHOLDER");
        assert!(text.starts_with("keryx-manifest v0\n"));
        assert!(text.contains(
            "schema-hash sha256:PLACEHOLDER  package keryx.t  target clingo  profile -  shape -  keryx "
        ));
    }

    #[test]
    fn write_renders_a_sort_and_its_field() {
        let sort = SortMapping {
            proto: FqName::new("keryx.t.Reading"),
            predicate: name("reading"),
            qualifier: Vec::new(),
            escaped: false,
            recursive: false,
            doc: None,
            fields: vec![FieldMapping {
                proto: FqName::new("keryx.t.Reading.sensor"),
                number: 1,
                predicate: name("sensor"),
                arity: 2,
                form: EmitForm::Function,
                value: ValueMapping::Scalar {
                    kind: Scalar::String,
                    treatment: ScalarTreatment::Text,
                },
                presence: Totality::Total,
                escaped: false,
                doc: None,
            }],
        };
        let unit = Unit {
            package: Package::parse("keryx.t").expect("valid package"),
            sorts: vec![sort],
            enums: vec![],
        };

        let text = write(&unit, "sha256:PLACEHOLDER");
        assert!(text.contains("keryx.t.Reading  sort  reading/1\n"));
        assert!(text.contains("keryx.t.Reading.sensor #1 fn  sensor/2  string  total\n"));
    }

    #[test]
    fn write_renders_an_enum_and_its_value() {
        let enumeration = EnumMapping {
            proto: FqName::new("keryx.t.Level"),
            predicate: name("level"),
            qualifier: Vec::new(),
            escaped: false,
            openness: Openness::Open,
            doc: None,
            values: vec![EnumValueMapping {
                proto_name: "LEVEL_LOW".to_owned(),
                number: 1,
                constant: name("low"),
                escaped: false,
                doc: None,
            }],
        };
        let unit = Unit {
            package: Package::parse("keryx.t").expect("valid package"),
            sorts: vec![],
            enums: vec![enumeration],
        };

        let text = write(&unit, "sha256:PLACEHOLDER");
        assert!(text.contains("keryx.t.Level  enum  level/1  (open)\n"));
        assert!(text.contains("LEVEL_LOW  #1  value  low\n"));
    }

    #[test]
    fn write_keeps_oneof_kind_for_a_message_arm() {
        // Regression: `kind` (from `EmitForm`) and `target` (from `ValueMapping`) are
        // independent axes. A message-typed oneof arm must render `oneof`, never demoted to
        // `fn` by grouping with the plain-`Function` case just because its value is a
        // message (the bug this test would have caught: `field_line`'s old single tuple
        // match conflated the two dimensions).
        let sort = SortMapping {
            proto: FqName::new("keryx.t.Choice"),
            predicate: name("choice"),
            qualifier: Vec::new(),
            escaped: false,
            recursive: false,
            doc: None,
            fields: vec![FieldMapping {
                proto: FqName::new("keryx.t.Choice.arm"),
                number: 1,
                predicate: name("arm"),
                arity: 2,
                form: EmitForm::OneofArm {
                    oneof: "x".to_owned(),
                },
                value: ValueMapping::Message(name("y")),
                presence: Totality::Partial,
                escaped: false,
                doc: None,
            }],
        };
        let unit = Unit {
            package: Package::parse("keryx.t").expect("valid package"),
            sorts: vec![sort],
            enums: vec![],
        };

        let text = write(&unit, "sha256:PLACEHOLDER");
        // A message-typed arm is also a message field: named by its occupant term `arm/1`, with
        // its view `arm/2` noted — and the `kind` stays `oneof`, the property under test.
        assert!(
            text.contains("keryx.t.Choice.arm #1 oneof  arm/1 -> y  y  partial ; view arm/2\n")
        );
    }

    #[test]
    fn field_line_labels_a_set_form_as_rel() {
        // `EmitForm::Set` is reserved for `(keryx.set)` membership relations (Appendix B) and
        // never produced by `policy` at present, but `kind`'s match is exhaustive over every
        // `EmitForm` variant with no wildcard, so this label is already correct today.
        let field = FieldMapping {
            proto: FqName::new("keryx.t.Tags.name"),
            number: 1,
            predicate: name("name"),
            arity: 2,
            form: EmitForm::Set,
            value: ValueMapping::Scalar {
                kind: Scalar::String,
                treatment: ScalarTreatment::Text,
            },
            presence: Totality::Total,
            escaped: false,
            doc: None,
        };
        let mut out = String::new();
        field_line(&mut out, &field);
        assert!(out.contains("keryx.t.Tags.name #1 rel  name/2  string  set\n"));
    }

    #[test]
    fn write_marks_a_recursive_sort_and_a_closed_enum() {
        let sort = SortMapping {
            proto: FqName::new("keryx.t.Tree"),
            predicate: name("tree"),
            qualifier: Vec::new(),
            escaped: false,
            recursive: true,
            doc: None,
            fields: vec![],
        };
        let enumeration = EnumMapping {
            proto: FqName::new("keryx.t.Grade"),
            predicate: name("grade"),
            qualifier: Vec::new(),
            escaped: false,
            openness: Openness::Closed,
            doc: None,
            values: vec![],
        };
        let unit = Unit {
            package: Package::parse("keryx.t").expect("valid package"),
            sorts: vec![sort],
            enums: vec![enumeration],
        };

        let text = write(&unit, "sha256:PLACEHOLDER");
        assert!(text.contains("keryx.t.Tree  sort  tree/1  (recursive)\n"));
        assert!(text.contains("keryx.t.Grade  enum  grade/1  (closed)\n"));
    }

    #[test]
    fn write_notes_carried_qualifier_and_escape() {
        // The qualifier/escape decision is read as data from the mapping, so both notes render
        // from what `policy` recorded — not re-derived from the emitted symbol (§13.4).
        let sort = SortMapping {
            proto: FqName::new("keryx.t.dispatch.Reach"),
            predicate: name("dispatch__reach_"),
            qualifier: vec!["dispatch".to_owned()],
            escaped: true,
            recursive: false,
            doc: None,
            fields: vec![],
        };
        let unit = Unit {
            package: Package::parse("keryx.t").expect("valid package"),
            sorts: vec![sort],
            enums: vec![],
        };
        let text = write(&unit, "sha256:PLACEHOLDER");
        assert!(text.contains(
            "keryx.t.dispatch.Reach  sort  dispatch__reach_/1 [qualified dispatch] [escaped]\n"
        ));
    }
}
