//! `verbatim-oracle <language-slug> <file>...` — one JSON record per
//! file on stdout, from the library's verbatim path. The body lives in
//! `big_code_analysis_bench::verbatim` so it is covered by tests; this
//! file only maps its result onto the exit status.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match big_code_analysis_bench::verbatim::main_with(&argv, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // stderr is best-effort: there is nowhere else to report a
            // failure to write the failure.
            let _ = writeln!(std::io::stderr(), "verbatim-oracle: {message}");
            ExitCode::FAILURE
        }
    }
}
