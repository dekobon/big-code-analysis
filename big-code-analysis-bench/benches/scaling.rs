//! Combined legacy and source class gate. Arguments and reductions are library-tested.

use std::process::ExitCode;

use big_code_analysis_bench::cli::{Action, Mode, Scope, USAGE, parse_args};
use big_code_analysis_bench::documentation::{REGISTRY, combined::CombinedReport, runner};
use big_code_analysis_bench::{
    scaling,
    shapes::PROBES,
    vcs::{self, VCS_PROBES},
};

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Action::Run(args)) => args,
        Ok(Action::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let optimized = !cfg!(debug_assertions);
    let mode = if optimized { args.mode } else { Mode::Smoke };
    eprintln!(
        "scope {:?}, mode {:?}; legacy {} workloads, source {} workloads",
        args.scope,
        mode,
        if args.scope == Scope::Source {
            0
        } else {
            PROBES.len() + VCS_PROBES.len()
        },
        if args.scope == Scope::Legacy {
            0
        } else {
            REGISTRY.len()
        }
    );
    let result = (|| {
        let legacy = if args.scope == Scope::Source {
            None
        } else {
            let probes = if mode == Mode::Smoke {
                scaling::smoke_probes(PROBES)
            } else {
                PROBES.to_vec()
            };
            let ranking = if mode == Mode::Smoke {
                vcs::smoke_probes(VCS_PROBES)
            } else {
                VCS_PROBES.to_vec()
            };
            let mut report = scaling::run(&probes, args.rounds).map_err(|e| e.to_string())?;
            report.probes.extend(vcs::run(&ranking, args.rounds).probes);
            print!("{report}");
            Some(report)
        };
        let source = if args.scope == Scope::Legacy {
            None
        } else {
            let report = runner::run(REGISTRY, args.rounds, mode).map_err(|e| e.to_string())?;
            print!("{report}");
            Some(report)
        };
        let report = CombinedReport { legacy, source };
        let legacy_ids: Vec<_> = PROBES
            .iter()
            .map(|p| p.name)
            .chain(VCS_PROBES.iter().map(|p| p.name))
            .collect();
        let source_ids: Vec<_> = REGISTRY.iter().map(|p| p.id).collect();
        report
            .validate(args.scope, &legacy_ids, &source_ids)
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(report)
    })();
    match result {
        Err(error) => {
            eprintln!("scaling harness could not run: {error}");
            ExitCode::from(2)
        }
        Ok(report) => {
            print!("{}", report.final_summary(mode));
            ExitCode::from(report.exit_code(mode, optimized))
        }
    }
}
