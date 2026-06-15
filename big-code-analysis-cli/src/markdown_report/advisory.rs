// bca: suppress-file(halstead)
// The manifest-key string literals and the BTreeMap-driven test fixtures
// inflate the file-level Halstead effort (string-formatting / table-lookup
// volume), the same artifact the sibling report renderers suppress — not
// per-function logic complexity (cognitive/cyclomatic stay enforced).

//! Resolved advisory cutoffs for the report's Actionable Summary, the CC
//! note, and the Many-Parameters table (issue #630).
//!
//! These are *advisory* thresholds — the "N functions with CC > 10"-style
//! roll-up and the `(>3)` parameter filter — distinct from `bca check`'s
//! gate. Historically they were hardcoded magic numbers (10, 15, 100, 3, 1.0,
//! 20), so a project gating at `cyclomatic = 15` in its `bca.toml` read a
//! report scolding it about `CC > 10` with no way to align the two.
//!
//! [`AdvisoryThresholds`] resolves each cutoff from the manifest
//! `[thresholds]` table when present (falling back to the named defaults
//! per-key) and records a [`ThresholdSource`] so the report can always print
//! where the numbers came from. The renderers thread one of these through the
//! Actionable Summary, the CC note, and the Many-Parameters spec; the test
//! wrappers pass [`AdvisoryThresholds::DEFAULT`].

use std::collections::BTreeMap;

/// Default advisory cyclomatic-complexity cutoff (the Actionable Summary
/// `CC > N` bullet and the CC note's primary band).
pub(crate) const DEFAULT_CC: f64 = 10.0;
/// Default advisory cognitive-complexity cutoff.
pub(crate) const DEFAULT_COGNITIVE: f64 = 15.0;
/// Default advisory SLOC cutoff.
pub(crate) const DEFAULT_SLOC: u64 = 100;
/// Default advisory parameter-count cutoff (the Many-Parameters filter and
/// its dynamic `(>N)` title).
pub(crate) const DEFAULT_NARGS: u64 = 3;
/// Default advisory Halstead-bugs cutoff.
pub(crate) const DEFAULT_BUGS: f64 = 1.0;
/// Multiple of the resolved CC cutoff used for the CC note's *severe* second
/// band. There is no manifest key for a separate severe tier, so it stays a
/// named default multiple of the primary cutoff (default `2x` → `CC > 20`,
/// matching the historical hardcoded band) rather than inventing a new key
/// (issue #630).
pub(crate) const CC_SEVERE_MULTIPLE: f64 = 2.0;

/// Manifest `[thresholds]` keys the advisory cutoffs read from. `loc.sloc` is
/// the dotted form `bca check` uses (issue #514); `cyclomatic`, `cognitive`,
/// `nargs`, and `halstead.bugs` are the bare metric ids.
const KEY_CYCLOMATIC: &str = "cyclomatic";
const KEY_COGNITIVE: &str = "cognitive";
const KEY_SLOC: &str = "loc.sloc";
const KEY_NARGS: &str = "nargs";
const KEY_BUGS: &str = "halstead.bugs";

/// Where the resolved advisory cutoffs came from, printed as a provenance
/// line in both formats so the numbers are always attributable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ThresholdSource {
    /// At least one cutoff was sourced from the manifest `[thresholds]` table.
    Manifest,
    /// No manifest thresholds applied; every cutoff is the built-in default.
    Default,
}

/// The resolved advisory cutoffs plus their provenance.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AdvisoryThresholds {
    pub(crate) cc: f64,
    pub(crate) cognitive: f64,
    pub(crate) sloc: u64,
    pub(crate) nargs: u64,
    pub(crate) bugs: f64,
    pub(crate) source: ThresholdSource,
}

impl AdvisoryThresholds {
    /// The built-in advisory cutoffs, used when no manifest with thresholds is
    /// present (and by the snapshot-test wrappers).
    pub(crate) const DEFAULT: Self = Self {
        cc: DEFAULT_CC,
        cognitive: DEFAULT_COGNITIVE,
        sloc: DEFAULT_SLOC,
        nargs: DEFAULT_NARGS,
        bugs: DEFAULT_BUGS,
        source: ThresholdSource::Default,
    };

    /// Resolve advisory cutoffs from a manifest's hard `[thresholds]` table
    /// (the `metric = limit` scalars `bca check` already parses). Each cutoff
    /// falls back to its built-in default when the corresponding key is
    /// absent; the source is [`ThresholdSource::Manifest`] iff at least one
    /// recognised key was present, so a manifest with no relevant thresholds
    /// still reads as "default advisory thresholds".
    pub(crate) fn from_manifest_hard(hard: &BTreeMap<String, f64>) -> Self {
        let mut resolved = Self::DEFAULT;
        let mut any = false;
        // Cyclomatic and cognitive are integer-valued in this codebase
        // (`m.cyclomatic.cyclomatic() as f64`), and the report labels render
        // them with `{:.0}`. Round (not truncate) at resolution so the
        // displayed `CC > N` band and the `count_over` comparison describe the
        // same boundary; a fractional manifest cutoff like `10.5` would
        // otherwise print `> 10` while counting `> 10.5` (issue #845).
        if let Some(&v) = hard.get(KEY_CYCLOMATIC) {
            resolved.cc = v.max(0.0).round();
            any = true;
        }
        if let Some(&v) = hard.get(KEY_COGNITIVE) {
            resolved.cognitive = v.max(0.0).round();
            any = true;
        }
        // `loc.sloc` / `nargs` are integer-shaped metrics; a manifest carries
        // them as TOML integers parsed into `f64`. Round (not truncate) to the
        // nearest whole cutoff and clamp negatives to 0 so a hand-edited
        // fractional value resolves to the obvious integer rather than silently
        // flooring (e.g. `100.6` -> `101`, never a stray `100`).
        if let Some(&v) = hard.get(KEY_SLOC) {
            resolved.sloc = v.max(0.0).round() as u64;
            any = true;
        }
        if let Some(&v) = hard.get(KEY_NARGS) {
            resolved.nargs = v.max(0.0).round() as u64;
            any = true;
        }
        // Halstead bugs is genuinely fractional and its label renders with
        // `{:.1}`. Round to one decimal at resolution so the printed cutoff
        // and the `count_over` comparison agree at that precision (issue #845).
        if let Some(&v) = hard.get(KEY_BUGS) {
            const BUGS_LABEL_SCALE: f64 = 10.0; // one decimal place, matching `{:.1}`
            resolved.bugs = (v.max(0.0) * BUGS_LABEL_SCALE).round() / BUGS_LABEL_SCALE;
            any = true;
        }
        resolved.source = if any {
            ThresholdSource::Manifest
        } else {
            ThresholdSource::Default
        };
        resolved
    }

    /// The CC note's severe second-band cutoff: a named multiple of the
    /// resolved primary CC cutoff ([`CC_SEVERE_MULTIPLE`]).
    pub(crate) fn cc_severe(self) -> f64 {
        self.cc * CC_SEVERE_MULTIPLE
    }

    /// Tally how many of `funcs` exceed each advisory cutoff, in one pass.
    /// Single-sourced so the Markdown and HTML Actionable Summaries (and the
    /// HTML cross-language roll-up) cannot drift on what "over threshold"
    /// means (issue #630). The `usize -> u64` widening for the line/parameter
    /// counts is lossless on every supported target.
    pub(crate) fn count_over(self, funcs: &[&super::FunctionSummary]) -> AdvisoryCounts {
        let mut c = AdvisoryCounts::default();
        for s in funcs {
            c.cc += usize::from(s.cyclomatic > self.cc);
            c.cognitive += usize::from(s.cognitive > self.cognitive);
            c.sloc += usize::from(s.sloc as u64 > self.sloc);
            c.nargs += usize::from(s.nargs as u64 > self.nargs);
            c.bugs += usize::from(s.halstead_bugs > self.bugs);
        }
        c
    }

    /// The one-line provenance sentence printed in both formats so the
    /// advisory numbers are always attributable (issue #630).
    pub(crate) fn provenance_line(self) -> &'static str {
        match self.source {
            ThresholdSource::Manifest => {
                "Advisory thresholds: sourced from the `bca.toml` `[thresholds]` table."
            }
            ThresholdSource::Default => {
                "Advisory thresholds: built-in defaults (distinct from `bca check`'s gate)."
            }
        }
    }
}

/// Per-metric counts of functions over the advisory cutoffs, produced by
/// [`AdvisoryThresholds::count_over`] and consumed by both report formats'
/// Actionable Summaries.
#[derive(Default, Clone, Copy)]
pub(crate) struct AdvisoryCounts {
    pub(crate) cc: usize,
    pub(crate) cognitive: usize,
    pub(crate) sloc: usize,
    pub(crate) nargs: usize,
    pub(crate) bugs: usize,
}

impl AdvisoryCounts {
    /// Whether no function cleared any advisory cutoff (the "No major quality
    /// concerns detected." case).
    pub(crate) fn all_clear(self) -> bool {
        self.cc == 0 && self.cognitive == 0 && self.sloc == 0 && self.nargs == 0 && self.bugs == 0
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn default_when_no_manifest_keys() {
        let resolved = AdvisoryThresholds::from_manifest_hard(&BTreeMap::new());
        assert_eq!(resolved.source, ThresholdSource::Default);
        assert_eq!(resolved.cc, DEFAULT_CC);
        assert_eq!(resolved.nargs, DEFAULT_NARGS);
        assert_eq!(resolved.cc_severe(), DEFAULT_CC * CC_SEVERE_MULTIPLE);
    }

    #[test]
    fn manifest_overrides_recognised_keys() {
        let mut hard = BTreeMap::new();
        hard.insert(KEY_CYCLOMATIC.to_string(), 15.0);
        hard.insert(KEY_NARGS.to_string(), 5.0);
        let resolved = AdvisoryThresholds::from_manifest_hard(&hard);
        assert_eq!(resolved.source, ThresholdSource::Manifest);
        assert_eq!(resolved.cc, 15.0);
        assert_eq!(resolved.cc_severe(), 30.0);
        assert_eq!(resolved.nargs, 5);
        // Unspecified keys keep their defaults.
        assert_eq!(resolved.cognitive, DEFAULT_COGNITIVE);
        assert_eq!(resolved.sloc, DEFAULT_SLOC);
    }

    #[test]
    fn fractional_cutoffs_round_to_the_displayed_boundary() {
        // A fractional manifest cutoff must resolve to the same boundary the
        // label prints, so the band the report names equals the population
        // `count_over` tallies (issue #845). cc/cognitive render `{:.0}`, so
        // they round to whole; bugs renders `{:.1}`, so it rounds to one
        // decimal.
        let mut hard = BTreeMap::new();
        hard.insert(KEY_CYCLOMATIC.to_string(), 10.5);
        hard.insert(KEY_COGNITIVE.to_string(), 14.4);
        hard.insert(KEY_BUGS.to_string(), 1.05);
        let resolved = AdvisoryThresholds::from_manifest_hard(&hard);
        // 10.5 rounds to 11 (banker's-rounding-free `f64::round` rounds half
        // away from zero), so the label `CC > 11` matches `count_over`'s `> 11`.
        assert_eq!(resolved.cc, 11.0);
        assert_eq!(resolved.cc_severe(), 22.0);
        assert_eq!(resolved.cognitive, 14.0);
        assert_eq!(resolved.bugs, 1.1);
    }

    #[test]
    fn unrelated_keys_do_not_flip_source_to_manifest() {
        let mut hard = BTreeMap::new();
        hard.insert("halstead.effort".to_string(), 100_000.0);
        let resolved = AdvisoryThresholds::from_manifest_hard(&hard);
        assert_eq!(resolved.source, ThresholdSource::Default);
    }
}
