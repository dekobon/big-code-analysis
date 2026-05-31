//! `bca list-metrics` rendering.
//!
//! The catalog itself is no longer maintained here: rows are derived
//! from the library's canonical [`big_code_analysis::metric_catalog`]
//! (`FAMILIES`), so adding a metric to the library automatically
//! surfaces it here. Names match the keys downstream tools (e.g.
//! `bca diff`, which buckets per-file metric deltas by them) rely on in
//! `list-metrics` output: top-level family names plus the `loc`
//! sub-metrics, which are conceptually distinct measurements.

use std::io::{self, Write};

use big_code_analysis::metric_catalog::FAMILIES;

/// Mode for `list-metrics`. Names-only is the default to keep the output
/// machine-readable for shell pipelines.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListMetricsMode {
    /// Print metric names, one per line.
    Names,
    /// Print metric names alongside short descriptions.
    Descriptions,
}

/// Flattened `(name, summary)` rows across every family, in library
/// declaration order. `loc` expands to its sub-metrics (`sloc`, …);
/// every other family yields a single row whose name is the family
/// name.
fn rows() -> impl Iterator<Item = (&'static str, &'static str)> {
    FAMILIES
        .iter()
        .flat_map(|family| family.rows.iter().map(|row| (row.name, row.summary)))
}

/// Write the catalog to `out` according to `mode`. In `Descriptions` mode
/// names are left-aligned to the widest name so the two columns line up.
pub(crate) fn write_metrics(out: &mut dyn Write, mode: ListMetricsMode) -> io::Result<()> {
    match mode {
        ListMetricsMode::Names => {
            for (name, _) in rows() {
                writeln!(out, "{name}")?;
            }
        }
        ListMetricsMode::Descriptions => {
            let width = rows().map(|(name, _)| name.len()).max().unwrap_or(0);
            for (name, desc) in rows() {
                writeln!(out, "{name:<width$}  {desc}")?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::doc_markdown)]
mod tests {
    use super::*;

    #[test]
    fn names_unique_and_lowercase() {
        let mut seen = std::collections::HashSet::new();
        for (name, desc) in rows() {
            assert!(!name.is_empty(), "metric name must be non-empty");
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "metric name {name:?} must be ascii lowercase",
            );
            assert!(seen.insert(name), "duplicate metric name {name:?}");
            assert!(!desc.is_empty(), "metric {name:?} missing description");
        }
    }

    #[test]
    fn names_mode_prints_one_per_line() {
        let mut buf = Vec::new();
        write_metrics(&mut buf, ListMetricsMode::Names).expect("write");
        let out = String::from_utf8(buf).expect("utf8");
        let lines: Vec<&str> = out.lines().collect();
        let expected: Vec<&str> = rows().map(|(name, _)| name).collect();
        assert_eq!(lines, expected);
    }

    #[test]
    fn descriptions_mode_includes_descriptions() {
        let mut buf = Vec::new();
        write_metrics(&mut buf, ListMetricsMode::Descriptions).expect("write");
        let out = String::from_utf8(buf).expect("utf8");
        let lines: Vec<&str> = out.lines().collect();
        let expected: Vec<(&str, &str)> = rows().collect();
        assert_eq!(lines.len(), expected.len());
        for (line, (name, desc)) in lines.iter().zip(expected) {
            assert!(
                line.starts_with(name),
                "line {line:?} should start with name"
            );
            assert!(line.contains(desc), "line {line:?} missing description");
        }
    }

    /// The `list-metrics` view must include every top-level metric
    /// category the library emits in `--metrics` output, plus the `loc`
    /// sub-metrics. If `CodeMetrics` gains a new field, this fails until
    /// the library catalog is updated.
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
        let catalog: HashSet<&str> = rows().map(|(name, _)| name).collect();
        for name in &expected {
            assert!(
                catalog.contains(name.as_str()),
                "catalog missing metric {name:?}; CodeMetrics emits it but list-metrics does not"
            );
        }
    }
}
