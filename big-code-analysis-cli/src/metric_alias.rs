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
const EXPANDED_FAMILY: &str = "loc";

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
