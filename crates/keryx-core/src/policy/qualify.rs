//! Stage-1 qualification and injectivity (spec §4.2): resolve base sort-name collisions
//! by prefixing the shortest suffix of the proto path that restores injectivity, and
//! guarantee the emitted sort namespace is injective. Message and enum sorts share one /1
//! namespace, so two distinct types with the same base name collide; field predicates are
//! intentionally shared (§4.2) and not qualified here. Computed in Rust (R3/R4),
//! deterministic, with a unique result (multiplicity is a keryx bug, asserted in tests).

use std::collections::BTreeMap;

use themelios_program::Name;

use crate::descriptor::model::FqName;
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::policy::names::{SortEntry, identifier, lower_snake};

/// A resolved sort name with the decisions that produced it (§13.4): the final `name`, the
/// `qualifier` segments prepended (each `lower_snake`d; empty when bare), and whether the base
/// name was reserved-word `escaped`. Carried as data so the manifest need not re-derive it from
/// the name.
#[derive(Debug)]
pub(super) struct Qualified {
    pub(super) name: Name,
    pub(super) qualifier: Vec<String>,
    pub(super) escaped: bool,
}

/// Resolve the base sort table to the final `path → Name` map (spec §4.2). When distinct
/// sorts share a base name, **all** of them are prefixed together to the shortest common
/// path-suffix depth that separates them — the symmetric rule §4.2's own example shows
/// (`dispatch__status` *and* `logistics__status`), *not* the literal "leave one bare,
/// minimize total qualifier segments" reading, which is non-unique (which sort stays bare
/// is a free choice) and so would violate P3. A name is qualified only when it actually
/// collides, and only as deep as needed. The pass is choice-free and deterministic, so the
/// result is unique; distinct full paths guarantee termination. Total (§6). (§4.2's prose
/// objective and its example are themselves in tension — keryx follows the example's
/// symmetric, unique rule, recorded in spec §34 item 9.)
pub(super) fn resolve(table: &[SortEntry]) -> Result<BTreeMap<String, Qualified>, Diagnostics> {
    let mut depths = vec![0usize; table.len()];
    loop {
        let names: Vec<String> = table
            .iter()
            .zip(&depths)
            .map(|(entry, &depth)| qualified(entry, depth))
            .collect();
        match first_clash(&names) {
            None => break,
            Some(clashing) => {
                let mut progressed = false;
                for &i in &clashing {
                    if depths[i] < prefix_segments(&table[i].path).len() {
                        depths[i] += 1;
                        progressed = true;
                    }
                }
                // A stall — no member could advance — means two entries share the same
                // maximal qualified name: either the same full proto path (which `ingest`
                // cannot produce, P3), or distinct paths whose base and every qualifier
                // `lower_snake`-collapse to one string (e.g. sibling `Bar`/`Bar_`, both
                // `bar`, since `lower_snake` trims a trailing `_` and collapses `_`-runs) —
                // reachable from valid input. Both are genuinely non-injective, so this guard
                // is the injectivity backstop: without it `resolve` would emit a
                // non-injective map. `first_clash` only reports groups of two or more, so
                // `clashing[0]` names the first offender. Diagnose, never loop or conflate.
                if !progressed {
                    return Err(duplicate(table, &names, clashing[0]));
                }
            }
        }
    }
    table
        .iter()
        .zip(&depths)
        .map(|(entry, &depth)| {
            let name = identifier(&qualified(entry, depth), &entry.path)?;
            let qualifier = qualifier_segments(entry, depth);
            Ok((
                entry.path.as_str().to_owned(),
                Qualified {
                    name,
                    qualifier,
                    escaped: entry.escaped,
                },
            ))
        })
        .collect()
}

/// The emitted name for an entry at qualifier `depth`: its qualifier segments joined by `__`
/// and prepended to the escaped base leaf, or the base name alone when bare (`depth == 0`).
fn qualified(entry: &SortEntry, depth: usize) -> String {
    let segments = qualifier_segments(entry, depth);
    if segments.is_empty() {
        return entry.base.as_str().to_owned();
    }
    let mut out = segments.join("__");
    out.push_str("__");
    out.push_str(entry.base.as_str());
    out
}

/// The qualifier segments for an entry at `depth`: the last `depth` prefix segments, each
/// `lower_snake`d; empty when `depth == 0` (bare) — the decision the mapping records as data
/// (§13.4). Precondition `depth <= prefix_segments(path).len()`, upheld by `resolve`'s advance
/// guard, which never raises a depth to or past that bound; the suffix slice would otherwise
/// underflow.
fn qualifier_segments(entry: &SortEntry, depth: usize) -> Vec<String> {
    if depth == 0 {
        return Vec::new();
    }
    let prefix = prefix_segments(&entry.path);
    debug_assert!(
        depth <= prefix.len(),
        "qualifier depth exceeds available prefix segments"
    );
    prefix[prefix.len() - depth..]
        .iter()
        .map(|s| lower_snake(s))
        .collect()
}

/// The proto path's segments before the final (type-name) segment, e.g.
/// `a.b.C.Inner` → `["a", "b", "C"]` — the package and lexical-nesting qualifiers (§8).
fn prefix_segments(path: &FqName) -> Vec<&str> {
    let mut segments: Vec<&str> = path.as_str().split('.').collect();
    segments.pop(); // drop the type's own leaf
    segments
}

/// The indices of the lexicographically-least name shared by two or more entries, or
/// `None` when every name is unique (deterministic tie-break — the Ord-least clash first).
fn first_clash(names: &[String]) -> Option<Vec<usize>> {
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, name) in names.iter().enumerate() {
        by_name.entry(name).or_default().push(i);
    }
    by_name
        .into_iter()
        .find(|(_, indices)| indices.len() > 1)
        .map(|(_, indices)| indices)
}

/// The injectivity-backstop diagnostic (§6): two distinct sorts — at index `i` (the first
/// index of the stalled clash) and at least one other — resolve to the identical maximal
/// name `names[i]`, which qualification cannot separate. Reachable from valid input when
/// distinct proto names `lower_snake`-collapse (e.g. `Bar`/`Bar_`), so this is a real check,
/// not a can't-happen guard.
fn duplicate(table: &[SortEntry], names: &[String], i: usize) -> Diagnostics {
    Diagnostics::from(Diagnostic::new(
        DiagnosticKind::UnmappableName,
        Locus::at(table[i].path.as_str()),
        format!("two sorts resolve to the same predicate `{}`", names[i]),
    ))
}

#[cfg(test)]
mod tests {
    use themelios_program::Name;

    use super::resolve;
    use crate::descriptor::model::FqName;
    use crate::diagnostics::DiagnosticKind;
    use crate::policy::names::SortEntry;

    /// The `!progressed` guard keeps `resolve` total when two entries share a maximal
    /// qualified name and no member can advance. This hand-built table exercises the
    /// identical-full-path variant (which `ingest` cannot produce, so no fixture reaches it);
    /// the *reachable* variant — distinct paths that `lower_snake`-collapse — is covered
    /// end-to-end by the `collapsing_sorts` fixture. Here the two entries advance to max
    /// depth, stay identical, and `resolve` returns a `duplicate` diagnostic, not a loop.
    #[test]
    fn two_entries_sharing_a_full_path_diagnose_rather_than_loop() {
        let base = Name::new("dup").expect("`dup` is a valid identifier");
        let table = vec![
            SortEntry {
                path: FqName::new("keryx.coll.Dup"),
                base: base.clone(),
                escaped: false,
            },
            SortEntry {
                path: FqName::new("keryx.coll.Dup"),
                base,
                escaped: false,
            },
        ];
        let error = resolve(&table).expect_err("a shared full path is diagnosed, not looped");
        assert_eq!(error.len(), 1);
        let diagnostic = error.iter().next().expect("one diagnostic");
        assert_eq!(diagnostic.kind(), DiagnosticKind::UnmappableName);
        assert_eq!(diagnostic.locus().path(), Some("keryx.coll.Dup"));
    }
}
