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

use crate::spaces::SpaceKind;

/// The space kind a metric's threshold is meaningful on (issue #969).
///
/// A threshold gate (`bca check`, the Python `to_sarif` binding) walks
/// every [`crate::FuncSpace`] — the file-level [`SpaceKind::Unit`] root,
/// every container (class / impl / ...), and every individual function.
/// For the subtree-summed accessors a metric's value at any space that
/// owns children is a *sum across many functions*, so a per-function
/// limit would fire on every non-trivial file and multi-method `impl`.
/// Scope records the kind each metric actually measures so the front-ends
/// gate it there and nowhere else — keeping the CLI gate and the binding
/// in lockstep, the same way [`Direction`] keeps their breach direction
/// aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricScope {
    /// Gate only the whole-file [`SpaceKind::Unit`] root — the `loc.*`
    /// size family, whose limit is a per-file ceiling.
    File,
    /// Gate only individual function spaces ([`SpaceKind::Function`] —
    /// free functions, methods, closures). The per-function complexity
    /// metrics (cognitive, cyclomatic, abc, mi.*) and the subtree sums
    /// that describe one function (halstead.*, nargs, nexits, tokens)
    /// live here.
    ///
    /// Whether "one function" includes its nested closures is per
    /// metric, not per scope. `halstead.*`, `nexits` and `tokens` read
    /// subtree sums, because a closure's tokens and exits really are
    /// part of the enclosing body a reader must follow. `nargs` reads
    /// the space's own parameters instead (#1196): a closure that opens
    /// its own space is gated on its own row, and summing its arguments
    /// into the enclosing signature made the offender's number describe
    /// something its remediation could not change.
    Function,
    /// Gate only container spaces that own methods (class / struct /
    /// trait / impl / namespace / interface) — the object-oriented size
    /// metrics `nom`, `wmc`, `npm`, `npa`.
    Container,
}

impl MetricScope {
    /// Whether a threshold with this scope is evaluated against `kind`.
    ///
    /// The single source of truth for the kind-filtering both the CLI
    /// gate and the Python binding apply, so the two cannot drift on
    /// which space kinds a metric gates.
    #[must_use]
    pub fn admits(self, kind: SpaceKind) -> bool {
        match self {
            Self::File => matches!(kind, SpaceKind::Unit),
            Self::Function => matches!(kind, SpaceKind::Function),
            Self::Container => matches!(
                kind,
                SpaceKind::Class
                    | SpaceKind::Struct
                    | SpaceKind::Trait
                    | SpaceKind::Impl
                    | SpaceKind::Namespace
                    | SpaceKind::Interface
            ),
        }
    }
}

/// Which direction of a metric's value is unhealthy.
///
/// Most metrics grow worse as they grow larger; the Maintainability
/// Index family is the inverse — a *lower* value is worse. Code Climate
/// uses this to invert the threshold-breach ratio, and the rule
/// sentences use it to pick "exceeds" vs "falls below" phrasing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Whether the metric's JSON headline is an aggregate across
    /// descendant spaces (a `sum`/`*_sum` field) that does **not** match
    /// the CLI threshold accessor's per-space scalar at any interior
    /// space.
    ///
    /// `true` for the four metrics whose serialized JSON value diverges
    /// from the per-space accessor — `cognitive`, `cyclomatic`,
    /// `cyclomatic.modified`, and `abc` (#441). The aggregate equals the
    /// per-space scalar only at a leaf space (no descendant
    /// function/closure spaces); at any interior space — the file-level
    /// `unit` or a container with descendants — it is larger.
    ///
    /// This flag describes the `sum`/`*_sum` *aggregate* field, which
    /// still diverges. As of #958 the wire shape **also** serializes each
    /// of these four metrics' per-space own value (`cyclomatic.value`,
    /// `cyclomatic.modified.value`, `cognitive.value`, `abc.value`), so a
    /// JSON-walking front-end no longer needs this flag to stay correct:
    /// it reads the own value directly and emits at every space exactly
    /// like the CLI. The Python `to_sarif` binding was switched to that
    /// path in #958; before it, the binding emitted these only at leaf
    /// spaces to avoid subtree-wide values masquerading as per-space
    /// findings the CLI never produces (#855). The flag name retains its
    /// original unit-only framing.
    ///
    /// The flag is **not** derivable from the JSON path string: `nexits`
    /// also serialises a `sum` field, but its CLI accessor (`nexits_sum()`)
    /// reads that same aggregate, so it does not diverge and is `false`.
    /// The divergence is between the JSON field and the CLI accessor,
    /// which only this registry now records once for both front-ends to
    /// share (#442).
    pub skip_at_unit: bool,
    /// The space kind this metric's threshold gates (issue #969). Both
    /// the CLI threshold engine and the Python `to_sarif` binding read
    /// this to skip spaces a metric does not measure, so a metric's
    /// file-wide or `impl`-wide aggregate never fires as a per-function
    /// limit. See [`MetricScope`].
    pub scope: MetricScope,
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
    /// `"halstead"` or `"sloc"`. Downstream tooling (`bca diff`, which
    /// buckets per-file metric deltas by these names) relies on them, so
    /// they are an external contract.
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
    MetricInfo { id: "cognitive",           family: "cognitive",  long_description: "Cognitive Complexity exceeds the configured threshold.",          direction: Direction::HigherIsWorse, skip_at_unit: true,  scope: MetricScope::Function  },
    MetricInfo { id: "cyclomatic",          family: "cyclomatic", long_description: "Cyclomatic Complexity exceeds the configured threshold.",         direction: Direction::HigherIsWorse, skip_at_unit: true,  scope: MetricScope::Function  },
    MetricInfo { id: "cyclomatic.modified", family: "cyclomatic", long_description: "Modified Cyclomatic Complexity exceeds the configured threshold.", direction: Direction::HigherIsWorse, skip_at_unit: true,  scope: MetricScope::Function  },
    MetricInfo { id: "halstead.volume",     family: "halstead",   long_description: "Halstead volume exceeds the configured threshold.",               direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "halstead.difficulty", family: "halstead",   long_description: "Halstead difficulty exceeds the configured threshold.",           direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "halstead.effort",     family: "halstead",   long_description: "Halstead effort exceeds the configured threshold.",               direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "halstead.time",       family: "halstead",   long_description: "Halstead time-to-program exceeds the configured threshold.",      direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "halstead.bugs",       family: "halstead",   long_description: "Estimated Halstead bugs exceed the configured threshold.",         direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "loc.sloc",            family: "loc",        long_description: "Source lines of code exceed the configured threshold.",            direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File      },
    MetricInfo { id: "loc.ploc",            family: "loc",        long_description: "Physical lines of code exceed the configured threshold.",          direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File      },
    MetricInfo { id: "loc.lloc",            family: "loc",        long_description: "Logical lines of code exceed the configured threshold.",           direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File      },
    MetricInfo { id: "loc.cloc",            family: "loc",        long_description: "Comment lines of code exceed the configured threshold.",           direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File      },
    MetricInfo { id: "loc.blank",           family: "loc",        long_description: "Blank lines of code exceed the configured threshold.",             direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File      },
    MetricInfo { id: "nom",                 family: "nom",        long_description: "Number of methods/functions exceeds the configured threshold.",    direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Container },
    MetricInfo { id: "tokens",              family: "tokens",     long_description: "Number of tokens exceeds the configured threshold.",               direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "nexits",              family: "nexits",     long_description: "Number of exit points exceeds the configured threshold.",          direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "nargs",               family: "nargs",      long_description: "Number of function arguments exceeds the configured threshold.",   direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "mi.original",         family: "mi",         long_description: "Maintainability Index falls below the configured threshold.",      direction: Direction::LowerIsWorse,  skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "mi.sei",              family: "mi",         long_description: "Maintainability Index (SEI) falls below the configured threshold.", direction: Direction::LowerIsWorse,  skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "mi.visual_studio",    family: "mi",         long_description: "Maintainability Index (Visual Studio) falls below the configured threshold.", direction: Direction::LowerIsWorse,  skip_at_unit: false, scope: MetricScope::Function  },
    MetricInfo { id: "abc",                 family: "abc",        long_description: "ABC magnitude exceeds the configured threshold.",                  direction: Direction::HigherIsWorse, skip_at_unit: true,  scope: MetricScope::Function  },
    MetricInfo { id: "wmc",                 family: "wmc",        long_description: "Weighted Methods per Class exceeds the configured threshold.",     direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Container },
    MetricInfo { id: "npm",                 family: "npm",        long_description: "Number of public methods exceeds the configured threshold.",       direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Container },
    MetricInfo { id: "npa",                 family: "npa",        long_description: "Number of public attributes exceeds the configured threshold.",    direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Container },
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
///
/// Public so out-of-crate consumers (the CLI threshold engine) can read
/// a metric's [`Direction`] — the `mi.*` family is lower-is-worse, so
/// the gate and the offender wording must consult it rather than
/// assuming a higher value is always the violation (#698).
#[must_use]
pub fn lookup(id: &str) -> Option<&'static MetricInfo> {
    METRICS.iter().find(|m| m.id == id)
}

/// Whether a lower value of the metric `id` is the unhealthy direction
/// (the `mi.*` Maintainability Index family). The threshold gate, the
/// Code Climate severity-ratio inversion, and the SARIF/offender wording
/// all consult this so they never drift from one another. An id the
/// catalog does not know defaults to higher-is-worse — the same fallback
/// every offender formatter already uses (#698).
#[must_use]
pub fn lower_is_worse(id: &str) -> bool {
    lookup(id).is_some_and(|m| matches!(m.direction, Direction::LowerIsWorse))
}

/// The [`MetricScope`] of metric `id` — the space kind its threshold
/// gates (issue #969). `None` for an id the catalog does not know; both
/// front-ends treat an unknown id as a usage error before reaching here,
/// so the `None` arm is only a defensive fallback.
#[must_use]
pub fn scope(id: &str) -> Option<MetricScope> {
    lookup(id).map(|m| m.scope)
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

    #[test]
    fn lower_is_worse_helper_matches_catalog_and_defaults_false() {
        assert!(lower_is_worse("mi.original"), "mi.* is lower-is-worse");
        assert!(
            !lower_is_worse("cyclomatic"),
            "cyclomatic is higher-is-worse"
        );
        // An id the catalog does not know defaults to higher-is-worse, so
        // the shared gate never flags an unknown metric on the wrong side.
        assert!(!lower_is_worse("not_a_metric"));
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

    /// The per-metric [`MetricScope`] partition (#969): `loc.*` gates the
    /// file root, the OO size metrics gate containers, everything else
    /// gates leaf functions. Enumerated so a new metric must be placed
    /// deliberately rather than defaulting silently — both the CLI gate
    /// and the Python binding derive their kind-filtering from this.
    #[test]
    fn scope_partitions_metrics_by_measured_kind() {
        let by_scope = |want: MetricScope| {
            let mut ids: Vec<&str> = METRICS
                .iter()
                .filter(|m| m.scope == want)
                .map(|m| m.id)
                .collect();
            ids.sort_unstable();
            ids
        };
        assert_eq!(
            by_scope(MetricScope::File),
            ["loc.blank", "loc.cloc", "loc.lloc", "loc.ploc", "loc.sloc"],
            "only the loc.* size family is File-scoped",
        );
        assert_eq!(
            by_scope(MetricScope::Container),
            ["nom", "npa", "npm", "wmc"],
            "only the OO size metrics are Container-scoped",
        );
        // Everything else is per-function; spot-check the representatives
        // and confirm the partition is total (no metric left unscoped).
        for id in [
            "cognitive",
            "cyclomatic",
            "halstead.effort",
            "nargs",
            "nexits",
            "abc",
            "mi.original",
        ] {
            assert_eq!(
                scope(id),
                Some(MetricScope::Function),
                "{id} should be Function-scoped"
            );
        }
        let counted = by_scope(MetricScope::File).len()
            + by_scope(MetricScope::Function).len()
            + by_scope(MetricScope::Container).len();
        assert_eq!(
            counted,
            METRICS.len(),
            "every metric must have exactly one scope"
        );
    }

    /// [`MetricScope::admits`] gates exactly the intended kinds: File only
    /// the `Unit` root, Function only `Function`, Container the
    /// method-owning kinds — and nothing admits `Unknown`.
    #[test]
    fn scope_admits_only_its_kinds() {
        assert!(MetricScope::File.admits(SpaceKind::Unit));
        assert!(!MetricScope::File.admits(SpaceKind::Function));
        assert!(!MetricScope::File.admits(SpaceKind::Class));

        assert!(MetricScope::Function.admits(SpaceKind::Function));
        assert!(!MetricScope::Function.admits(SpaceKind::Unit));
        assert!(!MetricScope::Function.admits(SpaceKind::Impl));

        for kind in [
            SpaceKind::Class,
            SpaceKind::Struct,
            SpaceKind::Trait,
            SpaceKind::Impl,
            SpaceKind::Namespace,
            SpaceKind::Interface,
        ] {
            assert!(
                MetricScope::Container.admits(kind),
                "{kind:?} is a container"
            );
        }
        assert!(!MetricScope::Container.admits(SpaceKind::Unit));
        assert!(!MetricScope::Container.admits(SpaceKind::Function));

        for scope in [
            MetricScope::File,
            MetricScope::Function,
            MetricScope::Container,
        ] {
            assert!(
                !scope.admits(SpaceKind::Unknown),
                "{scope:?} must not admit Unknown"
            );
        }
    }
}
