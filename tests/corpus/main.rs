//! Real-world corpus driver: every suite that walks a checked-out
//! repository under `tests/repositories/` and compares each file's
//! metrics against the snapshots in the `big-code-analysis-output`
//! submodule.
//!
//! Grouped into one binary by #1124 — see `tests/api/main.rs` for the
//! rationale. These six modules held one `#[test]` each and linked a
//! ~280 MB binary apiece to run it.

// Per-language test gating (#1472) makes "is this import live" a
// function of the enabled feature set, which no `cfg` on the import
// itself can express. Partial builds only — the build CI gates on and
// the one a contributor runs still police every unused import. Dead
// *items* are not relaxed: unlike the two library roots this test crate
// carries no `allow(dead_code)`, so every helper, `const`, macro and
// test here still needs its own gate. See `.claude/rules/testing.md`,
// "Why the import lint is off on a partial build".
#![cfg_attr(not(feature = "all-languages"), allow(unused_imports))]
#[path = "../common/mod.rs"]
mod common;

mod csharp_test;
mod deepspeech_test;
mod irules_test;
mod pdf_js_test;
mod php_test;
mod serde_test;
mod snapshot_guard_test;
