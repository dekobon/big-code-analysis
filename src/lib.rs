// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::doc_markdown, clippy::enum_glob_use, clippy::wildcard_imports)]

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
//! - C/C++ (`cpp`, also exposes the internal `Ccomment` / `Preproc` helpers)
//! - C# (`csharp`)
//! - Elixir (`elixir`)
//! - Go (`go`)
//! - Groovy (`groovy`)
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
//! - TSX (`typescript`)
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

// Internal-only modules. Nothing is re-exported from these.
mod c_langs_macros;
mod c_macro;
mod cfg_predicate;
mod checker;
mod getter;
mod languages;
mod macros;

// `langs` hosts the `mk_langs!` macro expansion. Every name produced
// there — `LANG`, the `action` / `get_function_spaces` dispatch
// helpers, per-language `<Lang>Code` tags and `<Lang>Parser` aliases —
// is enumerated explicitly in the curated re-exports below.
mod langs;
pub use crate::langs::{
    BashCode, BashParser, CcommentCode, CcommentParser, CppCode, CppParser, CsharpCode,
    CsharpParser, ElixirCode, ElixirParser, GoCode, GoParser, GroovyCode, GroovyParser, JavaCode,
    JavaParser, JavascriptCode, JavascriptParser, KotlinCode, KotlinParser, LANG, LuaCode,
    LuaParser, MozjsCode, MozjsParser, PerlCode, PerlParser, PhpCode, PhpParser, PreprocCode,
    PreprocParser, PythonCode, PythonParser, RubyCode, RubyParser, RustCode, RustParser, TclCode,
    TclParser, TsxCode, TsxParser, TypescriptCode, TypescriptParser, action, analyze_dispatch,
    get_from_emacs_mode, get_from_ext, get_ops, metrics_from_tree,
};
// The path-positional `get_function_spaces*` shims are `#[deprecated]`
// at their definition sites; re-exporting them at the crate root keeps
// the previously-globbed surface intact, scoped with
// `#[allow(deprecated)]` so the re-export itself does not warn.
#[allow(deprecated)]
pub use crate::langs::{get_function_spaces, get_function_spaces_with_options};

// Internal crate-root re-exports. Hand-written per-language modules
// (`src/getter.rs`, `src/checker.rs`, `src/alterator.rs`, the
// per-language metric impls) use `use crate::*` to bring the
// macro-generated `<Lang>Code` token enums and per-language helper
// types into scope; the per-language token enums in
// `src/languages/language_*.rs` are also reached through the crate
// root. Re-exporting these as `pub(crate)` keeps internal compilation
// working without widening the published surface.
pub(crate) use crate::checker::*;
pub(crate) use crate::languages::*;

// Hand-written modules (`src/spaces.rs`, `src/output/dump_metrics.rs`,
// the metric macros) refer to per-metric submodules by their short
// crate-root path (`crate::abc`, `crate::cognitive`, ...). Re-export
// them under those names without widening the public surface.
pub(crate) use crate::metrics::{
    abc, cognitive, cyclomatic, exit, halstead, loc, mi, nargs, nom, npa, npm, tokens, wmc,
};

// Module declarations. Each `pub use` line below names exactly the
// items intended to be part of the public API surface; anything not
// listed stays out of the crate root. Per issue #255, glob re-exports
// (`pub use module::*`) are no longer used here because every newly
// `pub`-marked helper in any sub-module would silently leak into the
// published API.

// --- Core analysis entry points and result types (spaces.rs) ---
mod spaces;
pub use crate::spaces::{
    Ast, CodeMetrics, FuncSpace, Metrics, MetricsCfg, MetricsOptions, Source, SpaceKind, analyze,
};
// The path-positional `metrics` / `metrics_with_options` shims are
// `#[deprecated]` at their definition site; re-export them so the
// previously-globbed API surface keeps working, scoped with
// `#[allow(deprecated)]` to avoid lint noise at this seam.
// `metrics_inner` is consumed by feature-gated arms in `mk_action!`.
// With `--no-default-features` and no language feature, every arm
// compiles out and the re-export becomes nominally unused; the
// language-features that ship in the default set keep the symbol
// live in any normal build.
#[allow(unused_imports)]
pub(crate) use crate::spaces::metrics_inner;
#[allow(deprecated)]
pub use crate::spaces::{metrics, metrics_with_options};
#[cfg(test)]
pub(crate) use crate::tools::check_func_space;

/// Per-metric implementations.
///
/// Each sub-module owns one metric — its `Stats` accumulator, the
/// per-language trait implementations, and any small helpers used
/// only by tests. Most callers will not need these directly; reach
/// through [`CodeMetrics`] on a [`FuncSpace`] instead.
pub mod metrics;

// --- Errors ---
mod error;
pub use crate::error::MetricsError;

// --- Metric selection ---
mod metric_set;
pub use crate::metric_set::{Metric, MetricSet, ParseMetricError};

// --- Suppression markers ---
mod suppression;
pub use crate::suppression::{MetricKind, SuppressionPolicy, SuppressionScope};

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
    CSV_EXTENSION, CSV_HEADER, Dump, DumpCfg, OffenderRecord, Severity, TOOL_ID, dump_node,
    dump_ops, dump_root, write_checkstyle, write_clang_warning, write_code_climate, write_csv,
    write_msvc_warning, write_sarif,
};

// --- AST plumbing (Node, Cursor) ---
mod node;
pub use crate::node::{Cursor, Node};

// --- Language detection / I/O helpers ---
mod tools;
pub use crate::tools::{
    get_language_for_file, guess_language, is_generated, read_file, read_file_with_eol, write_file,
};

// --- Source walker ---
mod concurrent_files;
pub use crate::concurrent_files::{ConcurrentErrors, ConcurrentRunner, FilesData};

// --- Comment removal ---
mod comment_rm;
pub use crate::comment_rm::{CommentRm, CommentRmCfg, rm_comments};

// --- Per-function metric callbacks (CLI surface) ---
mod count;
pub use crate::count::{Count, CountCfg, count};

mod find;
pub use crate::find::{Find, FindCfg, find};

mod function;
pub use crate::function::{Function, FunctionCfg, FunctionSpan, function};

// --- AST dump ---
mod ast;
pub use crate::ast::{AstCallback, AstCfg, AstNode, AstPayload, AstResponse, Span};

// --- Halstead operator/operand callback ---
mod ops;
pub use crate::ops::{Ops, OpsCfg, OpsCode, operands_and_operators};

// --- Preprocessor handling (C/C++) ---
mod preproc;
pub use crate::preproc::{PreprocFile, PreprocResults, fix_includes, get_macros, preprocess};

// --- Alterator trait (per-language AST simplification) ---
mod alterator;
pub use crate::alterator::Alterator;

// --- Generic parser plumbing ---
//
// `Parser`, `ParserTrait`, `Filter`, `LanguageInfo`, and `Callback`
// are part of the value-not-stable surface — they are required for
// callers that want to feed pre-parsed trees through the metric
// pipeline or implement a custom `Callback`, but they are
// `#[doc(hidden)]` at their definition sites so they do not clutter
// the rendered rustdoc. See STABILITY.md.
mod parser;
pub use crate::parser::{Filter, Parser};

mod traits;
pub(crate) use crate::traits::Search;
pub use crate::traits::{Callback, LanguageInfo, ParserTrait};

/// Re-export of the underlying `tree-sitter` crate.
///
/// Lets callers build a [`tree_sitter::Tree`] (via
/// [`tree_sitter::Parser`]) against the exact grammar version this
/// library is pinned to, and feed it back through
/// [`Parser::from_tree`] / [`metrics_from_tree`] without taking a
/// separate `tree-sitter` dependency that may drift out of pin.
///
/// This is part of the value-not-stable surface: the underlying
/// pin may bump in any minor release (see `STABILITY.md`).
pub use ::tree_sitter;

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
        FuncSpace,
        // Language enum
        LANG,
        // Metric selection
        Metric,
        // Errors and options
        MetricsError,
        MetricsOptions,
        Source,
        SpaceKind,
        // Core entry points
        analyze,
        metrics_from_tree,
    };
}
