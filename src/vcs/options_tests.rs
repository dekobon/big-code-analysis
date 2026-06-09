use std::path::Path;
use std::str::FromStr;

use super::*;

const DAY: i64 = 86_400;

#[test]
fn suffix_units_resolve_to_seconds() {
    assert_eq!(parse_window("90d").expect("90d"), 90 * DAY);
    assert_eq!(parse_window("8w").expect("8w"), 8 * 7 * DAY);
    // Months and years use the average Gregorian length.
    assert_eq!(parse_window("1y").expect("1y"), SECONDS_PER_YEAR);
    assert_eq!(parse_window("12mo").expect("12mo"), 12 * SECONDS_PER_MONTH);
}

#[test]
fn twelve_months_and_one_year_both_round_to_365_days() {
    assert_eq!(secs_to_days(parse_window("12mo").expect("12mo")), 365);
    assert_eq!(secs_to_days(parse_window("1y").expect("1y")), 365);
    assert_eq!(secs_to_days(parse_window("90d").expect("90d")), 90);
}

#[test]
fn iso8601_durations_parse() {
    assert_eq!(parse_window("P90D").expect("P90D"), 90 * DAY);
    assert_eq!(parse_window("P8W").expect("P8W"), 8 * 7 * DAY);
    assert_eq!(parse_window("P1Y").expect("P1Y"), SECONDS_PER_YEAR);
    assert_eq!(parse_window("P12M").expect("P12M"), 12 * SECONDS_PER_MONTH);
    // Combined fields sum.
    assert_eq!(
        parse_window("P1Y6M").expect("P1Y6M"),
        SECONDS_PER_YEAR + 6 * SECONDS_PER_MONTH
    );
}

#[test]
fn bad_windows_are_rejected() {
    // A zero-length window degenerates the walk, so it is rejected too.
    for bad in [
        "", "  ", "12", "10x", "-5d", "P", "PT5S", "12m", "abc", "0d", "P0D", "0w",
    ] {
        assert!(
            matches!(parse_window(bad), Err(Error::InvalidWindow(_))),
            "expected {bad:?} to be rejected"
        );
    }
}

#[test]
fn defaults_match_the_issue_sample() {
    let options = Options::default();
    assert_eq!(options.long_window_days(), 365);
    assert_eq!(options.recent_window_days(), 90);
    assert_eq!(options.reference, "HEAD");
    assert!(options.follow_renames);
    assert!(options.exclude_bots);
    assert!(!options.full_history);
    assert!(!options.include_merges);
    assert_eq!(options.risk_formula, RiskFormula::Weighted);
}

#[test]
fn default_bot_pattern_is_a_valid_regex() {
    // Guards the `expect` documented in `Options::default` / `BotFilter`.
    assert!(regex::Regex::new(DEFAULT_BOT_PATTERN).is_ok());
}

#[test]
fn risk_formula_parses_known_names() {
    assert_eq!(
        "weighted".parse::<RiskFormula>().expect("weighted"),
        RiskFormula::Weighted
    );
    assert_eq!(
        "percentile".parse::<RiskFormula>().expect("percentile"),
        RiskFormula::Percentile
    );
}

#[test]
fn risk_formula_rejects_unknown_name() {
    assert!(
        matches!("bogus".parse::<RiskFormula>(), Err(Error::InvalidFormula(name)) if name == "bogus")
    );
}

#[test]
fn iso8601_unsupported_designator_is_rejected() {
    // 'X' is not a date designator (Y/M/W/D). Distinct from the
    // no-magnitude path (`PT5S`): here a magnitude precedes a bad unit.
    assert!(matches!(parse_window("P5X"), Err(Error::InvalidWindow(_))));
}

#[test]
fn iso8601_trailing_magnitude_without_designator_is_rejected() {
    // "P5" carries a magnitude with no unit designator to close it.
    assert!(matches!(parse_window("P5"), Err(Error::InvalidWindow(_))));
}

#[test]
fn overflowing_windows_are_rejected_not_wrapped() {
    // Magnitudes large enough to overflow i64 seconds must surface a
    // clean InvalidWindow, never a wrapped (and possibly negative)
    // duration. Covers the `checked_mul` guards in both forms.
    for bad in ["999999999999y", "P999999999999Y"] {
        // Match the message, not just the variant, so the test pins the
        // `checked_mul` overflow guard rather than any InvalidWindow path.
        assert!(
            matches!(parse_window(bad), Err(Error::InvalidWindow(msg)) if msg.contains("overflows")),
            "expected {bad:?} to be rejected as overflow"
        );
    }
}

#[test]
fn secs_to_days_saturates_huge_and_floors_negative() {
    // The conversion is total: a span past u32::MAX days saturates
    // rather than wrapping, and a negative second-count floors to 0.
    let huge = (i64::from(u32::MAX) + 10) * SECONDS_PER_DAY;
    assert_eq!(secs_to_days(huge), u32::MAX);
    assert_eq!(secs_to_days(-10 * SECONDS_PER_DAY), 0);
}

#[test]
fn defaults_leave_bus_factor_off_at_avelino_threshold() {
    // The aggregate is opt-in (the repeated JIT-prior walks must not pay
    // for it), and the default coverage threshold is Avelino's 0.5.
    let options = Options::default();
    assert!(!options.compute_bus_factor);
    assert!((options.bus_factor_threshold - 0.5).abs() < f64::EPSILON);
    assert!((DEFAULT_BUS_FACTOR_THRESHOLD - 0.5).abs() < f64::EPSILON);
}

#[test]
fn bus_factor_threshold_accepts_only_the_open_interval() {
    for good in [0.5, 0.9, 0.01, 0.99] {
        let got = validate_bus_factor_threshold(good).expect("valid threshold");
        assert!((got - good).abs() < f64::EPSILON);
    }
    for bad in [0.0, 1.0, -0.1, 1.5, f64::NAN, f64::INFINITY] {
        assert!(
            validate_bus_factor_threshold(bad).is_err(),
            "{bad} must be rejected"
        );
    }
}

#[test]
fn default_file_type_scope_is_metrics() {
    // The new default flips the standalone `bca vcs` ranking from "all
    // tracked files" to "files with metrics" (issue #576).
    assert_eq!(Options::default().file_types, FileTypeScope::Metrics);
    assert_eq!(FileTypeScope::default(), FileTypeScope::Metrics);
}

#[test]
fn file_type_scope_parses_keywords() {
    assert_eq!(
        FileTypeScope::from_str("metrics").expect("metrics"),
        FileTypeScope::Metrics
    );
    assert_eq!(
        FileTypeScope::from_str("all").expect("all"),
        FileTypeScope::All
    );
    // Surrounding whitespace is tolerated on the keyword form.
    assert_eq!(
        FileTypeScope::from_str("  all  ").expect("padded all"),
        FileTypeScope::All
    );
}

#[test]
fn file_type_scope_parses_custom_list_and_normalizes() {
    // Leading dots are stripped, case is lowered, and blanks dropped; the
    // order of first appearance is preserved and duplicates collapse.
    let scope = FileTypeScope::from_str(" .RS, py , rs,, .Py ").expect("custom list");
    assert_eq!(
        scope,
        FileTypeScope::Custom(vec!["rs".to_owned(), "py".to_owned()])
    );
}

#[test]
fn file_type_scope_rejects_empty_and_blank_lists() {
    // An empty value, or one that normalises to nothing, is an error
    // rather than a scope that silently ranks no files.
    for bad in ["", "   ", ",", " , . , "] {
        assert!(
            matches!(
                FileTypeScope::from_str(bad),
                Err(Error::InvalidFileTypeScope(_))
            ),
            "{bad:?} must be rejected as an empty scope"
        );
    }
}

#[test]
fn metrics_scope_includes_source_excludes_non_source() {
    // The `metrics` scope routes through the same extension predicate the
    // metrics walk uses, so a recognised source extension is in scope and
    // docs / config / lockfiles / extension-less files are not.
    let metrics = FileTypeScope::Metrics;
    assert!(metrics.includes(Path::new("src/lib.rs")));
    assert!(metrics.includes(Path::new("app/main.py")));
    assert!(!metrics.includes(Path::new("CHANGELOG.md")));
    assert!(!metrics.includes(Path::new("Cargo.lock")));
    assert!(!metrics.includes(Path::new("Cargo.toml")));
    assert!(!metrics.includes(Path::new("Makefile")));
    assert!(!metrics.includes(Path::new("LICENSE")));
}

#[test]
fn all_scope_includes_everything() {
    let all = FileTypeScope::All;
    assert!(all.includes(Path::new("src/lib.rs")));
    assert!(all.includes(Path::new("CHANGELOG.md")));
    assert!(all.includes(Path::new("Makefile")));
}

#[test]
fn custom_scope_matches_only_listed_extensions_case_insensitively() {
    let scope = FileTypeScope::from_str("rs,toml").expect("custom");
    assert!(scope.includes(Path::new("src/lib.rs")));
    // A custom list is a literal extension filter, so non-source
    // extensions like `toml` are honoured even though bca has no metrics
    // for them.
    assert!(scope.includes(Path::new("Cargo.toml")));
    // Matching is case-insensitive on the file's extension.
    assert!(scope.includes(Path::new("BUILD.RS")));
    assert!(!scope.includes(Path::new("app/main.py")));
    assert!(!scope.includes(Path::new("README.md")));
    // An extension-less file never matches a custom list.
    assert!(!scope.includes(Path::new("Makefile")));
}
