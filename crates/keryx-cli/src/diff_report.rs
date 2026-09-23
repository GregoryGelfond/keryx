//! The migration report — `keryx diff`'s default product (spec §13.4, §27): the comparison
//! rendered for a reader, where the JSON changeset (`Comparison::to_json`) is the product for a
//! program. Rendered from the [`Comparison`] tree — packages, then sorts and enums, then fields
//! and values, the unchanged nodes present — and not from its flat change rows, so the report
//! shows what stayed beside what changed: an unchanged message is a header marked `unchanged`,
//! not an absence.
//!
//! The layout: a banner naming the two versions (one package) or the package count, with the
//! keryx version on its right; a block per package under a double rule when the comparison spans
//! more than one; a block per message and per enum — its predicate and its version-free path
//! under a light rule — with one row per changed field or value; and the summary footer: the
//! counts by category (breaking, bridged, additive), each broken down by kind, and the hint to
//! write the bridge views. Every kind of change surfaces at its structural level as one of four
//! glyph-and-word marks — `~ renamed`, `- removed`, `+ added`, `! changed` — the word carrying
//! the meaning, so the report reads the same under `NO_COLOR`: a package's or an element's own
//! kind marks its header, a field's or a value's marks its row, the field or value number its
//! gutter. What a row says changed is read from the structured [`ChangeAspect`] — each differing
//! dimension its own old → new token beside the stable rest — never from a signature string
//! re-tokenized, so an emphasis on the token keys on the aspect alone. The vocabulary the rows
//! spell is the manifest's (spec §13.4), the words the changeset's signatures carry, pinned to
//! them by test.
//!
//! Styled through a [`Style`]: the writer paints each part by its role, and the plain style —
//! the one style here, the form for `NO_COLOR` and for a stdout that is not a terminal — paints
//! every role as the identity, so padding is measured on the text a reader sees.

use std::collections::{BTreeMap, BTreeSet};

use keryx_core::Name;
use keryx_core::descriptor::{Openness, Scalar};
use keryx_core::diff::{
    ChangeAspect, ChangeKind, Comparison, EnumDiff, FieldDiff, PackageDiff, SortDiff, ValueDiff,
};
use keryx_core::policy::{EmitForm, EnumMapping, FieldMapping, Totality, ValueMapping};

/// keryx's version, named on the banner's right — the one volatile text in a report, which a
/// golden masks.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The report's width in columns: the banner's rules span it, and a header's status is set
/// against it.
const WIDTH: usize = 70;

/// The column a row's signature starts at: the gutter (four columns, the number right-aligned),
/// a gap, the mark (nine columns at most), and three spaces.
const ROW_START: usize = 18;

/// The signature's name column: the predicate, or the predicate's old → new pair, padded to this
/// so the rest — the type and descriptor, or the differing dimensions — lines up beneath.
const NAME_WIDTH: usize = 18;

/// The signature column's width; the annotation starts a gap past it.
const SIGNATURE_WIDTH: usize = 30;

/// The least gap between two columns' texts.
const GAP: usize = 2;

/// How the report is styled: a table of terminal sequences, one per role the writer paints — the
/// dimmed context (rules, the gutter, the stable remainder of a row, a header's status), the
/// emphasized text (a header's predicate, the token that differs), and the four marks by
/// category — each closed by `reset`. The plain style — no escape sequence, the form for
/// `NO_COLOR` and for a stdout that is not a terminal — is the table with every entry empty, so
/// each hook is the identity by construction. The command's colour decision reaches the renderer
/// through [`Style::from`]; the coloured table is not filled in yet, so both decisions are plain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    dim: &'static str,
    bold: &'static str,
    renamed: &'static str,
    removed: &'static str,
    added: &'static str,
    changed: &'static str,
    reset: &'static str,
}

impl Style {
    /// The plain style: no escape sequence in the report.
    #[must_use]
    pub fn plain() -> Style {
        Style {
            dim: "",
            bold: "",
            renamed: "",
            removed: "",
            added: "",
            changed: "",
            reset: "",
        }
    }

    /// The style for the command's colour decision: coloured when `colored`, else plain. The
    /// coloured table is not filled in yet, so both are the plain style.
    #[must_use]
    pub fn from(_colored: bool) -> Style {
        Style::plain()
    }

    /// `text` between `open` and the reset.
    fn paint(&self, open: &str, text: &str) -> String {
        format!("{open}{text}{}", self.reset)
    }

    /// A rule.
    fn rule(&self, text: &str) -> String {
        self.paint(self.dim, text)
    }

    /// A row's gutter — its field or value number.
    fn gutter(&self, text: &str) -> String {
        self.paint(self.dim, text)
    }

    /// The stable context of a row or a header: what did not change.
    fn stable(&self, text: &str) -> String {
        self.paint(self.dim, text)
    }

    /// A token that differs between the sides.
    fn token(&self, text: &str) -> String {
        self.paint(self.bold, text)
    }

    /// A header's predicate.
    fn predicate(&self, text: &str) -> String {
        self.paint(self.bold, text)
    }

    /// A header's status word, and the banner's version.
    fn status(&self, text: &str) -> String {
        self.paint(self.dim, text)
    }

    /// A row's or a header's annotation — its bridge posture.
    fn annotation(&self, text: &str) -> String {
        self.paint("", text)
    }

    /// A mark — the glyph and its word — by its category.
    fn mark(&self, glyph: Glyph, text: &str) -> String {
        let open = match glyph {
            Glyph::Renamed => self.renamed,
            Glyph::Removed => self.removed,
            Glyph::Added => self.added,
            Glyph::Changed => self.changed,
        };
        self.paint(open, text)
    }
}

/// Render the report for `comparison` in `style`: the banner, a block per package when there is
/// more than one, a block per message and per enum with its rows, and the summary footer.
/// Newline-terminated, no trailing space on any line.
#[must_use]
pub fn render(comparison: &Comparison<'_>, style: Style) -> String {
    let mut report = Report {
        out: String::new(),
        style,
        tally: Tally::default(),
    };
    report.banner(comparison);
    let blocked = comparison.packages().len() > 1;
    for package in comparison.packages() {
        report.package(package, blocked);
    }
    report.footer();
    report.out
}

/// The four glyph categories every kind of change collapses to, each a glyph and a word — the
/// word carrying the meaning, the glyph a reinforcement a colour can pick up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Glyph {
    Renamed,
    Removed,
    Added,
    Changed,
}

impl Glyph {
    /// The category `kind` surfaces as, or `None` for the baseline, which marks nothing. Total
    /// over the seventeen kinds of change — the match is exhaustive, so a kind added to the model
    /// without a mark fails to compile — each at the structural level its node renders at: a
    /// package's kind on its package header, a message's or enum's on its header, a field's or a
    /// value's on its row.
    fn of(kind: ChangeKind) -> Option<Glyph> {
        match kind {
            ChangeKind::Unchanged => None,
            ChangeKind::Renamed
            | ChangeKind::SortRenamed
            | ChangeKind::EnumRenamed
            | ChangeKind::ValueRenamed => Some(Glyph::Renamed),
            ChangeKind::Removed
            | ChangeKind::MessageRemoved
            | ChangeKind::EnumRemoved
            | ChangeKind::ValueRemoved
            | ChangeKind::PackageRemoved => Some(Glyph::Removed),
            ChangeKind::Added
            | ChangeKind::MessageAdded
            | ChangeKind::EnumAdded
            | ChangeKind::ValueAdded
            | ChangeKind::PackageAdded => Some(Glyph::Added),
            ChangeKind::Changed | ChangeKind::OpennessChanged | ChangeKind::PreserveChanged => {
                Some(Glyph::Changed)
            }
        }
    }

    /// The mark: the glyph, a space, the word.
    fn mark(self) -> &'static str {
        match self {
            Glyph::Renamed => "~ renamed",
            Glyph::Removed => "- removed",
            Glyph::Added => "+ added",
            Glyph::Changed => "! changed",
        }
    }
}

/// The footer's categories: what breaks a model of the old vocabulary, what is bridged for it,
/// and what only adds to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Breaking,
    Bridged,
    Additive,
}

impl Category {
    /// The category `kind` counts under, or `None` for the baseline. Breaking is exactly
    /// [`ChangeKind::is_breaking`] — pinned by test; bridged is the three pure renames, each of
    /// which carries a bridge view; additive is every addition.
    fn of(kind: ChangeKind) -> Option<Category> {
        match kind {
            ChangeKind::Unchanged => None,
            ChangeKind::Renamed | ChangeKind::SortRenamed | ChangeKind::EnumRenamed => {
                Some(Category::Bridged)
            }
            ChangeKind::Removed
            | ChangeKind::Changed
            | ChangeKind::MessageRemoved
            | ChangeKind::EnumRemoved
            | ChangeKind::ValueRemoved
            | ChangeKind::ValueRenamed
            | ChangeKind::OpennessChanged
            | ChangeKind::PreserveChanged
            | ChangeKind::PackageRemoved => Some(Category::Breaking),
            ChangeKind::Added
            | ChangeKind::MessageAdded
            | ChangeKind::EnumAdded
            | ChangeKind::ValueAdded
            | ChangeKind::PackageAdded => Some(Category::Additive),
        }
    }

    fn word(self) -> &'static str {
        match self {
            Category::Breaking => "breaking",
            Category::Bridged => "bridged",
            Category::Additive => "additive",
        }
    }
}

/// The footer's noun for `count` changes of `kind` — a field's kinds bare (`2 removed`), the
/// structural kinds naming their level (`1 message removed`, `2 values added`).
fn noun(kind: ChangeKind, count: usize) -> &'static str {
    let (one, many) = match kind {
        ChangeKind::Unchanged => ("unchanged", "unchanged"),
        ChangeKind::Renamed => ("renamed", "renamed"),
        ChangeKind::Removed => ("removed", "removed"),
        ChangeKind::Added => ("added", "added"),
        ChangeKind::Changed => ("changed", "changed"),
        ChangeKind::SortRenamed => ("message renamed", "messages renamed"),
        ChangeKind::EnumRenamed => ("enum renamed", "enums renamed"),
        ChangeKind::MessageAdded => ("message added", "messages added"),
        ChangeKind::MessageRemoved => ("message removed", "messages removed"),
        ChangeKind::EnumAdded => ("enum added", "enums added"),
        ChangeKind::EnumRemoved => ("enum removed", "enums removed"),
        ChangeKind::ValueAdded => ("value added", "values added"),
        ChangeKind::ValueRemoved => ("value removed", "values removed"),
        ChangeKind::ValueRenamed => ("value renamed", "values renamed"),
        ChangeKind::OpennessChanged => ("openness change", "openness changes"),
        ChangeKind::PreserveChanged => ("preserve change", "preserve changes"),
        ChangeKind::PackageAdded => ("package added", "packages added"),
        ChangeKind::PackageRemoved => ("package removed", "packages removed"),
    };
    if count == 1 { one } else { many }
}

/// One token of a row's signature or a header's name: stable across the sides, or the (old,
/// new) pair of a dimension that differs — the structure an emphasis keys on, so the differing
/// token is known as data and never found by comparing two rendered strings.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Stable(String),
    Delta(String, String),
}

/// A node's sides as the comparison holds them: both, or one. Neither is unrepresentable in the
/// model — every node is built from a side — so the `unreachable` discharges an invariant rather
/// than guarding a live path.
enum Sides<'a, T> {
    Both(&'a T, &'a T),
    One(&'a T),
}

fn sides<'a, T>(old: Option<&'a T>, new: Option<&'a T>) -> Sides<'a, T> {
    match (old, new) {
        (Some(old), Some(new)) => Sides::Both(old, new),
        (Some(one), None) | (None, Some(one)) => Sides::One(one),
        (None, None) => unreachable!("a node of the comparison is on at least one side"),
    }
}

/// A header's name: the predicate as the sides spell it — an old → new pair when it differs
/// between two sides (a rename, or a rename beside a flip), else the one spelling.
fn predicate_piece<T>(sides: &Sides<'_, T>, predicate: impl Fn(&T) -> &Name) -> Piece {
    match sides {
        Sides::Both(old, new) if predicate(old) != predicate(new) => Piece::Delta(
            predicate(old).as_str().to_owned(),
            predicate(new).as_str().to_owned(),
        ),
        Sides::Both(one, _) | Sides::One(one) => Piece::Stable(predicate(one).as_str().to_owned()),
    }
}

/// The counts the footer reports: the changed nodes by kind, and the nodes carrying a bridge view.
#[derive(Default)]
struct Tally {
    kinds: BTreeMap<ChangeKind, usize>,
    bridges: usize,
}

impl Tally {
    /// Count one node of `kind` — the baseline counts as nothing — carrying a bridge or not.
    fn note(&mut self, kind: ChangeKind, bridged: bool) {
        if kind != ChangeKind::Unchanged {
            *self.kinds.entry(kind).or_default() += 1;
        }
        self.bridges += usize::from(bridged);
    }
}

/// A line under construction: the styled text and its visible width — measured on the plain
/// text as each part is pushed, never on the sequence a style adds — so a column is padded to
/// where a reader sees it.
#[derive(Default)]
struct Line {
    text: String,
    width: usize,
}

impl Line {
    /// Append `plain` as `paint` styles it, its width the plain text's.
    fn push(&mut self, plain: &str, paint: impl FnOnce(&str) -> String) {
        self.text.push_str(&paint(plain));
        self.width += width(plain);
    }

    /// Append `text` unstyled.
    fn plain(&mut self, text: &str) {
        self.push(text, str::to_owned);
    }

    /// Pad with spaces to column `column`, or to `gap` spaces past the text if that is further.
    fn column(&mut self, column: usize, gap: usize) {
        let target = column.max(self.width + gap);
        self.text.push_str(&" ".repeat(target - self.width));
        self.width = target;
    }

    /// Append `other`, set against the right edge: at [`WIDTH`] when it fits a gap past the text,
    /// else a gap past the text.
    fn right(&mut self, other: &Line) {
        self.column(WIDTH.saturating_sub(other.width), GAP);
        self.text.push_str(&other.text);
        self.width += other.width;
    }
}

/// A text's visible width: its characters, each one column — the report's glyphs included.
fn width(text: &str) -> usize {
    text.chars().count()
}

/// The writer: the text so far, the style each part is painted in, and the tally the footer
/// reports.
struct Report {
    out: String,
    style: Style,
    tally: Tally,
}

impl Report {
    /// Finish `line`: no trailing space, one newline.
    fn emit(&mut self, line: &Line) {
        self.out.push_str(line.text.trim_end());
        self.out.push('\n');
    }

    fn blank(&mut self) {
        self.out.push('\n');
    }

    /// A rule of `stroke` under a header: one space, then the stroke to the width.
    fn rule(&mut self, stroke: char) {
        let text = format!(" {}", stroke.to_string().repeat(WIDTH - 1));
        self.rule_text(&text);
    }

    fn rule_text(&mut self, text: &str) {
        let style = self.style;
        let mut line = Line::default();
        line.push(text, |text| style.rule(text));
        self.emit(&line);
    }

    /// The banner: the title rule, the transition (one package) or the package count with the
    /// keryx version on the right, and the closing rule.
    fn banner(&mut self, comparison: &Comparison<'_>) {
        let style = self.style;
        let title = "━━ keryx diff ";
        self.rule_text(&format!("{title}{}", "━".repeat(WIDTH - width(title))));
        let mut line = Line::default();
        line.plain("   ");
        match comparison.packages() {
            [package] => line.push(&transition(package), |text| style.predicate(text)),
            packages => line.plain(&format!("{} packages", packages.len())),
        }
        let mut version = Line::default();
        version.push(&format!("keryx {VERSION}"), |text| style.status(text));
        line.right(&version);
        self.emit(&line);
        self.rule_text(&"━".repeat(WIDTH));
        self.blank();
    }

    /// One package: its own header under a double rule when the report is `blocked` into
    /// packages — the transition, marked when the package is on one side only; a matched
    /// package's blocks say what changed in it, so its header carries no status — then its sorts
    /// and its enums.
    fn package(&mut self, package: &PackageDiff<'_>, blocked: bool) {
        let style = self.style;
        let kind = package.kind();
        self.tally.note(kind, false);
        if blocked {
            let mut line = Line::default();
            line.plain(" ");
            line.push(&transition(package), |text| style.predicate(text));
            if let Some(status) = self.status(kind, false, true) {
                line.right(&status);
            }
            self.emit(&line);
            self.rule('═');
            self.blank();
        }
        for sort in package.sorts() {
            self.sort(package.key(), sort);
        }
        for enumeration in package.enums() {
            self.enumeration(package.key(), enumeration);
        }
    }

    /// One message: its header — the predicate and the version-free path, its own kind as the
    /// status, `unchanged` when nothing beneath it changed either — its rule, and a row per
    /// changed field.
    fn sort(&mut self, key: &str, sort: &SortDiff<'_>) {
        let kind = sort.kind();
        let bridged = sort.bridge().is_some();
        self.tally.note(kind, bridged);
        let name = predicate_piece(&sides(sort.old_sort(), sort.new_sort()), |sort| {
            sort.predicate()
        });
        let rows = sort
            .fields()
            .iter()
            .any(|field| field.kind() != ChangeKind::Unchanged);
        let status = self.status(kind, bridged, rows);
        self.header(&name, &format!("{key}.{}", sort.name()), None, status);
        self.rule('─');
        for field in sort.fields() {
            self.field(field);
        }
        self.blank();
    }

    /// One enum, as [`Report::sort`] lays a message out: an openness or preserve flip shows the
    /// enum's descriptor old → new beside the path, and the values ride as rows.
    fn enumeration(&mut self, key: &str, enumeration: &EnumDiff<'_>) {
        let kind = enumeration.kind();
        let bridged = enumeration.bridge().is_some();
        self.tally.note(kind, bridged);
        let sides = sides(enumeration.old_enum(), enumeration.new_enum());
        let name = predicate_piece(&sides, |enumeration| enumeration.predicate());
        let delta = match sides {
            Sides::Both(old, new)
                if matches!(
                    kind,
                    ChangeKind::OpennessChanged | ChangeKind::PreserveChanged
                ) =>
            {
                Some(Piece::Delta(enum_descriptor(old), enum_descriptor(new)))
            }
            Sides::Both(..) | Sides::One(_) => None,
        };
        let rows = enumeration
            .values()
            .iter()
            .any(|value| value.kind() != ChangeKind::Unchanged);
        let status = self.status(kind, bridged, rows);
        self.header(
            &name,
            &format!("{key}.{}", enumeration.name()),
            delta.as_ref(),
            status,
        );
        self.rule('─');
        for value in enumeration.values() {
            self.value(value);
        }
        self.blank();
    }

    /// A header's status, set against the right edge: the node's own mark with its bridge
    /// posture — `bridge available` for a pure rename, `no bridge` for a removal or a change,
    /// nothing for an addition — or `unchanged` when the node's kind is the baseline and no `rows`
    /// beneath it changed either; `None` when its rows say what changed.
    fn status(&self, kind: ChangeKind, bridged: bool, rows: bool) -> Option<Line> {
        let style = self.style;
        let mut line = Line::default();
        match Glyph::of(kind) {
            None if rows => return None,
            None => line.push("unchanged", |text| style.status(text)),
            Some(glyph) => {
                line.push(glyph.mark(), |text| style.mark(glyph, text));
                if let Some(posture) = posture(glyph, bridged) {
                    line.push(" · ", |text| style.stable(text));
                    line.push(posture, |text| style.annotation(text));
                }
            }
        }
        Some(line)
    }

    /// A block header: the name (a predicate, emphasized), ` · `, the path, the descriptor
    /// `delta` when there is one, and the `status` on the right.
    fn header(&mut self, name: &Piece, path: &str, delta: Option<&Piece>, status: Option<Line>) {
        let style = self.style;
        let mut line = Line::default();
        line.plain(" ");
        match name {
            Piece::Stable(predicate) => line.push(predicate, |text| style.predicate(text)),
            Piece::Delta(old, new) => {
                line.push(old, |text| style.predicate(text));
                line.push(" → ", |text| style.stable(text));
                line.push(new, |text| style.predicate(text));
            }
        }
        line.push(" · ", |text| style.stable(text));
        line.push(path, |text| style.stable(text));
        if let Some(delta) = delta {
            line.plain("  ");
            piece(&mut line, delta, style);
        }
        if let Some(status) = status {
            line.right(&status);
        }
        self.emit(&line);
    }

    /// One field's row, when its kind is not the baseline: a pure rename shows the two names
    /// beside the arity they share and `bridge available` (`(view)` for a message field, whose
    /// bridge aliases its relational view); a change shows its predicate — or its two names,
    /// renamed as well — beside each dimension that differs, and `no bridge`; a one-sided field
    /// shows its whole signature, a removal with `no bridge`.
    fn field(&mut self, field: &FieldDiff<'_>) {
        let kind = field.kind();
        let bridged = field.bridge().is_some();
        self.tally.note(kind, bridged);
        let Some(glyph) = Glyph::of(kind) else {
            return;
        };
        let (name, rest, view) = match sides(field.old_field(), field.new_field()) {
            Sides::Both(old, new) => match field.aspect() {
                Some(aspect) => (
                    predicate_piece(&Sides::Both(old, new), |field| field.predicate()),
                    dimensions(&aspect),
                    false,
                ),
                None => (
                    Piece::Delta(
                        old.predicate().as_str().to_owned(),
                        new.predicate().as_str().to_owned(),
                    ),
                    vec![Piece::Stable(format!("/{}", old.arity()))],
                    old.view().is_some(),
                ),
            },
            Sides::One(one) => (
                Piece::Stable(format!("{}/{}", one.predicate().as_str(), one.arity())),
                vec![Piece::Stable(format!(
                    "{},{}",
                    declared(one.value()),
                    descriptor(one)
                ))],
                false,
            ),
        };
        let annotation = match posture(glyph, bridged) {
            Some("bridge available") if view => "bridge available (view)",
            Some(posture) => posture,
            None => "",
        };
        self.row(field.number(), glyph, &name, &rest, annotation);
    }

    /// One enum value's row, when its kind is not the baseline: the constant — its two spellings
    /// for a rename, which is breaking and unbridged, since a constant is vocabulary no rule can
    /// alias — with the bridge posture.
    fn value(&mut self, value: &ValueDiff<'_>) {
        let kind = value.kind();
        self.tally.note(kind, false);
        let Some(glyph) = Glyph::of(kind) else {
            return;
        };
        let (name, annotation) = match sides(value.old_value(), value.new_value()) {
            Sides::Both(old, new) => (
                Piece::Delta(
                    old.constant().as_str().to_owned(),
                    new.constant().as_str().to_owned(),
                ),
                "no bridge · constant",
            ),
            Sides::One(one) => (
                Piece::Stable(one.constant().as_str().to_owned()),
                posture(glyph, false).unwrap_or(""),
            ),
        };
        self.row(value.number(), glyph, &name, &[], annotation);
    }

    /// One row: the gutter, the mark, the name column, the rest of the signature, and the
    /// annotation at its column.
    fn row(&mut self, number: i32, glyph: Glyph, name: &Piece, rest: &[Piece], annotation: &str) {
        let style = self.style;
        let mut line = Line::default();
        let gutter = format!("#{number}");
        line.push(&format!("{gutter:>4}"), |text| style.gutter(text));
        line.plain("  ");
        line.push(glyph.mark(), |text| style.mark(glyph, text));
        line.column(ROW_START, 3);
        piece(&mut line, name, style);
        if !rest.is_empty() {
            line.column(ROW_START + NAME_WIDTH, GAP);
            for (index, dimension) in rest.iter().enumerate() {
                if index > 0 {
                    line.push(", ", |text| style.stable(text));
                }
                piece(&mut line, dimension, style);
            }
        }
        if !annotation.is_empty() {
            line.column(ROW_START + SIGNATURE_WIDTH + GAP, GAP);
            line.push(annotation, |text| style.annotation(text));
        }
        self.emit(&line);
    }

    /// The footer: a rule, then a line per non-empty category — the count, the category, and the
    /// breakdown by kind in the kinds' fixed order, the bridged line noting the bridges are
    /// inbound-facing — and the hint to write the bridge views when there are any; `no changes`
    /// when nothing changed.
    fn footer(&mut self) {
        self.rule('─');
        if self.tally.kinds.is_empty() {
            self.out.push_str("  no changes\n");
            return;
        }
        for category in [Category::Breaking, Category::Bridged, Category::Additive] {
            let counts: Vec<(ChangeKind, usize)> = self
                .tally
                .kinds
                .iter()
                .filter(|(kind, _)| Category::of(**kind) == Some(category))
                .map(|(kind, count)| (*kind, *count))
                .collect();
            if counts.is_empty() {
                continue;
            }
            let total: usize = counts.iter().map(|(_, count)| count).sum();
            let breakdown = counts
                .iter()
                .map(|(kind, count)| format!("{count} {}", noun(*kind, *count)))
                .collect::<Vec<_>>()
                .join(" · ");
            let word = category.word();
            let mut line = Line::default();
            line.plain(&format!("{total:>3} {word:<8}   {breakdown}"));
            if category == Category::Bridged {
                line.plain(" (inbound-facing)");
            }
            self.emit(&line);
        }
        if self.tally.bridges > 0 {
            let views = if self.tally.bridges == 1 {
                "view"
            } else {
                "views"
            };
            let mut line = Line::default();
            line.plain(&format!(
                "  → rerun with --bridge <path>  to write the {} bridge {views}",
                self.tally.bridges
            ));
            self.emit(&line);
        }
    }
}

/// A mark's bridge posture: `bridge available` when the node carries a bridge view, `no bridge`
/// for a removal or a change (a rename that also changed shape included), nothing for an
/// addition, which a model of the old vocabulary never reads.
fn posture(glyph: Glyph, bridged: bool) -> Option<&'static str> {
    match glyph {
        Glyph::Added => None,
        Glyph::Renamed | Glyph::Removed | Glyph::Changed => Some(if bridged {
            "bridge available"
        } else {
            "no bridge"
        }),
    }
}

/// Append `piece` to `line`: stable text as context, a delta as its two tokens around an arrow.
fn piece(line: &mut Line, piece: &Piece, style: Style) {
    match piece {
        Piece::Stable(text) => line.push(text, |text| style.stable(text)),
        Piece::Delta(old, new) => {
            line.push(old, |text| style.token(text));
            line.push(" → ", |text| style.stable(text));
            line.push(new, |text| style.token(text));
        }
    }
}

/// A package's transition: `old  →  new` across the sides, or the one side's name.
fn transition(package: &PackageDiff<'_>) -> String {
    match sides(package.old_package(), package.new_package()) {
        Sides::Both(old, new) => format!("{}  →  {}", old.as_str(), new.as_str()),
        Sides::One(one) => one.as_str().to_owned(),
    }
}

/// The dimensions a changed field differs on, each its old → new pair in the report's words:
/// the form, then the arity (a form change moves it), the value's type, the presence, and the
/// oneof cell — read from the aspect, where the comparison recorded exactly what it considered
/// changed, so a referent merely renamed never shows as a type change of the field.
fn dimensions(aspect: &ChangeAspect<'_>) -> Vec<Piece> {
    let mut pieces = Vec::new();
    if let Some((old, new)) = aspect.form() {
        pieces.push(Piece::Delta(form(old), form(new)));
    }
    if let Some((old, new)) = aspect.arity() {
        pieces.push(Piece::Delta(format!("/{old}"), format!("/{new}")));
    }
    if let Some((old, new)) = aspect.value() {
        pieces.push(Piece::Delta(declared(old), declared(new)));
    }
    if let Some((old, new)) = aspect.presence() {
        pieces.push(Piece::Delta(
            totality(old).to_owned(),
            totality(new).to_owned(),
        ));
    }
    if let Some((old, new)) = aspect.membership() {
        pieces.push(Piece::Delta(cell(old), cell(new)));
    }
    pieces
}

/// The report's word for a field's declared type — the manifest's `<declared>` column (spec
/// §13.4): a scalar's proto type name, a message's or enum's referent sort predicate.
fn declared(value: &ValueMapping) -> String {
    match value {
        ValueMapping::Scalar { kind, .. } => kind.as_str().to_owned(),
        ValueMapping::Message(name) | ValueMapping::Enum { referent: name, .. } => {
            name.as_str().to_owned()
        }
    }
}

/// The report's word for a field's descriptor — the manifest's trailing column (spec §13.4): a
/// family names its shape (`seq`, `set`, `map<key>`), a singular field or a oneof arm its
/// totality.
fn descriptor(field: &FieldMapping) -> String {
    match field.form() {
        EmitForm::Sequence | EmitForm::Set | EmitForm::Map { .. } => form(field.form()),
        EmitForm::Function | EmitForm::OneofArm { .. } => totality(field.presence()).to_owned(),
    }
}

/// The report's word for a form, every form named — a changed field's form dimension shows a
/// singular field or a oneof arm too, which the descriptor column names by totality instead.
fn form(form: &EmitForm) -> String {
    match form {
        EmitForm::Function => "singular".to_owned(),
        EmitForm::OneofArm { .. } => "oneof".to_owned(),
        EmitForm::Sequence => "seq".to_owned(),
        EmitForm::Set => "set".to_owned(),
        EmitForm::Map { key, .. } => format!("map<{}>", Scalar::from(*key).as_str()),
    }
}

/// The manifest's totality word (spec §13.4, §5).
fn totality(totality: Totality) -> &'static str {
    match totality {
        Totality::Total => "total",
        Totality::Partial => "partial",
        Totality::Required => "required",
    }
}

/// An enum's descriptor as the changeset's enum signature carries it: `(open)` or `(closed)`,
/// with `, preserve` under `(keryx.unknown) = PRESERVE` — so an openness or preserve flip shows
/// as one old → new pair.
fn enum_descriptor(enumeration: &EnumMapping) -> String {
    let openness = match enumeration.openness() {
        Openness::Open => "open",
        Openness::Closed => "closed",
    };
    if enumeration.preserve() {
        format!("({openness}, preserve)")
    } else {
        format!("({openness})")
    }
}

/// A oneof cell in the report's words: the member numbers of the oneof the field is an arm of,
/// or `no oneof` when it is no arm.
fn cell(members: &BTreeSet<i32>) -> String {
    if members.is_empty() {
        "no oneof".to_owned()
    } else {
        let numbers: Vec<String> = members.iter().map(ToString::to_string).collect();
        format!("oneof{{{}}}", numbers.join(","))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use keryx_core::descriptor::ingest;
    use keryx_core::diff::{self, ChangeKind, Comparison};
    use keryx_core::policy::{self, Mapping};
    use keryx_test_support as support;

    use super::{Category, Glyph, Line, WIDTH, declared, descriptor, enum_descriptor, noun, width};

    /// The seventeen kinds of change and the baseline, each once.
    const KINDS: [ChangeKind; 18] = [
        ChangeKind::Unchanged,
        ChangeKind::Renamed,
        ChangeKind::Removed,
        ChangeKind::Added,
        ChangeKind::Changed,
        ChangeKind::SortRenamed,
        ChangeKind::EnumRenamed,
        ChangeKind::MessageAdded,
        ChangeKind::MessageRemoved,
        ChangeKind::EnumAdded,
        ChangeKind::EnumRemoved,
        ChangeKind::ValueAdded,
        ChangeKind::ValueRemoved,
        ChangeKind::ValueRenamed,
        ChangeKind::OpennessChanged,
        ChangeKind::PreserveChanged,
        ChangeKind::PackageAdded,
        ChangeKind::PackageRemoved,
    ];

    fn mapping_of(fixture: &str) -> Mapping {
        let schema = ingest(&support::compile_fixture(fixture)).expect("ingests");
        policy::map(&schema).expect("maps")
    }

    /// Every element of the tree on either side, by its proto path: the report's spelling of it
    /// — a field's `predicate/arity declared descriptor`, a sort's `predicate/1`, an enum's
    /// `predicate/1 (open…)`, a value's constant — and the vocabulary words that spelling used.
    fn spellings(comparison: &Comparison<'_>) -> BTreeMap<String, (String, Vec<String>)> {
        let mut spelled = BTreeMap::new();
        for package in comparison.packages() {
            for sort in package.sorts() {
                for side in [sort.old_sort(), sort.new_sort()].into_iter().flatten() {
                    spelled.insert(
                        side.proto().as_str().to_owned(),
                        (format!("{}/1", side.predicate().as_str()), Vec::new()),
                    );
                    for field in side.fields() {
                        let words = vec![declared(field.value()), descriptor(field)];
                        let spelling = format!(
                            "{}/{} {} {}",
                            field.predicate().as_str(),
                            field.arity(),
                            words[0],
                            words[1]
                        );
                        spelled.insert(field.proto().as_str().to_owned(), (spelling, words));
                    }
                }
            }
            for enumeration in package.enums() {
                for side in [enumeration.old_enum(), enumeration.new_enum()]
                    .into_iter()
                    .flatten()
                {
                    let word = enum_descriptor(side);
                    let spelling = format!("{}/1 {word}", side.predicate().as_str());
                    spelled.insert(side.proto().as_str().to_owned(), (spelling, vec![word]));
                    for value in side.values() {
                        spelled.insert(
                            format!("{}.{}", side.proto().as_str(), value.proto_name()),
                            (value.constant().as_str().to_owned(), Vec::new()),
                        );
                    }
                }
            }
        }
        spelled
    }

    #[test]
    fn the_rows_vocabulary_is_the_changesets() {
        // The words a row spells — a field's `predicate/arity`, declared type, and descriptor; a
        // sort's `predicate/1`; an enum's `predicate/1 (open…)`; a value's constant — are the
        // changeset's signatures, which keryx-core renders through the manifest's own writers:
        // over the three fixture pairs, every change record's signature on each side equals the
        // report's spelling of that element, so the two cannot drift apart. And the guarantee
        // spans the vocabulary the report can emit, not only what one pair happens to hit: the
        // words the records carried are collected, and each totality word (`required` from a
        // proto2 field), each family shape with a map keyed two ways, the scalar kinds across the
        // pairs, and each enum descriptor a mapping can hold must be among them — so a fixture
        // simplified out of a word fails here. `(closed, preserve)` is no such descriptor: PRESERVE
        // on a closed enum is refused at the policy door, so no mapping, and no report, holds it.
        let mut covered: BTreeSet<String> = BTreeSet::new();
        for (old, new) in [
            ("evolution_v1.proto", "evolution_v2.proto"),
            ("report_v1.proto", "report_v2.proto"),
            ("telemetry_v1.proto", "telemetry_v2.proto"),
        ] {
            let (old, new) = (mapping_of(old), mapping_of(new));
            let comparison = diff::compare(&old, &new).expect("comparable");
            let spelled = spellings(&comparison);
            let changes = comparison.changes();
            assert!(!changes.is_empty());
            for change in &changes {
                for (path, signature) in [
                    (change.old_path(), change.old_signature()),
                    (change.new_path(), change.new_signature()),
                ] {
                    let Some(signature) = signature else {
                        continue;
                    };
                    let path = path.expect("a signature is on a side with a path");
                    let Some((spelling, words)) = spelled.get(path) else {
                        panic!("{path} is an element of the tree")
                    };
                    assert_eq!(spelling, signature, "{path}");
                    covered.extend(words.iter().cloned());
                }
            }
        }
        for word in [
            "total",
            "partial",
            "required",
            "seq",
            "set",
            "map<int32>",
            "map<string>",
            "string",
            "int32",
            "int64",
            "uint32",
            "bool",
            "bytes",
            "float",
            "double",
            "sfixed32",
            "(open)",
            "(closed)",
            "(open, preserve)",
        ] {
            assert!(
                covered.contains(word),
                "`{word}` is in a changeset signature: {covered:?}"
            );
        }
    }

    #[test]
    fn every_kind_of_change_has_a_mark_and_a_category() {
        // The baseline marks nothing and counts under nothing; each of the seventeen kinds of
        // change marks as one of the four glyphs and counts under one category — breaking
        // exactly when the kind is breaking, bridged for the three pure renames, additive for
        // every addition — under a noun that pluralizes.
        for kind in KINDS {
            let (glyph, category) = (Glyph::of(kind), Category::of(kind));
            assert_eq!(glyph.is_none(), kind == ChangeKind::Unchanged, "{kind:?}");
            assert_eq!(
                category.is_none(),
                kind == ChangeKind::Unchanged,
                "{kind:?}"
            );
            assert_eq!(
                category == Some(Category::Breaking),
                kind.is_breaking(),
                "{kind:?}"
            );
            match glyph {
                Some(Glyph::Added) => assert_eq!(category, Some(Category::Additive)),
                Some(Glyph::Removed | Glyph::Changed) => {
                    assert_eq!(category, Some(Category::Breaking));
                }
                Some(Glyph::Renamed) | None => {}
            }
            assert!(!noun(kind, 1).is_empty() && !noun(kind, 2).is_empty());
        }
        assert_eq!(
            Category::of(ChangeKind::ValueRenamed),
            Some(Category::Breaking)
        );
        assert_eq!(noun(ChangeKind::MessageAdded, 2), "messages added");
        assert_eq!(noun(ChangeKind::Removed, 2), "removed");
    }

    #[test]
    fn a_line_is_padded_by_visible_width() {
        // The report's glyphs are one column each, whatever their byte length: a column is
        // measured in characters, the gap is a floor, and the right edge is the width.
        let glyphs = " ─ · →";
        let mut line = Line::default();
        line.plain(glyphs);
        assert_eq!(line.width, width(glyphs));
        assert_eq!(width(glyphs), 6);
        assert!(glyphs.len() > 6, "the glyphs are more bytes than columns");
        line.column(10, 2);
        assert_eq!((line.width, line.text.len()), (10, glyphs.len() + 4));
        line.column(4, 2);
        assert_eq!(line.width, 12, "the gap is a floor past the text");
        let mut status = Line::default();
        status.plain("unchanged");
        line.right(&status);
        assert_eq!(line.width, WIDTH);
        assert!(line.text.ends_with("   unchanged"));
    }
}
