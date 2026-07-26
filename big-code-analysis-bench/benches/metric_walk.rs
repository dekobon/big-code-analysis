//! Criterion measurements of the metric walk (#1068).
//!
//! Three groups:
//!
//! - `corpus/parse` — `tree_sitter` parse cost over the corpus slice,
//!   the baseline the walk sits on top of.
//! - `corpus/walk` — one benchmark per metric family, walking the
//!   already-parsed slice. This is the number to quote when a change
//!   claims to make a metric cheaper.
//! - `shape/walk` — the depth-scaling shapes at a single depth, so a
//!   constant-factor change on a pathological input is visible even
//!   when its complexity class did not move.
//!
//! ```text
//! cargo bench -p big-code-analysis-bench --bench metric_walk
//! cargo bench -p big-code-analysis-bench --bench metric_walk -- --save-baseline before
//! cargo bench -p big-code-analysis-bench --bench metric_walk -- --baseline before
//! ```
//!
//! Criterion reports a confidence interval, not a point estimate, and
//! `--baseline` compares two runs with one. Both matter: a "~26%"
//! improvement quoted from min-of-5 single runs during the #1052 /
//! #1062 work re-measured to a bootstrap interval spanning roughly
//! 7-41%. See `docs/development/benchmarking.md`.

use std::hint::black_box;

use big_code_analysis::{Ast, Metric, MetricsOptions, Source};
use big_code_analysis_bench::corpus::{CorpusFile, CorpusSlice, repo_root};
use big_code_analysis_bench::shapes::PROBES;
use criterion::{Criterion, Throughput};

/// Depth at which each shape is measured in the criterion group.
///
/// The shallowest depth of the linear probes, which is also the
/// deepest of the quadratic ones: deep enough to be the pathological
/// input the shape exists to represent, shallow enough that the
/// already-quadratic probes still finish in a criterion sample.
const SHAPE_BENCH_DEPTH: usize = 1_000;

fn main() {
    let slice = CorpusSlice::load(&repo_root());
    // Printed before anything is measured: a slice is only a
    // trustworthy input if the reader can see what is in it.
    eprintln!("{}", slice.summary());

    let mut criterion = Criterion::default().configure_from_args();
    bench_corpus(&mut criterion, &slice);
    bench_shapes(&mut criterion);
    criterion.final_summary();
}

/// Parses the slice, dropping anything the walker cannot handle.
///
/// Pre-validating here is what lets the timed closures below treat
/// `metrics()` as infallible: a file that errors never reaches them.
/// Each retained file is returned alongside its `Ast` so the parse and
/// walk benches measure — and report throughput over — the same set.
fn parse_slice(slice: &CorpusSlice) -> Vec<(&CorpusFile, Ast)> {
    slice
        .files
        .iter()
        .filter_map(|file| {
            let ast = Ast::parse(Source::new(file.lang, &file.source)).ok()?;
            ast.metrics(MetricsOptions::default()).ok()?;
            Some((file, ast))
        })
        .collect()
}

/// Every [`Metric`] variant, without hardcoding the list.
///
/// `Metric::suppressible()` is the public enumeration of the full set
/// minus `Tokens` (the one metric with no configurable threshold), so
/// adding it back yields all of them — and a future variant is picked
/// up here without an edit.
fn all_metrics() -> Vec<Metric> {
    Metric::suppressible().chain([Metric::Tokens]).collect()
}

fn bench_corpus(criterion: &mut Criterion, slice: &CorpusSlice) {
    if slice.is_empty() {
        eprintln!("skipping corpus benches: no corpus files selected");
        return;
    }
    let parsed = parse_slice(slice);
    // A slice can be non-empty and still retain nothing — selection is
    // by file extension, so a build without those languages compiled in
    // drops every file in `parse_slice`. Benching that would time empty
    // loops and report them as excellent walk numbers.
    if parsed.is_empty() {
        eprintln!(
            "skipping corpus benches: none of the {} selected files could be \
             parsed and walked; are their languages compiled in?",
            slice.files.len(),
        );
        return;
    }
    let bytes = parsed
        .iter()
        .map(|(file, _)| file.source.len() as u64)
        .sum::<u64>();

    let mut parsing = criterion.benchmark_group("corpus/parse");
    parsing.throughput(Throughput::Bytes(bytes));
    parsing.bench_function("tree-sitter", |b| {
        b.iter(|| {
            for (file, _) in &parsed {
                black_box(Ast::parse(Source::new(file.lang, &file.source)).ok());
            }
        });
    });
    parsing.finish();

    let mut walk = criterion.benchmark_group("corpus/walk");
    walk.throughput(Throughput::Bytes(bytes));
    for metric in all_metrics() {
        let options = MetricsOptions::default().with_only(&[metric]);
        walk.bench_function(metric.to_string(), |b| {
            b.iter(|| {
                for (_, ast) in &parsed {
                    black_box(ast.metrics(options).expect(
                        "parse_slice already walked every retained Ast with the \
                         full metric set, so a narrower selection cannot fail",
                    ));
                }
            });
        });
    }
    walk.bench_function("all", |b| {
        b.iter(|| {
            for (_, ast) in &parsed {
                black_box(
                    ast.metrics(MetricsOptions::default())
                        .expect("pre-validated in parse_slice"),
                );
            }
        });
    });
    walk.finish();
}

fn bench_shapes(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("shape/walk");
    for probe in PROBES {
        let source = (probe.render)(SHAPE_BENCH_DEPTH);
        let Ok(ast) = Ast::parse(Source::new(probe.lang, source.as_bytes())) else {
            eprintln!(
                "skipping {}: {:?} is not compiled in",
                probe.name, probe.lang
            );
            continue;
        };
        let options = MetricsOptions::default().with_only(probe.metrics);
        if ast.metrics(options).is_err() {
            eprintln!("skipping {}: the walker rejected its shape", probe.name);
            continue;
        }
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_function(probe.name, |b| {
            b.iter(|| {
                black_box(
                    ast.metrics(options)
                        .expect("the same call succeeded during setup"),
                );
            });
        });
    }
    group.finish();
}
