//! Shared metric-name aliasing between `bca check --threshold` and
//! `bca diff --metric` (issue #514).
//!
//! The two gate subcommands historically spoke different vocabularies for
//! the same conceptual metric:
//!
//! - `check --threshold` keys its [`thresholds`] extractor table off the
//!   library's canonical **dotted** ids — `loc.sloc`, `halstead.volume`,
//!   `mi.original`, `cyclomatic.modified`, plus bare ids like `cognitive`
//!   and `nom` that have no sub-metric.
//! - `diff --metric` buckets per-file deltas by the **bare** names
//!   `bca list-metrics` prints — `sloc`, `halstead`, `mi`, `cyclomatic`,
//!   `cognitive`, `nom`, … (the `loc` family expands to its sub-rows;
//!   every other family collapses to its family name).
//!
//! So a `loc` sub-metric was `loc.sloc` in `check` but `sloc` in `diff`,
//! and copy-pasting a name between the two silently mis-targeted. This
//! module reconciles them non-breakingly (issue option 1): each
//! subcommand accepts the *other's* spelling and normalizes to its own
//! internal form. Existing spellings keep working unchanged.
//!
//! Both directions are derived from the library catalog
//! ([`big_code_analysis::metric_catalog`]), not hand-maintained — which
//! family expands into row-named buckets is read off its rows — so they
//! stay correct as metrics are added.
//!
//! [`thresholds`]: crate::thresholds

use big_code_analysis::metric_catalog::{
    FAMILIES, METRICS, MetricFamily, MetricRow, ThresholdMetricError, resolve_threshold_metric,
};

/// The `list-metrics` rows `diff` buckets `family`'s leaves by, when it
/// has them. A family whose rows name its sub-measurements, as `loc`'s
/// do (`sloc`, `ploc`, …), expands to one bucket per row; every other
/// family is a single row named after itself and one bucket of that
/// name, so a dotted id there collapses to its family (`halstead.volume`
/// -> `halstead`).
///
/// Read off the catalog rather than naming `loc`, because
/// [`known_diff_metric_names`] accepts every row of every family: a
/// family that grows such rows must expand on the `diff` side the same
/// moment, or `--metric <row>` would validate and then match no bucket —
/// an empty diff, and exit 0 under `--exit-code` (#662). The bucketing in
/// [`metric_diff`](crate::metric_diff) and [`normalize_for_diff`] both
/// read it through [`diff_bucket`].
/// `every_known_diff_name_is_a_bucket_a_leaf_can_land_in` walks every
/// row to pin the two sides equal over today's catalog, and
/// `a_second_family_with_rows_of_its_own_expands_too` grows a family
/// rows of its own, which a rule naming `loc` would miss.
pub(crate) fn expanded_rows(family: &MetricFamily) -> Option<&'static [MetricRow]> {
    family
        .rows
        .iter()
        .any(|row| row.name != family.name)
        .then_some(family.rows)
}

/// The `diff` bucket for `leaf`, a leaf path under `family`'s subtree:
/// the row it names when the family expands, otherwise the family
/// itself. A leaf of an expanded family that no row names falls back to
/// the family name too, so it is never silently dropped.
///
/// A leaf names a row when it is that row's name, alone or followed by
/// an emitter suffix (`sloc`, `sloc_average`, `sloc_max`). The row names
/// are matched whole, longest first, rather than splitting the leaf at
/// its first `_`: a row whose own name holds an underscore
/// (`unique_operators`) would otherwise bucket under `unique`, or under
/// the family, and `--metric unique_operators` would match nothing.
pub(crate) fn diff_bucket(family: &MetricFamily, leaf: &str) -> &'static str {
    let names_row = |row: &&MetricRow| {
        leaf.strip_prefix(row.name)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['_', '.']))
    };
    expanded_rows(family)
        .and_then(|rows| {
            rows.iter()
                .filter(names_row)
                .max_by_key(|row| row.name.len())
        })
        .map_or(family.name, |row| row.name)
}

/// Normalize a `diff --metric` bucket name into the dotted id
/// `check --threshold` understands.
///
/// The rules are the library's
/// [`resolve_threshold_metric`], which the Python bindings'
/// `to_sarif(thresholds=)` also uses, so a name works in both or in
/// neither (#1618): a [`METRICS`] id passes through, a bare `loc`
/// sub-metric (`sloc`) becomes its dotted id (`loc.sloc`), and an
/// ambiguous family head (`halstead`, `mi`) is `Err` with the concrete
/// ids, in the library error's own wording.
///
/// One rule is this layer's: a name the library finds *unknown* is
/// returned unchanged as `Ok`. The callers resolve it one layer down,
/// in `ThresholdSet::build_tiered`, which renders the library's
/// unknown-name error and its did-you-mean list; `canonical_cli_thresholds`
/// says why the two typo classes must not split across the clap boundary
/// (#1165).
pub(crate) fn normalize_for_check(name: &str) -> Result<&str, String> {
    match resolve_threshold_metric(name) {
        Ok(info) => Ok(info.id),
        Err(ThresholdMetricError::Unknown { .. }) => Ok(name),
        Err(err) => Err(err.to_string()),
    }
}

/// Normalize a `check`-style dotted id into the bare `diff --metric`
/// bucket name.
///
/// - A `loc` sub-metric keeps its leaf (`loc.sloc` -> `sloc`), matching
///   the expanded `loc` buckets.
/// - Any other dotted id collapses to its family (`halstead.volume` ->
///   `halstead`, `mi.original` -> `mi`, `cyclomatic.modified` ->
///   `cyclomatic`), matching the single bucket those families produce.
/// - A name with no catalog entry (already-bare bucket name, or an
///   unknown one) is returned unchanged; the diff filter then matches it
///   literally, exactly as before.
///
/// This direction is always unambiguous: every dotted id maps to exactly
/// one bucket, the one [`diff_bucket`] gives the leaf it names.
pub(crate) fn normalize_for_diff(name: &str) -> &str {
    let Some(info) = METRICS.iter().find(|m| m.id == name) else {
        return name;
    };
    FAMILIES
        .iter()
        .find(|family| family.name == info.family)
        .map_or(info.family, |family| diff_bucket(family, leaf_of(info.id)))
}

/// The leaf segment of a dotted id (`loc.sloc` -> `sloc`); a bare id is
/// its own leaf.
fn leaf_of(id: &str) -> &str {
    id.rsplit_once('.').map_or(id, |(_, leaf)| leaf)
}

/// The bare bucket names a `diff --metric` filter legitimately accepts:
/// the `list-metrics` names — every family's row names, which expand the
/// `loc` family to its sub-metrics and collapse every other family to
/// its single bucket. Sorted, deduplicated, for the unknown-name error
/// and "did you mean" suggestion. Derived from the library catalog so it
/// cannot drift. (`bca metrics --metrics` also takes the family names;
/// its list is the library's `metric_catalog::selector_names`.)
pub(crate) fn known_diff_metric_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = big_code_analysis::metric_catalog::FAMILIES
        .iter()
        .flat_map(|family| family.rows.iter().map(|row| row.name))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Validate one `diff --metric` name against the
/// catalog at parse time, reusing the `check --threshold` did-you-mean
/// machinery (#381). Accepts every spelling the diff filter handles:
/// canonical bucket names, the dotted `check --threshold` ids
/// (`cyclomatic.modified`), and the bare `loc` sub-metric aliases
/// (`sloc`, #514) — exactly what [`normalize_for_diff`] resolves. An
/// unknown name returns `Err` with the known-names list and a
/// suggestion, so a typo errors (exit 1) instead of silently matching
/// nothing (#662).
fn validate_one_diff_metric(name: &str, known: &[&'static str]) -> Result<(), String> {
    // Resolve dotted / aliased spellings to their bucket name first, so
    // `cyclomatic.modified` and `sloc` both validate against the bucket
    // set the diff filter actually compares against.
    if known.contains(&normalize_for_diff(name)) {
        return Ok(());
    }
    Err(unknown_metric_message(name, known))
}

/// The unknown-name error `diff --metric` and `metrics --metrics` share:
/// the input, a "did you mean" hint drawn from `known`, and `known`
/// itself. The two flags differ only in the vocabulary they pass.
fn unknown_metric_message(name: &str, known: &[&str]) -> String {
    format!(
        "unknown metric {name:?}{}; known metrics: {}",
        crate::threshold_suggestion::format_suggestion(name, known),
        known.join(", ")
    )
}

/// Validate every name in a `--metric` list, returning the first error.
/// Shared by `diff` and `diff-baseline` (#662); `metrics --metrics`
/// (#691) resolves through [`metric_for_name`] instead, which also takes
/// a family name.
pub(crate) fn validate_diff_metrics(names: &[String]) -> Result<(), String> {
    let known = known_diff_metric_names();
    names
        .iter()
        .try_for_each(|n| validate_one_diff_metric(n, &known))
}

/// Resolve one `bca metrics --metrics` name to the library
/// [`Metric`](big_code_analysis::Metric) family it computes, for the
/// `MetricsOptions::with_only` selection (#691).
///
/// The rules are the library's
/// [`resolve_selector`](big_code_analysis::metric_catalog::resolve_selector),
/// which the Python bindings' `metrics=` also uses, so a name works in
/// both or in neither (#1614). Only the error wording is the CLI's: it
/// quotes the input the way `diff --metric` does, so the hint is
/// recomputed here over the same vocabulary rather than taken from the
/// library error's `Display`.
pub(crate) fn metric_for_name(name: &str) -> Result<big_code_analysis::Metric, String> {
    big_code_analysis::metric_catalog::resolve_selector(name).map_err(|_| {
        unknown_metric_message(name, big_code_analysis::metric_catalog::selector_names())
    })
}

#[cfg(test)]
#[path = "metric_alias_tests.rs"]
mod tests;
