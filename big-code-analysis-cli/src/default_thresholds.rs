//! The shipped default `[thresholds]` table (issue #1140).
//!
//! This is the single source of truth for the limits `bca init`
//! scaffolds. It is deliberately *not* tied to this repository's own
//! `bca.toml`: the two answer different questions. This table answers
//! "what should an arbitrary project gate at on day one"; the repo-root
//! manifest answers "what does this Rust codebase, with
//! `exclude_tests = true` and a house style that front-loads rationale
//! comments, gate itself at". Before #1140 a drift test forced them
//! equal, which made every retune of one a retune of the other.
//!
//! # How these numbers were chosen
//!
//! Each limit is anchored to a published threshold wherever one exists,
//! and checked against a measured corpus: 43 real-world repositories
//! across 20 languages (tests, vendored trees, and generated code
//! excluded), 254k function spaces, 40k container spaces, 27k files.
//! The design rule is that a default should flag genuine outliers —
//! roughly 1-3% of in-scope spaces in the median language — because a
//! gate that fires on a tenth of a codebase is a style rule, not a
//! gate. The full derivation, the per-language override table, and the
//! per-use-case profiles live in the book's *Choosing thresholds*
//! recipe.
//!
//! Metrics deliberately left out of the default table are documented in
//! [`render_thresholds_block`]'s emitted prose, not here, so an adopter
//! reading their own scaffolded `bca.toml` sees the reasoning.

/// One shipped default limit, plus the rationale rendered above it in
/// the scaffolded manifest.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DefaultThreshold {
    /// Stable metric name, as accepted by `--threshold` and the
    /// `[thresholds]` TOML table.
    pub(crate) name: &'static str,
    /// The limit itself. `u32` rather than `f64` because every shipped
    /// default is integral, and an integral TOML literal is what the
    /// scaffold should emit; a future fractional default would need
    /// this widened and [`render_thresholds_block`] adjusted.
    pub(crate) limit: u32,
    /// Why this value, in one or more rendered comment lines. Wrapped at
    /// authoring time rather than reflowed at render time, so a line
    /// that overflows the manifest's comment column fails a test instead
    /// of silently rewrapping. The width itself lives with the test that
    /// enforces it, `every_default_has_a_rationale_that_fits_the_comment_column`.
    pub(crate) rationale: &'static [&'static str],
}

/// The shipped defaults. Order is the order they appear in a
/// freshly-scaffolded `bca.toml`: the per-function complexity family
/// first, then the per-function size family, then file size, then the
/// container-scoped OO size metrics.
pub(crate) const DEFAULT_THRESHOLDS: &[DefaultThreshold] = &[
    DefaultThreshold {
        name: "cognitive",
        limit: 15,
        rationale: &[
            "SonarSource's own default for the metric they designed. Fires on",
            "~3% of functions in the median language of the reference corpus.",
        ],
    },
    DefaultThreshold {
        name: "cyclomatic",
        limit: 15,
        rationale: &[
            "lizard's default, and the ceiling MISRA and NASA use for",
            "safety-critical work. McCabe's original 10 is stricter than most",
            "modern codebases sustain; NIST 500-235 already allows raising it",
            "for teams with the process to back it up.",
        ],
    },
    DefaultThreshold {
        name: "abc",
        limit: 40,
        rationale: &[
            "Between RuboCop's Ruby-tuned AbcSize of 17 and the folk \"60 and",
            "above is dangerous\" line from Flog. Chosen so ABC's strictness",
            "matches cyclomatic's rather than sitting a tier looser.",
        ],
    },
    DefaultThreshold {
        name: "nargs",
        limit: 5,
        rationale: &[
            "Matches RuboCop's ParameterLists; Code Climate is stricter at 4.",
            "Note Bash and Elixir report 0 arguments unconditionally, so this",
            "limit is inert for them (#1142). Perl counts signature subs; an",
            "`@_`-style sub has no formal parameters and reads 0.",
        ],
    },
    DefaultThreshold {
        name: "nexits",
        limit: 5,
        rationale: &[
            "Code Climate's return-statements limit of 4 is the only published",
            "anchor; 5 leaves room for the Go and C `if err != nil` idiom,",
            "which pushes honest functions past 4 exits.",
        ],
    },
    DefaultThreshold {
        name: "halstead.effort",
        limit: 50_000,
        rationale: &[
            "No published threshold exists for effort, so this one is purely",
            "percentile-derived. It is also the most language-dependent limit",
            "in the table — median-language p90 effort ranges from 2k (Java)",
            "to 56k (C) — so it is the first one worth overriding per language.",
        ],
    },
    DefaultThreshold {
        name: "loc.ploc",
        limit: 600,
        rationale: &[
            "The working file-size limit: physical lines of code, excluding",
            "blanks and comments. Gating code volume rather than total lines",
            "means documenting a decision costs nothing against the cap.",
        ],
    },
    DefaultThreshold {
        name: "loc.sloc",
        limit: 1200,
        rationale: &[
            "A loose bloat backstop, not the working limit — it counts",
            "comments and blanks, so a file cannot grow without bound on",
            "comment volume alone while still clearing loc.ploc.",
        ],
    },
    DefaultThreshold {
        name: "nom",
        limit: 30,
        rationale: &[
            "Code Climate's method-count is stricter at 20. Container-scoped:",
            "on languages whose module construct is not a class (Elixir's",
            "defmodule most sharply) this measures the module, not a class,",
            "and wants raising substantially — see the book's per-language",
            "table.",
        ],
    },
    DefaultThreshold {
        name: "wmc",
        limit: 60,
        rationale: &["Container-scoped, with the same Elixir caveat as nom."],
    },
];

/// Prose that heads the rendered `[thresholds]` block. Split from
/// [`render_thresholds_block`] because the two change for unrelated
/// reasons: this is copy that moves when the docs move, the renderer is
/// loop logic that moves when [`DefaultThreshold`] changes shape.
///
/// The two metric-name inventories below are hand-curated for grouping
/// rather than rendered from the registry, and are pinned against it by
/// `rendered_prose_lists_every_known_metric`.
const THRESHOLDS_HEADER: &str = "\
# bca metric threshold configuration.
#
# These are the shipped defaults (#1140): anchored to published
# thresholds where one exists, and calibrated against a 20-language
# corpus so each limit flags roughly the worst 1-3% of spaces rather
# than a tenth of the codebase. They are a starting point, not a
# verdict on your code — the book's `Choosing thresholds` recipe has
# per-language overrides and per-use-case profiles (CI gate, agent
# feedback, legacy audit, safety-critical).
#
# Editing rules:
#   * Each key is a stable metric name (or dotted sub-metric name) from
#     `bca list-metrics` / `bca check --help`. Available names:
#         cognitive, cyclomatic, cyclomatic.modified,
#         halstead.volume, halstead.difficulty, halstead.effort,
#         halstead.time, halstead.bugs,
#         loc.sloc, loc.ploc, loc.lloc, loc.cloc, loc.blank,
#         nom, tokens, nexits, nargs,
#         mi.original, mi.sei, mi.visual_studio,
#         abc, wmc, npm, npa
#   * Quote keys containing a dot (TOML requires it).
#   * Each limit is checked against the space kind the metric actually
#     measures: the complexity and per-function metrics gate individual
#     functions, the OO size metrics (nom, wmc, npm, npa) gate container
#     spaces, and loc.* gates the whole-file root.
#   * Adding a metric is a tightening — regenerate `.bca-baseline.toml`
#     in the same change so day-one CI does not flip red on offenders
#     that were previously invisible to the gate.
#
# Metrics intentionally NOT gated:
#         cyclomatic.modified,
#         halstead.volume, halstead.difficulty, halstead.time,
#         halstead.bugs,
#         loc.lloc, loc.cloc, loc.blank,
#         mi.original, mi.sei, mi.visual_studio,
#         npm, npa, tokens
# They are still computed and visible in `bca report markdown|html`.
# halstead.volume is the notable omission: the widely-cited \"function
# volume under 1000\" guideline fires on ~7% of functions in the median
# language and 20% in the worst, which is advisory-grade, not
# gate-grade.
[thresholds]
";

/// Render the `[thresholds]` block for the `bca init` scaffold from
/// [`DEFAULT_THRESHOLDS`], header prose included.
///
/// Keeping this a render rather than a hand-maintained string literal is
/// what makes the table the single source of truth: there is no second
/// copy of the numbers to drift.
pub(crate) fn render_thresholds_block() -> String {
    let mut out = String::from(THRESHOLDS_HEADER);
    for (index, entry) in DEFAULT_THRESHOLDS.iter().enumerate() {
        // Blank line between entries so a rationale block visibly
        // attaches to the key below it rather than to the key above.
        if index > 0 {
            out.push('\n');
        }
        for line in entry.rationale {
            out.push_str("# ");
            out.push_str(line);
            out.push('\n');
        }
        // TOML requires quoting on dotted keys; bare keys stay bare so
        // the scaffold reads the way the docs spell these names. Deciding
        // the quoting separately keeps the name itself emitted once.
        let quote = if entry.name.contains('.') { "\"" } else { "" };
        out.push_str(quote);
        out.push_str(entry.name);
        out.push_str(quote);
        out.push_str(" = ");
        out.push_str(&entry.limit.to_string());
        out.push('\n');
    }
    out
}

#[cfg(test)]
#[path = "default_thresholds_tests.rs"]
mod tests;
