//! Benchmark harness for the `big-code-analysis` metric walk (#1068).

// Production-only `unwrap()` ban. See `[workspace.lints.clippy]` in the
// root `Cargo.toml` for why this is a per-root attribute and not a
// Cargo lint (#1227).
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
//!
//! The crate is split three ways:
//!
//! - [`shapes`] generates the synthetic scaling inputs — nesting on
//!   one axis, sibling count on the other — and pairs each with the
//!   metric selection that exercises one hot path.
//! - [`scaling`] measures those inputs and fits an empirical complexity
//!   exponent, so a regression is caught as a *class* change rather
//!   than as a wall-clock budget overrun.
//! - [`corpus`] resolves a deterministic slice of the checked-out
//!   corpus submodules and reports what it actually contains.
//!
//! Two bench targets drive them: `benches/scaling.rs` (the
//! complexity-class gate) and `benches/metric_walk.rs` (criterion
//! measurements over the corpus slice, per metric). [`cli`] holds the
//! former's argument handling, which lives here rather than in the
//! bench target so it is covered by tests that actually run.
//!
//! See `docs/development/benchmarking.md` for invocation and for the
//! measurement traps this harness exists to prevent.

pub mod cli;
pub mod corpus;
pub mod scaling;
pub mod shapes;
