//! The manifest — the number↔name binding and evolution contract (spec §13.4, Appendix B;
//! architecture §3). A line-oriented text: a header (schema hash, keryx version, target,
//! profile, shape), then per package one `sort` line per message/enum and one record per
//! field/value, binding the fully-qualified proto path and field number to the emitted
//! name/arity/shape and recording qualifier/escape divergence. A pure, deterministic
//! function of the [`Mapping`] (P3); *write* only at M1 — read/diff (`keryx diff`) is
//! Increment 5. The final grammar is open (spec §32 item 7); this is the v0 form.
//!
//! [`Mapping`]: crate::policy::model::Mapping

use std::fmt::Write as _;

use crate::descriptor::model::Openness;
use crate::policy::model::{
    EmitForm, EnumMapping, FieldMapping, SortMapping, Totality, Unit, ValueMapping,
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
        unit.package(),
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

/// One message's manifest record (spec §13.4): a `sort <predicate>/1` line naming the
/// message's identity (`proto`) and emitted predicate, a `(recursive)` mark when the sort
/// participates in a containment cycle (§8), and any qualifier/escape divergence
/// (`escape_note`) between the proto leaf and the emitted predicate — followed by one
/// `field_line` per field, in the field-number order the mapping already carries.
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
        escape_note(final_leaf(sort.proto().as_str()), sort.predicate().as_str()),
    );
    for field in sort.fields() {
        field_line(out, field);
    }
}

/// One field's manifest record (spec §13.4, §4.1, §7):
/// `<path> #<number> <kind>  <name>/<arity>[ -> <target>]  <declared>  <total|partial>[  ;
/// view <name>/<arity>]`. `kind` and `target` are two independent axes, each a pure function
/// of one dimension of the field — never conflated, so a field's oneof-arm-ness and its
/// value's message-ness compose freely instead of one silently overriding the other:
/// - `kind` is a function of the field's `EmitForm` alone: `fn` (singular), `fam` (repeated
///   or map), `oneof` (an oneof arm, regardless of what its value is — a message-typed arm is
///   still `oneof`, never demoted to `fn`), `rel` (a `(keryx.set)` membership relation,
///   Appendix B's shape; `EmitForm::Set` is reserved and never produced at M1, so this arm is
///   presently unreachable but correctly labeled).
/// - `target` is a function of the field's `ValueMapping` alone: `-> <target>` names the
///   referent sort only for a message-typed occupant (an enum referent shows only in
///   `<declared>` — §13.4's occupant-vs-declared distinction).
///
/// `<declared>` is the proto-declared type regardless of `kind`/target (`declared`); the
/// totality word reflects `Totality` (§5), not the finer presence — the M1 fidelity the
/// `Mapping` carries (`LEGACY_REQUIRED`'s distinct outbound obligation is a shape concern,
/// Increment 4). A trailing `; view` note names the relational view the field gets, when one
/// exists (§13.2).
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
    let view = if field.view().is_some() {
        format!("  ; view {}/{}", field.predicate().as_str(), field.arity())
    } else {
        String::new()
    };
    let _ = writeln!(
        out,
        "{} #{} {}  {}/{}{}  {}  {}{}",
        field.proto().as_str(),
        field.number(),
        kind,
        field.predicate().as_str(),
        field.arity(),
        target,
        declared(field.value()),
        totality_word(field.presence()),
        view,
    );
}

/// One enum's manifest record (spec §13.4, §7.4): an `enum <predicate>/1 (open|closed)` line
/// naming the resolved `enum_type` feature, then a `#<number>  value  <constant>` line per
/// value in number order, each with its own qualifier/escape divergence (`escape_note`)
/// between the value's proto name and its lowered constant.
fn enum_lines(out: &mut String, e: &EnumMapping) {
    let openness = if matches!(e.openness(), Openness::Open) {
        "open"
    } else {
        "closed"
    };
    let _ = writeln!(
        out,
        "{}  enum  {}/1  ({openness})",
        e.proto().as_str(),
        e.predicate().as_str(),
    );
    for value in e.values() {
        let _ = writeln!(
            out,
            "{}  #{}  value  {}{}",
            value.proto_name(),
            value.number(),
            value.constant().as_str(),
            escape_note(value.proto_name(), value.constant().as_str()),
        );
    }
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

/// The final dotted segment of a fully-qualified proto path (`a.b.C` → `C`): the un-lowered
/// leaf `escape_note` compares a sort's emitted predicate against.
fn final_leaf(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

/// The qualifier/reserved-word-escape reconstruction (spec §13.4): recovers "qualified from
/// …" and/or "escaped" from the divergence between `base_leaf` (the un-lowered proto leaf —
/// a message/enum's final path segment, or an enum value's proto name) and `emitted` (the
/// symbol the `Mapping` actually carries), against `base = lower_snake(base_leaf)`. The two
/// decisions are independent and compositional — a name can be both qualified and escaped,
/// one, or neither — and the reconstruction is an **exact inverse**, not a lossy heuristic:
/// `lower_snake` collapses runs of `_` to one and trims a leading/trailing `_`, so `base`
/// itself never contains `__` and never starts or ends with `_`, while qualification's only
/// join is `__` and escape's only mark is one trailing `_`. So a trailing `_` beyond `base`
/// is unambiguously the escape, and a `__` immediately before `base` is unambiguously a
/// qualifier boundary: the decomposition `emitted = [qualifier "__"]? + base + ["_"]?` is
/// therefore unique.
///
/// 1. Strip a trailing `_` from `emitted` to get `unescaped` (unchanged if there is none).
///    The name was escaped iff that strip actually removed a character
///    (`emitted == "{unescaped}_"`) and `unescaped` ends in `base`.
/// 2. On that same `unescaped`, the name was qualified iff it is longer than `base` and ends
///    with `__{base}` (the part before that is the qualifier the collision resolved to).
fn escape_note(base_leaf: &str, emitted: &str) -> String {
    let base = lower_snake(base_leaf);
    let unescaped = emitted.strip_suffix('_').unwrap_or(emitted);
    let escaped = emitted == format!("{unescaped}_") && unescaped.ends_with(base.as_str());
    let qualified =
        unescaped.len() > base.len() && unescaped.ends_with(format!("__{base}").as_str());
    let mut note = String::new();
    if qualified {
        let _ = write!(note, " [qualified from {base}]");
    }
    if escaped {
        note.push_str(" [escaped]");
    }
    note
}

/// `UpperCamel`/`SCREAMING_SNAKE` → `lower_snake`, mirroring `policy::names::lower_snake`
/// exactly (that module is private to `policy`, and the manifest's consumed surface is the
/// `Mapping` plus `descriptor::model` alone, so it is duplicated here rather than reached
/// into): insert `_` at a lower/digit→upper case boundary, collapse runs of `_` to one, trim
/// a leading/trailing `_`, lowercase. `escape_note`'s exact-inverse property rests on exactly
/// this collapsing/trimming invariant.
fn lower_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut prev_lower_or_digit = false;
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            if prev_lower_or_digit {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_lower_or_digit = false;
        } else if ch == '_' {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_lower_or_digit = false;
        } else {
            out.push(ch);
            prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    out.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use themelios_program::Name;

    use super::{declared, escape_note, field_line, final_leaf, lower_snake, totality_word, write};
    use crate::descriptor::model::{FqName, Openness, Scalar};
    use crate::policy::model::{
        EmitForm, EnumMapping, EnumValueMapping, FieldMapping, ScalarTreatment, SortMapping,
        Totality, Unit, ValueMapping,
    };

    fn name(text: &str) -> Name {
        Name::new(text).expect("test name is a valid identifier")
    }

    #[test]
    fn escape_note_reports_neither_on_a_bare_name() {
        assert_eq!(escape_note("Reading", "reading"), "");
    }

    #[test]
    fn escape_note_reports_escape_only() {
        assert_eq!(escape_note("Not", "not_"), " [escaped]");
    }

    #[test]
    fn escape_note_reports_qualification_only() {
        assert_eq!(
            escape_note("Status", "dispatch__status"),
            " [qualified from status]"
        );
    }

    #[test]
    fn escape_note_composes_both_independently() {
        // A reserved-named `Not` message qualified under an `a` collision: both decisions
        // fire, and each is recorded (spec §13.4 — order is qualified, then escaped).
        assert_eq!(
            escape_note("Not", "a__not_"),
            " [qualified from not] [escaped]"
        );
    }

    #[test]
    fn escape_note_ignores_a_length_decrease() {
        // A legitimate §7.4 prefix-strip shortens the name (`LEVEL_LOW` -> `low`); it must
        // never register as an escape or a qualifier, both of which only ever lengthen it.
        assert_eq!(escape_note("LEVEL_LOW", "low"), "");
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
    fn final_leaf_takes_the_last_dotted_segment() {
        assert_eq!(final_leaf("keryx.p3.Reading"), "Reading");
        assert_eq!(final_leaf("Bare"), "Bare");
    }

    #[test]
    fn lower_snake_collapses_and_trims_underscores() {
        assert_eq!(lower_snake("Reading"), "reading");
        assert_eq!(lower_snake("HttpStatus"), "http_status");
        assert_eq!(lower_snake("LEVEL_LOW"), "level_low");
        assert_eq!(lower_snake("__weird___name__"), "weird_name");
    }

    #[test]
    fn write_includes_the_normalized_header() {
        let unit = Unit {
            package: "keryx.t".to_owned(),
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
            package: "keryx.t".to_owned(),
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
            package: "keryx.t".to_owned(),
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
            package: "keryx.t".to_owned(),
            sorts: vec![sort],
            enums: vec![],
        };

        let text = write(&unit, "sha256:PLACEHOLDER");
        assert!(
            text.contains("keryx.t.Choice.arm #1 oneof  arm/2 -> y  y  partial  ; view arm/2\n")
        );
    }

    #[test]
    fn field_line_labels_a_set_form_as_rel() {
        // `EmitForm::Set` is reserved for `(keryx.set)` membership relations (Appendix B) and
        // never produced by `policy` at M1, but `kind`'s match is exhaustive over every
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
        assert!(out.contains("keryx.t.Tags.name #1 rel  name/2  string  total\n"));
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
            package: "keryx.t".to_owned(),
            sorts: vec![sort],
            enums: vec![enumeration],
        };

        let text = write(&unit, "sha256:PLACEHOLDER");
        assert!(text.contains("keryx.t.Tree  sort  tree/1  (recursive)\n"));
        assert!(text.contains("keryx.t.Grade  enum  grade/1  (closed)\n"));
    }
}
