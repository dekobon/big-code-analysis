//! Argument handling for the `scaling` bench target.
//!
//! Lives in the library, not in `benches/scaling.rs`, because a
//! `harness = false` bench target runs `main` instead of libtest: a
//! `#[cfg(test)] mod tests` in that file compiles and is never
//! executed. The parsing below decides whether the process gates or
//! not, which is worth more than a test that silently does nothing.

use crate::scaling::DEFAULT_ROUNDS;

/// Usage text for `benches/scaling.rs`.
pub const USAGE: &str = "\
usage: cargo bench -p big-code-analysis-bench --bench scaling -- [options]

  --rounds N   measurement rounds per cell (default 7, must be odd)
  --no-gate    measure at full depth, report the exponents, never fail
  --smoke      shallow depths, no verdict (the `cargo test` path)
  --help       this message
";

/// What a run is for.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Mode {
    /// Measure at the probes' declared depths and fail on a probe that
    /// left its complexity class.
    Gate,
    /// Measure at the probes' declared depths, report, never fail.
    ReportOnly,
    /// Shallow depths, no verdict: enough to prove the harness still
    /// runs, not enough to mean anything.
    Smoke,
}

/// A parsed invocation.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct Args {
    /// Measurement rounds per cell.
    pub rounds: usize,
    /// What the run is for.
    pub mode: Mode,
}

/// What the caller should do with a parsed command line.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Action {
    /// Measure.
    Run(Args),
    /// Print [`USAGE`] and stop.
    Help,
}

/// Parses the `scaling` bench target's arguments.
///
/// `cargo bench` passes `--bench` to every `harness = false` bench
/// target; `cargo test --benches` runs the same binary with **no**
/// arguments at all. That absence is the only signal distinguishing
/// the two, so it selects [`Mode::Smoke`] by default: measuring every
/// probe at production depths in an unoptimised build would add tens
/// of seconds to `cargo test` and produce numbers nobody should read.
///
/// An explicit `--no-gate` / `--smoke` wins over `--bench` regardless
/// of order, because cargo appends `--bench` itself and the caller
/// does not control where it lands.
///
/// A bare positional is criterion's benchmark filter, which arrives
/// here too when `cargo bench` runs every target in the package. It
/// selects nothing in this harness and is ignored.
///
/// # Errors
///
/// Returns a message suitable for printing above [`USAGE`] when an
/// option is unknown, or when `--rounds` is missing its value or given
/// something that is not a positive odd integer.
pub fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Action, String> {
    let mut rounds = DEFAULT_ROUNDS;
    let mut cargo_bench = false;
    let mut requested: Option<Mode> = None;
    let mut argv = argv.into_iter();

    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--bench" => cargo_bench = true,
            // `--test` is accepted as an alias because that is what a
            // libtest-harnessed bench target would receive. This one
            // gets nothing, but the alias costs nothing either.
            "--smoke" | "--test" => requested = Some(Mode::Smoke),
            "--no-gate" => requested = Some(Mode::ReportOnly),
            "--rounds" => {
                let value = argv.next().ok_or("--rounds needs a value")?;
                rounds = value
                    .parse()
                    .map_err(|_| format!("--rounds: not a positive integer: {value}"))?;
                if rounds == 0 {
                    return Err("--rounds must be at least 1".to_owned());
                }
                // Same invariant the `DEFAULT_ROUNDS` const assert
                // holds: an even count makes every median the average
                // of its two middle samples, so one contended round
                // moves the fit — which is the sensitivity the median
                // is there to remove.
                if rounds.is_multiple_of(2) {
                    return Err(format!(
                        "--rounds must be odd so each median is an observed \
                         sample, not an average of two: {rounds}"
                    ));
                }
            }
            "--help" | "-h" => return Ok(Action::Help),
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}"));
            }
            _ => {}
        }
    }

    let default = if cargo_bench { Mode::Gate } else { Mode::Smoke };
    Ok(Action::Run(Args {
        rounds,
        mode: requested.unwrap_or(default),
    }))
}

#[cfg(test)]
mod tests {
    use super::{Action, Args, Mode, parse_args};

    fn parse(args: &[&str]) -> Args {
        match parse_args(args.iter().map(|a| (*a).to_owned())) {
            Ok(Action::Run(args)) => args,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    /// No arguments means `cargo test` ran the binary, not `cargo
    /// bench`. Gating there would fail the ordinary test suite on a
    /// measurement taken in an unoptimised build.
    #[test]
    fn bare_invocation_smokes() {
        assert_eq!(parse(&[]).mode, Mode::Smoke);
    }

    /// `--bench` is what `cargo bench` adds, and it is the only signal
    /// that a real measurement was asked for.
    #[test]
    fn cargo_bench_gates() {
        assert_eq!(parse(&["--bench"]).mode, Mode::Gate);
    }

    /// Cargo appends `--bench` itself, so an explicit mode has to win
    /// from either side of it.
    #[test]
    fn explicit_mode_wins_regardless_of_order() {
        assert_eq!(parse(&["--no-gate", "--bench"]).mode, Mode::ReportOnly);
        assert_eq!(parse(&["--bench", "--no-gate"]).mode, Mode::ReportOnly);
        assert_eq!(parse(&["--smoke", "--bench"]).mode, Mode::Smoke);
    }

    /// A positional is criterion's filter, forwarded to every bench
    /// target in the package. It must not be mistaken for an error.
    #[test]
    fn positional_filter_is_ignored() {
        assert_eq!(parse(&["--bench", "cognitive"]).mode, Mode::Gate);
    }

    #[test]
    fn help_short_circuits() {
        for flag in ["--help", "-h"] {
            let parsed = parse_args([flag.to_owned()]);
            assert_eq!(parsed, Ok(Action::Help), "{flag} must request usage");
        }
    }

    #[test]
    fn rounds_is_parsed_and_validated() {
        assert_eq!(parse(&["--bench", "--rounds", "11"]).rounds, 11);
        for bad in [
            vec!["--rounds"],
            vec!["--rounds", "0"],
            // Even counts make every median an average of two samples,
            // which `USAGE` forbids and `DEFAULT_ROUNDS` asserts.
            vec!["--rounds", "10"],
            vec!["--rounds", "many"],
            vec!["--unknown"],
        ] {
            assert!(
                parse_args(bad.iter().map(|a| (*a).to_owned())).is_err(),
                "expected {bad:?} to be rejected",
            );
        }
    }
}
