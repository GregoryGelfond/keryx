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
//! trailing version segment stripped — `normalize_package`.
//!
//! [`policy::map`]: crate::policy::map
//! [`Mapping`]: crate::policy::model::Mapping
//! [`manifest`]: crate::manifest

/// A package with its trailing version segment stripped — the key under which a unit is matched
/// across schema versions, so `thermal.v1` and `thermal.v2` are one unit, `thermal`. The version
/// grammar is buf's `PACKAGE_VERSION_SUFFIX` lint rule ([`is_version_segment`]), and a versioned
/// package is a name and a version — two dot-separated segments at least, as buf reads it — so a
/// lone version segment (`v1`) is not stripped. Only the last segment is examined, and only in
/// full: `acme.v1.dispatch` is unchanged (its last segment is not a version, whatever an earlier
/// one is), as are `acme.v1things` and a package with no version segment at all. Returns a
/// prefix of its input; nothing is allocated.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the comparison of two mappings consumes the key; until it lands, only the unit tests read it"
    )
)]
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
    use super::{is_version_segment, normalize_package};

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
}
