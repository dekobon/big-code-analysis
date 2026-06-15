// bca: suppress-file(halstead, nargs, nexits, nom)
// `bca diff` tree materialization + diffing; the offenders are many-fn /
// early-return aggregation artifacts, not per-function logic complexity.
// `nom` counts the many small, well-named helpers in this module — a
// file-level extraction artifact, not a per-function complexity signal.

//! Diff-aware filtering for `bca check`.
//!
//! Resolves the git ref to diff `HEAD` against (from the `--since`
//! flag or auto-detection of `BCA_DIFF_BASE` / `GITHUB_BASE_REF` /
//! `GITHUB_EVENT_BEFORE`) and shells out to
//! `git diff --name-only <base>...HEAD` to collect the set of files
//! touched in that range.
//!
//! The result is consumed by `run_check` to:
//!
//! - partition the per-file summary footer into "Files in this range:"
//!   (the developer's own contributions) vs the legacy offender list;
//! - drop violations from files outside the touched set entirely when
//!   `--changed-only` is passed (terser CI output for PR gates).
//!
//! All git invocations are best-effort: when the working directory is
//! not a git checkout, when `git` is not installed, or when the
//! requested base ref does not resolve, the resolver returns `None`
//! and the caller falls back to today's behaviour. The only hard
//! errors are user-supplied flag combinations that cannot be satisfied
//! (e.g. `--changed-only` with no resolvable base).

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Explicit-override env var honoured when `--since` is absent and the
/// GitHub Actions signals are inconclusive. Documented in
/// `recipes/ci.md` for users who want to mimic the GHA auto-detection
/// from a local shell or a non-GHA CI runner.
pub(crate) const BCA_DIFF_BASE_ENV: &str = "BCA_DIFF_BASE";

/// GitHub Actions sets `GITHUB_BASE_REF` to the target branch on
/// `pull_request` events (e.g. `main`). Empty on push / schedule
/// events, so the resolver treats an empty value as absent.
pub(crate) const GITHUB_BASE_REF_ENV: &str = "GITHUB_BASE_REF";

/// GitHub Actions sets `GITHUB_EVENT_BEFORE` to the SHA at HEAD before
/// the push on `push` events. On force-pushed or new branches it is
/// the all-zeroes sentinel (`0000000…`), which the resolver treats as
/// absent.
pub(crate) const GITHUB_EVENT_BEFORE_ENV: &str = "GITHUB_EVENT_BEFORE";

/// Is `sha` the all-zeroes "no previous commit" sentinel GitHub Actions
/// emits in `GITHUB_EVENT_BEFORE` on a force-push or brand-new branch?
///
/// Matching "all ASCII `0`s, non-empty" rather than a fixed 40-char
/// literal covers both SHA-1 (40 zeros) and SHA-256 (64 zeros)
/// object-format repos (#704): a SHA-256 repo's null ref is 64 zeros, so
/// the old exact-length literal compare let it slip through as a real ref
/// and the resolver tried to diff against an unreachable object.
fn is_null_sha(sha: &str) -> bool {
    !sha.is_empty() && sha.bytes().all(|b| b == b'0')
}

/// Origin remote name assumed when expanding `GITHUB_BASE_REF`
/// (`main` → `origin/main`). GitHub Actions always names the remote
/// `origin`, and there is no portable way to discover any other
/// remote name without an extra `git remote` invocation.
const ORIGIN_REMOTE: &str = "origin";

/// Resolved diff base plus the canonicalized set of touched files.
#[derive(Debug, Clone)]
pub(crate) struct DiffScope {
    /// The git ref used as the diff base (`<base>...HEAD` triple-dot
    /// form). Surfaced in the summary footer so the reader knows what
    /// the gate is treating as "their contribution".
    pub(crate) base: String,
    /// Where the base came from. Used in the footer label and in
    /// diagnostics.
    pub(crate) source: DiffSource,
    /// Set of touched files as absolute, canonicalized paths. Paths
    /// that fail canonicalization (deleted in the range, broken
    /// symlinks, missing on disk) are skipped — they cannot match a
    /// surviving `Violation::path` anyway.
    ///
    /// Lookups in [`DiffScope::contains`] also canonicalize their
    /// input, which resolves relative paths against the process CWD.
    /// This matches what the walker does: violation paths are
    /// emitted relative to the same CWD `--paths` was resolved from,
    /// so `cwd.join(violation.path)` and `repo_root.join(git_path)`
    /// always name the same on-disk file regardless of which
    /// subdirectory `bca check` was invoked from.
    pub(crate) changed: HashSet<PathBuf>,
}

/// Provenance of the diff base, surfaced in the summary footer so a
/// reader skimming a CI log knows which env signal the gate latched
/// onto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffSource {
    /// User passed `--since <ref>` explicitly.
    Explicit,
    /// `BCA_DIFF_BASE` env var (the documented override hatch).
    EnvOverride,
    /// `GITHUB_BASE_REF` (`pull_request` event).
    GithubPr,
    /// `GITHUB_EVENT_BEFORE` (`push` event).
    GithubPush,
}

impl DiffSource {
    /// Short label for the summary footer, e.g.
    /// `--since main` vs `auto-detected GITHUB_BASE_REF`.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Explicit => "--since",
            Self::EnvOverride => "BCA_DIFF_BASE",
            Self::GithubPr => "GITHUB_BASE_REF",
            Self::GithubPush => "GITHUB_EVENT_BEFORE",
        }
    }
}

impl DiffScope {
    pub(crate) fn contains(&self, path: &Path) -> bool {
        // `canonicalize_for_match` resolves relative paths against the
        // process CWD. This is the correct base: the walker emits
        // violation paths relative to *its* CWD (the same CWD the
        // shell that invoked `bca check` was in), and the shell's
        // CWD is also where `--paths` was resolved from. So
        // `cwd.join(violation.path)` always names the same file that
        // `repo_root.join(git_diff_path)` names in `collect_changed`
        // — both reach the same on-disk file regardless of which
        // subdirectory the user invoked bca from.
        //
        // `canonicalize_for_match` falls back to the input `PathBuf`
        // when the file does not exist (e.g. the violation references
        // a path that was deleted between the walker emitting it and
        // this lookup running). The constructor's canonicalization
        // also drops missing paths, so a missing-on-both-sides query
        // reliably returns `false`; this is the intended degraded
        // behaviour.
        self.changed.contains(&canonicalize_for_match(path))
    }
}

/// Outcome of [`resolve_scope`] when a scope cannot be produced. The
/// caller decides whether each variant is fatal: `--changed-only`
/// requires a scope, so any non-`Disabled` variant becomes a hard
/// error there.
#[derive(Debug, Clone)]
pub(crate) enum ResolveOutcome {
    /// A scope was successfully resolved.
    Ok(DiffScope),
    /// No signal was present (flag absent and every env var empty).
    /// The caller falls back to today's full-tree behaviour.
    Disabled,
    /// A signal was present but the resolver could not turn it into a
    /// usable diff (not a git checkout, ref doesn't exist, git not
    /// installed, …). Carries the human-readable reason for logging.
    Failed { reason: String, source: DiffSource },
}

pub(crate) fn resolve_scope(since: Option<&str>) -> ResolveOutcome {
    let (base, source) = match since {
        Some(b) => (b.to_string(), DiffSource::Explicit),
        None => match auto_detect_base() {
            Some(pair) => pair,
            None => return ResolveOutcome::Disabled,
        },
    };
    // Refuse dash-leading bases up-front. Git's `--` separator only
    // separates revs from paths, NOT options from positional args —
    // so `git diff --name-only -x...HEAD --` still has git's parser
    // looking at `-x` before it sees the `--`, producing an
    // "unknown option" exit. Bailing here gives the caller a clean
    // diagnostic that names the actual problem instead of a
    // confusing git error.
    if base.starts_with('-') {
        return ResolveOutcome::Failed {
            reason: format!(
                "diff base {base:?} starts with `-`; git would parse it as an option. \
                 Pass a plain ref (e.g. `main`, `origin/release`, a SHA) instead."
            ),
            source,
        };
    }
    match collect_changed(&base) {
        Ok(changed) => ResolveOutcome::Ok(DiffScope {
            base,
            source,
            changed,
        }),
        Err(reason) => ResolveOutcome::Failed { reason, source },
    }
}

/// Inspect the GitHub Actions / override env vars in precedence order
/// and return the first non-empty signal, with its provenance.
fn auto_detect_base() -> Option<(String, DiffSource)> {
    if let Some(v) = non_empty_env(BCA_DIFF_BASE_ENV) {
        return Some((v, DiffSource::EnvOverride));
    }
    if let Some(v) = non_empty_env(GITHUB_BASE_REF_ENV) {
        // GitHub Actions only fetches the branch tip into `FETCH_HEAD`
        // on PR runs; the merge-base is reachable via
        // `origin/<base-ref>`. Local users who set this variable from
        // a shell are responsible for the corresponding `git fetch`.
        return Some((format!("{ORIGIN_REMOTE}/{v}"), DiffSource::GithubPr));
    }
    if let Some(v) = non_empty_env(GITHUB_EVENT_BEFORE_ENV) {
        // The all-zeroes SHA is the documented "no previous commit"
        // sentinel for force-pushed or brand-new branches; treat it
        // as no signal so the caller falls back instead of trying to
        // diff against an unreachable ref.
        if !is_null_sha(&v) {
            return Some((v, DiffSource::GithubPush));
        }
    }
    None
}

fn non_empty_env(name: &str) -> Option<String> {
    // Trim leading/trailing whitespace. A `BCA_DIFF_BASE=' '` typo
    // (or a CI runner that exports an env var with a trailing
    // newline from a shell pipeline) would otherwise pass through
    // and produce a refspec like `origin/ ` that git rejects with a
    // confusing "bad revision" — better to fall through to the next
    // signal in the ladder.
    env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn collect_changed(base: &str) -> Result<HashSet<PathBuf>, String> {
    let repo_root = git_repo_root()?;
    let stdout = run_git(
        // `-c core.quotePath=false` keeps non-ASCII filenames literal
        // (default behaviour C-quotes them with octal escapes, which
        // never canonicalize against the on-disk path). The trailing
        // `--` separates revs from paths in git's grammar so the
        // empty positional tail can't be misread as a path list when
        // the working tree happens to contain a file named like the
        // refspec. NOTE: `--` does NOT protect against dash-leading
        // refs — `git diff -x...HEAD --` still has git's option
        // parser scan `-x` before reaching `--`. `resolve_scope`
        // refuses dash-leading bases up-front so we never reach this
        // call with one.
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--name-only",
            &format!("{base}...HEAD"),
            "--",
        ],
        Some(&repo_root),
    )
    .map_err(|e| e.into_message(&format!("git diff --name-only {base}...HEAD")))?;
    let mut out = HashSet::new();
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let joined = repo_root.join(line);
        // The common (and tested) case is `Ok(canon)` — the lookup
        // side's `canonicalize_for_match` produces the same canonical
        // absolute, so the two sides match.
        //
        // `NotFound` is genuinely unreachable: the file was deleted
        // in the diff range, so the walker cannot produce a violation
        // against it. Drop without storing.
        //
        // Other errors (EACCES on a parent, ELOOP on a symlink cycle,
        // transient EIO) are the audit's "hostile FS state" scenario.
        // Storing the joined absolute form here is a *partial*
        // mitigation: it converges with the lookup side only when
        // the violation path is also absolute (e.g. user ran
        // `--paths /abs/repo`). For relative-path invocations
        // (`--paths .`, the documented CI form), the lookup side's
        // identity fallback returns the relative `Violation::path`,
        // so the joined absolute we store here still doesn't match.
        // A thorough fix would join the lookup-side path to
        // `repo_root` before canonicalize — wave 1 attempted exactly
        // that and reverted because joining changed the production
        // semantics for subdir invocations. Pragmatic compromise:
        // store the joined form so the absolute-`--paths` case stays
        // robust, accept that the relative-`--paths` case is best-
        // effort under hostile FS state. The audit rated this
        // "low severity (requires hostile/transient FS state)".
        match joined.canonicalize() {
            Ok(canon) => {
                out.insert(canon);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                out.insert(joined);
            }
        }
    }
    Ok(out)
}

pub(crate) fn git_repo_root() -> Result<PathBuf, String> {
    let stdout = run_git(&["rev-parse", "--show-toplevel"], None).map_err(|e| match e {
        // Outside a git checkout / git binary missing / non-UTF-8
        // toplevel all collapse to the same diagnostic because the
        // caller cannot do anything actionable with the difference.
        // `collect_changed` does distinguish, since its stderr is
        // worth surfacing to the user — see `GitError::into_message`.
        GitError::Spawn(msg) | GitError::NonUtf8(msg) => msg,
        GitError::NonZero { .. } => "not inside a git checkout".to_string(),
    })?;
    let line = stdout.trim();
    if line.is_empty() {
        return Err("git rev-parse --show-toplevel returned empty".to_string());
    }
    Ok(PathBuf::from(line))
}

/// Failure mode for a single git invocation. Carried by [`run_git`]
/// so callers can decide how to render each shape — `collect_changed`
/// surfaces the original git stderr in the diagnostic, while
/// `git_repo_root` flattens any failure to "not inside a git
/// checkout" because that is the only outcome users care about there.
#[derive(Debug)]
enum GitError {
    /// The OS could not spawn the `git` binary (missing, ENOENT, …).
    Spawn(String),
    /// `git` ran but exited non-zero; carries the trimmed stderr so
    /// the caller can compose it into a human-readable message.
    NonZero { stderr: String },
    /// `git` succeeded but emitted non-UTF-8 stdout. Practically never
    /// happens (refs and paths in this repo are UTF-8), but we don't
    /// want a silent `from_utf8_lossy` to mask a corrupt repo.
    NonUtf8(String),
}

impl GitError {
    /// Render the error as a one-line diagnostic suitable for
    /// `bca: --since/auto-detect via … failed (…)` and the fatal
    /// `--changed-only` path. The `verb` describes what the caller
    /// was trying to do (e.g. `"git diff --name-only main...HEAD"`).
    fn into_message(self, verb: &str) -> String {
        match self {
            Self::Spawn(msg) | Self::NonUtf8(msg) => msg,
            Self::NonZero { stderr } if stderr.is_empty() => format!("{verb} failed"),
            Self::NonZero { stderr } => format!("{verb} failed: {stderr}"),
        }
    }
}

/// Run `git` and return its raw stdout bytes (no UTF-8 decoding). Used
/// for `git cat-file blob` (arbitrary binary content) and the
/// NUL-delimited `git ls-tree -z` listing, whose bytes [`run_git`]'s
/// UTF-8 decode would reject. Shares the spawn / non-zero
/// classification with [`run_git`].
fn run_git_bytes(args: &[&str], cwd: Option<&Path>) -> Result<Vec<u8>, GitError> {
    let output = git_output(args, cwd)?;
    if !output.status.success() {
        return Err(GitError::NonZero {
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(output.stdout)
}

/// Spawn `git` with `args` in `cwd`, mapping spawn failures to the
/// discriminated [`GitError::Spawn`] diagnostics. Returns the raw
/// `Output` so byte and text callers can each interpret stdout.
fn git_output(args: &[&str], cwd: Option<&Path>) -> Result<std::process::Output, GitError> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.output().map_err(|e| {
        // Discriminate the common spawn failure modes so the
        // diagnostic names the actionable problem instead of a
        // generic "failed to invoke git". `NotFound` = git not on
        // PATH (install git, or check the runner's container).
        // `PermissionDenied` = git on PATH but exec blocked (CI
        // policy / SELinux / mode bits).
        let context = match e.kind() {
            std::io::ErrorKind::NotFound => "git binary not found on PATH",
            std::io::ErrorKind::PermissionDenied => "git binary on PATH but execution blocked",
            _ => "failed to invoke git",
        };
        GitError::Spawn(format!("{context}: {e}"))
    })
}

fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<String, GitError> {
    let output = git_output(args, cwd)?;
    if !output.status.success() {
        return Err(GitError::NonZero {
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    std::str::from_utf8(&output.stdout)
        .map(str::to_string)
        .map_err(|e| GitError::NonUtf8(format!("git emitted non-UTF-8 output: {e}")))
}

/// Canonicalize a path for membership testing against
/// [`DiffScope::changed`]. Falls back to the input path on failure so
/// the caller can still answer "is this in the touched set?" — the
/// answer will just be "no" for both sides if neither canonicalizes,
/// which is the correct behaviour for a missing/synthetic path.
pub(crate) fn canonicalize_for_match(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Subcommand for `git rev-parse --verify <ref>^{tree}`, used to confirm
/// a `--since` ref resolves to a tree object before we try to archive
/// it. The `^{tree}` peel rejects refs that exist but do not name a
/// commit/tree (e.g. a blob SHA), giving a precise diagnostic instead of
/// a confusing `git archive` failure downstream.
const TREEISH_SUFFIX: &str = "^{tree}";

/// Validate that `since_ref` can serve as the "before" side of
/// `bca diff --since`: the dash guard from [`resolve_scope`] applies
/// (an unguarded `-x` ref would be parsed as a git option), the process
/// must be inside a git checkout, and the ref must resolve to a tree.
///
/// Unlike `bca check --since` (best-effort: a missing ref silently
/// disables diff scoping), `bca diff --since` is an explicit request to
/// diff against a ref, so every failure here is a hard error the caller
/// renders to stderr before exiting 1.
pub(crate) fn validate_since_ref(since_ref: &str) -> Result<(), String> {
    // Mirror `resolve_scope`'s up-front dash guard: git's `--` only
    // separates revs from paths, not options from args, so a dash-
    // leading ref reaches git's option parser and errors confusingly.
    if since_ref.starts_with('-') {
        return Err(format!(
            "diff --since ref {since_ref:?} starts with `-`; git would parse it as an option. \
             Pass a plain ref (e.g. `HEAD~1`, `main`, a SHA) instead."
        ));
    }
    // Confirm we are in a git checkout (and git is installed) before
    // probing the ref, so the diagnostic names the right problem.
    let repo_root = git_repo_root().map_err(|reason| {
        format!("diff --since {since_ref}: {reason}; --since requires a git checkout")
    })?;
    // Peel to a tree to reject non-tree-ish refs and missing refs alike.
    run_git(
        &["rev-parse", "--verify", "--quiet", &format!("{since_ref}{TREEISH_SUFFIX}")],
        Some(Path::new(&repo_root)),
    )
    .map(|_| ())
    .map_err(|e| {
        e.into_message(&format!(
            "diff --since: ref {since_ref:?} does not resolve to a tree (git rev-parse --verify {since_ref}{TREEISH_SUFFIX})"
        ))
    })
}

/// Materialize the tree at `since_ref` into `dest` (an existing,
/// empty directory — typically a [`tempfile::TempDir`] the caller owns
/// so it auto-cleans on every exit path, including errors).
///
/// Enumerates the blobs in `<ref>`'s tree with `git ls-tree -r -z` and
/// writes each blob's content (read with `git cat-file blob <oid>`) to
/// `dest`. This deliberately replaces the former
/// `git archive --format=tar` route, which silently honoured the
/// `export-ignore` / `export-subst` gitattributes: an `export-ignore`'d
/// source file was omitted from the archive entirely, so it appeared as
/// a spurious "added" file in every diff, and `export-subst`'d files had
/// their `$Format:…$` placeholders rewritten, perturbing their metrics
/// (#838). The `ls-tree` + `cat-file` plumbing never consults
/// gitattributes, so the before-side reflects the source exactly as
/// committed at `<ref>` — matching the unfiltered working-tree walk on
/// the after-side.
///
/// Like the prior `git archive` route, this needs no checkout-state
/// bookkeeping and conflicts with no existing worktree; the only artifact
/// is the caller-owned `dest` dir, auto-removed on drop. An empty-tree
/// ref simply yields an empty `ls-tree`, producing an empty before-side
/// (the #704 BSD-`tar` empty-archive fragility is structurally gone).
///
/// `dest` MUST be empty: blobs are written verbatim, so a pre-populated
/// `dest` would silently mix the ref tree with leftover files. This is
/// enforced (not merely documented) below.
pub(crate) fn materialize_tree(since_ref: &str, dest: &Path) -> Result<(), String> {
    let repo_root =
        git_repo_root().map_err(|reason| format!("diff --since {since_ref}: {reason}"))?;
    let repo_root = Path::new(&repo_root);

    // Enforce the empty-dest contract: a non-empty `dest` would let
    // leftover files masquerade as part of the extracted ref tree,
    // corrupting the before-side metric set. `read_dir().next().is_some()`
    // is a cheap "any entry?" probe.
    let mut entries = std::fs::read_dir(dest).map_err(|e| {
        format!(
            "diff --since: cannot read extraction dir {}: {e}",
            dest.display()
        )
    })?;
    if entries.next().is_some() {
        return Err(format!(
            "diff --since: extraction dir {} is not empty; refusing to mix the \
             extracted <ref> tree with existing files",
            dest.display()
        ));
    }

    // Enumerate the tree with the porcelain-stable `ls-tree -z` format:
    // each NUL-terminated record is `<mode> SP <type> SP <oid> TAB <path>`.
    // `--full-tree` makes paths repo-root-relative regardless of the
    // current subdirectory; `-z` keeps them intact even with newlines or
    // other shell-hostile bytes. The default format (rather than a
    // `--format` template) is used deliberately: it works on older git
    // and avoids embedding a literal tab in an argument.
    let listing = run_git_bytes(
        &["ls-tree", "-r", "-z", "--full-tree", since_ref],
        Some(repo_root),
    )
    .map_err(|e| e.into_message(&format!("diff --since: git ls-tree {since_ref}")))?;

    for record in listing.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let Some((mode, kind, oid, path_bytes)) = parse_ls_tree_record(record) else {
            return Err(format!(
                "diff --since: malformed git ls-tree record for {since_ref}"
            ));
        };
        // `-r` recurses, so trees never appear; a `commit` entry is a
        // submodule gitlink with no blob content to extract — skip it
        // (the after-side walker does not descend into submodules either).
        // A symlink is also a `blob`, but its content is the link target
        // path, not source — materializing that string as a regular file
        // would have the metrics walker parse a bogus file, diverging from
        // the after-side working-tree walk. Skip it by mode.
        if kind != "blob" || mode == GIT_SYMLINK_MODE {
            continue;
        }
        let rel_path = bytes_to_rel_path(path_bytes).ok_or_else(|| {
            format!("diff --since: unsafe or non-UTF-8 path in git ls-tree for {since_ref}")
        })?;

        // `git cat-file` over plumbing never consults gitattributes, so
        // the bytes here are the committed blob verbatim — no
        // `export-ignore` drop, no `export-subst` rewrite.
        let blob = run_git_bytes(&["cat-file", "blob", oid], Some(repo_root))
            .map_err(|e| e.into_message(&format!("diff --since: git cat-file {oid}")))?;

        let out_path = dest.join(rel_path);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "diff --since: cannot create {} for {since_ref}: {e}",
                    parent.display()
                )
            })?;
        }
        std::fs::write(&out_path, &blob).map_err(|e| {
            format!(
                "diff --since: cannot write {} for {since_ref}: {e}",
                out_path.display()
            )
        })?;
    }
    Ok(())
}

/// Git's octal mode for a symbolic link (`ls-tree` reports symlinks as
/// `blob` type with this mode).
const GIT_SYMLINK_MODE: &str = "120000";

/// Split one `git ls-tree -z` record into `(mode, type, oid, path_bytes)`.
/// The record shape is `<mode> SP <type> SP <oid> TAB <path>`; the path
/// is returned as raw bytes (decoded later by [`bytes_to_rel_path`]) so
/// a non-UTF-8 path is a path error, not a parse error. Returns `None`
/// when the metadata prefix is missing the tab or its three
/// space-separated fields.
fn parse_ls_tree_record(record: &[u8]) -> Option<(&str, &str, &str, &[u8])> {
    let tab = record.iter().position(|&b| b == b'\t')?;
    let (meta, path_with_tab) = record.split_at(tab);
    // The metadata prefix (`<mode> <type> <oid>`) is always ASCII, so a
    // UTF-8 decode here is safe and lets us split on spaces.
    let meta = std::str::from_utf8(meta).ok()?;
    let mut fields = meta.split(' ');
    let mode = fields.next()?;
    let kind = fields.next()?;
    let oid = fields.next()?;
    if fields.next().is_some() {
        return None;
    }
    // Skip the leading tab on the path side.
    Some((mode, kind, oid, &path_with_tab[1..]))
}

/// Decode a git-reported (repo-root-relative) path into a [`PathBuf`],
/// rejecting non-UTF-8 paths (consistent with the rest of the CLI, which
/// treats paths as identifiers) and any `..` / absolute component that
/// would escape `dest`. Returns `None` on either failure.
fn bytes_to_rel_path(path_bytes: &[u8]) -> Option<PathBuf> {
    let text = std::str::from_utf8(path_bytes).ok()?;
    let rel = PathBuf::from(text);
    let escapes = rel.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    });
    if escapes { None } else { Some(rel) }
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
