//! The migration report — `keryx diff`'s default product (spec §13.4, §27): the comparison
//! rendered as prose for a reader, where the JSON changeset (`Comparison::to_json`) is the
//! product for a program. Rendered from the [`Comparison`] model rather than its flat change
//! rows, so the report can show what stayed beside what changed; styled through a [`Style`],
//! plain unless the command decides the terminal wants colour. The layout here is a placeholder
//! — one line per matched package and per change, and a count — that the full report replaces.

use std::fmt::Write as _;

use keryx_core::diff::{Change, Comparison, PackageDiff};

/// How the report is styled: plain — no escape sequence, the form for `NO_COLOR` and for a
/// stdout that is not a terminal — or coloured. The placeholder report applies no emphasis in
/// either, so the two are one value for now; the value is carried so the command's colour
/// decision reaches the renderer through one door.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style;

impl Style {
    /// The plain style: no escape sequence in the report.
    #[must_use]
    pub fn plain() -> Style {
        Style
    }

    /// The style for the command's colour decision: coloured when `colored`, else plain. The
    /// placeholder report applies no emphasis yet, so both are the plain style.
    #[must_use]
    pub fn from(_colored: bool) -> Style {
        Style::plain()
    }
}

/// Render the report for `comparison` in `style`: per matched package, the two declared package
/// names (`thermal.v1 -> thermal.v2`, `-` for a side the package is not on), then one line per
/// change in it — `breaking` or `compatible`, and each side's proto path with its signature (`-`
/// for a side the element is not on) — and a closing count. Newline-terminated. A placeholder
/// layout: the full report lays the changes out under their sorts with the unchanged ones shown.
#[must_use]
pub fn render(comparison: &Comparison<'_>, _style: Style) -> String {
    let changes = comparison.changes();
    let mut out = String::new();
    for package in comparison.packages() {
        let _ = writeln!(out, "{}", package_line(package));
        for change in changes
            .iter()
            .filter(|change| change.package() == package.key())
        {
            let _ = writeln!(out, "  {}", change_line(change));
        }
    }
    let breaking = changes.iter().filter(|change| change.is_breaking()).count();
    let _ = match changes.len() {
        0 => writeln!(out, "no changes"),
        total => writeln!(out, "{total} changed, {breaking} breaking"),
    };
    out
}

/// A package's two declared names, `old -> new`, `-` for a side it is not on.
fn package_line(package: &PackageDiff<'_>) -> String {
    format!(
        "{} -> {}",
        package.old_package().map_or("-", |name| name.as_str()),
        package.new_package().map_or("-", |name| name.as_str())
    )
}

/// One change: its verdict, then its old and new sides.
fn change_line(change: &Change<'_>) -> String {
    let verdict = if change.is_breaking() {
        "breaking"
    } else {
        "compatible"
    };
    format!(
        "{verdict:<10} {} -> {}",
        side(change.old_path(), change.old_signature()),
        side(change.new_path(), change.new_signature())
    )
}

/// One side of a change: the element's proto path with its signature (a package has none), or
/// `-` when the element is not on that side.
fn side(path: Option<&str>, signature: Option<&str>) -> String {
    match (path, signature) {
        (Some(path), Some(signature)) => format!("{path} ({signature})"),
        (Some(path), None) => path.to_owned(),
        (None, _) => "-".to_owned(),
    }
}
