//! Structured-output driver: the CSV, SARIF, and Checkstyle
//! serializers.
//!
//! Grouped into one binary by #1124 — see `tests/api/main.rs`. The
//! `insta` snapshots these modules own moved with them, from
//! `tests/snapshots/` to `tests/output_formats/snapshots/`: insta
//! resolves the snapshot directory from the asserting file's own
//! location. Their names are unchanged, because insta keys the
//! `<prefix>__<name>.snap` prefix on the *last* component of
//! `module_path!()` — still `csv_test` / `sarif_test` now that they are
//! modules rather than crate roots.

#[path = "../common/mod.rs"]
mod common;

mod checkstyle_test;
mod csv_test;
mod sarif_test;
