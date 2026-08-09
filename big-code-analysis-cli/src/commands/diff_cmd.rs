//! The `diff` and `diff-baseline` subcommands.

use super::*;

/// Diff two baseline files and print the structured result (issue #382).
///
/// Both files are loaded through [`load_baseline`] — the same reader
/// `bca check` uses — so a supported legacy version is migrated on read
/// and an unsupported version dies with a clear message (exit 1) rather
/// than silently no-matching every entry. The matcher's `tolerance` and
/// `fuzzy` parameters do not influence the flattened entry set, so the
/// defaults are passed: the diff keys on `(path, qualified, metric)`
/// regardless.
///
/// Exits 0 on success by default — the diff is informational, not a
/// gate. With `--exit-code`, exits 2 ([`crate::EXIT_GATE_BREACH`]) when
/// the filtered diff is non-empty (git-diff-style opt-in); a tool error
/// exits 1 regardless.
pub(crate) fn run_command_diff_baseline(args: DiffBaselineArgs) {
    // Validate `--output` before the (cheaper, but still avoidable) load
    // so a bad path fails fast, mirroring `report` / `exemptions`.
    if let Some(ref output) = args.output {
        validate_output_path(output, "diff-baseline");
    }
    let old = load_baseline(&args.old, baseline::DEFAULT_LINE_TOLERANCE, false);
    let new = load_baseline(&args.new, baseline::DEFAULT_LINE_TOLERANCE, false);
    let diff = BaselineDiff::compute(&old.diff_entries(), &new.diff_entries());
    let filter = SectionFilter::from_flags([
        args.added_only,
        args.removed_only,
        args.worsened_only,
        args.improved_only,
    ]);
    let rendered = match args.format {
        OutputFormat::Text => diff.render_tty(filter, &args.strip_prefix),
        OutputFormat::Markdown => diff.render_markdown(filter, &args.strip_prefix),
        // Serialization of a fixed-shape struct of owned scalars cannot
        // fail in practice; surface any future error as a tool error
        // rather than panicking.
        OutputFormat::Json => diff
            .render_json()
            .unwrap_or_else(|e| die(format_args!("failed to serialize diff to JSON: {e}"))),
    };
    write_output_or_stdout(
        args.output.as_deref(),
        "write diff-baseline to",
        rendered.as_bytes(),
    );
    // Opt-in metric-gate signal (#692): exit 2 when the filtered diff
    // carries any entry, so CI can detect "something changed" without
    // parsing the output. Off by default — the diff stays informational.
    if args.exit_code && !diff.is_empty_under(filter) {
        process::exit(crate::EXIT_GATE_BREACH);
    }
}

pub(crate) fn run_command_diff(globals: GlobalOpts, args: crate::DiffArgs) {
    // Validate every `--metric` name against the catalog up front, so a
    // typo (`--metric cylomatic`) errors with a did-you-mean (exit 1)
    // instead of silently filtering the diff to nothing (#662). Reuses
    // the `check --threshold` known-names + suggestion machinery and
    // accepts the #514 dotted/alias spellings the diff filter handles.
    crate::metric_alias::validate_diff_metrics(&args.metric).unwrap_or_else(|e| die(e));
    // Validate `--output` before the (potentially slow) `--since` analysis
    // walk so a bad path fails fast, mirroring `report` / `exemptions`.
    if let Some(ref output) = args.output {
        validate_output_path(output, "diff");
    }
    let diff = if let Some(since_ref) = args.since.as_deref() {
        // `--since` takes at most one positional (the after-side tree),
        // which clap binds to `old` first. A second positional (`new`)
        // is ambiguous in this mode, so reject it with a clear message
        // rather than silently ignoring it.
        if args.new.is_some() {
            die(
                "bca diff --since takes at most one positional (the after-side tree); \
                 omit it to diff against the working tree",
            );
        }
        compute_since_diff(&globals, &args, since_ref)
    } else {
        // File/dir mode: both positionals are required captured metric
        // sets, so reaching here with `--since` absent means `old`/`new`
        // came from the positionals; enforce both are present.
        let Some(old) = args.old.as_deref() else {
            die("bca diff: provide two metric-output paths (<old> <new>) or use --since <ref>");
        };
        let Some(new) = args.new.as_deref() else {
            die("bca diff: missing <new> metric-output path (or use --since <ref>)");
        };
        crate::metric_diff::MetricDiff::compute(old, new, args.min_change, &args.metric)
    }
    .unwrap_or_else(|e| die(format_args!("{e}")));
    let rendered = match args.format {
        OutputFormat::Text => diff.render_tty(&args.strip_prefix),
        OutputFormat::Markdown => diff.render_markdown(&args.strip_prefix),
        // Serialization of a fixed-shape struct of owned scalars cannot
        // fail in practice; surface any future error as a tool error
        // rather than panicking.
        OutputFormat::Json => diff
            .render_json()
            .unwrap_or_else(|e| die(format_args!("failed to serialize diff to JSON: {e}"))),
    };
    write_output_or_stdout(args.output.as_deref(), "write diff to", rendered.as_bytes());
    // Opt-in metric-gate signal (#692): exit 2 when any delta survives the
    // active `--min-change` / `--metric` filtering. Off by default — the
    // diff stays informational.
    if args.exit_code && !diff.is_empty() {
        process::exit(crate::EXIT_GATE_BREACH);
    }
}

pub(crate) fn compute_since_diff(
    globals: &GlobalOpts,
    args: &crate::DiffArgs,
    since_ref: &str,
) -> Result<crate::metric_diff::MetricDiff, crate::metric_diff::DiffError> {
    // Hard-error early on an unresolvable ref / non-git checkout, before
    // creating any temp state, so nothing needs cleaning up on this path.
    diff::validate_since_ref(since_ref).unwrap_or_else(|reason| die(reason));

    // Both sides are rooted at their own tree top (the materialized
    // `<ref>` tree for the before side, the repo root for the after
    // side) and pair on root-relative keys. Selection — `--paths`
    // and the optional positional scope — must therefore be *relative*:
    // an absolute path addresses the live filesystem, not the extracted
    // <ref> tree, so it would walk the current tree for both sides and
    // yield a silent all-zero diff. Reject any absolute selector with a
    // clear message rather than mis-pair.
    let scope = args.old.as_deref();
    let selectors = || globals.paths.iter().map(PathBuf::as_path).chain(scope);
    let absolute_selector = selectors().find(|p| p.is_absolute());
    if let Some(abs) = absolute_selector {
        die(format_args!(
            "diff --since: paths must be relative (got {}); an absolute \
             path cannot address the extracted <ref> tree — scope with a \
             relative --paths / positional instead",
            abs.display()
        ));
    }
    // A relative selector that escapes its walk root via `..` is just as
    // unpairable as an absolute one (#704): on the before side it climbs
    // out of the `/tmp/…` extraction of `<ref>` (reaching unrelated host
    // files or nothing), while on the after side it climbs out of the
    // repo root — so the two sides walk different trees and the diff
    // silently mis-pairs. `materialize_tree` routes every path it
    // materializes through `bytes_to_rel_path`, which rejects `..` and
    // absolute components, so no `..` selector addresses a real tracked
    // file on the before side regardless. Reject it with the same clear
    // message rather than produce a bogus all-zero / partial diff.
    let escaping_selector = selectors().find(|p| escapes_root(p));
    if let Some(esc) = escaping_selector {
        die(format_args!(
            "diff --since: paths must stay within the tree (got {}); a `..` \
             component escapes the extracted <ref> tree and the working \
             tree differently, mis-pairing the diff — scope with an in-tree \
             relative --paths / positional instead",
            esc.display()
        ));
    }

    // `--paths-from` names a file list outside either tree; it cannot be
    // resolved consistently against both the extracted <ref> tree and
    // the working tree, so reject it rather than silently ignore the
    // user's selection (scope a `--since` diff with --paths / globs).
    if globals.paths_from.is_some() {
        die(format_args!(
            "diff --since: --paths-from is not supported; scope the diff \
             with --paths / --include / --exclude instead"
        ));
    }

    // TempDir auto-removes on drop — including every `?` below — so the
    // "no leftover temp trees, even on error" acceptance holds without
    // manual teardown.
    let before_tree = tempfile::TempDir::new().map_err(io_to_diff_error)?;
    diff::materialize_tree(since_ref, before_tree.path()).unwrap_or_else(|reason| die(reason));

    let before = crate::walk_metric_set(
        before_tree.path(),
        side_globals(globals, scope),
        DiffSide::Before,
    )?;

    // After side: always the working tree, rooted at the *git repo root*
    // (not the process CWD) so its root-relative keys line up with the
    // before side — a materialization of the whole ref tree, always
    // rooted at the repo top. This lets `bca diff --since` run from any
    // subdirectory. The optional positional is a relative *scope* folded
    // into both sides by `side_globals` (#497), never an alternate root,
    // so a subtree positional (`bca diff --since HEAD src`) selects the
    // same files on each side instead of mis-rooting the after walk.
    let after_root = diff::git_repo_root().unwrap_or_else(|reason| die(reason));
    let after = crate::walk_metric_set(&after_root, side_globals(globals, scope), DiffSide::After)?;

    Ok(crate::metric_diff::MetricDiff::from_sets(
        &before,
        &after,
        args.min_change,
        &args.metric,
    ))
}

/// Does relative `path` lexically escape its walk root? Walks the
/// components tracking depth: a `..` at depth 0 (or that drives depth
/// below 0 at any point) reaches above the root. `CurDir` and `Normal`
/// components stay at or below it. Used to reject `bca diff --since`
/// selectors that would address different trees on the before/after
/// sides (#704). Absolute paths are rejected earlier, so this only sees
/// relative ones.
pub(crate) fn escapes_root(path: &Path) -> bool {
    let mut depth: i32 = 0;
    for component in path.components() {
        match component {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            Component::Normal(_) => depth += 1,
            // `CurDir` is a no-op; `RootDir`/`Prefix` cannot appear in a
            // relative path (absolute selectors are rejected upstream).
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    false
}

pub(crate) fn side_globals(globals: &GlobalOpts, scope: Option<&Path>) -> GlobalOpts {
    let mut side = globals.clone();
    // The `--since` positional is a relative path *scope*, not a tree
    // root: merge it into `--paths` so both sides walk the same subtree
    // and pair on the same keys (#497). Both sides are rooted at their
    // own tree top (the materialized `<ref>` tree / the repo root), so a
    // scope of `src` selects `src/…` on each — never re-roots one side.
    if let Some(scope) = scope {
        side.paths.push(scope.to_path_buf());
    }
    if side.paths.is_empty() {
        side.paths = vec![PathBuf::from(".")];
    }
    // `--paths-from` is rejected upstream in `compute_since_diff` (it
    // cannot resolve against both the <ref> tree and the working tree),
    // so it is already `None` here — selection is via `--paths`/globs.
    side
}

/// Adapt a `std::io::Error` raised while creating temp state for the
/// `--since` walk into the `DiffError` the caller already renders.
pub(crate) fn io_to_diff_error(source: std::io::Error) -> crate::metric_diff::DiffError {
    crate::metric_diff::DiffError::Read {
        path: PathBuf::from("<temp>"),
        source,
    }
}
