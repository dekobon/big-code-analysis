//! Interleaved measurement of the depth-scaling probes, and the
//! complexity-class fit derived from it.
//!
//! # What is asserted, and why it is not a duration
//!
//! The useful property is "doubling the nesting depth roughly doubles
//! the cost", not "this finishes within N milliseconds". An absolute
//! budget is calibrated to one machine; the same assertion produced
//! four false failures during the #1052 / #1062 work — on
//! `windows-latest`, under `cargo llvm-cov`, and twice on a local host
//! running the rest of the validation gate alongside it.
//!
//! A ratio between two depths is host-independent, but it is not
//! load-independent: the two measurements are sequential, so a load
//! spike between them skews the ratio by itself. Two things fix that
//! here.
//!
//! - **Interleaving.** Every (probe, depth) cell is measured once per
//!   round, and the visit order is rotated each round. Contention that
//!   arrives mid-run lands on all cells rather than on whichever one
//!   happened to be running, so it inflates the readings without
//!   tilting the ratio between them.
//! - **Three points, not two.** Each probe is measured at `d`, `2d`
//!   and `4d`, and the reported figure is the slope of `ln(time)`
//!   against `ln(depth)` — an exponent, near 1.0 for a linear walk and
//!   near 2.0 for a quadratic one. A single pair of timings cannot
//!   distinguish "twice as slow because quadratic" from "twice as slow
//!   because busy".
//!
//! Parsing is excluded: each cell parses once up front and the timed
//! region is [`Ast::metrics`] alone. This harness is about the metric
//! walk, and `tree_sitter`'s own depth behaviour would otherwise be
//! folded into the exponent.
#![allow(
    // Every cast here is a measurement (`Duration` -> f64 nanoseconds,
    // byte counts -> f64) feeding a ratio or a logarithm. Precision
    // loss at f64 is irrelevant at these magnitudes and truncation is
    // impossible: the inputs are non-negative and bounded by the
    // measurement itself.
    clippy::cast_precision_loss
)]

use std::fmt;
use std::hint::black_box;
use std::time::{Duration, Instant};

use big_code_analysis::{Ast, MetricsError, MetricsOptions, Source};

use crate::shapes::Probe;

/// Measurement rounds performed when the caller does not choose.
///
/// Odd, so the median is an observed sample rather than an average of
/// two. Seven is enough for the median to shed a couple of contended
/// rounds while keeping a full run of [`crate::shapes::PROBES`] inside
/// a handful of seconds.
pub const DEFAULT_ROUNDS: usize = 7;

// An even default would make every median the average of two samples,
// quietly reintroducing the sensitivity to a single slow round that
// the median is there to remove.
const _: () = assert!(DEFAULT_ROUNDS % 2 == 1);

/// Rounds discarded before measurement starts.
///
/// The first pass over a cell pays for cold instruction cache, lazy
/// page faults on the freshly parsed tree, and CPU frequency ramp. Two
/// throwaway rounds are cheap next to the cost of having them land in
/// the median.
pub const WARMUP_ROUNDS: usize = 2;

/// Floor applied to a measured duration before it is logged.
///
/// A cell that reads as zero nanoseconds would produce `ln(0)` and
/// poison the fit. One nanosecond is below the resolution of any
/// platform clock this runs on, so the floor can only ever replace a
/// reading that was already meaningless.
const MIN_MEASURABLE_NS: f64 = 1.0;

/// Duration a single timed sample aims for.
///
/// The cheapest cells run in a few hundred microseconds, where clock
/// resolution and scheduler granularity are a visible fraction of the
/// reading — the shallowest `tokens/nested-paren` cell swung 0.21 to
/// 0.49 ms across rounds before this existed. Repeating the walk
/// inside one timed region until it reaches a millisecond amortises
/// that away without changing what is being measured.
const TARGET_SAMPLE: Duration = Duration::from_millis(1);

/// Ceiling on inner repetitions per sample.
///
/// Guards the total runtime: without it, a cell that got dramatically
/// faster would silently multiply its repetition count instead of
/// simply finishing sooner.
const MAX_ITERATIONS: u32 = 64;

/// Wall clock one walk may take before the harness stops deepening a
/// probe.
///
/// The failure mode this exists for is the one the retired unit-test
/// budgets were guarding against: a reintroduced quadratic walk does
/// not fail, it *hangs*. The pre-#1052 `tokens` implementation took
/// over two minutes at depth 2000, so measuring the same probe at
/// 1000, 2000 and 4000 for nine rounds each would have run for hours
/// and tripped a CI timeout instead of reporting a regression.
///
/// Cells are built in increasing depth order, so a probe that blows
/// past this at one depth is abandoned before the deeper ones are
/// attempted, and reported as over budget — which counts as a failure,
/// since a walk this slow is the regression. An abandoned probe
/// contributes *no* cells to the measurement schedule, not merely no
/// deeper ones: the verdict no longer depends on its exponent, so
/// measuring the shallower cells would multiply an input already known
/// to be pathological by `rounds + WARMUP_ROUNDS` for nothing.
///
/// Twenty seconds is chosen against the magnitudes on record rather
/// than as a round number: the pre-#1052 `tokens` walk cost ~19 s at
/// depth 1000 and over two minutes at 2000, so this abandons that
/// regression at its second cell. It does not bound the *first* walk
/// of a probe, which is unavoidable — the harness cannot know a walk
/// is slow until it finishes one.
pub const MAX_CELL_WALK: Duration = Duration::from_secs(20);

/// One (probe, depth) cell, reduced across rounds.
#[derive(Debug, Clone)]
pub struct Cell {
    /// Nesting depth of the generated input.
    pub depth: usize,
    /// Size of the generated input. Reported so a reader can confirm
    /// the input grew linearly and check cost per byte directly.
    pub bytes: usize,
    /// Headline metric value the walk produced at this depth.
    pub reading: u64,
    /// Walks performed inside one timed sample. Reported so a reader
    /// can tell an amortised measurement from a directly observed one.
    pub iterations: u32,
    /// Fastest round.
    pub min: Duration,
    /// Median round — the figure the fit uses.
    pub median: Duration,
    /// Slowest round. A wide min-max spread is the signal that the
    /// host was too busy for the run to mean anything.
    pub max: Duration,
}

/// Every cell for one probe, plus the exponent fitted across them.
#[derive(Debug, Clone)]
pub struct ProbeReport {
    /// The probe's stable identifier.
    pub name: &'static str,
    /// Cells in increasing depth order.
    pub cells: Vec<Cell>,
    /// Fitted slope of `ln(median time)` against `ln(depth)`.
    pub exponent: f64,
    /// Bound the probe declared for that slope.
    pub max_exponent: f64,
    /// The probe's rationale, echoed into the report so a failure is
    /// self-explanatory without opening the source.
    pub rationale: &'static str,
    /// Set when a single walk exceeded [`MAX_CELL_WALK`], carrying the
    /// depth it happened at and how long it took. An abandoned probe
    /// contributes no cells at all, so [`ProbeReport::cells`] is empty
    /// and [`ProbeReport::exponent`] is `0.0` — flattering, and not the
    /// verdict. This is.
    pub over_budget: Option<(usize, Duration)>,
}

impl ProbeReport {
    /// Whether the probe stayed within both its complexity bound and
    /// the per-walk time budget.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.over_budget.is_none() && self.exponent <= self.max_exponent
    }
}

/// A full run of the depth-scaling probes.
#[derive(Debug, Clone)]
pub struct Report {
    /// Measurement rounds actually performed.
    pub rounds: usize,
    /// One entry per probe, in declaration order.
    pub probes: Vec<ProbeReport>,
}

impl Report {
    /// Probes that did not [`ProbeReport::passed`]: a fitted exponent
    /// over the declared bound, or a walk abandoned for exceeding
    /// [`MAX_CELL_WALK`] — the latter can carry a flattering exponent,
    /// so callers must report both reasons rather than only the bound.
    #[must_use]
    pub fn failures(&self) -> Vec<&ProbeReport> {
        self.probes.iter().filter(|p| !p.passed()).collect()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "metric-walk depth scaling ({} rounds)", self.rounds)?;
        for probe in &self.probes {
            writeln!(f)?;
            // An abandoned probe has no exponent worth printing: it is
            // fitted over the cells that finished, which for an
            // abandoned probe is none, so the header would read
            // `exponent 0.00 (bound 1.50) OVER BOUND` — a passing
            // number next to a failing verdict, for the worst
            // regression this gate can see.
            if let Some((depth, elapsed)) = probe.over_budget {
                writeln!(
                    f,
                    "{name}  ABANDONED  one walk at depth {depth} took {elapsed:?}, \
                     over the {MAX_CELL_WALK:?} budget",
                    name = probe.name,
                )?;
                writeln!(f, "  {}", probe.rationale)?;
                continue;
            }
            writeln!(
                f,
                "{name}  exponent {exp:.2} (bound {bound:.2})  {verdict}",
                name = probe.name,
                exp = probe.exponent,
                bound = probe.max_exponent,
                verdict = if probe.passed() { "ok" } else { "OVER BOUND" },
            )?;
            writeln!(
                f,
                "  {:>7}  {:>9}  {:>10}  {:>10}  {:>10}  {:>9}  {:>5}  {:>12}",
                "depth", "bytes", "median ms", "min ms", "max ms", "ns/byte", "iter", "reading",
            )?;
            for cell in &probe.cells {
                writeln!(
                    f,
                    "  {depth:>7}  {bytes:>9}  {median:>10.3}  {min:>10.3}  \
                     {max:>10.3}  {per_byte:>9.2}  {iterations:>5}  {reading:>12}",
                    depth = cell.depth,
                    bytes = cell.bytes,
                    median = millis(cell.median),
                    min = millis(cell.min),
                    max = millis(cell.max),
                    per_byte = cell.median.as_nanos() as f64 / cell.bytes as f64,
                    iterations = cell.iterations,
                    reading = cell.reading,
                )?;
            }
            if !probe.passed() {
                writeln!(f, "  {}", probe.rationale)?;
            }
        }
        Ok(())
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

/// A cell under measurement: the parsed input plus its accumulating
/// timings.
struct Pending {
    probe: usize,
    depth: usize,
    bytes: usize,
    reading: u64,
    iterations: u32,
    ast: Ast,
    options: MetricsOptions,
    timings: Vec<Duration>,
}

/// Repetitions a single timed sample needs to reach
/// [`TARGET_SAMPLE`], given one observed walk.
fn iterations_for(single_walk: Duration) -> u32 {
    let observed = single_walk.as_nanos().max(1);
    let needed = TARGET_SAMPLE.as_nanos().div_ceil(observed);
    // A walk so fast that `needed` overflows `u32` cannot happen at
    // these depths, but saturating at the cap is the right answer for
    // it either way.
    u32::try_from(needed)
        .unwrap_or(MAX_ITERATIONS)
        .clamp(1, MAX_ITERATIONS)
}

/// Depths substituted into every probe for a smoke run.
///
/// Two orders of magnitude under the real ones. A smoke run answers
/// "does the harness still work", not "how does the walk scale", and
/// it happens in contexts (`cargo test`, an unoptimised build) where
/// the production depths would cost tens of seconds and produce a
/// number nobody should read.
pub const SMOKE_DEPTHS: [usize; 3] = [32, 64, 128];

/// Copies `probes` with [`SMOKE_DEPTHS`] substituted.
#[must_use]
pub fn smoke_probes(probes: &[Probe]) -> Vec<Probe> {
    probes
        .iter()
        .map(|probe| Probe {
            depths: SMOKE_DEPTHS,
            ..*probe
        })
        .collect()
}

/// Measures every probe at every depth and fits a complexity exponent.
///
/// `rounds` measurement rounds are performed after
/// [`WARMUP_ROUNDS`] discarded ones. Every cell is visited once per
/// round, with the visit order rotated each round so no cell sits at a
/// fixed position in the schedule.
///
/// A probe whose walk exceeds [`MAX_CELL_WALK`] at one depth is
/// reported as over budget, and neither that cell nor the deeper ones
/// are measured.
///
/// # Errors
///
/// Returns the first [`MetricsError`] raised while parsing a generated
/// shape or walking it — in practice, a language feature disabled in
/// the build.
pub fn run(probes: &[Probe], rounds: usize) -> Result<Report, MetricsError> {
    run_with_budget(probes, rounds, MAX_CELL_WALK)
}

/// [`run`], with the per-walk budget supplied by the caller.
///
/// Exists so the abandon path is testable: a budget of zero abandons
/// every probe at its shallowest depth in milliseconds, where
/// reproducing it through [`MAX_CELL_WALK`] would need a walk that
/// genuinely takes twenty seconds.
///
/// # Errors
///
/// As [`run`].
pub fn run_with_budget(
    probes: &[Probe],
    rounds: usize,
    max_cell_walk: Duration,
) -> Result<Report, MetricsError> {
    let mut pending = Vec::new();
    let mut over_budget: Vec<Option<(usize, Duration)>> = vec![None; probes.len()];
    for (index, probe) in probes.iter().enumerate() {
        // A probe's cells accumulate here and reach the measurement
        // schedule only if the probe finished. Abandoning it must drop
        // the cells it already built, not just stop it deepening:
        // `passed()` ignores the exponent once `over_budget` is set, so
        // measuring them changes no verdict and costs `rounds +
        // WARMUP_ROUNDS` walks of an input already known to be
        // pathological. Accumulating locally makes that structural
        // rather than a cleanup someone has to remember.
        let mut cells = Vec::new();
        // Ascending depth, so an intractably slow walk is caught at the
        // cheapest depth and the deeper cells are never attempted.
        for depth in probe.depths {
            let source = (probe.render)(depth);
            let ast = Ast::parse(Source::new(probe.lang, source.as_bytes()))?;
            let options = MetricsOptions::default().with_only(probe.metrics);
            // One untimed walk serves three purposes: it proves the
            // metric selection works on this shape, it produces the
            // reading the report shows, and its duration sizes the
            // inner repetition count.
            let started = Instant::now();
            let space = ast.metrics(options)?;
            let single_walk = started.elapsed();
            if single_walk > max_cell_walk {
                over_budget[index] = Some((depth, single_walk));
                break;
            }
            cells.push(Pending {
                probe: index,
                depth,
                bytes: source.len(),
                reading: (probe.reading)(&space.metrics),
                iterations: iterations_for(single_walk),
                ast,
                options,
                timings: Vec::with_capacity(rounds),
            });
        }
        if over_budget[index].is_none() {
            pending.append(&mut cells);
        }
    }

    let cell_count = pending.len();
    for round in 0..(rounds + WARMUP_ROUNDS) {
        for offset in 0..cell_count {
            // Rotating the visit order spreads any drift or contention
            // over every cell instead of concentrating it on whichever
            // one always runs last.
            let cell = &mut pending[(offset + round) % cell_count];
            let started = Instant::now();
            for _ in 0..cell.iterations {
                let space = cell.ast.metrics(cell.options)?;
                black_box(&space);
            }
            let elapsed = started.elapsed() / cell.iterations;
            if round >= WARMUP_ROUNDS {
                cell.timings.push(elapsed);
            }
        }
    }

    Ok(Report {
        rounds,
        probes: probes
            .iter()
            .enumerate()
            .map(|(index, probe)| {
                reduce(
                    probe,
                    over_budget[index],
                    pending.iter_mut().filter(|c| c.probe == index),
                )
            })
            .collect(),
    })
}

fn reduce<'a>(
    probe: &Probe,
    over_budget: Option<(usize, Duration)>,
    cells: impl Iterator<Item = &'a mut Pending>,
) -> ProbeReport {
    let cells: Vec<Cell> = cells
        .map(|pending| {
            pending.timings.sort_unstable();
            Cell {
                depth: pending.depth,
                bytes: pending.bytes,
                reading: pending.reading,
                iterations: pending.iterations,
                min: pending.timings.first().copied().unwrap_or_default(),
                median: median(&pending.timings),
                max: pending.timings.last().copied().unwrap_or_default(),
            }
        })
        .collect();

    let points: Vec<(f64, f64)> = cells
        .iter()
        .map(|cell| {
            (
                cell.depth as f64,
                (cell.median.as_nanos() as f64).max(MIN_MEASURABLE_NS),
            )
        })
        .collect();

    ProbeReport {
        name: probe.name,
        exponent: fit_exponent(&points),
        max_exponent: probe.max_exponent,
        rationale: probe.rationale,
        over_budget,
        cells,
    }
}

/// Median of an already-sorted slice of durations.
///
/// An even-length slice averages the two middle samples; an empty one
/// is zero, which only arises from a zero-round run.
#[must_use]
pub fn median(sorted: &[Duration]) -> Duration {
    match sorted.len() {
        0 => Duration::ZERO,
        len if len % 2 == 1 => sorted[len / 2],
        len => (sorted[len / 2 - 1] + sorted[len / 2]) / 2,
    }
}

/// Fits `time ~ depth^k` by least squares on the log-log points and
/// returns `k`.
///
/// Returns `0.0` for fewer than two points, or when every point shares
/// one depth — there is no slope to recover and reporting a fabricated
/// one would read as a pass.
#[must_use]
pub fn fit_exponent(points: &[(f64, f64)]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let logs: Vec<(f64, f64)> = points
        .iter()
        .map(|&(x, y)| (x.max(1.0).ln(), y.max(MIN_MEASURABLE_NS).ln()))
        .collect();
    let n = logs.len() as f64;
    let mean_x = logs.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = logs.iter().map(|p| p.1).sum::<f64>() / n;
    let covariance: f64 = logs.iter().map(|&(x, y)| (x - mean_x) * (y - mean_y)).sum();
    let variance: f64 = logs.iter().map(|&(x, _)| (x - mean_x).powi(2)).sum();
    if variance == 0.0 {
        return 0.0;
    }
    covariance / variance
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        ProbeReport, Report, SMOKE_DEPTHS, fit_exponent, median, run, run_with_budget, smoke_probes,
    };
    use crate::shapes::PROBES;

    /// Perfectly linear data fits an exponent of 1.
    #[test]
    fn fit_recovers_linear() {
        let points = [(1_000.0, 1_000.0), (2_000.0, 2_000.0), (4_000.0, 4_000.0)];
        assert!((fit_exponent(&points) - 1.0).abs() < 1e-9);
    }

    /// Perfectly quadratic data fits an exponent of 2.
    #[test]
    fn fit_recovers_quadratic() {
        let points = [(1_000.0, 1_000.0), (2_000.0, 4_000.0), (4_000.0, 16_000.0)];
        assert!((fit_exponent(&points) - 2.0).abs() < 1e-9);
    }

    /// A constant-cost walk fits an exponent of 0 — the reading a
    /// probe would produce if its shape stopped nesting.
    #[test]
    fn fit_recovers_constant() {
        let points = [(1_000.0, 500.0), (2_000.0, 500.0), (4_000.0, 500.0)];
        assert!(fit_exponent(&points).abs() < 1e-9);
    }

    /// Degenerate inputs return 0 rather than a fabricated slope: too
    /// few points, and points that share a single depth.
    ///
    /// Exact equality is the assertion: `fit_exponent` returns the
    /// literal `0.0` on these paths rather than computing a value that
    /// happens to be near zero, so a tolerance would let a genuine
    /// (tiny) fitted slope pass as the refusal.
    #[allow(clippy::float_cmp)]
    #[test]
    fn fit_refuses_degenerate_input() {
        assert_eq!(fit_exponent(&[]), 0.0);
        assert_eq!(fit_exponent(&[(1_000.0, 5.0)]), 0.0);
        assert_eq!(fit_exponent(&[(1_000.0, 5.0), (1_000.0, 50.0)]), 0.0);
    }

    /// The fit is scale-free: multiplying every timing by a constant
    /// (a slower host, an instrumented build) leaves the exponent
    /// untouched. This is the property that makes the gate portable
    /// where a wall-clock budget was not.
    #[test]
    fn fit_is_invariant_under_uniform_slowdown() {
        let base = [(1_000.0, 700.0), (2_000.0, 1_400.0), (4_000.0, 2_800.0)];
        let slowed: Vec<(f64, f64)> = base.iter().map(|&(x, y)| (x, y * 37.5)).collect();
        assert!((fit_exponent(&base) - fit_exponent(&slowed)).abs() < 1e-9);
    }

    /// A probe abandoned for exceeding the per-walk budget fails even
    /// though its truncated cell set fits a flattering exponent.
    ///
    /// This is the case that matters: the abandoned probe is the one
    /// whose walk got catastrophically slower, and fewer points make
    /// `fit_exponent` *more* likely to return something innocuous
    /// (`0.0` for a single cell). Without `over_budget` in the verdict,
    /// the worst possible regression would report as a pass.
    #[test]
    fn an_over_budget_probe_fails_regardless_of_its_exponent() {
        let mut report = ProbeReport {
            name: "probe",
            cells: Vec::new(),
            exponent: 0.0,
            max_exponent: 1.5,
            rationale: "",
            over_budget: None,
        };
        assert!(report.passed(), "a flat exponent under bound is a pass");

        report.over_budget = Some((1_000, Duration::from_secs(45)));
        assert!(!report.passed(), "an abandoned probe must never pass");

        let full = Report {
            rounds: 1,
            probes: vec![report],
        };
        assert_eq!(full.failures().len(), 1);
        let rendered = format!("{full}");
        assert!(
            rendered.contains("ABANDONED"),
            "the report must say the probe was abandoned:\n{rendered}",
        );
        // The exponent of an abandoned probe is fitted over the cells
        // that finished — none — so it reads `0.00`, under any bound.
        // Printing it beside the failing verdict produced
        // `exponent 0.00 (bound 1.50) OVER BOUND`: a passing number
        // next to a failing one, for the worst regression the gate can
        // see. It must not appear at all.
        assert!(
            !rendered.contains("exponent"),
            "an abandoned probe must not report a flattering exponent:\n{rendered}",
        );
    }

    /// An over-budget cell is dropped, not measured.
    ///
    /// The budget exists because a reintroduced quadratic walk hangs
    /// rather than fails; retaining the offending cell would walk it
    /// once per round and multiply the very cost being escaped. A
    /// zero budget abandons every probe at its shallowest depth, so
    /// the invariant is checked without a genuinely slow walk: no
    /// cells at all, and the run finishes fast enough to sit in the
    /// unit suite.
    #[test]
    fn an_over_budget_cell_is_never_measured() {
        let probes = smoke_probes(PROBES);
        let report = run_with_budget(&probes, 3, Duration::ZERO)
            .expect("every probe language is compiled in");
        for probe in &report.probes {
            assert!(
                probe.cells.is_empty(),
                "{}: abandoned at depth {:?} but kept {} cell(s) to measure",
                probe.name,
                probe.over_budget.map(|(depth, _)| depth),
                probe.cells.len(),
            );
            assert!(
                probe.over_budget.is_some(),
                "{}: a zero budget must abandon the shallowest cell",
                probe.name,
            );
            assert!(!probe.passed(), "{}: an abandoned probe fails", probe.name);
        }
    }

    #[test]
    fn median_picks_the_middle_sample() {
        let odd = [
            Duration::from_millis(1),
            Duration::from_millis(4),
            Duration::from_millis(90),
        ];
        assert_eq!(median(&odd), Duration::from_millis(4));

        let even = [
            Duration::from_millis(2),
            Duration::from_millis(4),
            Duration::from_millis(6),
            Duration::from_millis(100),
        ];
        assert_eq!(median(&even), Duration::from_millis(5));

        assert_eq!(median(&[]), Duration::ZERO);
    }

    /// A one-round run over the real probe set produces a complete,
    /// well-formed report.
    ///
    /// Runs at [`SMOKE_DEPTHS`]: the scheduling, reduction and
    /// reporting logic under test is depth-independent, and the
    /// production depths would add half a minute to every `cargo
    /// test`. The real depths are exercised by `benches/scaling.rs`
    /// under the bench profile.
    ///
    /// Deliberately does **not** assert on the exponents: this runs
    /// inside the ordinary (unoptimised, possibly instrumented,
    /// possibly contended) test suite, which is precisely where a
    /// timing assertion has already produced four false failures. The
    /// gate lives in `benches/scaling.rs`.
    #[test]
    fn run_produces_a_cell_per_probe_depth() {
        let probes = smoke_probes(PROBES);
        let report = run(&probes, 1).expect("every probe language is compiled in");
        assert_eq!(report.probes.len(), PROBES.len());
        for (result, probe) in report.probes.iter().zip(PROBES) {
            assert_eq!(result.name, probe.name);
            assert_eq!(result.cells.len(), SMOKE_DEPTHS.len());
            for (cell, depth) in result.cells.iter().zip(SMOKE_DEPTHS) {
                assert_eq!(cell.depth, depth);
                assert!(
                    cell.bytes > 0,
                    "{}: empty input at depth {depth}",
                    probe.name
                );
                assert!(
                    cell.reading > 0,
                    "{}: zero metric reading at depth {depth}",
                    probe.name,
                );
                assert!(cell.min <= cell.median && cell.median <= cell.max);
            }
        }
    }
}
