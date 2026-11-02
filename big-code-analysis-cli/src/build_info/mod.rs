//! Build provenance for `bca --version` and the report footers (#1773).
//!
//! `build.rs` bakes the fields in as `BCA_BUILD_*` environment
//! variables; this module reads them with `env!` and renders the long
//! version block. One field is read at run time rather than build time:
//! the linker's GNU build-id note, which does not exist until the
//! binary has been linked ([`elf`]).
//!
//! Of the fields, only `inputs` ever decides anything — the self-scan
//! resolver compares it with the live tree. The others are evidence
//! for a human reading a log or a report later.

pub(crate) mod elf;

#[cfg(test)]
pub(crate) mod digest;

#[cfg(test)]
#[path = "build_info_tests.rs"]
mod tests;

/// What `build.rs` writes for a field it could not determine. The
/// `--version` text prints the word; every machine-readable consumer
/// reads the field through [`build_inputs`] / [`build_git`], which map
/// it to `None` once, so a crates.io `bca` stamps no digest into SARIF
/// and prints no `(build …)` in a footer rather than the literal word.
const UNAVAILABLE: &str = "unavailable";

/// The content digest of everything compiled into this binary, or
/// [`UNAVAILABLE`] for a build from a registry tarball, where the
/// workspace roots do not exist beside the crate. The `--version`
/// spelling; see [`build_inputs`] for the value.
pub(crate) const BUILD_INPUTS: &str = env!("BCA_BUILD_INPUTS");

/// `<sha>` or `<sha>-dirty` for the checkout the binary was built from,
/// or [`UNAVAILABLE`]. The `--version` spelling; see [`build_git`].
const BUILD_GIT: &str = env!("BCA_BUILD_GIT");

const BUILD_TIME: &str = env!("BCA_BUILD_TIME");
const BUILD_RUSTC: &str = env!("BCA_BUILD_RUSTC");
const BUILD_TARGET: &str = env!("BCA_BUILD_TARGET");
const BUILD_PROFILE: &str = env!("BCA_BUILD_PROFILE");

fn known(field: &'static str) -> Option<&'static str> {
    (field != UNAVAILABLE).then_some(field)
}

/// The build-inputs digest, or `None` when the binary was not built
/// from a workspace checkout.
pub(crate) fn build_inputs() -> Option<&'static str> {
    known(BUILD_INPUTS)
}

/// The build revision, or `None` outside a git checkout.
pub(crate) fn build_git() -> Option<&'static str> {
    known(BUILD_GIT)
}

/// The lines clap prints after `bca <version>` for `--version`. The
/// first line stays clap's own, so `-V` and the first line of
/// `--version` are the one-line form every existing consumer parses.
///
/// `debug-assertions` is read from this crate's own `cfg`, not from
/// cargo's `DEBUG` variable, which reports debug *info* and is `true`
/// for this workspace's release profile.
pub(crate) fn long_version() -> String {
    let assertions = if cfg!(debug_assertions) { "on" } else { "off" };
    let binary_id = elf::current_build_id().unwrap_or_else(|| "unavailable".to_owned());
    format!(
        "{}\nbuild inputs: {BUILD_INPUTS}\ngit: {BUILD_GIT}\nbuilt: {BUILD_TIME} \
         (profile {BUILD_PROFILE}, {BUILD_RUSTC}, {BUILD_TARGET}, debug-assertions \
         {assertions})\nbuild id: {binary_id}",
        env!("CARGO_PKG_VERSION"),
    )
}
