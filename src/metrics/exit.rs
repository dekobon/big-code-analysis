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

/// The `NExit` metric.
///
/// This metric counts the number of possible exit points
/// from a function/method.
#[derive(Debug, Clone)]
pub struct Stats {
    exit: usize,
    exit_sum: usize,
    total_space_functions: usize,
    exit_min: usize,
    exit_max: usize,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            exit: 0,
            exit_sum: 0,
            total_space_functions: 1,
            exit_min: usize::MAX,
            exit_max: 0,
        }
    }
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("nexits", 4)?;
        st.serialize_field("sum", &self.exit_sum())?;
        st.serialize_field("average", &self.exit_average())?;
        st.serialize_field("min", &self.exit_min())?;
        st.serialize_field("max", &self.exit_max())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "sum: {}, average: {} min: {}, max: {}",
            self.exit_sum(),
            self.exit_average(),
            self.exit_min(),
            self.exit_max()
        )
    }
}

impl Stats {
    /// Merges a second `NExit` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.exit_max = self.exit_max.max(other.exit_max);
        self.exit_min = self.exit_min.min(other.exit_min);
        self.exit_sum += other.exit_sum;
    }

    /// Returns the `NExit` metric value
    #[must_use]
    pub fn exit(&self) -> f64 {
        self.exit as f64
    }
    /// Returns the `NExit` metric sum value
    #[must_use]
    pub fn exit_sum(&self) -> f64 {
        self.exit_sum as f64
    }
    /// Returns the `NExit` metric minimum value.
    ///
    /// Collapses the `usize::MAX` sentinel that `Stats::default()` plants
    /// into `exit_min` to `0.0`, so a never-observed space
    /// serializes to a meaningful number rather than `1.8446744e19`.
    #[must_use]
    pub fn exit_min(&self) -> f64 {
        if self.exit_min == usize::MAX {
            0.0
        } else {
            self.exit_min as f64
        }
    }
    /// Returns the `NExit` metric maximum value
    #[must_use]
    pub fn exit_max(&self) -> f64 {
        self.exit_max as f64
    }

    /// Returns the `NExit` metric average value
    ///
    /// This value is computed dividing the `NExit` value
    /// for the total number of functions/closures in a space.
    ///
    /// If there are no functions in a code, its value is `NAN`.
    #[must_use]
    pub fn exit_average(&self) -> f64 {
        self.exit_sum() / self.total_space_functions as f64
    }
    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.exit_sum += self.exit;
    }
    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.exit_max = self.exit_max.max(self.exit);
        self.exit_min = self.exit_min.min(self.exit);
        self.compute_sum();
    }
    pub(crate) fn finalize(&mut self, total_space_functions: usize) {
        self.total_space_functions = total_space_functions;
    }
}

#[doc(hidden)]
/// Per-language computation of the exit-point count.
pub trait Exit
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats);
}

// Bumps `stats.exit` whenever the current node matches any of the
// supplied per-language token variants. Mirrors the `js_cognitive!` /
// `impl_cyclomatic_c_family!` shape used elsewhere in `src/metrics/`.
macro_rules! impl_exit_match_kinds {
    ($code:ty, $lang:ident, [$($kind:ident),+ $(,)?]) => {
        impl Exit for $code {
            fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
                if matches!(node.kind_id().into(), $($lang::$kind)|+) {
                    stats.exit += 1;
                }
            }
        }
    };
}

// `Python::Yield` is the yield-expression node (kind text "yield"); Python
// has no dedicated yield-statement variant. Counting it as an exit mirrors
// `CsharpCode` / `PhpCode`: generator suspension hands control back to the
// caller, so the function does leave even though it may later resume.
impl_exit_match_kinds!(PythonCode, Python, [ReturnStatement, RaiseStatement, Yield]);
// JS-family generators: `yield` / `yield*` parse as `YieldExpression`.
// Counted for the same reason as Python — see comment above.
impl_exit_match_kinds!(
    MozjsCode,
    Mozjs,
    [ReturnStatement, ThrowStatement, YieldExpression]
);
impl_exit_match_kinds!(
    JavascriptCode,
    Javascript,
    [ReturnStatement, ThrowStatement, YieldExpression]
);
impl_exit_match_kinds!(
    TypescriptCode,
    Typescript,
    [ReturnStatement, ThrowStatement, YieldExpression]
);
impl_exit_match_kinds!(
    TsxCode,
    Tsx,
    [ReturnStatement, ThrowStatement, YieldExpression]
);
impl_exit_match_kinds!(CppCode, Cpp, [ReturnStatement, ThrowStatement]);
impl_exit_match_kinds!(JavaCode, Java, [ReturnStatement, ThrowStatement]);
// Groovy's `yield` is the Java-14+ switch-expression yield, identical
// to Java's. Implicit-return-from-closure is NOT counted as an exit
// (consistent with Java) — only explicit return / throw / yield count.
impl_exit_match_kinds!(
    GroovyCode,
    Groovy,
    [ReturnStatement, ThrowStatement, YieldStatement]
);

impl Exit for RustCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        // Count only explicit `return` and `?` (TryExpression). The
        // implicit final-expression path is NOT an exit — peer-language
        // impls have the same convention. See #243 for the prior bug
        // that added a spurious +1 for every function with a return
        // type.
        if matches!(
            node.kind_id().into(),
            Rust::ReturnExpression | Rust::TryExpression
        ) {
            stats.exit += 1;
        }
    }
}

impl Exit for CsharpCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(
            node.kind_id().into(),
            Csharp::ReturnStatement
                | Csharp::YieldStatement
                | Csharp::ThrowStatement
                | Csharp::ThrowExpression
        ) {
            stats.exit += 1;
        }
    }
}

impl Exit for GoCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Go::ReturnStatement) {
            stats.exit += 1;
        }
    }
}

impl Exit for PerlCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if node.kind_id() == Perl::ReturnExpression {
            stats.exit += 1;
        }
    }
}

impl Exit for KotlinCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(
            node.kind_id().into(),
            Kotlin::ReturnExpression | Kotlin::ThrowExpression
        ) {
            stats.exit += 1;
        }
    }
}

impl Exit for LuaCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if node.kind_id() == Lua::ReturnStatement {
            stats.exit += 1;
        }
    }
}

impl Exit for BashCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        // Bash has no `return_statement` node: `return` and `exit` are
        // ordinary builtins parsed as `Bash::Command` whose `name` field
        // points at a `Bash::CommandName`. Identify them by comparing the
        // command-name text against the literal builtins.
        if matches!(node.kind_id().into(), Bash::Command)
            && let Some(name) = node.child_by_field_name("name")
            && matches!(name.utf8_text(code), Some("return" | "exit"))
        {
            stats.exit += 1;
        }
    }
}

impl Exit for TclCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        // Tcl has no return keyword node; `return` is a generic Command whose
        // name field is a simple_word with text "return".
        if node.kind_id() == Tcl::Command
            && let Some(name) = node.child_by_field_name("name")
            && name.kind_id() == Tcl::SimpleWord
            && name.utf8_text(code) == Some("return")
        {
            stats.exit += 1;
        }
    }
}

impl Exit for PhpCode {
    // tree-sitter-php 0.24.2's `exit_statement` rule covers `exit` only
    // (with or without parentheses); `die(...)` is grammar-classified as
    // a `function_call_expression` and therefore is NOT counted here.
    // Detecting `die` would require inspecting call-expression callee
    // text — brittle and likely to false-match user-defined `die`
    // functions. Modern PHP idiom favors `throw new Exception()` over
    // `die`, so leaving this asymmetric is acceptable.
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(
            node.kind_id().into(),
            Php::ReturnStatement | Php::YieldExpression | Php::ThrowExpression | Php::ExitStatement
        ) {
            stats.exit += 1;
        }
    }
}

// Real defaults — no functions to return from. Audited in #188.
implement_metric_trait!(Exit, PreprocCode, CcommentCode);

impl Exit for RubyCode {
    // Ruby's `return` is the only dedicated grammar node for an
    // intra-function exit. `yield` passes control to the block but does
    // not exit the enclosing method; `raise`/`exit` are ordinary method
    // calls without grammar nodes. tree-sitter-ruby exposes the
    // `return_statement` rule under two aliased visible kinds
    // (`Return`, `Return2`); the `Return3` token is the bare `return`
    // keyword inside those nodes and is not counted on its own.
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Ruby::Return | Ruby::Return2) {
            stats.exit += 1;
        }
    }
}

impl Exit for ElixirCode {
    // Elixir has no `return` statement: the last expression in a function
    // body is the return value. Early-exit happens through `throw`,
    // `raise`, `reraise`, or `exit`, all of which surface as `Call`
    // nodes whose target is an `Identifier` whose text spells the
    // keyword. Mirrors the Bash/Tcl pattern of comparing target text.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        if node.kind_id() == Elixir::Call
            && let Some(target) = node.child_by_field_name("target")
            && target.kind_id() == Elixir::Identifier
            && matches!(
                target.utf8_text(code),
                Some("throw" | "raise" | "reraise" | "exit")
            )
        {
            stats.exit += 1;
        }
    }
}

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

    /// A `Stats::default()` that never sees an
    /// observation must not leak the `usize::MAX` sentinel for
    /// `exit_min`. The getter collapses the sentinel to `0.0` so
    /// JSON never emits `1.8446744e19`.
    #[test]
    fn exit_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.exit_min(), 0.0);
    }

    #[test]
    fn python_no_exit() {
        check_metrics::<PythonParser>("a = 42", "foo.py", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn rust_no_exit() {
        check_metrics::<RustParser>("let a = 42;", "foo.rs", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn rust_question_mark() {
        check_metrics::<RustParser>("let _ = a? + b? + c?;", "foo.rs", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                    {
                      "sum": 3.0,
                      "average": null,
                      "min": 3.0,
                      "max": 3.0
                    }"###
            );
        });
    }

    // Regression for #243: `Exit for RustCode` used to add 1 whenever
    // a function_item with an explicit `-> T` was visited. Because the
    // spaces traversal pushes a new State *before* Exit::compute runs
    // for that function_item, every Rust function with an explicit
    // return type was getting one extra exit on top of its real
    // `return` / `?` exits. The fix drops the spurious clause; this
    // test pins exit == 1 for a function with one explicit return.
    #[test]
    fn rust_explicit_return_with_return_type() {
        check_metrics::<RustParser>("fn foo() -> i32 { return 1; }", "foo.rs", |metric| {
            // 1 explicit return / 1 space
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
            );
        });
    }

    // Regression for #243: an implicit final-expression return must
    // NOT count as an exit — matching every other language's
    // convention (Java, C++, Go, etc. don't count implicit returns).
    #[test]
    fn rust_implicit_return_not_counted() {
        check_metrics::<RustParser>("fn foo() -> i32 { 0 }", "foo.rs", |metric| {
            // 0 explicit exits / 1 space
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                {
                  "sum": 0.0,
                  "average": 0.0,
                  "min": 0.0,
                  "max": 0.0
                }"###
            );
        });
    }

    // Regression for #243: a function with both an explicit return on
    // one branch and an implicit final expression should count only
    // the explicit return.
    #[test]
    fn rust_mixed_explicit_and_implicit_return() {
        check_metrics::<RustParser>(
            "fn foo(x: bool) -> i32 { if x { return 1; } 0 }",
            "foo.rs",
            |metric| {
                // 1 explicit return; the implicit `0` is not an exit
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    // Regression for #243: `?` inside a function body is the only
    // implicit-exit form that does count, and the function having an
    // explicit `Result` return type must not double it.
    #[test]
    fn rust_question_mark_in_function() {
        check_metrics::<RustParser>(
            "fn foo() -> Result<i32, ()> { Ok(do_thing()?) }",
            "foo.rs",
            |metric| {
                // 1 `?` operator, no explicit `return`
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    // Regression for #243: a unit-returning function with no
    // explicit `return` or `?` must report 0 exits.
    #[test]
    fn rust_unit_return_no_exit() {
        check_metrics::<RustParser>("fn foo() { let _x = 1; }", "foo.rs", |metric| {
            // 0 exits / 1 space
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                {
                  "sum": 0.0,
                  "average": 0.0,
                  "min": 0.0,
                  "max": 0.0
                }"###
            );
        });
    }

    #[test]
    fn c_no_exit() {
        check_metrics::<CppParser>("int a = 42;", "foo.c", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    /// Multiple `return` statements across `if` / `else` branches.  Every
    /// `Cpp::ReturnStatement` adds +1 — there is no early-out collapse.
    #[test]
    fn c_multiple_returns_in_branches() {
        check_metrics::<CppParser>(
            "int f(int x) {
                 if (x < 0) {
                     return -1;
                 } else if (x == 0) {
                     return 0;
                 } else {
                     return 1;
                 }
             }",
            "foo.c",
            |metric| {
                // 1 function, 3 returns
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                assert_eq!(metric.nexits.exit_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    /// `return` statements inside `try` and `catch` blocks both count;
    /// the impl matches `Cpp::ReturnStatement` regardless of enclosing
    /// scope.  C++-only: bare C has no `try`/`catch`.
    #[test]
    fn cpp_return_in_try_catch() {
        check_metrics::<CppParser>(
            "int f(int x) {
                 try {
                     if (x == 0) {
                         return 1;
                     }
                     return 2;
                 } catch (...) {
                     return -1;
                 }
             }",
            "foo.cpp",
            |metric| {
                // 1 function, 3 returns (2 in try, 1 in catch); no
                // `throw` here, so the return-only path stays at 3.
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                assert_eq!(metric.nexits.exit_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    /// Early `return` inside a loop body is counted separately from the
    /// trailing return — every reachable `return` is an exit.
    #[test]
    fn c_early_return_in_loop() {
        check_metrics::<CppParser>(
            "int find(int* a, int n, int target) {
                 for (int i = 0; i < n; ++i) {
                     if (a[i] == target) {
                         return i;
                     }
                 }
                 return -1;
             }",
            "foo.c",
            |metric| {
                // 1 function, 2 returns
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                assert_eq!(metric.nexits.exit_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    /// `void` function with no explicit `return` — exit count is 0.
    /// The implicit fall-through return is intentionally not modelled.
    #[test]
    fn c_void_no_explicit_return() {
        check_metrics::<CppParser>(
            "void greet(const char* who) {
                 printf(\"hi %s\\n\", who);
             }",
            "foo.c",
            |metric| {
                // 1 function with zero ReturnStatement nodes.
                assert_eq!(metric.nexits.exit_sum(), 0.0);
                assert_eq!(metric.nexits.exit_max(), 0.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 0.0,
                      "average": 0.0,
                      "min": 0.0,
                      "max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_no_exit() {
        check_metrics::<JavascriptParser>("var a = 42;", "foo.js", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn javascript_simple_function() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 if (a) {
                     return a;
                 }
                 return b;
             }",
            "foo.js",
            |metric| {
                // 1 function with 2 return statements
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_nested_functions() {
        check_metrics::<JavascriptParser>(
            "function outer() {
                 function inner() {
                     return 1;
                 }
                 return inner();
             }",
            "foo.js",
            |metric| {
                // 2 functions, each with 1 return
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_simple_function() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 if a:
                     return a",
            "foo.py",
            |metric| {
                // 1 function
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_more_functions() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 if a:
                     return a
            def f(a, b):
                 if b:
                     return b",
            "foo.py",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_nested_functions() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 def foo(a):
                     if a:
                         return 1
                 bar = lambda a: lambda b: b or True or True
                 return bar(foo(a))(a)",
            "foo.py",
            |metric| {
                // 2 functions + 2 lambdas = 4
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 0.5,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_no_exit() {
        check_metrics::<JavaParser>("int a = 42;", "foo.java", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn java_simple_function() {
        check_metrics::<JavaParser>(
            "class A {
              public int sum(int x, int y) {
                return x + y;
              }
            }",
            "foo.java",
            |metric| {
                // 1 exit / 1 space
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_no_return() {
        check_metrics::<GoParser>(
            "package main
            func f() {
                x := 1
                _ = x
            }",
            "foo.go",
            |metric| {
                // No return_statement → exit_sum = 0.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 0.0,
                      "average": 0.0,
                      "min": 0.0,
                      "max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_single_return() {
        check_metrics::<GoParser>(
            "package main
            func f() int {
                return 1
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_multiple_returns() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) int {
                if x > 0 {
                    return 1
                }
                if x < 0 {
                    return -1
                }
                return 0
            }",
            "foo.go",
            |metric| {
                // 3 distinct return_statements across branches.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_naked_return() {
        check_metrics::<GoParser>(
            "package main
            func f() (x int) {
                x = 1
                return
            }",
            "foo.go",
            |metric| {
                // Bare `return` with named results is still a return_statement.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_multivalue_return() {
        check_metrics::<GoParser>(
            "package main
            func f() (int, error) {
                return 0, nil
            }",
            "foo.go",
            |metric| {
                // `return a, b` is one return_statement (Go has no comma operator).
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_split_function() {
        check_metrics::<JavaParser>(
            "class A {
              public int multiply(int x, int y) {
                if(x == 0 || y == 0){
                    return 0;
                }
                return x * y;
              }
            }",
            "foo.java",
            |metric| {
                // 2 exit / space 1
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_no_exit() {
        check_metrics::<CsharpParser>("int a = 42;", "foo.cs", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                {
                  "sum": 0.0,
                  "average": null,
                  "min": 0.0,
                  "max": 0.0
                }"###
            );
        });
    }

    #[test]
    fn csharp_simple_function() {
        check_metrics::<CsharpParser>(
            "class A {
              public int Sum(int x, int y) {
                return x + y;
              }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_split_function() {
        check_metrics::<CsharpParser>(
            "class A {
              public int Multiply(int x, int y) {
                if (x == 0 || y == 0) {
                    return 0;
                }
                return x * y;
              }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_yield_and_throw() {
        check_metrics::<CsharpParser>(
            "class A {
              public IEnumerable<int> Gen() {
                yield return 1;
                yield break;
              }
              public int Bad(int x) {
                if (x < 0) throw new System.Exception();
                return x;
              }
            }",
            "foo.cs",
            |metric| {
                // 2 yields + 1 throw + 1 return = 4 across two methods.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_no_exit() {
        check_metrics::<PerlParser>(
            "sub f {
                print 'hi';
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 0.0,
                  "average": 0.0,
                  "min": 0.0,
                  "max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_no_function_no_exit() {
        check_metrics::<PerlParser>("my $x = 1;\nprint $x;\n", "foo.pl", |metric| {
            insta::assert_json_snapshot!(metric.nexits, @r#"
            {
              "sum": 0.0,
              "average": null,
              "min": 0.0,
              "max": 0.0
            }
            "#);
        });
    }

    #[test]
    fn perl_multiple_returns() {
        check_metrics::<PerlParser>(
            "sub f {
                return 1 if $_[0];
                return 0;
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                 "#
                );
            },
        );
    }

    #[test]
    fn tsx_function_with_returns() {
        check_metrics::<TsxParser>(
            "function clamp(val: number, min: number, max: number) {
                 if (val < min) {
                     return min;
                 }
                 if (val > max) {
                     return max;
                 }
                 return val;
             }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_no_exit() {
        check_metrics::<TypescriptParser>("const x: number = 42;", "foo.ts", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn typescript_function_with_returns() {
        check_metrics::<TypescriptParser>(
            "function safeDivide(a: number, b: number): number | null {
                 if (b === 0) {
                     return null;
                 }
                 return a / b;
             }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_no_exit() {
        check_metrics::<MozjsParser>("var a = 42;", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn mozjs_function_with_returns() {
        check_metrics::<MozjsParser>(
            "function f(a, b) {
                 if (a) {
                     return a;
                 }
                 return b;
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn kotlin_exit_return_and_throw() {
        check_metrics::<KotlinParser>(
            "fun divide(a: Int, b: Int): Int {
                if (b == 0) {
                    throw IllegalArgumentException(\"zero\")
                }
                return a / b
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn lua_no_exit() {
        check_metrics::<LuaParser>(
            "local function f(x)
  local y = x + 1
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 0.0,
                      "average": 0.0,
                      "min": 0.0,
                      "max": 0.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn lua_return() {
        check_metrics::<LuaParser>(
            "local function f(x)
  if x > 0 then
    return x
  end
  return 0
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn bash_no_exit() {
        check_metrics::<BashParser>("echo \"no exits\"", "foo.sh", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                {
                  "sum": 0.0,
                  "average": null,
                  "min": 0.0,
                  "max": 0.0
                }"###
            );
        });
    }

    #[test]
    fn bash_explicit_return() {
        check_metrics::<BashParser>(
            "f() {
                 if [ -z \"$1\" ]; then
                     return 1
                 fi
                 echo ok
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_explicit_exit() {
        check_metrics::<BashParser>(
            "f() {
                 exit 0
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_multiple_exits() {
        check_metrics::<BashParser>(
            "f() {
                 if [ \"$1\" = die ]; then
                     exit 1
                 fi
                 return 0
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_returnish_names_are_not_exits() {
        // `returncode=1` is a `variable_assignment`, not a Command. The
        // function `returns` is invoked via a Command whose CommandName is
        // the literal "returns" — it must NOT be matched as a return/exit
        // builtin (whole-token match, no prefix collision).
        check_metrics::<BashParser>(
            "returncode=1
             returns() {
                 echo named
             }
             returns",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 0.0,
                      "average": 0.0,
                      "min": 0.0,
                      "max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn tcl_no_exit() {
        check_metrics::<TclParser>(
            "proc f {x} {
    puts $x
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                    {
                      "sum": 0.0,
                      "average": 0.0,
                      "min": 0.0,
                      "max": 0.0
                    }
                    "#
                );
            },
        );
    }

    #[test]
    fn tcl_return() {
        check_metrics::<TclParser>(
            "proc f {x} {
    return $x
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 1.0);
                assert_eq!(metric.nexits.exit_max(), 1.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn tcl_multiple_returns() {
        check_metrics::<TclParser>(
            "proc f {x} {
    if {$x > 0} {
        return positive
    }
    return nonpositive
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                assert_eq!(metric.nexits.exit_max(), 2.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn typescript_multiple_returns() {
        check_metrics::<TypescriptParser>(
            "function classify(n: number): string {
             if (n > 0) {
                 return 'positive';
             } else if (n < 0) {
                 return 'negative';
             }
             return 'zero';
         }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                assert_eq!(metric.nexits.exit_max(), 3.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn typescript_nested_functions() {
        check_metrics::<TypescriptParser>(
            "function outer(): number {
             function inner(): number {
                 return 42;
             }
             return inner();
         }",
            "foo.ts",
            |metric| {
                // outer has 1 return, inner has 1 return → sum=2, max=1
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                assert_eq!(metric.nexits.exit_max(), 1.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn tsx_no_exit() {
        check_metrics::<TsxParser>(
            "function f(): void {
             console.log('hello');
         }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 0.0);
                assert_eq!(metric.nexits.exit_max(), 0.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn tsx_multiple_returns() {
        check_metrics::<TsxParser>(
            "function classify(n: number): string {
             if (n > 0) {
                 return 'positive';
             } else if (n < 0) {
                 return 'negative';
             }
             return 'zero';
         }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                assert_eq!(metric.nexits.exit_max(), 3.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn kotlin_multiple_returns() {
        check_metrics::<KotlinParser>(
            "fun classify(n: Int): String {
             if (n > 0) {
                 return \"positive\"
             } else if (n < 0) {
                 return \"negative\"
             }
             return \"zero\"
         }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                assert_eq!(metric.nexits.exit_max(), 3.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn kotlin_no_exit() {
        check_metrics::<KotlinParser>(
            "fun f(): Unit {
             println(\"hello\")
         }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 0.0);
                assert_eq!(metric.nexits.exit_max(), 0.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn mozjs_nested_functions() {
        check_metrics::<MozjsParser>(
            "function outer() {
             function inner() {
                 return 42;
             }
             return inner();
         }",
            "foo.js",
            |metric| {
                // outer has 1 return, inner has 1 return → sum=2, max=1
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                assert_eq!(metric.nexits.exit_max(), 1.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn php_no_exit() {
        check_metrics::<PhpParser>("<?php $a = 42;", "foo.php", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r###"
                {
                  "sum": 0.0,
                  "average": null,
                  "min": 0.0,
                  "max": 0.0
                }"###
            );
        });
    }

    #[test]
    fn php_yield_throw() {
        // Generator yields and a throw expression in statement position both
        // count as exits.
        check_metrics::<PhpParser>(
            "<?php
            function gen() {
                yield 1;
                yield 2;
                throw new \\Exception('x');
            }",
            "foo.php",
            |metric| {
                // 3 exits (2 yields + 1 throw) inside one function space.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_exit_statement() {
        // `exit_statement` covers both `exit;` (bare) and `exit(N);` (with
        // optional argument). `die` is NOT in the `exit_statement` rule of
        // tree-sitter-php 0.24.2 — `die(...)` parses as a function call —
        // so we only count `exit` here.
        check_metrics::<PhpParser>(
            "<?php
            function bail(int $code): void {
                if ($code === 1) {
                    exit(1);
                }
                exit;
            }",
            "foo.php",
            |metric| {
                // 2 exit_statements inside one function space.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn elixir_no_exit() {
        // Plain function returning a value has no early-exit calls. The
        // `average` is `null` because Elixir's only function space is
        // the Unit; there is no per-function aggregation to average
        // over.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def add(a, b) do\n    a + b\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 0.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 0.0,
                  "average": null,
                  "min": 0.0,
                  "max": 0.0
                }"###
                );
            },
        );
    }

    #[test]
    fn elixir_raise_throw_exit() {
        // `raise`/`throw`/`exit` are recognised by inspecting the `target`
        // field text of `Call` nodes — there is no dedicated AST kind.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def bad(x) do\n    raise \"first\"\n    throw(:second)\n    exit(:third)\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": null,
                  "min": 3.0,
                  "max": 3.0
                }"###
                );
            },
        );
    }

    #[test]
    fn elixir_reraise_counts() {
        // `reraise` is the Elixir variant of `raise` that re-throws an
        // existing exception while preserving the stacktrace; we count
        // it as an exit alongside `raise`.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def wrap(stack) do\n    reraise(\"oops\", stack)\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 1.0);
            },
        );
    }

    #[test]
    fn elixir_lookalike_call_is_not_exit() {
        // Only the exact identifiers `throw`/`raise`/`reraise`/`exit` are
        // exits; a user-defined `throw_event` or remote-call must NOT
        // count. This guards against future text-match regressions.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    throw_event(:click)\n    Logger.raise_alert()\n    exit_code = 0\n    exit_code\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 0.0);
            },
        );
    }

    #[test]
    fn ruby_no_exit() {
        // Function body without any `return` produces zero exits.
        check_metrics::<RubyParser>("def foo\n  a = 1\n  a + 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.nexits.exit_sum(), 0.0);
        });
    }

    #[test]
    fn ruby_multiple_returns() {
        // Four explicit `return` statements (no modifier sugar) — one
        // per branch. Anchors the headline sum.
        check_metrics::<RubyParser>(
            "def kind(x)\n  return :zero if x == 0\n  if x > 0\n    return :pos\n  elsif x < 0\n    return :neg\n  end\n  return :unknown\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 4.0);
            },
        );
    }

    #[test]
    fn ruby_explicit_returns() {
        // Each `return` (statement or modifier-wrapped) contributes one
        // exit. `yield` is intentionally NOT counted (it does not exit
        // the method).
        check_metrics::<RubyParser>(
            "def foo(x)\n  return 0 if x.nil?\n  yield x\n  return x * 2\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn python_return_and_raise() {
        // `raise` exits the function (stack unwinds)
        // just like `return`. Mirrors the C# / Kotlin / PHP / Elixir
        // behaviour. One `raise` + one `return` => 2 exits.
        check_metrics::<PythonParser>(
            "def parse(s):
                 if not s:
                     raise ValueError(\"empty\")
                 return int(s)",
            "foo.py",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn javascript_return_and_throw() {
        // `throw` is a function exit.
        check_metrics::<JavascriptParser>(
            "function parseLength(s) {
                 if (s === null) throw new Error('null');
                 return s.length;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn mozjs_return_and_throw() {
        // Same shape as plain JavaScript.
        check_metrics::<MozjsParser>(
            "function parseLength(s) {
                 if (s === null) throw new Error('null');
                 return s.length;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn typescript_return_and_throw() {
        check_metrics::<TypescriptParser>(
            "function parseLength(s: string | null): number {
                 if (s === null) throw new Error('null');
                 return s.length;
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn tsx_return_and_throw() {
        check_metrics::<TsxParser>(
            "function parseLength(s: string | null): number {
                 if (s === null) throw new Error('null');
                 return s.length;
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn java_return_and_throw() {
        // `throw` exits the method.
        check_metrics::<JavaParser>(
            "class A {
                 int parseLength(String s) {
                     if (s == null) throw new NullPointerException();
                     return s.length();
                 }
             }",
            "foo.java",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn groovy_no_exit() {
        // No functions at all — `nexits.sum` is 0.
        check_metrics::<GroovyParser>("int a = 42", "foo.groovy", |metric| {
            assert_eq!(metric.nexits.exit_sum(), 0.0);
        });
    }

    #[test]
    fn groovy_simple_function() {
        // One explicit return in a top-level function.
        check_metrics::<GroovyParser>(
            "int answer() {
                return 42
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_return_and_throw() {
        check_metrics::<GroovyParser>(
            "class A {
                int parseLength(String s) {
                    if (s == null) throw new NullPointerException()
                    return s.length()
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
            },
        );
    }

    #[test]
    fn groovy_yield_in_switch_expression() {
        // Groovy inherits Java-14+ switch-expression `yield`. Each
        // explicit `yield` counts as one exit.
        check_metrics::<GroovyParser>(
            "class A {
                int describe(int n) {
                    return switch (n) {
                        case 0: yield 100;
                        default: yield 200;
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_implicit_return_not_counted() {
        // Groovy allows implicit return of the last expression in a
        // closure / function body. The Exit metric only counts
        // *explicit* `return` / `yield` / `throw` — consistent with
        // Java's docstring.
        check_metrics::<GroovyParser>("int identity(int x) { x }", "foo.groovy", |metric| {
            assert_eq!(metric.nexits.exit_sum(), 0.0);
        });
    }

    #[test]
    fn cpp_return_and_throw() {
        // `throw` exits the function.
        check_metrics::<CppParser>(
            "int parseLength(const char* s) {
                 if (s == nullptr) throw std::invalid_argument(\"null\");
                 return 0;
             }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn python_yield_counts_as_exit() {
        // Generator suspension via `yield` hands control back to the
        // caller — the function does leave its frame, just resumably.
        // Mirrors the long-standing C# / PHP behaviour. Two yields plus
        // one return == 3 exits inside the one generator function.
        check_metrics::<PythonParser>(
            "def gen():
                 yield 1
                 yield 2
                 return",
            "foo.py",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn javascript_yield_counts_as_exit() {
        // `function*` generator: each `yield` is an exit edge, same as
        // Python/C#/PHP. Two yields + one return == 3.
        check_metrics::<JavascriptParser>(
            "function* gen() {
                 yield 1;
                 yield 2;
                 return;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn mozjs_yield_counts_as_exit() {
        // Same shape as plain JavaScript.
        check_metrics::<MozjsParser>(
            "function* gen() {
                 yield 1;
                 yield 2;
                 return;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn typescript_yield_counts_as_exit() {
        check_metrics::<TypescriptParser>(
            "function* gen(): Generator<number> {
                 yield 1;
                 yield 2;
                 return;
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn tsx_yield_counts_as_exit() {
        check_metrics::<TsxParser>(
            "function* gen(): Generator<number> {
                 yield 1;
                 yield 2;
                 return;
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn python_yield_forms_count_as_exit() {
        // tree-sitter-python emits a single `Python::Yield` node kind for
        // every yield form: bare `yield`, `yield value`, and `yield from
        // iter`. The match arm therefore covers all three with no extra
        // variants needed. Three yield forms == 3 exits.
        check_metrics::<PythonParser>(
            "def gen():
                 yield
                 yield 1
                 yield from range(3)",
            "foo.py",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn javascript_yield_delegate_counts_as_exit() {
        // Delegating yield (`yield*`) parses as the same
        // `Javascript::YieldExpression` node as plain `yield`, so the
        // existing match arm covers it. Two regular yields + one
        // delegate == 3 exits.
        check_metrics::<JavascriptParser>(
            "function* gen() {
                 yield 1;
                 yield* other();
                 yield 2;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn mozjs_yield_delegate_counts_as_exit() {
        check_metrics::<MozjsParser>(
            "function* gen() {
                 yield 1;
                 yield* other();
                 yield 2;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn typescript_yield_delegate_counts_as_exit() {
        check_metrics::<TypescriptParser>(
            "function* gen(): Generator<number> {
                 yield 1;
                 yield* other();
                 yield 2;
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn tsx_yield_delegate_counts_as_exit() {
        check_metrics::<TsxParser>(
            "function* gen(): Generator<number> {
                 yield 1;
                 yield* other();
                 yield 2;
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.nexits.exit_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }
}
