//! Pins the workspace lockfile against `h2 0.3` (RUSTSEC-2026-0258).
//!
//! `actix-http` 3 is pinned to `h2 0.3`, whose last release (0.3.27)
//! queues empty HTTP/2 DATA frames without limit; the fix exists only in
//! `h2 0.4.16`. This crate keeps the line out of the graph by building
//! `actix-web` without its `http2` feature — a plaintext daemon never
//! negotiates HTTP/2 anyway — and the lockfile drops the crate once no
//! workspace member can enable it.
//!
//! cargo-deny cannot guard this: krates filters `h2 0.3.27` out of the
//! graph before the advisory and ban checks run, on 0.19 and 0.20 alike,
//! so `deny.toml` is silent whether or not the feature is on. Reading the
//! lockfile is the only observation that fails when `http2` comes back —
//! through a re-enabled default, or a new dependency that turns it on.

use std::path::Path;

/// First `h2` release with the RUSTSEC-2026-0258 fix.
const H2_FIXED: (u64, u64, u64) = (0, 4, 16);

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// Every `h2` entry in the workspace `Cargo.lock`, as `(major, minor, patch)`.
fn locked_h2_versions(lock: &str) -> Vec<(u64, u64, u64)> {
    let mut versions = Vec::new();
    let mut in_h2 = false;
    for line in lock.lines() {
        if line == "[[package]]" {
            in_h2 = false;
        } else if line == "name = \"h2\"" {
            in_h2 = true;
        } else if in_h2 && let Some(v) = line.strip_prefix("version = \"") {
            let v = v.trim_end_matches('"');
            versions
                .push(parse_version(v).unwrap_or_else(|| panic!("unparseable h2 version {v:?}")));
        }
    }
    versions
}

#[test]
fn lockfile_carries_no_h2_release_older_than_the_rustsec_2026_0258_fix() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock");
    let lock = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} must be readable from the web crate dir: {e}",
            path.display()
        )
    });
    let versions = locked_h2_versions(&lock);
    // Non-vacuity: h2 0.4 is in the lock through other dependents today,
    // so an empty scan means the selector broke — or the last dependent
    // left, in which case this test has nothing to pin and should go.
    assert!(
        !versions.is_empty(),
        "no h2 entry in Cargo.lock: either the `[[package]]` scan no longer matches \
         cargo's lockfile format, or nothing depends on h2 any more and this test is \
         obsolete"
    );
    let vulnerable: Vec<_> = versions.iter().filter(|v| **v < H2_FIXED).collect();
    assert!(
        vulnerable.is_empty(),
        "Cargo.lock resolves h2 {vulnerable:?}, below the RUSTSEC-2026-0258 fix \
         {H2_FIXED:?}; something re-enabled `actix-web/http2` (see \
         big-code-analysis-web/Cargo.toml)"
    );
}

#[test]
fn locked_h2_versions_reads_each_package_block_independently() {
    let lock = "[[package]]\nname = \"h2\"\nversion = \"0.3.27\"\n\n\
                [[package]]\nname = \"http\"\nversion = \"0.2.12\"\n\n\
                [[package]]\nname = \"h2\"\nversion = \"0.4.18\"\n";
    assert_eq!(locked_h2_versions(lock), vec![(0, 3, 27), (0, 4, 18)]);
}
