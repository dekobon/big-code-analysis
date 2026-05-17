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
//! ## Supported Languages
//!
//! - Bash
//! - C++
//! - Go
//! - Java
//! - JavaScript
//! - JavaScript (Firefox-internal, "MozJS")
//! - Kotlin
//! - Perl
//! - PHP
//! - Python
//! - Rust
//! - TSX
//! - TypeScript
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

mod c_langs_macros;
mod c_macro;
mod getter;
mod macros;

mod alterator;
pub use alterator::*;

mod node;
pub use crate::node::*;

mod metrics;
pub use metrics::*;

mod languages;
pub(crate) use languages::*;

mod checker;
pub(crate) use checker::*;

mod output;
pub use output::*;

mod spaces;
pub use crate::spaces::*;

mod ops;
pub use crate::ops::*;

mod find;
pub use crate::find::*;

mod function;
pub use crate::function::*;

mod ast;
pub use crate::ast::*;

mod count;
pub use crate::count::*;

mod preproc;
pub use crate::preproc::*;

mod langs;
pub use crate::langs::*;

mod tools;
pub use crate::tools::*;

mod concurrent_files;
pub use crate::concurrent_files::*;

mod traits;
pub use crate::traits::*;

mod parser;
pub use crate::parser::*;

mod comment_rm;
pub use crate::comment_rm::*;
