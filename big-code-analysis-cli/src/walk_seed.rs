//! Walk-seed re-anchoring.
//!
//! Keeps the walker's emitted path form independent of how the user
//! spelled `--paths` (or how a `bca.toml` manifest resolved it), so
//! that exclude/include globs and baseline keys match the same files
//! regardless of seed form (#488).

use std::path::PathBuf;

/// Exclude globs a `bca.toml` supplied, kept apart from the globs the
/// caller typed because the two anchor to different roots (#1164).
///
/// A `--exclude` / `--check-exclude` glob is written in the shell the
/// user is standing in, so the working directory is its natural anchor.
/// A manifest glob is written against the project layout the manifest
/// describes, and the walk's own `paths = ["."]` seed resolves to the
/// manifest directory — so that directory, not the caller's, is what
/// makes `exclude = ["./vendor/**"]` mean `vendor/` at the project
/// root. Merging the two lists (which is what shipped) forced one
/// anchor on both, and the manifest half silently stopped matching for
/// any caller whose working directory was not the project root.
///
/// Both fields are the manifest's own values only; the CLI's stay in
/// `GlobalOpts::exclude` / `CheckArgs::check_exclude`. Compiling them
/// into separate glob sets is what lets each keep its own anchor, so
/// the two must not be re-merged into one list.
#[derive(Debug, Default, Clone)]
pub(crate) struct ManifestExcludes {
    /// The manifest's inline `exclude` / `[check] exclude` list.
    pub(crate) globs: Vec<String>,
    /// The manifest's `exclude_from` / `[check] exclude_from` file,
    /// already resolved against `dir`. Its lines carry the same anchor
    /// as `globs`.
    pub(crate) globs_from: Option<PathBuf>,
    /// Directory holding the `bca.toml`, and therefore the root every
    /// relative pattern above is written against.
    pub(crate) dir: PathBuf,
}

impl ManifestExcludes {
    /// Whether the manifest configured no exclude patterns at all, so
    /// the caller can keep its allocation-free fast path.
    pub(crate) fn is_empty(&self) -> bool {
        self.globs.is_empty() && self.globs_from.is_none()
    }

    /// `cli` unioned with the manifest's globs, CLI patterns first and
    /// duplicates dropped.
    ///
    /// For *display* only — `--print-effective-config` and
    /// `bca exemptions` report the resolved set, and by effect that is
    /// the union. Glob *matching* must keep the two apart so each keeps
    /// its own anchor; see the type doc.
    pub(crate) fn union_globs(&self, cli: &[String]) -> Vec<String> {
        let mut union = cli.to_vec();
        for glob in &self.globs {
            if !union.contains(glob) {
                union.push(glob.clone());
            }
        }
        union
    }
}

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
    // Try the *as-spelled* seed first. `strip_prefix` is purely lexical and
    // skips `CurDir` components, so a manifest-resolved `/abs/repo/.` strips
    // cleanly against the `/abs/repo` CWD to an empty remainder. This is the
    // whole of the pre-existing behavior, kept first and unchanged so it
    // cannot regress: a seed lexically at or under the CWD keeps the same
    // relative tail it always produced, which is what makes `--paths vendor`
    // and `--paths "$PWD/vendor"` emit one identity even when `vendor` is a
    // symlink to a tree elsewhere (#488).
    //
    // When the lexical strip fails, the seed may still reach the CWD *through*
    // a symlinked ancestor. `current_dir` returns the canonical CWD — the
    // getcwd syscall resolves every symlink component — so such a seed shares
    // no lexical prefix with `cwd` and would otherwise stay absolute, nesting
    // every emitted file under its full path. This is the default on macOS,
    // where a `TempDir` (`/var/folders/…`) and `/tmp` are both symlinks into
    // `/private`. Retry against the canonicalized seed (computed only on the
    // failure path, so the common at/under-CWD seed pays no extra syscall); an
    // unresolvable or genuinely-outside seed keeps its as-spelled absolute
    // form, the only stable identity for it.
    match relative_tail(&seed, &cwd) {
        Some(rel) if rel.as_os_str().is_empty() => PathBuf::from("."),
        Some(rel) => rel,
        // Neither lexically nor canonically under the CWD (an unresolvable or
        // genuinely-outside seed). Keep the as-spelled absolute seed; its
        // emitted paths keep that form, the only stable identity for them.
        None => seed,
    }
}

/// The `root`-relative remainder of `path`, or `None` when it is
/// neither lexically nor canonically under `root`. The lexical strip is
/// tried first (no syscall on the common at/under-root case); the
/// canonical retry covers a path spelled through a symlinked ancestor
/// (the macOS `/tmp` → `/private/tmp` default); the final
/// canonical-`root` retry covers Windows, where `canonicalize` yields a
/// `\\?\`-verbatim path while `current_dir` is typically non-verbatim,
/// so the first two strips never share a prefix — canonicalizing both
/// sides puts them in the same form (it also rescues a Unix `root`
/// reached through a symlink).
///
/// `root` is the current directory for [`reanchor_seed`] and
/// [`file_seed_match_path`], which differ only in what they do with the
/// remainder, and a `bca.toml` directory for
/// [`root_relative_match_path`].
fn relative_tail(path: &std::path::Path, root: &std::path::Path) -> Option<PathBuf> {
    if let Ok(rel) = path.strip_prefix(root) {
        return Some(rel.to_path_buf());
    }
    let canonical = path.canonicalize().ok()?;
    if let Ok(rel) = canonical.strip_prefix(root) {
        return Some(rel.to_path_buf());
    }
    let canonical_root = root.canonicalize().ok()?;
    canonical
        .strip_prefix(&canonical_root)
        .ok()
        .map(PathBuf::from)
}

/// The path to match globs written relative to `root` against, or
/// `None` when `path` does not lie under `root`.
///
/// Exclude globs supplied by a `bca.toml` are written relative to the
/// manifest's own directory — that is the root the walk's
/// `paths = ["."]` seed resolves to, so it is the form the patterns are
/// authored against. Every other anchor in this module resolves against
/// the *process working directory* instead, which coincides with the
/// manifest root only when the caller happens to be standing in it: run
/// `bca check sub/a.rs` one directory down and a `[check] exclude`
/// entry silently stopped matching (#1164).
///
/// A relative `path` is joined onto the working directory first, so the
/// two spellings a per-file caller produces (`a.rs` and
/// `/abs/repo/sub/a.rs`) reach the same answer. `None` — a path outside
/// the project entirely — has no manifest-relative identity, and the
/// caller falls back to the working-directory form rather than
/// silently matching nothing.
///
/// The join runs through [`crate::baseline::lexical_normalize`], the
/// same `.` / `..` folding the baseline path keys use, because
/// `strip_prefix` is purely lexical and would otherwise leave the
/// `..` in place: `bca check ../outside/f.rs` from `sub/` would strip
/// to `sub/../outside/f.rs` and be *exempted* by a `./sub/**` glob
/// describing a directory the file is not in. Folding is deliberately
/// lexical rather than `canonicalize`: it needs no filesystem access,
/// it cannot fail on a path that no longer exists, and it agrees with
/// how the baseline keys the very same file.
pub(crate) fn root_relative_match_path(
    root: &std::path::Path,
    path: &std::path::Path,
) -> Option<PathBuf> {
    let absolute = if path.is_relative() {
        std::env::current_dir().ok()?.join(path)
    } else {
        path.to_path_buf()
    };
    relative_tail(&crate::baseline::lexical_normalize(&absolute), root)
        .filter(|rel| !rel.as_os_str().is_empty())
}

/// The path a *manifest-anchored* glob set should be matched against.
///
/// `root` is the `bca.toml` directory (`None` when no manifest
/// applied), `path` the file as the run knows it, and `cwd_form` the
/// working-directory-anchored spelling every other filter in this
/// module produces. A file inside the manifest's tree is matched in
/// manifest-relative form; one outside it has no manifest-relative
/// identity and falls back to `cwd_form`, so a run pointed at paths
/// outside the project keeps matching exactly as it did before #1164.
///
/// Both the `bca check` gate-exemption set and the walker's
/// override warning apply this rule; it lives here so the two cannot
/// drift into disagreeing about which files a manifest glob describes.
pub(crate) fn manifest_match_path<'a>(
    root: Option<&std::path::Path>,
    path: &std::path::Path,
    cwd_form: &'a std::path::Path,
) -> std::borrow::Cow<'a, std::path::Path> {
    root.and_then(|root| root_relative_match_path(root, path))
        .map_or(
            std::borrow::Cow::Borrowed(cwd_form),
            std::borrow::Cow::Owned,
        )
}

/// The path to match `--include` globs against for an *explicitly named
/// file seed*: its CWD-relative tail when the file lies under the CWD,
/// otherwise the seed as spelled.
///
/// [`reanchor_seed`] deliberately leaves file seeds untouched because it
/// rewrites the *emitted* `name` (which must echo the caller's spelling
/// for single-file API parity, #488). This helper exists for the *match*
/// form only — the same emit/match separation `match_path_for` draws for
/// directory walks (#489) — so `--paths "$PWD/src/foo.rs" --include
/// 'src/**'` matches exactly like `--paths src/foo.rs` does, without
/// changing what the run emits (#726). A relative seed already is its
/// own match form; a file outside the CWD has no relative identity and
/// is matched as spelled (a `**/`-prefixed or `*`-style include still
/// applies to it).
pub(crate) fn file_seed_match_path(seed: &std::path::Path) -> PathBuf {
    if seed.is_relative() {
        return seed.to_path_buf();
    }
    let Ok(cwd) = std::env::current_dir() else {
        return seed.to_path_buf();
    };
    match relative_tail(seed, &cwd) {
        // A file seed can never *be* the CWD, so a non-empty remainder is
        // the only Some shape reachable here; guard anyway.
        Some(rel) if !rel.as_os_str().is_empty() => rel,
        _ => seed.to_path_buf(),
    }
}

/// Strip a single leading `./` from a glob pattern so the bare-relative
/// spelling (`dir/**`) and the explicit-CWD spelling (`./dir/**`) compile
/// to the identical glob (#726).
///
/// `globset` matches are whole-path anchored, so before this strip
/// `./dir/**` matched the `./`-prefixed walk-root form (see
/// [`match_path_for`]) while `dir/**` required the path to *start with*
/// `dir` and silently matched nothing. Normalising the pattern side here
/// and the match-path side in [`strip_cur_dir`] makes the two spellings
/// exactly equivalent. `**/`, `*`, and absolute (`/…`) patterns have no
/// leading `./` and are returned untouched. A doubled-slash spelling
/// (`.//x`) is also returned untouched: stripping it would leave `/x`,
/// silently turning a (malformed) relative pattern into an
/// absolute-anchored one.
pub(crate) fn strip_dot_slash(pattern: &str) -> &str {
    pattern
        .strip_prefix("./")
        .filter(|rest| !rest.starts_with('/'))
        .unwrap_or(pattern)
}

/// Strip a single leading `CurDir` (`.`) component from a match path so it
/// compares in the same no-`./` space as a [`strip_dot_slash`]-normalised
/// pattern (#726).
///
/// `strip_prefix(".")` is purely lexical: it removes only a leading
/// `CurDir` component and leaves absolute or already-bare paths unchanged,
/// so `./dir/foo` becomes `dir/foo` while `/abs/foo` and `dir/foo` pass
/// through verbatim. Applied immediately before `is_match` at both match
/// sites; the emitted output path (baseline keys, report names) derives
/// from the real file path, not this match form, so it is unaffected.
pub(crate) fn strip_cur_dir(match_path: &std::path::Path) -> &std::path::Path {
    match_path.strip_prefix(".").unwrap_or(match_path)
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

/// Anchor `path` to the `./`-relative walk-root form using the first
/// `seed` that *contains* it, delegating the per-seed transform to
/// [`match_path_for`]. For callers that filter *after* the walk (e.g.
/// `[check.exclude]` matching already-emitted violation paths, #493) and
/// so have lost the per-seed association `match_path_for` relies on.
///
/// A `seed` equal to `path` (a single explicit file `--paths`) does not
/// anchor here — the walk's file-seed branch matches it as spelled — so
/// that seed is skipped and a later *directory* seed that contains the
/// path may still anchor it. When none does, the path falls back to
/// [`file_seed_match_path`], the same CWD-relative form the walk's own
/// include filter derives for a file seed; a path outside the CWD keeps
/// its absolute form, the only stable identity for it.
///
/// That fallback is load-bearing (#1146). `bca check "$PWD/f.js"` — the
/// shape both shipped per-edit agent hooks use — reaches here with the
/// file seed as its own only seed, so before the fallback the absolute
/// path was matched verbatim and a `./`-anchored `[check.exclude]` glob
/// never fired. `[check] exclude` is the one exclude surface an
/// explicitly-named path does *not* override, so it has to hold for
/// every spelling of that path.
///
/// `seeds` must already be [`reanchor_seed`]-normalised (the form the
/// walk emitted).
pub(crate) fn anchor_against_seeds(seeds: &[PathBuf], path: &std::path::Path) -> PathBuf {
    seeds
        .iter()
        .find_map(|seed| match path.strip_prefix(seed) {
            // Strictly under `seed` (non-empty remainder): anchor it.
            Ok(rel) if !rel.as_os_str().is_empty() => Some(match_path_for(seed, path)),
            // path == seed (file seed) or not under it: try the next seed.
            _ => None,
        })
        .unwrap_or_else(|| file_seed_match_path(path))
}

#[cfg(test)]
#[path = "walk_seed_tests.rs"]
mod tests;
