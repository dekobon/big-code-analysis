//! Per-file dispatch for the `bca` walker.
//!
//! `act_on_file` is the entry point: it runs the shared pre-dispatch
//! filters (read + file-count bump, empty-file skip, generated-code skip,
//! language resolution) via `validate_and_resolve_file`, then forwards
//! to the per-action `dispatch_*` helper that implements one `Action`
//! variant. The helpers are intentionally one-screen each so a reader
//! can follow exactly the path a given subcommand takes without
//! scrolling past nine unrelated arms.
//!
//! The metrics / ops helpers analyze each file through the
//! explicit-name `Ast` seam (`parse_ast` → `Ast::metrics` / `Ast::ops`).
//! The display name is the file's UTF-8 path — `None` for a non-UTF-8 path,
//! rather than the lossy-mangled name the retired path-positional shims
//! emitted (#568) — while the `&Path` is still forwarded as the C++
//! preprocessor lookup key.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use big_code_analysis::{
    Ast, FuncSpace, LANG, MetricsError, MetricsOptions, PreprocResults, Source,
    dump_function_spans_with_color, dump_node_with_color, dump_ops_with_color,
    dump_root_with_color, guess_language, is_generated, preprocess, read_file_with_eol, write_file,
};

use crate::exemptions::FileMarkers;
use crate::formats::{MetricsDispatch, MetricsFormat, dump_csv};
use crate::markdown_report::extract_summaries;
use crate::{Action, Config, FEATURES_PINNED, note, warn};

/// Analyze one already-read file via the explicit-name [`Source`] seam.
///
/// The display name carried into [`FuncSpace::name`] is the UTF-8 form
/// of `path` (`None` when the path is not valid UTF-8 — the
/// path-positional shims this replaced instead emitted a lossy-mangled
/// name). `path` is still forwarded as the C++ preprocessor lookup key.
fn analyze_file(
    language: LANG,
    source: Vec<u8>,
    path: &Path,
    pr: Option<Arc<PreprocResults>>,
    options: MetricsOptions,
) -> Result<FuncSpace, MetricsError> {
    parse_ast(language, source, path, pr)?.metrics(options)
}

/// Parse one already-read file into a reusable [`Ast`] via the
/// explicit-name [`Source`] seam, applying the same name / preprocessor
/// wiring as [`analyze_file`]. Backs the non-format dispatch paths that
/// need the parsed handle (dump, ops, comment-strip, function spans,
/// find, count).
fn parse_ast(
    language: LANG,
    source: Vec<u8>,
    path: &Path,
    pr: Option<Arc<PreprocResults>>,
) -> Result<Ast, MetricsError> {
    Ast::parse(
        Source::from_bytes(language, source)
            .with_name(path.to_str().map(str::to_owned))
            .with_preproc_path(Some(path))
            .with_preproc(pr),
    )
}

pub(crate) fn act_on_file(path: PathBuf, cfg: &Config) -> std::io::Result<()> {
    let Some((path, source, language)) = validate_and_resolve_file(path, cfg)? else {
        return Ok(());
    };
    // Tally here rather than in the runner's error printer: everything
    // that reaches this point has already been read, so an error can
    // only have come from emitting the result. The error itself is still
    // returned so the runner prints its per-file line.
    //
    // `BrokenPipe` is excluded for the same reason the printer swallows
    // it — `bca metrics | head` closing the pipe is routine.
    dispatch_action(language, source, path, cfg).inspect_err(|err| {
        if err.kind() != std::io::ErrorKind::BrokenPipe {
            cfg.write_failures.fetch_add(1, Ordering::Relaxed);
        }
    })
}

/// Route one read, non-empty, language-resolved file to the helper that
/// implements `cfg.action`.
fn dispatch_action(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    cfg: &Config,
) -> std::io::Result<()> {
    let pr = cfg.preproc.clone();
    match &cfg.action {
        Action::Dump => dispatch_dump(language, source, path, pr, cfg),
        Action::Metrics { format, pretty } => {
            dispatch_metrics(language, source, path, pr, cfg, format.as_ref(), *pretty)
        }
        Action::Ops { format, pretty } => {
            dispatch_ops(language, source, path, pr, cfg, format.as_ref(), *pretty)
        }
        Action::StripComments { in_place, output } => {
            dispatch_strip_comments(language, source, path, pr, *in_place, output.as_deref())
        }
        Action::Functions => dispatch_functions(language, source, path, pr, cfg),
        Action::Find(filters) => dispatch_find(language, source, path, pr, cfg, filters),
        Action::Count(filters) => dispatch_count(language, source, path, pr, cfg, filters),
        Action::Report => dispatch_report(language, source, path, pr, cfg),
        Action::Check => dispatch_check_file(language, source, path, pr, cfg),
        Action::Exemptions => dispatch_exemptions(language, source, path, pr, cfg),
        Action::PreprocProduce => dispatch_preproc(source, path, cfg),
    }
}

/// Apply the three pre-dispatch filters every CLI subcommand shares:
/// read the file (bumping `files_dispatched` on success and
/// `read_failures` on failure), skip empty files, skip
/// generated files (unless we're producing preproc data — that
/// pipeline genuinely needs every C/C++ file walked), and resolve
/// the source language. Returns `Ok(None)` when the file should be
/// skipped (logging the per-`cfg.warning` reason inline). Returns
/// `Ok(Some((path, source, lang)))` to hand off to dispatch.
fn validate_and_resolve_file(
    path: PathBuf,
    cfg: &Config,
) -> std::io::Result<Option<(PathBuf, Vec<u8>, LANG)>> {
    // Read first, count second: a file we could not open was never
    // analysed, and counting it let the zero-files-matched guard in
    // `run_check` pass a gate run that read nothing at all (#1060). The
    // failure is tallied separately so the caller can distinguish
    // "nothing matched" from "everything was unreadable".
    let source = read_file_with_eol(&path).inspect_err(|_| {
        cfg.read_failures.fetch_add(1, Ordering::Relaxed);
    })?;

    if let Some(counter) = &cfg.files_dispatched {
        // Count every file we managed to read, including those skipped
        // below for empty content / unrecognized language. The user
        // pointed at these files and the runner walked them — they count
        // as "the input was non-empty" for the zero-files-matched check
        // in `run_check`.
        counter.fetch_add(1, Ordering::Relaxed);
    }

    let Some(source) = source else {
        if cfg.warning {
            warn(format_args!("skipping empty file: {}", path.display()));
        }
        return Ok(None);
    };

    if cfg.skip_generated && !matches!(cfg.action, Action::PreprocProduce) && is_generated(&source)
    {
        if cfg.report_skipped || cfg.warning {
            note(format_args!("skipped (generated): {}", path.display()));
        }
        return Ok(None);
    }

    let Some(language) = cfg.language.or_else(|| guess_language(&source, &path).0) else {
        // An explicitly-named file (not a directory-walk product) whose
        // language is unrecognized is a user error, parallel to the #596
        // nonexistent-explicit-path rule: the user named one file and got
        // nothing back. Warn unconditionally (not gated behind `-w`) and
        // tally it so a run that produced no output at all can exit 1
        // (#663). A directory-expanded file stays silently skipped unless
        // `-w` is set — a tree of READMEs/configs must not be noisy.
        if cfg.explicit_seeds.contains(&path) {
            warn(format_args!(
                "skipping explicitly-named file with unrecognized \
                 language: {} (pass --language to force a parser)",
                path.display()
            ));
            if let Some(counter) = &cfg.explicit_unrecognized {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        } else if cfg.warning {
            warn(format_args!(
                "skipping file with unrecognized language: {}",
                path.display()
            ));
        }
        return Ok(None);
    };

    // The file resolved to a recognized language and is about to be
    // dispatched: count it as analyzable output for the #663 zero-output
    // exit-1 check.
    if let Some(counter) = &cfg.output_produced {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    Ok(Some((path, source, language)))
}

fn dispatch_dump(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
) -> std::io::Result<()> {
    // The CLI pins the library's `all-languages` feature, so
    // `LanguageDisabled` from `Ast::parse` is unreachable; the `expect`
    // documents that invariant.
    let ast = parse_ast(language, source, &path, pr).expect(FEATURES_PINNED);
    // Per-file banner so a multi-file dump is attributable: the parallel
    // walk interleaves trees by worker scheduling, and without a header
    // which tree belongs to which file is unrecoverable (#690).
    //
    // The stdout lock is held across both writes so no other worker's
    // banner can land between this one and its tree. `dump_node_with_color`
    // now renders into memory before printing, which widens that window
    // from a few instructions to a whole tree walk; `Stdout::lock` is
    // reentrant, so the nested lock the print takes is fine.
    //
    // The banner is written through the guard rather than `println!`,
    // which panics on a write error instead of returning one (#1132):
    // going through `?` routes a full disk into the walk's
    // `write_failures` tally and leaves `| head` a swallowed `BrokenPipe`.
    let mut out = std::io::stdout().lock();
    writeln!(out, "== {} ==", path.display())?;
    dump_node_with_color(
        ast.source(),
        &ast.root_node(),
        -1,
        cfg.line_start,
        cfg.line_end,
        cfg.color,
    )
}

fn dispatch_metrics(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
    format: Option<&MetricsFormat>,
    pretty: bool,
) -> std::io::Result<()> {
    // bca: suppress(cognitive)
    // Output-mode dispatch: each nesting level is a real emit decision
    // (format present? --vcs? per-function blame? aggregate vs per-file?
    // Generic vs Csv?). Flattening would relocate the nesting, not remove
    // it. Kept in lockstep with the sibling `dispatch_ops`.
    if let Some(fmt) = format {
        if let Ok(mut space) = analyze_file(language, source, &path, pr, cfg.metrics_options()) {
            // `bca metrics --vcs`: attach the file's change-history block
            // (and hotspot) to its file-level metrics before emitting.
            if let Some(index) = &cfg.vcs_index {
                crate::vcs_command::inject(&mut space, &path, index);
                // `--vcs-per-function` (issue #329) additionally blames the
                // file and attaches a block to each nested function space.
                if let Some(blame) = &cfg.vcs_blame {
                    crate::vcs_command::inject_per_function(&mut space, &path, blame);
                }
            }
            // Single-file aggregate mode (`--output <FILE>`, #669): stream
            // the space to the post-walk collector instead of writing a
            // per-file document. The format is applied once, to the whole
            // collected set, by the command runner.
            if let Some(tx) = &cfg.aggregate_tx {
                let _ = tx.send(crate::AggregateItem::Metrics(Box::new(space), path));
                return Ok(());
            }
            // Per-file directory mode (`--output-dir <DIR>`) or stdout.
            match fmt.dispatch() {
                MetricsDispatch::Generic(g) => {
                    g.dump(space, path, cfg.output_dir.as_ref(), pretty)?;
                }
                MetricsDispatch::Csv => {
                    dump_csv(&space, path, cfg.output_dir.as_ref())?;
                }
            }
        }
        Ok(())
    } else {
        // Human-readable metric dump: parse once, then render the tree.
        // A walker error degrades to no output (matching the prior
        // `Metrics` callback), never an `Err`.
        match parse_ast(language, source, &path, pr)
            .expect(FEATURES_PINNED)
            .metrics(cfg.metrics_options())
        {
            Ok(space) => dump_root_with_color(&space, cfg.color),
            Err(_) => Ok(()),
        }
    }
}

fn dispatch_ops(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
    format: Option<&MetricsFormat>,
    pretty: bool,
) -> std::io::Result<()> {
    if let Some(fmt) = format {
        if let Ok(ops) = parse_ast(language, source, &path, pr).and_then(|ast| ast.ops()) {
            // Single-file aggregate mode (`--output <FILE>`, #669): stream
            // the ops tree to the post-walk collector.
            if let Some(tx) = &cfg.aggregate_tx {
                let _ = tx.send(crate::AggregateItem::Ops(Box::new(ops)));
                return Ok(());
            }
            // CSV is rejected upstream in `run()` for the Ops command,
            // so the dispatch here is always Generic. The match is
            // still exhaustive to keep the compiler honest if that
            // upstream guard ever drifts.
            match fmt.dispatch() {
                MetricsDispatch::Generic(g) => {
                    g.dump(ops, path, cfg.output_dir.as_ref(), pretty)?;
                }
                MetricsDispatch::Csv => {}
            }
        }
        Ok(())
    } else {
        // Human-readable ops dump: a walker error degrades to no output
        // (matching the prior `OpsCode` callback), never an `Err`.
        match parse_ast(language, source, &path, pr)
            .expect(FEATURES_PINNED)
            .ops()
        {
            Ok(ops) => dump_ops_with_color(&ops, cfg.color),
            Err(_) => Ok(()),
        }
    }
}

fn dispatch_strip_comments(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    in_place: bool,
    output: Option<&Path>,
) -> std::io::Result<()> {
    // C-family comment removal goes through the dedicated Ccomment
    // grammar for `C` (#721) and both C++ dialects (`Cpp` and the
    // Mozilla fork `Mozcpp`, #720).
    let lang = if matches!(language, LANG::C | LANG::Cpp | LANG::Mozcpp) {
        LANG::Ccomment
    } else {
        language
    };
    let ast = parse_ast(lang, source, &path, pr).expect(FEATURES_PINNED);
    if let Some(new_source) = ast.strip_comments() {
        if in_place {
            write_file(&path, &new_source)?;
        } else if let Some(output) = output {
            write_file(output, &new_source)?;
        } else if let Ok(text) = std::str::from_utf8(&new_source) {
            // Fallible for the same reason as the `dump` banner (#1132):
            // `println!` would panic on a full disk rather than let the
            // walk tally the failure and exit 1.
            writeln!(std::io::stdout().lock(), "{text}")?;
        } else {
            std::io::stdout().write_all(&new_source)?;
        }
    }
    Ok(())
}

fn dispatch_functions(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
) -> std::io::Result<()> {
    let ast = parse_ast(language, source, &path, pr).expect(FEATURES_PINNED);
    dump_function_spans_with_color(ast.functions(), &path, cfg.color)
}

fn dispatch_find(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
    filters: &Arc<[String]>,
) -> std::io::Result<()> {
    let ast = parse_ast(language, source, &path, pr).expect(FEATURES_PINNED);
    // A walker error degrades to no output, matching `dispatch_metrics`
    // / `dispatch_ops`. `Ast::find` is infallible today, but its `Result`
    // is contracted to become fallible under a future strict-parsing mode
    // (see `src/spaces.rs`); skipping the file keeps that future `Err`
    // from panicking a `ConcurrentRunner` worker thread (#839).
    let Ok(found) = ast.find(&filters[..]) else {
        return Ok(());
    };
    if !found.is_empty() {
        // Per-file banner, consistent with `dump` (#690), so interleaved
        // multi-file `find` output stays attributable. The stdout lock is
        // held across the banner, every match, and the trailing blank line
        // for the reason given in `dispatch_dump`.
        let mut out = std::io::stdout().lock();
        writeln!(out, "== {} ==", path.display())?;
        for node in &found {
            dump_node_with_color(
                ast.source(),
                node,
                1,
                cfg.line_start,
                cfg.line_end,
                cfg.color,
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}

// Returns Result<()> for dispatch-table uniformity with sibling
// helpers that propagate I/O errors via `?`; the body never produces an
// `Err` itself (the tally is accumulated into the shared collector).
#[allow(clippy::unnecessary_wraps)]
fn dispatch_count(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
    filters: &Arc<[String]>,
) -> std::io::Result<()> {
    let stats = cfg
        .count_lock
        .clone()
        .expect("Count handler initializes count_lock before dispatch");
    let (good, total) = parse_ast(language, source, &path, pr)
        .expect(FEATURES_PINNED)
        .count(&filters[..]);
    stats.add(good, total);
    Ok(())
}

// Returns Result<()> for dispatch-table uniformity with sibling
// helpers that do propagate I/O errors via `?` (e.g. `dispatch_metrics`).
// The body never produces an `Err` itself.
#[allow(clippy::unnecessary_wraps)]
fn dispatch_report(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
) -> std::io::Result<()> {
    if let Ok(space) = analyze_file(language, source, &path, pr, cfg.metrics_options())
        && let Some(ref tx) = cfg.markdown_tx
        && !matches!(language, LANG::Preproc | LANG::Ccomment)
    {
        // Markdown reports are human-readable text and the downstream
        // `FunctionSummary::file: String` is rendered into the report
        // body, so non-UTF-8 paths cannot round-trip through this
        // pipeline regardless of how we carry them upstream. Skip with
        // a warning. The threshold pipeline (Action::Check) carries
        // `&Path` end-to-end because its JSON/SARIF outputs can
        // preserve raw bytes.
        let Some(file_str) = path.to_str() else {
            if cfg.warning {
                warn(format_args!(
                    "skipping non-UTF-8 path in report: {}",
                    path.display()
                ));
            }
            return Ok(());
        };
        let mut summaries = Vec::new();
        extract_summaries(
            &space,
            file_str,
            language,
            &cfg.strip_prefix,
            &mut summaries,
        );
        for s in summaries {
            let _ = tx.send(s);
        }
        // `report --vcs` joins the file's hotspot score (complexity ×
        // recent churn) onto the change-history section, mirroring
        // `bca metrics --vcs`. Stream the file-level cyclomatic sum keyed
        // by the absolute walk path so the downstream join canonicalizes
        // and matches against the index work-tree exactly like
        // `vcs_command::inject` (issue #615).
        if let Some(ref hotspot_tx) = cfg.report_hotspot_tx {
            #[allow(clippy::cast_precision_loss)]
            let cyclomatic_sum = space.metrics.cyclomatic.cyclomatic_sum() as f64;
            let _ = hotspot_tx.send((path, cyclomatic_sum));
        }
    }
    Ok(())
}

// Returns Result<()> for dispatch-table uniformity; never produces
// an `Err` itself.
#[allow(clippy::unnecessary_wraps)]
fn dispatch_check_file(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
) -> std::io::Result<()> {
    // Retain the source bytes for body hashing only when fuzzy baseline
    // matching is active — the cost (one clone per file) is paid solely
    // by users who opted in via `--baseline-fuzzy-match`.
    let source_for_hash = cfg.fuzzy_baseline.then(|| source.clone());
    if let Ok(space) = analyze_file(language, source, &path, pr, cfg.metrics_options())
        && let (Some(set), Some(tx)) = (cfg.threshold_set.as_ref(), cfg.check_tx.as_ref())
        && !matches!(language, LANG::Preproc | LANG::Ccomment)
    {
        // Pass the path through as `&Path` so non-UTF-8 bytes are
        // preserved on each emitted `Violation`. Display / offender
        // serialization decide their own lossy strategy at the output
        // boundary; the threshold pipeline itself stays byte-faithful.
        let mut violations = Vec::new();
        set.evaluate_with_policy(
            &path,
            &space,
            cfg.suppression_policy,
            cfg.report_suppressed,
            &mut violations,
        );
        if let Some(src) = &source_for_hash {
            // Stamp each offender with a normalised body digest so the
            // baseline can match a renamed-but-unchanged function. The
            // function's own (bare) name is elided from the digest so a
            // pure rename still matches.
            for v in &mut violations {
                let name = crate::baseline::bare_name(&v.function).to_owned();
                v.body_hash = Some(crate::baseline::hash_body(
                    src,
                    v.start_line,
                    v.end_line,
                    &name,
                ));
            }
        }
        // Receiver lives until `run_check` drains `rx`, which happens
        // only after `run_walk` joins all worker threads — so `send`
        // cannot fail here. Use `let _` rather than `expect` to avoid
        // panicking the worker pool on the (unreachable) drop path.
        for v in violations {
            let _ = tx.send(v);
        }
    }
    Ok(())
}

// Returns Result<()> for dispatch-table uniformity; never produces
// an `Err` itself.
#[allow(clippy::unnecessary_wraps)]
fn dispatch_exemptions(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
) -> std::io::Result<()> {
    // Auxiliary grammars (`Preproc`, `Ccomment`) carry no user-authored
    // suppression markers and have no function spaces to attribute them
    // to; skip them so the audit mirrors the set of files `bca check`
    // actually gates.
    let Some(tx) = cfg.exemptions_tx.as_ref() else {
        return Ok(());
    };
    if matches!(language, LANG::Preproc | LANG::Ccomment) {
        return Ok(());
    }
    // The marker report renders the path into human-readable text and a
    // JSON `path` field, so a non-UTF-8 path cannot round-trip; skip it
    // with a warning rather than lossily mangling the identifier.
    let Some(file_str) = path.to_str() else {
        if cfg.warning {
            warn(format_args!(
                "skipping non-UTF-8 path in exemptions audit: {}",
                path.display()
            ));
        }
        return Ok(());
    };
    let markers = parse_ast(language, source, &path, pr)
        .expect(FEATURES_PINNED)
        .suppressions();
    // Empty files are the dominant case (most source carries no
    // markers); skip the channel send and the per-file allocation when
    // there is nothing to report.
    if markers.is_empty() {
        return Ok(());
    }
    // Receiver lives until the post-walk aggregator drains `rx`, which
    // happens only after all worker threads join — so `send` cannot
    // fail. Use `let _` rather than `expect` to avoid panicking the
    // worker pool on the unreachable drop path.
    let _ = tx.send(FileMarkers {
        path: file_str.to_owned(),
        markers,
    });
    Ok(())
}

// Returns Result<()> for dispatch-table uniformity; never produces
// an `Err` itself.
#[allow(clippy::unnecessary_wraps)]
fn dispatch_preproc(source: Vec<u8>, path: PathBuf, cfg: &Config) -> std::io::Result<()> {
    if let Some(preproc_lock) = &cfg.preproc_lock
        && let Some(language) = guess_language(&source, &path).0
        && matches!(language, LANG::C | LANG::Cpp | LANG::Mozcpp)
    {
        let Ok(mut results) = preproc_lock.lock() else {
            if cfg.warning {
                warn(format_args!(
                    "skipping {}: preproc results lock poisoned",
                    path.display()
                ));
            }
            return Ok(());
        };
        preprocess(source, &path, &mut results);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_code_analysis::SuppressionPolicy;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    // Minimal `Config` for exercising `dispatch_preproc` in isolation.
    // Only `preproc_lock` and `warning` are load-bearing here; every
    // other field is defaulted to the inert value used elsewhere.
    fn preproc_test_config(preproc_lock: Option<Arc<Mutex<PreprocResults>>>) -> Config {
        Config {
            action: Action::PreprocProduce,
            output_dir: None,
            aggregate_tx: None,
            language: None,
            line_start: None,
            line_end: None,
            preproc_lock,
            preproc: None,
            count_lock: None,
            markdown_tx: None,
            report_hotspot_tx: None,
            strip_prefix: String::new(),
            threshold_set: None,
            check_tx: None,
            exemptions_tx: None,
            files_dispatched: None,
            read_failures: Arc::new(AtomicUsize::new(0)),
            write_failures: Arc::new(AtomicUsize::new(0)),
            explicit_seeds: Arc::new(std::collections::HashSet::new()),
            explicit_unrecognized: None,
            output_produced: None,
            suppression_policy: SuppressionPolicy::Honor,
            report_suppressed: false,
            warning: false,
            skip_generated: true,
            report_skipped: false,
            exclude_tests: false,
            no_cyclomatic_try: false,
            fuzzy_baseline: false,
            vcs_index: None,
            vcs_blame: None,
            color: big_code_analysis::ColorMode::Never,
            selected_metrics: None,
        }
    }

    // The two post-walk counters the pre-dispatch filters feed, owned
    // together with the `Config` that carries them. Named fields rather
    // than a pair of `Arc<AtomicUsize>` arguments: transposing them at a
    // call site would invert the very distinction these tests exist to
    // pin.
    struct Counters {
        dispatched: Arc<AtomicUsize>,
        failures: Arc<AtomicUsize>,
    }

    impl Counters {
        fn new() -> Self {
            Self {
                dispatched: Arc::new(AtomicUsize::new(0)),
                failures: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn config(&self) -> Config {
            Config {
                files_dispatched: Some(Arc::clone(&self.dispatched)),
                read_failures: Arc::clone(&self.failures),
                ..preproc_test_config(None)
            }
        }

        fn dispatched(&self) -> usize {
            self.dispatched.load(Ordering::Relaxed)
        }

        fn failures(&self) -> usize {
            self.failures.load(Ordering::Relaxed)
        }
    }

    // Regression test for issue #1060: a file the runner cannot read
    // was never analysed, so it must not bump `files_dispatched` — that
    // counter is what turns "no input files matched" into a tool error
    // in `run_check`, and counting an unreadable file made `bca check`
    // exit 0 on a run that analysed nothing. A missing path stands in
    // for the reported permission-denied case: both surface as an
    // `Err` from `read_file_with_eol`, and this form also runs where
    // POSIX modes do not (Windows, and as root).
    #[test]
    fn unreadable_file_counts_as_read_failure_not_dispatched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let counters = Counters::new();
        let cfg = counters.config();

        let err = validate_and_resolve_file(dir.path().join("missing.py"), &cfg)
            .expect_err("a nonexistent path must surface the read error");

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(counters.dispatched(), 0);
        assert_eq!(counters.failures(), 1);
    }

    // The counterpart to the test above: a file that reads cleanly is
    // dispatched and leaves the failure tally untouched.
    #[test]
    fn readable_file_counts_as_dispatched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ok.py");
        std::fs::write(&path, "def f():\n    return 1\n").expect("write fixture");
        let counters = Counters::new();
        let cfg = counters.config();

        let (_, source, language) = validate_and_resolve_file(path, &cfg)
            .expect("readable file")
            .expect("a Python file must resolve a language");

        assert_eq!(language, LANG::Python);
        assert_eq!(source, b"def f():\n    return 1\n");
        assert_eq!(counters.dispatched(), 1);
        assert_eq!(counters.failures(), 0);
    }

    // The #1060 reorder moved the `files_dispatched` bump below the
    // read, which must not narrow it to files that produce metrics: an
    // empty file is skipped for analysis but still counts as "the user
    // pointed at something", the semantics `run_check`'s zero-files
    // guard is written against.
    #[test]
    fn empty_file_still_counts_as_dispatched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.py");
        std::fs::write(&path, "").expect("write fixture");
        let counters = Counters::new();
        let cfg = counters.config();

        let resolved = validate_and_resolve_file(path, &cfg).expect("readable file");

        assert!(resolved.is_none(), "an empty file is skipped for analysis");
        assert_eq!(counters.dispatched(), 1);
        assert_eq!(counters.failures(), 0);
    }

    // Regression test for issue #425: a poisoned `preproc_lock` must
    // degrade like the sibling worker dispatchers (warn + `Ok(())`)
    // rather than panic and cascade across the pool. Verified by
    // revert: replacing the `let-else` with `.expect("...")` makes
    // this test panic instead of returning `Ok`, so it pins the
    // changed line rather than an unrelated path.
    #[test]
    fn dispatch_preproc_degrades_on_poisoned_lock() {
        let lock = Arc::new(Mutex::new(PreprocResults::default()));

        // Poison the mutex: a thread that panics while holding the
        // guard leaves the lock in the poisoned state, exactly the
        // hazard a faulting preproc worker creates for its peers.
        let poisoner = lock.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().expect("fresh mutex is unpoisoned");
            panic!("intentional panic to poison the preproc lock");
        })
        .join();
        assert!(lock.is_poisoned(), "test setup failed to poison the lock");

        let cfg = preproc_test_config(Some(lock));
        // C++ source so `guess_language` resolves to `LANG::Cpp` and the
        // dispatcher reaches the lock acquisition under test.
        let source = b"#define FOO 1\nint main() { return FOO; }\n".to_vec();
        let result = dispatch_preproc(source, PathBuf::from("poisoned.cpp"), &cfg);

        assert!(
            result.is_ok(),
            "poisoned preproc lock should degrade to Ok(()), not panic"
        );
    }
}
