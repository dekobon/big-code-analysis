//! End-to-end directory- / repo-level bus-factor tests (issue #332)
//! against real, deterministic git repositories built through the `git`
//! CLI. Every commit carries a fixed identity and UNIX timestamp and the
//! walk pins `as_of` to [`vcs_fixture::FIXED_NOW`], so the `DoA` inputs and
//! the resulting bus factors are exact and reproducible. Gated behind the
//! `vcs-git` backend feature.
#![cfg(feature = "vcs-git")]

use big_code_analysis::vcs::{self, Options, build_history_index};

mod common;
use common::vcs_fixture::{DAY, FIXED_NOW, Repo};

/// Options pinned to the fixture clock with the bus-factor aggregate on.
fn opts() -> Options {
    Options {
        as_of: Some(FIXED_NOW),
        compute_bus_factor: true,
        ..Options::default()
    }
}

/// The per-directory bus factor for `dir`, or `None` when absent.
fn directory(bf: &vcs::BusFactor, dir: &str) -> Option<u32> {
    bf.by_directory
        .iter()
        .find(|d| d.directory == dir)
        .map(|d| d.group.bus_factor)
}

#[test]
fn aggregate_absent_unless_opted_in() {
    // The default walk must not pay for the aggregate (the JIT-prior and
    // per-file-injection paths rely on this).
    let repo = Repo::init();
    repo.write("a.rs", "fn a() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "init");

    let index = build_history_index(repo.path(), &Options::default()).expect("walk");
    assert!(index.bus_factor().is_none());

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    assert!(index.bus_factor().is_some());
}

#[test]
fn single_author_repository_has_bus_factor_one() {
    let repo = Repo::init();
    for (i, file) in ["a.rs", "b.rs", "c.rs"].iter().enumerate() {
        repo.write(file, "fn f() {}\n");
        #[allow(clippy::cast_possible_wrap)]
        repo.commit(
            "Ada",
            "ada@example.com",
            FIXED_NOW - (30 - i as i64) * DAY,
            "work",
        );
    }
    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let bf = index.bus_factor().expect("aggregate computed");

    assert_eq!(bf.repo.bus_factor, 1, "one owner abandons the whole repo");
    assert_eq!(bf.repo.files, 3);
    assert_eq!(bf.repo.authors, 1);
    assert_eq!(bf.schema_version, vcs::BUS_FACTOR_SCHEMA_VERSION);
    assert!((bf.coverage_threshold - 0.5).abs() < 1e-9);
}

#[test]
fn two_owners_split_across_directories() {
    // Ada owns everything under `dir1`, Grace everything under `dir2`.
    // Each directory has a bus factor of 1; the repository needs both.
    let repo = Repo::init();
    repo.write("dir1/a.rs", "fn a() {}\n");
    repo.write("dir1/b.rs", "fn b() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 40 * DAY, "ada dir1");
    repo.write("dir1/a.rs", "fn a() {}\n// more\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "ada again");

    repo.write("dir2/c.rs", "fn c() {}\n");
    repo.write("dir2/d.rs", "fn d() {}\n");
    repo.commit(
        "Grace",
        "grace@example.com",
        FIXED_NOW - 20 * DAY,
        "grace dir2",
    );
    repo.write("dir2/c.rs", "fn c() {}\n// more\n");
    repo.commit(
        "Grace",
        "grace@example.com",
        FIXED_NOW - 10 * DAY,
        "grace again",
    );

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let bf = index.bus_factor().expect("aggregate computed");

    assert_eq!(bf.repo.bus_factor, 2, "both owners must leave");
    assert_eq!(bf.repo.authors, 2);
    assert_eq!(directory(bf, "dir1"), Some(1));
    assert_eq!(directory(bf, "dir2"), Some(1));
}

#[test]
fn bots_are_excluded_from_authorship() {
    // A bot solely owns one file; with bot filtering on (the default) that
    // file has no human authorship and drops out of the denominator, so
    // only Ada's file drives the factor.
    let repo = Repo::init();
    repo.write("human.rs", "fn h() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 20 * DAY, "human");
    repo.write("auto.rs", "// generated\n");
    repo.commit(
        "dependabot[bot]",
        "dependabot[bot]@users.noreply.github.com",
        FIXED_NOW - 10 * DAY,
        "bump",
    );

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let bf = index.bus_factor().expect("aggregate computed");

    // Only the human-authored file counts toward the bus factor.
    assert_eq!(bf.repo.files, 1);
    assert_eq!(bf.repo.bus_factor, 1);
    assert_eq!(bf.repo.authors, 1);
}

#[test]
fn aggregate_omitted_when_no_human_authorship() {
    // A repository whose only commit is bot-authored has no human
    // authorship to aggregate, so the bus factor is omitted entirely
    // (`None`) rather than emitting a meaningless "0 over 0 files".
    let repo = Repo::init();
    repo.write("auto.rs", "// generated\n");
    repo.commit(
        "dependabot[bot]",
        "dependabot[bot]@users.noreply.github.com",
        FIXED_NOW - 10 * DAY,
        "bump",
    );
    let index = build_history_index(repo.path(), &opts()).expect("walk");
    assert!(
        index.bus_factor().is_none(),
        "no human authorship ⇒ no aggregate"
    );
}

#[test]
fn key_author_ids_emitted_under_opt_in() {
    let repo = Repo::init();
    repo.write("a.rs", "fn a() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "init");

    let with_details = Options {
        emit_author_details: true,
        ..opts()
    };
    let index = build_history_index(repo.path(), &with_details).expect("walk");
    let bf = index.bus_factor().expect("aggregate computed");
    let ids = bf.repo.key_author_ids.as_ref().expect("ids emitted");
    assert_eq!(
        u32::try_from(ids.len()).expect("small count"),
        bf.repo.bus_factor
    );
    // Hashed, never the plaintext email.
    assert!(ids.iter().all(|id| !id.contains('@')));

    // Default keeps identities inside the process.
    let plain = build_history_index(repo.path(), &opts()).expect("walk");
    assert!(
        plain
            .bus_factor()
            .expect("aggregate")
            .repo
            .key_author_ids
            .is_none()
    );
}
