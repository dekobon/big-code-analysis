//! Combined membership and exit semantics without changing legacy report types.

use super::runner::SourceReport;
use super::{HarnessError, require};
use crate::cli::{Mode, Scope};
use crate::scaling;

/// Independent result collections from the two scaling methods.
pub struct CombinedReport {
    /// Legacy exponents, preserving their library contract.
    pub legacy: Option<scaling::Report>,
    /// Typed source statistics.
    pub source: Option<SourceReport>,
}

impl CombinedReport {
    /// Validate selected registry membership and count both kinds of failures.
    ///
    /// # Errors
    /// Rejects absent, extra, duplicated or reordered results in either selected set.
    pub fn validate(
        &self,
        scope: Scope,
        legacy_ids: &[&str],
        source_ids: &[&str],
    ) -> Result<(), HarnessError> {
        require(
            self.legacy.is_some() == (scope != Scope::Source),
            "legacy report scope mismatch",
        )?;
        require(
            self.source.is_some() == (scope != Scope::Legacy),
            "source report scope mismatch",
        )?;
        if let Some(report) = &self.legacy {
            require(
                legacy_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == legacy_ids.len(),
                "duplicate expected legacy ID",
            )?;
            require(
                !report.probes.is_empty()
                    && report
                        .probes
                        .iter()
                        .map(|p| p.name)
                        .eq(legacy_ids.iter().copied()),
                "legacy result membership mismatch",
            )?;
        }
        if let Some(report) = &self.source {
            require(
                source_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == source_ids.len(),
                "duplicate expected source ID",
            )?;
            report.validate_data()?;
            require(
                !report.results.is_empty()
                    && report
                        .results
                        .iter()
                        .map(|p| p.id)
                        .eq(source_ids.iter().copied()),
                "source result membership mismatch",
            )?;
        }
        Ok(())
    }

    /// Total performance failures across separate report collections.
    #[must_use]
    pub fn failures(&self) -> usize {
        self.legacy.as_ref().map_or(0, |r| r.failures().len())
            + self.source.as_ref().map_or(0, |r| r.failures().len())
    }

    /// Exit 1 for a performance failure in gate mode, otherwise 0. Harness errors
    /// are represented by failed construction/validation and use exit 2 upstream.
    #[must_use]
    pub fn exit_code(&self, mode: Mode, optimized: bool) -> u8 {
        u8::from(mode == Mode::Gate && optimized && self.failures() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::runner::{Batch, SourceCell, SourceResult, Statistic};
    use super::*;

    #[test]
    fn combined_membership_and_source_failure_cannot_hide_behind_legacy_pass() {
        let legacy = scaling::Report {
            rounds: 3,
            probes: vec![scaling::ProbeReport {
                name: "legacy",
                cells: vec![],
                exponent: 1.0,
                max_exponent: 1.5,
                rationale: "",
                over_budget: None,
            }],
        };
        let source = SourceReport {
            rounds: 3,
            mode: Mode::Gate,
            results: vec![SourceResult {
                id: "source",
                cells: [256, 512, 1024]
                    .into_iter()
                    .zip([200_000, 200_000, 400_000])
                    .map(|(size, ns)| {
                        let elapsed = std::time::Duration::from_nanos(ns);
                        SourceCell {
                            counts: super::super::SourceCounts::default(),
                            size,
                            bytes: size,
                            segments: 1,
                            iterations: 1,
                            batches: (0..3).map(|round| Batch { round, elapsed }).collect(),
                            batch_min: elapsed,
                            batch_median: elapsed,
                            batch_max: elapsed,
                            ns_per_operation: elapsed.as_secs_f64() * 1e9,
                        }
                    })
                    .collect(),
                statistic: Statistic::FixedOutputRatio(2.0),
                bound: 1.5,
                dimensions: "",
                timing: "",
                policy: "",
            }],
        };
        let mut report = CombinedReport {
            legacy: Some(legacy),
            source: Some(source),
        };
        assert!(
            report
                .validate(Scope::All, &["legacy"], &["source"])
                .is_ok()
        );
        assert_eq!(report.failures(), 1);
        assert_eq!(report.exit_code(Mode::Gate, true), 1);
        assert_eq!(report.exit_code(Mode::ReportOnly, true), 0);
        assert_eq!(report.exit_code(Mode::Smoke, true), 0);
        report.legacy.as_mut().unwrap().probes[0].over_budget =
            Some((256, std::time::Duration::from_secs(21)));
        assert_eq!(report.failures(), 2);
        assert_eq!(report.exit_code(Mode::Gate, true), 1);
        assert!(
            report
                .validate(Scope::All, &["legacy"], &["different"])
                .is_err()
        );
        assert!(report.validate(Scope::Legacy, &["legacy"], &[]).is_err());
        report.source.as_mut().unwrap().results[0].cells.clear();
        assert!(
            report
                .validate(Scope::All, &["legacy"], &["source"])
                .is_err()
        );
        let missing = CombinedReport {
            legacy: None,
            source: None,
        };
        assert!(missing.validate(Scope::All, &[], &[]).is_err());
    }
}
