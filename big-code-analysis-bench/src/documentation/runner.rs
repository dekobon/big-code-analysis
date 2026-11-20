//! Source-specific raw-batch calibration, validation and typed statistics.
#![allow(clippy::cast_precision_loss)]

use std::collections::HashSet;
use std::fmt;
use std::time::{Duration, Instant};

use super::{HarnessError, Parsed, Prepared, SourceCounts, SourceProbe, StatisticClass, require};
use crate::cli::Mode;
use crate::scaling::{MAX_CELL_WALK, SMOKE_SIZES, WARMUP_ROUNDS, schedule};

/// Target elapsed duration for source-only calibration; legacy calibration is unchanged.
pub const SOURCE_TARGET_SAMPLE: Duration = Duration::from_millis(2);
/// Maximum source repetitions in one batch.
pub const SOURCE_MAX_ITERATIONS: u32 = 1 << 20;
/// A full-size cell with a shorter measured median is unresolved, never a pass.
pub const MIN_SOURCE_SAMPLE: Duration = Duration::from_micros(200);

/// Source calibration, frozen before warmup/measurement. A zero calibration
/// observation saturates the cap; measured zero batches still fail validation.
#[must_use]
pub fn iterations_for(single: Duration) -> u32 {
    let needed = SOURCE_TARGET_SAMPLE
        .as_nanos()
        .div_ceil(single.as_nanos().max(1));
    u32::try_from(needed)
        .unwrap_or(SOURCE_MAX_ITERATIONS)
        .clamp(1, SOURCE_MAX_ITERATIONS)
}

/// One actual scheduled batch, retained before any per-operation division.
#[derive(Clone, Debug)]
pub struct Batch {
    /// Measured-round ordinal, excluding warmup.
    pub round: usize,
    /// Actual elapsed duration of all repetitions.
    pub elapsed: Duration,
}

/// A complete validated cell with raw batches and fractional ns/operation.
#[derive(Clone, Debug)]
pub struct SourceCell {
    /// Input/output dimensions observed outside measurement.
    pub counts: SourceCounts,
    /// Declared size rung.
    pub size: usize,
    /// Actual held source bytes.
    pub bytes: usize,
    /// Map segments used by logarithmic normalization, zero when unavailable.
    pub segments: usize,
    /// Frozen repetitions for every warmup/measured round.
    pub iterations: u32,
    /// Complete samples in scheduled-round order.
    pub batches: Vec<Batch>,
    /// Shortest actual batch.
    pub batch_min: Duration,
    /// Median actual batch.
    pub batch_median: Duration,
    /// Longest actual batch.
    pub batch_max: Duration,
    /// Median batch divided in floating point, without a Duration floor.
    pub ns_per_operation: f64,
}

/// Class-specific result. A ratio is never stored in a legacy exponent field.
#[derive(Clone, Copy, Debug)]
pub enum Statistic {
    /// Least-squares log-size slope.
    Exponent(f64),
    /// Largest/smallest per-rung median.
    FixedOutputRatio(f64),
    /// Largest/smallest median after division by log2(S+1).
    LogMapRatio(f64),
}

impl Statistic {
    /// Numeric value interpreted according to this statistic's variant.
    #[must_use]
    pub fn value(self) -> f64 {
        match self {
            Self::Exponent(x) | Self::FixedOutputRatio(x) | Self::LogMapRatio(x) => x,
        }
    }
}

impl fmt::Display for Statistic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Exponent(_) => "exponent",
            Self::FixedOutputRatio(_) => "fixed-K ratio",
            Self::LogMapRatio(_) => "log-map ratio",
        };
        write!(f, "{name} {:.4}", self.value())
    }
}

/// A source ladder with a class-correct statistic and complete membership.
#[derive(Clone, Debug)]
pub struct SourceResult {
    /// Registry identity.
    pub id: &'static str,
    /// All declared rungs in declaration order.
    pub cells: Vec<SourceCell>,
    /// Statistic derived from raw batches.
    pub statistic: Statistic,
    /// Predeclared bound.
    pub bound: f64,
    /// Varied and fixed dimensions.
    pub dimensions: &'static str,
    /// Declared operation, preparation and destruction boundary.
    pub timing: &'static str,
    /// Source of the predeclared class/bound policy.
    pub policy: &'static str,
}

/// Validated source report, kept separate from the legacy result collection.
#[derive(Clone, Debug)]
pub struct SourceReport {
    /// Measured rounds per cell.
    pub rounds: usize,
    /// Run purpose. Smoke/report-only never carry a performance pass.
    pub mode: Mode,
    /// Complete registry membership.
    pub results: Vec<SourceResult>,
}

impl SourceReport {
    pub(super) fn validate_data(&self) -> Result<(), HarnessError> {
        require(!self.results.is_empty(), "empty source report")?;
        validate_rounds(self.rounds)?;
        let mut ids = HashSet::new();
        for result in &self.results {
            require(ids.insert(result.id), "duplicate source result")?;
            require(
                result.cells.len() == 3 && result.bound.is_finite() && result.bound > 0.0,
                "invalid source result",
            )?;
            require(
                result.cells[0].size > 0
                    && result
                        .cells
                        .windows(2)
                        .all(|pair| pair[0].size.checked_mul(2) == Some(pair[1].size)),
                "invalid source result ladder",
            )?;
            for cell in &result.cells {
                let verified = reduce_cell(
                    RawCell {
                        counts: cell.counts,
                        id: result.id,
                        size: cell.size,
                        bytes: cell.bytes,
                        segments: cell.segments,
                        iterations: cell.iterations,
                        batches: cell.batches.clone(),
                    },
                    self.rounds,
                    self.mode,
                )?;
                require(
                    verified.batch_min == cell.batch_min
                        && verified.batch_median == cell.batch_median
                        && verified.batch_max == cell.batch_max
                        && verified.ns_per_operation.to_bits() == cell.ns_per_operation.to_bits(),
                    "source cell summary differs from raw batches",
                )?;
            }
            let class = match result.statistic {
                Statistic::Exponent(_) => StatisticClass::Exponent,
                Statistic::FixedOutputRatio(_) => StatisticClass::FixedOutputRatio,
                Statistic::LogMapRatio(_) => StatisticClass::LogMapRatio,
            };
            require(
                statistic(class, &result.cells)?.value().to_bits()
                    == result.statistic.value().to_bits(),
                "source statistic differs from validated cells",
            )?;
        }
        Ok(())
    }

    /// Bound failures only; harness errors prevent report construction entirely.
    #[must_use]
    pub fn failures(&self) -> Vec<&SourceResult> {
        self.results
            .iter()
            .filter(|r| {
                !r.bound.is_finite()
                    || !r.statistic.value().is_finite()
                    || r.statistic.value() > r.bound
            })
            .collect()
    }
}

impl fmt::Display for SourceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "source documentation: {} workloads, {} cells, {} median-of-scheduled rounds; {:?}",
            self.results.len(),
            self.results.iter().map(|r| r.cells.len()).sum::<usize>(),
            self.rounds,
            self.mode
        )?;
        for result in &self.results {
            if self.mode == Mode::Smoke {
                writeln!(
                    f,
                    "{}: structural smoke; no timing-resolution or performance claim",
                    result.id
                )?;
            } else {
                writeln!(
                    f,
                    "{}: {} (bound {:.2}){}",
                    result.id,
                    result.statistic,
                    result.bound,
                    if self.mode == Mode::ReportOnly {
                        " report-only; no performance verdict"
                    } else if result.statistic.value() <= result.bound {
                        " ok"
                    } else {
                        " OVER BOUND"
                    }
                )?;
            }
            writeln!(f, "  {}", result.dimensions)?;
            writeln!(f, "  timing: {}", result.timing)?;
            writeln!(f, "  policy: {}", result.policy)?;
            writeln!(
                f,
                "  size bytes segments iterations batch-min-us batch-median-us batch-max-us ns/operation"
            )?;
            for cell in &result.cells {
                writeln!(
                    f,
                    "  {} {} {} {} {:.3} {:.3} {:.3} {:.6}",
                    cell.size,
                    cell.bytes,
                    cell.segments,
                    cell.iterations,
                    cell.batch_min.as_secs_f64() * 1e6,
                    cell.batch_median.as_secs_f64() * 1e6,
                    cell.batch_max.as_secs_f64() * 1e6,
                    cell.ns_per_operation
                )?;
                write!(f, "    raw-batches-ns:")?;
                for batch in &cell.batches {
                    write!(f, " {}:{}", batch.round, batch.elapsed.as_nanos())?;
                }
                writeln!(f)?;
                writeln!(f, "    dimensions: {:?}", cell.counts)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct RawCell {
    counts: SourceCounts,
    id: &'static str,
    size: usize,
    bytes: usize,
    segments: usize,
    iterations: u32,
    batches: Vec<Batch>,
}

/// Run the complete supplied source registry with source-specific calibration.
///
/// # Errors
/// Rejects invalid registry/rounds, shape/operation failures, over-budget work,
/// malformed samples and under-resolved full-size cells. There is no retry.
pub fn run(
    probes: &[SourceProbe],
    rounds: usize,
    mode: Mode,
) -> Result<SourceReport, HarnessError> {
    run_with_budget(probes, rounds, mode, MAX_CELL_WALK)
}

/// Run with an explicit post-operation budget, primarily to test abandonment.
/// This cannot interrupt the first operation; an external campaign deadline is separate.
///
/// # Errors
/// As [`run`], including every abandoned cell as a harness failure.
#[allow(clippy::too_many_lines)] // Parse, index and prepared borrows must remain in separate ordered lifetime layers.
pub fn run_with_budget(
    probes: &[SourceProbe],
    rounds: usize,
    mode: Mode,
    budget: Duration,
) -> Result<SourceReport, HarnessError> {
    validate_registry(probes, rounds)?;
    let probes: Vec<_> = probes
        .iter()
        .map(|probe| {
            if mode == Mode::Smoke {
                SourceProbe {
                    sizes: SMOKE_SIZES,
                    ..*probe
                }
            } else {
                *probe
            }
        })
        .collect();
    let mut parsed = Vec::new();
    for probe in &probes {
        for size in probe.sizes {
            parsed.push(Parsed::new(probe, size).map_err(|e| context(probe.id, size, e))?);
        }
    }
    let mut indexes = Vec::new();
    for (at, parse) in parsed.iter().enumerate() {
        let started = Instant::now();
        indexes.push(
            parse
                .collect(&probes[at / 3])
                .map_err(|e| context(probes[at / 3].id, probes[at / 3].sizes[at % 3], e))?,
        );
        check_budget(
            probes[at / 3].id,
            probes[at / 3].sizes[at % 3],
            started.elapsed(),
            1,
            budget,
        )?;
    }
    let mut prepared = Vec::new();
    for (at, (parse, index)) in parsed.iter().zip(&indexes).enumerate() {
        let probe = &probes[at / 3];
        prepared.push(
            Prepared::new(probe, probe.sizes[at % 3], parse, index)
                .map_err(|e| context(probe.id, probe.sizes[at % 3], e))?,
        );
    }
    let mut raw = Vec::new();
    for (at, operation) in prepared.iter().enumerate() {
        let probe = &probes[at / 3];
        let size = probe.sizes[at % 3];
        let single = batch(operation, 1).map_err(|e| context(probe.id, size, e))?;
        check_budget(probe.id, size, single, 1, budget)?;
        raw.push(RawCell {
            counts: operation.counts(),
            id: probe.id,
            size,
            bytes: parsed[at].source().len(),
            segments: operation.segments(),
            iterations: if mode == Mode::Smoke {
                1
            } else {
                iterations_for(single)
            },
            batches: Vec::with_capacity(rounds),
        });
    }
    for (round, at) in schedule(raw.len(), rounds) {
        let cell = &raw[at];
        let elapsed = batch(&prepared[at], cell.iterations)
            .and_then(|elapsed| {
                check_budget(cell.id, cell.size, elapsed, cell.iterations, budget)?;
                require(
                    !elapsed.is_zero(),
                    format!(
                        "{} size {}: zero batch in round {round}",
                        cell.id, cell.size
                    ),
                )?;
                Ok(elapsed)
            })
            .map_err(|error| {
                HarnessError(format!(
                    "{}\nretained source batches: {raw:#?}",
                    context(cell.id, cell.size, error)
                ))
            })?;
        if round >= WARMUP_ROUNDS {
            raw[at].batches.push(Batch {
                round: round - WARMUP_ROUNDS,
                elapsed,
            });
        }
    }
    reduce(&probes, rounds, mode, raw.clone())
        .map_err(|error| HarnessError(format!("{error}\nretained source batches: {raw:#?}")))
}

fn batch(operation: &Prepared<'_, '_>, iterations: u32) -> Result<Duration, HarnessError> {
    let start = Instant::now();
    for _ in 0..iterations {
        operation.execute()?;
    }
    Ok(start.elapsed())
}

#[allow(clippy::needless_pass_by_value)] // map_err transfers ownership of the original diagnostic.
fn context(id: &str, size: usize, error: HarnessError) -> HarnessError {
    HarnessError(format!("{id} size {size}: {error}"))
}

fn check_budget(
    id: &str,
    size: usize,
    elapsed: Duration,
    iterations: u32,
    budget: Duration,
) -> Result<(), HarnessError> {
    require(
        elapsed.as_secs_f64() / f64::from(iterations) <= budget.as_secs_f64(),
        format!(
            "AbandonedCell: {id} size {size}, {iterations} iterations, batch {elapsed:?}, per-operation budget {budget:?}"
        ),
    )
}

fn validate_registry(probes: &[SourceProbe], rounds: usize) -> Result<(), HarnessError> {
    validate_rounds(rounds)?;
    require(!probes.is_empty(), "empty source registry")?;
    let mut ids = HashSet::new();
    for probe in probes {
        require(!probe.id.is_empty(), "empty source identifier")?;
        require(
            ids.insert(probe.id),
            format!("duplicate source ID {}", probe.id),
        )?;
        require(
            probe.sizes[0] > 0
                && probe.sizes[2] <= 1 << 20
                && probe
                    .sizes
                    .windows(2)
                    .all(|s| s[0].checked_mul(2) == Some(s[1])),
            format!("{}: invalid size ladder", probe.id),
        )?;
        require(
            probe.bound.is_finite() && probe.bound > 0.0,
            format!("{}: invalid bound", probe.id),
        )?;
    }
    Ok(())
}

fn validate_rounds(rounds: usize) -> Result<(), HarnessError> {
    require(
        rounds > 0 && rounds % 2 == 1 && rounds <= 1023,
        "source rounds must be odd in 1..=1023",
    )
}

fn reduce(
    probes: &[SourceProbe],
    rounds: usize,
    mode: Mode,
    raw: Vec<RawCell>,
) -> Result<SourceReport, HarnessError> {
    validate_registry(probes, rounds)?;
    require(raw.len() == probes.len() * 3, "missing/extra source cells")?;
    let mut results = Vec::new();
    let mut raw = raw.into_iter();
    for probe in probes {
        let mut cells = Vec::new();
        for size in probe.sizes {
            let cell = raw
                .next()
                .ok_or_else(|| HarnessError("missing source cell".into()))?;
            require(
                cell.id == probe.id && cell.size == size,
                format!(
                    "wrong source cell membership: expected {} size {size}, got {} size {}",
                    probe.id, cell.id, cell.size
                ),
            )?;
            cells.push(reduce_cell(cell, rounds, mode)?);
        }
        let statistic = statistic(probe.class, &cells)?;
        results.push(SourceResult {
            id: probe.id,
            cells,
            statistic,
            bound: probe.bound,
            dimensions: probe.dimensions,
            timing: probe.timing,
            policy: probe.policy,
        });
    }
    Ok(SourceReport {
        rounds,
        mode,
        results,
    })
}

fn reduce_cell(raw: RawCell, rounds: usize, mode: Mode) -> Result<SourceCell, HarnessError> {
    validate_rounds(rounds)?;
    require(
        raw.bytes > 0 && raw.iterations > 0 && raw.iterations <= SOURCE_MAX_ITERATIONS,
        format!("{}: invalid cell work/count", raw.id),
    )?;
    require(
        raw.batches.len() == rounds,
        format!("{} size {}: missing samples", raw.id, raw.size),
    )?;
    let mut times = Vec::new();
    for (round, batch) in raw.batches.iter().enumerate() {
        require(
            batch.round == round
                && !batch.elapsed.is_zero()
                && batch.elapsed.as_secs_f64().is_finite(),
            format!("{} size {}: invalid batch round {round}", raw.id, raw.size),
        )?;
        times.push(batch.elapsed);
    }
    times.sort_unstable();
    let median = times[rounds / 2];
    if mode != Mode::Smoke && median < MIN_SOURCE_SAMPLE {
        return Err(HarnessError(format!(
            "UnresolvedCell: {} size {}, iterations {}, actual median batch {median:?} < {MIN_SOURCE_SAMPLE:?}",
            raw.id, raw.size, raw.iterations
        )));
    }
    let ns = median.as_secs_f64() * 1e9 / f64::from(raw.iterations);
    require(
        ns.is_finite() && ns > 0.0,
        format!("{}: invalid per-operation median", raw.id),
    )?;
    Ok(SourceCell {
        counts: raw.counts,
        size: raw.size,
        bytes: raw.bytes,
        segments: raw.segments,
        iterations: raw.iterations,
        batches: raw.batches,
        batch_min: times[0],
        batch_median: median,
        batch_max: times[rounds - 1],
        ns_per_operation: ns,
    })
}

fn statistic(class: StatisticClass, cells: &[SourceCell]) -> Result<Statistic, HarnessError> {
    require(
        cells.len() == 3
            && cells
                .iter()
                .all(|c| c.ns_per_operation > 0.0 && c.ns_per_operation.is_finite()),
        "invalid source statistic inputs",
    )?;
    require(
        cells[0].size > 0 && cells.windows(2).all(|pair| pair[0].size < pair[1].size),
        "degenerate source statistic ladder",
    )?;
    let mut values = Vec::new();
    for cell in cells {
        let divisor = if class == StatisticClass::LogMapRatio {
            require(cell.segments > 0, "map query has no segments")?;
            (cell.segments as f64 + 1.0).log2()
        } else {
            1.0
        };
        values.push(cell.ns_per_operation / divisor);
    }
    let value = if class == StatisticClass::Exponent {
        // The source fit intentionally has no legacy minimum-time floor.
        let x: Vec<_> = cells.iter().map(|c| (c.size as f64).ln()).collect();
        let y: Vec<_> = values.iter().map(|v| v.ln()).collect();
        let mx = x.iter().sum::<f64>() / 3.0;
        let my = y.iter().sum::<f64>() / 3.0;
        x.iter()
            .zip(&y)
            .map(|(x, y)| (x - mx) * (y - my))
            .sum::<f64>()
            / x.iter().map(|x| (x - mx).powi(2)).sum::<f64>()
    } else {
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            / values.iter().copied().fold(f64::INFINITY, f64::min)
    };
    require(value.is_finite(), "nonfinite source statistic")?;
    Ok(match class {
        StatisticClass::Exponent => Statistic::Exponent(value),
        StatisticClass::FixedOutputRatio => Statistic::FixedOutputRatio(value),
        StatisticClass::LogMapRatio => Statistic::LogMapRatio(value),
    })
}

#[cfg(test)]
mod tests {
    use super::super::REGISTRY;
    use super::*;

    fn raw(probe: &SourceProbe, ns: [u64; 3]) -> Vec<RawCell> {
        probe
            .sizes
            .into_iter()
            .zip(ns)
            .map(|(size, ns)| RawCell {
                counts: SourceCounts::default(),
                id: probe.id,
                size,
                bytes: size,
                segments: size,
                iterations: 1,
                batches: (0..3)
                    .map(|round| Batch {
                        round,
                        elapsed: Duration::from_nanos(ns),
                    })
                    .collect(),
            })
            .collect()
    }

    #[test]
    fn calibration_caps_zero_and_preserves_ceiling() {
        assert_eq!(iterations_for(Duration::ZERO), 1 << 20);
        assert_eq!(iterations_for(Duration::from_nanos(3)), 666_667);
        assert_eq!(iterations_for(Duration::from_millis(3)), 1);
        assert_eq!(iterations_for(Duration::from_micros(1)), 2000);
    }

    #[test]
    fn actual_batches_control_resolution_even_at_iteration_cap() {
        let probe = REGISTRY[18];
        let mut cells = raw(&probe, [199_999; 3]);
        for cell in &mut cells {
            cell.iterations = SOURCE_MAX_ITERATIONS;
        }
        let error = reduce(&[probe], 3, Mode::Gate, cells).unwrap_err();
        assert!(error.0.contains("UnresolvedCell"));
        assert!(error.0.contains("1048576"));
        assert!(reduce(&[probe], 3, Mode::ReportOnly, raw(&probe, [199_999; 3])).is_err());
        assert!(reduce(&[probe], 3, Mode::Smoke, raw(&probe, [1; 3])).is_ok());
    }

    #[test]
    fn malformed_membership_and_samples_fail_in_every_mode() {
        let probe = REGISTRY[18];
        for mode in [Mode::Gate, Mode::ReportOnly, Mode::Smoke] {
            assert!(reduce(&[], 3, mode, vec![]).is_err());
            assert!(reduce(&[probe, probe], 3, mode, vec![]).is_err());
            assert!(reduce(&[probe], 3, mode, vec![]).is_err());
            for mutation in 0..9 {
                let mut cells = raw(&probe, [200_000; 3]);
                match mutation {
                    0 => cells[0].id = "unknown",
                    1 => cells[0].size *= 2,
                    2 => {
                        cells[0].batches.pop();
                    }
                    3 => cells[0].batches[1].round = 0,
                    4 => cells[0].batches[1].elapsed = Duration::ZERO,
                    5 => cells[0].iterations = 0,
                    6 => cells[0].iterations = SOURCE_MAX_ITERATIONS + 1,
                    7 => {
                        cells.push(cells[0].clone());
                    }
                    _ => cells.swap(0, 1),
                }
                assert!(
                    reduce(&[probe], 3, mode, cells).is_err(),
                    "mutation {mutation}, {mode:?}"
                );
            }
        }
    }

    #[test]
    fn ratios_keep_subnanosecond_values_and_report_performance_failure() {
        let probe = REGISTRY[18];
        let mut cells = raw(&probe, [200_000, 400_000, 800_000]);
        for cell in &mut cells {
            cell.iterations = SOURCE_MAX_ITERATIONS;
        }
        let report = reduce(&[probe], 3, Mode::Gate, cells).unwrap();
        assert!((report.results[0].statistic.value() - 4.0).abs() < 1e-12);
        assert_eq!(report.failures().len(), 1);
        assert!(report.results[0].cells[0].ns_per_operation < 1.0);
        assert_eq!(
            report.results[0].cells[0].batch_median,
            Duration::from_micros(200)
        );
        assert!(report.to_string().contains("fixed-K ratio"));
    }

    #[test]
    fn typed_statistics_reject_invalid_values_and_distinguish_growth() {
        let probe = REGISTRY[0];
        let report = reduce(
            &[probe],
            3,
            Mode::Gate,
            raw(&probe, [200_000, 400_000, 800_000]),
        )
        .unwrap();
        assert!((report.results[0].statistic.value() - 1.0).abs() < 1e-12);
        for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            let mut cells = report.results[0].cells.clone();
            cells[0].ns_per_operation = bad;
            assert!(statistic(StatisticClass::FixedOutputRatio, &cells).is_err());
        }
        let mut cells = report.results[0].cells.clone();
        for cell in &mut cells {
            cell.ns_per_operation = (cell.segments as f64 + 1.0).log2() * 0.125;
        }
        assert!(
            (statistic(StatisticClass::LogMapRatio, &cells)
                .unwrap()
                .value()
                - 1.0)
                .abs()
                < 1e-12
        );
        assert!(check_budget("probe", 32, Duration::from_nanos(1), 1, Duration::ZERO).is_err());
    }

    #[test]
    fn abandoned_source_operation_is_a_harness_error_in_all_modes() {
        let probe = SourceProbe {
            sizes: SMOKE_SIZES,
            ..REGISTRY[0]
        };
        for mode in [Mode::Gate, Mode::ReportOnly, Mode::Smoke] {
            let error = run_with_budget(&[probe], 1, mode, Duration::ZERO).unwrap_err();
            assert!(error.0.contains("AbandonedCell"), "{mode:?}: {error}");
        }
    }

    #[test]
    fn report_revalidates_batches_counts_and_statistic() {
        let probe = REGISTRY[18];
        let report = reduce(&[probe], 3, Mode::Gate, raw(&probe, [200_000; 3])).unwrap();
        assert!(report.validate_data().is_ok());
        for mutation in 0..5 {
            let mut invalid = report.clone();
            match mutation {
                0 => invalid.results[0].cells[0].ns_per_operation = f64::NAN,
                1 => invalid.results[0].statistic = Statistic::FixedOutputRatio(f64::INFINITY),
                2 => invalid.results[0].bound = f64::NAN,
                3 => invalid.results[0].cells[0].batches.clear(),
                _ => invalid.results[0].cells[0].iterations = SOURCE_MAX_ITERATIONS + 1,
            }
            assert!(invalid.validate_data().is_err(), "mutation {mutation}");
        }
    }

    #[test]
    fn source_round_and_size_configuration_is_bounded_before_work() {
        for rounds in [0, 2, 1025, usize::MAX] {
            assert!(validate_registry(&REGISTRY[..1], rounds).is_err());
        }
        assert!(REGISTRY[0].source(0).is_err());
        assert!(REGISTRY[0].source(usize::MAX).is_err());
        let duplicate = [REGISTRY[0], REGISTRY[0]];
        assert!(validate_registry(&duplicate, 3).is_err());
        let wrong_sizes = SourceProbe {
            sizes: [1, 1, 1],
            ..REGISTRY[0]
        };
        assert!(run(&[wrong_sizes], 1, Mode::Smoke).is_err());
    }

    #[test]
    fn median_uses_complete_scheduled_batches_without_rounding() {
        let probe = REGISTRY[18];
        let mut cells = raw(&probe, [200_001; 3]);
        for cell in &mut cells {
            cell.iterations = 3;
            cell.batches[0].elapsed = Duration::from_nanos(900_001);
            cell.batches[1].elapsed = Duration::from_nanos(200_002);
        }
        let report = reduce(&[probe], 3, Mode::Gate, cells).unwrap();
        let cell = &report.results[0].cells[0];
        assert_eq!(cell.batch_min, Duration::from_nanos(200_001));
        assert_eq!(cell.batch_median, Duration::from_nanos(200_002));
        assert_eq!(cell.batch_max, Duration::from_nanos(900_001));
        assert!((cell.ns_per_operation - 200_002.0 / 3.0).abs() < 1e-9);
        assert_eq!(cell.batches[0].elapsed, Duration::from_nanos(900_001));
        assert_eq!(
            cell.batches.iter().map(|b| b.round).collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn source_smoke_exercises_every_registered_cell() {
        let report = run(super::super::REGISTRY, 1, Mode::Smoke).unwrap();
        assert_eq!(report.results.len(), 46);
        assert_eq!(
            report.results.iter().map(|r| r.cells.len()).sum::<usize>(),
            138
        );
        assert!(!report.to_string().contains(" ok"));
    }

    #[test]
    fn source_schedule_executes_calibration_warmup_and_measured_work() {
        let probe = REGISTRY
            .iter()
            .find(|p| p.id == "documentation/cold/width")
            .unwrap();
        let before = big_code_analysis_ast::documentation::observations();
        let report = run(&[*probe], 1, Mode::Smoke).unwrap();
        let after = big_code_analysis_ast::documentation::observations();
        // Per rung: one initial index, one calibration, two warmups and one
        // measured collection. Each collection renders one document per sibling.
        assert_eq!(
            after.renders - before.renders,
            5 * SMOKE_SIZES.iter().sum::<usize>()
        );
        assert_eq!(report.results[0].cells.len(), 3);
        assert!(
            report.results[0]
                .cells
                .iter()
                .all(|c| c.batches.len() == 1 && c.iterations == 1)
        );
    }
}
