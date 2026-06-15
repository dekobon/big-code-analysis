//! Unit tests for the bus-factor aggregate. These exercise the pure DoA
//! formula and greedy truck-factor algorithm on synthetic authorship
//! distributions with *known* answers — no git repository involved (the
//! end-to-end backend path is covered by `tests/vcs_bus_factor.rs`).

use std::path::Path;

use super::*;

/// A canonical author identity keyed by a synthetic email.
fn author(name: &str) -> AuthorId {
    AuthorId::new(b"", format!("{name}@example.com").as_bytes())
}

/// Build one file's authorship from `(author, deliveries, first)` tuples.
fn file(path: &str, contributions: &[(&str, u32, bool)]) -> FileAuthorship {
    FileAuthorship {
        path: PathBuf::from(path),
        contributions: contributions
            .iter()
            .map(|&(name, deliveries, first_authorship)| AuthorContribution {
                author: author(name),
                deliveries,
                first_authorship,
            })
            .collect(),
    }
}

#[test]
fn schema_version_is_pinned() {
    // Schema 2: renamed the version-stamp key `schema_version` →
    // `bus_factor_schema_version` (#591).
    assert_eq!(BUS_FACTOR_SCHEMA_VERSION, 2);
}

#[test]
fn empty_input_yields_zero_everywhere() {
    let bf = compute(&[], DEFAULT_COVERAGE_THRESHOLD, false);
    assert_eq!(bf.repo, GroupBusFactor::default());
    assert!(bf.by_directory.is_empty());
}

#[test]
fn single_author_repo_has_bus_factor_one() {
    // One developer solely authors every file: losing them abandons the
    // whole repo, so the bus factor is 1 (the documented downward skew of
    // single-author repositories).
    let files = [
        file("a.rs", &[("alice", 10, true)]),
        file("b.rs", &[("alice", 5, false)]),
        file("c.rs", &[("alice", 3, false)]),
    ];
    let bf = compute(&files, DEFAULT_COVERAGE_THRESHOLD, false);
    assert_eq!(bf.repo.bus_factor, 1);
    assert_eq!(bf.repo.files, 3);
    assert_eq!(bf.repo.authors, 1);
}

#[test]
fn two_single_author_files_need_both_owners_removed() {
    // Two files, each owned by a different solo author. Removing one
    // orphans 1/2 = 50%, which is *not* strictly more than the 0.5
    // threshold, so the bus factor is 2.
    let files = [
        file("a.rs", &[("alice", 5, true)]),
        file("b.rs", &[("bob", 5, true)]),
    ];
    let bf = compute(&files, DEFAULT_COVERAGE_THRESHOLD, false);
    assert_eq!(bf.repo.bus_factor, 2);
    assert_eq!(bf.repo.authors, 2);
}

#[test]
fn shared_authorship_raises_the_factor() {
    // Four files each co-authored by both developers at equal weight: no
    // single departure orphans anything, so the bus factor is 2.
    let files = [
        file("a.rs", &[("alice", 5, true), ("bob", 5, false)]),
        file("b.rs", &[("alice", 5, false), ("bob", 5, true)]),
        file("c.rs", &[("alice", 5, true), ("bob", 5, false)]),
        file("d.rs", &[("alice", 5, false), ("bob", 5, true)]),
    ];
    let bf = compute(&files, DEFAULT_COVERAGE_THRESHOLD, false);
    assert_eq!(bf.repo.bus_factor, 2);
}

#[test]
fn dominant_author_excludes_minor_contributor() {
    // Alice dominates every file; Bob makes a single token change to each.
    // Bob's normalised DoA stays below 0.75, so he is not an author and
    // removing Alice alone abandons the repo: bus factor 1.
    let files = [
        file("a.rs", &[("alice", 40, true), ("bob", 1, false)]),
        file("b.rs", &[("alice", 40, true), ("bob", 1, false)]),
    ];
    let bf = compute(&files, DEFAULT_COVERAGE_THRESHOLD, false);
    assert_eq!(bf.repo.bus_factor, 1);
    // Bob never clears the authorship threshold for any file.
    assert_eq!(bf.repo.authors, 1);
}

#[test]
fn coverage_threshold_changes_the_factor() {
    // Three files, three solo owners. At 0.5, orphaning 2/3 (>50%) takes
    // two removals; at 0.9, orphaning all three takes three.
    let files = [
        file("a.rs", &[("alice", 5, true)]),
        file("b.rs", &[("bob", 5, true)]),
        file("c.rs", &[("carol", 5, true)]),
    ];
    assert_eq!(compute(&files, 0.5, false).repo.bus_factor, 2);
    assert_eq!(compute(&files, 0.9, false).repo.bus_factor, 3);
}

#[test]
fn directory_grouping_is_independent_of_the_repo() {
    // dir1 is Alice's, dir2 is Bob's. Each directory has a bus factor of 1
    // (its sole owner), but the repo needs both removed.
    let files = [
        file("dir1/a.rs", &[("alice", 5, true)]),
        file("dir1/b.rs", &[("alice", 5, false)]),
        file("dir2/c.rs", &[("bob", 5, true)]),
        file("dir2/d.rs", &[("bob", 5, false)]),
    ];
    let bf = compute(&files, DEFAULT_COVERAGE_THRESHOLD, false);
    assert_eq!(bf.repo.bus_factor, 2);

    let dirs: Vec<(&str, u32)> = bf
        .by_directory
        .iter()
        .map(|d| (d.directory.as_str(), d.group.bus_factor))
        .collect();
    assert_eq!(dirs, vec![("dir1", 1), ("dir2", 1)]);
}

#[test]
fn directory_grouping_breaks_out_depth_two() {
    // `src/` is the dominant top-level directory; its immediate
    // subdirectories are broken out at depth 2 (the issue's "each src/
    // subdirectory"), while deeper nesting rolls up into them.
    let files = [
        file("src/vcs/git/mod.rs", &[("alice", 5, true)]),
        file("src/vcs/score.rs", &[("alice", 5, false)]),
        file("src/metrics/abc.rs", &[("bob", 5, true)]),
        file("README.md", &[("carol", 5, true)]),
    ];
    let bf = compute(&files, DEFAULT_COVERAGE_THRESHOLD, false);
    let dirs: Vec<&str> = bf
        .by_directory
        .iter()
        .map(|d| d.directory.as_str())
        .collect();
    // Depth 1 (`src`) and depth 2 (`src/metrics`, `src/vcs`); the
    // root-level README contributes no directory key.
    assert_eq!(dirs, vec!["src", "src/metrics", "src/vcs"]);
}

#[test]
fn result_is_independent_of_input_order() {
    let files = [
        file("dir1/a.rs", &[("alice", 7, true), ("bob", 2, false)]),
        file("dir1/b.rs", &[("bob", 9, true), ("alice", 1, false)]),
        file("dir2/c.rs", &[("carol", 4, true)]),
    ];
    let mut reversed = files.to_vec();
    reversed.reverse();
    assert_eq!(
        compute(&files, DEFAULT_COVERAGE_THRESHOLD, true),
        compute(&reversed, DEFAULT_COVERAGE_THRESHOLD, true),
    );
}

#[test]
fn key_author_ids_emitted_only_on_opt_in() {
    let files = [file("a.rs", &[("alice", 5, true)])];
    assert!(
        compute(&files, DEFAULT_COVERAGE_THRESHOLD, false)
            .repo
            .key_author_ids
            .is_none()
    );
    let opted = compute(&files, DEFAULT_COVERAGE_THRESHOLD, true);
    let ids = opted.repo.key_author_ids.expect("opt-in emits ids");
    // One removed key author, surfaced as a SHA-256 hex digest, never the
    // plaintext email.
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], author("alice").hashed());
    assert!(!ids[0].contains('@'));
}

#[test]
fn doa_rises_with_first_authorship_and_deliveries_falls_with_accepted() {
    let base = AuthorContribution {
        author: author("alice"),
        deliveries: 5,
        first_authorship: false,
    };
    let with_fa = AuthorContribution {
        first_authorship: true,
        ..base.clone()
    };
    let with_more_dl = AuthorContribution {
        deliveries: 20,
        ..base.clone()
    };
    assert!(
        doa(&with_fa, 3) > doa(&base, 3),
        "first authorship raises DoA"
    );
    assert!(
        doa(&with_more_dl, 3) > doa(&base, 3),
        "deliveries raise DoA"
    );
    assert!(
        doa(&base, 50) < doa(&base, 3),
        "more accepted (other-author) changes lower DoA"
    );
}

#[test]
fn directory_keys_cover_depth_one_and_two_only() {
    assert!(directory_keys(Path::new("README.md")).is_empty());
    assert_eq!(
        directory_keys(Path::new("src/lib.rs")),
        vec![PathBuf::from("src")]
    );
    assert_eq!(
        directory_keys(Path::new("src/vcs/git/mod.rs")),
        vec![PathBuf::from("src"), PathBuf::from("src/vcs")]
    );
}

#[test]
fn coverage_threshold_is_clamped_into_the_open_interval() {
    // NaN falls back to the default; out-of-range values are nudged inside
    // (0, 1) so neither extreme degenerates the greedy loop.
    assert!((clamp_threshold(f64::NAN) - DEFAULT_COVERAGE_THRESHOLD).abs() < f64::EPSILON);
    assert!(clamp_threshold(0.0) > 0.0);
    assert!(clamp_threshold(1.0) < 1.0);
    assert!(clamp_threshold(-5.0) > 0.0);
    assert!((clamp_threshold(0.5) - 0.5).abs() < f64::EPSILON);
}

/// Issue #701: the per-file author resolution must not depend on the order
/// of a file's `contributions`, which arrive in non-deterministic HashMap
/// order from the accumulator. Two files with identical authorship but
/// permuted contribution lists must yield the same authors (and the same
/// bus factor / key-author list).
#[test]
fn authors_of_file_is_independent_of_contribution_order() {
    // Two co-equal authors (same DL, same FA) → an exact DoA tie. The
    // resolved author set must be identical regardless of list order.
    let forward = file("m.rs", &[("alice", 4, true), ("bob", 4, true)]);
    let mut backward_contribs = forward.contributions.clone();
    backward_contribs.reverse();
    let backward = FileAuthorship {
        path: forward.path.clone(),
        contributions: backward_contribs,
    };

    let mut a = authors_of_file(&forward);
    let mut b = authors_of_file(&backward);
    a.sort_by_key(|id| id.hashed());
    b.sort_by_key(|id| id.hashed());
    assert_eq!(a, b, "resolved authors must not depend on input order");

    // And the whole aggregate is order-independent over the same file.
    assert_eq!(
        compute(
            std::slice::from_ref(&forward),
            DEFAULT_COVERAGE_THRESHOLD,
            true
        ),
        compute(
            std::slice::from_ref(&backward),
            DEFAULT_COVERAGE_THRESHOLD,
            true
        ),
    );
}

/// Issue #701: `compute` must defensively drop a file whose `contributions`
/// are empty (the documented non-empty invariant) rather than letting it
/// orphan trivially and inflate the bus factor. `FileAuthorship` is `pub`,
/// so an external caller can construct one.
#[test]
fn compute_ignores_empty_contribution_files() {
    let real = file("a.rs", &[("alice", 5, true)]);
    let empty = FileAuthorship {
        path: PathBuf::from("b.rs"),
        contributions: Vec::new(),
    };

    let with_empty = compute(&[real.clone(), empty], DEFAULT_COVERAGE_THRESHOLD, false);
    let without = compute(
        std::slice::from_ref(&real),
        DEFAULT_COVERAGE_THRESHOLD,
        false,
    );

    // The empty file contributes no authorship signal, so it must not
    // change the denominator or the factor.
    assert_eq!(with_empty.repo, without.repo);
    assert_eq!(with_empty.repo.files, 1, "the empty file is excluded");
}

/// Issue #929 / #701: the *degenerate* `max_doa <= 0.0` fallback in
/// [`authors_of_file`] is the only order-sensitive path (its `min_by`
/// hashed-id tie-break is what #701 added). The pre-existing
/// order-independence test feeds positive-DoA inputs, so it only ever
/// exercises the order-stable normal path and never this branch.
///
/// To reach `max_doa <= 0.0` every contributor's DoA must be non-positive.
/// Each author's `accepted_changes` is `total_deliveries - own_deliveries`,
/// and `doa = 3.293 + 0.164·own − 0.321·ln(1 + accepted)`. A high-delivery
/// author always stays positive (the `0.164·own` term dominates), so the
/// only way to drive *every* DoA non-positive is many low-delivery authors:
/// with `K` authors each delivering `1`, every `accepted` is `K − 1` while
/// every `own` is `1`, giving `doa = 3.457 − 0.321·ln(K)`, which turns
/// non-positive once `K ≥ ~47_600`. All authors are then bit-identical, so
/// the DoA tie is exact and the resolved author is decided solely by the
/// hashed-id tie-break — the logic #701 added.
#[test]
fn authors_of_file_degenerate_tie_break_is_order_independent() {
    // Comfortably past the ~47_600 threshold where every DoA goes
    // non-positive (derived above).
    const DEGENERATE_AUTHOR_COUNT: usize = 50_000;

    let names: Vec<String> = (0..DEGENERATE_AUTHOR_COUNT)
        .map(|i| format!("dev{i:05}"))
        .collect();
    let contrib_refs: Vec<(&str, u32, bool)> = names
        .iter()
        .map(|name| (name.as_str(), 1u32, false))
        .collect();

    let forward = file("huge.rs", &contrib_refs);
    let mut backward_contribs = forward.contributions.clone();
    backward_contribs.reverse();
    let backward = FileAuthorship {
        path: forward.path.clone(),
        contributions: backward_contribs,
    };

    // The degenerate branch returns exactly one author (the highest DoA,
    // ties broken by smallest hashed id) — not the multi-author normal
    // path, which would credit every author at the 0.75 ratio threshold.
    // This length check pins that the `max_doa <= 0.0` branch is taken.
    let forward_authors = authors_of_file(&forward);
    let backward_authors = authors_of_file(&backward);
    assert_eq!(
        forward_authors.len(),
        1,
        "degenerate branch credits exactly one author"
    );
    assert_eq!(
        backward_authors.len(),
        1,
        "degenerate branch credits exactly one author"
    );

    // The crux of #701: the resolved author is identical under forward and
    // reversed contribution order. A last-wins `max_by` (no hashed-id
    // tie-break) would return the *last* tied author, which differs once
    // the list is reversed.
    assert_eq!(
        forward_authors[0], backward_authors[0],
        "degenerate-path author resolution must not depend on input order"
    );

    // The deterministic pick is the smallest hashed id among all tied
    // contributors — independent of how the list was ordered.
    let expected = forward
        .contributions
        .iter()
        .map(|c| &c.author)
        .min_by_key(|id| id.hashed())
        .expect("non-empty contributions");
    assert_eq!(
        forward_authors[0], expected,
        "tie-break must pick the smallest hashed id"
    );
}
