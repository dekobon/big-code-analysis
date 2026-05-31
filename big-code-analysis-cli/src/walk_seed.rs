//! Walk-seed re-anchoring.
//!
//! Keeps the walker's emitted path form independent of how the user
//! spelled `--paths` (or how a `bca.toml` manifest resolved it), so
//! that exclude/include globs and baseline keys match the same files
//! regardless of seed form (#488).

use std::path::PathBuf;

/// Re-anchor a walk seed to the same `./`-relative form a bare
/// `--paths .` would produce.
///
/// The walker emits each file path prefixed by its seed (`ignore`'s
/// `WalkBuilder` does not canonicalise), so `--paths .` yields
/// `./src/foo.rs` while `--paths "$PWD"` or a manifest-resolved
/// absolute `paths = ["."]` yields `/abs/repo/src/foo.rs`. Downstream
/// glob filters (`--exclude` / `--exclude-from` / `.bcaignore`,
/// `[check] exclude`) and the baseline `path` field are anchored to
/// the `./`-prefixed relative form, so an absolute seed silently
/// defeats every exclude and floods the offender set (#488).
///
/// To make exclusion path-form independent — the contract #376
/// documents for baseline keys (`--paths .`, `--paths "$PWD"`,
/// `--paths $(BASE_DIR)` byte-identical) — convert an absolute seed
/// that lies at or under the current directory into the equivalent
/// CWD-relative seed. A seed at the CWD (including the `/abs/repo/.`
/// form a manifest `paths = ["."]` resolves to) becomes `.`, so the
/// walker re-emits the canonical `./`-prefixed paths; a seed under
/// the CWD becomes the relative remainder. Seeds outside the CWD,
/// already-relative seeds, and the CWD-unavailable case are returned
/// unchanged — they already match the patterns or have no relative
/// form to anchor to.
///
/// Only **directory** seeds are re-anchored. Excludes (`--exclude` /
/// `--exclude-from` / `.bcaignore`, `[check] exclude`) only ever
/// filter the entries a *tree walk* discovers; a single explicit
/// file seed is never subject to them, so it has nothing to anchor.
/// Re-anchoring a file seed does only harm: it rewrites the emitted
/// `name` from the absolute path the user passed to a CWD-relative
/// one, breaking `bca metrics --paths /abs/file.rs` parity with the
/// single-file `bca.analyze()` API (which echoes the path verbatim).
/// File seeds — and non-existent seeds, whose kind is unknown — are
/// therefore returned unchanged (#488).
pub(crate) fn reanchor_seed(seed: PathBuf) -> PathBuf {
    if seed.is_relative() {
        return seed;
    }
    // Excludes apply to directory walks only; a single-file seed keeps
    // the (absolute) form the caller spelled so its emitted `name`
    // matches the single-file API. `is_dir()` is false for both files
    // and non-existent paths, leaving each untouched.
    if !seed.is_dir() {
        return seed;
    }
    let Ok(cwd) = std::env::current_dir() else {
        return seed;
    };
    // `strip_prefix` is purely lexical and skips `CurDir` components,
    // so a manifest-resolved `/abs/repo/.` strips cleanly against the
    // `/abs/repo` CWD to an empty remainder.
    match seed.strip_prefix(&cwd) {
        Ok(rel) if rel.as_os_str().is_empty() => PathBuf::from("."),
        Ok(rel) => rel.to_path_buf(),
        // Outside the CWD (e.g. an absolute sibling tree). Leave it as
        // an absolute seed; its emitted paths keep the absolute form,
        // which is the only stable identity available for them.
        Err(_) => seed,
    }
}

#[cfg(test)]
#[path = "walk_seed_tests.rs"]
mod tests;
