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
mod nargs_cross_language_parity;
mod ops_metrics_space_parity;
mod space_span_containment;
