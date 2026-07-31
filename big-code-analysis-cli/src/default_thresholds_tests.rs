//! Tests for the shipped default `[thresholds]` table (#1140).

use std::collections::BTreeMap;

use big_code_analysis::metric_catalog::MetricScope;

use super::{DEFAULT_THRESHOLDS, render_thresholds_block};
use crate::thresholds::{ThresholdSet, known_metric_names};

/// Longest line the scaffolded manifest's comment prose may occupy.
/// The rendered rationale is prefixed with `"# "`, so a rationale line
/// may be two characters shorter than this.
const MANIFEST_COMMENT_WIDTH: usize = 72;

#[test]
fn every_default_name_is_a_known_metric() {
    let known = known_metric_names();
    for entry in DEFAULT_THRESHOLDS {
        assert!(
            known.contains(&entry.name),
            "default threshold {:?} is not a known metric name; \
             `bca init` would scaffold a manifest that `bca check` rejects. \
             Known: {}",
            entry.name,
            known.join(", ")
        );
    }
}

/// The names must also survive `ThresholdSet::build`, which is what
/// `bca check` actually calls — `known_metric_names` only proves the
/// registry lists them, not that a limit of this magnitude is accepted.
#[test]
fn default_table_builds_a_threshold_set() {
    let raw: BTreeMap<String, f64> = DEFAULT_THRESHOLDS
        .iter()
        .map(|e| (e.name.to_string(), f64::from(e.limit)))
        .collect();
    let set = ThresholdSet::build(&raw).expect("the shipped defaults must build a ThresholdSet");
    assert!(!set.is_empty(), "the default table must not be empty");
    assert_eq!(
        set.iter().count(),
        DEFAULT_THRESHOLDS.len(),
        "every default must resolve to exactly one threshold entry"
    );
}

#[test]
fn default_names_are_unique() {
    let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
    for entry in DEFAULT_THRESHOLDS {
        assert!(
            seen.insert(entry.name, entry.limit).is_none(),
            "duplicate default threshold {:?}; the later entry would silently \
             win when the rendered TOML is parsed",
            entry.name
        );
    }
}

/// The rendered block is the only copy of these numbers an adopter
/// sees, so it must parse back to exactly the table. This is the
/// property that replaced the old
/// `init_template_thresholds_match_repo_root` drift test: the scaffold
/// is now pinned to the default table rather than to this repository's
/// own calibration.
#[test]
fn rendered_block_round_trips_to_the_table() {
    let rendered = render_thresholds_block();
    let parsed: toml::Table =
        toml::from_str(&rendered).expect("rendered [thresholds] block parses");
    let thresholds = parsed
        .get("thresholds")
        .and_then(toml::Value::as_table)
        .expect("rendered block has a [thresholds] table");

    assert_eq!(
        thresholds.len(),
        DEFAULT_THRESHOLDS.len(),
        "rendered table has {} keys but the source table has {}",
        thresholds.len(),
        DEFAULT_THRESHOLDS.len()
    );
    for entry in DEFAULT_THRESHOLDS {
        let value = thresholds
            .get(entry.name)
            .unwrap_or_else(|| panic!("rendered table is missing {:?}", entry.name));
        assert_eq!(
            value.as_integer(),
            Some(i64::from(entry.limit)),
            "rendered limit for {:?} does not match the table",
            entry.name
        );
    }
}

/// A dotted key rendered bare is a TOML parse error, and a bare key
/// rendered quoted is merely ugly — but the round-trip test above
/// passes either way for the quoted case, so assert the spelling
/// directly.
#[test]
fn dotted_keys_are_quoted_and_bare_keys_are_not() {
    let rendered = render_thresholds_block();
    for entry in DEFAULT_THRESHOLDS {
        let expected = if entry.name.contains('.') {
            format!("\"{}\" = {}\n", entry.name, entry.limit)
        } else {
            format!("{} = {}\n", entry.name, entry.limit)
        };
        assert!(
            rendered.contains(&expected),
            "expected the rendered block to contain {expected:?}"
        );
    }
}

/// `AGENTS.md` repeats the default table so a coding agent reads the
/// numbers without following a link. That is a second copy, and a
/// second copy drifts. Pin it: the summary table there must list every
/// default, with the same limit and the same scope.
///
/// The scope column is checked against the library catalog rather than
/// against a hand-written list, so a metric whose scope is later
/// corrected in `metric_catalog` fails here until the doc follows.
#[test]
fn agents_md_summary_table_matches_the_default_table() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../AGENTS.md"))
        .expect("repo-root AGENTS.md must be readable from the CLI crate dir");

    for entry in DEFAULT_THRESHOLDS {
        let scope = match big_code_analysis::metric_catalog::scope(entry.name) {
            Some(MetricScope::File) => "file",
            Some(MetricScope::Container) => "container",
            // `Function` is also the fallback the CLI gate applies to an
            // id the catalog does not know, so an unknown id renders the
            // narrowest scope here too rather than failing obscurely.
            Some(MetricScope::Function) | None => "function",
        };
        let row = format!("| `{}` | {} | {scope} |", entry.name, entry.limit);
        assert!(
            text.contains(&row),
            "AGENTS.md's threshold summary table is missing or has drifted from \
             the row {row:?}; update the table under `## Metric thresholds` to \
             match default_thresholds::DEFAULT_THRESHOLDS"
        );
    }
}

/// Every default carries a rationale, and it fits the manifest's
/// comment column. An unexplained number in a scaffolded config is how
/// the pre-#1140 defaults became folklore in the first place.
#[test]
fn every_default_has_a_rationale_that_fits_the_comment_column() {
    for entry in DEFAULT_THRESHOLDS {
        assert!(
            !entry.rationale.is_empty(),
            "default threshold {:?} has no rationale",
            entry.name
        );
        for line in entry.rationale {
            assert!(
                !line.trim().is_empty(),
                "default threshold {:?} has a blank rationale line",
                entry.name
            );
            let rendered_width = line.chars().count() + "# ".len();
            assert!(
                rendered_width <= MANIFEST_COMMENT_WIDTH,
                "rationale line for {:?} renders {rendered_width} columns wide, \
                 over the {MANIFEST_COMMENT_WIDTH}-column manifest comment width: {line:?}",
                entry.name
            );
        }
    }
}
