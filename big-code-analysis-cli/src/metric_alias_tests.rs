//! Tests for the shared metric-name aliasing (issue #514).

use super::*;

#[test]
fn loc_family_present_for_expansion() {
    // Guards the `EXPANDED_FAMILY` constant against a future catalog
    // rename that would silently disable `loc` sub-metric aliasing.
    assert!(expanded_family_exists());
}

#[test]
fn check_accepts_bare_loc_submetric() {
    // `diff` spelling `sloc` -> `check` dotted id `loc.sloc`.
    assert_eq!(
        normalize_for_check("sloc").expect("unambiguous"),
        "loc.sloc"
    );
    assert_eq!(
        normalize_for_check("lloc").expect("unambiguous"),
        "loc.lloc"
    );
    assert_eq!(
        normalize_for_check("blank").expect("unambiguous"),
        "loc.blank"
    );
}

#[test]
fn check_passes_through_its_own_spelling() {
    // Dotted ids and bare-no-submetric ids are already valid for check.
    for name in [
        "loc.sloc",
        "halstead.volume",
        "mi.original",
        "cognitive",
        "nom",
    ] {
        assert_eq!(
            normalize_for_check(name).expect("valid"),
            name,
            "{name} should pass through unchanged"
        );
    }
}

#[test]
fn check_rejects_ambiguous_family_head() {
    // `halstead` and `mi` have no single threshold scalar.
    let err = normalize_for_check("halstead").expect_err("ambiguous");
    assert!(err.contains("ambiguous"), "message: {err}");
    assert!(
        err.contains("halstead.volume"),
        "should list candidates: {err}"
    );

    let err = normalize_for_check("mi").expect_err("ambiguous");
    assert!(err.contains("mi.original"), "should list candidates: {err}");
}

#[test]
fn check_leaves_unknown_for_caller() {
    // Unknown names are returned unchanged so the caller's own
    // suggestion path reports them.
    assert_eq!(
        normalize_for_check("not_a_metric").expect("passthrough"),
        "not_a_metric"
    );
}

#[test]
fn diff_collapses_dotted_to_bucket() {
    assert_eq!(normalize_for_diff("loc.sloc"), "sloc");
    assert_eq!(normalize_for_diff("loc.lloc"), "lloc");
    assert_eq!(normalize_for_diff("halstead.volume"), "halstead");
    assert_eq!(normalize_for_diff("mi.original"), "mi");
    assert_eq!(normalize_for_diff("cyclomatic.modified"), "cyclomatic");
}

#[test]
fn diff_passes_through_bare_bucket_names() {
    for name in [
        "sloc",
        "halstead",
        "cyclomatic",
        "cognitive",
        "nom",
        "unknown",
    ] {
        assert_eq!(normalize_for_diff(name), name, "{name} unchanged");
    }
}

/// Round-trip: every `loc` sub-metric and dotted family id is accepted by
/// `check` in both spellings, and the `diff` form of each dotted id is the
/// bucket name a delta would land in.
#[test]
fn loc_submetrics_round_trip_both_directions() {
    for leaf in ["sloc", "ploc", "lloc", "cloc", "blank"] {
        let dotted = format!("loc.{leaf}");
        assert_eq!(normalize_for_check(leaf).expect("bare ok"), dotted);
        assert_eq!(normalize_for_diff(&dotted), leaf);
    }
}
