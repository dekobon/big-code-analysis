//! Complexity-class gate for the metric walk (#1068).
//!
//! Measures every probe in `shapes::PROBES` at three doubling depths,
//! fits `time ~ depth^k`, and fails when a probe's `k` exceeds the
//! bound it declared. This is where the wall-clock assertions that used
//! to live in `cognitive_deep_nesting_is_tractable` and
//! `tokens_deep_nesting_is_tractable` went: the unit suite keeps the
//! value assertions, which are host-independent, and the timing half
//! moved here, where it runs under the bench profile on a host the
//! operator chose.
//!
//! ```text
//! cargo bench -p big-code-analysis-bench --bench scaling
//! cargo bench -p big-code-analysis-bench --bench scaling -- --rounds 11
//! ```
//!
//! Argument handling lives in `big_code_analysis_bench::cli`, where it
//! is covered by tests that actually run: a `harness = false` bench
//! target executes `main` instead of libtest, so a `#[cfg(test)]`
//! module in this file would compile and never be exercised.
//!
//! See `docs/development/benchmarking.md`.

use std::process::ExitCode;

use big_code_analysis_bench::cli::{Action, Mode, USAGE, parse_args};
use big_code_analysis_bench::scaling;
use big_code_analysis_bench::shapes::{PROBES, Probe};

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Action::Run(args)) => args,
        Ok(Action::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    // An unoptimised build measures the walk through overflow checks
    // and unelided bounds checks, which perturbs the constant factor
    // enough that the exponent is not worth a verdict.
    let optimized = !cfg!(debug_assertions);
    if !optimized {
        eprintln!(
            "note: built with debug assertions, so nothing here is gated. \
             Use `cargo bench` for a measurement worth quoting."
        );
    }

    let smoke_set;
    let probes: &[Probe] = if args.mode == Mode::Smoke {
        eprintln!(
            "note: smoke run at shallow depths. Use `cargo bench -p \
             big-code-analysis-bench --bench scaling` for the real gate."
        );
        smoke_set = scaling::smoke_probes(PROBES);
        &smoke_set
    } else {
        PROBES
    };

    let report = match scaling::run(probes, args.rounds) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("scaling harness could not run: {error}");
            return ExitCode::FAILURE;
        }
    };
    print!("{report}");

    if args.mode != Mode::Gate || !optimized {
        return ExitCode::SUCCESS;
    }

    let failures = report.failures();
    if failures.is_empty() {
        println!(
            "\nall {} probes within their complexity bound",
            report.probes.len()
        );
        return ExitCode::SUCCESS;
    }
    eprintln!("\n{} probe(s) failed:", failures.len());
    for probe in failures {
        // An abandoned probe is reported by the budget it blew, not by
        // its exponent: it was fitted over the cells that finished, so
        // that number is flattering and quoting it reads as a harness
        // bug rather than as the worst regression the gate can see.
        if let Some((depth, elapsed)) = probe.over_budget {
            eprintln!(
                "  {name}: one walk at depth {depth} took {elapsed:?}, over the \
                 {budget:?} per-walk budget\n    {rationale}",
                name = probe.name,
                budget = scaling::MAX_CELL_WALK,
                rationale = probe.rationale,
            );
        } else {
            eprintln!(
                "  {name}: fitted exponent {exp:.2} > {bound:.2}\n    {rationale}",
                name = probe.name,
                exp = probe.exponent,
                bound = probe.max_exponent,
                rationale = probe.rationale,
            );
        }
    }
    eprintln!(
        "\nRe-run on an idle host before treating this as a regression; a wide \
         min-max spread in the table above means the measurement, not the code, \
         is what changed."
    );
    ExitCode::FAILURE
}
