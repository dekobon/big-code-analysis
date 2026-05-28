use std::path::PathBuf;

use clap::Parser;
use clap::builder::{PossibleValuesParser, TypedValueParser};

use enums::*;

#[derive(Debug, Clone)]
enum OutputLanguage {
    Rust,
    Go,
    Json,
    CMacros,
}

impl std::str::FromStr for OutputLanguage {
    type Err = &'static str;

    fn from_str(env: &str) -> std::result::Result<Self, Self::Err> {
        match env {
            "rust" => Ok(Self::Rust),
            "go" => Ok(Self::Go),
            "json" => Ok(Self::Json),
            "c_macros" => Ok(Self::CMacros),
            _ => Err("Not a valid value, run `--help` to know valid values"),
        }
    }
}

impl OutputLanguage {
    const fn variants() -> [&'static str; 4] {
        ["rust", "go", "json", "c_macros"]
    }
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
    #[clap(long, short, default_value = "rust", value_parser = PossibleValuesParser::new(OutputLanguage::variants())
        .map(|s| s.parse::<OutputLanguage>().unwrap()))]
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
