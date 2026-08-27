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

use std::fmt;

use crate::checker::Checker;
use crate::macros::implement_metric_trait;
use crate::*;

/// The `NExit` metric.
///
/// This metric counts the number of possible exit points
/// from a function/method.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
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

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "sum: {}, average: {} min: {}, max: {}",
            self.nexits_sum(),
            self.nexits_average(),
            self.nexits_min(),
            self.nexits_max()
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
    pub fn nexits(&self) -> u64 {
        self.exit as u64
    }
    /// Returns the `NExit` metric sum value
    #[must_use]
    pub fn nexits_sum(&self) -> u64 {
        self.exit_sum as u64
    }
    /// Returns the `NExit` metric minimum value.
    ///
    /// Collapses the `usize::MAX` sentinel that `Stats::default()` plants
    /// into `exit_min` to `0`, so a never-observed space
    /// serializes to a meaningful number rather than `1.8446744e19`.
    #[must_use]
    pub fn nexits_min(&self) -> u64 {
        if self.exit_min == usize::MAX {
            0
        } else {
            self.exit_min as u64
        }
    }
    /// Returns the `NExit` metric maximum value
    #[must_use]
    pub fn nexits_max(&self) -> u64 {
        self.exit_max as u64
    }

    /// Returns the `NExit` metric average value
    ///
    /// This value is computed dividing the `NExit` value
    /// for the total number of functions/closures in a space.
    ///
    /// The per-function divisor (shared with `cyclomatic`/`cognitive`/
    /// `nargs`, #512) is guarded with `.max(1)` via the shared `average`
    /// helper, so a space with no counted functions (or one where `Nom`
    /// was not selected) degrades to `sum / 1` instead of producing
    /// `inf`/`NaN` (#428).
    #[must_use]
    pub fn nexits_average(&self) -> f64 {
        crate::metrics::average(self.nexits_sum() as f64, self.total_space_functions)
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
pub(crate) trait Exit
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
impl_exit_match_kinds!(MozcppCode, Mozcpp, [ReturnStatement, ThrowStatement]);
// C has no exceptions: `return` is the only exit kind (no `throw`).
impl_exit_match_kinds!(CCode, C, [ReturnStatement]);
// Objective-C adds `@throw` on top of C's `return` (the `throw_statement`
// node), mirroring the C++ exit set.
impl_exit_match_kinds!(ObjcCode, Objc, [ReturnStatement, ThrowStatement]);
// Java's `yield` is the Java-14+ switch-expression yield statement
// (an unambiguous statement node, distinct from a labeled `break`).
// It hands the switch-expression value back as an explicit exit, so it
// counts identically to Groovy and C#. Implicit final-expression
// returns are not counted — only explicit return / throw / yield.
impl_exit_match_kinds!(
    JavaCode,
    Java,
    [ReturnStatement, ThrowStatement, YieldStatement]
);
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
    // Go has no dedicated `panic` node: `panic(...)` is the built-in
    // abrupt-exit call (it unwinds the stack like `throw`/`raise` in the
    // exception languages), parsed as a `call_expression` whose `function`
    // field is a bare `identifier` spelling `panic`. Count it as an exit
    // alongside `return`. Matching the bare identifier (not a
    // `selector_expression`) means a package-qualified call like
    // `foo.panic()` — a user function, not the built-in — is not counted,
    // mirroring how Bash matches the bare builtin command name.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Go::ReturnStatement) {
            stats.exit += 1;
        } else if node.kind_id() == Go::CallExpression
            && let Some(function) = node.child_by_field_name("function")
            && function.kind_id() == Go::Identifier
            && function.utf8_text(code) == Some("panic")
        {
            stats.exit += 1;
        }
    }
}

impl Exit for PerlCode {
    // Perl's abrupt-exit builtins are `die` (raises an exception that
    // unwinds to the nearest `eval`) and `exit` (terminates the
    // process). Neither has a dedicated grammar node: every call form
    // (`die;`, `die "m"`, `die("m")`, `... or die "m"`) nests a
    // `call_expression_with_bareword` holding just the callee name, so
    // matching that one kind counts each occurrence exactly once
    // whatever wrapper carries the arguments (#1270). Compare the
    // bareword text for the same reason Go matches `panic` and Lua
    // matches `error`.
    //
    // Deliberate exclusions:
    // - A package-qualified callee keeps its qualifier in the bareword
    //   text (`Carp::croak`), so it can never equal `die`/`exit`.
    // - `croak` / `confess` are Carp *library* functions, not builtins;
    //   an unqualified `croak "m"` is left uncounted rather than
    //   guessing that the module is loaded.
    // - `$obj->die` parses as a `method_invocation` whose callee is a
    //   plain `identifier`, never a bareword, so a user method named
    //   `die` is not counted.
    //
    // Known artifact: a fat-comma hash key auto-quotes its bareword in
    // Perl, but the grammar still emits `call_expression_with_bareword`
    // for it, so `(die => 1)` counts as an exit. Gating on the `=>`
    // that follows would need a forward sibling lookup from inside the
    // metric walk, which #1096 took out of these bodies; the same node
    // already counts as an ABC branch (`src/metrics/abc/perl.rs`), so
    // the artifact is accepted rather than paid for here.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        let is_abrupt_exit_builtin = node.kind_id() == Perl::CallExpressionWithBareword
            && matches!(node.utf8_text(code), Some("die" | "exit"));
        if node.kind_id() == Perl::ReturnExpression || is_abrupt_exit_builtin {
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
    // Lua has no `throw`/`raise` keyword: the abrupt-exit primitives are the
    // built-in `error(...)` (raises a Lua error that unwinds to the nearest
    // `pcall`) and `os.exit(...)` (terminates the process). Both parse as a
    // `function_call` whose `name` field is the callee. `error(...)` is a
    // bare `identifier`; `os.exit(...)` is a `dot_index_expression` with text
    // `os.exit`. Count them as exits alongside `return`. Matching the exact
    // callee text means a user call such as `foo()` or `myError()` is not
    // counted, mirroring how Bash/Elixir match the bare builtin name.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        if node.kind_id() == Lua::ReturnStatement {
            stats.exit += 1;
        } else if node.kind_id() == Lua::FunctionCall
            && let Some(name) = node.child_by_field_name("name")
            && matches!(name.utf8_text(code), Some("error" | "os.exit"))
        {
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
        // name field is a simple_word with text "return" — the same
        // leading-word resolution the Cognitive / Cyclomatic `switch` and
        // `for` detectors use, shared rather than restated here.
        //
        // `error` (raises an error that unwinds to the nearest `catch`)
        // and the Tcl 8.6 `throw` are the abrupt-exit builtins and parse
        // to the same generic Command shape — the vendored grammar has
        // no dedicated rule for either, so the leading word is the only
        // seam (#1270, lesson 19). Because `tcl_command_name` reads the
        // `name` field, `error` in argument position (`puts error`) is a
        // `word_list` child and is not counted.
        if matches!(
            crate::metrics::cognitive::tcl_command_name(node, code),
            Some("return" | "error" | "throw")
        ) {
            stats.exit += 1;
        }
    }
}

impl Exit for IrulesCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        // Like Tcl, iRules has no `return` keyword node — `return` is a
        // generic Command (it is not among the grammar's `_builtin`
        // commands). The bare name word can surface as either `simple_word`
        // or `concat_word` depending on context, so match on the name text
        // rather than a fixed kind. A multi-value `return $a $b` still has a
        // single `name` field and is counted once.
        //
        // `error` is the Tcl abrupt-exit builtin and reaches iRules
        // unchanged; re-derived against the iRules grammar rather than
        // assumed from Tcl's, it parses to the same `command` +
        // name-word shape (#1270). Tcl 8.6's `throw` is *not* matched
        // here: TMOS iRules runs a Tcl 8.4-derived interpreter that has
        // no `throw` builtin, so the word could only ever name a user
        // proc. iRules flow commands (`event disable`, `TCP::close`,
        // `reject`, `drop`) remain deliberately uncounted as exits in
        // v1.
        if node.kind_id() == Irules::Command
            && let Some(name) = node.child_by_field_name("name")
            && matches!(name.utf8_text(code), Some("return" | "error"))
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
    // intra-function exit; `yield` passes control to the block but does
    // not exit the enclosing method. tree-sitter-ruby exposes the
    // `return_statement` rule under two aliased visible kinds
    // (`Return`, `Return2`); the `Return3` token is the bare `return`
    // keyword inside those nodes and is not counted on its own.
    //
    // `raise` and `exit` are ordinary method calls with no grammar node
    // of their own — which is the shape the Go / Lua / Elixir impls
    // above already match by callee text, not a reason to leave them
    // uncounted (#1270). Both parse as a `call` whose `method` field is
    // a bare `identifier`; `Checker::is_call` carries the four visible
    // `call` aliases, so the arm cannot miss one by position.
    //
    // Deliberate exclusions:
    // - A call with a `receiver` (`obj.raise`, `self.exit`) is a user
    //   method, never the Kernel builtin — the same bare-callee gate Go
    //   uses to keep `foo.panic()` out.
    // - A *bare* `raise` / `exit` with no arguments parses as a plain
    //   `identifier`, indistinguishable from reading a local variable
    //   of that name, so the argument-less re-raise idiom inside
    //   `rescue` goes uncounted rather than blanket-matching every
    //   identifier.
    // - `fail` (a Kernel alias of `raise`), `abort`, and `throw`
    //   (catch/throw non-local flow) are not matched: each is a common
    //   user or test-DSL method name, and the counted set stays the two
    //   primitives the cross-language exit table names.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Ruby::Return | Ruby::Return2) {
            stats.exit += 1;
        } else if Self::is_call(node)
            && node.child_by_field_name("receiver").is_none()
            && let Some(method) = node.child_by_field_name("method")
            && method.kind_id() == Ruby::Identifier
            && matches!(method.utf8_text(code), Some("raise" | "exit"))
        {
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
    use crate::test_support::{
        check_func_space_only_shim, check_metrics_only_shim, child_space, function_space,
    };

    use super::*;

    // Nexits pulls Nom for its per-function average divisor, which is
    // also what this module's one `metric.nom.functions_sum()`
    // assertion reads.
    check_metrics_only_shim!(check_metrics, Nexits);
    check_func_space_only_shim!(check_func_space, Nexits);

    /// A `Stats::default()` that never sees an
    /// observation must not leak the `usize::MAX` sentinel for
    /// `exit_min`. The getter collapses the sentinel to `0.0` so
    /// JSON never emits `1.8446744e19`.
    #[test]
    fn exit_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.nexits_min(), 0);
    }

    #[test]
    fn python_no_exit() {
        check_metrics::<PythonParser>("a = 42", "foo.py", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn rust_no_exit() {
        check_metrics::<RustParser>("let a = 42;", "foo.rs", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn rust_question_mark() {
        check_metrics::<RustParser>("let _ = a? + b? + c?;", "foo.rs", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r#"
            {
              "sum": 3,
              "average": 3.0,
              "min": 3,
              "max": 3
            }
            "#
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
                @r#"
            {
              "sum": 1,
              "average": 1.0,
              "min": 0,
              "max": 1
            }
            "#
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
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn c_no_exit() {
        check_metrics::<CParser>("int a = 42;", "foo.c", |metric| {
            // 0 functions
            insta::assert_json_snapshot!(
                metric.nexits,
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    /// Multiple `return` statements across `if` / `else` branches.  Every
    /// `Cpp::ReturnStatement` adds +1 — there is no early-out collapse.
    #[test]
    fn c_multiple_returns_in_branches() {
        check_metrics::<CParser>(
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                assert_eq!(metric.nexits.nexits_max(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    /// The raison d'être of `LANG::C` (#721): C code that uses C++
    /// keywords (`new`, `class`, `delete`) as plain identifiers parses
    /// cleanly through `tree-sitter-c`, where the C++ grammar
    /// ERROR-cascades. The load-bearing assertion is `!root.has_error()`:
    /// the C++ grammar errors on this input yet *still* recovers a
    /// function node and two `return`s, so a metric-count assertion alone
    /// does not distinguish the two grammars — only the error-free parse
    /// does. C has no `throw`, so `return` is the sole exit kind.
    #[test]
    fn c_keyword_identifiers_parse_and_returns_count() {
        use std::path::PathBuf;

        let source = "int process(int new, int class) {
                 int delete = new + class;
                 if (delete > 0) {
                     return delete;
                 }
                 return 0;
             }";
        let parser = CParser::new(source.as_bytes().to_vec(), &PathBuf::from("foo.c"), None);
        assert!(
            !parser.root().has_error(),
            "C grammar must parse C++-keyword identifiers without an error cascade"
        );

        check_metrics::<CParser>(source, "foo.c", |metric| {
            assert_eq!(metric.nom.functions_sum(), 1);
            assert_eq!(metric.nexits.nexits_sum(), 2);
        });
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                assert_eq!(metric.nexits.nexits_max(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    /// Early `return` inside a loop body is counted separately from the
    /// trailing return — every reachable `return` is an exit.
    #[test]
    fn c_early_return_in_loop() {
        check_metrics::<CParser>(
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                assert_eq!(metric.nexits.nexits_max(), 2);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    /// `void` function with no explicit `return` — exit count is 0.
    /// The implicit fall-through return is intentionally not modelled.
    #[test]
    fn c_void_no_explicit_return() {
        check_metrics::<CParser>(
            "void greet(const char* who) {
                 printf(\"hi %s\\n\", who);
             }",
            "foo.c",
            |metric| {
                // 1 function with zero ReturnStatement nodes.
                assert_eq!(metric.nexits.nexits_sum(), 0);
                assert_eq!(metric.nexits.nexits_max(), 0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#
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
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 0.5,
                  "min": 0,
                  "max": 1
                }
                "#
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
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_panic_counts_as_exit() {
        check_metrics::<GoParser>(
            "package main
            func f() {
                panic(\"boom\")
            }",
            "foo.go",
            |metric| {
                // panic(...) is the built-in abrupt-exit call, counted like
                // throw/raise — one exit even though there is no `return`.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_panic_and_return_both_count() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) int {
                if x < 0 {
                    panic(\"negative\")
                }
                return x
            }",
            "foo.go",
            |metric| {
                // panic(...) + return are both abrupt exits → 2.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_package_qualified_panic_is_not_exit() {
        check_metrics::<GoParser>(
            "package main
            func f() {
                foo.panic()
            }",
            "foo.go",
            |metric| {
                // `foo.panic()` is a user method on package `foo`, not the
                // built-in `panic` — its callee is a selector_expression, not
                // a bare identifier, so it must not be counted.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_no_exit() {
        check_metrics::<CsharpParser>("int a = 42;", "foo.cs", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                  "sum": 4,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
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
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
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
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
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
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    /// `die` and `exit` are Perl's abrupt-exit builtins (#1270). Every
    /// call form nests one `call_expression_with_bareword`, so the
    /// spaced-args (`die "m"`), bracketed (`exit(1)`) and bare
    /// (`die;`) spellings each count exactly once — a wrapper node is
    /// never counted alongside its bareword.
    #[test]
    fn perl_die_and_exit_are_exits() {
        check_metrics::<PerlParser>(
            "sub f {
                die \"bad\" if $_[0];
                open(my $fh, '<', $p) or die;
                exit(1) if $_[1];
                exit 2;
                return 0;
            }",
            "foo.pl",
            |metric| {
                // expected: 4 abrupt exits (two `die`, two `exit`) plus
                // one `return`.
                assert_eq!(metric.nexits.nexits_sum(), 5);
            },
        );
    }

    /// Only the unqualified builtins count. A package-qualified callee
    /// keeps its qualifier in the bareword text (`Carp::croak`), the
    /// Carp helpers are library functions rather than builtins, and
    /// `$obj->die` parses as a `method_invocation` whose callee is a
    /// plain `identifier` — none of them is an exit.
    #[test]
    fn perl_lookalike_call_is_not_exit() {
        check_metrics::<PerlParser>(
            "sub f {
                $obj->die;
                $obj->exit(1);
                Carp::croak(\"x\");
                croak \"x\";
                my $s = \"die\";
            }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 0);
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
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_no_exit() {
        check_metrics::<TypescriptParser>("const x: number = 42;", "foo.ts", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_no_exit() {
        check_metrics::<MozjsParser>("var a = 42;", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                    @r#"
                {
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_error_counts_as_exit() {
        check_metrics::<LuaParser>(
            "local function f(x)
  error(\"bad\")
end",
            "foo.lua",
            |metric| {
                // error(...) raises a Lua error that unwinds the stack — a
                // built-in abrupt exit, counted like throw/raise.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_os_exit_counts_as_exit() {
        check_metrics::<LuaParser>(
            "local function f()
  os.exit(1)
end",
            "foo.lua",
            |metric| {
                // os.exit(...) terminates the process — its callee is a
                // dot_index_expression spelling `os.exit`, counted as an exit.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_error_and_return_both_count() {
        check_metrics::<LuaParser>(
            "local function f(x)
  if x < 0 then
    error(\"negative\")
  end
  return x
end",
            "foo.lua",
            |metric| {
                // error(...) + return are both abrupt exits → 2.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_user_call_is_not_exit() {
        check_metrics::<LuaParser>(
            "local function f()
  foo()
  myError(\"x\")
end",
            "foo.lua",
            |metric| {
                // Neither `foo()` nor a user `myError(...)` is the built-in
                // `error`/`os.exit`, so neither is counted.
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_no_exit() {
        check_metrics::<BashParser>("echo \"no exits\"", "foo.sh", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 1,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                    @r#"
                {
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#
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
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
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
                assert_eq!(metric.nexits.nexits_sum(), 1);
                assert_eq!(metric.nexits.nexits_max(), 1);
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                assert_eq!(metric.nexits.nexits_max(), 2);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    /// Tcl's abrupt-exit builtins have no dedicated grammar rule: both
    /// `error` and the 8.6 `throw` parse as generic commands told apart
    /// by their leading word, the same seam `return` uses (#1270).
    #[test]
    fn tcl_error_and_throw_are_exits() {
        check_metrics::<TclParser>(
            "proc f {x} {
    if {$x < 0} {
        error \"negative\"
    }
    if {$x == 0} {
        throw {ARITH DIVZERO} \"div by zero\"
    }
    return $x
}",
            "foo.tcl",
            |metric| {
                // expected: `error` + `throw` + `return` = 3.
                assert_eq!(metric.nexits.nexits_sum(), 3);
            },
        );
    }

    /// The command *name* is the seam, so the same words in argument
    /// position (`puts error`) or inside a string are not exits. The
    /// leading word of a nested braced command (`{ARITH DIVZERO}`) is
    /// likewise a different command name and contributes nothing.
    #[test]
    fn tcl_error_in_argument_position_is_not_exit() {
        check_metrics::<TclParser>(
            "proc f {x} {
    puts error
    puts throw
    set y \"error\"
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 0);
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                assert_eq!(metric.nexits.nexits_max(), 3);
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                assert_eq!(metric.nexits.nexits_max(), 1);
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
                assert_eq!(metric.nexits.nexits_sum(), 0);
                assert_eq!(metric.nexits.nexits_max(), 0);
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                assert_eq!(metric.nexits.nexits_max(), 3);
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                assert_eq!(metric.nexits.nexits_max(), 3);
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
                assert_eq!(metric.nexits.nexits_sum(), 0);
                assert_eq!(metric.nexits.nexits_max(), 0);
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                assert_eq!(metric.nexits.nexits_max(), 1);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    #[test]
    fn php_no_exit() {
        check_metrics::<PhpParser>("<?php $a = 42;", "foo.php", |metric| {
            insta::assert_json_snapshot!(
                metric.nexits,
                @r#"
            {
              "sum": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
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
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 0);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 1);
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
                assert_eq!(metric.nexits.nexits_sum(), 0);
            },
        );
    }

    #[test]
    fn ruby_no_exit() {
        // Function body without any `return` produces zero exits.
        check_metrics::<RubyParser>("def foo\n  a = 1\n  a + 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.nexits.nexits_sum(), 0);
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
                assert_eq!(metric.nexits.nexits_sum(), 4);
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                insta::assert_json_snapshot!(metric.nexits);
            },
        );
    }

    /// `raise` and `exit` are receiver-less Kernel calls with no
    /// grammar node of their own, matched by callee text the way Go
    /// matches `panic` (#1270). Both the paren-less command form
    /// (`raise ArgumentError, "m"`) and the parenthesised form
    /// (`exit(1)`) parse as `call`, so both count.
    #[test]
    fn ruby_raise_and_exit_are_exits() {
        check_metrics::<RubyParser>(
            "def f(x)\n  raise ArgumentError, \"bad\" if x\n  raise(RuntimeError)\n  exit 1 if x\n  exit(2)\n  return 0\nend\n",
            "foo.rb",
            |metric| {
                // expected: two `raise` + two `exit` + one `return` = 5.
                assert_eq!(metric.nexits.nexits_sum(), 5);
            },
        );
    }

    /// A call with a receiver is a user method, never the Kernel
    /// builtin, so `obj.raise` / `self.exit` must not count — the same
    /// bare-callee gate Go uses to keep `foo.panic()` out. A symbol or
    /// hash key spelling the builtin parses as `simple_symbol` /
    /// `hash_key_symbol` and is likewise not a call.
    #[test]
    fn ruby_receiver_call_is_not_exit() {
        check_metrics::<RubyParser>(
            "def f(x)\n  obj.raise(x)\n  self.exit(1)\n  logger.raise\n  h = { raise: 1 }\n  s = :exit\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 0);
            },
        );
    }

    /// A *bare* `raise` / `exit` — the argument-less re-raise idiom —
    /// parses as a plain `identifier`, indistinguishable from reading a
    /// local variable of that name, so it is deliberately not counted.
    /// Pinning the exclusion here keeps a future "just match bare
    /// identifiers too" change from silently counting every variable
    /// read.
    #[test]
    fn ruby_bare_raise_identifier_is_not_exit() {
        check_metrics::<RubyParser>(
            "def f(x)\n  begin\n    g(x)\n  rescue StandardError\n    raise\n  end\n  return 0\nend\n",
            "foo.rb",
            |metric| {
                // expected: only the `return`; the bare `raise` is an
                // `identifier`, not a `call`.
                assert_eq!(metric.nexits.nexits_sum(), 1);
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    /// #1160 is an *attribution* bug, and `nexits` is where that shows
    /// most plainly: the file-level sum never moved, so only a per-space
    /// assertion can see it. The compact constructor's `throw` belonged
    /// to the enclosing `class R` because the constructor opened no space
    /// of its own.
    ///
    /// Both halves are asserted. `class R`'s own count must be 0 — the
    /// aggregate `nexits_sum` is 2 either way, so checking only the new
    /// space would pass against the unfixed code as long as the space
    /// existed at all.
    #[test]
    fn java_record_compact_constructor_owns_its_exits() {
        check_func_space::<JavaParser, _>(
            "record R(int a, int b) {
                 R {
                     if (a < 0) { throw new IllegalArgumentException(); }
                 }
                 int sum() { return a + b; }
             }",
            "R.java",
            |space| {
                assert_eq!(
                    space.metrics.nexits.nexits_sum(),
                    2,
                    "one throw, one return"
                );
                assert_eq!(
                    child_space(&space, "R").metrics.nexits.nexits(),
                    0,
                    "class R owns neither",
                );
                assert_eq!(
                    function_space(&space, "R").metrics.nexits.nexits(),
                    1,
                    "the compact constructor owns its throw",
                );
            },
        );
    }

    #[test]
    fn java_yield_in_switch_expression() {
        // Java-14+ switch-expression `yield` is an explicit exit. Each
        // `yield` counts as one, alongside the enclosing `return`.
        check_metrics::<JavaParser>(
            "class A {
                int describe(int n) {
                    return switch (n) {
                        case 0: yield 100;
                        default: yield 200;
                    };
                }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 3);
            },
        );
    }

    #[test]
    fn groovy_no_exit() {
        // No functions at all — `nexits.sum` is 0.
        check_metrics::<GroovyParser>("int a = 42", "foo.groovy", |metric| {
            assert_eq!(metric.nexits.nexits_sum(), 0);
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
                assert_eq!(metric.nexits.nexits_sum(), 1);
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
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
            assert_eq!(metric.nexits.nexits_sum(), 0);
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
                assert_eq!(metric.nexits.nexits_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
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
                assert_eq!(metric.nexits.nexits_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.nexits,
                    @r#"
                {
                  "sum": 3,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    /// A handler with no `return` has zero exits (iRules has no `return`
    /// keyword node; `return` is a generic command matched by name).
    #[test]
    fn irules_no_exit() {
        check_metrics::<IrulesParser>(
            "when HTTP_REQUEST {
    set x 1
    log local0. $x
}
",
            "foo.irule",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 0);
            },
        );
    }

    /// A `return` command contributes one exit.
    #[test]
    fn irules_return() {
        check_metrics::<IrulesParser>(
            "when HTTP_REQUEST {
    if { [HTTP::uri] eq \"/\" } {
        return
    }
    log local0. \"served\"
}
",
            "foo.irule",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 1);
            },
        );
    }

    /// A multi-value `return` (`return [list ...]`) is a single command and
    /// counts once, not once per returned value.
    #[test]
    fn irules_multi_value_return_counts_once() {
        check_metrics::<IrulesParser>(
            "proc pair { a b } {
    return [list $a $b]
}
",
            "foo.irule",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 1);
            },
        );
    }

    /// `error` is a plain Tcl builtin that reaches iRules unchanged and
    /// parses to the same `command` + name-word shape (#1270),
    /// re-derived against the iRules grammar rather than assumed from
    /// Tcl's.
    #[test]
    fn irules_error_is_an_exit() {
        check_metrics::<IrulesParser>(
            "proc f { x } {
    if { $x < 0 } {
        error \"negative\"
    }
    return $x
}
",
            "foo.irule",
            |metric| {
                // expected: `error` + `return` = 2.
                assert_eq!(metric.nexits.nexits_sum(), 2);
            },
        );
    }

    /// Tcl 8.6's `throw` is deliberately absent from the iRules exit
    /// set — TMOS runs a Tcl 8.4-derived interpreter with no such
    /// builtin, so the word can only ever name a user proc — and
    /// `error` in argument position is not an exit either.
    #[test]
    fn irules_throw_and_argument_position_error_are_not_exits() {
        check_metrics::<IrulesParser>(
            "proc f { x } {
    throw {ARITH DIVZERO} \"boom\"
    log local0. error
}
",
            "foo.irule",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 0);
            },
        );
    }

    /// Objective-C method with no `return` and no `@throw` has zero exit
    /// points.
    #[test]
    fn objc_no_exit() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)bar {
    [self doWork];
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 0);
                insta::assert_json_snapshot!(metric.nexits, @r#"
                {
                  "sum": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#);
            },
        );
    }

    /// Objective-C exit set is `return_statement` + `@throw`
    /// (`throw_statement`): a method with one of each counts 2.
    #[test]
    fn objc_return_and_throw() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (int)bar:(int)x {
    if (x < 0) {
        @throw [NSException exceptionWithName:@\"e\" reason:@\"r\" userInfo:nil];
    }
    return x;
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nexits.nexits_sum(), 2);
                insta::assert_json_snapshot!(metric.nexits, @r#"
                {
                  "sum": 2,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }
}
