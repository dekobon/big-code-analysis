//! Cross-language parity driver: suites that feed the same construct to
//! every language that has one and assert the metrics agree, plus the
//! Cpp/Mozcpp grammar parity pin.
//!
//! Grouped into one binary by #1124 — see `tests/api/main.rs`.

mod cognitive_cross_language_parity;
mod cpp_mozcpp_parity;
mod cyclomatic_cross_language_parity;
mod exit_cross_language_parity;
mod halstead_set_target_parity;
mod nargs_cross_language_parity;

// The next three suites share `ops_metrics_space_parity::fixture`, an
// exhaustive `match` on `LANG`, so each sweeps *every* language and each
// asserts `checked > 0`. Their row set is therefore the whole roster,
// and a build that compiled no grammar at all — the
// `no-default-features (lib)` CI configuration — would fail all four of
// their guards rather than skipping them. Gated as a group so that build
// drops them together, which is also required: the other two read
// `fixture` out of the middle one (`.claude/rules/testing.md`, #1286).
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
mod functions_metrics_parity;
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
mod ops_metrics_space_parity;
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
mod space_span_containment;

// Gated on the union of the languages whose fixture rows are `Some`, so
// a build enabling only self-reference-free languages drops the module
// rather than failing its non-vacuity guard (`.claude/rules/testing.md`).
#[cfg(any(
    feature = "cpp",
    feature = "csharp",
    feature = "groovy",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "typescript",
))]
mod self_reference_operand_parity;
