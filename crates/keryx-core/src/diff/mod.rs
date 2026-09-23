//! The evolution instrument's core — `keryx diff` (spec §13.4, §27; architecture §5): the old and
//! the new `.proto`, each compiled through the shipped descriptor doors and mapped by
//! [`policy::map`], are compared as two [`Mapping`]s by protobuf identity — a sort or enum by its
//! path, a field or value by its number; names are free to change, and a rename that silently
//! renames a generated predicate is what the instrument catches. No manifest is read: the
//! [`manifest`] is the write-only per-version record, so the comparison opens no input door of
//! its own.
//!
//! The matching key: two versions of one schema are two packages under buf's convention
//! (`thermal.v1`, `thermal.v2`), so a unit is matched across versions by its package with the
//! trailing version segment stripped — `normalize_package` — and a sort or enum within it by
//! its path relative to that package (`relative_name`), so the version never enters an identity.
//!
//! The comparison is a model, not a change list: [`compare`] builds a [`Comparison`] — a matched
//! tree of packages, then sorts and enums, then fields and values, each node carrying its
//! per-side references with the unchanged nodes present — and every node classifies itself
//! ([`ChangeKind`]). The flat rows of [`Comparison::changes`] serve the JSON changeset
//! ([`Comparison::to_json`]) and an exit code; the tree serves a report that shows what stayed
//! beside what changed.
//!
//! Only *subject* vocabulary is compared ([`SortMapping::is_subject`]): the referent closure a
//! subject pulls in — a well-known type, an imported dependency — is neither the schema's own
//! nor a change the schema made, so it is skipped, and two schemas sharing nothing but such an
//! import are not comparable.
//!
//! A pure rename — a field's, sort's, or enum's predicate re-spelled with its identity and shape
//! intact — carries a *bridge view* ([`FieldDiff::bridge`], [`SortDiff::bridge`],
//! [`EnumDiff::bridge`]; spec §13.4): the one rule `old(…) :- new(…).` through which a model
//! written against the old vocabulary *reads* the new facts during a cutover. It is constructed
//! through `emit`'s one themelios construction site and rendered by `emit`'s renderer — syntax
//! values crossing the emission boundary every generated module crosses (§18), never a format
//! string (`bridge_rule`). It is inbound-facing only: a model that *emits* old atoms is not
//! shimmed, and a message-field bridge aliases the field's relational view, not the occupant-term
//! functor a model might spell directly — limits the rows state rather than paper over.
//!
//! [`policy::map`]: crate::policy::map
//! [`Mapping`]: crate::policy::model::Mapping
//! [`manifest`]: crate::manifest
//! [`SortMapping::is_subject`]: crate::policy::model::SortMapping::is_subject

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use themelios_program::Name;

use crate::descriptor::model::{FqName, Package};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::emit;
use crate::manifest;
use crate::policy::model::{
    EmitForm, EnumMapping, EnumValueMapping, FieldMapping, Mapping, SortMapping, Totality, Unit,
    ValueMapping,
};

/// Compare the old and the new mapping — each from [`policy::map`] — into a [`Comparison`] of
/// their subject vocabularies. A pure, deterministic function of the two mappings (P3), borrowing
/// both: the caller maps both schemas, compares, and reads the comparison in one scope.
///
/// Matching is by protobuf identity (spec §13.4): a unit by its version-normalized package, a
/// sort or enum by its package-relative path, a field or value by its number — so `thermal.v1`
/// against `thermal.v2` matches field for field, and a field whose number changed is a removal
/// beside an addition. Classification is each node's own ([`FieldDiff::kind`] and its siblings).
/// Linear in the subject elements beyond the identity-keyed maps' `O(n log n)`; no search.
///
/// # Errors
///
/// `AmbiguousVersionPackages` — one per collision, the side named — when a side declares two
/// subject packages that normalize to one; `NoComparableSchemas` when the two sides' subject
/// packages are disjoint once normalized (a side with none included). Every cause is collected
/// before returning (§6). Both are a caller's arguments error, never a translation's.
///
/// [`policy::map`]: crate::policy::map
pub fn compare<'a>(old: &'a Mapping, new: &'a Mapping) -> Result<Comparison<'a>, Diagnostics> {
    let (old_units, mut faults) = subject_units(old, Side::Old);
    let (new_units, new_faults) = subject_units(new, Side::New);
    faults.extend(new_faults);
    if old_units.keys().all(|key| !new_units.contains_key(key)) {
        faults.push(no_comparable_schemas(&old_units, &new_units));
    }
    if let Some(diagnostics) = Diagnostics::collect(faults) {
        return Err(diagnostics);
    }
    let referents = Referents::of(&old_units, &new_units);
    let packages = merge(old_units, new_units)
        .into_iter()
        .map(|(key, units)| PackageDiff::build(key, units, &referents))
        .collect();
    Ok(Comparison { packages })
}

/// The comparison of two mappings' subject vocabularies ([`compare`]): the matched packages in
/// normalized-package order, each a tree down to its fields and values with the unchanged nodes
/// present. Borrows both mappings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison<'a> {
    packages: Vec<PackageDiff<'a>>,
}

impl<'a> Comparison<'a> {
    /// The matched packages, in normalized-package order.
    #[must_use]
    pub fn packages(&self) -> &[PackageDiff<'a>] {
        &self.packages
    }

    /// Whether any change is of a breaking kind ([`ChangeKind::is_breaking`]).
    #[must_use]
    pub fn is_breaking(&self) -> bool {
        self.changes().iter().any(Change::is_breaking)
    }

    /// The tree flattened to its changed nodes, the unchanged skipped: one [`Change`] per node of
    /// a kind other than `Unchanged`, sorted by the normalized package, then the sort's or enum's
    /// relative path (empty for a package's own row), then the field or value number (an
    /// element's own row first), then the kind's declaration order — a function of the two
    /// mappings alone (P3). A one-sided package or element rides with its children as rows of
    /// their own.
    #[must_use]
    pub fn changes(&self) -> Vec<Change<'a>> {
        let mut rows = Vec::new();
        for package in &self.packages {
            package.rows(&mut rows);
        }
        rows.sort_by_key(|(key, _)| *key);
        rows.into_iter().map(|(_, change)| change).collect()
    }

    /// The JSON changeset — the `--json` product (spec §13.4, §27): the rows of
    /// [`changes`](Comparison::changes) as one flat array, one record per changed node in that
    /// order, so the text is a function of the two mappings alone (P3). A record carries
    /// `package` (the normalized package), `kind` (the change's slug — the [`ChangeKind`]
    /// variant's name in `snake_case`: `renamed`, `removed`, `added`, `changed`, `sort_renamed`,
    /// `enum_renamed`, `message_added`, `message_removed`, `enum_added`, `enum_removed`,
    /// `value_added`, `value_removed`, `value_renamed`, `openness_changed`, `preserve_changed`,
    /// `package_added`, `package_removed`), `old_path` and `new_path` (the element's
    /// fully-qualified proto path per side, `null` on a side the element is not on), `old` and
    /// `new` (the per-side rendered signatures, [`old_signature`](Change::old_signature) and its
    /// twin — `null` on a missing side and on a package row, which has none), and `breaking`
    /// (the kind's classification); and, only where the row has one, `number` (a field's or
    /// value's — absent on a package, sort, or enum row) and `bridge` (the rendered bridge view
    /// of a pure rename, its newlines escaped). Compact — one line, no trailing newline — with a
    /// record's keys in `serde_json`'s sorted order.
    ///
    /// Built over `serde_json::Value` and serialized by `serde_json` — already the payload
    /// door's JSON serializer — which escapes the newlines a bridge string carries.
    #[must_use]
    pub fn to_json(&self) -> String {
        let records = self.changes().iter().map(Change::record).collect();
        serde_json::to_string(&Value::Array(records))
            .expect("a changeset is strings, integers, booleans, and nulls, which serialize")
    }
}

/// The order [`Comparison::changes`] sorts its rows by: normalized package, the sort's or enum's
/// relative path, the number, the kind.
type RowKey<'a> = (&'a str, &'a str, Option<i32>, ChangeKind);

/// A row of [`Comparison::changes`] with its sort key.
type Row<'a> = (RowKey<'a>, Change<'a>);

/// One package matched across the sides by its normalized key — `thermal` for `thermal.v1`
/// against `thermal.v2` — with its subject sorts and enums matched within, in relative-path
/// order. Present on one side only, it is a `PackageAdded`/`PackageRemoved` whose elements are
/// every one one-sided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageDiff<'a> {
    key: &'a str,
    sides: Pair<&'a Package>,
    sorts: Vec<SortDiff<'a>>,
    enums: Vec<EnumDiff<'a>>,
}

impl<'a> PackageDiff<'a> {
    fn build(
        key: &'a str,
        units: Pair<SubjectUnit<'a>>,
        referents: &Referents<'a>,
    ) -> PackageDiff<'a> {
        let (sides, sorts, enums) = match units {
            Pair::Old(unit) => (
                Pair::Old(unit.package),
                merge(unit.sorts, BTreeMap::new()),
                merge(unit.enums, BTreeMap::new()),
            ),
            Pair::New(unit) => (
                Pair::New(unit.package),
                merge(BTreeMap::new(), unit.sorts),
                merge(BTreeMap::new(), unit.enums),
            ),
            Pair::Both(old, new) => (
                Pair::Both(old.package, new.package),
                merge(old.sorts, new.sorts),
                merge(old.enums, new.enums),
            ),
        };
        PackageDiff {
            key,
            sides,
            sorts: sorts
                .into_iter()
                .map(|(name, pair)| SortDiff::build(name, pair, referents))
                .collect(),
            enums: enums
                .into_iter()
                .map(|(name, pair)| EnumDiff::build(name, pair))
                .collect(),
        }
    }

    /// The normalized package the two sides matched on (`thermal`).
    #[must_use]
    pub fn key(&self) -> &'a str {
        self.key
    }

    /// The old side's package as declared (`thermal.v1`), when the package is on that side.
    #[must_use]
    pub fn old_package(&self) -> Option<&'a Package> {
        self.sides.old()
    }

    /// The new side's package as declared (`thermal.v2`), when the package is on that side.
    #[must_use]
    pub fn new_package(&self) -> Option<&'a Package> {
        self.sides.new_side()
    }

    /// The matched sorts, in relative-path order.
    #[must_use]
    pub fn sorts(&self) -> &[SortDiff<'a>] {
        &self.sorts
    }

    /// The matched enums, in relative-path order.
    #[must_use]
    pub fn enums(&self) -> &[EnumDiff<'a>] {
        &self.enums
    }

    /// The package's own kind: `PackageRemoved` or `PackageAdded` when one-sided, else
    /// `Unchanged` — a version bump is the match the instrument expects, not a change; the
    /// elements carry their own kinds.
    #[must_use]
    pub fn kind(&self) -> ChangeKind {
        match self.sides {
            Pair::Old(_) => ChangeKind::PackageRemoved,
            Pair::New(_) => ChangeKind::PackageAdded,
            Pair::Both(..) => ChangeKind::Unchanged,
        }
    }

    fn rows(&self, rows: &mut Vec<Row<'a>>) {
        let kind = self.kind();
        if kind != ChangeKind::Unchanged {
            rows.push(Change::row(
                kind,
                self.key,
                "",
                None,
                self.old_package().map(render_package),
                self.new_package().map(render_package),
                None,
            ));
        }
        for sort in &self.sorts {
            sort.rows(self.key, rows);
        }
        for enumeration in &self.enums {
            enumeration.rows(self.key, rows);
        }
    }
}

/// One message sort matched across the sides by its package-relative path (`Reading`,
/// `Outer.Inner`), with its fields matched by number, in number order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortDiff<'a> {
    name: &'a str,
    sides: Pair<&'a SortMapping>,
    fields: Vec<FieldDiff<'a>>,
}

impl<'a> SortDiff<'a> {
    fn build(
        name: &'a str,
        sides: Pair<&'a SortMapping>,
        referents: &Referents<'a>,
    ) -> SortDiff<'a> {
        let (old_fields, old_cells) = index_fields(sides.old());
        let (new_fields, new_cells) = index_fields(sides.new_side());
        let pairs = merge(old_fields, new_fields);
        // The membership dimension reads a cell among the fields both sides hold: an arm added
        // or removed is its own row, not a change of every partner.
        let matched: BTreeSet<i32> = pairs
            .iter()
            .filter(|(_, pair)| matches!(pair, Pair::Both(..)))
            .map(|(number, _)| *number)
            .collect();
        let fields = pairs
            .into_iter()
            .map(|(number, pair)| FieldDiff {
                number,
                old_cell: cell_of(&old_cells, pair.old(), &matched),
                new_cell: cell_of(&new_cells, pair.new_side(), &matched),
                same_value: match pair {
                    Pair::Both(old, new) => referents.same_value(old.value(), new.value()),
                    Pair::Old(_) | Pair::New(_) => true,
                },
                sides: pair,
            })
            .collect();
        SortDiff {
            name,
            sides,
            fields,
        }
    }

    /// The sort's path relative to its package — the identity the two sides matched on.
    #[must_use]
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The old side's sort, when the sort is on that side.
    #[must_use]
    pub fn old_sort(&self) -> Option<&'a SortMapping> {
        self.sides.old()
    }

    /// The new side's sort, when the sort is on that side.
    #[must_use]
    pub fn new_sort(&self) -> Option<&'a SortMapping> {
        self.sides.new_side()
    }

    /// The matched fields, in number order.
    #[must_use]
    pub fn fields(&self) -> &[FieldDiff<'a>] {
        &self.fields
    }

    /// The sort's own kind: `MessageRemoved` or `MessageAdded` when one-sided; `SortRenamed`
    /// when both sides hold the path but the predicate differs (a new collision qualified it,
    /// §4.2); else `Unchanged` — the fields carry their own kinds.
    #[must_use]
    pub fn kind(&self) -> ChangeKind {
        match self.sides {
            Pair::Old(_) => ChangeKind::MessageRemoved,
            Pair::New(_) => ChangeKind::MessageAdded,
            Pair::Both(old, new) if old.predicate() == new.predicate() => ChangeKind::Unchanged,
            Pair::Both(..) => ChangeKind::SortRenamed,
        }
    }

    /// The bridge view for a `SortRenamed` sort (spec §13.4) — the one rule `old(A) :- new(A).`
    /// aliasing the old sort predicate to the new at arity 1, through which a model written
    /// against the old vocabulary *reads* the new sort's occupants — or `None` for any other
    /// kind. Inbound-facing only: a model that *emits* atoms of the old sort is not shimmed.
    /// Constructed and rendered through `emit` (`bridge_rule`), never string-templated.
    #[must_use]
    pub fn bridge(&self) -> Option<String> {
        match self.sides {
            Pair::Both(old, new) if self.kind() == ChangeKind::SortRenamed => {
                Some(bridge_rule(Bridge {
                    old: old.predicate(),
                    new: new.predicate(),
                    arity: 1,
                    from: old.proto(),
                    to: new.proto(),
                    view: false,
                }))
            }
            _ => None,
        }
    }

    fn rows(&self, package: &'a str, rows: &mut Vec<Row<'a>>) {
        let kind = self.kind();
        if kind != ChangeKind::Unchanged {
            rows.push(Change::row(
                kind,
                package,
                self.name,
                None,
                self.old_sort().map(render_sort),
                self.new_sort().map(render_sort),
                self.bridge(),
            ));
        }
        for field in &self.fields {
            let kind = field.kind();
            if kind != ChangeKind::Unchanged {
                rows.push(Change::row(
                    kind,
                    package,
                    self.name,
                    Some(field.number),
                    field.old_field().map(render_field),
                    field.new_field().map(render_field),
                    field.bridge(),
                ));
            }
        }
    }
}

/// One enum matched across the sides by its package-relative path, with its values matched by
/// number, in number order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumDiff<'a> {
    name: &'a str,
    sides: Pair<&'a EnumMapping>,
    values: Vec<ValueDiff<'a>>,
}

impl<'a> EnumDiff<'a> {
    fn build(name: &'a str, sides: Pair<&'a EnumMapping>) -> EnumDiff<'a> {
        let index = |side: Option<&'a EnumMapping>| -> BTreeMap<i32, &'a EnumValueMapping> {
            side.map_or(&[][..], EnumMapping::values)
                .iter()
                .map(|value| (value.number(), value))
                .collect()
        };
        let values = merge(index(sides.old()), index(sides.new_side()))
            .into_iter()
            .map(|(number, pair)| ValueDiff {
                number,
                sides: pair,
            })
            .collect();
        EnumDiff {
            name,
            sides,
            values,
        }
    }

    /// The enum's path relative to its package — the identity the two sides matched on.
    #[must_use]
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The old side's enum, when the enum is on that side.
    #[must_use]
    pub fn old_enum(&self) -> Option<&'a EnumMapping> {
        self.sides.old()
    }

    /// The new side's enum, when the enum is on that side.
    #[must_use]
    pub fn new_enum(&self) -> Option<&'a EnumMapping> {
        self.sides.new_side()
    }

    /// The matched values, in number order.
    #[must_use]
    pub fn values(&self) -> &[ValueDiff<'a>] {
        &self.values
    }

    /// The enum's own kind — one per node, the dominant of its differences: `EnumRemoved` or
    /// `EnumAdded` when one-sided; then `OpennessChanged`, then `PreserveChanged` (the breaking
    /// flips, §7.4), then `EnumRenamed` (the predicate differs while the path holds); else
    /// `Unchanged`. A rename beside a flip is reported as the flip, its names legible in the
    /// row's two signatures; the values carry their own kinds.
    #[must_use]
    pub fn kind(&self) -> ChangeKind {
        match self.sides {
            Pair::Old(_) => ChangeKind::EnumRemoved,
            Pair::New(_) => ChangeKind::EnumAdded,
            Pair::Both(old, new) => {
                if old.openness() != new.openness() {
                    ChangeKind::OpennessChanged
                } else if old.preserve() != new.preserve() {
                    ChangeKind::PreserveChanged
                } else if old.predicate() == new.predicate() {
                    ChangeKind::Unchanged
                } else {
                    ChangeKind::EnumRenamed
                }
            }
        }
    }

    /// The bridge view for an `EnumRenamed` enum (spec §13.4) — the one rule `old(A) :- new(A).`
    /// aliasing the old enum predicate to the new at arity 1, through which a model written
    /// against the old vocabulary *reads* the new value sort it quantifies over (§7.4) — or
    /// `None` for any other kind: an enum renamed beside an openness or preserve flip is the flip,
    /// breaking and unbridged. Inbound-facing only, as [`SortDiff::bridge`]. Constructed and
    /// rendered through `emit` (`bridge_rule`), never string-templated.
    #[must_use]
    pub fn bridge(&self) -> Option<String> {
        match self.sides {
            Pair::Both(old, new) if self.kind() == ChangeKind::EnumRenamed => {
                Some(bridge_rule(Bridge {
                    old: old.predicate(),
                    new: new.predicate(),
                    arity: 1,
                    from: old.proto(),
                    to: new.proto(),
                    view: false,
                }))
            }
            _ => None,
        }
    }

    fn rows(&self, package: &'a str, rows: &mut Vec<Row<'a>>) {
        let kind = self.kind();
        if kind != ChangeKind::Unchanged {
            rows.push(Change::row(
                kind,
                package,
                self.name,
                None,
                self.old_enum().map(render_enum),
                self.new_enum().map(render_enum),
                self.bridge(),
            ));
        }
        for value in &self.values {
            let kind = value.kind();
            if kind != ChangeKind::Unchanged {
                let old = value
                    .old_value()
                    .zip(self.old_enum())
                    .map(|(value, enumeration)| render_value(enumeration, value));
                let new = value
                    .new_value()
                    .zip(self.new_enum())
                    .map(|(value, enumeration)| render_value(enumeration, value));
                rows.push(Change::row(
                    kind,
                    package,
                    self.name,
                    Some(value.number),
                    old,
                    new,
                    None,
                ));
            }
        }
    }
}

/// One field matched across the sides by its number, carrying the two dimensions its own two
/// sides cannot decide: for the membership dimension, its oneof cell on each side among the
/// fields both sides hold; for the value dimension, whether the two values are one value with
/// their referents compared by identity (`Referents`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDiff<'a> {
    number: i32,
    sides: Pair<&'a FieldMapping>,
    old_cell: BTreeSet<i32>,
    new_cell: BTreeSet<i32>,
    same_value: bool,
}

impl<'a> FieldDiff<'a> {
    /// The field number — the identity the two sides matched on.
    #[must_use]
    pub fn number(&self) -> i32 {
        self.number
    }

    /// The old side's field, when the field is on that side.
    #[must_use]
    pub fn old_field(&self) -> Option<&'a FieldMapping> {
        self.sides.old()
    }

    /// The new side's field, when the field is on that side.
    #[must_use]
    pub fn new_field(&self) -> Option<&'a FieldMapping> {
        self.sides.new_side()
    }

    /// The field's kind — the detection line. One-sided: `Removed` or `Added`. Both sides with
    /// the shape intact — the arity, form, value, presence, and oneof cell all equal — it is
    /// `Unchanged` when the predicate holds and `Renamed` when the predicate alone differs; any
    /// shape difference is `Changed`, renamed or not. A value's message or enum referent is
    /// compared by identity, not by predicate: a referent renamed is that sort's or enum's own
    /// row, bridged there, not a change of every field naming it (a closure referent, which no
    /// row bridges, is compared as spelled). A oneof's label is not shape: two arms are one form
    /// whatever their oneofs are named, since the name reaches only `emit.lp`'s `violates`
    /// string. Its *cell* is — among the fields both sides hold, so an arm added or removed is
    /// its own row and no partner's change — the member numbers sharing the oneof: a field moved
    /// between oneofs, or one whose partner moved, changed its membership, which changes the
    /// exclusivity partition `emit.lp` states, while a oneof merely renamed changed nothing.
    #[must_use]
    pub fn kind(&self) -> ChangeKind {
        match self.sides {
            Pair::Old(_) => ChangeKind::Removed,
            Pair::New(_) => ChangeKind::Added,
            Pair::Both(old, new) => {
                let aspect = self.differences(old, new);
                if aspect.shape_holds() {
                    if aspect.predicate.is_some() {
                        ChangeKind::Renamed
                    } else {
                        ChangeKind::Unchanged
                    }
                } else {
                    ChangeKind::Changed
                }
            }
        }
    }

    /// The dimensions a `Changed` field differs on, per side ([`ChangeAspect`]); `None` for every
    /// other kind — a pure rename's two names are on its two sides.
    #[must_use]
    pub fn aspect(&self) -> Option<ChangeAspect<'a>> {
        let Pair::Both(old, new) = self.sides else {
            return None;
        };
        let aspect = self.differences(old, new);
        (!aspect.shape_holds()).then_some(aspect)
    }

    /// The bridge view for a `Renamed` field (spec §13.4) — the one rule `old(A, …) :- new(A, …).`
    /// aliasing the old predicate to the new at the field's arity, through which a model written
    /// against the old vocabulary *reads* the new facts — or `None` for any other kind, a rename
    /// that also changed shape (`Changed`) included. The predicate aliased is the model-facing
    /// one: a scalar, enum, or set field's base predicate at its arity; a message field's
    /// relational view (§13.2) at the view arity — the projection idiom `readings(B, _, E)` that
    /// `views.lp` prescribes. Not aliased, because no rule can alias a functor: the occupant-term
    /// functor `readings(B, I)` inside the child sort atom, which the rename renames too — so a
    /// model spelling the occupant term directly is not bridged, and the bridge's `%!` line names
    /// it as the view's. Inbound-facing only: a model that *emits* old atoms is not shimmed.
    /// Constructed and rendered through `emit` (`bridge_rule`), never string-templated.
    #[must_use]
    pub fn bridge(&self) -> Option<String> {
        match self.sides {
            Pair::Both(old, new) if self.kind() == ChangeKind::Renamed => {
                Some(bridge_rule(Bridge {
                    old: old.predicate(),
                    new: new.predicate(),
                    // A `Renamed` field's arity holds across the sides, and it is the mapping's
                    // own: the view arity for a message field with a view, the base arity for a
                    // scalar, enum, or set field.
                    arity: old.arity(),
                    from: old.proto(),
                    to: new.proto(),
                    view: old.view().is_some(),
                }))
            }
            _ => None,
        }
    }

    /// Every dimension compared, each present when it differs.
    fn differences(&self, old: &'a FieldMapping, new: &'a FieldMapping) -> ChangeAspect<'a> {
        ChangeAspect {
            predicate: (old.predicate() != new.predicate())
                .then(|| (old.predicate(), new.predicate())),
            arity: (old.arity() != new.arity()).then_some((old.arity(), new.arity())),
            form: (!same_form(old.form(), new.form())).then(|| (old.form(), new.form())),
            value: (!self.same_value).then(|| (old.value(), new.value())),
            presence: (old.presence() != new.presence())
                .then_some((old.presence(), new.presence())),
            membership: (self.old_cell != self.new_cell)
                .then(|| (self.old_cell.clone(), self.new_cell.clone())),
        }
    }
}

/// The dimensions a `Changed` field differs on — each its (old, new) pair of the raw per-side
/// data when it differs and `None` when it holds — structured so a report can emphasize the
/// changed token beside the stable rest, never a re-tokenized string: the field's predicate,
/// arity, form, value, and presence, and its oneof-cell membership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeAspect<'a> {
    predicate: Option<(&'a Name, &'a Name)>,
    arity: Option<(u32, u32)>,
    form: Option<(&'a EmitForm, &'a EmitForm)>,
    value: Option<(&'a ValueMapping, &'a ValueMapping)>,
    presence: Option<(Totality, Totality)>,
    membership: Option<(BTreeSet<i32>, BTreeSet<i32>)>,
}

impl<'a> ChangeAspect<'a> {
    /// The (old, new) predicates, when the field was renamed as well as changed.
    #[must_use]
    pub fn predicate(&self) -> Option<(&'a Name, &'a Name)> {
        self.predicate
    }

    /// The (old, new) arities, when they differ.
    #[must_use]
    pub fn arity(&self) -> Option<(u32, u32)> {
        self.arity
    }

    /// The (old, new) forms, when they differ — two oneof arms never do, whatever their oneofs
    /// are named.
    #[must_use]
    pub fn form(&self) -> Option<(&'a EmitForm, &'a EmitForm)> {
        self.form
    }

    /// The (old, new) value mappings, when they differ — the declared type (a message or enum
    /// referent by its identity, never by a predicate a rename re-spelled) or a scalar's
    /// resolved treatment, either changing the term a fact carries.
    #[must_use]
    pub fn value(&self) -> Option<(&'a ValueMapping, &'a ValueMapping)> {
        self.value
    }

    /// The (old, new) totalities, when they differ.
    #[must_use]
    pub fn presence(&self) -> Option<(Totality, Totality)> {
        self.presence
    }

    /// The (old, new) oneof cells, when the membership changed — each the member numbers of the
    /// oneof the field is an arm of on that side, among the fields both sides hold; empty when
    /// it is no arm there (a one-member oneof is `{n}`, distinct from no oneof).
    #[must_use]
    pub fn membership(&self) -> Option<(&BTreeSet<i32>, &BTreeSet<i32>)> {
        self.membership.as_ref().map(|(old, new)| (old, new))
    }

    /// Whether every shape dimension holds — everything but the predicate.
    fn shape_holds(&self) -> bool {
        self.arity.is_none()
            && self.form.is_none()
            && self.value.is_none()
            && self.presence.is_none()
            && self.membership.is_none()
    }
}

/// One enum value matched across the sides by its number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueDiff<'a> {
    number: i32,
    sides: Pair<&'a EnumValueMapping>,
}

impl<'a> ValueDiff<'a> {
    /// The value's number — the identity the two sides matched on.
    #[must_use]
    pub fn number(&self) -> i32 {
        self.number
    }

    /// The old side's value, when the value is on that side.
    #[must_use]
    pub fn old_value(&self) -> Option<&'a EnumValueMapping> {
        self.sides.old()
    }

    /// The new side's value, when the value is on that side.
    #[must_use]
    pub fn new_value(&self) -> Option<&'a EnumValueMapping> {
        self.sides.new_side()
    }

    /// The value's kind: `ValueRemoved` or `ValueAdded` when one-sided; `ValueRenamed` when the
    /// lowered constant differs while the number holds; else `Unchanged` — protobuf lets the
    /// value's name change, and the constant is what a model names.
    #[must_use]
    pub fn kind(&self) -> ChangeKind {
        match self.sides {
            Pair::Old(_) => ChangeKind::ValueRemoved,
            Pair::New(_) => ChangeKind::ValueAdded,
            Pair::Both(old, new) if old.constant() == new.constant() => ChangeKind::Unchanged,
            Pair::Both(..) => ChangeKind::ValueRenamed,
        }
    }
}

/// How a node of a [`Comparison`] differs between the sides — one kind per node. The seventeen
/// kinds of change and the baseline `Unchanged`; the declaration order is the fixed order
/// [`Comparison::changes`] sorts on last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeKind {
    /// Both sides present and one in the vocabulary — the baseline outside the seventeen kinds
    /// of change; never a row of [`Comparison::changes`].
    Unchanged,
    /// A field's predicate changed with its shape intact — the same number, arity, form, value,
    /// presence, and oneof cell: a clean rename, bridgeable, not breaking.
    Renamed,
    /// A field removed — or renumbered, a number change being a removal beside an addition:
    /// breaking.
    Removed,
    /// A field added: informational.
    Added,
    /// A field whose arity, form, value, presence, or oneof-cell membership changed, renamed or
    /// not: breaking, with no bridge; the differing dimensions ride in its [`ChangeAspect`].
    Changed,
    /// A sort's predicate changed while its path held (a new collision qualified it, §4.2): a
    /// rename at arity 1, bridgeable, not breaking.
    SortRenamed,
    /// An enum's predicate changed while its path held: as `SortRenamed`.
    EnumRenamed,
    /// A message added, its fields riding as `Added` rows: informational.
    MessageAdded,
    /// A message removed, its fields riding as `Removed` rows: breaking.
    MessageRemoved,
    /// An enum added, its values riding as `ValueAdded` rows: informational.
    EnumAdded,
    /// An enum removed, its values riding as `ValueRemoved` rows: breaking.
    EnumRemoved,
    /// An enum value added: informational.
    ValueAdded,
    /// An enum value removed: breaking.
    ValueRemoved,
    /// An enum value's constant changed while its number held: breaking — a constant is
    /// vocabulary no rule can alias.
    ValueRenamed,
    /// An enum's resolved openness flipped (§7.4): breaking.
    OpennessChanged,
    /// An enum's `(keryx.unknown) = PRESERVE` flipped (§7.4): breaking — the escape term
    /// `unknown(N)` enters or leaves the vocabulary.
    PreserveChanged,
    /// A subject package on the new side only, its elements riding as additions: informational.
    PackageAdded,
    /// A subject package on the old side only, its elements riding as removals: breaking.
    PackageRemoved,
}

impl ChangeKind {
    /// Whether this kind breaks a model written against the old vocabulary: every removal, a
    /// field whose shape changed, a constant renamed, an enum whose openness or preserve flipped.
    /// A rename of a field, sort, or enum is not breaking — a bridge view aliases it — and no
    /// addition is.
    #[must_use]
    pub fn is_breaking(self) -> bool {
        match self {
            ChangeKind::Removed
            | ChangeKind::Changed
            | ChangeKind::MessageRemoved
            | ChangeKind::EnumRemoved
            | ChangeKind::ValueRemoved
            | ChangeKind::ValueRenamed
            | ChangeKind::OpennessChanged
            | ChangeKind::PreserveChanged
            | ChangeKind::PackageRemoved => true,
            ChangeKind::Unchanged
            | ChangeKind::Renamed
            | ChangeKind::Added
            | ChangeKind::SortRenamed
            | ChangeKind::EnumRenamed
            | ChangeKind::MessageAdded
            | ChangeKind::EnumAdded
            | ChangeKind::ValueAdded
            | ChangeKind::PackageAdded => false,
        }
    }

    /// The kind's slug in the JSON changeset ([`Comparison::to_json`]): the variant's name in
    /// `snake_case`, one per kind of change — or `None` for `Unchanged`, the baseline, which is
    /// no change and never a row of [`Comparison::changes`], so it names no record. That arm is
    /// its own rather than a wildcard, so the match stays exhaustive over the seventeen and a
    /// kind added without a slug fails to compile.
    fn slug(self) -> Option<&'static str> {
        match self {
            ChangeKind::Unchanged => None,
            ChangeKind::Renamed => Some("renamed"),
            ChangeKind::Removed => Some("removed"),
            ChangeKind::Added => Some("added"),
            ChangeKind::Changed => Some("changed"),
            ChangeKind::SortRenamed => Some("sort_renamed"),
            ChangeKind::EnumRenamed => Some("enum_renamed"),
            ChangeKind::MessageAdded => Some("message_added"),
            ChangeKind::MessageRemoved => Some("message_removed"),
            ChangeKind::EnumAdded => Some("enum_added"),
            ChangeKind::EnumRemoved => Some("enum_removed"),
            ChangeKind::ValueAdded => Some("value_added"),
            ChangeKind::ValueRemoved => Some("value_removed"),
            ChangeKind::ValueRenamed => Some("value_renamed"),
            ChangeKind::OpennessChanged => Some("openness_changed"),
            ChangeKind::PreserveChanged => Some("preserve_changed"),
            ChangeKind::PackageAdded => Some("package_added"),
            ChangeKind::PackageRemoved => Some("package_removed"),
        }
    }
}

/// One row of [`Comparison::changes`]: a changed node flattened with what a changeset record or
/// an exit code needs — its kind, the normalized package, the number (a field's or value's;
/// `None` for a package, sort, or enum row), and, per side, the element's fully-qualified proto
/// path and rendered signature, absent on a side the element is not on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change<'a> {
    kind: ChangeKind,
    package: &'a str,
    number: Option<i32>,
    old: Option<Rendered>,
    new: Option<Rendered>,
    bridge: Option<String>,
}

/// One side of a change row as rendered: the element's fully-qualified proto path, and its
/// signature (a package has none).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rendered {
    path: String,
    signature: Option<String>,
}

impl<'a> Change<'a> {
    fn row(
        kind: ChangeKind,
        package: &'a str,
        element: &'a str,
        number: Option<i32>,
        old: Option<Rendered>,
        new: Option<Rendered>,
        bridge: Option<String>,
    ) -> Row<'a> {
        (
            (package, element, number, kind),
            Change {
                kind,
                package,
                number,
                old,
                new,
                bridge,
            },
        )
    }

    /// The kind.
    #[must_use]
    pub fn kind(&self) -> ChangeKind {
        self.kind
    }

    /// The normalized package (`thermal`), one for both sides.
    #[must_use]
    pub fn package(&self) -> &'a str {
        self.package
    }

    /// The field or value number; `None` for a package, sort, or enum row.
    #[must_use]
    pub fn number(&self) -> Option<i32> {
        self.number
    }

    /// The old side's fully-qualified proto path — a field's (`thermal.v1.Reading.temp_c`), a
    /// sort's or enum's, a value's (`thermal.v1.Level.LEVEL_LOW`), or a package's dotted name —
    /// `None` for an addition.
    #[must_use]
    pub fn old_path(&self) -> Option<&str> {
        self.old.as_ref().map(|side| side.path.as_str())
    }

    /// The new side's fully-qualified proto path, as [`old_path`](Change::old_path); `None` for
    /// a removal.
    #[must_use]
    pub fn new_path(&self) -> Option<&str> {
        self.new.as_ref().map(|side| side.path.as_str())
    }

    /// The old side's rendered signature, in the manifest's own vocabulary (spec §13.4) so the
    /// two cannot drift: a field's `predicate/arity declared descriptor` — `temp_c/2 int32
    /// total`, `readings/3 reading seq`, `counts/3 int32 map<string>` — a sort's `predicate/1`,
    /// an enum's `predicate/1 (open)` (`(open, preserve)` under `PRESERVE`), a value's constant.
    /// A package has none; `None` for an addition.
    #[must_use]
    pub fn old_signature(&self) -> Option<&str> {
        self.old.as_ref().and_then(|side| side.signature.as_deref())
    }

    /// The new side's rendered signature, as [`old_signature`](Change::old_signature); `None`
    /// for a removal.
    #[must_use]
    pub fn new_signature(&self) -> Option<&str> {
        self.new.as_ref().and_then(|side| side.signature.as_deref())
    }

    /// Whether the kind is breaking ([`ChangeKind::is_breaking`]).
    #[must_use]
    pub fn is_breaking(&self) -> bool {
        self.kind.is_breaking()
    }

    /// The node's bridge view, as [`FieldDiff::bridge`] and its siblings give it.
    #[must_use]
    pub fn bridge(&self) -> Option<String> {
        self.bridge.clone()
    }

    /// The row as one record of the JSON changeset ([`Comparison::to_json`]): the fixed keys
    /// always, a missing side `null`; `number` and `bridge` only where the row has them.
    fn record(&self) -> Value {
        let mut record = Map::new();
        let mut put = |key: &str, value: Value| {
            record.insert(key.to_owned(), value);
        };
        put("package", self.package.into());
        put(
            "kind",
            self.kind
                .slug()
                .expect("a row of `changes` is a change; the baseline is skipped at the flatten")
                .into(),
        );
        put("old_path", self.old_path().into());
        put("new_path", self.new_path().into());
        put("old", self.old_signature().into());
        put("new", self.new_signature().into());
        put("breaking", self.is_breaking().into());
        if let Some(number) = self.number {
            put("number", number.into());
        }
        if let Some(bridge) = &self.bridge {
            put("bridge", bridge.as_str().into());
        }
        Value::Object(record)
    }
}

/// A package row's side: its dotted name, no signature.
fn render_package(package: &Package) -> Rendered {
    Rendered {
        path: package.as_str().to_owned(),
        signature: None,
    }
}

/// A sort row's side: `predicate/1`.
fn render_sort(sort: &SortMapping) -> Rendered {
    Rendered {
        path: sort.proto().as_str().to_owned(),
        signature: Some(format!("{}/1", sort.predicate().as_str())),
    }
}

/// An enum row's side: `predicate/1 (open)`, with `, preserve` under `PRESERVE`.
fn render_enum(enumeration: &EnumMapping) -> Rendered {
    let mut signature = format!(
        "{}/1 ({}",
        enumeration.predicate().as_str(),
        manifest::openness_word(enumeration.openness())
    );
    if enumeration.preserve() {
        signature.push_str(", preserve");
    }
    signature.push(')');
    Rendered {
        path: enumeration.proto().as_str().to_owned(),
        signature: Some(signature),
    }
}

/// A field row's side: `predicate/arity`, the manifest's declared-type column, and its descriptor
/// column (`seq`, `map<key>`, `set`, or the totality word) — the manifest's writers, so the
/// comparison and the manifest cannot spell a field two ways.
fn render_field(field: &FieldMapping) -> Rendered {
    Rendered {
        path: field.proto().as_str().to_owned(),
        signature: Some(format!(
            "{}/{} {} {}",
            field.predicate().as_str(),
            field.arity(),
            manifest::declared(field.value()),
            manifest::descriptor(field)
        )),
    }
}

/// A value row's side: its path `<enum>.<VALUE>` (the value's address in the mapping) and its
/// lowered constant.
fn render_value(enumeration: &EnumMapping, value: &EnumValueMapping) -> Rendered {
    Rendered {
        path: format!("{}.{}", enumeration.proto().as_str(), value.proto_name()),
        signature: Some(value.constant().as_str().to_owned()),
    }
}

/// What one bridge view aliases (`bridge_rule`): the old and the new predicate, the arity the
/// two share, the renamed element's proto paths on each side (the `%!` line's provenance), and
/// whether the predicate is a message field's relational view (§13.2), which the line says. A
/// handful of borrows beside a count and a flag, so `Copy`.
#[derive(Clone, Copy)]
struct Bridge<'a> {
    old: &'a Name,
    new: &'a Name,
    arity: u32,
    from: &'a FqName,
    to: &'a FqName,
    view: bool,
}

/// The positional variables a bridge view spells its argument positions over — a fixed
/// compile-time set of valid variable names, one per position of the widest predicate keryx emits
/// (a sort is unary, a function or set binary, a family ternary; spec §4.1), so the width `expect`
/// in [`bridge_rule`] is a discharged invariant (§6). A bridge aliases positions and interprets
/// none, so the letters carry no role.
const BRIDGE_POSITIONS: [&str; 3] = ["A", "B", "C"];

/// The bridge view for one pure rename (spec §13.4): the one rule `old(A, …) :- new(A, …).`
/// aliasing the old predicate to the new at their arity over fresh positional variables, its `%!`
/// line the provenance — `temp_c/2 reads celsius/2  (inbound-facing bridge: <old path> renamed
/// <new path>)`, `bridge of the view` for a message field. Constructed through `emit::build` and
/// rendered through `emit::render` — themelios syntax values crossing the emission boundary every
/// generated module crosses (spec §18), never a format string, so the text is the renderer's one
/// spelling; the `%!` line is prose the renderer writes verbatim as a comment, no part of the rule.
/// Inbound-facing (`old :- new.`, one rule): a model written against the old vocabulary *reads*
/// the new facts through it; one that *emits* old atoms is not shimmed — a limit stated, not
/// papered over.
///
/// Total for what a mapping holds: themelios refuses to spell only a string constant or an
/// `#include` path bearing a control character, and a bridge spells neither — two validated
/// `Name`s over variables — so the render `expect` is a discharged invariant (§6), not a live
/// failure path.
fn bridge_rule(bridge: Bridge<'_>) -> String {
    let Bridge {
        old,
        new,
        arity,
        from,
        to,
        view,
    } = bridge;
    let width = usize::try_from(arity).expect("an arity is a small count");
    let letters = BRIDGE_POSITIONS
        .get(..width)
        .expect("keryx emits no predicate wider than a ternary family (spec §4.1)");
    let positions = || letters.iter().map(|letter| emit::build::var(letter));
    let head = emit::build::atom(old.clone(), positions());
    let body = emit::build::positive(emit::build::atom(new.clone(), positions()));
    let of = if view { " of the view" } else { "" };
    let doc = format!(
        "{}/{arity} reads {}/{arity}  (inbound-facing bridge{of}: {} renamed {})",
        old.as_str(),
        new.as_str(),
        from.as_str(),
        to.as_str()
    );
    emit::render(vec![emit::build::rule(head, body, doc)])
        .expect("a bridge spells validated names over variables, which the clingo dialect renders")
}

/// The sides of one matched node — the old, the new, or both — so a node with neither is
/// unrepresentable and every classification is total over the three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pair<T> {
    Old(T),
    New(T),
    Both(T, T),
}

impl<'a, T> Pair<&'a T> {
    fn old(self) -> Option<&'a T> {
        match self {
            Pair::Old(old) | Pair::Both(old, _) => Some(old),
            Pair::New(_) => None,
        }
    }

    fn new_side(self) -> Option<&'a T> {
        match self {
            Pair::New(new) | Pair::Both(_, new) => Some(new),
            Pair::Old(_) => None,
        }
    }
}

/// Match two identity-keyed maps: a key on both sides pairs its values, a key on one side keeps
/// that side, in key order. The linear join beneath every level of the comparison — a rename is
/// a same-identity, different-name pair that only such a join reveals.
fn merge<K: Ord + Copy, V>(old: BTreeMap<K, V>, mut new: BTreeMap<K, V>) -> Vec<(K, Pair<V>)> {
    let mut merged: Vec<(K, Pair<V>)> = old
        .into_iter()
        .map(|(key, old_value)| match new.remove(&key) {
            Some(new_value) => (key, Pair::Both(old_value, new_value)),
            None => (key, Pair::Old(old_value)),
        })
        .collect();
    merged.extend(new.into_iter().map(|(key, value)| (key, Pair::New(value))));
    merged.sort_by_key(|(key, _)| *key);
    merged
}

/// Which of the two mappings a diagnostic speaks of.
#[derive(Clone, Copy)]
enum Side {
    Old,
    New,
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Old => "old",
            Side::New => "new",
        }
    }
}

/// One side's subject vocabulary for one package: the unit's package as declared (its version
/// still on it), and its subject sorts and enums keyed by package-relative path.
struct SubjectUnit<'a> {
    package: &'a Package,
    sorts: BTreeMap<&'a str, &'a SortMapping>,
    enums: BTreeMap<&'a str, &'a EnumMapping>,
}

impl<'a> SubjectUnit<'a> {
    fn of(unit: &'a Unit) -> SubjectUnit<'a> {
        let package = unit.package();
        let key = |path: &'a str| relative_name(path, package.as_str());
        SubjectUnit {
            package,
            sorts: unit
                .sorts()
                .iter()
                .filter(|sort| sort.is_subject())
                .map(|sort| (key(sort.proto().as_str()), sort))
                .collect(),
            enums: unit
                .enums()
                .iter()
                .filter(|enumeration| enumeration.is_subject())
                .map(|enumeration| (key(enumeration.proto().as_str()), enumeration))
                .collect(),
        }
    }
}

/// One side's subject units keyed by normalized package, with the collisions found. A unit is
/// subject vocabulary when at least one of its sorts or enums is; a pure-closure unit (an
/// imported well-known-type package) is skipped. Two subject units normalizing to one key are
/// `AmbiguousVersionPackages`, one diagnostic per further unit; the first keeps the key, so the
/// comparison's other faults are still collected.
fn subject_units(
    mapping: &Mapping,
    side: Side,
) -> (BTreeMap<&str, SubjectUnit<'_>>, Vec<Diagnostic>) {
    let mut units: BTreeMap<&str, SubjectUnit<'_>> = BTreeMap::new();
    let mut faults = Vec::new();
    for unit in mapping.units().iter().filter(|unit| is_subject_unit(unit)) {
        let key = normalize_package(unit.package().as_str());
        match units.entry(key) {
            Entry::Occupied(first) => faults.push(ambiguous_version_packages(
                side,
                key,
                first.get().package,
                unit.package(),
            )),
            Entry::Vacant(slot) => {
                slot.insert(SubjectUnit::of(unit));
            }
        }
    }
    (units, faults)
}

/// The referents one side's subject vocabulary declares: each subject sort's and enum's emitted
/// predicate, keyed to its identity — the normalized package and the package-relative path.
/// Comparison-wide, since a field may name a sort of another subject package.
type ReferentIndex<'a> = BTreeMap<&'a Name, (&'a str, &'a str)>;

/// Both sides' referent indexes — what decides whether two field values name one type. A
/// value's message or enum referent is the referent's emitted *predicate*, which a rename
/// re-spells (`reading` → `v2__reading`) while the type is the same sort; so two referents are
/// compared by the identity each side's index resolves them to, and only a referent neither
/// side's subject vocabulary declares — a closure sort or enum, which no row of the comparison
/// bridges — is compared as spelled.
struct Referents<'a> {
    old: ReferentIndex<'a>,
    new: ReferentIndex<'a>,
}

impl<'a> Referents<'a> {
    fn of(
        old: &BTreeMap<&'a str, SubjectUnit<'a>>,
        new: &BTreeMap<&'a str, SubjectUnit<'a>>,
    ) -> Referents<'a> {
        Referents {
            old: referent_index(old),
            new: referent_index(new),
        }
    }

    /// Whether two field values are one value for the detection line: two scalars by kind and
    /// treatment; two message referents, or two enum referents, by identity — equal when both
    /// resolve in their side's index to one identity, as spelled when neither resolves (a closure
    /// referent), unequal when exactly one does; anything else (a scalar against a referent, a
    /// message against an enum) unequal. An enum referent's denormalized `preserve` is not
    /// compared here: a flip is the enum's own row (`PreserveChanged`), not every field's.
    fn same_value(&self, old: &ValueMapping, new: &ValueMapping) -> bool {
        match (old, new) {
            (ValueMapping::Scalar { .. }, ValueMapping::Scalar { .. }) => old == new,
            (ValueMapping::Message(old_referent), ValueMapping::Message(new_referent))
            | (
                ValueMapping::Enum {
                    referent: old_referent,
                    ..
                },
                ValueMapping::Enum {
                    referent: new_referent,
                    ..
                },
            ) => self.same_referent(old_referent, new_referent),
            // A scalar against a referent, or a message against an enum: two types.
            _ => false,
        }
    }

    fn same_referent(&self, old: &Name, new: &Name) -> bool {
        match (self.old.get(old), self.new.get(new)) {
            (Some(old_identity), Some(new_identity)) => old_identity == new_identity,
            (None, None) => old == new,
            (Some(_), None) | (None, Some(_)) => false,
        }
    }
}

/// One side's referent index over its subject units.
fn referent_index<'a>(units: &BTreeMap<&'a str, SubjectUnit<'a>>) -> ReferentIndex<'a> {
    let mut index = BTreeMap::new();
    for (&key, unit) in units {
        for (&name, &sort) in &unit.sorts {
            index.insert(sort.predicate(), (key, name));
        }
        for (&name, &enumeration) in &unit.enums {
            index.insert(enumeration.predicate(), (key, name));
        }
    }
    index
}

/// Whether a unit holds any subject vocabulary.
fn is_subject_unit(unit: &Unit) -> bool {
    unit.sorts().iter().any(SortMapping::is_subject)
        || unit.enums().iter().any(EnumMapping::is_subject)
}

fn ambiguous_version_packages(
    side: Side,
    key: &str,
    first: &Package,
    second: &Package,
) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::AmbiguousVersionPackages,
        Locus::whole(),
        format!(
            "the {} schema declares both `{}` and `{}` — two versions of one package, `{key}` — so which version it is stays ambiguous; give each side one version of a package",
            side.word(),
            first.as_str(),
            second.as_str(),
        ),
    )
}

fn no_comparable_schemas(
    old: &BTreeMap<&str, SubjectUnit<'_>>,
    new: &BTreeMap<&str, SubjectUnit<'_>>,
) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::NoComparableSchemas,
        Locus::whole(),
        format!(
            "the old schema ({}) and the new schema ({}) have no package in common once version segments are stripped (imported packages aside); keryx diff compares two versions of one schema",
            own_packages(old),
            own_packages(new),
        ),
    )
}

/// A side's normalized subject packages for a diagnostic — `` `alpha`, `beta` `` — or that it
/// has none.
fn own_packages(units: &BTreeMap<&str, SubjectUnit<'_>>) -> String {
    if units.is_empty() {
        "no package of its own".to_owned()
    } else {
        units
            .keys()
            .map(|key| format!("`{key}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A sort's or enum's path relative to its declaring package — the identity a sort or enum is
/// matched by across versions, the version living in the package alone: `thermal.v1.Reading` in
/// `thermal.v1` is `Reading`, `thermal.v1.Outer.Inner` is `Outer.Inner`. Only a whole run of
/// leading segments is stripped (`thermal.v1x.Foo` is no member of `thermal.v1`); a path outside
/// the package, or one under the empty package (a package-less file, refused upstream), is
/// returned whole. Nothing is allocated.
fn relative_name<'a>(path: &'a str, package: &str) -> &'a str {
    if package.is_empty() {
        return path;
    }
    path.strip_prefix(package)
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(path)
}

/// A side's fields by number, and its oneof cells: each oneof's member numbers under its label —
/// the label being the join key within one side only, never across the sides.
fn index_fields(sort: Option<&SortMapping>) -> (BTreeMap<i32, &FieldMapping>, Cells<'_>) {
    let mut fields = BTreeMap::new();
    let mut cells: Cells<'_> = BTreeMap::new();
    for field in sort.map_or(&[][..], SortMapping::fields) {
        fields.insert(field.number(), field);
        if let EmitForm::OneofArm { oneof } = field.form() {
            cells
                .entry(oneof.as_str())
                .or_default()
                .insert(field.number());
        }
    }
    (fields, cells)
}

/// One side's oneof cells: the member numbers of each oneof, under its label.
type Cells<'a> = BTreeMap<&'a str, BTreeSet<i32>>;

/// A field's oneof cell on its side, among the `matched` fields (those both sides hold): the
/// member numbers of the oneof it is an arm of, or empty when it is no arm — or is absent from
/// that side. Restricting a cell to the matched fields is what makes an arm added or removed its
/// own row rather than a change of every partner: a partner's cell reads the same on both sides
/// once the one-sided member is left out, while a member that moved — held by both sides — still
/// changes the cells it left and joined.
fn cell_of(
    cells: &Cells<'_>,
    field: Option<&FieldMapping>,
    matched: &BTreeSet<i32>,
) -> BTreeSet<i32> {
    match field.map(FieldMapping::form) {
        Some(EmitForm::OneofArm { oneof }) => cells
            .get(oneof.as_str())
            .map_or_else(BTreeSet::new, |cell| cell & matched),
        _ => BTreeSet::new(),
    }
}

/// Form equality for the detection line: two oneof arms are one form whatever their oneofs are
/// named — the name reaches only `emit.lp`'s `violates` string, never the model's vocabulary —
/// and every other form compares by value (a map by its key and the key's treatment).
fn same_form(old: &EmitForm, new: &EmitForm) -> bool {
    matches!(
        (old, new),
        (EmitForm::OneofArm { .. }, EmitForm::OneofArm { .. })
    ) || old == new
}

/// A package with its trailing version segment stripped — the key under which a unit is matched
/// across schema versions, so `thermal.v1` and `thermal.v2` are one unit, `thermal`. The version
/// grammar is buf's `PACKAGE_VERSION_SUFFIX` lint rule ([`is_version_segment`]), and a versioned
/// package is a name and a version — two dot-separated segments at least, as buf reads it — so a
/// lone version segment (`v1`) is not stripped. Only the last segment is examined, and only in
/// full: `acme.v1.dispatch` is unchanged (its last segment is not a version, whatever an earlier
/// one is), as are `acme.v1things` and a package with no version segment at all. Returns a
/// prefix of its input; nothing is allocated.
#[must_use]
pub(crate) fn normalize_package(package: &str) -> &str {
    match package.rsplit_once('.') {
        Some((prefix, last)) if is_version_segment(last) => prefix,
        _ => package,
    }
}

/// Whether `segment` is a package version under buf's `PACKAGE_VERSION_SUFFIX` rule: `v\d+`
/// (`v1`), `v\d+test.*` (`v1test`, `v1testfoo`), `v\d+(alpha|beta)\d*` (`v1beta`, `v2alpha1`),
/// or `v\d+p\d+(alpha|beta)\d*` (`v1p1beta1`) — the whole segment, each numeric part at least 1
/// by value as buf reads it (`v0` is refused; `v01` is `v1`). So `v1things` is not a version,
/// nor `v1p1` (a patch demands its stability), nor `V1`. Hand-written over the grammar; no
/// regular-expression dependency.
fn is_version_segment(segment: &str) -> bool {
    let Some(rest) = segment.strip_prefix('v').and_then(strip_number) else {
        return false;
    };
    // `v\d+` and `v\d+test.*`.
    if rest.is_empty() || rest.starts_with("test") {
        return true;
    }
    // The optional patch `p\d+`, then the stability the two remaining forms share.
    let stability = match rest.strip_prefix('p').map(strip_number) {
        Some(None) => return false,
        Some(Some(after_patch)) => after_patch,
        None => rest,
    };
    let Some(number) = stability
        .strip_prefix("alpha")
        .or_else(|| stability.strip_prefix("beta"))
    else {
        return false;
    };
    number.is_empty() || strip_number(number).is_some_and(str::is_empty)
}

/// `s` past its leading numeric part, when it has one buf would accept: a run of ASCII digits of
/// value at least 1 — so an empty run and an all-zero run are `None`, and a leading zero is
/// admitted as buf's integer parse admits it.
fn strip_number(s: &str) -> Option<&str> {
    let (digits, rest) = s.split_at(s.bytes().take_while(u8::is_ascii_digit).count());
    digits.bytes().any(|digit| digit != b'0').then_some(rest)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;
    use themelios_program::prelude::{
        Arguments, Atom, BodyElement, DefaultNegation, Dialect, Head, Literal, LiteralInner, Name,
        Source, SourceId, Statement,
    };
    use themelios_program::raise::raise_source;

    use super::{
        Change, ChangeKind, Comparison, PackageDiff, SortDiff, compare, is_version_segment,
        normalize_package, relative_name,
    };
    use crate::descriptor::model::{FqName, MapKey, Openness, Package, Scalar};
    use crate::diagnostics::DiagnosticKind;
    use crate::emit::{build, render};
    use crate::policy::model::{
        EmitForm, EnumMapping, EnumValueMapping, FieldMapping, Mapping, ScalarTreatment,
        SortMapping, Totality, Unit, ValueMapping,
    };

    fn name(text: &str) -> Name {
        Name::new(text).expect("test name is a valid identifier")
    }

    fn mapping(units: Vec<Unit>) -> Mapping {
        Mapping { units }
    }

    fn unit(package: &str, sorts: Vec<SortMapping>, enums: Vec<EnumMapping>) -> Unit {
        Unit {
            package: Package::parse(package).expect("test package is valid"),
            sorts,
            enums,
        }
    }

    /// A subject message sort (a test flips `subject` on the literal for a closure sort).
    fn sort(path: &str, predicate: &str, fields: Vec<FieldMapping>) -> SortMapping {
        SortMapping {
            proto: FqName::new(path),
            predicate: name(predicate),
            qualifier: Vec::new(),
            escaped: false,
            recursive: false,
            doc: None,
            fields,
            subject: true,
        }
    }

    /// A field whose arity follows its form as the policy sets it: a family is ternary, the
    /// rest binary.
    fn field(
        path: &str,
        number: i32,
        predicate: &str,
        form: EmitForm,
        value: ValueMapping,
        presence: Totality,
    ) -> FieldMapping {
        let arity = match form {
            EmitForm::Function | EmitForm::OneofArm { .. } | EmitForm::Set => 2,
            EmitForm::Sequence | EmitForm::Map { .. } => 3,
        };
        FieldMapping {
            proto: FqName::new(path),
            number,
            predicate: name(predicate),
            arity,
            form,
            value,
            presence,
            escaped: false,
            doc: None,
        }
    }

    fn int32() -> ValueMapping {
        ValueMapping::Scalar {
            kind: Scalar::Int32,
            treatment: ScalarTreatment::Native,
        }
    }

    fn text() -> ValueMapping {
        ValueMapping::Scalar {
            kind: Scalar::String,
            treatment: ScalarTreatment::Text,
        }
    }

    fn message(referent: &str) -> ValueMapping {
        ValueMapping::Message(name(referent))
    }

    /// A singular, total `int32` field — the common case.
    fn scalar(path: &str, number: i32, predicate: &str) -> FieldMapping {
        field(
            path,
            number,
            predicate,
            EmitForm::Function,
            int32(),
            Totality::Total,
        )
    }

    /// A partial `int32` oneof arm under `oneof`.
    fn arm(path: &str, number: i32, predicate: &str, oneof: &str) -> FieldMapping {
        field(
            path,
            number,
            predicate,
            EmitForm::OneofArm {
                oneof: oneof.to_owned(),
            },
            int32(),
            Totality::Partial,
        )
    }

    fn enumeration(
        path: &str,
        predicate: &str,
        openness: Openness,
        preserve: bool,
        values: Vec<EnumValueMapping>,
    ) -> EnumMapping {
        EnumMapping {
            proto: FqName::new(path),
            predicate: name(predicate),
            qualifier: Vec::new(),
            escaped: false,
            openness,
            preserve,
            doc: None,
            values,
            subject: true,
        }
    }

    fn value(proto_name: &str, number: i32, constant: &str) -> EnumValueMapping {
        EnumValueMapping {
            proto_name: proto_name.to_owned(),
            number,
            constant: name(constant),
            escaped: false,
            doc: None,
        }
    }

    /// The thermal reading at version `v`: `sensor` #1 (a string) and `temp_c` #2 (an int32).
    fn reading(v: &str) -> SortMapping {
        sort(
            &format!("thermal.{v}.Reading"),
            "reading",
            vec![
                field(
                    &format!("thermal.{v}.Reading.sensor"),
                    1,
                    "sensor",
                    EmitForm::Function,
                    text(),
                    Totality::Total,
                ),
                scalar(&format!("thermal.{v}.Reading.temp_c"), 2, "temp_c"),
            ],
        )
    }

    /// The open `Level` enum at version `v`, with `values`.
    fn level(v: &str, values: Vec<EnumValueMapping>) -> EnumMapping {
        enumeration(
            &format!("thermal.{v}.Level"),
            "level",
            Openness::Open,
            false,
            values,
        )
    }

    /// A one-unit mapping for `thermal.<v>`.
    fn thermal(v: &str, sorts: Vec<SortMapping>, enums: Vec<EnumMapping>) -> Mapping {
        mapping(vec![unit(&format!("thermal.{v}"), sorts, enums)])
    }

    /// A referent-closure `google.protobuf.Timestamp` sort with one field named `seconds`.
    fn timestamp(seconds: &str) -> SortMapping {
        let mut sort = sort(
            "google.protobuf.Timestamp",
            "timestamp",
            vec![scalar(
                &format!("google.protobuf.Timestamp.{seconds}"),
                1,
                seconds,
            )],
        );
        sort.subject = false;
        sort
    }

    fn kinds(comparison: &Comparison<'_>) -> Vec<ChangeKind> {
        comparison.changes().iter().map(Change::kind).collect()
    }

    fn field_kinds(sort: &SortDiff<'_>) -> Vec<(i32, ChangeKind)> {
        sort.fields()
            .iter()
            .map(|field| (field.number(), field.kind()))
            .collect()
    }

    /// Parse a bridge back through themelios and return its one rule as the head atom and the one
    /// positive atom of its body — the round trip the text must survive as ASP, whatever wrote it.
    fn parse_bridge(text: &str) -> (Atom, Atom) {
        let source = Source::new(SourceId::new(0), text.to_owned()).expect("a bridge is small");
        let raised = raise_source(&source, Dialect::Clingo);
        assert!(raised.syntax_diagnostics().is_empty(), "parses: {text}");
        assert!(raised.lowering_diagnostics().is_empty(), "raises: {text}");
        let statements: Vec<_> = raised.program().statements().collect();
        let [statement] = statements.as_slice() else {
            panic!("one statement: {text}")
        };
        let Statement::Rule(rule) = statement.get() else {
            panic!("a rule: {text}")
        };
        let Head::Literal(Literal {
            negation: DefaultNegation::None,
            inner: LiteralInner::Atom(head),
        }) = rule.head().get()
        else {
            panic!("a positive atom head: {text}")
        };
        let elements: Vec<_> = rule.body().get().elements().collect();
        let [element] = elements.as_slice() else {
            panic!("one body element: {text}")
        };
        let BodyElement::Literal(Literal {
            negation: DefaultNegation::None,
            inner: LiteralInner::Atom(body),
        }) = element.get()
        else {
            panic!("a positive body atom: {text}")
        };
        (head.get().clone(), body.get().clone())
    }

    #[test]
    fn a_trailing_version_segment_is_stripped_in_each_of_bufs_four_forms() {
        for (package, want) in [
            ("thermal.v1", "thermal"),
            ("thermal.v2", "thermal"),
            ("thermal.v10", "thermal"),
            ("acme.v1beta1", "acme"),
            ("acme.v2alpha1", "acme"),
            ("acme.v1beta", "acme"),
            ("acme.v1p1beta1", "acme"),
            ("acme.v2p3alpha", "acme"),
            ("acme.v1test", "acme"),
            ("acme.v1testfoo", "acme"),
            ("acme.inner.v1", "acme.inner"),
        ] {
            assert_eq!(normalize_package(package), want, "{package}");
        }
    }

    #[test]
    fn a_package_whose_last_segment_is_not_a_version_is_unchanged() {
        for package in [
            "thermal",
            "acme.v1things",
            "acme.v1.dispatch",
            "",
            "acme.V1",
            "acme.v1p1",
            "acme.v",
        ] {
            assert_eq!(normalize_package(package), package);
        }
    }

    #[test]
    fn a_package_that_is_one_version_segment_alone_is_unchanged() {
        // buf's versioned package is a name and a version, two segments at least: a bare `v1`
        // is not a versioned package, so there is nothing to strip.
        assert_eq!(normalize_package("v1"), "v1");
        assert_eq!(normalize_package("v1beta1"), "v1beta1");
    }

    #[test]
    fn a_version_segment_matches_one_of_bufs_four_forms_in_full() {
        for segment in [
            "v1",
            "v10",
            "v1test",
            "v1testfoo",
            "v1beta",
            "v1beta1",
            "v2alpha",
            "v2alpha12",
            "v1p1beta1",
            "v2p3alpha",
            "v1p1beta",
        ] {
            assert!(is_version_segment(segment), "{segment} is a version");
        }
        for segment in [
            "",
            "v",
            "1",
            "V1",
            "v1x",
            "v1things",
            "v1p1",
            "v1p",
            "v1pbeta1",
            "vp1beta1",
            "v1alpha1x",
            "v1p1test",
            "v1betax",
            "v1gamma1",
            "dispatch",
            "version1",
            "v1 ",
            " v1",
            "v1.2",
        ] {
            assert!(!is_version_segment(segment), "{segment} is not a version");
        }
    }

    #[test]
    fn a_numeric_part_is_at_least_one_by_value_as_buf_reads_it() {
        // buf parses each numeric part as an integer and demands at least 1: a zero is refused in
        // every position; a leading zero is a spelling of the same number, so it is admitted.
        for segment in ["v0", "v00", "v1beta0", "v1p0beta1", "v1p1alpha0"] {
            assert!(!is_version_segment(segment), "{segment} is not a version");
        }
        for segment in ["v01", "v1beta01", "v1p01beta1"] {
            assert!(is_version_segment(segment), "{segment} is a version");
        }
        assert_eq!(normalize_package("acme.v0"), "acme.v0");
        assert_eq!(normalize_package("acme.v01"), "acme");
    }

    #[test]
    fn a_relative_name_strips_the_declaring_package_at_a_segment_boundary() {
        assert_eq!(relative_name("thermal.v1.Reading", "thermal.v1"), "Reading");
        assert_eq!(
            relative_name("thermal.v1.Outer.Inner", "thermal.v1"),
            "Outer.Inner"
        );
        // Only a whole leading run of segments is stripped: `thermal.v1` is a prefix of
        // `thermal.v1x` but not a segment boundary of it.
        assert_eq!(
            relative_name("thermal.v1x.Foo", "thermal.v1"),
            "thermal.v1x.Foo"
        );
        assert_eq!(relative_name("other.Foo", "thermal.v1"), "other.Foo");
        assert_eq!(relative_name("Foo", ""), "Foo");
    }

    #[test]
    fn a_version_bump_matches_fields_by_number_rather_than_removing_and_adding() {
        // thermal.v1 → thermal.v2: the package matches by its normalized key, the sort by its
        // relative name, field #2 by its number — so `temp_c` → `celsius` is one rename, never a
        // removal beside an addition from a mis-stripped key.
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let new = thermal(
            "v2",
            vec![sort(
                "thermal.v2.Reading",
                "reading",
                vec![
                    field(
                        "thermal.v2.Reading.sensor",
                        1,
                        "sensor",
                        EmitForm::Function,
                        text(),
                        Totality::Total,
                    ),
                    scalar("thermal.v2.Reading.celsius", 2, "celsius"),
                    scalar("thermal.v2.Reading.humidity", 3, "humidity"),
                ],
            )],
            vec![],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let [package] = comparison.packages() else {
            panic!("one matched package: {comparison:?}")
        };
        assert_eq!(package.key(), "thermal");
        assert_eq!(
            package.old_package().map(Package::as_str),
            Some("thermal.v1")
        );
        assert_eq!(
            package.new_package().map(Package::as_str),
            Some("thermal.v2")
        );
        assert_eq!(package.kind(), ChangeKind::Unchanged);
        let [reading] = package.sorts() else {
            panic!("one matched sort: {package:?}")
        };
        assert_eq!(reading.name(), "Reading");
        assert_eq!(reading.kind(), ChangeKind::Unchanged);
        assert_eq!(
            field_kinds(reading),
            [
                (1, ChangeKind::Unchanged),
                (2, ChangeKind::Renamed),
                (3, ChangeKind::Added)
            ]
        );
        assert_eq!(kinds(&comparison), [ChangeKind::Renamed, ChangeKind::Added]);
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn a_rename_row_carries_both_sides_paths_and_signatures() {
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut renamed = reading("v2");
        renamed.fields[1] = scalar("thermal.v2.Reading.celsius", 2, "celsius");
        let new = thermal("v2", vec![renamed], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let changes = comparison.changes();
        let [change] = changes.as_slice() else {
            panic!("one change: {changes:?}")
        };
        assert_eq!(change.kind(), ChangeKind::Renamed);
        assert_eq!(change.package(), "thermal");
        assert_eq!(change.number(), Some(2));
        assert_eq!(change.old_path(), Some("thermal.v1.Reading.temp_c"));
        assert_eq!(change.new_path(), Some("thermal.v2.Reading.celsius"));
        assert_eq!(change.old_signature(), Some("temp_c/2 int32 total"));
        assert_eq!(change.new_signature(), Some("celsius/2 int32 total"));
        assert!(!change.is_breaking());
    }

    #[test]
    fn an_added_field_is_additive() {
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut with_humidity = reading("v2");
        with_humidity
            .fields
            .push(scalar("thermal.v2.Reading.humidity", 3, "humidity"));
        let new = thermal("v2", vec![with_humidity], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let changes = comparison.changes();
        let [change] = changes.as_slice() else {
            panic!("one change: {changes:?}")
        };
        assert_eq!(change.kind(), ChangeKind::Added);
        assert!(!change.is_breaking());
        assert_eq!(change.number(), Some(3));
        assert_eq!(change.old_path(), None);
        assert_eq!(change.new_path(), Some("thermal.v2.Reading.humidity"));
        assert_eq!(change.old_signature(), None);
        assert_eq!(change.new_signature(), Some("humidity/2 int32 total"));
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn a_removed_field_is_breaking() {
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut without_sensor = reading("v2");
        without_sensor.fields.remove(0);
        let new = thermal("v2", vec![without_sensor], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let changes = comparison.changes();
        let [change] = changes.as_slice() else {
            panic!("one change: {changes:?}")
        };
        assert_eq!(change.kind(), ChangeKind::Removed);
        assert!(change.is_breaking());
        assert_eq!(change.old_path(), Some("thermal.v1.Reading.sensor"));
        assert_eq!(change.new_path(), None);
        assert_eq!(change.old_signature(), Some("sensor/2 string total"));
        assert_eq!(change.new_signature(), None);
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_rename_that_also_changes_shape_is_a_change_with_no_bridge() {
        // `temp_c` (int32) → `celsius` (int64): the name and the value type both differ, so this
        // is no rename to bridge but a breaking change, its aspect naming exactly those two
        // dimensions.
        let int64 = ValueMapping::Scalar {
            kind: Scalar::Int64,
            treatment: ScalarTreatment::DecimalString,
        };
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut widened = reading("v2");
        widened.fields[1] = field(
            "thermal.v2.Reading.celsius",
            2,
            "celsius",
            EmitForm::Function,
            int64.clone(),
            Totality::Total,
        );
        let new = thermal("v2", vec![widened], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let celsius = &comparison.packages()[0].sorts()[0].fields()[1];
        assert_eq!(celsius.kind(), ChangeKind::Changed);
        assert_eq!(celsius.bridge(), None);
        let aspect = celsius
            .aspect()
            .expect("a changed field carries its aspect");
        assert_eq!(
            aspect.predicate().map(|(o, n)| (o.as_str(), n.as_str())),
            Some(("temp_c", "celsius"))
        );
        assert_eq!(aspect.value(), Some((&int32(), &int64)));
        assert_eq!(aspect.arity(), None);
        assert_eq!(aspect.form(), None);
        assert_eq!(aspect.presence(), None);
        assert_eq!(aspect.membership(), None);
        assert_eq!(kinds(&comparison), [ChangeKind::Changed]);
        assert!(comparison.is_breaking());
        let changes = comparison.changes();
        assert_eq!(changes[0].bridge(), None);
    }

    #[test]
    fn a_field_number_change_is_a_removal_and_an_addition() {
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut renumbered = reading("v2");
        renumbered.fields[1] = scalar("thermal.v2.Reading.temp_c", 4, "temp_c");
        let new = thermal("v2", vec![renumbered], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let rows: Vec<(Option<i32>, ChangeKind)> = comparison
            .changes()
            .iter()
            .map(|change| (change.number(), change.kind()))
            .collect();
        assert_eq!(
            rows,
            [(Some(2), ChangeKind::Removed), (Some(4), ChangeKind::Added)]
        );
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_changed_field_records_each_differing_dimension_in_its_aspect() {
        // `alerts` set → sequence (the form, and with it the arity); `note` total → partial (the
        // presence); `count` native → decimal string (the value's resolved treatment is part of
        // the value dimension — it changes the term a fact carries). Unchanged dimensions are
        // absent, and a pure rename carries no aspect at all.
        let count = |treatment| ValueMapping::Scalar {
            kind: Scalar::Uint32,
            treatment,
        };
        let batch = |v: &str, form: EmitForm, presence: Totality, treatment| {
            sort(
                &format!("thermal.{v}.Batch"),
                "batch",
                vec![
                    field(
                        &format!("thermal.{v}.Batch.alerts"),
                        1,
                        "alerts",
                        form,
                        message("alert"),
                        Totality::Total,
                    ),
                    field(
                        &format!("thermal.{v}.Batch.note"),
                        2,
                        "note",
                        EmitForm::Function,
                        text(),
                        presence,
                    ),
                    field(
                        &format!("thermal.{v}.Batch.count"),
                        3,
                        "count",
                        EmitForm::Function,
                        count(treatment),
                        Totality::Total,
                    ),
                ],
            )
        };
        let old = thermal(
            "v1",
            vec![batch(
                "v1",
                EmitForm::Set,
                Totality::Total,
                ScalarTreatment::Native,
            )],
            vec![],
        );
        let new = thermal(
            "v2",
            vec![batch(
                "v2",
                EmitForm::Sequence,
                Totality::Partial,
                ScalarTreatment::DecimalString,
            )],
            vec![],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let fields = comparison.packages()[0].sorts()[0].fields();
        let alerts = fields[0].aspect().expect("alerts changed");
        assert_eq!(alerts.form(), Some((&EmitForm::Set, &EmitForm::Sequence)));
        assert_eq!(alerts.arity(), Some((2, 3)));
        assert_eq!(alerts.predicate(), None);
        assert_eq!(alerts.value(), None);
        assert_eq!(alerts.presence(), None);
        let note = fields[1].aspect().expect("note changed");
        assert_eq!(note.presence(), Some((Totality::Total, Totality::Partial)));
        assert_eq!(note.form(), None);
        assert_eq!(note.arity(), None);
        let count = fields[2].aspect().expect("count changed");
        assert_eq!(
            count.value(),
            Some((
                &count_value(ScalarTreatment::Native),
                &count_value(ScalarTreatment::DecimalString)
            ))
        );
        assert_eq!(kinds(&comparison), [ChangeKind::Changed; 3]);
    }

    fn count_value(treatment: ScalarTreatment) -> ValueMapping {
        ValueMapping::Scalar {
            kind: Scalar::Uint32,
            treatment,
        }
    }

    #[test]
    fn a_pure_rename_and_an_unchanged_field_carry_no_aspect() {
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut renamed = reading("v2");
        renamed.fields[1] = scalar("thermal.v2.Reading.celsius", 2, "celsius");
        let new = thermal("v2", vec![renamed], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let fields = comparison.packages()[0].sorts()[0].fields();
        assert_eq!(fields[0].kind(), ChangeKind::Unchanged);
        assert_eq!(fields[0].aspect(), None);
        assert_eq!(fields[1].kind(), ChangeKind::Renamed);
        assert_eq!(fields[1].aspect(), None);
    }

    #[test]
    fn a_closure_only_package_is_not_diffed() {
        // `google.protobuf` is referent closure on both sides (a well-known type pulled in by a
        // subject field): it changes between the versions and the comparison never looks. A
        // closure element inside a subject unit is skipped just the same.
        let mut imported_v1 = sort("thermal.v1.Imported", "imported", vec![]);
        imported_v1.subject = false;
        let mut imported_v2 = sort(
            "thermal.v2.Imported",
            "imported",
            vec![scalar("thermal.v2.Imported.n", 1, "n")],
        );
        imported_v2.subject = false;
        let old = mapping(vec![
            unit("google.protobuf", vec![timestamp("seconds")], vec![]),
            unit("thermal.v1", vec![imported_v1, reading("v1")], vec![]),
        ]);
        let new = mapping(vec![
            unit("google.protobuf", vec![timestamp("secs")], vec![]),
            unit("thermal.v2", vec![imported_v2, reading("v2")], vec![]),
        ]);
        let comparison = compare(&old, &new).expect("comparable");
        let keys: Vec<&str> = comparison.packages().iter().map(PackageDiff::key).collect();
        assert_eq!(keys, ["thermal"]);
        let names: Vec<&str> = comparison.packages()[0]
            .sorts()
            .iter()
            .map(SortDiff::name)
            .collect();
        assert_eq!(names, ["Reading"]);
        assert!(comparison.changes().is_empty());
    }

    #[test]
    fn unrelated_schemas_sharing_only_a_closure_package_are_not_comparable() {
        let old = mapping(vec![
            unit("alpha.v1", vec![sort("alpha.v1.A", "a", vec![])], vec![]),
            unit("google.protobuf", vec![timestamp("seconds")], vec![]),
        ]);
        let new = mapping(vec![
            unit("beta.v1", vec![sort("beta.v1.B", "b", vec![])], vec![]),
            unit("google.protobuf", vec![timestamp("seconds")], vec![]),
        ]);
        let diagnostics = compare(&old, &new).expect_err("nothing comparable");
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.iter().next().expect("one cause");
        assert_eq!(diagnostic.kind(), DiagnosticKind::NoComparableSchemas);
        assert!(diagnostic.locus().is_whole());
        let detail = diagnostic.detail();
        assert!(
            detail.contains("`alpha`") && detail.contains("`beta`"),
            "{detail}"
        );
        assert!(!detail.contains("google"), "closure never named: {detail}");
    }

    #[test]
    fn a_side_with_no_subject_package_is_not_comparable() {
        let old = mapping(vec![unit(
            "google.protobuf",
            vec![timestamp("seconds")],
            vec![],
        )]);
        let new = thermal("v1", vec![reading("v1")], vec![]);
        let diagnostics = compare(&old, &new).expect_err("nothing comparable");
        let diagnostic = diagnostics.iter().next().expect("one cause");
        assert_eq!(diagnostic.kind(), DiagnosticKind::NoComparableSchemas);
        let detail = diagnostic.detail();
        assert!(
            detail.contains("the old schema (no package of its own)"),
            "{detail}"
        );
        assert!(detail.contains("`thermal`"), "{detail}");
    }

    #[test]
    fn two_subject_units_normalizing_to_one_key_are_ambiguous() {
        let two_versions = mapping(vec![
            unit("thermal.v1", vec![reading("v1")], vec![]),
            unit("thermal.v2", vec![reading("v2")], vec![]),
        ]);
        let one = thermal("v3", vec![reading("v3")], vec![]);
        let diagnostics = compare(&two_versions, &one).expect_err("ambiguous");
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.iter().next().expect("one cause");
        assert_eq!(diagnostic.kind(), DiagnosticKind::AmbiguousVersionPackages);
        assert!(diagnostic.locus().is_whole());
        let detail = diagnostic.detail();
        assert!(
            detail.contains("old schema")
                && detail.contains("`thermal.v1`")
                && detail.contains("`thermal.v2`"),
            "{detail}"
        );
        // Both sides ambiguous: every cause is collected, the new side's named as such.
        let diagnostics = compare(&two_versions, &two_versions).expect_err("ambiguous");
        assert_eq!(diagnostics.len(), 2);
        let second = diagnostics.iter().nth(1).expect("two causes");
        assert_eq!(second.kind(), DiagnosticKind::AmbiguousVersionPackages);
        assert!(
            second.detail().contains("new schema"),
            "{}",
            second.detail()
        );
    }

    #[test]
    fn partial_overlap_carries_a_removed_package_with_its_elements() {
        let legacy = unit(
            "legacy.v1",
            vec![sort(
                "legacy.v1.Old",
                "old",
                vec![scalar("legacy.v1.Old.n", 1, "n")],
            )],
            vec![enumeration(
                "legacy.v1.Mode",
                "mode",
                Openness::Open,
                false,
                vec![value("MODE_A", 0, "a")],
            )],
        );
        let old = mapping(vec![
            legacy,
            unit("thermal.v1", vec![reading("v1")], vec![]),
        ]);
        let new = thermal("v2", vec![reading("v2")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let [legacy, thermal] = comparison.packages() else {
            panic!("two packages: {comparison:?}")
        };
        assert_eq!(thermal.kind(), ChangeKind::Unchanged);
        assert_eq!(
            (legacy.key(), legacy.kind()),
            ("legacy", ChangeKind::PackageRemoved)
        );
        assert_eq!(legacy.old_package().map(Package::as_str), Some("legacy.v1"));
        assert_eq!(legacy.new_package(), None);
        let [old_sort] = legacy.sorts() else {
            panic!("one removed sort: {legacy:?}")
        };
        assert_eq!(old_sort.kind(), ChangeKind::MessageRemoved);
        assert!(old_sort.new_sort().is_none());
        assert_eq!(field_kinds(old_sort), [(1, ChangeKind::Removed)]);
        let [mode] = legacy.enums() else {
            panic!("one removed enum: {legacy:?}")
        };
        assert_eq!(mode.kind(), ChangeKind::EnumRemoved);
        assert!(mode.new_enum().is_none());
        let values: Vec<(i32, ChangeKind)> = mode
            .values()
            .iter()
            .map(|value| (value.number(), value.kind()))
            .collect();
        assert_eq!(values, [(0, ChangeKind::ValueRemoved)]);
        assert_eq!(
            kinds(&comparison),
            [
                ChangeKind::PackageRemoved,
                ChangeKind::EnumRemoved,
                ChangeKind::ValueRemoved,
                ChangeKind::MessageRemoved,
                ChangeKind::Removed,
            ]
        );
        assert!(comparison.is_breaking());
    }

    #[test]
    fn partial_overlap_carries_an_added_package_with_its_elements() {
        let fresh = unit(
            "fresh.v1",
            vec![sort(
                "fresh.v1.New",
                "new",
                vec![scalar("fresh.v1.New.n", 1, "n")],
            )],
            vec![enumeration(
                "fresh.v1.Mode",
                "mode",
                Openness::Open,
                false,
                vec![value("MODE_A", 0, "a")],
            )],
        );
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let new = mapping(vec![fresh, unit("thermal.v2", vec![reading("v2")], vec![])]);
        let comparison = compare(&old, &new).expect("comparable");
        let [fresh, thermal] = comparison.packages() else {
            panic!("two packages: {comparison:?}")
        };
        assert_eq!(thermal.kind(), ChangeKind::Unchanged);
        assert_eq!(
            (fresh.key(), fresh.kind()),
            ("fresh", ChangeKind::PackageAdded)
        );
        assert_eq!(fresh.old_package(), None);
        assert_eq!(fresh.new_package().map(Package::as_str), Some("fresh.v1"));
        assert!(fresh.sorts()[0].old_sort().is_none());
        assert!(fresh.enums()[0].old_enum().is_none());
        assert_eq!(
            kinds(&comparison),
            [
                ChangeKind::PackageAdded,
                ChangeKind::EnumAdded,
                ChangeKind::ValueAdded,
                ChangeKind::MessageAdded,
                ChangeKind::Added,
            ]
        );
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn a_package_row_names_the_package_on_its_side_and_has_no_signature() {
        let old = mapping(vec![
            unit(
                "legacy.v1",
                vec![sort("legacy.v1.Old", "old", vec![])],
                vec![],
            ),
            unit("thermal.v1", vec![reading("v1")], vec![]),
        ]);
        let new = thermal("v2", vec![reading("v2")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let changes = comparison.changes();
        let row = &changes[0];
        assert_eq!(
            (row.kind(), row.package(), row.number()),
            (ChangeKind::PackageRemoved, "legacy", None)
        );
        assert_eq!(row.old_path(), Some("legacy.v1"));
        assert_eq!(row.new_path(), None);
        assert_eq!(row.old_signature(), None);
        assert_eq!(row.new_signature(), None);
        assert!(row.is_breaking());
        assert_eq!(row.bridge(), None);
    }

    #[test]
    fn a_message_removed_within_a_matched_package_is_breaking() {
        let old = thermal(
            "v1",
            vec![
                sort(
                    "thermal.v1.Alert",
                    "alert",
                    vec![scalar("thermal.v1.Alert.n", 1, "n")],
                ),
                reading("v1"),
            ],
            vec![],
        );
        let new = thermal("v2", vec![reading("v2")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let [alert, reading] = comparison.packages()[0].sorts() else {
            panic!("two sorts: {comparison:?}")
        };
        assert_eq!(alert.kind(), ChangeKind::MessageRemoved);
        assert_eq!(reading.kind(), ChangeKind::Unchanged);
        assert_eq!(
            kinds(&comparison),
            [ChangeKind::MessageRemoved, ChangeKind::Removed]
        );
        let changes = comparison.changes();
        assert_eq!(changes[0].old_path(), Some("thermal.v1.Alert"));
        assert_eq!(changes[0].old_signature(), Some("alert/1"));
        assert_eq!(changes[0].number(), None);
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_message_added_within_a_matched_package_is_additive() {
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let new = thermal(
            "v2",
            vec![
                sort(
                    "thermal.v2.Alert",
                    "alert",
                    vec![scalar("thermal.v2.Alert.n", 1, "n")],
                ),
                reading("v2"),
            ],
            vec![],
        );
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            comparison.packages()[0].sorts()[0].kind(),
            ChangeKind::MessageAdded
        );
        assert_eq!(
            kinds(&comparison),
            [ChangeKind::MessageAdded, ChangeKind::Added]
        );
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn an_enum_removed_within_a_matched_package_is_breaking() {
        let old = thermal(
            "v1",
            vec![reading("v1")],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal("v2", vec![reading("v2")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let [level] = comparison.packages()[0].enums() else {
            panic!("one removed enum: {comparison:?}")
        };
        assert_eq!(level.kind(), ChangeKind::EnumRemoved);
        assert_eq!(level.name(), "Level");
        assert_eq!(
            kinds(&comparison),
            [ChangeKind::EnumRemoved, ChangeKind::ValueRemoved]
        );
        assert!(comparison.is_breaking());
    }

    #[test]
    fn an_enum_added_within_a_matched_package_is_additive() {
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let new = thermal(
            "v2",
            vec![reading("v2")],
            vec![level("v2", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            comparison.packages()[0].enums()[0].kind(),
            ChangeKind::EnumAdded
        );
        assert_eq!(
            kinds(&comparison),
            [ChangeKind::EnumAdded, ChangeKind::ValueAdded]
        );
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn a_sort_whose_predicate_changes_while_its_identity_holds_is_a_sort_rename() {
        // A new collision in v2 qualifies `reading/1` to `v2__reading/1` (§4.2): the path
        // `Reading` still matches, the predicate differs, the fields are untouched — a rename,
        // not a silent fork.
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut qualified = reading("v2");
        qualified.predicate = name("v2__reading");
        let new = thermal("v2", vec![qualified], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let [reading] = comparison.packages()[0].sorts() else {
            panic!("one matched sort: {comparison:?}")
        };
        assert_eq!(reading.kind(), ChangeKind::SortRenamed);
        assert_eq!(
            reading.old_sort().map(|sort| sort.predicate().as_str()),
            Some("reading")
        );
        assert_eq!(
            reading.new_sort().map(|sort| sort.predicate().as_str()),
            Some("v2__reading")
        );
        assert_eq!(
            field_kinds(reading),
            [(1, ChangeKind::Unchanged), (2, ChangeKind::Unchanged)]
        );
        let changes = comparison.changes();
        let [change] = changes.as_slice() else {
            panic!("one change: {changes:?}")
        };
        assert_eq!(
            (change.kind(), change.number()),
            (ChangeKind::SortRenamed, None)
        );
        assert_eq!(change.old_path(), Some("thermal.v1.Reading"));
        assert_eq!(change.new_path(), Some("thermal.v2.Reading"));
        assert_eq!(change.old_signature(), Some("reading/1"));
        assert_eq!(change.new_signature(), Some("v2__reading/1"));
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn nested_sorts_match_by_their_relative_path() {
        let inner =
            |v: &str, predicate: &str| sort(&format!("thermal.{v}.Outer.Inner"), predicate, vec![]);
        let old = thermal("v1", vec![inner("v1", "inner")], vec![]);
        let new = thermal("v2", vec![inner("v2", "outer__inner")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let [inner] = comparison.packages()[0].sorts() else {
            panic!("one matched sort: {comparison:?}")
        };
        assert_eq!(inner.name(), "Outer.Inner");
        assert_eq!(inner.kind(), ChangeKind::SortRenamed);
    }

    #[test]
    fn an_enum_whose_predicate_changes_while_its_identity_holds_is_an_enum_rename() {
        let mut qualified = level("v2", vec![value("LEVEL_LOW", 0, "low")]);
        qualified.predicate = name("v2__level");
        let old = thermal(
            "v1",
            vec![],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal("v2", vec![], vec![qualified]);
        let comparison = compare(&old, &new).expect("comparable");
        let [level] = comparison.packages()[0].enums() else {
            panic!("one matched enum: {comparison:?}")
        };
        assert_eq!(level.kind(), ChangeKind::EnumRenamed);
        assert_eq!(level.name(), "Level");
        assert_eq!(kinds(&comparison), [ChangeKind::EnumRenamed]);
        let changes = comparison.changes();
        assert_eq!(changes[0].old_path(), Some("thermal.v1.Level"));
        assert_eq!(changes[0].old_signature(), Some("level/1 (open)"));
        assert_eq!(changes[0].new_signature(), Some("v2__level/1 (open)"));
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn an_openness_flip_is_breaking() {
        let grade = |v: &str, openness| {
            enumeration(
                &format!("thermal.{v}.Grade"),
                "grade",
                openness,
                false,
                vec![value("GRADE_A", 0, "a")],
            )
        };
        let old = thermal("v1", vec![], vec![grade("v1", Openness::Closed)]);
        let new = thermal("v2", vec![], vec![grade("v2", Openness::Open)]);
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            comparison.packages()[0].enums()[0].kind(),
            ChangeKind::OpennessChanged
        );
        let changes = comparison.changes();
        assert_eq!(changes[0].old_signature(), Some("grade/1 (closed)"));
        assert_eq!(changes[0].new_signature(), Some("grade/1 (open)"));
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_preserve_flip_is_breaking() {
        let mut preserving = level("v2", vec![value("LEVEL_LOW", 0, "low")]);
        preserving.preserve = true;
        let old = thermal(
            "v1",
            vec![],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal("v2", vec![], vec![preserving]);
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            comparison.packages()[0].enums()[0].kind(),
            ChangeKind::PreserveChanged
        );
        let changes = comparison.changes();
        assert_eq!(changes[0].old_signature(), Some("level/1 (open)"));
        assert_eq!(changes[0].new_signature(), Some("level/1 (open, preserve)"));
        assert!(comparison.is_breaking());
    }

    #[test]
    fn the_enum_kind_is_the_dominant_of_its_differences() {
        // One kind per node: a breaking difference outranks a rename, and an openness flip
        // outranks a preserve flip; the rest stays legible in the row's per-side signatures.
        let old = thermal(
            "v1",
            vec![],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let mut renamed_and_preserving = level("v2", vec![value("LEVEL_LOW", 0, "low")]);
        renamed_and_preserving.predicate = name("v2__level");
        renamed_and_preserving.preserve = true;
        let new = thermal("v2", vec![], vec![renamed_and_preserving.clone()]);
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(kinds(&comparison), [ChangeKind::PreserveChanged]);
        let changes = comparison.changes();
        assert_eq!(
            changes[0].new_signature(),
            Some("v2__level/1 (open, preserve)")
        );
        let mut closed_too = renamed_and_preserving;
        closed_too.openness = Openness::Closed;
        closed_too.preserve = false;
        let new = thermal("v2", vec![], vec![closed_too]);
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(kinds(&comparison), [ChangeKind::OpennessChanged]);
    }

    #[test]
    fn an_added_enum_value_is_additive() {
        let old = thermal(
            "v1",
            vec![],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal(
            "v2",
            vec![],
            vec![level(
                "v2",
                vec![value("LEVEL_LOW", 0, "low"), value("LEVEL_HIGH", 1, "high")],
            )],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let [level] = comparison.packages()[0].enums() else {
            panic!("one matched enum: {comparison:?}")
        };
        assert_eq!(level.kind(), ChangeKind::Unchanged);
        let values: Vec<(i32, ChangeKind)> = level
            .values()
            .iter()
            .map(|value| (value.number(), value.kind()))
            .collect();
        assert_eq!(
            values,
            [(0, ChangeKind::Unchanged), (1, ChangeKind::ValueAdded)]
        );
        let changes = comparison.changes();
        let [change] = changes.as_slice() else {
            panic!("one change: {changes:?}")
        };
        assert_eq!(change.kind(), ChangeKind::ValueAdded);
        assert_eq!(change.number(), Some(1));
        assert_eq!(change.old_path(), None);
        assert_eq!(change.new_path(), Some("thermal.v2.Level.LEVEL_HIGH"));
        assert_eq!(change.new_signature(), Some("high"));
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn a_removed_enum_value_is_breaking() {
        let old = thermal(
            "v1",
            vec![],
            vec![level(
                "v1",
                vec![value("LEVEL_LOW", 0, "low"), value("LEVEL_HIGH", 1, "high")],
            )],
        );
        let new = thermal(
            "v2",
            vec![],
            vec![level("v2", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let changes = comparison.changes();
        let [change] = changes.as_slice() else {
            panic!("one change: {changes:?}")
        };
        assert_eq!(change.kind(), ChangeKind::ValueRemoved);
        assert_eq!(change.old_path(), Some("thermal.v1.Level.LEVEL_HIGH"));
        assert_eq!(change.old_signature(), Some("high"));
        assert_eq!(change.new_path(), None);
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_renamed_enum_value_is_breaking() {
        // A constant is vocabulary no rule aliases: `low` → `lo` breaks every model naming it.
        let old = thermal(
            "v1",
            vec![],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal(
            "v2",
            vec![],
            vec![level("v2", vec![value("LEVEL_LO", 0, "lo")])],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let changes = comparison.changes();
        let [change] = changes.as_slice() else {
            panic!("one change: {changes:?}")
        };
        assert_eq!(change.kind(), ChangeKind::ValueRenamed);
        assert_eq!(change.old_signature(), Some("low"));
        assert_eq!(change.new_signature(), Some("lo"));
        assert!(change.is_breaking());
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_value_whose_constant_holds_is_unchanged_whatever_its_proto_name() {
        // Protobuf lets a value's name change; the generated constant is what a model names.
        let old = thermal(
            "v1",
            vec![],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal(
            "v2",
            vec![],
            vec![level("v2", vec![value("LOW", 0, "low")])],
        );
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            comparison.packages()[0].enums()[0].values()[0].kind(),
            ChangeKind::Unchanged
        );
        assert!(comparison.changes().is_empty());
    }

    #[test]
    fn a_oneof_rename_is_not_breaking() {
        // `source { manual = 1; channel = 2; }` → `origin { … }`: the label reaches only
        // `emit.lp`'s `violates` string; the cell's member set {1, 2} is the same on both sides,
        // so nothing in the vocabulary changed.
        let gauge = |v: &str, oneof: &str| {
            sort(
                &format!("thermal.{v}.Gauge"),
                "gauge",
                vec![
                    arm(&format!("thermal.{v}.Gauge.manual"), 1, "manual", oneof),
                    arm(&format!("thermal.{v}.Gauge.channel"), 2, "channel", oneof),
                ],
            )
        };
        let old = thermal("v1", vec![gauge("v1", "source")], vec![]);
        let new = thermal("v2", vec![gauge("v2", "origin")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            field_kinds(&comparison.packages()[0].sorts()[0]),
            [(1, ChangeKind::Unchanged), (2, ChangeKind::Unchanged)]
        );
        assert!(comparison.changes().is_empty());
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn a_oneof_regrouping_is_breaking() {
        // v1: `x { a = 1; b = 2; }  y { c = 3; }`; v2: `x { a = 1; }  y { b = 2; c = 3; }` — `b`
        // moved from x to y. Its cell {1, 2} → {2, 3}: the exclusivity partition changed, so `b`
        // is a breaking change on the membership dimension alone (its form is still an arm). Its
        // old and new partners' cells changed with it, and the rule reads cells, never labels.
        let choices = |v: &str, b_oneof: &str| {
            sort(
                &format!("thermal.{v}.S"),
                "s",
                vec![
                    arm(&format!("thermal.{v}.S.a"), 1, "a", "x"),
                    arm(&format!("thermal.{v}.S.b"), 2, "b", b_oneof),
                    arm(&format!("thermal.{v}.S.c"), 3, "c", "y"),
                ],
            )
        };
        let old = thermal("v1", vec![choices("v1", "x")], vec![]);
        let new = thermal("v2", vec![choices("v2", "y")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let fields = comparison.packages()[0].sorts()[0].fields();
        let b = fields[1].aspect().expect("b regrouped");
        assert_eq!(
            b.membership(),
            Some((&BTreeSet::from([1, 2]), &BTreeSet::from([2, 3])))
        );
        assert_eq!(b.form(), None);
        assert_eq!(b.predicate(), None);
        assert_eq!(
            field_kinds(&comparison.packages()[0].sorts()[0]),
            [
                (1, ChangeKind::Changed),
                (2, ChangeKind::Changed),
                (3, ChangeKind::Changed)
            ]
        );
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_field_leaving_its_oneof_changes_form_and_membership() {
        // v1: `x { a = 1; b = 2; }`; v2: `a` is a plain optional field beside `x { b = 2; }`.
        let old = thermal(
            "v1",
            vec![sort(
                "thermal.v1.S",
                "s",
                vec![
                    arm("thermal.v1.S.a", 1, "a", "x"),
                    arm("thermal.v1.S.b", 2, "b", "x"),
                ],
            )],
            vec![],
        );
        let new = thermal(
            "v2",
            vec![sort(
                "thermal.v2.S",
                "s",
                vec![
                    field(
                        "thermal.v2.S.a",
                        1,
                        "a",
                        EmitForm::Function,
                        int32(),
                        Totality::Partial,
                    ),
                    arm("thermal.v2.S.b", 2, "b", "x"),
                ],
            )],
            vec![],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let fields = comparison.packages()[0].sorts()[0].fields();
        let a = fields[0].aspect().expect("a left its oneof");
        assert_eq!(
            a.form(),
            Some((
                &EmitForm::OneofArm {
                    oneof: "x".to_owned()
                },
                &EmitForm::Function
            ))
        );
        assert_eq!(
            a.membership(),
            Some((&BTreeSet::from([1, 2]), &BTreeSet::new()))
        );
        assert_eq!(a.presence(), None);
        assert_eq!(a.arity(), None);
        let b = fields[1].aspect().expect("b lost its partner");
        assert_eq!(b.form(), None);
        assert_eq!(
            b.membership(),
            Some((&BTreeSet::from([1, 2]), &BTreeSet::from([2])))
        );
    }

    #[test]
    fn unchanged_sorts_and_enums_are_present_in_the_comparison() {
        let old = thermal(
            "v1",
            vec![reading("v1")],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal(
            "v2",
            vec![reading("v2")],
            vec![level("v2", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let package = &comparison.packages()[0];
        let [reading] = package.sorts() else {
            panic!("the unchanged sort is present: {package:?}")
        };
        assert_eq!(reading.kind(), ChangeKind::Unchanged);
        assert!(reading.old_sort().is_some() && reading.new_sort().is_some());
        assert_eq!(
            field_kinds(reading),
            [(1, ChangeKind::Unchanged), (2, ChangeKind::Unchanged)]
        );
        assert!(
            reading
                .fields()
                .iter()
                .all(|field| field.old_field().is_some() && field.new_field().is_some())
        );
        let [level] = package.enums() else {
            panic!("the unchanged enum is present: {package:?}")
        };
        assert_eq!(level.kind(), ChangeKind::Unchanged);
        assert!(level.values().iter().all(|value| {
            value.kind() == ChangeKind::Unchanged
                && value.old_value().is_some()
                && value.new_value().is_some()
        }));
        assert!(comparison.changes().is_empty());
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn changes_are_sorted_by_package_then_element_then_number_then_kind() {
        // Two packages, `beta` removed and `alpha` matched; within `alpha`, the enum `Mode` (a
        // value added) sorts before the sort `Zed` (renamed, a field added), and an element's own
        // row precedes its numbered children.
        let old = mapping(vec![
            unit(
                "alpha.v1",
                vec![sort(
                    "alpha.v1.Zed",
                    "zed",
                    vec![scalar("alpha.v1.Zed.n", 1, "n")],
                )],
                vec![enumeration(
                    "alpha.v1.Mode",
                    "mode",
                    Openness::Open,
                    false,
                    vec![value("MODE_A", 0, "a")],
                )],
            ),
            unit(
                "beta.v1",
                vec![sort("beta.v1.Gone", "gone", vec![])],
                vec![],
            ),
        ]);
        let new = mapping(vec![unit(
            "alpha.v2",
            vec![sort(
                "alpha.v2.Zed",
                "v2__zed",
                vec![
                    scalar("alpha.v2.Zed.n", 1, "n"),
                    scalar("alpha.v2.Zed.m", 5, "m"),
                ],
            )],
            vec![enumeration(
                "alpha.v2.Mode",
                "mode",
                Openness::Open,
                false,
                vec![value("MODE_A", 0, "a"), value("MODE_B", 1, "b")],
            )],
        )]);
        let comparison = compare(&old, &new).expect("comparable");
        let rows: Vec<(&str, Option<i32>, ChangeKind)> = comparison
            .changes()
            .iter()
            .map(|change| (change.package(), change.number(), change.kind()))
            .collect();
        assert_eq!(
            rows,
            [
                ("alpha", Some(1), ChangeKind::ValueAdded),
                ("alpha", None, ChangeKind::SortRenamed),
                ("alpha", Some(5), ChangeKind::Added),
                ("beta", None, ChangeKind::PackageRemoved),
                ("beta", None, ChangeKind::MessageRemoved),
            ]
        );
    }

    #[test]
    fn a_field_signature_names_its_form_in_the_manifests_vocabulary() {
        // `predicate/arity`, the declared type (the manifest's `<declared>` column), then the
        // manifest's descriptor column: a family names its shape, a singular field or arm its
        // totality — one vocabulary for the manifest and the comparison.
        let batch = sort(
            "thermal.v1.Batch",
            "batch",
            vec![
                field(
                    "thermal.v1.Batch.readings",
                    1,
                    "readings",
                    EmitForm::Sequence,
                    message("reading"),
                    Totality::Total,
                ),
                field(
                    "thermal.v1.Batch.counts",
                    2,
                    "counts",
                    EmitForm::Map {
                        key: MapKey::String,
                        key_treatment: ScalarTreatment::Text,
                    },
                    int32(),
                    Totality::Total,
                ),
                field(
                    "thermal.v1.Batch.tags",
                    3,
                    "tags",
                    EmitForm::Set,
                    text(),
                    Totality::Total,
                ),
                field(
                    "thermal.v1.Batch.id",
                    4,
                    "id",
                    EmitForm::Function,
                    text(),
                    Totality::Required,
                ),
                arm("thermal.v1.Batch.note", 5, "note", "x"),
            ],
        );
        let old = thermal("v1", vec![batch], vec![]);
        let new = thermal(
            "v2",
            vec![sort("thermal.v2.Batch", "batch", vec![])],
            vec![],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let changes = comparison.changes();
        let signatures: Vec<Option<&str>> = changes.iter().map(Change::old_signature).collect();
        assert_eq!(
            signatures,
            [
                Some("readings/3 reading seq"),
                Some("counts/3 int32 map<string>"),
                Some("tags/2 string set"),
                Some("id/2 string required"),
                Some("note/2 int32 partial"),
            ]
        );
    }

    #[test]
    fn the_breaking_line_follows_the_detection_rules() {
        for kind in [
            ChangeKind::Removed,
            ChangeKind::Changed,
            ChangeKind::MessageRemoved,
            ChangeKind::EnumRemoved,
            ChangeKind::ValueRemoved,
            ChangeKind::ValueRenamed,
            ChangeKind::OpennessChanged,
            ChangeKind::PreserveChanged,
            ChangeKind::PackageRemoved,
        ] {
            assert!(kind.is_breaking(), "{kind:?} breaks a model");
        }
        for kind in [
            ChangeKind::Unchanged,
            ChangeKind::Renamed,
            ChangeKind::Added,
            ChangeKind::SortRenamed,
            ChangeKind::EnumRenamed,
            ChangeKind::MessageAdded,
            ChangeKind::EnumAdded,
            ChangeKind::ValueAdded,
            ChangeKind::PackageAdded,
        ] {
            assert!(!kind.is_breaking(), "{kind:?} bridges or adds");
        }
    }

    #[test]
    fn every_change_kind_is_pinned_by_a_test() {
        // The checklist — each kind and the test that pins it. The match is exhaustive with no
        // wildcard, so a kind added without a line here fails to compile.
        //
        //   Renamed          a_version_bump_matches_fields_by_number_rather_than_removing_and_adding
        //   Removed          a_removed_field_is_breaking
        //   Added            an_added_field_is_additive
        //   Changed          a_rename_that_also_changes_shape_is_a_change_with_no_bridge
        //   SortRenamed      a_sort_whose_predicate_changes_while_its_identity_holds_is_a_sort_rename
        //   EnumRenamed      an_enum_whose_predicate_changes_while_its_identity_holds_is_an_enum_rename
        //   MessageAdded     a_message_added_within_a_matched_package_is_additive
        //   MessageRemoved   a_message_removed_within_a_matched_package_is_breaking
        //   EnumAdded        an_enum_added_within_a_matched_package_is_additive
        //   EnumRemoved      an_enum_removed_within_a_matched_package_is_breaking
        //   ValueAdded       an_added_enum_value_is_additive
        //   ValueRemoved     a_removed_enum_value_is_breaking
        //   ValueRenamed     a_renamed_enum_value_is_breaking
        //   OpennessChanged  an_openness_flip_is_breaking
        //   PreserveChanged  a_preserve_flip_is_breaking
        //   PackageAdded     partial_overlap_carries_an_added_package_with_its_elements
        //   PackageRemoved   partial_overlap_carries_a_removed_package_with_its_elements
        //
        // and the baseline outside the seventeen:
        //
        //   Unchanged        unchanged_sorts_and_enums_are_present_in_the_comparison
        //
        // These tests pin all seventeen; the worked evolution example (`examples/evolution`)
        // demonstrates twelve of them end to end, and the CLI suite's report goldens the other five.
        match ChangeKind::Unchanged {
            ChangeKind::Unchanged
            | ChangeKind::Renamed
            | ChangeKind::Removed
            | ChangeKind::Added
            | ChangeKind::Changed
            | ChangeKind::SortRenamed
            | ChangeKind::EnumRenamed
            | ChangeKind::MessageAdded
            | ChangeKind::MessageRemoved
            | ChangeKind::EnumAdded
            | ChangeKind::EnumRemoved
            | ChangeKind::ValueAdded
            | ChangeKind::ValueRemoved
            | ChangeKind::ValueRenamed
            | ChangeKind::OpennessChanged
            | ChangeKind::PreserveChanged
            | ChangeKind::PackageAdded
            | ChangeKind::PackageRemoved => {}
        }
    }

    fn enum_value(referent: &str, preserve: bool) -> ValueMapping {
        ValueMapping::Enum {
            referent: name(referent),
            preserve,
        }
    }

    /// A `Batch` sort at version `v` whose `readings` #1 is a sequence of the `referent` sort.
    fn batch_of(v: &str, referent: &str) -> SortMapping {
        sort(
            &format!("thermal.{v}.Batch"),
            "batch",
            vec![field(
                &format!("thermal.{v}.Batch.readings"),
                1,
                "readings",
                EmitForm::Sequence,
                message(referent),
                Totality::Total,
            )],
        )
    }

    /// A `Panel` sort at version `v` whose `level` #1 is a value of the `referent` enum.
    fn panel(v: &str, referent: &str, preserve: bool) -> SortMapping {
        sort(
            &format!("thermal.{v}.Panel"),
            "panel",
            vec![field(
                &format!("thermal.{v}.Panel.level"),
                1,
                "level",
                EmitForm::Function,
                enum_value(referent, preserve),
                Totality::Total,
            )],
        )
    }

    #[test]
    fn a_field_referencing_a_renamed_sort_is_unchanged() {
        // `Reading` is re-qualified `reading` → `v2__reading` in v2 (a `SortRenamed`, bridged at
        // arity 1). `Batch.readings` names it by predicate on each side, but its *type* — the
        // sort's identity — is the same sort, so the field is unchanged and nothing breaks: the
        // rename is the sort's own row alone.
        let mut qualified = reading("v2");
        qualified.predicate = name("v2__reading");
        let old = thermal("v1", vec![batch_of("v1", "reading"), reading("v1")], vec![]);
        let new = thermal("v2", vec![batch_of("v2", "v2__reading"), qualified], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let [batch, reading] = comparison.packages()[0].sorts() else {
            panic!("two sorts: {comparison:?}")
        };
        assert_eq!(reading.kind(), ChangeKind::SortRenamed);
        assert_eq!(field_kinds(batch), [(1, ChangeKind::Unchanged)]);
        assert_eq!(kinds(&comparison), [ChangeKind::SortRenamed]);
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn a_field_referencing_a_renamed_enum_is_unchanged() {
        let mut qualified = level("v2", vec![value("LEVEL_LOW", 0, "low")]);
        qualified.predicate = name("v2__level");
        let old = thermal(
            "v1",
            vec![panel("v1", "level", false)],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal("v2", vec![panel("v2", "v2__level", false)], vec![qualified]);
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            field_kinds(&comparison.packages()[0].sorts()[0]),
            [(1, ChangeKind::Unchanged)]
        );
        assert_eq!(kinds(&comparison), [ChangeKind::EnumRenamed]);
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn a_field_referencing_a_preserve_changed_enum_is_unchanged() {
        // The flip is the enum's own row (`PreserveChanged`, breaking); the `preserve` a field
        // carries is that enum's, denormalized onto it, not a change of the field.
        let mut preserving = level("v2", vec![value("LEVEL_LOW", 0, "low")]);
        preserving.preserve = true;
        let old = thermal(
            "v1",
            vec![panel("v1", "level", false)],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal("v2", vec![panel("v2", "level", true)], vec![preserving]);
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            field_kinds(&comparison.packages()[0].sorts()[0]),
            [(1, ChangeKind::Unchanged)]
        );
        assert_eq!(kinds(&comparison), [ChangeKind::PreserveChanged]);
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_field_retargeted_to_another_sort_is_changed() {
        // `Batch.readings` names `reading` in v1 and `alert` in v2 — both sorts on both sides,
        // two identities — so the field's type changed: breaking, on the value dimension.
        let alert = |v: &str| sort(&format!("thermal.{v}.Alert"), "alert", vec![]);
        let old = thermal(
            "v1",
            vec![alert("v1"), batch_of("v1", "reading"), reading("v1")],
            vec![],
        );
        let new = thermal(
            "v2",
            vec![alert("v2"), batch_of("v2", "alert"), reading("v2")],
            vec![],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let [_, batch, _] = comparison.packages()[0].sorts() else {
            panic!("three sorts: {comparison:?}")
        };
        assert_eq!(batch.fields()[0].kind(), ChangeKind::Changed);
        let aspect = batch.fields()[0].aspect().expect("retargeted");
        assert_eq!(
            aspect.value(),
            Some((&message("reading"), &message("alert")))
        );
        assert_eq!(kinds(&comparison), [ChangeKind::Changed]);
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_field_whose_referent_resolves_on_one_side_only_is_changed() {
        // `Batch.readings` still says `reading` in v2, but no subject sort answers to it there
        // (`Reading` was removed, its two fields riding as removals): one side resolves the
        // referent, the other does not, so the field's type changed beside the removal.
        let old = thermal("v1", vec![batch_of("v1", "reading"), reading("v1")], vec![]);
        let new = thermal("v2", vec![batch_of("v2", "reading")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let [batch, _] = comparison.packages()[0].sorts() else {
            panic!("two sorts: {comparison:?}")
        };
        assert_eq!(field_kinds(batch), [(1, ChangeKind::Changed)]);
        assert_eq!(
            kinds(&comparison),
            [
                ChangeKind::Changed,
                ChangeKind::MessageRemoved,
                ChangeKind::Removed,
                ChangeKind::Removed
            ]
        );
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_field_whose_closure_referent_is_requalified_is_changed() {
        // `Reading.at` names the closure sort `google.protobuf.Timestamp`, whose predicate a new
        // subject collision re-qualifies `timestamp` → `v2__timestamp`. A closure referent has no
        // identity in the comparison (it is not diffed, so nothing bridges it): the predicates
        // are compared as spelled, and the field is a breaking change.
        let at = |v: &str, referent: &str| {
            sort(
                &format!("thermal.{v}.Reading"),
                "reading",
                vec![field(
                    &format!("thermal.{v}.Reading.at"),
                    1,
                    "at",
                    EmitForm::Function,
                    message(referent),
                    Totality::Partial,
                )],
            )
        };
        let mut requalified = timestamp("seconds");
        requalified.predicate = name("v2__timestamp");
        let old = mapping(vec![
            unit("google.protobuf", vec![timestamp("seconds")], vec![]),
            unit("thermal.v1", vec![at("v1", "timestamp")], vec![]),
        ]);
        let new = mapping(vec![
            unit("google.protobuf", vec![requalified], vec![]),
            unit("thermal.v2", vec![at("v2", "v2__timestamp")], vec![]),
        ]);
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            field_kinds(&comparison.packages()[0].sorts()[0]),
            [(1, ChangeKind::Changed)]
        );
        assert!(comparison.is_breaking());
    }

    #[test]
    fn an_arm_added_to_a_oneof_leaves_the_existing_arms_unchanged() {
        // v1: `x { a = 1; b = 2; }`; v2: `x { a = 1; b = 2; c = 3; }` — the additive evolution
        // protobuf allows. Among the fields both sides hold, a's and b's cells are {1, 2} on
        // each side, so `c` is the one row and nothing breaks.
        let old = thermal(
            "v1",
            vec![sort(
                "thermal.v1.S",
                "s",
                vec![
                    arm("thermal.v1.S.a", 1, "a", "x"),
                    arm("thermal.v1.S.b", 2, "b", "x"),
                ],
            )],
            vec![],
        );
        let new = thermal(
            "v2",
            vec![sort(
                "thermal.v2.S",
                "s",
                vec![
                    arm("thermal.v2.S.a", 1, "a", "x"),
                    arm("thermal.v2.S.b", 2, "b", "x"),
                    arm("thermal.v2.S.c", 3, "c", "x"),
                ],
            )],
            vec![],
        );
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            field_kinds(&comparison.packages()[0].sorts()[0]),
            [
                (1, ChangeKind::Unchanged),
                (2, ChangeKind::Unchanged),
                (3, ChangeKind::Added)
            ]
        );
        assert_eq!(kinds(&comparison), [ChangeKind::Added]);
        assert!(!comparison.is_breaking());
    }

    #[test]
    fn an_arm_removed_from_a_oneof_leaves_its_surviving_partners_unchanged() {
        // v1: `x { a = 1; b = 2; c = 3; }`; v2: `x { a = 1; b = 2; }` — the removal is the
        // breaking row; a and b, compared among the fields both sides hold, are unchanged.
        let old = thermal(
            "v1",
            vec![sort(
                "thermal.v1.S",
                "s",
                vec![
                    arm("thermal.v1.S.a", 1, "a", "x"),
                    arm("thermal.v1.S.b", 2, "b", "x"),
                    arm("thermal.v1.S.c", 3, "c", "x"),
                ],
            )],
            vec![],
        );
        let new = thermal(
            "v2",
            vec![sort(
                "thermal.v2.S",
                "s",
                vec![
                    arm("thermal.v2.S.a", 1, "a", "x"),
                    arm("thermal.v2.S.b", 2, "b", "x"),
                ],
            )],
            vec![],
        );
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(
            field_kinds(&comparison.packages()[0].sorts()[0]),
            [
                (1, ChangeKind::Unchanged),
                (2, ChangeKind::Unchanged),
                (3, ChangeKind::Removed)
            ]
        );
        assert_eq!(kinds(&comparison), [ChangeKind::Removed]);
        assert!(comparison.is_breaking());
    }

    #[test]
    fn a_scalar_field_rename_bridges_the_base_predicate_at_its_arity() {
        // `temp_c` → `celsius` at #2 with the shape intact: the bridge is one rule aliasing the old
        // base predicate to the new over fresh positional variables — `old :- new.`, a v1 model
        // reading v2 facts — its provenance the `%!` line, and the row carries the same text.
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut renamed = reading("v2");
        renamed.fields[1] = scalar("thermal.v2.Reading.celsius", 2, "celsius");
        let new = thermal("v2", vec![renamed], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let celsius = &comparison.packages()[0].sorts()[0].fields()[1];
        assert_eq!(celsius.kind(), ChangeKind::Renamed);
        let bridge = celsius.bridge().expect("a pure rename is bridged");
        assert_eq!(
            bridge,
            "%! temp_c/2 reads celsius/2  (inbound-facing bridge: thermal.v1.Reading.temp_c renamed thermal.v2.Reading.celsius)\ntemp_c(A, B) :- celsius(A, B).\n"
        );
        let (head, body) = parse_bridge(&bridge);
        assert_eq!(head.name.as_str(), "temp_c");
        assert_eq!(body.name.as_str(), "celsius");
        assert_eq!(
            head.arguments,
            Arguments::Single(vec![build::var("A"), build::var("B")])
        );
        assert_eq!(body.arguments, head.arguments);
        assert_eq!(comparison.changes()[0].bridge(), Some(bridge));
    }

    #[test]
    fn a_message_sequence_field_rename_bridges_the_view_predicate_at_the_view_arity() {
        // `Batch.readings` (a sequence of `Reading`) → `samples`: the bridge aliases the *view*
        // predicate `readings/3` at the view arity — the projection idiom `readings(B, _, E)` a
        // model reads — and its `%!` line says so; the occupant-term functor `readings(B, I)`
        // inside the child sort atom is renamed too, and no rule aliases a functor.
        let old = thermal("v1", vec![batch_of("v1", "reading"), reading("v1")], vec![]);
        let mut renamed = batch_of("v2", "reading");
        renamed.fields[0] = field(
            "thermal.v2.Batch.samples",
            1,
            "samples",
            EmitForm::Sequence,
            message("reading"),
            Totality::Total,
        );
        let new = thermal("v2", vec![renamed, reading("v2")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let readings = &comparison.packages()[0].sorts()[0].fields()[0];
        assert_eq!(readings.kind(), ChangeKind::Renamed);
        assert!(
            readings
                .old_field()
                .is_some_and(|field| field.view().is_some()),
            "a message sequence field has a view"
        );
        let bridge = readings.bridge().expect("a pure rename is bridged");
        assert_eq!(
            bridge,
            "%! readings/3 reads samples/3  (inbound-facing bridge of the view: thermal.v1.Batch.readings renamed thermal.v2.Batch.samples)\nreadings(A, B, C) :- samples(A, B, C).\n"
        );
        let (head, body) = parse_bridge(&bridge);
        assert_eq!(head.name.as_str(), "readings");
        assert_eq!(body.name.as_str(), "samples");
        assert_eq!(
            head.arguments,
            Arguments::Single(vec![build::var("A"), build::var("B"), build::var("C")])
        );
        assert_eq!(body.arguments, head.arguments);
    }

    #[test]
    fn a_sort_rename_bridges_at_arity_one() {
        // `reading/1` re-qualified to `v2__reading/1` (§4.2): the bridge aliases the sort predicate
        // at arity 1, so a v1 model's `reading(E)` ranges over the v2 occupants.
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut qualified = reading("v2");
        qualified.predicate = name("v2__reading");
        let new = thermal("v2", vec![qualified], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let reading = &comparison.packages()[0].sorts()[0];
        assert_eq!(reading.kind(), ChangeKind::SortRenamed);
        let bridge = reading.bridge().expect("a sort rename is bridged");
        assert_eq!(
            bridge,
            "%! reading/1 reads v2__reading/1  (inbound-facing bridge: thermal.v1.Reading renamed thermal.v2.Reading)\nreading(A) :- v2__reading(A).\n"
        );
        let (head, body) = parse_bridge(&bridge);
        assert_eq!(head.name.as_str(), "reading");
        assert_eq!(body.name.as_str(), "v2__reading");
        assert_eq!(head.arguments, Arguments::Single(vec![build::var("A")]));
        assert_eq!(body.arguments, head.arguments);
        assert_eq!(comparison.changes()[0].bridge(), Some(bridge));
    }

    #[test]
    fn an_enum_rename_bridges_at_arity_one() {
        // `level/1` → `v2__level/1` with its openness and preserve intact: the bridge aliases the
        // value sort a model quantifies over (§7.4) at arity 1.
        let mut qualified = level("v2", vec![value("LEVEL_LOW", 0, "low")]);
        qualified.predicate = name("v2__level");
        let old = thermal(
            "v1",
            vec![],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal("v2", vec![], vec![qualified]);
        let comparison = compare(&old, &new).expect("comparable");
        let level = &comparison.packages()[0].enums()[0];
        assert_eq!(level.kind(), ChangeKind::EnumRenamed);
        let bridge = level.bridge().expect("an enum rename is bridged");
        assert_eq!(
            bridge,
            "%! level/1 reads v2__level/1  (inbound-facing bridge: thermal.v1.Level renamed thermal.v2.Level)\nlevel(A) :- v2__level(A).\n"
        );
        let (head, body) = parse_bridge(&bridge);
        assert_eq!(head.name.as_str(), "level");
        assert_eq!(body.name.as_str(), "v2__level");
        assert_eq!(head.arguments, Arguments::Single(vec![build::var("A")]));
        assert_eq!(body.arguments, head.arguments);
        assert_eq!(comparison.changes()[0].bridge(), Some(bridge));
    }

    #[test]
    fn only_a_pure_rename_carries_a_bridge() {
        // A bridge exists for a `Renamed`, `SortRenamed`, or `EnumRenamed` node and for no other:
        // not for a rename that also changed shape (pinned with its aspect in
        // `a_rename_that_also_changes_shape_is_a_change_with_no_bridge`), not for an enum renamed
        // beside a flip (which is the flip), and not for a one-sided or an unchanged node.
        let old = thermal(
            "v1",
            vec![
                sort(
                    "thermal.v1.Alert",
                    "alert",
                    vec![scalar("thermal.v1.Alert.n", 1, "n")],
                ),
                reading("v1"),
            ],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let mut requalified = reading("v2");
        requalified.predicate = name("v2__reading");
        requalified.fields[1] = scalar("thermal.v2.Reading.celsius", 2, "celsius");
        requalified
            .fields
            .push(scalar("thermal.v2.Reading.humidity", 3, "humidity"));
        let mut renamed_and_preserving = level("v2", vec![value("LEVEL_LOW", 0, "low")]);
        renamed_and_preserving.predicate = name("v2__level");
        renamed_and_preserving.preserve = true;
        let new = thermal("v2", vec![requalified], vec![renamed_and_preserving]);
        let comparison = compare(&old, &new).expect("comparable");
        let [alert, reading] = comparison.packages()[0].sorts() else {
            panic!("two sorts: {comparison:?}")
        };
        assert_eq!(alert.kind(), ChangeKind::MessageRemoved);
        assert_eq!(alert.bridge(), None);
        assert_eq!(alert.fields()[0].bridge(), None);
        assert_eq!(reading.kind(), ChangeKind::SortRenamed);
        assert!(reading.bridge().is_some());
        let [sensor, celsius, humidity] = reading.fields() else {
            panic!("three fields: {reading:?}")
        };
        assert_eq!(
            (sensor.kind(), sensor.bridge()),
            (ChangeKind::Unchanged, None)
        );
        assert_eq!(celsius.kind(), ChangeKind::Renamed);
        assert!(celsius.bridge().is_some());
        assert_eq!(
            (humidity.kind(), humidity.bridge()),
            (ChangeKind::Added, None)
        );
        let level = &comparison.packages()[0].enums()[0];
        assert_eq!(
            (level.kind(), level.bridge()),
            (ChangeKind::PreserveChanged, None)
        );
        let bridged: Vec<ChangeKind> = comparison
            .changes()
            .iter()
            .filter(|change| change.bridge().is_some())
            .map(Change::kind)
            .collect();
        assert_eq!(bridged, [ChangeKind::SortRenamed, ChangeKind::Renamed]);
    }

    #[test]
    fn a_bridge_is_the_rendering_of_one_constructed_rule() {
        // The bridge is byte for byte the rule built through `emit::build` and rendered through
        // `emit::render` — one spelling, the renderer's — so no format string stands between the
        // constructed rule and the text: the one `:-` in it is the rendered rule's, and the text
        // is one documented statement, its `%!` line above its one rule line.
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut renamed = reading("v2");
        renamed.fields[1] = scalar("thermal.v2.Reading.celsius", 2, "celsius");
        let new = thermal("v2", vec![renamed], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let bridge = comparison.packages()[0].sorts()[0].fields()[1]
            .bridge()
            .expect("a pure rename is bridged");
        let constructed = render(vec![build::rule(
            build::atom(name("temp_c"), [build::var("A"), build::var("B")]),
            build::positive(build::atom(
                name("celsius"),
                [build::var("A"), build::var("B")],
            )),
            "temp_c/2 reads celsius/2  (inbound-facing bridge: thermal.v1.Reading.temp_c renamed thermal.v2.Reading.celsius)".to_owned(),
        )])
        .expect("renders");
        assert_eq!(bridge, constructed);
        assert_eq!(bridge.matches(":-").count(), 1);
        assert_eq!(bridge.matches('\n').count(), 2);
        assert!(bridge.starts_with("%! ") && bridge.ends_with(".\n"));
    }

    #[test]
    fn an_enum_field_and_a_set_field_rename_bridge_their_base_predicates() {
        // The base-arity clause beyond a scalar: an enum-typed field, and a `(keryx.set)` field —
        // a message set here, which has no view (§13.2) — each bridge the base predicate at arity
        // 2, and neither line says `of the view`.
        let panel = |v: &str, predicate: &str| {
            sort(
                &format!("thermal.{v}.Panel"),
                "panel",
                vec![field(
                    &format!("thermal.{v}.Panel.{predicate}"),
                    1,
                    predicate,
                    EmitForm::Function,
                    enum_value("level", false),
                    Totality::Total,
                )],
            )
        };
        let batch = |v: &str, predicate: &str| {
            sort(
                &format!("thermal.{v}.Batch"),
                "batch",
                vec![field(
                    &format!("thermal.{v}.Batch.{predicate}"),
                    1,
                    predicate,
                    EmitForm::Set,
                    message("reading"),
                    Totality::Total,
                )],
            )
        };
        let old = thermal(
            "v1",
            vec![batch("v1", "readings"), panel("v1", "level"), reading("v1")],
            vec![level("v1", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let new = thermal(
            "v2",
            vec![batch("v2", "members"), panel("v2", "grade"), reading("v2")],
            vec![level("v2", vec![value("LEVEL_LOW", 0, "low")])],
        );
        let comparison = compare(&old, &new).expect("comparable");
        let [batch, panel, _] = comparison.packages()[0].sorts() else {
            panic!("three sorts: {comparison:?}")
        };
        let members = &batch.fields()[0];
        assert_eq!(members.kind(), ChangeKind::Renamed);
        assert!(
            members
                .old_field()
                .is_some_and(|field| field.view().is_none()),
            "a message set has no view"
        );
        assert_eq!(
            members.bridge().as_deref(),
            Some(
                "%! readings/2 reads members/2  (inbound-facing bridge: thermal.v1.Batch.readings renamed thermal.v2.Batch.members)\nreadings(A, B) :- members(A, B).\n"
            )
        );
        let grade = &panel.fields()[0];
        assert_eq!(grade.kind(), ChangeKind::Renamed);
        assert_eq!(
            grade.bridge().as_deref(),
            Some(
                "%! level/2 reads grade/2  (inbound-facing bridge: thermal.v1.Panel.level renamed thermal.v2.Panel.grade)\nlevel(A, B) :- grade(A, B).\n"
            )
        );
        for bridge in [members.bridge(), grade.bridge()] {
            let (head, body) = parse_bridge(&bridge.expect("a pure rename is bridged"));
            assert_eq!(
                head.arguments,
                Arguments::Single(vec![build::var("A"), build::var("B")])
            );
            assert_eq!(body.arguments, head.arguments);
        }
    }

    #[test]
    fn every_kind_of_change_has_its_slug_and_the_baseline_has_none() {
        // The seventeen, each the variant's name in `snake_case` and no two alike; `Unchanged` is
        // no change and never a row, so it names no record.
        let table = [
            (ChangeKind::Renamed, "renamed"),
            (ChangeKind::Removed, "removed"),
            (ChangeKind::Added, "added"),
            (ChangeKind::Changed, "changed"),
            (ChangeKind::SortRenamed, "sort_renamed"),
            (ChangeKind::EnumRenamed, "enum_renamed"),
            (ChangeKind::MessageAdded, "message_added"),
            (ChangeKind::MessageRemoved, "message_removed"),
            (ChangeKind::EnumAdded, "enum_added"),
            (ChangeKind::EnumRemoved, "enum_removed"),
            (ChangeKind::ValueAdded, "value_added"),
            (ChangeKind::ValueRemoved, "value_removed"),
            (ChangeKind::ValueRenamed, "value_renamed"),
            (ChangeKind::OpennessChanged, "openness_changed"),
            (ChangeKind::PreserveChanged, "preserve_changed"),
            (ChangeKind::PackageAdded, "package_added"),
            (ChangeKind::PackageRemoved, "package_removed"),
        ];
        for (kind, slug) in table {
            assert_eq!(kind.slug(), Some(slug), "{kind:?}");
        }
        let distinct: BTreeSet<&str> = table.iter().map(|(_, slug)| *slug).collect();
        assert_eq!(distinct.len(), 17);
        assert_eq!(ChangeKind::Unchanged.slug(), None);
    }

    #[test]
    fn a_comparison_with_no_change_serializes_to_the_empty_array() {
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let new = thermal("v2", vec![reading("v2")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        assert_eq!(comparison.to_json(), "[]");
    }

    #[test]
    fn a_record_spells_its_keys_in_sorted_order_and_its_bridge_verbatim() {
        // One rename: the record's keys serialize in `serde_json`'s sorted order — one fixed
        // spelling whatever order they were put in — and the bridge text round-trips through
        // JSON's escaping of its newlines, so a consumer reads the rule as the renderer wrote it.
        let old = thermal("v1", vec![reading("v1")], vec![]);
        let mut renamed = reading("v2");
        renamed.fields[1] = scalar("thermal.v2.Reading.celsius", 2, "celsius");
        let new = thermal("v2", vec![renamed], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let bridge = comparison.changes()[0].bridge().expect("bridged");
        assert!(bridge.contains('\n'));
        let text = comparison.to_json();
        assert_eq!(
            text,
            format!(
                "[{{\"breaking\":false,\"bridge\":{},\"kind\":\"renamed\",\"new\":\"celsius/2 int32 total\",\"new_path\":\"thermal.v2.Reading.celsius\",\"number\":2,\"old\":\"temp_c/2 int32 total\",\"old_path\":\"thermal.v1.Reading.temp_c\",\"package\":\"thermal\"}}]",
                serde_json::to_string(&bridge).expect("a string serializes")
            )
        );
        let parsed: Value = serde_json::from_str(&text).expect("the changeset is JSON");
        assert_eq!(parsed[0]["bridge"], Value::from(bridge));
    }

    #[test]
    fn a_package_row_serializes_without_a_number_or_a_signature() {
        // `legacy.v1` is on the old side alone: its row is a `package_removed` record whose old
        // path is the package's dotted name, with no new side and no signature on either side (a
        // package has none) — the absent sides null — and neither a number nor a bridge — the
        // absent keys absent. Its sort rides beneath it as a `message_removed` record.
        let old = mapping(vec![
            unit(
                "legacy.v1",
                vec![sort("legacy.v1.Old", "old", vec![])],
                vec![],
            ),
            unit("thermal.v1", vec![reading("v1")], vec![]),
        ]);
        let new = thermal("v2", vec![reading("v2")], vec![]);
        let comparison = compare(&old, &new).expect("comparable");
        let json: Value = serde_json::from_str(&comparison.to_json()).expect("JSON");
        assert_eq!(json.as_array().map(Vec::len), Some(2));
        let package = &json[0];
        assert_eq!(package["kind"], "package_removed");
        assert_eq!(package["package"], "legacy");
        assert_eq!(package["old_path"], "legacy.v1");
        assert_eq!(package["new_path"], Value::Null);
        assert_eq!(package["old"], Value::Null);
        assert_eq!(package["new"], Value::Null);
        assert_eq!(package["breaking"], Value::Bool(true));
        assert!(package.get("number").is_none(), "{package}");
        assert!(package.get("bridge").is_none(), "{package}");
        let removed = &json[1];
        assert_eq!(removed["kind"], "message_removed");
        assert_eq!(removed["old_path"], "legacy.v1.Old");
        assert_eq!(removed["old"], "old/1");
        assert!(removed.get("number").is_none(), "{removed}");
    }
}
