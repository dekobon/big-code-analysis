// bca: suppress-file(halstead, nargs)
// Catalog-driven name mapping: the offenders are the doc-heavy module's
// string-literal volume and the summed-arg count across the two small
// normalize helpers, not per-function logic complexity (cognitive /
// cyclomatic stay enforced).

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
//! ([`big_code_analysis::metric_catalog`]), not hand-maintained, so they
//! stay correct as metrics are added.
//!
//! [`thresholds`]: crate::thresholds

use std::borrow::Cow;

use big_code_analysis::metric_catalog::METRICS;

/// Family name whose dotted ids carry a meaningful leaf the `diff`
/// buckets surface directly (`loc.sloc` -> `sloc`). Every other family's
/// `diff` bucket is the family name, so a dotted id there collapses to
/// its family (`halstead.volume` -> `halstead`).
///
/// Single source of truth for the one expanded family;
/// [`metric_diff`](crate::metric_diff) imports it so the alias layer and
/// the diff bucketing cannot drift apart.
pub(crate) const EXPANDED_FAMILY: &str = "loc";

/// Normalize a `diff --metric` bucket name into the dotted id
/// `check --threshold` understands.
///
/// - A name already accepted by `check` (a [`METRICS`] id, dotted or
///   bare) passes through unchanged.
/// - A bare `loc` sub-metric (`sloc`, `ploc`, …) maps to its dotted id
///   (`loc.sloc`, …) — the unambiguous case.
/// - A bare family head that `check` only exposes through sub-metrics
///   (`halstead`, `mi`) is **ambiguous**: there is no single threshold
///   scalar for it. These return `Err` with a "did you mean" hint listing
///   the concrete dotted ids, rather than silently guessing one.
/// - Any other name is returned unchanged as `Ok`; the caller's existing
///   unknown-metric handling produces the final error so the suggestion
///   set stays in one place.
pub(crate) fn normalize_for_check(name: &str) -> Result<Cow<'_, str>, String> {
    // Already a recognised check id (covers every dotted leaf and the
    // bare ids with no sub-metric): nothing to do.
    if METRICS.iter().any(|m| m.id == name) {
        return Ok(Cow::Borrowed(name));
    }
    // A bare `loc` sub-metric: rewrite to its dotted id.
    if let Some(info) = METRICS
        .iter()
        .find(|m| m.family == EXPANDED_FAMILY && leaf_of(m.id) == name)
    {
        return Ok(Cow::Owned(info.id.to_string()));
    }
    // A bare family head that check only exposes via dotted sub-metrics
    // (e.g. `halstead`, `mi`): ambiguous, so reject with the concrete
    // candidates rather than picking one.
    let dotted: Vec<&str> = METRICS
        .iter()
        .filter(|m| m.family == name && m.id != name)
        .map(|m| m.id)
        .collect();
    if !dotted.is_empty() {
        return Err(format!(
            "ambiguous metric {name:?}: this family has no single threshold scalar; \
             did you mean one of: {}?",
            dotted.join(", ")
        ));
    }
    // Unknown to this mapping; let the caller's own unknown-name path
    // report it (it owns the canonical suggestion list).
    Ok(Cow::Borrowed(name))
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
/// one bucket.
pub(crate) fn normalize_for_diff(name: &str) -> Cow<'_, str> {
    match METRICS.iter().find(|m| m.id == name) {
        Some(info) if info.family == EXPANDED_FAMILY => Cow::Owned(leaf_of(info.id).to_string()),
        Some(info) => Cow::Borrowed(info.family),
        None => Cow::Borrowed(name),
    }
}

/// The leaf segment of a dotted id (`loc.sloc` -> `sloc`); a bare id is
/// its own leaf. Used only for `loc` sub-metrics, whose bucket name is
/// the leaf.
fn leaf_of(id: &str) -> &str {
    id.rsplit_once('.').map_or(id, |(_, leaf)| leaf)
}

/// The bare bucket names a `diff --metric` filter (and the `bca metrics
/// --metrics` selector) legitimately accepts: the `list-metrics` names —
/// every family's row names, which expand the `loc` family to its
/// sub-metrics and collapse every other family to its single bucket.
/// Sorted, deduplicated, for the unknown-name error and "did you mean"
/// suggestion. Derived from the library catalog so it cannot drift.
pub(crate) fn known_diff_metric_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = big_code_analysis::metric_catalog::FAMILIES
        .iter()
        .flat_map(|family| family.rows.iter().map(|row| row.name))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Validate one `diff --metric` / `metrics --metrics` name against the
/// catalog at parse time, reusing the `check --threshold` did-you-mean
/// machinery (#381). Accepts every spelling the diff filter handles:
/// canonical bucket names, the dotted `check --threshold` ids
/// (`cyclomatic.modified`), and the bare `loc` sub-metric aliases
/// (`sloc`, #514) — exactly what [`normalize_for_diff`] resolves. An
/// unknown name returns `Err` with the known-names list and a
/// suggestion, so a typo errors (exit 1) instead of silently matching
/// nothing (#662).
pub(crate) fn validate_diff_metric(name: &str) -> Result<(), String> {
    let known = known_diff_metric_names();
    // Resolve dotted / aliased spellings to their bucket name first, so
    // `cyclomatic.modified` and `sloc` both validate against the bucket
    // set the diff filter actually compares against.
    let resolved = normalize_for_diff(name);
    if known.contains(&resolved.as_ref()) {
        return Ok(());
    }
    Err(format!(
        "unknown metric {name:?}{}; known metrics: {}",
        crate::threshold_suggestion::format_suggestion(name, &known),
        known.join(", ")
    ))
}

/// Validate every name in a `--metric` / `--metrics` list, returning the
/// first error. Shared by `diff`, `diff-baseline`, and `metrics
/// --metrics` (#662, #691).
pub(crate) fn validate_diff_metrics(names: &[String]) -> Result<(), String> {
    names.iter().try_for_each(|n| validate_diff_metric(n))
}

/// Resolve a validated `--metrics` name (a bucket name, a dotted id, or a
/// bare `loc` sub-metric) to the library [`big_code_analysis::Metric`]
/// family it computes. A `loc` sub-metric (`sloc`, `lloc`, …) resolves to
/// [`Metric::Loc`](big_code_analysis::Metric::Loc); every other bucket
/// name is itself a family name. Returns `None` for a name with no
/// catalog family — callers validate first via [`validate_diff_metric`],
/// so `None` should not occur for accepted names. Used by `bca metrics
/// --metrics` (#691) to build the `MetricsOptions::with_only` selection.
pub(crate) fn metric_for_name(name: &str) -> Option<big_code_analysis::Metric> {
    let bucket = normalize_for_diff(name);
    let family = big_code_analysis::metric_catalog::FAMILIES
        .iter()
        .find(|f| f.rows.iter().any(|r| r.name == bucket.as_ref()))
        .map(|f| f.name)?;
    family.parse().ok()
}

/// Static assertion at first use that the catalog still contains the
/// `loc` family we expand; a future catalog edit that renamed or dropped
/// it would otherwise silently disable the `loc` sub-metric aliasing.
#[cfg(test)]
fn expanded_family_exists() -> bool {
    big_code_analysis::metric_catalog::FAMILIES
        .iter()
        .any(|f| f.name == EXPANDED_FAMILY)
}

#[cfg(test)]
#[path = "metric_alias_tests.rs"]
mod tests;
