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
    resolve_counted(table).map(|(map, _rounds)| map)
}

/// As [`resolve`], additionally returning the number of **advancing rounds** — each raises some
/// still-clashing member's qualifier by one segment. Exposed so a test asserts the round bound (the
/// count is the qualifier prefix depth, not the number of collisions an adversary can multiply)
/// *structurally* — `resolve` itself discards it.
fn resolve_counted(
    table: &[SortEntry],
) -> Result<(BTreeMap<String, Qualified>, usize), Diagnostics> {
    let mut depths = vec![0usize; table.len()];
    let mut rounds = 0usize;
    loop {
        let names: Vec<String> = table
            .iter()
            .zip(&depths)
            .map(|(entry, &depth)| qualified(entry, depth))
            .collect();
        // Advance *every* clashing member together, not one Ord-least group per round. Distinct
        // clash-groups are independent, so resolving them in parallel bounds the round count by the
        // deepest qualifier prefix — each still-clashing member advances one segment per round — rather
        // than by the *total* number of advances, which an adversary could stretch to a superlinear
        // number of rounds (a denial-of-service the property set named open). The fixpoint is the same:
        // two names collide only at equal depth (a qualified name's `__`-separator count equals its
        // depth), so a base-group advances in lockstep and members split off monotonically, never to
        // re-collide — so the least depth at which each is unique is unchanged. The prefix length is
        // bounded (the package-segment cap and the nesting cap), so `resolve` is bounded work on any
        // admitted schema.
        let clashing = clashing_indices(&names);
        if clashing.is_empty() {
            break;
        }
        let mut progressed = false;
        let mut stuck: Option<usize> = None;
        for &i in &clashing {
            if depths[i] < prefix_segments(&table[i].path).len() {
                depths[i] += 1;
                progressed = true;
            } else if stuck.is_none() {
                stuck = Some(i);
            }
        }
        // If some member advanced we continue — a maxed member still clashing this round separates
        // next round by its shorter `__`-count. Only when *no* clashing member can advance
        // (`!progressed`) is every one at its maximal name and still sharing it: two distinct sorts
        // resolve to one maximal predicate qualification cannot separate — the same full proto path
        // (which `ingest` cannot produce, P3), or distinct paths whose base and every qualifier
        // `lower_snake`-collapse to one string (e.g. sibling `Bar`/`Bar_`, both `bar`, since
        // `lower_snake` trims a trailing `_` and collapses `_`-runs) — reachable from valid input. Both
        // are genuinely non-injective, so this is the injectivity backstop; `stuck` names the first
        // offender deterministically. Diagnose, never loop or conflate.
        if !progressed {
            return Err(duplicate(
                table,
                &names,
                stuck.expect("a non-empty clash has at least one member"),
            ));
        }
        rounds += 1;
    }
    let map = table
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
        .collect::<Result<BTreeMap<String, Qualified>, Diagnostics>>()?;
    Ok((map, rounds))
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

/// The indices of *every* entry whose emitted name is shared by two or more entries — the clashing
/// members this round, advanced together. Empty when every name is unique. Deterministic (index
/// order), O(*n* · *L* · log *n*) per round via a `BTreeMap` count (no `HashMap`, so no seed enters
/// the result, P3); the round count is bounded, so the whole pass is bounded work.
fn clashing_indices(names: &[String]) -> Vec<usize> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names {
        *counts.entry(name).or_default() += 1;
    }
    (0..names.len())
        .filter(|&i| counts[names[i].as_str()] > 1)
        .collect()
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
    use std::collections::{BTreeMap, BTreeSet};

    use proptest::prelude::*;
    use themelios_program::Name;

    use super::{Qualified, resolve, resolve_counted};
    use crate::descriptor::model::FqName;
    use crate::diagnostics::DiagnosticKind;
    use crate::policy::names::SortEntry;

    #[test]
    fn independent_collisions_resolve_in_bounded_rounds() {
        // The denial-of-service the property set named open, distilled: many *independent* base-name
        // collisions. Advancing every clashing member per round resolves them in parallel, so the
        // round count is the qualifier prefix depth (one here), not the number of groups — where
        // advancing one Ord-least group per round would take a round per group, superlinear in the
        // input. Asserted on the round count itself, not a wall-clock bound: each pair shares a base
        // and separates at its distinct package in a single advancing round, and the result is
        // injective.
        let groups = 8_000;
        let mut table = Vec::with_capacity(groups * 2);
        for i in 0..groups {
            let base = Name::new(format!("t{i}")).expect("valid base");
            for pkg in ["p", "q"] {
                table.push(SortEntry {
                    path: FqName::new(format!("{pkg}{i}.T{i}")),
                    base: base.clone(),
                    escaped: false,
                });
            }
        }
        let (resolved, rounds) =
            resolve_counted(&table).expect("the independent collisions resolve");

        assert_eq!(
            rounds, 1,
            "one advancing round for a prefix depth of one, whatever the group count — \
             a per-group walk would take {groups}"
        );
        let names: BTreeSet<&str> = resolved.values().map(|q| q.name.as_str()).collect();
        assert_eq!(
            names.len(),
            table.len(),
            "every sort gets a distinct predicate"
        );
    }

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

    proptest! {
        // `resolve` is order-independent and injective (P3, §4.2): a set of distinct paths that
        // all lower to one base predicate resolves to distinct qualified names, and the result
        // does not depend on the table's order — `resolve` makes no per-round choice (every clashing
        // member advances together, not an Ord-least one) and returns a path-keyed map, so input
        // order cannot reach the output. A qualifier that collapsed two paths fails the injectivity
        // check here.
        #[test]
        fn resolve_is_order_independent_and_injective(
            segments in prop::collection::hash_set("[a-z]{1,6}", 1..6)
        ) {
            let base = Name::new("msg").expect("`msg` is valid");
            let mut table: Vec<SortEntry> = segments
                .iter()
                .map(|seg| SortEntry {
                    path: FqName::new(format!("keryx.{seg}.Msg")),
                    base: base.clone(),
                    escaped: false,
                })
                .collect();

            let forward = resolve(&table).expect("resolves");
            table.reverse();
            let reversed = resolve(&table).expect("resolves");

            // Compare the resolved decisions by value (Qualified is not `PartialEq`): equal
            // regardless of input order.
            let project = |m: &BTreeMap<String, Qualified>| -> BTreeMap<String, (String, Vec<String>, bool)> {
                m.iter()
                    .map(|(k, q)| {
                        (k.clone(), (q.name.as_str().to_owned(), q.qualifier.clone(), q.escaped))
                    })
                    .collect()
            };
            prop_assert_eq!(project(&forward), project(&reversed));

            // Injective: distinct paths resolve to distinct predicate names.
            let names: BTreeSet<&str> = forward.values().map(|q| q.name.as_str()).collect();
            prop_assert_eq!(names.len(), forward.len());
        }
    }
}
