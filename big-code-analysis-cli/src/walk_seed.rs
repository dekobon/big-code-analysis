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

/// Compute the path to match exclude/include globs against for a file
/// `path` discovered under the directory walk `seed`.
///
/// `reanchor_seed` (above) makes the walker's *emitted* path form
/// independent of how `--paths` was spelled, but it can only express a
/// walk root **at or under** the current directory: it rewrites an
/// absolute seed to its CWD-relative remainder. A manifest-driven `bca
/// check` invoked from a subdirectory *below* the manifest directory
/// resolves `paths = ["."]` to the manifest dir, an **ancestor** of the
/// CWD, which `reanchor_seed` cannot collapse, so the seed stays
/// absolute and the walker emits absolute file paths that the
/// `./`-anchored deny-set never matches (#489).
///
/// Glob matching must therefore be anchored to the **walk root**, not
/// the CWD: every file discovered under `seed` is matched against its
/// path *relative to that seed*, with a `./` prefix to match the
/// convention the patterns (`.bcaignore`, `--exclude-from`, `[check]
/// exclude`) and a bare `--paths .` walk both use. This is correct for
/// every seed form (absolute, relative, `$PWD`, the reanchored `.`, and
/// a manifest root above the CWD) because the relative tail under the
/// walk root is invariant across all of them.
///
/// `strip_prefix` is purely lexical and skips `CurDir` components, so
/// the already-reanchored `.` seed (whose emitted files carry a leading
/// `./`) strips just as cleanly as an absolute seed: `./vendor/x` minus
/// `.` is `vendor/x`, re-prefixed to `./vendor/x` — no double `./`.
/// When `path` is not under `seed` it is returned unchanged as a
/// defensive fallback (the walker always produces files under `seed`,
/// so this branch is unreachable in practice).
///
/// Used for **directory** seeds only — the sole case excludes apply to.
/// A single explicit file `--paths` seed is matched as the caller
/// spelled it (matching `reanchor_seed`'s contract), so it never
/// reaches this helper.
pub(crate) fn match_path_for(seed: &std::path::Path, path: &std::path::Path) -> PathBuf {
    match path.strip_prefix(seed) {
        Ok(rel) => PathBuf::from(".").join(rel),
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
#[path = "walk_seed_tests.rs"]
mod tests;
