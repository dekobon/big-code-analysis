//! Structured-output driver: the CSV, SARIF, and Checkstyle
//! serializers.
//!
//! Grouped into one binary by #1124 — see `tests/api/main.rs`. The
//! `insta` snapshots these modules own moved with them, from
//! `tests/snapshots/` to `tests/output_formats/snapshots/`, because
//! insta resolves the snapshot directory from the asserting file's own
//! location. They were also renamed: insta keys the
//! `<prefix>__<name>.snap` prefix on the *whole* of `module_path!()`,
//! not its last component, so `csv_test__csv_cpp_widget` became
//! `output_formats__csv_test__csv_cpp_widget` once these files became
//! modules of this driver rather than crate roots. See "Moving a test
//! file" in `tests/README.md`.

#[path = "../common/mod.rs"]
mod common;

mod checkstyle_test;
mod csv_test;
mod sarif_test;
