//! Real-world corpus driver: every suite that walks a checked-out
//! repository under `tests/repositories/` and compares each file's
//! metrics against the snapshots in the `big-code-analysis-output`
//! submodule.
//!
//! Grouped into one binary by #1124 — see `tests/api/main.rs` for the
//! rationale. These six modules held one `#[test]` each and linked a
//! ~280 MB binary apiece to run it.

#[path = "../common/mod.rs"]
mod common;

mod csharp_test;
mod deepspeech_test;
mod irules_test;
mod pdf_js_test;
mod php_test;
mod serde_test;
mod snapshot_guard_test;
