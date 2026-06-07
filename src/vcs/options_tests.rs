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
