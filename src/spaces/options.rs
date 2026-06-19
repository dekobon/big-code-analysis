//! `Default` and inherent impl blocks for [`super::MetricsOptions`].
//!
//! Split out of `spaces.rs` to keep that module focused on the public
//! API type definitions. The blocks are moved verbatim; method and
//! trait resolution is by type, so `crate::spaces::MetricsOptions`'s
//! methods and `Default` impl resolve unchanged.

use super::*;

impl Default for MetricsOptions {
    /// Defaults preserve every metric value emitted by the pre-#182
    /// [`analyze`] entry point: every metric selected, tests
    /// included, and Rust `?` counted toward cyclomatic (#409).
    fn default() -> Self {
        Self {
            exclude_tests: false,
            metrics: MetricSet::default(),
            count_cyclomatic_try: true,
        }
    }
}

impl MetricsOptions {
    /// Builder-style setter for `MetricsOptions::exclude_tests`.
    ///
    /// Provided because `MetricsOptions` is `#[non_exhaustive]` — the
    /// struct-literal form is unavailable to downstream crates, so
    /// external callers chain `MetricsOptions::default()
    /// .with_exclude_tests(true)` instead.
    #[inline]
    #[must_use]
    pub fn with_exclude_tests(mut self, exclude_tests: bool) -> Self {
        self.exclude_tests = exclude_tests;
        self
    }

    /// Builder-style setter for `MetricsOptions::count_cyclomatic_try`.
    ///
    /// Pass `false` to stop Rust's `?` operator from contributing to
    /// cyclomatic complexity (standard and modified). The default is
    /// `true`, which keeps every published metric value unchanged
    /// (#409). Inert for non-Rust languages, none of which emit the
    /// `try_expression` grammar node.
    #[inline]
    #[must_use]
    pub fn with_count_cyclomatic_try(mut self, count: bool) -> Self {
        self.count_cyclomatic_try = count;
        self
    }

    /// Restrict computation to the given metrics. Metrics outside
    /// this set are skipped during the walk; their `Stats` fields on
    /// [`CodeMetrics`] remain at their `Default` value and are
    /// elided from the [`Serialize`] output. Pass an empty slice to
    /// disable every metric (the walker still runs and produces the
    /// space tree, but no metric values are populated).
    ///
    /// # Dependencies
    ///
    /// Derived metrics implicitly pull in the inputs they require:
    ///
    /// - [`Metric::Mi`] adds [`Metric::Loc`], [`Metric::Cyclomatic`],
    ///   [`Metric::Halstead`].
    /// - [`Metric::Wmc`] adds [`Metric::Cyclomatic`] and
    ///   [`Metric::Nom`].
    ///
    /// This auto-resolution is silent: a caller asking for `Mi`
    /// alone gets a populated `Mi` value, not a zero. See
    /// [`Metric::dependencies`] for the source of truth.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Metric, MetricsOptions};
    ///
    /// // Compute LoC only.
    /// let _opts = MetricsOptions::default().with_only(&[Metric::Loc]);
    ///
    /// // Compute Mi: Loc + Cyclomatic + Halstead are auto-added.
    /// let _opts = MetricsOptions::default().with_only(&[Metric::Mi]);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_only(mut self, metrics: &[Metric]) -> Self {
        self.metrics = MetricSet::from_slice_with_deps(metrics);
        self
    }

    /// Restrict computation to the metrics in `metrics`, closing the
    /// set under [`Metric::dependencies`] before storing it.
    ///
    /// Like [`MetricsOptions::with_only`], a derived metric pulls in
    /// the inputs it needs: passing `MetricSet::empty().with(Metric::Mi)`
    /// also selects [`Metric::Loc`], [`Metric::Cyclomatic`], and
    /// [`Metric::Halstead`], so the maintainability index is computed
    /// from real inputs rather than zero-valued defaults (#743). The
    /// resolution is idempotent: an already-closed set is stored
    /// unchanged.
    ///
    /// Use this builder when you already hold a [`MetricSet`]; reach
    /// for [`MetricsOptions::with_only`] when you have a `&[Metric]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Metric, MetricSet, MetricsOptions};
    ///
    /// // `Mi` alone — Loc + Cyclomatic + Halstead are auto-added so the
    /// // resulting MI value is meaningful.
    /// let set = MetricSet::empty().with(Metric::Mi);
    /// let _opts = MetricsOptions::default().with_metric_set(set);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_metric_set(mut self, metrics: MetricSet) -> Self {
        self.metrics = metrics.resolved();
        self
    }
}
