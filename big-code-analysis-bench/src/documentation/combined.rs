//! Combined membership and exit semantics without changing legacy report types.

use super::runner::SourceReport;
use super::{HarnessError, require};
use crate::cli::{Mode, Scope};
use crate::scaling;
use std::fmt::Write;

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

    /// Compact trailing diagnosis after both full tables. All failure rows are
    /// retained; tail-only excerpts can omit earlier rows and name the full log.
    #[must_use]
    pub fn final_summary(&self, mode: Mode) -> String {
        if mode != Mode::Gate {
            return format!("{mode:?}: no performance verdict\n");
        }
        let mut output = String::new();
        if let Some(report) = &self.legacy {
            for result in report.failures() {
                if let Some((size, elapsed)) = result.over_budget {
                    let _ = writeln!(
                        output,
                        "FAIL legacy {}: abandoned size {size}, duration {elapsed:?} > budget {:?}; {}",
                        result.name,
                        scaling::MAX_CELL_WALK,
                        result.rationale
                    );
                } else {
                    let _ = writeln!(
                        output,
                        "FAIL legacy {}: exponent {:.4} > bound {:.2}; {}",
                        result.name, result.exponent, result.max_exponent, result.rationale
                    );
                }
            }
        }
        if let Some(report) = &self.source {
            for result in report.failures() {
                let _ = writeln!(
                    output,
                    "FAIL source {}: {} > bound {:.2}; policy: {}",
                    result.id, result.statistic, result.bound, result.policy
                );
            }
        }
        let count = self.failures();
        if count > 0 {
            let _ = writeln!(
                output,
                "Tail excerpts are truncated if fewer than {count} FAIL rows appear; full logs: metric-walk-benchmarks artifact, target/bench/scaling*.log."
            );
        }
        let _ = writeln!(output, "combined gate: {count} performance failures");
        output
    }
}

#[cfg(test)]
mod tests {
    use super::super::runner::{Batch, SourceCell, SourceResult, Statistic};
    use super::*;

    #[cfg(unix)]
    fn legacy_failure(name: &'static str) -> scaling::ProbeReport {
        scaling::ProbeReport {
            name,
            exponent: 2.0,
            max_exponent: 1.5,
            rationale: "walk growth",
            over_budget: None,
            cells: [256, 512, 1024]
                .into_iter()
                .zip([200, 800, 3200])
                .map(|(size, micros)| {
                    let elapsed = std::time::Duration::from_micros(micros);
                    scaling::Cell {
                        size,
                        bytes: size,
                        reading: 1,
                        iterations: 1,
                        min: elapsed,
                        median: elapsed,
                        max: elapsed,
                    }
                })
                .collect(),
        }
    }

    fn source_reports(failing: bool) -> SourceReport {
        let results = super::super::REGISTRY
            .iter()
            .enumerate()
            .map(|(at, probe)| {
                let times = if failing && matches!(at, 0 | 18 | 19) {
                    [200_000, 800_000, 3_200_000]
                } else {
                    [200_000; 3]
                };
                let cells: Vec<_> = [256, 512, 1024]
                    .into_iter()
                    .zip(times)
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
                    .collect();
                SourceResult {
                    id: probe.id,
                    bound: 1.5,
                    dimensions: probe.dimensions,
                    timing: probe.timing,
                    policy: probe.policy,
                    statistic: super::super::runner::statistic(probe.class, &cells).unwrap(),
                    cells,
                }
            })
            .collect();
        SourceReport {
            rounds: 3,
            mode: Mode::Gate,
            results,
        }
    }

    #[cfg(unix)]
    #[test]
    #[allow(clippy::too_many_lines)] // One rendered log exercises mixed typed failures and the workflow's actual tail size.
    fn final_failure_tail_identifies_early_failures_and_each_statistic() {
        let source = source_reports(true);
        source.validate_data().unwrap();
        let legacy = scaling::Report {
            rounds: 3,
            probes: vec![
                legacy_failure("early-legacy"),
                scaling::ProbeReport {
                    name: "abandoned-legacy",
                    cells: vec![],
                    exponent: 0.0,
                    max_exponent: 1.5,
                    rationale: "walk budget",
                    over_budget: Some((256, std::time::Duration::from_secs(21))),
                },
            ],
        };
        let report = CombinedReport {
            legacy: Some(legacy),
            source: Some(source),
        };
        let full = format!(
            "{}{}{}",
            report.legacy.as_ref().unwrap(),
            report.source.as_ref().unwrap(),
            report.final_summary(Mode::Gate)
        );
        let lines: Vec<_> = full.lines().collect();
        let excerpt = lines[lines
            .len()
            .saturating_sub(super::super::workflow_tests::issue_tail_lines())..]
            .join("\n");
        for needle in [
            "FAIL legacy early-legacy: exponent 2.0000 > bound 1.50; walk growth",
            "FAIL legacy abandoned-legacy: abandoned size 256, duration 21s > budget 20s",
            "documentation/cold/depth: exponent 2.0000",
            "documentation/prepared/owner: fixed-K ratio 16.0000",
            "documentation/prepared/late-map: log-map ratio 16.0000",
            "policy:",
            "combined gate: 5 performance failures",
        ] {
            assert!(excerpt.contains(needle), "missing {needle}: {excerpt}");
        }
        assert_eq!(
            full.lines()
                .filter(|line| line.starts_with("FAIL "))
                .count(),
            5
        );
        assert!(excerpt.contains("metric-walk-benchmarks artifact"));
        assert!(excerpt.ends_with("combined gate: 5 performance failures"));
    }

    #[test]
    fn final_summary_distinguishes_all_pass_and_non_gating_output() {
        let source = source_reports(false);
        source.validate_data().unwrap();
        let report = CombinedReport {
            legacy: None,
            source: Some(source),
        };
        assert_eq!(
            report.final_summary(Mode::Gate),
            "combined gate: 0 performance failures\n"
        );
        for mode in [Mode::Smoke, Mode::ReportOnly] {
            let text = report.final_summary(mode);
            assert!(text.contains("no performance verdict"));
            assert!(!text.contains("combined gate"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn failure_tail_reports_when_the_excerpt_omits_rows() {
        let limit = super::super::workflow_tests::issue_tail_lines();
        let count = limit + 3;
        let legacy = scaling::Report {
            rounds: 3,
            probes: crate::shapes::PROBES
                .iter()
                .take(count)
                .map(|probe| legacy_failure(probe.name))
                .collect(),
        };
        assert_eq!(
            legacy.probes.len(),
            count,
            "fixture needs more unique legacy IDs than the workflow tail"
        );
        let report = CombinedReport {
            legacy: Some(legacy),
            source: None,
        };
        let full = report.final_summary(Mode::Gate);
        assert_eq!(
            full.lines().filter(|l| l.starts_with("FAIL ")).count(),
            count
        );
        let lines: Vec<_> = full.lines().collect();
        let tail = lines[lines.len() - limit..].join("\n");
        assert!(tail.contains(&format!("truncated if fewer than {count} FAIL rows appear")));
        assert!(tail.contains("target/bench/scaling*.log"));
        assert!(tail.ends_with(&format!("combined gate: {count} performance failures")));
        assert!(tail.lines().filter(|l| l.starts_with("FAIL ")).count() < count);
    }

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
