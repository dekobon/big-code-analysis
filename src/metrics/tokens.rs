// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use crate::checker::Checker;
use crate::macros::implement_metric_trait;

use crate::*;

/// The `Tokens` metric: per-function and per-file count of tree-sitter
/// leaf tokens, excluding any leaf whose ancestor chain includes a
/// comment node.
///
/// This is a token-based size proxy: it counts the lexer's tokens
/// (identifiers, literals, keywords, punctuation) rather than lines or
/// Halstead operators/operands. Punctuation that Halstead skips
/// (parentheses, semicolons, separators) does contribute, so
/// `tokens` ≠ Halstead `N1 + N2`.
#[derive(Clone, Debug)]
pub struct Stats {
    tokens: usize,
    tokens_sum: usize,
    tokens_min: usize,
    tokens_max: usize,
    space_count: usize,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            tokens: 0,
            tokens_sum: 0,
            tokens_min: usize::MAX,
            tokens_max: 0,
            space_count: 1,
        }
    }
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("tokens", 4)?;
        st.serialize_field("tokens", &self.tokens_sum())?;
        st.serialize_field("tokens_average", &self.tokens_average())?;
        st.serialize_field("tokens_min", &self.tokens_min())?;
        st.serialize_field("tokens_max", &self.tokens_max())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "tokens: {}, \
             tokens_average: {}, \
             tokens_min: {}, \
             tokens_max: {}",
            self.tokens_sum(),
            self.tokens_average(),
            self.tokens_min(),
            self.tokens_max(),
        )
    }
}

impl Stats {
    /// Merges a second `Tokens` metric suite into the first one.
    pub fn merge(&mut self, other: &Stats) {
        self.tokens_min = self.tokens_min.min(other.tokens_min);
        self.tokens_max = self.tokens_max.max(other.tokens_max);
        self.tokens_sum += other.tokens_sum;
        self.space_count += other.space_count;
    }

    /// Returns the total token count across all merged spaces.
    #[inline]
    #[must_use]
    pub fn tokens_sum(&self) -> f64 {
        self.tokens_sum as f64
    }

    /// Returns the average tokens per space.
    #[inline]
    #[must_use]
    pub fn tokens_average(&self) -> f64 {
        self.tokens_sum() / self.space_count as f64
    }

    /// Returns the smallest single-space token count.
    ///
    /// Diverges intentionally from `nom::Stats::functions_min`, which
    /// surfaces the raw `usize::MAX` sentinel for a never-observed
    /// space. We collapse the sentinel to `0.0` so a `Stats::default()`
    /// that bypasses the metric pipeline serializes to a meaningful
    /// number rather than `1.8446744e19`.
    #[inline]
    #[must_use]
    pub fn tokens_min(&self) -> f64 {
        if self.tokens_min == usize::MAX {
            0.0
        } else {
            self.tokens_min as f64
        }
    }

    /// Returns the largest single-space token count.
    #[inline]
    #[must_use]
    pub fn tokens_max(&self) -> f64 {
        self.tokens_max as f64
    }

    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.tokens_sum += self.tokens;
    }

    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.tokens_min = self.tokens_min.min(self.tokens);
        self.tokens_max = self.tokens_max.max(self.tokens);
        self.compute_sum();
    }
}

/// Per-language counting of tokens.
pub trait Tokens
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    fn compute(node: &Node, stats: &mut Stats) {
        if node.child_count() != 0 {
            return;
        }
        // Walk the leaf's ancestors so grammars whose comments have
        // internal structure (e.g. Rust doc comments split into
        // markers and content) also exclude inner leaves; the leaf
        // itself is the first item, so bare comment nodes are caught
        // immediately.
        let in_comment =
            std::iter::successors(Some(*node), Node::parent).any(|n| Self::is_comment(&n));
        if !in_comment {
            stats.tokens += 1;
        }
    }
}

implement_metric_trait!(
    [Tokens],
    PythonCode,
    MozjsCode,
    JavascriptCode,
    TypescriptCode,
    TsxCode,
    CppCode,
    RustCode,
    PreprocCode,
    CcommentCode,
    JavaCode,
    KotlinCode,
    GoCode,
    PerlCode,
    BashCode,
    LuaCode,
    TclCode,
    PhpCode,
    CsharpCode,
    ElixirCode,
    RubyCode
);

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use crate::tools::check_metrics;

    use super::*;

    /// `def foo(x): return x` → leaves: `def`, `foo`, `(`, `x`, `)`,
    /// `:`, `return`, `x` = 8 tokens, hand-counted.
    #[test]
    fn python_tokens_exact_count() {
        check_metrics::<PythonParser>("def foo(x): return x", "foo.py", |metric| {
            assert_eq!(metric.tokens.tokens_sum(), 8.0);
            assert!(metric.tokens.tokens_max() >= 7.0);
        });
    }

    /// Adding a Python comment must not change the token count.
    #[test]
    fn python_tokens_comments_excluded() {
        check_metrics::<PythonParser>(
            "def foo(x): return x  # explanation\n# header\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.tokens.tokens_sum(), 8.0);
            },
        );
    }

    /// Blank lines and indentation must not change the token count.
    #[test]
    fn python_tokens_whitespace_excluded() {
        check_metrics::<PythonParser>(
            "\n\n    def foo(x):\n        return x\n\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.tokens.tokens_sum(), 8.0);
            },
        );
    }

    /// Tokens must exceed Halstead `N1 + N2` for code containing
    /// punctuation Halstead skips. Guards against accidental Halstead
    /// reuse.
    #[test]
    fn python_tokens_distinct_from_halstead() {
        check_metrics::<PythonParser>("def foo(x): return (x + 1)", "foo.py", |metric| {
            let halstead_total = metric.halstead.operators() + metric.halstead.operands();
            assert!(
                metric.tokens.tokens_sum() > halstead_total,
                "expected tokens ({}) > halstead N1+N2 ({}); punctuation \
                 like `(`, `)`, `:` should contribute to tokens but not Halstead",
                metric.tokens.tokens_sum(),
                halstead_total,
            );
        });
    }

    /// Inner functions get attributed to their innermost scope. For
    /// `def outer(): def inner(): return 1`, the inner scope owns
    /// `def, inner, (, ), :, return, 1` = 7 tokens; the outer scope
    /// owns `def, outer, (, ), :` = 5; the unit owns 0 directly.
    /// Asserting the exact `tokens_max` is what catches an attribution
    /// regression — a broken implementation that credited all 12
    /// tokens to one scope would still pass `max <= sum`.
    #[test]
    fn python_tokens_nested_attribution() {
        check_metrics::<PythonParser>(
            "def outer():\n    def inner():\n        return 1\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.tokens.tokens_sum(), 12.0);
                assert_eq!(metric.tokens.tokens_max(), 7.0);
                assert_eq!(metric.tokens.tokens_min(), 0.0);
            },
        );
    }

    /// C++ `/* … */` block comments must not contribute.
    /// Same fixture with and without comment yields the same count.
    #[test]
    fn cpp_tokens_block_comments_excluded() {
        check_metrics::<CppParser>(
            "int foo(int x) { /* multi\n   line */ return x; }",
            "foo.cpp",
            |m| {
                // Leaves outside the comment:
                // int, foo, (, int, x, ), {, return, x, ;, } = 11.
                assert_eq!(m.tokens.tokens_sum(), 11.0);
            },
        );
        check_metrics::<CppParser>("int foo(int x) { return x; }", "foo.cpp", |m| {
            assert_eq!(m.tokens.tokens_sum(), 11.0);
        });
    }

    /// C++ `// …` line comments must not contribute, matching the Python
    /// hand-counted style.  Leaves outside the comment:
    /// `int`, `x`, `=`, `1`, `;` = 5.
    #[test]
    fn cpp_tokens_line_comments_excluded() {
        check_metrics::<CppParser>("int x = 1; // a one-line comment\n", "foo.cpp", |m| {
            assert_eq!(m.tokens.tokens_sum(), 5.0);
        });
        check_metrics::<CppParser>("int x = 1;\n", "foo.cpp", |m| {
            assert_eq!(m.tokens.tokens_sum(), 5.0);
        });
    }

    /// Whitespace and blank lines must not contribute to the token count
    /// (mirrors `python_tokens_whitespace_excluded`).
    #[test]
    fn cpp_tokens_whitespace_excluded() {
        check_metrics::<CppParser>("\n\nint foo(int x) {\n    return x;\n}\n", "foo.cpp", |m| {
            // int, foo, (, int, x, ), {, return, x, ;, } = 11.
            assert_eq!(m.tokens.tokens_sum(), 11.0);
        });
    }

    /// Tokens count punctuation that Halstead skips (parentheses, braces,
    /// semicolons), so `tokens_sum` must exceed `N1 + N2` for a fixture
    /// with significant punctuation.  Mirrors
    /// `python_tokens_distinct_from_halstead`.
    #[test]
    fn cpp_tokens_distinct_from_halstead() {
        check_metrics::<CppParser>("int foo(int x) { return (x + 1); }", "foo.cpp", |m| {
            let halstead_total = m.halstead.operators() + m.halstead.operands();
            assert!(
                m.tokens.tokens_sum() > halstead_total,
                "expected tokens ({}) > halstead N1+N2 ({}); punctuation like \
                 `(`, `)`, `{{`, `}}` and `;` should contribute to tokens but not Halstead",
                m.tokens.tokens_sum(),
                halstead_total,
            );
        });
    }

    /// Inner functions attribute their tokens to their innermost scope.
    /// For `void outer() { void inner_stub(); int x = 1; }` with the
    /// inner forward-declaration, leaves split across the outer space
    /// and the unit, mirroring the Python nested-attribution test.
    #[test]
    fn cpp_tokens_nested_attribution() {
        check_metrics::<CppParser>(
            "int outer() {\n    auto inner = []() { return 1; };\n    return inner();\n}\n",
            "foo.cpp",
            |m| {
                // Outer function owns its statements; the inner lambda owns its body.
                // The unit-level sum must equal the total of all scopes.
                // tokens_max must equal one of the scope sums and be at least the
                // tokens count of the lambda body (`return 1 ;` plus surrounding
                // brackets — minimum 7).
                assert!(m.tokens.tokens_sum() > 0.0, "expected non-zero tokens_sum");
                assert!(
                    m.tokens.tokens_max() >= 7.0,
                    "expected tokens_max >= 7 (outer scope dominates), got {}",
                    m.tokens.tokens_max(),
                );
                assert!(
                    m.tokens.tokens_max() <= m.tokens.tokens_sum(),
                    "tokens_max ({}) cannot exceed tokens_sum ({})",
                    m.tokens.tokens_max(),
                    m.tokens.tokens_sum(),
                );
            },
        );
    }

    /// Java `// …` line comments must not contribute.
    #[test]
    fn java_tokens_line_comments_excluded() {
        check_metrics::<JavaParser>(
            "class A { void foo() { // hi\n return; } }",
            "A.java",
            |m| {
                // class, A, {, void, foo, (, ), {, return, ;, }, } = 12.
                assert_eq!(m.tokens.tokens_sum(), 12.0);
            },
        );
        check_metrics::<JavaParser>("class A { void foo() { return; } }", "A.java", |m| {
            assert_eq!(m.tokens.tokens_sum(), 12.0);
        });
    }

    /// Rust doc comments may split into structured children under
    /// some grammars; the ancestor walk must filter every inner leaf.
    #[test]
    fn rust_tokens_doc_comments_excluded() {
        check_metrics::<RustParser>(
            "/// outer doc\n/// more doc\nfn f() { let x = 1; }",
            "foo.rs",
            |m| {
                // fn, f, (, ), {, let, x, =, 1, ;, } = 11.
                assert_eq!(m.tokens.tokens_sum(), 11.0);
            },
        );
        check_metrics::<RustParser>("fn f() { let x = 1; }", "foo.rs", |m| {
            assert_eq!(m.tokens.tokens_sum(), 11.0);
        });
    }

    // -- Per-language smoke tests --------------------------------------
    //
    // Lesson 1 (`docs/development/lessons_learned.md`): every supported
    // language must have a positive test that asserts non-zero tokens
    // on real source. Catches the silent-zero regression where a
    // metric is registered but never fires. `check_metrics` takes a
    // `fn` pointer so each test inlines its assertion directly.

    #[test]
    fn smoke_python() {
        check_metrics::<PythonParser>("x = 1\n", "foo.py", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_rust() {
        check_metrics::<RustParser>("fn f() { let x = 1; }", "foo.rs", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_cpp() {
        check_metrics::<CppParser>("int x = 1;", "foo.cpp", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_java() {
        check_metrics::<JavaParser>("class A { int x = 1; }", "A.java", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_csharp() {
        check_metrics::<CsharpParser>("class A { int X = 1; }", "A.cs", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_javascript() {
        check_metrics::<JavascriptParser>("let x = 1;", "foo.js", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_mozjs() {
        check_metrics::<MozjsParser>("let x = 1;", "foo.js", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_typescript() {
        check_metrics::<TypescriptParser>("const x: number = 1;", "foo.ts", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_tsx() {
        check_metrics::<TsxParser>("const x: number = 1;", "foo.tsx", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_go() {
        check_metrics::<GoParser>("package main\nfunc f() {}", "foo.go", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_kotlin() {
        check_metrics::<KotlinParser>("fun f(): Int = 1", "foo.kt", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_lua() {
        check_metrics::<LuaParser>("local x = 1", "foo.lua", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_bash() {
        check_metrics::<BashParser>("x=1", "foo.sh", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_tcl() {
        check_metrics::<TclParser>("set x 1", "foo.tcl", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_perl() {
        check_metrics::<PerlParser>("my $x = 1;", "foo.pl", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_php() {
        check_metrics::<PhpParser>("<?php $x = 1;", "foo.php", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_preproc() {
        check_metrics::<PreprocParser>("#define FOO 1\n", "foo.h", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }

    #[test]
    fn smoke_ccomment() {
        // Ccomment's grammar parses bare C source; non-comment text
        // produces non-comment leaves.
        check_metrics::<CcommentParser>("int x = 1;", "foo.c", |m| {
            assert!(m.tokens.tokens_sum() > 0.0);
        });
    }
}
