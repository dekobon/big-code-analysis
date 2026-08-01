//! Path-discovery driver: how the walk resolves `--paths`, honours
//! include / exclude globs in all their spellings, skips generated
//! files, reports unreadable inputs, and drains its worker channel.
//!
//! Grouped into one binary by #1124 — see
//! `big-code-analysis-cli/tests/check/main.rs` for the rationale.
//!
//! `warning_flag` and `skip_generated` are the workspace's only two
//! users of `common::CwdGuard`, which mutates the *process* working
//! directory to prove the hermetic command builders ignore it. They
//! are deliberately in the same driver so that hazard stays confined to
//! one binary, exactly as wide as it was when they were two. It is
//! inert under `cargo nextest` (a process per test, which is what
//! `make test` and CI run) and bounded to a single spawn under the
//! `cargo test` fallback, where the guard's own mutex already
//! serializes it against its peer.

#[path = "../common/mod.rs"]
mod common;

mod exclude_from;
mod exclude_path_form;
mod explicit_path_excludes;
mod include_exclude_arity;
mod invalid_glob;
mod paths_discovery;
mod read_failures;
mod skip_generated;
mod walk_channel_completeness;
mod warning_flag;
