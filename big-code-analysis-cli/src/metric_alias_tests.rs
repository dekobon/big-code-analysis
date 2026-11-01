//! Tests for the shared metric-name aliasing (issue #514).

use super::*;

/// Which family expands is read off the catalog's rows, and `loc` is
/// the one family whose rows name its sub-measurements today. Pinned so
/// a catalog edit that collapsed or renamed those rows, or gave a second
/// family rows of its own, changes the `diff` vocabulary deliberately
/// rather than silently.
#[test]
fn loc_is_the_one_family_that_expands() {
    let expanded: Vec<&str> = FAMILIES
        .iter()
        .filter(|family| expanded_rows(family).is_some())
        .map(|family| family.name)
        .collect();
    assert_eq!(expanded, ["loc"]);
}

/// Every name `known_diff_metric_names` accepts for `--metric` is a
/// bucket a delta can land in: a row of an expanded family buckets by
/// its own name, and every other family's one row is the family name.
/// Walks every row, including one with no dotted `METRICS` id, which the
/// round-trip test below skips — a row validated here but produced
/// nowhere would be an empty diff, and exit 0 under `--exit-code` (#662).
///
/// Each row is fed as the leaves the emitter writes for it, bare and
/// suffixed. No row in today's catalog has an underscore in its name, so
/// this cannot tell whole-name matching from a split at the first `_`;
/// `a_second_family_with_rows_of_its_own_expands_too` is what can.
#[test]
fn every_known_diff_name_is_a_bucket_a_leaf_can_land_in() {
    let known = known_diff_metric_names();
    assert!(!known.is_empty(), "the catalog has rows");
    for name in &known {
        let produced = FAMILIES.iter().any(|family| {
            family.rows.iter().any(|row| {
                row.name == *name
                    && ["", "_average", "_min", "_max"].iter().all(|suffix| {
                        diff_bucket(family, &format!("{}{suffix}", row.name)) == *name
                    })
            })
        });
        assert!(
            produced,
            "`--metric {name}` validates but no family buckets a leaf under it"
        );
    }
}

/// The catalog has one expanding family, so the two tests above hold
/// just as well for a rule that names `loc`. This one gives a second
/// family sub-measurement rows of its own and requires its leaves to
/// bucket by row the moment it does: `known_diff_metric_names` would
/// accept `--metric volume` at once, and a rule that missed the new
/// family would then match no bucket, an empty diff that exits 0 under
/// `--exit-code` (#662).
#[test]
fn a_second_family_with_rows_of_its_own_expands_too() {
    let halstead = FAMILIES
        .iter()
        .find(|family| family.name == "halstead")
        .expect("the catalog has a halstead family");
    assert_eq!(diff_bucket(halstead, "volume"), "halstead");

    let renamed = |name: &'static str| {
        let mut row = halstead.rows[0];
        row.name = name;
        row
    };
    let mut grown = *halstead;
    grown.rows = Box::leak(Box::new([renamed("volume"), renamed("length")]));
    assert!(expanded_rows(&grown).is_some());
    assert_eq!(diff_bucket(&grown, "volume"), "volume");
    assert_eq!(diff_bucket(&grown, "length"), "length");
    // A leaf no row names still lands somewhere.
    assert_eq!(diff_bucket(&grown, "effort"), "halstead");

    // Rows whose names hold an underscore, one extending another's name:
    // a leaf buckets by the whole row it names, suffix or not, and never
    // by its first `_`-separated segment or by a shorter row it starts
    // with. A split at the first `_` files every `unique_operators*` leaf
    // under `unique`, so `--metric unique_operators` would match nothing.
    grown.rows = Box::leak(Box::new([
        renamed("unique"),
        renamed("unique_operators"),
        renamed("operators"),
    ]));
    for (leaf, bucket) in [
        ("unique_operators", "unique_operators"),
        ("unique_operators_average", "unique_operators"),
        ("unique_max", "unique"),
        ("operators_min", "operators"),
        // Starts with a row's name but does not end it at a boundary.
        ("uniqueness", "halstead"),
    ] {
        assert_eq!(diff_bucket(&grown, leaf), bucket, "{leaf}");
    }
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
    for leaf in ["sloc", "ploc", "lloc", "cloc", "blank", "dloc"] {
        let dotted = format!("loc.{leaf}");
        assert_eq!(normalize_for_check(leaf).expect("bare ok"), dotted);
        assert_eq!(normalize_for_diff(&dotted), leaf);
    }
}

/// Both directions derive "which row names abbreviate a dotted id" from
/// the catalog's rows — `check` through `resolve_threshold_metric`, `diff`
/// through [`expanded_rows`] — but by different rules; this walks every
/// family row the catalog has, so a second family whose rows are named
/// like its dotted leaves fails here if the two readings disagree
/// (#1618).
#[test]
fn every_row_named_like_a_dotted_id_round_trips_both_directions() {
    use big_code_analysis::metric_catalog::{FAMILIES, lookup};
    let mut pairs = 0;
    for family in FAMILIES {
        for row in family.rows {
            let dotted = format!("{}.{}", family.name, row.name);
            if lookup(&dotted).is_none() {
                continue;
            }
            pairs += 1;
            assert_eq!(normalize_for_check(row.name).expect("resolves"), dotted);
            assert_eq!(normalize_for_diff(&dotted), row.name, "{dotted}");
        }
    }
    assert!(
        pairs >= 6,
        "non-vacuity: at least the six loc rows, got {pairs}"
    );
}

/// `--metrics` takes a family name, a `list-metrics` row name and a
/// dotted id, each selecting a whole family (#691, #1614).
#[test]
fn metric_for_name_resolves_every_selector_spelling() {
    use big_code_analysis::Metric;
    assert_eq!(metric_for_name("loc"), Ok(Metric::Loc));
    assert_eq!(metric_for_name("dloc"), Ok(Metric::Loc));
    assert_eq!(
        metric_for_name("cyclomatic.modified"),
        Ok(Metric::Cyclomatic)
    );
    assert_eq!(metric_for_name("halstead"), Ok(Metric::Halstead));
}

/// The `--metrics` error text, byte for byte as it read before #1614
/// moved the resolution into the library: quoted input, the hint, then
/// the family and row names.
#[test]
fn metric_for_name_error_text_is_pinned() {
    const KNOWN: &str = "abc, blank, cloc, cognitive, cyclomatic, dloc, halstead, lloc, loc, \
                         mi, nargs, nexits, nom, npa, npm, ploc, sloc, tokens, wmc";
    assert_eq!(
        metric_for_name("dlocc"),
        Err(format!(
            "unknown metric \"dlocc\"; did you mean `dloc`?; known metrics: {KNOWN}"
        ))
    );
    assert_eq!(
        metric_for_name("xyzq"),
        Err(format!("unknown metric \"xyzq\"; known metrics: {KNOWN}"))
    );
}
