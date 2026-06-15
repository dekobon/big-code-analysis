use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use enums::*;

// `ValueEnum` is the single source of truth for the `--language` values:
// clap both restricts input to these variants and constructs the enum
// directly, so there is no separate string table to keep in sync and no
// fallible parse step that could panic on a drifted value (issue #866).
#[derive(Debug, Clone, ValueEnum)]
enum OutputLanguage {
    Rust,
    Go,
    Json,
    #[value(name = "c_macros")]
    CMacros,
}

#[derive(Parser, Debug)]
#[clap(
    name = "enums",
    version,
    author,
    about = "Generate enums for a target language to use with tree-sitter."
)]
struct Opts {
    /// Output directory.
    #[clap(long, short, default_value = ".", value_parser)]
    output: PathBuf,
    /// Target language.
    #[clap(long, short, value_enum, default_value_t = OutputLanguage::Rust)]
    language: OutputLanguage,
    /// File name template.
    #[clap(long, short, default_value = "language_$")]
    file_template: String,
}

fn main() -> std::process::ExitCode {
    let opts = Opts::parse();

    let result = match opts.language {
        OutputLanguage::Rust => generate_rust(&opts.output, &opts.file_template),
        OutputLanguage::Go => generate_go(&opts.output, &opts.file_template),
        OutputLanguage::Json => generate_json(&opts.output, &opts.file_template),
        OutputLanguage::CMacros => generate_macros(&opts.output),
    };
    if let Err(err) = result {
        // Print the io::Error and exit non-zero so callers
        // (drift gate, recreate-grammars.sh) can detect failure.
        // The prior `if let Some(err) = ...err() { eprintln!(...) }`
        // pattern swallowed the error and exited 0, silently
        // shipping a partial / empty output tree.
        eprintln!("enums: {err:?}");
        return std::process::ExitCode::from(2);
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    // Issue #866: an out-of-sync `--language` value must surface as a clap
    // usage error, never a panic. `ValueEnum` makes the possible-values set
    // and the constructed enum a single source of truth, so this cannot
    // regress into the old `.unwrap()` panic on a drifted variant.
    #[test]
    fn bogus_language_is_a_usage_error_not_a_panic() {
        let err = Opts::try_parse_from(["enums", "--language", "nope"])
            .expect_err("an unknown --language value must be rejected");
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    // Every renamed / non-default variant must remain reachable from the
    // CLI; `c_macros` carries the `#[value(name = ...)]` rename.
    #[test]
    fn c_macros_value_selects_the_macro_generator() {
        let opts = Opts::try_parse_from(["enums", "--language", "c_macros"])
            .expect("c_macros is a valid --language value");
        assert!(matches!(opts.language, OutputLanguage::CMacros));
    }

    // clap's own assertion that the derived command is internally
    // consistent (every variant has a value name, no duplicates), the
    // compile-time replacement for the hand-maintained variants() table.
    #[test]
    fn command_definition_is_well_formed() {
        Opts::command().debug_assert();
    }
}
