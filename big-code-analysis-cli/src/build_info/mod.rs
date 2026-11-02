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
#[path = "digest.rs"]
pub(crate) mod digest;

#[cfg(test)]
#[path = "build_info_tests.rs"]
mod tests;

/// The content digest of everything compiled into this binary, or
/// `unavailable` for a build from a registry tarball, where the
/// workspace roots do not exist beside the crate.
pub(crate) const BUILD_INPUTS: &str = env!("BCA_BUILD_INPUTS");

/// `<sha>` or `<sha>-dirty` for the checkout the binary was built from,
/// or `unavailable`.
pub(crate) const BUILD_GIT: &str = env!("BCA_BUILD_GIT");

/// RFC 3339 UTC; `SOURCE_DATE_EPOCH` when it was set at build time.
pub(crate) const BUILD_TIME: &str = env!("BCA_BUILD_TIME");

const BUILD_RUSTC: &str = env!("BCA_BUILD_RUSTC");
const BUILD_TARGET: &str = env!("BCA_BUILD_TARGET");
const BUILD_PROFILE: &str = env!("BCA_BUILD_PROFILE");

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
         {assertions})\nbinary id: {binary_id}",
        env!("CARGO_PKG_VERSION"),
    )
}
