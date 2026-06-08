//! Complexity × churn "hotspot" score (Tornhill; Nagappan & Ball).
//!
//! A hotspot is a file that is both complex *and* changes often — the
//! intersection the empirical work finds most defect-prone. It is only
//! meaningful when an AST-derived complexity figure is available
//! alongside the change history, so the field is `Option<f64>` on the
//! serialized stats and is filled in by the consumer that has both
//! halves (e.g. `bca metrics --metrics cyclomatic,vcs`).

/// Compute the hotspot score from a complexity index and recent churn.
///
/// `complexity_index` is supplied by the caller from whichever
/// AST metric it treats as the complexity axis (the CLI uses the
/// file-level cyclomatic sum, the conventional hotspot proxy). The
/// product is ordinal, like the risk score: rank files by it, do not
/// read absolute magnitudes.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn hotspot_score(complexity_index: f64, churn_recent: u64) -> f64 {
    complexity_index.max(0.0) * churn_recent as f64
}

#[cfg(test)]
#[path = "hotspot_tests.rs"]
mod tests;
