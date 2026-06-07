// bca: suppress-file(halstead, nargs, exit)
// Per-subcommand dispatch fns; the offenders are many-fn / early-return
// aggregation artifacts, not per-function logic complexity
// (cognitive/cyclomatic stay enforced).

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
//! The metrics / ops helpers analyze each file through the
//! explicit-name `Source` / `Ast` seams (`analyze`, `Ast::ops`). The
//! display name is the file's UTF-8 path — `None` for a non-UTF-8 path,
//! rather than the lossy-mangled name the retired path-positional shims
//! emitted (#568) — while the `&Path` is still forwarded as the C++
//! preprocessor lookup key.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use std::io::Write;

use big_code_analysis::LANG;
use big_code_analysis::{
    Ast, FuncSpace, MetricsError, MetricsOptions, PreprocResults, Source, analyze,
    dump_function_spans, dump_node, dump_ops, dump_root, guess_language, is_generated, preprocess,
    read_file_with_eol, write_file,
};

use crate::exemptions::FileMarkers;
use crate::formats::{MetricsDispatch, MetricsFormat, dump_csv};
use crate::markdown_report::extract_summaries;
use crate::{Action, Config, FEATURES_PINNED};

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
    analyze(
        Source::new(language, &source)
            .with_name(path.to_str().map(str::to_owned))
            .with_preproc_path(Some(path))
            .with_preproc(pr),
        options,
    )
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
        Source::new(language, &source)
            .with_name(path.to_str().map(str::to_owned))
            .with_preproc_path(Some(path))
            .with_preproc(pr),
    )
}

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
        Action::StripComments { in_place, output } => {
            dispatch_strip_comments(language, source, path, pr, *in_place, output.as_deref())
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
    dump_node(ast.source(), &ast.root_node(), -1, cfg.line_start, cfg.line_end)
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
    if let Some(fmt) = format {
        if let Ok(space) = analyze_file(language, source, &path, pr, cfg.metrics_options()) {
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
        // Human-readable metric dump: parse once, then render the tree.
        // A walker error degrades to no output (matching the prior
        // `Metrics` callback), never an `Err`.
        match parse_ast(language, source, &path, pr)
            .expect(FEATURES_PINNED)
            .metrics(cfg.metrics_options())
        {
            Ok(space) => dump_root(&space),
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
        if let Ok(ops) = Ast::parse(
            Source::new(language, &source)
                .with_name(path.to_str().map(str::to_owned))
                .with_preproc_path(Some(&path))
                .with_preproc(pr),
        )
        .and_then(|ast| ast.ops())
        {
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
        // Human-readable ops dump: a walker error degrades to no output
        // (matching the prior `OpsCode` callback), never an `Err`.
        match parse_ast(language, source, &path, pr)
            .expect(FEATURES_PINNED)
            .ops()
        {
            Ok(ops) => dump_ops(&ops),
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
    // C++ comment removal goes through the dedicated Ccomment grammar
    // even when the file's primary language is Cpp.
    let lang = if language == LANG::Cpp {
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
            println!("{text}");
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
) -> std::io::Result<()> {
    let ast = parse_ast(language, source, &path, pr).expect(FEATURES_PINNED);
    dump_function_spans(ast.functions(), &path)
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
    let found = ast.find(&filters[..]).expect("find is infallible today");
    if !found.is_empty() {
        println!("In file {}", path.display());
        for node in &found {
            dump_node(ast.source(), node, 1, cfg.line_start, cfg.line_end)?;
        }
        println!();
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
    let markers = parse_ast(language, source, &path, pr)
        .expect(FEATURES_PINNED)
        .suppressions();
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
        let Ok(mut results) = preproc_lock.lock() else {
            if cfg.warning {
                eprintln!(
                    "warning: skipping {}: preproc results lock poisoned",
                    path.display()
                );
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

    // Minimal `Config` for exercising `dispatch_preproc` in isolation.
    // Only `preproc_lock` and `warning` are load-bearing here; every
    // other field is defaulted to the inert value used elsewhere.
    fn preproc_test_config(preproc_lock: Option<Arc<Mutex<PreprocResults>>>) -> Config {
        Config {
            action: Action::PreprocProduce,
            output: None,
            language: None,
            line_start: None,
            line_end: None,
            preproc_lock,
            preproc: None,
            count_lock: None,
            markdown_tx: None,
            strip_prefix: String::new(),
            threshold_set: None,
            check_tx: None,
            exemptions_tx: None,
            files_dispatched: None,
            suppression_policy: SuppressionPolicy::Honor,
            report_suppressed: false,
            warning: false,
            skip_generated: true,
            report_skipped: false,
            exclude_tests: false,
            no_cyclomatic_try: false,
            fuzzy_baseline: false,
        }
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
