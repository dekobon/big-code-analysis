// Per-language modules deliberately consume the macro-generated
// tree-sitter token enums via `use crate::*` and `use Foo::*` inside
// match expressions — explicit imports would list dozens of variants per
// arm and obscure the per-language token sets that are the point of
// these files. Allowed at the module level rather than per function so
// the per-language impl blocks stay readable.
#![allow(clippy::doc_markdown, clippy::enum_glob_use, clippy::wildcard_imports)]
// Per-language Cargo features let a downstream build only a subset of
// grammars. In such a build the code for the disabled languages — their
// macro-generated `*Code` / `*Parser` tags plus the getter / checker
// helpers only those languages reach — is compiled but never
// constructed, so `-D dead-code` fires on items that are all live in the
// default `all-languages` build. Relax dead-code to a warning only when
// the full language set is NOT enabled; the default build and
// `--all-features` (and thus the primary CI gate and `make pre-commit`)
// still hard-deny it, so genuine dead code is caught there.
#![cfg_attr(not(feature = "all-languages"), allow(dead_code))]

//! The parse and classification layer behind
//! [`big-code-analysis`](https://crates.io/crates/big-code-analysis).
//!
//! This crate turns source bytes into a [`tree_sitter`] tree and
//! answers structural questions about it: which [`LANG`] a file is in,
//! what a [`Node`] is (`is_func`, `is_call`, `is_string`, … through
//! [`Checker`]), what it is called and which Halstead class or
//! [`SpaceKind`] it opens ([`Getter`]), and how it should be rendered
//! as an [`AstNode`] ([`Alterator`]). It also owns the C-family
//! preprocessor pass ([`preproc`]), comment stripping, node counting and
//! finding, and language detection ([`guess_language`]). It computes no
//! metric.
//!
//! # Stability
//!
//! **This crate is internal plumbing.** It exists so the metric walk in
//! `big-code-analysis` and any future structural consumer (a linter, a
//! call-graph builder, a language server) can share one classification
//! layer without the metric machinery. `big-code-analysis` pins it at
//! an exact `=X.Y.Z` version, the two are released together, and no
//! item here carries a stability promise of its own: names, signatures
//! and module paths may change in any release. Depend on
//! `big-code-analysis` and reach what it re-exports; depend on this
//! crate directly only when you accept re-pinning on every release.
//!
//! The one contract that does hold is the one `big-code-analysis`
//! documents in its `STABILITY.md` for the items it re-exports from
//! here (`LANG`, `Node`, `MetricsError`, `SpaceKind`, the AST dump
//! types, the preprocessor types, the file readers). Those are stable
//! *through that crate*.
//!
//! # Layout
//!
//! - [`languages`]: one generated enum per grammar, one variant per
//!   tree-sitter kind id.
//! - [`langs`]: the [`LANG`] enum, extension / emacs-mode lookup, the
//!   per-language `*Code` tags and `*Parser` aliases, and [`AnyParser`],
//!   the runtime-dispatched parser every consumer matches on with
//!   [`with_any_parser!`].
//! - [`node`], [`traits`], [`parser`]: the tree-sitter wrappers and the
//!   [`ParserTrait`] / [`Search`] / [`LanguageInfo`] contracts.
//! - [`checker`], [`getter`], [`alterator`], [`lang_helpers`]: the
//!   per-language classifiers and the byte-level identity helpers they
//!   share with the metrics.
//! - [`preproc`], `c_macro`, [`c_declarator`]: the C-family pipeline.
//! - `comment_rm`, [`ast`], [`count`], `find`, [`tools`]: the
//!   per-file operations reached through `big_code_analysis::Ast`.
//!
//! The unlinked names above are crate-private modules, reached only
//! through the `AnyParser` methods `mk_action!` generates.

#![allow(clippy::upper_case_acronyms)]
// Production-only `unwrap()` ban. See `[workspace.lints.clippy]` in the
// root `Cargo.toml` for why this is a per-root attribute and not a
// Cargo lint (#1227).
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

// The `pub(crate)` entries below are named by nothing outside this
// crate, so they stay narrow per AGENTS.md ("widen visibility only when
// an item is re-exported from `lib.rs`"). `comment_rm` and `find` look
// like exceptions and are not: they are reached through `$crate::` in
// `mk_action!`, which expands here.
pub mod alterator;
pub mod ast;
pub mod c_declarator;
pub(crate) mod c_langs_macros;
pub(crate) mod c_macro;
pub(crate) mod cfg_predicate;
pub mod checker;
pub(crate) mod comment_rm;
pub mod count;
pub mod error;
pub(crate) mod find;
pub mod getter;
pub mod lang_helpers;
pub mod langs;
pub mod languages;
pub mod macros;
pub mod node;
pub mod observation;
pub mod parser;
pub mod preproc;
pub mod recursion;
pub mod space_kind;
pub mod token_role;
pub mod tools;
pub mod traits;

#[cfg(test)]
mod language_enum_roundtrip;

/// Parse-and-inspect helpers for tests, in this crate and in
/// `big-code-analysis`'s metric tests (behind the `test-support`
/// feature). Never part of a shipping build.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod test_support;

// Flat re-exports. The per-language modules reach every token enum, tag
// and classifier through `use crate::*`; `big-code-analysis` does the
// same through one `pub use big_code_analysis_ast::*`, so the
// names below are the crate's working vocabulary rather than a curated
// public surface.
pub use crate::alterator::Alterator;
pub use crate::ast::{AstCfg, AstNode, AstPayload, AstResponse, MAX_AST_SERIALIZE_DEPTH, Span};
pub use crate::checker::*;
pub use crate::count::{Count, CountCollector};
pub use crate::error::MetricsError;
pub use crate::getter::Getter;
pub use crate::langs::*;
pub use crate::languages::*;
pub use crate::macros::ParseLangError;
pub use crate::node::{Ancestors, Node};
pub use crate::parser::Parser;
pub use crate::preproc::{
    PreprocDiagnostic, PreprocFile, PreprocResults, fix_includes, get_macros, preprocess,
};
pub use crate::space_kind::SpaceKind;
pub use crate::token_role::TokenRole;
pub use crate::tools::{
    SkipReason, get_language_for_file, guess_language, is_generated, normalize_eol, read_file,
    read_file_with_eol, read_file_with_eol_classified, write_file,
};
pub use crate::traits::{LanguageInfo, ParserTrait, Search};

/// Re-export of the underlying `tree-sitter` crate, so a consumer can
/// build a [`tree_sitter::Tree`] against the exact grammar version this
/// crate is pinned to.
pub use ::tree_sitter;
