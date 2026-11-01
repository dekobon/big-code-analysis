//! The hand-written control-flow fixtures that were the oracle inputs
//! for the role-table series (#1697, preserved by #1744): one `r07.*`
//! per language spelling every `Branch` / `Switch` / `Loop` / `Catch` /
//! `ElseIf` / `Exit` owner and payer plus the keyword-outside-owner
//! cases, the two `c5b.*` Tcl / Elixir negatives, and the two `sw.*`
//! switch-nesting inputs. Every file parses with zero `ERROR` /
//! `MISSING` nodes at the pinned grammars, so none is a recovery-shape
//! fixture.
//!
//! The snapshots were captured at `fdd12ad4`, the tree the R07 oracle
//! runs verified byte-identical to base except for two fixes they
//! therefore freeze: in `rust/r07.rs` the type-syntax `for` of `hrtb`
//! / `wherebound` / `fnptr` scores cyclomatic 1 while the loop `for`
//! of `loops` still pays — and so does every keyword inside `mac`'s
//! opaque `token_tree`, which stays at 5 by policy (#1720); and in
//! `elixir/r07.ex` / `elixir/c5b.ex` the keyword sections (`do:` /
//! `else:` / `rescue:` …) score cognitive as their block twins for
//! every `Call` (#1602).
#![allow(missing_docs)]
use crate::common;

use std::path::Path;

use common::compare_rca_output_with_files_under;

// Hand-written, not derived: the corpus walk picks a language per
// file at run time from its extension, so nothing in this body names
// the grammars it needs. The glob list is what decides them, and a
// build lacking any one of them scores that file zero rather than
// matching its snapshot (#1472) — hence `all(…)` over every language
// the globs route to, not `any(…)`. `*.tsx` rides the `typescript`
// feature with `*.ts`.
// test-lang-gates: hand-written(bash,c,cpp,csharp,elixir,go,groovy,irules,java,javascript,kotlin,lua,objc,perl,php,python,ruby,rust,tcl,typescript)
//     — the corpus walk picks a language per file from its extension,
//     so the glob list decides it and nothing in the body names it
#[cfg(all(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
#[test]
fn test_control_flow() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("repositories")
        .join("big-code-analysis-output");

    compare_rca_output_with_files_under(
        &source_root,
        "control_flow",
        &[
            "*.sh", "*.c", "*.cpp", "*.cs", "*.ex", "*.go", "*.groovy", "*.irule", "*.java",
            "*.js", "*.kt", "*.lua", "*.m", "*.pl", "*.php", "*.py", "*.rb", "*.rs", "*.tcl",
            "*.ts", "*.tsx",
        ],
        &[],
        25,
    );
}
