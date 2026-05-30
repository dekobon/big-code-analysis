//! Single source of truth for the metric catalog.
//!
//! Before this module existed, the same set of offender metric ids was
//! hand-maintained in three places — [`output::sarif`]'s rule
//! descriptions, the CLI's threshold extractor table, and a third copy
//! inside a "does every extractor have a description" test — plus a
//! fourth, differently-shaped table powering `bca list-metrics`. Those
//! tables drifted: ten of twenty-one rule-description keys once failed
//! to match any real offender id and went unnoticed for two model
//! versions.
//!
//! [`METRICS`](crate::metric_catalog::METRICS) is now the canonical
//! list of offender sub-metric ids (`halstead.volume`, `mi.original`,
//! …) together with their long-form sentences and
//! [`Direction`](crate::metric_catalog::Direction).
//! [`FAMILIES`](crate::metric_catalog::FAMILIES) is the canonical view
//! that `bca list-metrics` renders. The library's offender formatters
//! ([`output::sarif`], [`output::code_climate`]) read `METRICS`; the
//! CLI's threshold engine keys its extractor table off the same ids and
//! a parity test pins the two id-sets together, so a new metric cannot
//! ship with a half-updated catalog.
//!
//! [`output::sarif`]: crate::output
//! [`output::code_climate`]: crate::output

#![allow(clippy::doc_markdown)]

/// Which direction of a metric's value is unhealthy.
///
/// Most metrics grow worse as they grow larger; the Maintainability
/// Index family is the inverse — a *lower* value is worse. Code Climate
/// uses this to invert the threshold-breach ratio, and the rule
/// sentences use it to pick "exceeds" vs "falls below" phrasing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// A higher value is worse (cyclomatic, halstead.*, loc.*, …).
    HigherIsWorse,
    /// A lower value is worse (the `mi.*` Maintainability Index family).
    LowerIsWorse,
}

/// Catalog entry for one offender-emitting sub-metric id.
///
/// The `id` is the dotted key the threshold engine emits for an
/// offender (`halstead.volume`); `family` groups ids under a top-level
/// metric (`halstead`) and must match a [`MetricFamily::name`].
///
/// `#[non_exhaustive]`: these are read-only records the library
/// constructs (downstream consumers read fields, never build them), so
/// a new field can be added in a future minor without a SemVer break.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MetricInfo {
    /// Dotted offender id, e.g. `"halstead.volume"` or `"cognitive"`.
    pub id: &'static str,
    /// Top-level family this id belongs to, e.g. `"halstead"`.
    pub family: &'static str,
    /// Long-form sentence for SARIF `rule.shortDescription.text` and
    /// the Code Climate `description` prefix.
    pub long_description: &'static str,
    /// Whether a higher or lower value is the unhealthy direction.
    pub direction: Direction,
    /// Whether the metric's JSON headline at the file-level `unit` space
    /// is an aggregate across descendant spaces (a `sum`/`*_sum` field)
    /// that does **not** match the CLI threshold accessor's per-space
    /// scalar.
    ///
    /// `true` for the four metrics whose serialized JSON value diverges
    /// from the per-space accessor at the unit level — `cognitive`,
    /// `cyclomatic`, `cyclomatic.modified`, and `abc` (#441). Front-ends
    /// that walk the JSON shape rather than the typed `CodeMetrics`
    /// (the Python `to_sarif` binding) must skip the unit space for
    /// these so they do not emit file-wide values masquerading as
    /// per-space findings the CLI never produces.
    ///
    /// The flag is **not** derivable from the JSON path string: `nexits`
    /// also serialises a `sum` field, but its CLI accessor (`exit_sum()`)
    /// reads that same aggregate, so it does not diverge and is `false`.
    /// The divergence is between the JSON field and the CLI accessor,
    /// which only this registry now records once for both front-ends to
    /// share (#442).
    pub skip_at_unit: bool,
}

/// A `bca list-metrics` row: the bare name printed in `names` mode and
/// the one-line summary printed in `descriptions` mode.
///
/// `#[non_exhaustive]` for the same forward-compat reason as
/// [`MetricInfo`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MetricRow {
    /// Bare name printed one-per-line by `list-metrics`, e.g.
    /// `"halstead"` or `"sloc"`. Downstream tooling
    /// (`split-minimal-tests.py`) greps these names, so they are an
    /// external contract.
    pub name: &'static str,
    /// One-line description printed in `list-metrics descriptions` mode.
    pub summary: &'static str,
}

/// A top-level metric family as surfaced by `bca list-metrics`.
///
/// Most families render as a single [`MetricRow`] whose name equals
/// [`name`](Self::name). `loc` is the exception: it renders one row per
/// sub-measurement (`sloc`, `ploc`, …) because those bare names are an
/// external grep contract.
///
/// `#[non_exhaustive]` for the same forward-compat reason as
/// [`MetricInfo`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MetricFamily {
    /// Family key, e.g. `"halstead"`, `"loc"`. Matches
    /// [`MetricInfo::family`].
    pub name: &'static str,
    /// `list-metrics` rows for this family, in display order.
    pub rows: &'static [MetricRow],
}

/// Canonical offender sub-metric catalog. Long-form sentences and the
/// `mi.*` lower-is-worse direction moved here verbatim from the former
/// `output::rule_descriptions` table.
///
/// `#[rustfmt::skip]`: the one-row-per-entry layout keeps the table
/// scannable; rustfmt would otherwise wrap each struct over many lines.
#[rustfmt::skip]
pub const METRICS: &[MetricInfo] = &[
    MetricInfo { id: "cognitive",           family: "cognitive",  long_description: "Cognitive Complexity exceeds the configured threshold.",          direction: Direction::HigherIsWorse, skip_at_unit: true  },
    MetricInfo { id: "cyclomatic",          family: "cyclomatic", long_description: "Cyclomatic Complexity exceeds the configured threshold.",         direction: Direction::HigherIsWorse, skip_at_unit: true  },
    MetricInfo { id: "cyclomatic.modified", family: "cyclomatic", long_description: "Modified Cyclomatic Complexity exceeds the configured threshold.", direction: Direction::HigherIsWorse, skip_at_unit: true  },
    MetricInfo { id: "halstead.volume",     family: "halstead",   long_description: "Halstead volume exceeds the configured threshold.",               direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "halstead.difficulty", family: "halstead",   long_description: "Halstead difficulty exceeds the configured threshold.",           direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "halstead.effort",     family: "halstead",   long_description: "Halstead effort exceeds the configured threshold.",               direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "halstead.time",       family: "halstead",   long_description: "Halstead time-to-program exceeds the configured threshold.",      direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "halstead.bugs",       family: "halstead",   long_description: "Estimated Halstead bugs exceed the configured threshold.",         direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "loc.sloc",            family: "loc",        long_description: "Source lines of code exceed the configured threshold.",            direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "loc.ploc",            family: "loc",        long_description: "Physical lines of code exceed the configured threshold.",          direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "loc.lloc",            family: "loc",        long_description: "Logical lines of code exceed the configured threshold.",           direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "loc.cloc",            family: "loc",        long_description: "Comment lines of code exceed the configured threshold.",           direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "loc.blank",           family: "loc",        long_description: "Blank lines of code exceed the configured threshold.",             direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "nom",                 family: "nom",        long_description: "Number of methods/functions exceeds the configured threshold.",    direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "tokens",              family: "tokens",     long_description: "Number of tokens exceeds the configured threshold.",               direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "nexits",              family: "nexits",     long_description: "Number of exit points exceeds the configured threshold.",          direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "nargs",               family: "nargs",      long_description: "Number of function arguments exceeds the configured threshold.",   direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "mi.original",         family: "mi",         long_description: "Maintainability Index falls below the configured threshold.",      direction: Direction::LowerIsWorse,  skip_at_unit: false },
    MetricInfo { id: "mi.sei",              family: "mi",         long_description: "Maintainability Index (SEI) falls below the configured threshold.", direction: Direction::LowerIsWorse,  skip_at_unit: false },
    MetricInfo { id: "mi.visual_studio",    family: "mi",         long_description: "Maintainability Index (Visual Studio) falls below the configured threshold.", direction: Direction::LowerIsWorse,  skip_at_unit: false },
    MetricInfo { id: "abc",                 family: "abc",        long_description: "ABC magnitude exceeds the configured threshold.",                  direction: Direction::HigherIsWorse, skip_at_unit: true  },
    MetricInfo { id: "wmc",                 family: "wmc",        long_description: "Weighted Methods per Class exceeds the configured threshold.",     direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "npm",                 family: "npm",        long_description: "Number of public methods exceeds the configured threshold.",       direction: Direction::HigherIsWorse, skip_at_unit: false },
    MetricInfo { id: "npa",                 family: "npa",        long_description: "Number of public attributes exceeds the configured threshold.",    direction: Direction::HigherIsWorse, skip_at_unit: false },
];

/// Canonical `bca list-metrics` view. Family summaries moved here
/// verbatim from the CLI's former hand-maintained catalog. Declaration
/// order is the `list-metrics` print order.
///
/// Only `loc` expands to multiple rows; every other family is a single
/// row whose name equals the family name.
pub const FAMILIES: &[MetricFamily] = &[
    MetricFamily {
        name: "cognitive",
        rows: &[MetricRow {
            name: "cognitive",
            summary: "Cognitive Complexity: how difficult code is to understand.",
        }],
    },
    MetricFamily {
        name: "cyclomatic",
        rows: &[MetricRow {
            name: "cyclomatic",
            summary: "Cyclomatic Complexity: linearly independent paths through the code; the modified variant collapses switch/match/when arms in a single switch statement into one decision point.",
        }],
    },
    MetricFamily {
        name: "halstead",
        rows: &[MetricRow {
            name: "halstead",
            summary: "Halstead suite: vocabulary, length, volume, difficulty, effort, time, bugs.",
        }],
    },
    MetricFamily {
        name: "loc",
        rows: &[
            MetricRow {
                name: "sloc",
                summary: "Source lines of code: total lines in a source file.",
            },
            MetricRow {
                name: "ploc",
                summary: "Physical lines of code: instruction lines.",
            },
            MetricRow {
                name: "lloc",
                summary: "Logical lines of code: statement count.",
            },
            MetricRow {
                name: "cloc",
                summary: "Comment lines of code.",
            },
            MetricRow {
                name: "blank",
                summary: "Blank lines.",
            },
        ],
    },
    MetricFamily {
        name: "nom",
        rows: &[MetricRow {
            name: "nom",
            summary: "Number of methods and closures.",
        }],
    },
    MetricFamily {
        name: "tokens",
        rows: &[MetricRow {
            name: "tokens",
            summary: "Per-function token count: AST leaves excluding comments.",
        }],
    },
    MetricFamily {
        name: "nexits",
        rows: &[MetricRow {
            name: "nexits",
            summary: "Number of exit points from a function or method.",
        }],
    },
    MetricFamily {
        name: "nargs",
        rows: &[MetricRow {
            name: "nargs",
            summary: "Number of arguments to a function or method.",
        }],
    },
    MetricFamily {
        name: "mi",
        rows: &[MetricRow {
            name: "mi",
            summary: "Maintainability Index suite.",
        }],
    },
    MetricFamily {
        name: "abc",
        rows: &[MetricRow {
            name: "abc",
            summary: "ABC: assignments, branches, and conditions.",
        }],
    },
    MetricFamily {
        name: "wmc",
        rows: &[MetricRow {
            name: "wmc",
            summary: "Weighted Methods per Class.",
        }],
    },
    MetricFamily {
        name: "npm",
        rows: &[MetricRow {
            name: "npm",
            summary: "Number of public methods of a class.",
        }],
    },
    MetricFamily {
        name: "npa",
        rows: &[MetricRow {
            name: "npa",
            summary: "Number of public attributes of a class.",
        }],
    },
];

/// Catalog entry for a known offender id, or `None`. Callers pick their
/// own fallback for unknown ids (SARIF emits the raw id; Code Climate
/// falls through to its default message).
pub(crate) fn lookup(id: &str) -> Option<&'static MetricInfo> {
    METRICS.iter().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn metric_ids_are_unique() {
        let mut seen = HashSet::new();
        for m in METRICS {
            assert!(seen.insert(m.id), "duplicate metric id {:?}", m.id);
        }
    }

    #[test]
    fn family_names_are_unique() {
        let mut seen = HashSet::new();
        for f in FAMILIES {
            assert!(seen.insert(f.name), "duplicate family name {:?}", f.name);
        }
    }

    #[test]
    fn every_metric_family_is_declared() {
        let families: HashSet<&str> = FAMILIES.iter().map(|f| f.name).collect();
        for m in METRICS {
            assert!(
                families.contains(m.family),
                "metric {:?} references undeclared family {:?}",
                m.id,
                m.family,
            );
        }
    }

    #[test]
    fn every_family_has_a_metric() {
        let metric_families: HashSet<&str> = METRICS.iter().map(|m| m.family).collect();
        for f in FAMILIES {
            assert!(
                metric_families.contains(f.name),
                "family {:?} has no METRICS entry",
                f.name,
            );
        }
    }

    #[test]
    fn lookup_round_trips_and_rejects_unknown() {
        for m in METRICS {
            assert_eq!(lookup(m.id).map(|i| i.id), Some(m.id));
        }
        assert!(lookup("not.a.metric").is_none());
    }

    /// `mi.*` is the only lower-is-worse family. This pins the data that
    /// replaced the former `is_lower_is_worse` prefix predicate; if the
    /// `Direction` of an `mi.*` row is flipped (or a non-`mi` row is
    /// marked `LowerIsWorse`), Code Climate's breach-ratio inversion
    /// silently flips with it.
    #[test]
    fn lower_is_worse_iff_mi_family() {
        for m in METRICS {
            let expect_lower = m.family == "mi";
            assert_eq!(
                matches!(m.direction, Direction::LowerIsWorse),
                expect_lower,
                "metric {:?} has the wrong direction",
                m.id,
            );
        }
    }

    /// `mi.*` sentences phrase the breach as "falls below"; every other
    /// metric phrases it as "exceeds"/"exceed". This pins the wording to
    /// the direction so a copy-paste sentence with the wrong verb is
    /// caught.
    #[test]
    fn sentence_phrasing_matches_direction() {
        for m in METRICS {
            match m.direction {
                Direction::LowerIsWorse => assert!(
                    m.long_description.contains("falls below"),
                    "{:?} should use `falls below`: {:?}",
                    m.id,
                    m.long_description,
                ),
                Direction::HigherIsWorse => assert!(
                    m.long_description.contains("exceed"),
                    "{:?} should use `exceed(s)`: {:?}",
                    m.id,
                    m.long_description,
                ),
            }
        }
    }

    /// `skip_at_unit` is `true` for exactly the four metrics whose
    /// serialized JSON headline at the file-level `unit` space is an
    /// aggregate over descendant spaces that does not match the CLI
    /// threshold accessor's per-space scalar (#441). The Python
    /// `to_sarif` binding mirrors this registry; a cross-crate test in
    /// `big-code-analysis-py/src/sarif.rs` pins its `METRIC_FIELDS`
    /// table's flags to these values, so this set is the single source
    /// of truth both front-ends derive from (#442).
    ///
    /// The property is deliberately enumerated rather than derived from
    /// the id string: `nexits` also serialises a `sum` field but reads
    /// that same aggregate via its CLI accessor, so it does not diverge.
    #[test]
    fn skip_at_unit_is_the_sum_vs_per_space_divergence_set() {
        let mut skip: Vec<&str> = METRICS
            .iter()
            .filter(|m| m.skip_at_unit)
            .map(|m| m.id)
            .collect();
        skip.sort_unstable();
        assert_eq!(
            skip,
            ["abc", "cognitive", "cyclomatic", "cyclomatic.modified"],
            "skip_at_unit set drifted from the JSON-aggregate-vs-CLI-accessor \
             property; review against the CLI EXTRACTORS accessors before editing",
        );
    }
}
