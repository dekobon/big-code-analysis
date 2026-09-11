//! Cross-language parity driver: suites that feed the same construct to
//! every language that has one and assert the metrics agree, plus the
//! Cpp/Mozcpp grammar parity pin.
//!
//! Grouped into one binary by #1124 — see `tests/api/main.rs`.

mod cognitive_cross_language_parity;
mod cpp_mozcpp_parity;
mod cyclomatic_cross_language_parity;
mod exit_cross_language_parity;
mod functions_metrics_parity;
mod halstead_set_target_parity;
mod nargs_cross_language_parity;
mod ops_metrics_space_parity;
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
mod space_span_containment;
