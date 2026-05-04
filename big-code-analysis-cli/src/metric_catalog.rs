//! Catalog of metric categories that `big-code-analysis` can compute.
//!
//! Names match the keys downstream tools (e.g. `split-minimal-tests.py`)
//! grep for in `--metrics` output: top-level keys from `CodeMetrics` plus
//! the `loc` sub-metrics, which are conceptually distinct measurements.
//! Descriptions are short (one line); the book contains the long form.

use std::io::{self, Write};

/// One row of the metric catalog: short identifier and a one-line summary.
pub(crate) struct MetricEntry {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
}

/// Mode for `--list-metrics`. Names-only is the default to keep the output
/// machine-readable for shell pipelines.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListMetricsMode {
    /// Print metric names, one per line.
    Names,
    /// Print metric names alongside short descriptions.
    Descriptions,
}

pub(crate) const METRICS: &[MetricEntry] = &[
    MetricEntry {
        name: "cognitive",
        description: "Cognitive complexity: how difficult code is to understand.",
    },
    MetricEntry {
        name: "cyclomatic",
        description: "Cyclomatic complexity: linearly independent paths through the code.",
    },
    MetricEntry {
        name: "halstead",
        description: "Halstead suite: vocabulary, length, volume, difficulty, effort, time, bugs.",
    },
    MetricEntry {
        name: "sloc",
        description: "Source lines of code: total lines in a source file.",
    },
    MetricEntry {
        name: "ploc",
        description: "Physical lines of code: instruction lines.",
    },
    MetricEntry {
        name: "lloc",
        description: "Logical lines of code: statement count.",
    },
    MetricEntry {
        name: "cloc",
        description: "Comment lines of code.",
    },
    MetricEntry {
        name: "blank",
        description: "Blank lines.",
    },
    MetricEntry {
        name: "nom",
        description: "Number of methods and closures.",
    },
    MetricEntry {
        name: "nexits",
        description: "Number of exit points from a function or method.",
    },
    MetricEntry {
        name: "nargs",
        description: "Number of arguments to a function or method.",
    },
    MetricEntry {
        name: "mi",
        description: "Maintainability index suite.",
    },
    MetricEntry {
        name: "abc",
        description: "ABC: assignments, branches, and conditions.",
    },
    MetricEntry {
        name: "wmc",
        description: "Weighted methods per class.",
    },
    MetricEntry {
        name: "npm",
        description: "Number of public methods of a class.",
    },
    MetricEntry {
        name: "npa",
        description: "Number of public attributes of a class.",
    },
];

/// Write the catalog to `out` according to `mode`. In `Descriptions` mode
/// names are left-aligned to the widest name so the two columns line up.
pub(crate) fn write_metrics(out: &mut dyn Write, mode: ListMetricsMode) -> io::Result<()> {
    match mode {
        ListMetricsMode::Names => {
            for m in METRICS {
                writeln!(out, "{}", m.name)?;
            }
        }
        ListMetricsMode::Descriptions => {
            let width = METRICS
                .iter()
                .map(|m| m.name.len())
                .max()
                .expect("METRICS is non-empty");
            for m in METRICS {
                writeln!(
                    out,
                    "{name:<width$}  {desc}",
                    name = m.name,
                    desc = m.description
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_unique_and_lowercase() {
        let mut seen = std::collections::HashSet::new();
        for m in METRICS {
            let name = m.name;
            assert!(!name.is_empty(), "metric name must be non-empty");
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "metric name {name:?} must be ascii lowercase",
            );
            assert!(seen.insert(name), "duplicate metric name {name:?}");
            assert!(
                !m.description.is_empty(),
                "metric {name:?} missing description",
            );
        }
    }

    #[test]
    fn names_mode_prints_one_per_line() {
        let mut buf = Vec::new();
        write_metrics(&mut buf, ListMetricsMode::Names).expect("write");
        let out = String::from_utf8(buf).expect("utf8");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), METRICS.len());
        for (line, m) in lines.iter().zip(METRICS.iter()) {
            assert_eq!(*line, m.name);
        }
    }

    #[test]
    fn descriptions_mode_includes_descriptions() {
        let mut buf = Vec::new();
        write_metrics(&mut buf, ListMetricsMode::Descriptions).expect("write");
        let out = String::from_utf8(buf).expect("utf8");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), METRICS.len());
        for (line, m) in lines.iter().zip(METRICS.iter()) {
            assert!(
                line.starts_with(m.name),
                "line {line:?} should start with name"
            );
            assert!(
                line.contains(m.description),
                "line {line:?} missing description"
            );
        }
    }

    /// The catalog must include every top-level metric category the library
    /// emits in `--metrics` output, plus the `loc` sub-metrics. If
    /// `CodeMetrics` gains a new field, this test fails until the catalog
    /// is updated.
    #[test]
    fn catalog_covers_library_output() {
        use big_code_analysis::CodeMetrics;
        use std::collections::HashSet;

        let json = serde_json::to_value(CodeMetrics::default()).expect("CodeMetrics serializes");
        let mut expected: HashSet<String> = json
            .as_object()
            .expect("object")
            .keys()
            .filter(|k| *k != "loc")
            .cloned()
            .collect();
        // `loc` expands to sub-metrics; `wmc`/`npm`/`npa` are skipped from
        // the default JSON because they're disabled — both belong in the
        // catalog. `HashSet::insert` is idempotent so no dedup needed.
        expected.extend(
            ["sloc", "ploc", "lloc", "cloc", "blank", "wmc", "npm", "npa"].map(String::from),
        );
        let catalog: HashSet<&str> = METRICS.iter().map(|m| m.name).collect();
        for name in &expected {
            assert!(
                catalog.contains(name.as_str()),
                "catalog missing metric {name:?}; CodeMetrics emits it but --list-metrics does not"
            );
        }
    }
}
