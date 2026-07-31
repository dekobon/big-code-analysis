//! Tests for the shipped default `[thresholds]` table (#1140).

use std::collections::{BTreeMap, BTreeSet};

use big_code_analysis::metric_catalog::MetricScope;

use super::{DEFAULT_THRESHOLDS, render_thresholds_block};
use crate::thresholds::{ThresholdSet, known_metric_names, metric_scope};

/// Longest line the scaffolded manifest's comment prose may occupy. A
/// [`super::DefaultThreshold::rationale`] line renders with a `"# "`
/// prefix, so it may be two characters shorter than this. Lives here
/// rather than beside the field because this test is the only thing
/// that enforces it, and a production const nothing reads is dead code.
const MANIFEST_COMMENT_WIDTH: usize = 72;

/// Docs that repeat the default table verbatim. Each is a second copy
/// of the numbers, and a second copy drifts, so each is pinned by
/// `doc_summary_tables_match_the_default_table`. Paths are relative to
/// the CLI crate directory.
const DOCS_REPEATING_THE_TABLE: &[&str] = &[
    "/../AGENTS.md",
    "/../big-code-analysis-book/src/recipes/thresholds.md",
];

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
/// `bca check` actually calls, and which additionally validates the
/// limit itself.
///
/// This does not subsume `every_default_name_is_a_known_metric`:
/// `build` runs `metric_alias::normalize_for_check` first, so it would
/// happily accept a default spelled `sloc`. Rejecting alias spellings
/// in the scaffold is what the other test uniquely covers.
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

/// The docs repeat the default table so a reader (or a coding agent)
/// gets the numbers without following a link. Pin every such copy: each
/// must list every default, with the same limit and the same scope.
///
/// The scope column is resolved through the gate's own
/// `thresholds::metric_scope` rather than re-deriving the
/// unknown-id fallback here, so a metric whose scope is later corrected
/// in `metric_catalog` fails until the docs follow.
#[test]
fn doc_summary_tables_match_the_default_table() {
    for relative in DOCS_REPEATING_THE_TABLE {
        let path = format!("{}{relative}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{path} must be readable from the CLI crate dir: {e}"));

        for entry in DEFAULT_THRESHOLDS {
            let scope = scope_word(entry.name);
            // A prefix match, so a table may carry extra trailing
            // columns (the book's adds an "Anchor" column).
            let row = format!("| `{}` | {} | {scope} |", entry.name, entry.limit);
            assert!(
                text.contains(&row),
                "{path}: threshold summary table is missing or has drifted from \
                 the row {row:?}; update it to match \
                 default_thresholds::DEFAULT_THRESHOLDS"
            );
        }

        // The per-entry loop above only catches a *missing* row. Retiring
        // or renaming a default leaves its old row behind, and a stale row
        // advertises a default `bca init` does not write — so pin the set
        // of documented metrics as well, not just its members.
        let documented = documented_threshold_rows(&text);
        let expected: BTreeSet<String> = DEFAULT_THRESHOLDS
            .iter()
            .map(|e| e.name.to_string())
            .collect();
        assert_eq!(
            documented, expected,
            "{path}: the threshold summary table documents a different set of \
             metrics than default_thresholds::DEFAULT_THRESHOLDS; drop the \
             rows for metrics that are no longer shipped defaults"
        );
    }
}

/// The scope column as the doc tables spell it.
fn scope_word(name: &str) -> &'static str {
    match metric_scope(name) {
        MetricScope::File => "file",
        MetricScope::Container => "container",
        MetricScope::Function => "function",
    }
}

/// Metric names appearing as threshold-summary rows in a Markdown doc.
///
/// A row is recognized only by its full shape — backticked name, integer
/// limit, then one of the three scope words — because both documents
/// carry unrelated tables whose first cell is also backticked (AGENTS.md
/// lists the workspace crates that way, and the book's per-language table
/// backticks metric names in its header row).
fn documented_threshold_rows(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.strip_prefix("| `")?.split_once("` | ")?;
            let (limit, rest) = rest.split_once(" | ")?;
            limit.parse::<u32>().ok()?;
            let scope = rest.split(" |").next()?;
            matches!(scope, "file" | "container" | "function").then(|| name.to_owned())
        })
        .collect()
}

/// The rendered prose carries two hand-curated metric-name inventories:
/// the "Available names" list and the "NOT gated" list. Both are copies
/// of the registry, kept by hand for their family grouping, so both can
/// rot when a metric is added or newly gated.
///
/// This caught a real omission: the NOT-gated list read as exhaustive
/// while silently missing `loc.lloc`, `loc.cloc`, and `loc.blank`.
///
/// Both halves assert against the parsed inventory block, never against
/// the whole rendered string. Searching the whole string makes the
/// assertion unfalsifiable: every known metric also appears as a
/// rendered `name = limit` key or in the *other* inventory, so deleting
/// the "Available names" list outright failed zero tests. Token
/// equality rather than `contains` additionally stops `cyclomatic` from
/// being satisfied by `cyclomatic.modified`.
#[test]
fn rendered_prose_lists_every_known_metric() {
    let rendered = render_thresholds_block();
    let gated: Vec<&str> = DEFAULT_THRESHOLDS.iter().map(|e| e.name).collect();
    let available = inventory_after(&rendered, "Available names:");
    let not_gated = inventory_after(&rendered, "# Metrics intentionally NOT gated:");

    for name in known_metric_names() {
        assert!(
            available.contains(name),
            "the scaffold's \"Available names\" list omits the known metric \
             {name:?}; add it in default_thresholds::THRESHOLDS_HEADER. \
             Listed: {available:?}"
        );
        if gated.contains(&name) {
            continue;
        }
        assert!(
            not_gated.contains(name),
            "{name:?} is a known metric that no default gates, but the \
             scaffold's \"NOT gated\" list does not mention it; that list \
             reads as exhaustive, so a gap there is a wrong claim. \
             Listed: {not_gated:?}"
        );
    }
}

/// Parse one of the rendered header's hand-curated metric-name
/// inventories: the run of deeply-indented comment lines that follows
/// `marker`, split into comma-separated names.
///
/// The indent is what delimits the block — an inventory continuation
/// line is `#` plus nine spaces, while the surrounding editing-rule
/// bullets use `#   * ` / `#     `. Bounding the block this way is the
/// point of the helper: prose elsewhere in the header happens to name
/// `nom`, `wmc`, `npm`, and `npa`, so a looser slice would let those
/// four go missing from the inventory undetected.
fn inventory_after(rendered: &str, marker: &str) -> BTreeSet<String> {
    const CONTINUATION: &str = "#         ";
    let (_, rest) = rendered
        .split_once(marker)
        .unwrap_or_else(|| panic!("the rendered header is missing {marker:?}"));
    rest.lines()
        .skip_while(|line| !line.starts_with(CONTINUATION))
        .take_while(|line| line.starts_with(CONTINUATION))
        .flat_map(|line| line[CONTINUATION.len()..].split(','))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
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
