// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::doc_markdown, clippy::enum_glob_use, clippy::wildcard_imports)]
// Per-language Cargo features (commit 7e96b466) let a downstream build
// only a subset of grammars. In such a build the code for the disabled
// languages — their macro-generated `*Code` / `*Parser` tags plus the
// getter / checker / metric helpers and shared plumbing only those
// languages reach — is compiled but never constructed, so `-D dead-code`
// fires on ~180 items that are all live in the default `all-languages`
// build. Relax dead-code to a warning only when the full language set is
// NOT enabled; the default build and `--all-features` (and thus the
// primary CI gate and `make pre-commit`) still hard-deny it, so genuine
// dead code is caught there. Fixes the long-red `features
// (no-default-features / minimal-langs (lib))` CI legs.
#![cfg_attr(not(feature = "all-languages"), allow(dead_code))]
// docs.rs (and the local `doc-check-docsrs` gate) build with `--cfg docsrs`
// on nightly, enabling per-item "Available on crate feature …" badges for
// non-default surfaces such as `vcs`. Guarded so stable builds are unaffected.
#![cfg_attr(docsrs, feature(doc_cfg))]

//! big-code-analysis is a library to analyze and extract information
//! from source codes written in many different programming languages.
//!
//! You can find the source code of this software on
//! <a href="https://github.com/dekobon/big-code-analysis/" target="_blank">GitHub</a>,
//! while issues and feature requests can be posted on the respective
//! <a href="https://github.com/dekobon/big-code-analysis/issues/" target="_blank">GitHub Issue Tracker</a>.
//!
//! ## Quick start
//!
//! Most callers want the recommended entry points exposed in
//! [`prelude`]:
//!
//! ```no_run
//! use big_code_analysis::prelude::*;
//!
//! let source = b"fn main() {}";
//! let space = analyze(
//!     Source::new(LANG::Rust, source),
//!     MetricsOptions::default(),
//! ).expect("Rust source parses");
//! println!("cognitive sum: {}", space.metrics.cognitive.cognitive_sum());
//! ```
//!
//! ## Supported Languages
//!
//! Each grammar is gated behind a per-language Cargo feature; the
//! default `all-languages` feature enables every grammar so the
//! historical "every language compiled in" behaviour is preserved.
//! Library consumers that only need a subset can opt out of the
//! defaults — see [Per-language Cargo features][feat] in the book.
//!
//! - Bash (`bash`)
//! - C (`c`, upstream `tree-sitter-c`; owns `.c`)
//! - C/C++ (`cpp`, upstream `tree-sitter-cpp`; the default for `.cpp` /
//!   `.cc` / `.h` and also exposes the internal `ccomment` / `preproc`
//!   C-family helpers)
//! - C++, Firefox-internal "Mozcpp" (`mozcpp`, opt-in; owns no file
//!   extensions — select it by name)
//! - C# (`csharp`)
//! - Objective-C (`objc`, upstream `tree-sitter-objc`; owns `.m`; `.mm`
//!   Objective-C++ stays on C/C++)
//! - Elixir (`elixir`)
//! - Go (`go`)
//! - Groovy (`groovy`)
//! - F5 iRules (`irules`)
//! - Java (`java`)
//! - JavaScript (`javascript`)
//! - JavaScript, Firefox-internal "MozJS" (`mozjs`)
//! - Kotlin (`kotlin`)
//! - Lua (`lua`)
//! - Perl (`perl`)
//! - PHP (`php`)
//! - Python (`python`)
//! - Ruby (`ruby`)
//! - Rust (`rust`)
//! - Tcl (`tcl`)
//! - TSX (`tsx`)
//! - TypeScript (`typescript`)
//!
//! [feat]: https://dekobon.github.io/big-code-analysis/library/cargo-features.html
//!
//! ## Supported Metrics
//!
//! - ABC: it measures the size of a source code based on
//!   assignments, branches, and conditions.
//! - CC: it calculates the code complexity examining the control flow of a
//!   program.  Both standard and modified flavours are exposed: the
//!   modified variant collapses all case/match arms inside a single
//!   switch/match/when/select into one decision point.
//! - Cognitive Complexity: it measures how difficult it is
//!   to understand a unit of code.
//! - SLOC: it counts the number of lines in a source file.
//! - PLOC: it counts the number of physical lines (instructions)
//!   contained in a source file.
//! - LLOC: it counts the number of logical lines (statements)
//!   contained in a source file.
//! - CLOC: it counts the number of comments in a source file.
//! - BLANK: it counts the number of blank lines in a source file.
//! - HALSTEAD: it is a suite that provides a series of information,
//!   such as the effort required to maintain the analyzed code,
//!   the size in bits to store the program, the difficulty to understand
//!   the code, an estimate of the number of bugs present in the codebase,
//!   and an estimate of the time needed to implement the software.
//! - MI: it is a suite that allows to evaluate the maintainability
//!   of a software.
//! - NOM: it counts the number of functions and closures
//!   in a file/trait/class.
//! - NEXITS: it counts the number of possible exit points
//!   from a method/function.
//! - NARGS: it counts the number of arguments of a function/method.
//! - NPA: it counts the number of public attributes of a class.
//! - NPM: it counts the number of public methods of a class.
//! - WMC: it is the sum of the complexities of all methods
//!   in a class.

#![allow(clippy::upper_case_acronyms)]
// Production-only `unwrap()` ban. See `[workspace.lints.clippy]` in the
// root `Cargo.toml` for why this is a per-root attribute and not a
// Cargo lint (#1227).
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

// The parse and classification layer lives in `big-code-analysis-ast`
// (#1376). Its public names — the generated token enums, the `*Code` /
// `*Parser` tags, `Node`, `Ancestors`, `Checker`, `Getter`, the language
// helpers — are the working vocabulary every metric module reaches
// through `use crate::*`, so the whole crate root is glob-imported here
// at `pub(crate)`. Only the explicit `pub use` lines further down widen
// the published surface.
#[doc(hidden)]
pub(crate) use big_code_analysis_ast::*;

// Fast hashing for the walk's integer-keyed maps. Shared by `spaces`
// (node ids) and `metrics::halstead` (grammar `kind_id`s); `metrics::loc`
// was the third until #1109 moved its line sets to a bitset. The module
// doc is the single place that lists them and says why the text-keyed
// collections are excluded — extend it, not this line, when a third
// arrives.
mod int_hash;
// The metric-side macros (`implement_metric_trait!`) plus re-exports of
// the kind-set and dispatch macros defined in `big-code-analysis-ast`.
mod macros;
// Parse-and-inspect shims shared by the per-metric test modules. Kept out
// of any production file so the self-scan gate does not spend a shipping
// module's metric budget on test-only code (#1066).
#[cfg(test)]
mod test_support;
// Gated on the four grammars its tables name, so a build enabling none
// of them drops the module rather than tripping its `checked > 0`
// non-vacuity guard (`.claude/rules/testing.md`, #1286). The guard then
// covers only the residual case where `is_enabled` stops agreeing with
// the feature it compiled under.
#[cfg(test)]
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
mod c_family_space_names_tests;
// Drift guard for the crate-level `## Supported Languages` list above.
#[cfg(test)]
mod lib_docs_tests;
#[cfg(test)]
mod observation_tests;

// --- Language identification (defined in `big-code-analysis-ast`) ---
pub use big_code_analysis_ast::{LANG, ParseLangError, get_from_emacs_mode, get_from_ext};

// Hand-written modules (`src/spaces.rs`, `src/output/dump_metrics.rs`,
// the metric macros) refer to per-metric submodules by their short
// crate-root path (`crate::abc`, `crate::cognitive`, ...). Re-export
// them under those names without widening the public surface.
pub(crate) use crate::metrics::{
    abc, cognitive, cyclomatic, halstead, loc, mi, nargs, nexits, nom, npa, npm, tokens, wmc,
};

// Module declarations. Each `pub use` line below names exactly the
// items intended to be part of the public API surface; anything not
// listed stays out of the crate root. Per issue #255, glob re-exports
// (`pub use module::*`) are no longer used here because every newly
// `pub`-marked helper in any sub-module would silently leak into the
// published API.

// --- Core analysis entry points and result types (spaces.rs) ---
mod spaces;
pub use crate::spaces::{Ast, CodeMetrics, FuncSpace, MetricsOptions, Source, SpaceKind, analyze};

/// Per-metric implementations.
///
/// Each sub-module owns one metric — its `Stats` accumulator, the
/// per-language trait implementations, and any small helpers used
/// only by tests. Most callers will not need these directly; reach
/// through [`CodeMetrics`] on a [`FuncSpace`] instead.
pub mod metrics;

/// Plain, `Deserialize`-capable data-transfer structs mirroring the
/// serialized metric wire shape. The compute types' `Serialize` impls
/// delegate here, making these the single definition of the JSON / YAML /
/// TOML / CBOR output format and the canonical way to read `bca` output
/// back (`serde_json::from_str::<wire::FuncSpace>(…)`).
pub mod wire;

// --- Change-history (VCS) metrics ---
//
// The project's first language-agnostic, non-AST metric family
// (issue #328). Gated behind the `vcs-git` backend feature (the
// `vcs` umbrella turns it on); the generic surface is backend-neutral
// so future backends (#335) reuse it unchanged.
/// Change-history (VCS) metrics derived from version-control history:
/// churn, commit frequency, author count / ownership dilution, bug- and
/// security-fix history, and an ordinal composite risk score. See
/// [`vcs::build_history_index`].
///
/// Enable with the `vcs` Cargo feature (the umbrella over the current
/// `vcs-git` backend, which is what the availability badge names).
#[cfg(feature = "vcs-git")]
#[cfg_attr(docsrs, doc(cfg(feature = "vcs-git")))]
pub mod vcs;

// --- Diagnostics ---
//
// The single place the library writes a `warning:` prefix; the CLI has
// its own severity ladder in `big-code-analysis-cli/src/diag.rs`.
mod diag;

// --- Errors ---
//
// `MetricsError` is the parse layer's error (every dispatch entry point
// returns it, including `LANG::tree_sitter_language`), so it is defined
// in `big-code-analysis-ast`; `FromPathError` wraps it for the
// file-backed `Ast::from_path` and stays here.
pub use big_code_analysis_ast::MetricsError;
mod from_path_error;
pub use crate::from_path_error::FromPathError;

// --- Metric selection ---
mod metric_set;
pub use crate::metric_set::{Metric, MetricSet, ParseMetricError};

// --- Suppression markers ---
mod suppression;
pub use crate::suppression::{
    SuppressionDialect, SuppressionMarker, SuppressionPolicy, SuppressionScope, SuppressionTarget,
    threshold_metric_for_name,
};

/// Canonical metric catalog: offender sub-metric ids with their
/// long-form sentences and [`metric_catalog::Direction`], plus the
/// family view rendered by `bca list-metrics`. Single source of truth
/// shared by the library's offender formatters and the CLI's threshold
/// engine, which pins its extractor ids to [`metric_catalog::METRICS`]
/// via a parity test.
pub mod metric_catalog;

/// Output formatters: CSV, SARIF, Checkstyle, clang/MSVC warning
/// lines, and AST/metric pretty-dumps used by `bca` and the offender
/// reporters.
///
/// The most commonly used writers (`write_csv`, `write_sarif`,
/// `write_checkstyle`, `write_clang_warning`, `write_code_climate`,
/// `write_msvc_warning`) and shared types (`OffenderRecord`,
/// `Severity`, `TOOL_ID`, `CSV_HEADER`, `CSV_EXTENSION`) are also
/// re-exported at the crate root.
pub mod output;
pub use crate::output::{
    CSV_EXTENSION, CSV_HEADER, ColorMode, OffenderRecord, Severity, TOOL_ID, defang_formula,
    dump_node, dump_node_with_color, dump_ops, dump_ops_with_color, dump_root,
    dump_root_with_color, write_checkstyle, write_clang_warning, write_code_climate, write_csv,
    write_csv_aggregate, write_msvc_warning, write_sarif, write_sarif_with_suppressed,
};

// --- AST plumbing (Node) ---
pub use big_code_analysis_ast::Node;

// --- Language detection / I/O helpers ---
pub use big_code_analysis_ast::{
    SkipReason, get_language_for_file, guess_language, is_generated, normalize_eol, read_file,
    read_file_with_eol, read_file_with_eol_classified, write_file,
};

// --- Source walker ---
mod concurrent_files;
pub use crate::concurrent_files::{
    ConcurrentErrors, ConcurrentRunner, FilesData, NumJobs, ParseNumJobsError,
};

// --- Per-file node counting (reached via the `Ast` seam) ---
pub use big_code_analysis_ast::{Count, CountCollector};

mod function;
pub use crate::function::{FunctionSpan, dump_function_spans, dump_function_spans_with_color};

// --- AST dump ---
pub use big_code_analysis_ast::{
    AstCfg, AstNode, AstPayload, AstResponse, MAX_AST_SERIALIZE_DEPTH, Span,
};

// --- Halstead operator/operand result type ---
mod ops;
pub use crate::ops::Ops;

// --- Preprocessor handling (C/C++) ---
pub use big_code_analysis_ast::{
    PreprocDiagnostic, PreprocFile, PreprocResults, fix_includes, get_macros, preprocess,
};

// --- Generic parser plumbing ---
//
// `Parser`, `ParserTrait`, `Filter`, `LanguageInfo`, `Checker`,
// `Getter` and `Alterator` are defined in `big-code-analysis-ast` and
// reach this crate through the glob import above. They are not
// re-exported: the single public analysis seam is [`Ast`], which wraps
// the language-dispatched `AnyParser` carrier. See STABILITY.md.
//
// The metric half of the parser contract; `ParserTrait` is the parse
// half. See `src/metric_suite.rs`.
mod metric_suite;
pub(crate) use crate::metric_suite::MetricSuite;

/// Re-export of the underlying `tree-sitter` crate.
///
/// Lets callers build a [`tree_sitter::Tree`] (via
/// [`tree_sitter::Parser`]) against the exact grammar version this
/// library is pinned to, and feed it back through
/// [`Ast::from_tree_sitter`] without taking a separate `tree-sitter`
/// dependency that may drift out of pin.
///
/// This is part of the value-not-stable surface: the underlying
/// pin may bump in any minor release (see `STABILITY.md`). The inner
/// node of a [`Node`] is reached the same way, through
/// [`Node::as_tree_sitter`], and carries the same value-not-stable
/// caveat.
pub use ::tree_sitter;

/// The version of this `big-code-analysis` library crate.
///
/// Sourced from the crate's own `CARGO_PKG_VERSION` at compile time.
/// Exposed so downstream surfaces (the REST `/v1/version` endpoint, the
/// Python `__version__` attribute, …) can report the exact library
/// version they were built against without re-deriving it from Cargo
/// metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Recommended entry points for the 90% case.
///
/// Star-import this module to get the curated set of types and
/// functions most callers need:
///
/// ```no_run
/// use big_code_analysis::prelude::*;
///
/// let source = b"fn main() {}";
/// let space = analyze(
///     Source::new(LANG::Rust, source),
///     MetricsOptions::default(),
/// ).expect("Rust source parses");
/// # let _ = space;
/// ```
///
/// Anything not exposed here can still be imported with its
/// fully-qualified name from the crate root (`use
/// big_code_analysis::Something;`). Items deliberately omitted from
/// the prelude are either deprecated, doc-hidden, or unlikely to
/// appear in typical caller code.
pub mod prelude {
    pub use crate::{
        // Parse-once handle
        Ast,
        // Result types
        CodeMetrics,
        // Errors and options
        FromPathError,
        FuncSpace,
        // Language enum
        LANG,
        // Metric selection
        Metric,
        MetricsError,
        MetricsOptions,
        Source,
        SpaceKind,
        // Core entry points
        analyze,
    };
}
