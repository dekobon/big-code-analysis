//! Change-history (VCS) driver: bus factor, churn history, JIT risk,
//! per-function attribution, trend, file-type routing, and the
//! persistent cache.
//!
//! Grouped into one binary by #1124 — see `tests/api/main.rs`. Every
//! module here needs the `vcs-git` backend, so each carries the feature
//! gate on its `mod` declaration rather than as a crate-level
//! `#![cfg]`: this file's `//!` doc must stay ungated so the
//! no-default-features and minimal-langs CI legs do not see an
//! undocumented empty crate root.

#[cfg(feature = "vcs-git")]
#[path = "../common/mod.rs"]
mod common;

#[cfg(feature = "vcs-git")]
mod vcs_bus_factor;
#[cfg(feature = "vcs-git")]
mod vcs_cache;
#[cfg(feature = "vcs-git")]
mod vcs_file_types;
#[cfg(feature = "vcs-git")]
mod vcs_history;
#[cfg(feature = "vcs-git")]
mod vcs_jit;
#[cfg(feature = "vcs-git")]
mod vcs_per_function;
#[cfg(feature = "vcs-git")]
mod vcs_trend;
