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
    pub fn exit(&self) -> f64 {
        self.exit as f64
    }
    /// Returns the `NExit` metric sum value
    pub fn exit_sum(&self) -> f64 {
        self.exit_sum as f64
    }
    /// Returns the `NExit` metric  minimum value
    pub fn exit_min(&self) -> f64 {
        self.exit_min as f64
    }
    /// Returns the `NExit` metric maximum value
    pub fn exit_max(&self) -> f64 {
        self.exit_max as f64
    }

    /// Returns the `NExit` metric average value
    ///
    /// This value is computed dividing the `NExit` value
    /// for the total number of functions/closures in a space.
    ///
    /// If there are no functions in a code, its value is `NAN`.
    pub fn exit_average(&self) -> f64 {
        self.exit_sum() / self.total_space_functions as f64
    }
    #[inline(always)]
    pub(crate) fn compute_sum(&mut self) {
        self.exit_sum += self.exit;
    }
    #[inline(always)]
    pub(crate) fn compute_minmax(&mut self) {
        self.exit_max = self.exit_max.max(self.exit);
        self.exit_min = self.exit_min.min(self.exit);
        self.compute_sum();
    }
    pub(crate) fn finalize(&mut self, total_space_functions: usize) {
        self.total_space_functions = total_space_functions;
    }
}

pub trait Exit
where
    Self: Checker,
{
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats);
}

impl Exit for PythonCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Python::ReturnStatement) {
            stats.exit += 1;
        }
    }
}

impl Exit for MozjsCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Mozjs::ReturnStatement) {
            stats.exit += 1;
        }
    }
}

impl Exit for JavascriptCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Javascript::ReturnStatement) {
            stats.exit += 1;
        }
    }
}

impl Exit for TypescriptCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Typescript::ReturnStatement) {
            stats.exit += 1;
        }
    }
}

impl Exit for TsxCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Tsx::ReturnStatement) {
            stats.exit += 1;
        }
    }
}

impl Exit for RustCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(
            node.kind_id().into(),
            Rust::ReturnExpression | Rust::TryExpression
        ) || Self::is_func(node) && node.child_by_field_name("return_type").is_some()
        {
            stats.exit += 1;
        }
    }
}

impl Exit for CppCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Cpp::ReturnStatement) {
            stats.exit += 1;
        }
    }
}

impl Exit for JavaCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        if matches!(node.kind_id().into(), Java::ReturnStatement) {
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

implement_metric_trait!(Exit, PreprocCode, CcommentCode);

#[cfg(test)]
mod tests {
    use crate::tools::check_metrics;

    use super::*;

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
}
