//! Long-form rule descriptions and direction predicates for
//! offender output formats (SARIF `rule.shortDescription.text`,
//! GitLab Code Climate `description` prefix). Unknown ids return
//! `None` so each caller picks its own fallback.

#![allow(clippy::doc_markdown)]

/// True when a HIGHER value is healthier for `metric` — currently
/// the Maintainability Index family. Single source of truth for
/// "direction" semantics: [`RULE_DESCRIPTIONS`] uses it to pick
/// the sentence verb, and `code_climate::severity_band` uses it
/// to invert the threshold-breach ratio.
pub(crate) fn is_lower_is_worse(metric: &str) -> bool {
    metric.starts_with("mi.")
}

/// Long-form sentences keyed by metric id. The keys MUST stay in
/// lock-step with `big-code-analysis-cli/src/thresholds.rs::EXTRACTORS`
/// — the `each_extractor_metric_has_a_description` test pins that
/// invariant. Mirrors the `lookup_extractor` shape (linear scan of
/// a static `&[(name, …)]`) for consistency with the CLI side.
///
/// `#[rustfmt::skip]`: the one-line-per-entry layout is part of the
/// readability fix; rustfmt would otherwise wrap each tuple onto
/// four lines.
#[rustfmt::skip]
const RULE_DESCRIPTIONS: &[(&str, &str)] = &[
    ("cognitive", "Cognitive Complexity exceeds the configured threshold."),
    ("cyclomatic", "Cyclomatic Complexity exceeds the configured threshold."),
    ("cyclomatic.modified", "Modified Cyclomatic Complexity exceeds the configured threshold."),
    ("halstead.volume", "Halstead volume exceeds the configured threshold."),
    ("halstead.difficulty", "Halstead difficulty exceeds the configured threshold."),
    ("halstead.effort", "Halstead effort exceeds the configured threshold."),
    ("halstead.time", "Halstead time-to-program exceeds the configured threshold."),
    ("halstead.bugs", "Estimated Halstead bugs exceed the configured threshold."),
    ("loc.sloc", "Source lines of code exceed the configured threshold."),
    ("loc.ploc", "Physical lines of code exceed the configured threshold."),
    ("loc.lloc", "Logical lines of code exceed the configured threshold."),
    ("loc.cloc", "Comment lines of code exceed the configured threshold."),
    ("loc.blank", "Blank lines of code exceed the configured threshold."),
    ("nom", "Number of methods/functions exceeds the configured threshold."),
    ("tokens", "Number of tokens exceeds the configured threshold."),
    ("nexits", "Number of exit points exceeds the configured threshold."),
    ("nargs", "Number of function arguments exceeds the configured threshold."),
    ("mi.original", "Maintainability Index falls below the configured threshold."),
    ("mi.sei", "Maintainability Index (SEI) falls below the configured threshold."),
    ("mi.visual_studio", "Maintainability Index (Visual Studio) falls below the configured threshold."),
    ("abc", "ABC magnitude exceeds the configured threshold."),
    ("wmc", "Weighted Methods per Class exceeds the configured threshold."),
    ("npm", "Number of public methods exceeds the configured threshold."),
    ("npa", "Number of public attributes exceeds the configured threshold."),
];

/// Long-form sentence for a known metric id, or `None`.
pub(crate) fn rule_description(metric: &str) -> Option<&'static str> {
    RULE_DESCRIPTIONS
        .iter()
        .find_map(|(name, desc)| (*name == metric).then_some(*desc))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every metric id emitted by the CLI's threshold engine must
    /// have a long-form sentence. If this test fails, a new metric
    /// shipped without updating `RULE_DESCRIPTIONS`; SARIF and
    /// code-climate would silently fall back to the raw id.
    #[test]
    fn each_extractor_metric_has_a_description() {
        for name in [
            "cognitive",
            "cyclomatic",
            "cyclomatic.modified",
            "halstead.volume",
            "halstead.difficulty",
            "halstead.effort",
            "halstead.time",
            "halstead.bugs",
            "loc.sloc",
            "loc.ploc",
            "loc.lloc",
            "loc.cloc",
            "loc.blank",
            "nom",
            "tokens",
            "nexits",
            "nargs",
            "mi.original",
            "mi.sei",
            "mi.visual_studio",
            "abc",
            "wmc",
            "npm",
            "npa",
        ] {
            assert!(
                rule_description(name).is_some(),
                "no rule description for extractor metric {name:?}",
            );
        }
    }

    #[test]
    fn mi_family_phrasing_matches_direction() {
        for id in ["mi.original", "mi.sei", "mi.visual_studio"] {
            let desc = rule_description(id).expect("mi.* metrics must have descriptions");
            assert!(
                desc.contains("falls below"),
                "{id} description should use `falls below` phrasing: {desc}",
            );
            assert!(is_lower_is_worse(id), "{id} should be lower-is-worse");
        }
    }

    #[test]
    fn non_mi_metrics_are_higher_is_worse() {
        assert!(!is_lower_is_worse("cyclomatic"));
        assert!(!is_lower_is_worse("halstead.effort"));
        assert!(!is_lower_is_worse("loc.sloc"));
    }

    #[test]
    fn unknown_metric_returns_none() {
        assert_eq!(rule_description("not.a.metric"), None);
    }
}
