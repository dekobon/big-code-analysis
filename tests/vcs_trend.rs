//! End-to-end historical-trend (VCS) tests against real, deterministic
//! git repositories built through the `git` CLI (issue #333).
//!
//! Every commit carries a fixed identity and UNIX timestamp, and every
//! trend pins its end anchor to [`vcs_fixture::FIXED_NOW`] via
//! `Options::as_of`, so the sampled points and per-point counts are exact
//! and reproducible. Gated behind the `vcs-git` backend feature.
#![cfg(feature = "vcs-git")]
// Exact-equality on f64 is intentional here: the asserted scores are
// compared for ordering/sign or for equality against values the same walk
// produced.
#![allow(clippy::float_cmp)]

use std::path::{Path, PathBuf};

use big_code_analysis::vcs::{self, Options, build_trend};
use big_code_analysis::wire;

mod common;
use common::vcs_fixture::{DAY, FIXED_NOW, Repo};

/// Base options pinned to the fixture clock; the trend's most-recent point
/// lands exactly on `FIXED_NOW`.
fn opts() -> Options {
    Options {
        as_of: Some(FIXED_NOW),
        ..Options::default()
    }
}

/// A three-commit history: `early.rs` is born first and edited twice;
/// `late.rs` only appears at the middle commit. The timestamps are spaced
/// so a 3-point / 300-day trend samples one commit per point.
fn staged_repo() -> Repo {
    let repo = Repo::init();
    repo.write("early.rs", "one\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 300 * DAY,
        "init early",
    );

    repo.write("early.rs", "one\ntwo\n");
    repo.write("late.rs", "x\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 150 * DAY, "add late");

    repo.write("early.rs", "one\ntwo\nthree\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "edit early");
    repo
}

fn series<'a>(trend: &'a vcs::Trend, rel: &str) -> Vec<Option<&'a vcs::Stats>> {
    let target = PathBuf::from(rel);
    let Some((_, points)) = trend.iter().find(|(path, _)| **path == target) else {
        panic!("no series for {rel}");
    };
    points.iter().map(Option::as_ref).collect()
}

#[test]
fn samples_span_endpoints_oldest_first() {
    let repo = staged_repo();
    let trend = build_trend(repo.path(), &opts(), 3, 300 * DAY).expect("trend");
    let points = trend.as_of_points();
    assert_eq!(points.len(), 3);
    assert_eq!(points[0], FIXED_NOW - 300 * DAY, "oldest is end - span");
    assert_eq!(points[2], FIXED_NOW, "newest is the end anchor");
    assert!(points[0] < points[1] && points[1] < points[2]);
}

#[test]
fn a_file_born_later_is_absent_at_earlier_points() {
    // Load-bearing for the historical-tip resolution: a naïve `--as-of`
    // walk anchored at HEAD would count `late.rs` (it is in today's tree)
    // at the oldest point. Re-anchoring at the historical tip leaves it
    // `None` until the commit that actually introduced it.
    let repo = staged_repo();
    let trend = build_trend(repo.path(), &opts(), 3, 300 * DAY).expect("trend");

    let late = series(&trend, "late.rs");
    assert!(
        late[0].is_none(),
        "late.rs did not exist at the oldest point"
    );
    assert!(late[1].is_some(), "late.rs exists from the middle commit");
    assert!(late[2].is_some());

    // early.rs exists at every point.
    let early = series(&trend, "early.rs");
    assert!(
        early.iter().all(Option::is_some),
        "early.rs present throughout"
    );
}

#[test]
fn per_point_commit_counts_grow_with_history() {
    let repo = staged_repo();
    let trend = build_trend(repo.path(), &opts(), 3, 300 * DAY).expect("trend");
    let early = series(&trend, "early.rs");
    // One commit accumulated by the oldest point, two by the middle, three
    // by HEAD (all within the 365-day long window).
    assert_eq!(early[0].expect("p0").commits_long, 1);
    assert_eq!(early[1].expect("p1").commits_long, 2);
    assert_eq!(early[2].expect("p2").commits_long, 3);
}

#[test]
fn points_before_first_commit_are_all_absent() {
    let repo = Repo::init();
    repo.write("only.rs", "a\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 5 * DAY, "init");

    // Span reaches back to before the single commit; the two oldest points
    // predate the repository and must be empty.
    let trend = build_trend(repo.path(), &opts(), 3, 100 * DAY).expect("trend");
    let only = series(&trend, "only.rs");
    assert!(
        only[0].is_none() && only[1].is_none(),
        "predate first commit"
    );
    assert!(only[2].is_some(), "HEAD point sees the file");
}

#[test]
fn invalid_point_count_is_rejected() {
    let repo = staged_repo();
    assert!(matches!(
        build_trend(repo.path(), &opts(), 1, 300 * DAY),
        Err(vcs::Error::InvalidTrend(_))
    ));
}

#[test]
fn wire_projection_keeps_alignment_and_marks_absent_as_null() {
    let repo = staged_repo();
    let trend = build_trend(repo.path(), &opts(), 3, 300 * DAY).expect("trend");
    let wire = wire::VcsTrend::from_trend(&trend, 0, 0);

    assert_eq!(wire.trend_schema_version, vcs::TREND_SCHEMA_VERSION);
    assert_eq!(wire.as_of_points.len(), 3);
    let late = &wire.files["late.rs"];
    assert!(late[0].is_none(), "absent point serializes as null");
    let point1 = late[1].as_ref().expect("present point");
    assert_eq!(
        point1.as_of, wire.as_of_points[1],
        "point carries its as_of"
    );

    // The whole document round-trips through JSON unchanged.
    let json = serde_json::to_string(&wire).expect("serialize");
    let back: wire::VcsTrend = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(wire, back);
}

#[test]
fn wire_top_files_keeps_the_riskiest() {
    let repo = staged_repo();
    let trend = build_trend(repo.path(), &opts(), 3, 300 * DAY).expect("trend");
    let full = wire::VcsTrend::from_trend(&trend, 0, 0);
    assert_eq!(full.files.len(), 2);
    let one = wire::VcsTrend::from_trend(&trend, 1, 0);
    assert_eq!(one.files.len(), 1, "top_files = 1 keeps a single series");
}

#[test]
fn deltas_flag_a_file_whose_risk_moved() {
    // early.rs is edited across all three points; whichever direction its
    // risk moved, it must land in exactly one delta list (its endpoints
    // differ), and never in both.
    let repo = staged_repo();
    let trend = build_trend(repo.path(), &opts(), 3, 300 * DAY).expect("trend");
    let deltas = trend.deltas(0);
    let in_improved = deltas
        .improved
        .iter()
        .any(|d| d.path == Path::new("early.rs"));
    let in_regressed = deltas
        .regressed
        .iter()
        .any(|d| d.path == Path::new("early.rs"));
    assert!(
        in_improved ^ in_regressed,
        "early.rs risk moved, so it belongs to exactly one list"
    );
}
