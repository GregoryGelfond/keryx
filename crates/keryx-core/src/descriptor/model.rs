//! The schema model (architecture §3, §5): keryx's de-sugared, engine-independent
//! view of a descriptor set — the first stable interface, consumed by `policy`
//! (Increment 2) and `codec` (Increments 3–4). Every proto sugar is normalized
//! away (maps, proto3-optional, groups; §20), presence is resolved (§5), and
//! identity is the fully-qualified proto path plus field number (§4.2, §13.4). No
//! `prost_reflect` type appears here — this is the far side of the
//! descriptor-engine boundary. A `Schema`'s element lists are `pub(crate)`, so a
//! foreign leaf cannot enter an assembled schema — the model is built only at the
//! ingest door; deterministically ordered (messages by path, fields by number,
//! enums by path, values by number) so the whole model is a pure, golden-comparable
//! function of the input (P3).

/// A fully-qualified proto name — a dotted path (`dispatch.v1.Shipment.tags`), the
/// machine-checked identity of a vocabulary element (§4.2, §13.4). A validated
/// newtype in themelios's `Name` idiom; the validation is descriptor provenance,
/// so construction is crate-internal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FqName(String);

impl FqName {
    pub(crate) fn new(path: impl Into<String>) -> FqName {
        FqName(path.into())
    }

    /// The dotted path text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// keryx's bound on a package's dot-separated segment count. A package contributes its segments to
/// the qualification prefix of every type it holds (§4.2), so an unbounded package depth would let a
/// crafted schema drive `policy::qualify` past linear work — the qualifier prefix length, and thus
/// the resolver's round count, is bounded by the nesting cap ([`super::RECURSION_LIMIT`]) *and* this.
/// No real package approaches it; one deeper is refused at the door (`MalformedDescriptor`).
pub(crate) const MAX_PACKAGE_SEGMENTS: usize = 64;

/// A validated protobuf package — empty, or a dot-separated sequence of proto identifiers
/// (`[A-Za-z_][A-Za-z0-9_]*`, e.g. `dispatch.v1`), within `MAX_PACKAGE_SEGMENTS`. Adversary-chosen at
/// the descriptor door yet consumed by two sinks that each assume an identifier shape — the emitted
/// `#include` operand (`emit::views`) and the CLI's per-package output path — so the shape is
/// *represented* here: a `Package` is constructed only at the door (`parse`, through
/// `descriptor::pre_validate`, which refuses every other shape before a `Schema` is built), so no
/// sink re-derives trust from a bare `String`. The leading-`.` package the engine panics on is one
/// refused shape among these.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Package(String);

impl Package {
    /// Parse a package string into a `Package`, or the reason it is not one (§4.2): empty is a
    /// package-less file (valid here; `policy` refuses it separately with a fix-it), otherwise every
    /// dot-separated segment must be a proto identifier and the count must be within
    /// [`MAX_PACKAGE_SEGMENTS`]. The one construction point — the door — so the type is a proof of
    /// shape.
    pub(crate) fn parse(text: &str) -> Result<Package, PackageProblem> {
        if text.is_empty() {
            return Ok(Package(String::new()));
        }
        let segments: Vec<&str> = text.split('.').collect();
        if segments.len() > MAX_PACKAGE_SEGMENTS {
            return Err(PackageProblem::TooDeep(segments.len()));
        }
        if let Some(segment) = segments.iter().find(|s| !is_proto_ident(s)) {
            return Err(PackageProblem::Segment((*segment).to_owned()));
        }
        Ok(Package(text.to_owned()))
    }

    /// The package's dotted text — empty when the file declares none.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the file declares no package.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Why a package string is not a valid [`Package`] — carried to the door's `MalformedDescriptor`
/// detail (`descriptor::pre_validate`); the model composes no diagnostics itself.
#[derive(Debug)]
pub(crate) enum PackageProblem {
    /// A dot-separated segment is not a proto identifier — an empty segment (a leading, trailing, or
    /// doubled `.`, the leading-`.` the engine panics on among them), or one carrying a character an
    /// identifier cannot (`/`, `"`, whitespace), the shapes that reach the path and `#include` sinks.
    Segment(String),
    /// More dot-separated segments than [`MAX_PACKAGE_SEGMENTS`].
    TooDeep(usize),
}

impl PackageProblem {
    /// The diagnostic detail naming the refusal.
    pub(crate) fn detail(&self) -> String {
        match self {
            PackageProblem::Segment(segment) => {
                format!("package name segment {segment:?} is not a proto identifier")
            }
            PackageProblem::TooDeep(count) => format!(
                "package has {count} dot-separated segments, beyond keryx's limit of {MAX_PACKAGE_SEGMENTS}"
            ),
        }
    }
}

/// Whether `s` is a proto identifier `[A-Za-z_][A-Za-z0-9_]*` (§4.2) — the shape a package segment,
/// and a declared message/enum/field/value name, must have (the reference protobuf grammar's
/// `ident`). Read at the door by `descriptor::pre_validate`.
pub(crate) fn is_proto_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// The syntax era a file declares (spec §5, §20). `#[non_exhaustive]` so a future
/// edition is added as a variant, not a redesign — an `Edition(u32)` variant lands
/// with the editions increment and its first consumer. Today only proto2/proto3 reach
/// here: an editions set is refused up front (`UnsupportedEdition`), since the
/// descriptor engine has no editions `Syntax`. Produced by `desugar::version`, consumed
/// by openness resolution; read only to *resolve* features, never to branch
/// translation, which keys on resolved features alone (§5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SchemaVersion {
    /// `syntax = "proto2"`.
    Proto2,
    /// `syntax = "proto3"`.
    Proto3,
}

/// A field's shape (§4.1, §5, §7.1): the proto-structural form, and — for a
/// singular field, the only place it is meaningful — its resolved presence.
/// Set-ness is *not* here; `(keryx.set)` is an annotation on the field, given
/// meaning at Increment 2 (§7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldShape {
    /// A singular field — a (total or partial) function on its parent sort.
    Singular {
        /// The value type.
        value: ValueType,
        /// The resolved presence.
        presence: Presence,
    },
    /// A repeated field — an index-keyed family (a sequence, §7.1).
    Repeated {
        /// The element value type.
        value: ValueType,
    },
    /// A map field — a key-keyed family (§7.2), de-sugared from the synthetic
    /// `*Entry` message.
    Map {
        /// The key kind.
        key: MapKey,
        /// The value type.
        value: ValueType,
    },
}

/// The type of a field's value (§4.1, §6): a scalar, a message occupant, or an
/// enum; message and enum references carry the referent's fully-qualified name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueType {
    /// A scalar value.
    Scalar(Scalar),
    /// A message-typed occupant.
    Message(FqName),
    /// An enum value.
    Enum(FqName),
}

/// A protobuf scalar kind (§6). The clingo term shape each maps to is policy's
/// concern (Increment 2); here the kind is recorded faithfully.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scalar {
    /// `int32`.
    Int32,
    /// `int64`.
    Int64,
    /// `uint32`.
    Uint32,
    /// `uint64`.
    Uint64,
    /// `sint32`.
    Sint32,
    /// `sint64`.
    Sint64,
    /// `fixed32`.
    Fixed32,
    /// `fixed64`.
    Fixed64,
    /// `sfixed32`.
    Sfixed32,
    /// `sfixed64`.
    Sfixed64,
    /// `bool`.
    Bool,
    /// `float`.
    Float,
    /// `double`.
    Double,
    /// `string`.
    String,
    /// `bytes`.
    Bytes,
}

impl Scalar {
    /// The proto type name (`int32`, `sfixed64`, …) — the one home of the scalar-name
    /// table, read by the emitted signature (spec §13.1) and any later consumer.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Scalar::Int32 => "int32",
            Scalar::Int64 => "int64",
            Scalar::Uint32 => "uint32",
            Scalar::Uint64 => "uint64",
            Scalar::Sint32 => "sint32",
            Scalar::Sint64 => "sint64",
            Scalar::Fixed32 => "fixed32",
            Scalar::Fixed64 => "fixed64",
            Scalar::Sfixed32 => "sfixed32",
            Scalar::Sfixed64 => "sfixed64",
            Scalar::Bool => "bool",
            Scalar::Float => "float",
            Scalar::Double => "double",
            Scalar::String => "string",
            Scalar::Bytes => "bytes",
        }
    }
}

/// A map key kind (§7.2): the proto-restricted subset — integral, bool, or
/// string; never float, double, bytes, message, or enum. Illegal keys are
/// unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapKey {
    /// `int32`.
    Int32,
    /// `int64`.
    Int64,
    /// `uint32`.
    Uint32,
    /// `uint64`.
    Uint64,
    /// `sint32`.
    Sint32,
    /// `sint64`.
    Sint64,
    /// `fixed32`.
    Fixed32,
    /// `fixed64`.
    Fixed64,
    /// `sfixed32`.
    Sfixed32,
    /// `sfixed64`.
    Sfixed64,
    /// `bool`.
    Bool,
    /// `string`.
    String,
}

/// A map key is a [`Scalar`] restricted to the legal key subset (§7.2). The subset
/// relation lives here, so the scalar-kind name table has one home ([`Scalar::as_str`])
/// and a scalar that is also a legal key does not fan out to a second table.
impl From<MapKey> for Scalar {
    fn from(key: MapKey) -> Scalar {
        match key {
            MapKey::Int32 => Scalar::Int32,
            MapKey::Int64 => Scalar::Int64,
            MapKey::Uint32 => Scalar::Uint32,
            MapKey::Uint64 => Scalar::Uint64,
            MapKey::Sint32 => Scalar::Sint32,
            MapKey::Sint64 => Scalar::Sint64,
            MapKey::Fixed32 => Scalar::Fixed32,
            MapKey::Fixed64 => Scalar::Fixed64,
            MapKey::Sfixed32 => Scalar::Sfixed32,
            MapKey::Sfixed64 => Scalar::Sfixed64,
            MapKey::Bool => Scalar::Bool,
            MapKey::String => Scalar::String,
        }
    }
}

/// Resolved field presence (spec §5): read from the resolved `field_presence`
/// feature, never the syntax era. Repeated and map fields resolve to `Implicit`
/// (collection presence).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// The field always has a value; the atom is always emitted (§5).
    Implicit,
    /// The field has a value only when set; the function is partial (§5).
    Explicit,
    /// `LEGACY_REQUIRED`; treated as `Explicit` for translation, with an
    /// outbound totality obligation (§5).
    LegacyRequired,
}

/// Whether unknown numeric values are legal on the wire for an enum (§7.4): the
/// resolved `enum_type` feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Openness {
    /// Unknown integers are legal on the wire (proto3 and the editions default).
    Open,
    /// Only declared values are legal (proto2).
    Closed,
}

/// An annotation's value, lowered from the applied option's protobuf value (§15).
/// The scalar-policy meanings (§6) are applied at Increment 2; here it is faithful
/// data. `Enum` carries the applied enum value's name (e.g. `CLINGCON`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnotationValue {
    /// A `bool` option.
    Bool(bool),
    /// An integral option, widened to `i64`.
    Int(i64),
    /// A `string` option.
    Text(String),
    /// An enum option, carrying the value's name.
    Enum(String),
}

/// A source file: its name and package (Appendix C `file/2`). The file's version is
/// resolved transiently for feature resolution (openness) and is not stored — a `version`
/// field is added when a consumer (e.g. `explain` showing the era) needs it. Plain data with
/// public fields, reached through [`Schema::files`]; a `Schema`'s file list is `pub(crate)`, so
/// a foreign `File` cannot enter an assembled schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    /// The file's name (Appendix C `file/2`).
    pub name: String,
    /// The file's [`Package`] — validated at the door; empty when the file declares none.
    pub package: Package,
}

/// A message type — a sort (§4.1). Identity is `path`; `outer` is its lexical
/// nesting parent (Appendix C `nested/2`), `None` at file top level. `recursive`
/// marks participation in a containment cycle (§8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub(crate) path: FqName,
    pub(crate) file: String,
    pub(crate) outer: Option<FqName>,
    pub(crate) fields: Vec<Field>,
    pub(crate) oneofs: Vec<Oneof>,
    pub(crate) options: Vec<Annotation>,
    pub(crate) doc: Option<String>,
    pub(crate) recursive: bool,
}

impl Message {
    /// The message's fully-qualified name — its identity (§4.1).
    #[must_use]
    pub fn path(&self) -> &FqName {
        &self.path
    }

    /// The name of the file that declares this message.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Its lexical nesting parent (Appendix C `nested/2`); `None` at file top
    /// level.
    #[must_use]
    pub fn outer(&self) -> Option<&FqName> {
        self.outer.as_ref()
    }

    /// The message's fields, in declaration order.
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The message's oneofs, in declaration order.
    #[must_use]
    pub fn oneofs(&self) -> &[Oneof] {
        &self.oneofs
    }

    /// The applied keryx annotations, in declaration order.
    #[must_use]
    pub fn options(&self) -> &[Annotation] {
        &self.options
    }

    /// The doc comment, if the descriptor carried source info for it.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }

    /// Whether this message participates in a containment cycle (§8).
    #[must_use]
    pub fn is_recursive(&self) -> bool {
        self.recursive
    }
}

/// A field — a function on its parent sort (§4.1). Identity is `number` within the
/// parent; `path` is its own fully-qualified name, the key of its `opt`/`doc` facts
/// and of any diagnostic that names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub(crate) number: i32,
    pub(crate) name: String,
    pub(crate) path: FqName,
    pub(crate) shape: FieldShape,
    pub(crate) options: Vec<Annotation>,
    pub(crate) doc: Option<String>,
}

impl Field {
    /// The field number — the field's identity within its parent (§4.2).
    #[must_use]
    pub fn number(&self) -> i32 {
        self.number
    }

    /// The short field name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The field's fully-qualified name.
    #[must_use]
    pub fn path(&self) -> &FqName {
        &self.path
    }

    /// The resolved shape and presence.
    #[must_use]
    pub fn shape(&self) -> &FieldShape {
        &self.shape
    }

    /// The applied keryx annotations, in declaration order.
    #[must_use]
    pub fn options(&self) -> &[Annotation] {
        &self.options
    }

    /// The doc comment, if the descriptor carried source info for it.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }
}

/// A oneof (§7.3) — the *real* ones only; the synthetic oneof proto3 `optional`
/// generates is de-sugared away (§20). Records which arm field numbers belong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Oneof {
    /// The oneof's short name.
    pub name: String,
    /// The oneof's fully-qualified name.
    pub path: FqName,
    /// The field numbers of the oneof's arms.
    pub arms: Vec<i32>,
    /// The doc comment, if the descriptor carried source info for it.
    pub doc: Option<String>,
}

/// An enum type — a closed sort of symbolic constants (§7.4). `openness` is the
/// resolved `enum_type` feature (see the resolution rule in `desugar`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enum {
    pub(crate) path: FqName,
    pub(crate) file: String,
    pub(crate) outer: Option<FqName>,
    pub(crate) openness: Openness,
    pub(crate) values: Vec<EnumValue>,
    pub(crate) options: Vec<Annotation>,
    pub(crate) doc: Option<String>,
}

impl Enum {
    /// The enum's fully-qualified name — its identity (§4.1).
    #[must_use]
    pub fn path(&self) -> &FqName {
        &self.path
    }

    /// The name of the file that declares this enum.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Its lexical nesting parent; `None` at file top level.
    #[must_use]
    pub fn outer(&self) -> Option<&FqName> {
        self.outer.as_ref()
    }

    /// The resolved `enum_type` feature (§7.4).
    #[must_use]
    pub fn openness(&self) -> Openness {
        self.openness
    }

    /// The enum's values, in declaration order.
    #[must_use]
    pub fn values(&self) -> &[EnumValue] {
        &self.values
    }

    /// The applied keryx annotations, in declaration order.
    #[must_use]
    pub fn options(&self) -> &[Annotation] {
        &self.options
    }

    /// The doc comment, if the descriptor carried source info for it.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }
}

/// An enum value: its name and number (Appendix C `enum_value/3`); `path` keys its
/// `opt`/`doc` facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumValue {
    /// The value's short name.
    pub name: String,
    /// The value's number (Appendix C `enum_value/3`).
    pub number: i32,
    /// The value's fully-qualified name.
    pub path: FqName,
    /// The applied keryx annotations, in declaration order.
    pub options: Vec<Annotation>,
    /// The doc comment, if the descriptor carried source info for it.
    pub doc: Option<String>,
}

/// A keryx annotation — one applied custom option (§15), keyed by its option name
/// with `keryx.` stripped (`set`, `numeric`, …). A repeated option expands to one
/// `Annotation` per element. Inline-sourced at ingestion; overlay provenance (§16) is
/// added with overlays at Increment 5.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    /// The option name with `keryx.` stripped (e.g. `set`, `numeric`).
    pub key: String,
    /// The applied value.
    pub value: AnnotationValue,
}

/// The schema model root (§3, §5): the de-sugared files, messages, and enums of
/// one descriptor set, each list in deterministic order (P3) — files by name,
/// messages and enums by fully-qualified path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub(crate) files: Vec<File>,
    pub(crate) messages: Vec<Message>,
    pub(crate) enums: Vec<Enum>,
}

impl Schema {
    /// The files of the descriptor set, ordered by name.
    #[must_use]
    pub fn files(&self) -> &[File] {
        &self.files
    }

    /// The messages of the descriptor set, ordered by fully-qualified path.
    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// The enums of the descriptor set, ordered by fully-qualified path.
    #[must_use]
    pub fn enums(&self) -> &[Enum] {
        &self.enums
    }
}

#[cfg(test)]
mod tests {
    use super::{FieldShape, FqName, MapKey, Presence, Scalar, ValueType};

    #[test]
    fn fq_name_round_trips_its_text() {
        assert_eq!(FqName::new("x").as_str(), "x");
    }

    #[test]
    fn singular_shape_carries_its_presence() {
        let shape = FieldShape::Singular {
            value: ValueType::Scalar(Scalar::Bool),
            presence: Presence::Explicit,
        };
        match shape {
            FieldShape::Singular { presence, .. } => assert_eq!(presence, Presence::Explicit),
            FieldShape::Repeated { .. } | FieldShape::Map { .. } => {
                panic!("expected a singular shape")
            }
        }
    }

    #[test]
    fn map_key_widens_to_its_scalar() {
        assert_eq!(Scalar::from(MapKey::Int64), Scalar::Int64);
    }
}
