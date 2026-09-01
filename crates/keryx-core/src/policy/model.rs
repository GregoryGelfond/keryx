//! The mapping model (architecture §3, §5; spec §21.3): keryx's engine-independent
//! record of the stage-1 decisions — the *second* stable interface, consumed by `emit`
//! (§21.4), `manifest` (§13.4), and `explain` (§21.3). For every schema element it
//! records the emitted ASP symbol and the shape it takes: the sort predicate of a
//! message, the predicate/arity/form of a field, the lowered constant of an enum value,
//! and the qualifier/escape decisions materialized into those symbols. A pure function
//! of the [`Schema`] (P3), built only at the policy door (`pub(crate)` constructors),
//! read through accessors, deterministically ordered (units by package, sorts/enums by
//! path, fields by number, values by number). Emitted names are themelios [`Name`]s —
//! validated once here, so `emit` cannot spell an illegal identifier (R1/R2; the
//! direct themelios binding, as the codec's `Sym = themelios::Symbol`).
//!
//! [`Schema`]: crate::descriptor::model::Schema
//! [`Name`]: themelios_program::Name

use themelios_program::Name;

use crate::descriptor::model::{FqName, MapKey, Openness, Scalar};

/// A field's emitted form (spec §4.1, §7): the ASP shape its predicate takes. Closed —
/// the treatment classification (`ValueMapping`) rides beside it, never inside it. At
/// M1 every `repeated` is a `Sequence`; the `Set` form lands when `(keryx.set)` gains
/// meaning (Increment 5). `OneofArm` is an ordinary partial function that also records
/// its oneof (spec §7.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmitForm {
    /// A singular field — a unary function on its parent sort.
    Function,
    /// A repeated field — an index-keyed family (spec §7.1).
    Sequence,
    /// A repeated field under `(keryx.set)` — a membership relation (spec §7.1);
    /// reserved for Increment 5, never produced at M1.
    Set,
    /// A map field — a key-keyed family (spec §7.2).
    Map {
        /// The key kind.
        key: MapKey,
    },
    /// A oneof arm — a partial function recording its oneof's name (spec §7.3).
    OneofArm {
        /// The declaring oneof's proto name.
        oneof: String,
    },
}

/// A field value's emitted treatment (spec §6, §4.1). A scalar's *default* §6
/// classification, or a reference to the referent's emitted sort predicate. The scalar
/// classification is recorded for the codec/shape (Increments 3–4); nothing in M1 emit
/// reads it (the signature shows the proto type; the views concern message fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueMapping {
    /// A scalar — its proto `kind` (the §13.1 signature shows the proto type) and its §6
    /// default `treatment` (consumed by the codec/shape at Increments 3–4; not read by M1
    /// emit).
    Scalar {
        /// The proto scalar kind (for the signature).
        kind: Scalar,
        /// The §6 default treatment classification.
        treatment: ScalarTreatment,
    },
    /// A message occupant — carries the referent message's sort predicate.
    Message(Name),
    /// An enum value — carries the referent enum's sort predicate.
    Enum(Name),
}

/// The §6 default treatment of a scalar (the classification, **not enforced** at M1 —
/// range checks, the float error, and annotation overrides land at Increment 5). The
/// families follow §6: the machine-int family is `Native` (uint32/fixed32 carry a
/// downstream range obligation); the 64-bit family is `DecimalString`; float/double have
/// no default (`NeedsAnnotation` — the error is Increment 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarTreatment {
    /// `int32`, `sint32`, `sfixed32`, `uint32`, `fixed32` — native clingo integer
    /// (uint32/fixed32 range-checked downstream).
    Native,
    /// `int64`, `uint64`, `fixed64`, `sfixed64`, `sint64` — decimal-string constant.
    DecimalString,
    /// `float`, `double` — no default; an annotation is required (the translation error
    /// is Increment 5).
    NeedsAnnotation,
    /// `bool` — the constants `true`/`false`.
    Bool,
    /// `string` — a clingo string constant.
    Text,
    /// `bytes` — a lowercase-hex string constant.
    HexString,
}

/// A field's resolved totality (spec §5): the presence classification stage 1 makes.
/// `Total` for IMPLICIT (the atom always exists); `Partial` for EXPLICIT and
/// `LEGACY_REQUIRED` (`LEGACY_REQUIRED` additionally carries an outbound totality
/// obligation, applied by the shape module at Increment 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Totality {
    /// IMPLICIT presence — total on its sort.
    Total,
    /// EXPLICIT or `LEGACY_REQUIRED` presence — partial.
    Partial,
}

/// Which relational view (if any) `emit::views` generates for a field (spec §13.2).
/// Only message-typed fields with an access-path occupant get one; scalar fields are
/// already relational, and set membership (Increment 5) needs none. The variant selects
/// the rule form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    /// `f(P, A) :- t(A), A = f(P).` — a singular message field.
    Singular,
    /// `f(P, I, E) :- t(E), E = f(P, I).` — a sequence of messages.
    Sequence,
    /// `f(P, K, E) :- t(E), E = f(P, K).` — a map with message values.
    Map,
}

/// A message type's mapping — its sort predicate and the mapping of each field. `proto`
/// is the identity (spec §13.4); `predicate` is the emitted sort `s/1` (qualifier/escape
/// decisions already materialized in). `recursive` carries the §8 containment-cycle mark
/// through for `explain` and the manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortMapping {
    pub(crate) proto: FqName,
    pub(crate) predicate: Name,
    pub(crate) recursive: bool,
    pub(crate) doc: Option<String>,
    pub(crate) fields: Vec<FieldMapping>,
}

impl SortMapping {
    /// The message's fully-qualified proto path — its identity (§13.4).
    #[must_use]
    pub fn proto(&self) -> &FqName {
        &self.proto
    }

    /// The emitted sort predicate `s/1` (qualifier/escape decisions materialized in).
    #[must_use]
    pub fn predicate(&self) -> &Name {
        &self.predicate
    }

    /// Whether this sort participates in a containment cycle (§8).
    #[must_use]
    pub fn is_recursive(&self) -> bool {
        self.recursive
    }

    /// The doc comment, if the descriptor carried one.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }

    /// The mapping of each field, in field-number order.
    #[must_use]
    pub fn fields(&self) -> &[FieldMapping] {
        &self.fields
    }
}

/// A field's mapping (spec §4.1, §21.3): the emitted `predicate` at `arity`, its `form`
/// and value treatment, its resolved totality, and the relational `view` (if any) `emit`
/// generates. `proto` and `number` are the identity (spec §13.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldMapping {
    pub(crate) proto: FqName,
    pub(crate) number: i32,
    pub(crate) predicate: Name,
    pub(crate) arity: u32,
    pub(crate) form: EmitForm,
    pub(crate) value: ValueMapping,
    pub(crate) presence: Totality,
    pub(crate) view: Option<ViewKind>,
    pub(crate) doc: Option<String>,
}

impl FieldMapping {
    /// The field's fully-qualified proto path — its identity with `number` (§13.4).
    #[must_use]
    pub fn proto(&self) -> &FqName {
        &self.proto
    }

    /// The field number — the field's identity within its parent (§13.4).
    #[must_use]
    pub fn number(&self) -> i32 {
        self.number
    }

    /// The emitted predicate symbol (qualifier/escape decisions materialized in).
    #[must_use]
    pub fn predicate(&self) -> &Name {
        &self.predicate
    }

    /// The emitted arity (2 or 3 at M1).
    #[must_use]
    pub fn arity(&self) -> u32 {
        self.arity
    }

    /// The emitted form.
    #[must_use]
    pub fn form(&self) -> &EmitForm {
        &self.form
    }

    /// The value treatment.
    #[must_use]
    pub fn value(&self) -> &ValueMapping {
        &self.value
    }

    /// The resolved totality.
    #[must_use]
    pub fn presence(&self) -> Totality {
        self.presence
    }

    /// The relational view `emit::views` generates, if any.
    #[must_use]
    pub fn view(&self) -> Option<ViewKind> {
        self.view
    }

    /// The doc comment, if the descriptor carried one.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }
}

/// An enum type's mapping (spec §7.4): its sort predicate, resolved openness, and the
/// lowered constant of each value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumMapping {
    pub(crate) proto: FqName,
    pub(crate) predicate: Name,
    pub(crate) openness: Openness,
    pub(crate) doc: Option<String>,
    pub(crate) values: Vec<EnumValueMapping>,
}

impl EnumMapping {
    /// The enum's fully-qualified proto path — its identity (§13.4).
    #[must_use]
    pub fn proto(&self) -> &FqName {
        &self.proto
    }

    /// The emitted sort predicate `s/1`.
    #[must_use]
    pub fn predicate(&self) -> &Name {
        &self.predicate
    }

    /// The resolved `enum_type` feature (§7.4).
    #[must_use]
    pub fn openness(&self) -> Openness {
        self.openness
    }

    /// The doc comment, if the descriptor carried one.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }

    /// The lowered mapping of each value, in number order.
    #[must_use]
    pub fn values(&self) -> &[EnumValueMapping] {
        &self.values
    }
}

/// An enum value's mapping (spec §7.4): the lowered `constant` (prefix stripped, escapes
/// materialized), keyed to its `proto_name` and `number` for the manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumValueMapping {
    pub(crate) proto_name: String,
    pub(crate) number: i32,
    pub(crate) constant: Name,
    pub(crate) doc: Option<String>,
}

impl EnumValueMapping {
    /// The value's short proto name — its identity with `number` (§13.4).
    #[must_use]
    pub fn proto_name(&self) -> &str {
        &self.proto_name
    }

    /// The value's number (§7.4).
    #[must_use]
    pub fn number(&self) -> i32 {
        self.number
    }

    /// The lowered constant (prefix stripped, escapes materialized).
    #[must_use]
    pub fn constant(&self) -> &Name {
        &self.constant
    }

    /// The doc comment, if the descriptor carried one.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }
}

/// One generation unit — a proto package (spec §13: four files per package). `sorts` in
/// fq-path order, `enums` in fq-path order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub(crate) package: String,
    pub(crate) sorts: Vec<SortMapping>,
    pub(crate) enums: Vec<EnumMapping>,
}

impl Unit {
    /// The proto package this unit generates for.
    #[must_use]
    pub fn package(&self) -> &str {
        &self.package
    }

    /// The unit's sorts, in fully-qualified-path order.
    #[must_use]
    pub fn sorts(&self) -> &[SortMapping] {
        &self.sorts
    }

    /// The unit's enums, in fully-qualified-path order.
    #[must_use]
    pub fn enums(&self) -> &[EnumMapping] {
        &self.enums
    }
}

/// The mapping model root (spec §3, §21.3): the generation units of one schema, in
/// deterministic package order (P3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub(crate) units: Vec<Unit>,
}

impl Mapping {
    /// The generation units, in deterministic package order (P3).
    #[must_use]
    pub fn units(&self) -> &[Unit] {
        &self.units
    }
}

#[cfg(test)]
mod tests {
    use themelios_program::Name;

    use super::{
        EmitForm, EnumMapping, EnumValueMapping, FieldMapping, Mapping, ScalarTreatment,
        SortMapping, Totality, Unit, ValueMapping, ViewKind,
    };
    use crate::descriptor::model::{FqName, MapKey, Openness, Scalar};

    fn name(text: &str) -> Name {
        Name::new(text).expect("test name is a valid identifier")
    }

    fn sample_field() -> FieldMapping {
        FieldMapping {
            proto: FqName::new("keryx.test.Sample.tags"),
            number: 3,
            predicate: name("tags"),
            arity: 2,
            form: EmitForm::Sequence,
            value: ValueMapping::Scalar {
                kind: Scalar::String,
                treatment: ScalarTreatment::Text,
            },
            presence: Totality::Total,
            view: Some(ViewKind::Singular),
            doc: Some("the tags field".to_owned()),
        }
    }

    #[test]
    fn field_mapping_accessors_round_trip_its_fields() {
        let field = sample_field();
        assert_eq!(field.proto().as_str(), "keryx.test.Sample.tags");
        assert_eq!(field.number(), 3);
        assert_eq!(field.predicate().as_str(), "tags");
        assert_eq!(field.arity(), 2);
        assert_eq!(field.form(), &EmitForm::Sequence);
        assert_eq!(
            field.value(),
            &ValueMapping::Scalar {
                kind: Scalar::String,
                treatment: ScalarTreatment::Text,
            }
        );
        assert_eq!(field.presence(), Totality::Total);
        assert_eq!(field.view(), Some(ViewKind::Singular));
        assert_eq!(field.doc(), Some("the tags field"));
    }

    #[test]
    fn map_emit_form_holds_its_key() {
        let form = EmitForm::Map { key: MapKey::Int64 };
        match form {
            EmitForm::Map { key } => assert_eq!(key, MapKey::Int64),
            EmitForm::Function | EmitForm::Sequence | EmitForm::Set | EmitForm::OneofArm { .. } => {
                panic!("expected a Map form")
            }
        }
    }

    #[test]
    fn scalar_treatment_is_copy() {
        let treatment = ScalarTreatment::Native;
        let copied = treatment;
        assert_eq!(treatment, copied);
    }

    #[test]
    fn unit_and_mapping_accessors_return_the_built_slices() {
        let sort = SortMapping {
            proto: FqName::new("keryx.test.Sample"),
            predicate: name("sample"),
            recursive: false,
            doc: Some("the sample sort".to_owned()),
            fields: vec![sample_field()],
        };
        let enumeration = EnumMapping {
            proto: FqName::new("keryx.test.Kind"),
            predicate: name("kind"),
            openness: Openness::Open,
            doc: Some("the sample enum".to_owned()),
            values: vec![EnumValueMapping {
                proto_name: "ACTIVE".to_owned(),
                number: 1,
                constant: name("active"),
                doc: Some("the active value".to_owned()),
            }],
        };
        let unit = Unit {
            package: "keryx.test".to_owned(),
            sorts: vec![sort],
            enums: vec![enumeration],
        };
        let mapping = Mapping { units: vec![unit] };

        assert_eq!(mapping.units().len(), 1);
        let unit = &mapping.units()[0];
        assert_eq!(unit.package(), "keryx.test");
        assert_eq!(unit.sorts().len(), 1);
        assert_eq!(unit.enums().len(), 1);

        let sort = &unit.sorts()[0];
        assert_eq!(sort.proto().as_str(), "keryx.test.Sample");
        assert_eq!(sort.predicate().as_str(), "sample");
        assert!(!sort.is_recursive());
        assert_eq!(sort.doc(), Some("the sample sort"));
        assert_eq!(sort.fields().len(), 1);

        let enumeration = &unit.enums()[0];
        assert_eq!(enumeration.proto().as_str(), "keryx.test.Kind");
        assert_eq!(enumeration.predicate().as_str(), "kind");
        assert_eq!(enumeration.openness(), Openness::Open);
        assert_eq!(enumeration.doc(), Some("the sample enum"));
        assert_eq!(enumeration.values().len(), 1);

        let value = &enumeration.values()[0];
        assert_eq!(value.proto_name(), "ACTIVE");
        assert_eq!(value.number(), 1);
        assert_eq!(value.constant().as_str(), "active");
        assert_eq!(value.doc(), Some("the active value"));
    }
}
