//! Scaling probes for the change-history ranking pass (#1267).
//!
//! `vcs::score::apply_percentile` is the one hot path the gate times
//! that is not a walk over an AST: its input is a vector of per-file
//! [`Stats`], and its cost is a function of how many files are ranked.
//! The probes here grow that count and fit the same `time ~ size^k`
//! exponent [`crate::scaling`] fits for the metric walks, so a
//! reintroduced quadratic ranking fails the gate the same way a
//! quadratic walk does.
//!
//! The population shapes are chosen for the tie structure they put in
//! front of the ranking, since that is what a mid-rank implementation
//! has to get right: every signal identical, every signal drawn from a
//! handful of values so each file shares its rank with many others,
//! and a spread with few ties. A ranking whose cost depends on the tie
//! structure — as a per-group scan would — shows up as a different
//! exponent per shape.

use std::hint::black_box;
use std::time::{Duration, Instant};

use big_code_analysis::vcs::Stats;
use big_code_analysis::vcs::score::apply_percentile;

use crate::scaling::{MAX_CELL_WALK, Measured, ProbeReport, Report, iterations_for, schedule};

/// The shape of the ranked population.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Population {
    /// Every file carries the same signals: one tie group per signal
    /// spanning the whole population, the shape the bug-fix and
    /// security-fix signals take on most real repositories.
    AllEqual,
    /// Each signal is drawn from a small fixed value set (two to seventy
    /// distinct values, by signal), so every file shares each rank with
    /// a large slice of the population.
    DuplicateHeavy,
    /// Each signal is spread across the population with few exact ties.
    Spread,
}

impl Population {
    /// Builds `size` records of this shape.
    ///
    /// Deterministic: the same `(shape, size)` always yields the same
    /// records, so two builds of the harness measure the same input.
    #[must_use]
    pub fn render(self, size: usize) -> Vec<Stats> {
        (0..size).map(|i| self.record(i, size)).collect()
    }

    #[allow(clippy::cast_precision_loss)]
    fn record(self, i: usize, size: usize) -> Stats {
        let i = i as u64;
        let (n, k) = match self {
            Self::AllEqual => (7, 0),
            Self::DuplicateHeavy => (i % 10, i % 7),
            Self::Spread => (i, (i * 7_919) % size as u64),
        };
        // `n` and `k` are bounded by the record index, which is bounded by
        // the ladder in `RANKING_SIZES` (8 000); the saturation is the
        // documented behaviour for a caller that renders a population
        // larger than `u32`, and the arithmetic below saturates with it
        // so no field can wrap.
        let count = u32::try_from(n).unwrap_or(u32::MAX);
        let variant = u32::try_from(k).unwrap_or(u32::MAX);
        Stats {
            churn_recent: n.saturating_mul(13),
            churn_long: n.saturating_mul(101).saturating_add(k),
            commits_recent: count,
            commits_long: count.saturating_mul(3).saturating_add(variant),
            authors_long: variant.saturating_add(1),
            ownership_top_share: 1.0 / (f64::from(variant) + 1.0),
            bug_fix_commits: variant % 3,
            security_fix_commits: count % 2,
            change_entropy_recent: n as f64 * 0.125,
            cochange_entropy_recent: k as f64 * 0.25,
            ..Stats::default()
        }
    }
}

/// One scaling probe over the ranking pass.
#[derive(Clone, Copy)]
pub struct VcsProbe {
    /// Stable `vcs/<shape>` identifier, used as the report key.
    pub name: &'static str,
    /// Shape of the ranked population.
    pub population: Population,
    /// File counts measured, each a doubling of the previous.
    pub sizes: [usize; 3],
    /// Upper bound on the fitted log-log exponent.
    pub max_exponent: f64,
    /// What the probe covers, and why its bound is where it is.
    pub rationale: &'static str,
}

/// File counts for the ranking probes.
///
/// Large enough that a quadratic ranking is unmistakable — the
/// pre-#1267 pass took ~100 ms at the top cell in release — and small
/// enough that a linearithmic one stays well below the per-ranking
/// budget there (1.5–7 ms, by shape), so the whole set adds a few
/// seconds to the gate.
const RANKING_SIZES: [usize; 3] = [2_000, 4_000, 8_000];

/// Bound for the ranking probes.
///
/// A sort-based mid-rank is `O(n log n)` per signal, which over this
/// ladder fits a little above 1.0 (the `log n` term moves from 11 to
/// 13). The bound leaves room for that and for measurement noise
/// while keeping the pre-#1267 pairwise form — a 2.0 — far outside.
const RANKING_BOUND: f64 = 1.5;

/// The ranking probes, one per [`Population`] shape.
pub const VCS_PROBES: &[VcsProbe] = &[
    VcsProbe {
        name: "vcs/all-equal",
        population: Population::AllEqual,
        sizes: RANKING_SIZES,
        max_exponent: RANKING_BOUND,
        rationale: "#1267: `apply_percentile` on a population where every signal \
                    ties across all files. A per-element scan is quadratic here \
                    exactly as on any other shape; a per-tie-group walk would be \
                    linear on this shape only, so the three shapes together pin \
                    the class rather than one of them.",
    },
    VcsProbe {
        name: "vcs/duplicate-heavy",
        population: Population::DuplicateHeavy,
        sizes: RANKING_SIZES,
        max_exponent: RANKING_BOUND,
        rationale: "#1267: `apply_percentile` on a population whose signals each \
                    take a small fixed set of values (two to seventy, by signal), \
                    so every file shares each rank with a large slice of the \
                    population — the shape the integer-count signals take on a \
                    real repository.",
    },
    VcsProbe {
        name: "vcs/spread",
        population: Population::Spread,
        sizes: RANKING_SIZES,
        max_exponent: RANKING_BOUND,
        rationale: "#1267: `apply_percentile` on a population with few exact ties, \
                    so every file's three binary searches land at a different \
                    index — the shape under which the search phase, which \
                    dominates the sort, costs the most.",
    },
];

/// Sizes substituted into every ranking probe for a smoke run.
///
/// The ranking probes' real sizes are a decade above the walk probes',
/// so [`crate::scaling::SMOKE_SIZES`] would put the top cell at 128
/// files — below where the ranking's cost clears the fixed overhead
/// of building the result vector. Smoke still asks only whether the
/// harness runs, so the exponent is not read.
pub const VCS_SMOKE_SIZES: [usize; 3] = [64, 128, 256];

/// Copies `probes` with [`VCS_SMOKE_SIZES`] substituted.
#[must_use]
pub fn smoke_probes(probes: &[VcsProbe]) -> Vec<VcsProbe> {
    probes
        .iter()
        .map(|probe| VcsProbe {
            sizes: VCS_SMOKE_SIZES,
            ..*probe
        })
        .collect()
}

/// Runs the ranking pass on a copy of `population` and returns the
/// ranked records. This is the timed region: `apply_percentile` plus
/// one `to_vec`, which is linear and the same for every shape.
#[must_use]
fn rank(population: &[Stats]) -> Vec<Stats> {
    let mut stats = population.to_vec();
    apply_percentile(&mut stats);
    stats
}

/// A reading that proves the pass did the work: the number of distinct
/// `risk_score` values in `ranked`. Taken once per cell, outside the
/// timed repetitions — it sorts, and would otherwise fold a second
/// `O(n log n)` term into every sample.
///
/// A reading of 1 on a spread population would mean the pass had
/// collapsed every file to one score — the failure
/// [`probe_workload_is_exercised`](crate::shapes) exists to catch for
/// the walks — so `readings_follow_the_tie_structure` below asserts it
/// grows with the shape.
#[must_use]
fn distinct_scores(ranked: &[Stats]) -> u64 {
    let mut scores: Vec<u64> = ranked.iter().map(|s| s.risk_score.to_bits()).collect();
    scores.sort_unstable();
    scores.dedup();
    u64::try_from(scores.len()).unwrap_or(u64::MAX)
}

struct PendingCell {
    probe: usize,
    measured: Measured,
    population: Vec<Stats>,
}

/// Measures every ranking probe at every size and fits an exponent.
///
/// Mirrors [`crate::scaling::run`] — the same warm-up, rotation and
/// per-cell repetition — over a `Vec<Stats>` input instead of a parsed
/// tree. The timed region is `apply_percentile` plus one `to_vec` of
/// the population, which is linear and the same for every shape.
#[must_use]
pub fn run(probes: &[VcsProbe], rounds: usize) -> Report {
    run_with_budget(probes, rounds, MAX_CELL_WALK)
}

/// [`run`], with the per-ranking budget supplied by the caller.
#[must_use]
fn run_with_budget(probes: &[VcsProbe], rounds: usize, max_cell_walk: Duration) -> Report {
    let mut pending = Vec::new();
    let mut over_budget: Vec<Option<(usize, Duration)>> = vec![None; probes.len()];
    for (index, probe) in probes.iter().enumerate() {
        let mut cells = Vec::new();
        for size in probe.sizes {
            let population = probe.population.render(size);
            let started = Instant::now();
            let ranked = rank(&population);
            let single = started.elapsed();
            if single > max_cell_walk {
                over_budget[index] = Some((size, single));
                break;
            }
            let reading = distinct_scores(&ranked);
            cells.push(PendingCell {
                probe: index,
                measured: Measured {
                    size,
                    bytes: size * size_of::<Stats>(),
                    reading,
                    iterations: iterations_for(single),
                    timings: Vec::with_capacity(rounds),
                },
                population,
            });
        }
        if over_budget[index].is_none() {
            pending.append(&mut cells);
        }
    }

    for (round, index) in schedule(pending.len(), rounds) {
        let cell = &mut pending[index];
        let started = Instant::now();
        for _ in 0..cell.measured.iterations {
            black_box(rank(&cell.population));
        }
        cell.measured.record(round, started.elapsed());
    }

    Report {
        rounds,
        probes: probes
            .iter()
            .enumerate()
            .map(|(index, probe)| {
                ProbeReport::reduce(
                    probe.name,
                    probe.max_exponent,
                    probe.rationale,
                    over_budget[index],
                    pending
                        .iter_mut()
                        .filter(|c| c.probe == index)
                        .map(|c| c.measured.reduce())
                        .collect(),
                )
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use big_code_analysis::vcs::Stats;

    use super::{
        Population, VCS_PROBES, VCS_SMOKE_SIZES, distinct_scores, rank, run, run_with_budget,
        smoke_probes,
    };

    /// Every shape produces a population the ranking does real work
    /// on, and the reading grows with the tie structure: one score for
    /// an all-equal population, a handful for duplicate-heavy, many for
    /// spread.
    #[test]
    fn readings_follow_the_tie_structure() {
        let reading = |shape: Population| distinct_scores(&rank(&shape.render(256)));
        let all_equal = reading(Population::AllEqual);
        let duplicates = reading(Population::DuplicateHeavy);
        let spread = reading(Population::Spread);
        assert_eq!(all_equal, 1, "every file ties on every signal");
        // The record index cycles mod 10 and mod 7, so 70 distinct records
        // at any size ≥ 70; under the v2 percentile blend, at 256 those
        // collapse to exactly 68 distinct scores. Rendering and blend are
        // both deterministic, so this is pinned exactly — a wide bound
        // would let a collapsed cycle (`k` fixed, say, giving 10) pass.
        // A change to the percentile formula (a RISK_SCORE_VERSION bump
        // that touches it) is expected to move this; re-derive it then.
        assert_eq!(duplicates, 68, "duplicate-heavy reading at 256");
        assert!(
            spread > duplicates,
            "spread must outrank duplicates: {spread}"
        );
    }

    /// The ranking probes join the walk probes' report keyed by name,
    /// so a duplicate would collide there; and their sizes double, which
    /// is what makes the fitted exponent readable as "cost per doubling".
    /// Mirrors the two guards `shapes::tests` keeps over `PROBES`.
    #[test]
    fn probe_names_are_unique_and_sizes_double() {
        let mut names: Vec<&str> = VCS_PROBES.iter().map(|probe| probe.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(count, names.len(), "duplicate probe name in VCS_PROBES");
        for probe in VCS_PROBES {
            let [a, b, c] = probe.sizes;
            assert_eq!(b, a * 2, "{}: sizes must double", probe.name);
            assert_eq!(c, b * 2, "{}: sizes must double", probe.name);
        }
    }

    /// Rendering is deterministic across calls.
    #[test]
    fn render_is_deterministic() {
        for shape in [
            Population::AllEqual,
            Population::DuplicateHeavy,
            Population::Spread,
        ] {
            assert_eq!(shape.render(64), shape.render(64));
        }
    }

    /// A smoke run over every probe produces a well-formed report.
    /// Deliberately does not assert the exponent: this runs in an
    /// unoptimised build where the number means nothing.
    #[test]
    fn smoke_run_reports_every_probe() {
        let report = run(&smoke_probes(VCS_PROBES), 3);
        assert_eq!(report.probes.len(), VCS_PROBES.len());
        for (probe, entry) in VCS_PROBES.iter().zip(&report.probes) {
            assert_eq!(entry.name, probe.name);
            assert_eq!(entry.cells.len(), 3, "{}: three cells", entry.name);
            for (cell, size) in entry.cells.iter().zip(VCS_SMOKE_SIZES) {
                assert_eq!(cell.size, size);
                assert!(cell.reading > 0, "{}: zero reading", entry.name);
                assert_eq!(cell.bytes, size * size_of::<Stats>());
                assert!(cell.iterations >= 1);
                assert!(cell.min <= cell.median && cell.median <= cell.max);
            }
        }
    }

    /// A zero budget abandons every probe at its smallest size.
    #[test]
    fn zero_budget_abandons_every_probe() {
        let report = run_with_budget(&smoke_probes(VCS_PROBES), 1, Duration::ZERO);
        for entry in &report.probes {
            assert_eq!(
                entry.over_budget.map(|(size, _)| size),
                Some(VCS_SMOKE_SIZES[0])
            );
            assert!(entry.cells.is_empty());
            assert!(!entry.passed());
        }
    }
}
