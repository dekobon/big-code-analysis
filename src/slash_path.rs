//! A path as the `/`-separated string that identifies it in output.
//!
//! One projection for every identifier a path becomes: the
//! `by_directory[].directory` keys the bus-factor report renders here,
//! and in the CLI the `files[].path` of the same `bca vcs` report and the
//! keys `bca diff` pairs its two sets by. The CLI reaches it through the
//! `#[doc(hidden)]` re-export at the crate root, as it does
//! [`format_suggestion`](crate::format_suggestion), so a prefix strip or
//! a non-UTF-8 policy change cannot land in one copy and not the other.

use std::path::Path;

/// A path as the `/`-separated UTF-8 string that identifies it in output
/// and pairing keys, or `None` when it is not UTF-8: identifiers never
/// go through `to_string_lossy` (AGENTS.md), so the caller decides
/// between an error and a warned skip. The OS separator is normalised
/// so a key is the same whether the walk ran on Unix or Windows, where
/// `to_str()` would otherwise yield `nested\y.json` and pair against
/// nothing on the `/`-keyed side.
#[must_use]
pub fn slash_path(path: &Path) -> Option<String> {
    path.to_str()
        .map(|s| s.replace(std::path::MAIN_SEPARATOR, "/"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::slash_path;

    #[test]
    fn a_component_built_path_renders_with_forward_slashes() {
        // Assembled from components, so it carries the OS separator (`\`
        // on Windows); the identifier must read the same on both.
        let rel: PathBuf = ["src", "work.rs"].iter().collect();
        assert_eq!(slash_path(&rel).as_deref(), Some("src/work.rs"));
    }
}
