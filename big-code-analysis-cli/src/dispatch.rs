//! Per-file dispatch for the `bca` walker.
//!
//! `act_on_file` is the entry point: it runs the shared pre-dispatch
//! filters (file-count bump, empty-file skip, generated-code skip,
//! language resolution) via `validate_and_resolve_file`, then forwards
//! to the per-action `dispatch_*` helper that implements one `Action`
//! variant. The helpers are intentionally one-screen each so a reader
//! can follow exactly the path a given subcommand takes without
//! scrolling past nine unrelated arms.
//!
//! Every dispatch helper reaches the deprecated
//! `get_function_spaces_with_options` shim because the CLI is the
//! canonical path-based caller (it always holds the `&Path` for the
//! file it just read) and migration to `analyze(Source { ... }, ...)`
//! tracks issue #254's follow-up. The function-scope
//! `#[allow(deprecated)]` keeps the helpers readable without
//! per-call-site attributes.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[allow(deprecated)]
use big_code_analysis::get_function_spaces_with_options;
use big_code_analysis::{
    CommentRm, CommentRmCfg, Count, CountCfg, Dump, DumpCfg, Find, FindCfg, Function, FunctionCfg,
    Metrics, MetricsCfg, OpsCfg, OpsCode, PreprocParser, PreprocResults, SuppressionScan, action,
    get_ops, guess_language, is_generated, preprocess, read_file_with_eol,
};
use big_code_analysis::{LANG, ParserTrait};

use crate::exemptions::FileMarkers;
use crate::formats::{MetricsDispatch, MetricsFormat, dump_csv};
use crate::markdown_report::extract_summaries;
use crate::{Action, Config, FEATURES_PINNED};

pub(crate) fn act_on_file(path: PathBuf, cfg: &Config) -> std::io::Result<()> {
    let Some((path, source, language)) = validate_and_resolve_file(path, cfg)? else {
        return Ok(());
    };
    let pr = cfg.preproc.clone();
    match &cfg.action {
        Action::Dump => dispatch_dump(language, source, path, pr, cfg),
        Action::Metrics { format, pretty } => {
            dispatch_metrics(language, source, path, pr, cfg, format.as_ref(), *pretty)
        }
        Action::Ops { format, pretty } => {
            dispatch_ops(language, source, path, pr, cfg, format.as_ref(), *pretty)
        }
        Action::StripComments { in_place } => {
            dispatch_strip_comments(language, source, path, pr, *in_place)
        }
        Action::Functions => dispatch_functions(language, source, path, pr),
        Action::Find(filters) => dispatch_find(language, source, path, pr, cfg, filters),
        Action::Count(filters) => dispatch_count(language, source, path, pr, cfg, filters),
        Action::Report => dispatch_report(language, source, path, pr, cfg),
        Action::Check => dispatch_check_file(language, source, path, pr, cfg),
        Action::Exemptions => dispatch_exemptions(language, source, path, pr, cfg),
        Action::PreprocProduce => dispatch_preproc(source, path, cfg),
    }
}

/// Apply the three pre-dispatch filters every CLI subcommand shares:
/// bump the `files_dispatched` counter, skip empty files, skip
/// generated files (unless we're producing preproc data — that
/// pipeline genuinely needs every C/C++ file walked), and resolve
/// the source language. Returns `Ok(None)` when the file should be
/// skipped (logging the per-`cfg.warning` reason inline). Returns
/// `Ok(Some((path, source, lang)))` to hand off to dispatch.
fn validate_and_resolve_file(
    path: PathBuf,
    cfg: &Config,
) -> std::io::Result<Option<(PathBuf, Vec<u8>, LANG)>> {
    if let Some(counter) = &cfg.files_dispatched {
        // Count every dispatched file, including those skipped below for
        // empty content / unrecognized language. The user pointed at
        // these files and the runner walked them — they count as "the
        // input was non-empty" for the zero-files-matched check in
        // `run_check`.
        counter.fetch_add(1, Ordering::Relaxed);
    }

    let Some(source) = read_file_with_eol(&path)? else {
        if cfg.warning {
            eprintln!("warning: skipping empty file: {}", path.display());
        }
        return Ok(None);
    };

    if cfg.skip_generated && !matches!(cfg.action, Action::PreprocProduce) && is_generated(&source)
    {
        if cfg.report_skipped || cfg.warning {
            eprintln!("skipped (generated): {}", path.display());
        }
        return Ok(None);
    }

    let Some(language) = cfg.language.or_else(|| guess_language(&source, &path).0) else {
        if cfg.warning {
            eprintln!(
                "warning: skipping file with unrecognized language: {}",
                path.display()
            );
        }
        return Ok(None);
    };

    Ok(Some((path, source, language)))
}

#[allow(deprecated)]
fn dispatch_dump(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
) -> std::io::Result<()> {
    let dump_cfg = DumpCfg {
        line_start: cfg.line_start,
        line_end: cfg.line_end,
    };
    // The CLI pins the library's `all-languages` feature, so
    // `LanguageDisabled` from `action::<T>` is unreachable; the
    // `expect` documents that invariant.
    action::<Dump>(&language, source, &path, pr, dump_cfg).expect(FEATURES_PINNED)
}

#[allow(deprecated)]
fn dispatch_metrics(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
    format: Option<&MetricsFormat>,
    pretty: bool,
) -> std::io::Result<()> {
    if let Some(fmt) = format {
        if let Ok(space) =
            get_function_spaces_with_options(&language, source, &path, pr, cfg.metrics_options())
        {
            match fmt.dispatch() {
                MetricsDispatch::Generic(g) => {
                    g.dump(space, path, cfg.output.as_ref(), pretty)?;
                }
                MetricsDispatch::Csv => {
                    dump_csv(&space, path, cfg.output.as_ref())?;
                }
            }
        }
        Ok(())
    } else {
        let metrics_cfg = MetricsCfg::new(path).with_options(cfg.metrics_options());
        let path = metrics_cfg.path.clone();
        action::<Metrics>(&language, source, &path, pr, metrics_cfg).expect(FEATURES_PINNED)
    }
}

#[allow(deprecated)]
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
        if let Ok(ops) = get_ops(&language, source, &path, pr) {
            // CSV is rejected upstream in `run()` for the Ops command,
            // so the dispatch here is always Generic. The match is
            // still exhaustive to keep the compiler honest if that
            // upstream guard ever drifts.
            match fmt.dispatch() {
                MetricsDispatch::Generic(g) => {
                    g.dump(ops, path, cfg.output.as_ref(), pretty)?;
                }
                MetricsDispatch::Csv => {}
            }
        }
        Ok(())
    } else {
        let ops_cfg = OpsCfg { path };
        let path = ops_cfg.path.clone();
        action::<OpsCode>(&language, source, &path, pr, ops_cfg).expect(FEATURES_PINNED)
    }
}

#[allow(deprecated)]
fn dispatch_strip_comments(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    in_place: bool,
) -> std::io::Result<()> {
    let comment_cfg = CommentRmCfg { in_place, path };
    let path = comment_cfg.path.clone();
    // C++ comment removal goes through the dedicated Ccomment grammar
    // even when the file's primary language is Cpp.
    let lang = if language == LANG::Cpp {
        LANG::Ccomment
    } else {
        language
    };
    action::<CommentRm>(&lang, source, &path, pr, comment_cfg).expect(FEATURES_PINNED)
}

#[allow(deprecated)]
fn dispatch_functions(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
) -> std::io::Result<()> {
    let fn_cfg = FunctionCfg { path: path.clone() };
    action::<Function>(&language, source, &path, pr, fn_cfg).expect(FEATURES_PINNED)
}

#[allow(deprecated)]
fn dispatch_find(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
    filters: &Arc<[String]>,
) -> std::io::Result<()> {
    let find_cfg = FindCfg {
        path: path.clone(),
        filters: Arc::clone(filters),
        line_start: cfg.line_start,
        line_end: cfg.line_end,
    };
    action::<Find>(&language, source, &path, pr, find_cfg).expect(FEATURES_PINNED)
}

#[allow(deprecated)]
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
    let count_cfg = CountCfg {
        filters: Arc::clone(filters),
        stats,
    };
    action::<Count>(&language, source, &path, pr, count_cfg).expect(FEATURES_PINNED)
}

// Returns Result<()> for dispatch-table uniformity with sibling
// helpers that do propagate I/O errors via `?` (e.g. `dispatch_metrics`).
// The body never produces an `Err` itself.
#[allow(deprecated, clippy::unnecessary_wraps)]
fn dispatch_report(
    language: LANG,
    source: Vec<u8>,
    path: PathBuf,
    pr: Option<Arc<PreprocResults>>,
    cfg: &Config,
) -> std::io::Result<()> {
    if let Ok(space) =
        get_function_spaces_with_options(&language, source, &path, pr, cfg.metrics_options())
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
                eprintln!(
                    "warning: skipping non-UTF-8 path in report: {}",
                    path.display()
                );
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
        let Ok(sender) = tx.lock() else {
            if cfg.warning {
                eprintln!(
                    "warning: skipping {}: report channel lock poisoned",
                    path.display()
                );
            }
            return Ok(());
        };
        for s in summaries {
            let _ = sender.send(s);
        }
    }
    Ok(())
}

// Returns Result<()> for dispatch-table uniformity; never produces
// an `Err` itself.
#[allow(deprecated, clippy::unnecessary_wraps)]
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
    if let Ok(space) =
        get_function_spaces_with_options(&language, source, &path, pr, cfg.metrics_options())
        && let (Some(set), Some(tx)) = (cfg.threshold_set.as_ref(), cfg.check_tx.as_ref())
        && !matches!(language, LANG::Preproc | LANG::Ccomment)
    {
        // Pass the path through as `&Path` so non-UTF-8 bytes are
        // preserved on each emitted `Violation`. Display / offender
        // serialization decide their own lossy strategy at the output
        // boundary; the threshold pipeline itself stays byte-faithful.
        let mut violations = Vec::new();
        set.evaluate_with_policy(&path, &space, cfg.suppression_policy, &mut violations);
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
        if !violations.is_empty() {
            let Ok(sender) = tx.lock() else {
                if cfg.warning {
                    eprintln!(
                        "warning: skipping {}: check channel lock poisoned",
                        path.display()
                    );
                }
                return Ok(());
            };
            // Receiver lives until `run_check` drains `rx`, which
            // happens only after `run_walk` joins all worker threads —
            // so `send` cannot fail here. Use `let _` rather than
            // `expect` to avoid panicking the worker pool on the
            // (unreachable) drop path.
            for v in violations {
                let _ = sender.send(v);
            }
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
            eprintln!(
                "warning: skipping non-UTF-8 path in exemptions audit: {}",
                path.display()
            );
        }
        return Ok(());
    };
    let markers =
        action::<SuppressionScan>(&language, source, &path, pr, ()).expect(FEATURES_PINNED);
    // Empty files are the dominant case (most source carries no
    // markers); skip the channel send and the per-file allocation when
    // there is nothing to report.
    if markers.is_empty() {
        return Ok(());
    }
    let Ok(sender) = tx.lock() else {
        if cfg.warning {
            eprintln!(
                "warning: skipping {}: exemptions channel lock poisoned",
                path.display()
            );
        }
        return Ok(());
    };
    // Receiver lives until the post-walk aggregator drains `rx`, which
    // happens only after all worker threads join — so `send` cannot
    // fail. Use `let _` rather than `expect` to avoid panicking the
    // worker pool on the unreachable drop path.
    let _ = sender.send(FileMarkers {
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
        && language == LANG::Cpp
    {
        let mut results = preproc_lock.lock().expect("mutex not poisoned");
        preprocess(
            &PreprocParser::new(source, &path, None),
            &path,
            &mut results,
        );
    }
    Ok(())
}
