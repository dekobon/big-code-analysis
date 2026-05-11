// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use crate::checker::Checker;
use crate::macros::implement_metric_trait;
use crate::*;

/// The `NArgs` metric.
///
/// This metric counts the number of arguments
/// of functions/closures.
#[derive(Debug, Clone)]
pub struct Stats {
    fn_nargs: usize,
    closure_nargs: usize,
    fn_nargs_sum: usize,
    closure_nargs_sum: usize,
    fn_nargs_min: usize,
    closure_nargs_min: usize,
    fn_nargs_max: usize,
    closure_nargs_max: usize,
    total_functions: usize,
    total_closures: usize,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            fn_nargs: 0,
            closure_nargs: 0,
            fn_nargs_sum: 0,
            closure_nargs_sum: 0,
            fn_nargs_min: usize::MAX,
            closure_nargs_min: usize::MAX,
            fn_nargs_max: 0,
            closure_nargs_max: 0,
            total_functions: 0,
            total_closures: 0,
        }
    }
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("nargs", 10)?;
        st.serialize_field("total_functions", &self.fn_args_sum())?;
        st.serialize_field("total_closures", &self.closure_args_sum())?;
        st.serialize_field("average_functions", &self.fn_args_average())?;
        st.serialize_field("average_closures", &self.closure_args_average())?;
        st.serialize_field("total", &self.nargs_total())?;
        st.serialize_field("average", &self.nargs_average())?;
        st.serialize_field("functions_min", &self.fn_args_min())?;
        st.serialize_field("functions_max", &self.fn_args_max())?;
        st.serialize_field("closures_min", &self.closure_args_min())?;
        st.serialize_field("closures_max", &self.closure_args_max())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "total_functions: {}, total_closures: {}, average_functions: {}, average_closures: {}, total: {}, average: {}, functions_min: {}, functions_max: {}, closures_min: {}, closures_max: {}",
            self.fn_args(),
            self.closure_args(),
            self.fn_args_average(),
            self.closure_args_average(),
            self.nargs_total(),
            self.nargs_average(),
            self.fn_args_min(),
            self.fn_args_max(),
            self.closure_args_min(),
            self.closure_args_max()
        )
    }
}

impl Stats {
    /// Merges a second `NArgs` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.closure_nargs_min = self.closure_nargs_min.min(other.closure_nargs_min);
        self.closure_nargs_max = self.closure_nargs_max.max(other.closure_nargs_max);
        self.fn_nargs_min = self.fn_nargs_min.min(other.fn_nargs_min);
        self.fn_nargs_max = self.fn_nargs_max.max(other.fn_nargs_max);
        self.fn_nargs_sum += other.fn_nargs_sum;
        self.closure_nargs_sum += other.closure_nargs_sum;
    }

    /// Returns the number of function arguments in a space.
    #[inline]
    #[must_use]
    pub fn fn_args(&self) -> f64 {
        self.fn_nargs as f64
    }

    /// Returns the number of closure arguments in a space.
    #[inline]
    #[must_use]
    pub fn closure_args(&self) -> f64 {
        self.closure_nargs as f64
    }

    /// Returns the number of function arguments sum in a space.
    #[inline]
    #[must_use]
    pub fn fn_args_sum(&self) -> f64 {
        self.fn_nargs_sum as f64
    }

    /// Returns the number of closure arguments sum in a space.
    #[inline]
    #[must_use]
    pub fn closure_args_sum(&self) -> f64 {
        self.closure_nargs_sum as f64
    }

    /// Returns the average number of functions arguments in a space.
    #[inline]
    #[must_use]
    pub fn fn_args_average(&self) -> f64 {
        self.fn_nargs_sum as f64 / self.total_functions.max(1) as f64
    }

    /// Returns the average number of closures arguments in a space.
    #[inline]
    #[must_use]
    pub fn closure_args_average(&self) -> f64 {
        self.closure_nargs_sum as f64 / self.total_closures.max(1) as f64
    }

    /// Returns the total number of arguments of each function and
    /// closure in a space.
    #[inline]
    #[must_use]
    pub fn nargs_total(&self) -> f64 {
        self.fn_args_sum() + self.closure_args_sum()
    }

    /// Returns the `NArgs` metric average value
    ///
    /// This value is computed dividing the `NArgs` value
    /// for the total number of functions/closures in a space.
    #[inline]
    #[must_use]
    pub fn nargs_average(&self) -> f64 {
        self.nargs_total() / (self.total_functions + self.total_closures).max(1) as f64
    }
    /// Returns the minimum number of function arguments in a space.
    #[inline]
    #[must_use]
    pub fn fn_args_min(&self) -> f64 {
        self.fn_nargs_min as f64
    }
    /// Returns the maximum number of function arguments in a space.
    #[inline]
    #[must_use]
    pub fn fn_args_max(&self) -> f64 {
        self.fn_nargs_max as f64
    }
    /// Returns the minimum number of closure arguments in a space.
    #[inline]
    #[must_use]
    pub fn closure_args_min(&self) -> f64 {
        self.closure_nargs_min as f64
    }
    /// Returns the maximum number of closure arguments in a space.
    #[inline]
    #[must_use]
    pub fn closure_args_max(&self) -> f64 {
        self.closure_nargs_max as f64
    }
    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.closure_nargs_sum += self.closure_nargs;
        self.fn_nargs_sum += self.fn_nargs;
    }
    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.closure_nargs_min = self.closure_nargs_min.min(self.closure_nargs);
        self.closure_nargs_max = self.closure_nargs_max.max(self.closure_nargs);
        self.fn_nargs_min = self.fn_nargs_min.min(self.fn_nargs);
        self.fn_nargs_max = self.fn_nargs_max.max(self.fn_nargs);
        self.compute_sum();
    }
    pub(crate) fn finalize(&mut self, total_functions: usize, total_closures: usize) {
        self.total_functions = total_functions;
        self.total_closures = total_closures;
    }
}

#[inline]
fn compute_args<T: Checker>(node: &Node, nargs: &mut usize) {
    if let Some(params) = node.child_by_field_name("parameters") {
        let node_params = params;
        node_params.act_on_child(&mut |n| {
            if !T::is_non_arg(n) {
                *nargs += 1;
            }
        });
    } else if node.child_by_field_name("parameter").is_some() {
        // JS/TS/TSX/MozJS arrow functions with a bare identifier parameter
        // (`x => …`) use the singular `parameter` field instead of the plural
        // `parameters` field. The grammar guarantees this is exactly one
        // identifier, so count it as one argument.
        *nargs += 1;
    }
}

pub trait NArgs
where
    Self: Checker,
    Self: std::marker::Sized,
{
    fn compute(node: &Node, stats: &mut Stats) {
        if Self::is_func(node) {
            compute_args::<Self>(node, &mut stats.fn_nargs);
            return;
        }

        if Self::is_closure(node) {
            compute_args::<Self>(node, &mut stats.closure_nargs);
        }
    }
}

impl NArgs for CppCode {
    fn compute(node: &Node, stats: &mut Stats) {
        if Self::is_func(node) {
            if let Some(declarator) = node.child_by_field_name("declarator") {
                let new_node = declarator;
                compute_args::<Self>(&new_node, &mut stats.fn_nargs);
            }
            return;
        }

        if Self::is_closure(node)
            && let Some(declarator) = node.child_by_field_name("declarator")
        {
            let new_node = declarator;
            compute_args::<Self>(&new_node, &mut stats.closure_nargs);
        }
    }
}

// Go's `parameter_declaration` allows multiple names to share one type
// (`func f(a, b int)` is one parameter_declaration with two `name` children
// but two formal parameters). Count names rather than declarations so the
// reported nargs matches Go's parameter count.
fn compute_go_args(node: &Node, nargs: &mut usize) {
    let Some(params) = node.child_by_field_name("parameters") else {
        return;
    };
    *nargs += params
        .children()
        .map(|child| match child.kind_id().into() {
            Go::ParameterDeclaration => child
                .children()
                .filter(|c| c.kind_id() == Go::Identifier)
                .count()
                .max(1),
            Go::VariadicParameterDeclaration => 1,
            _ => 0,
        })
        .sum::<usize>();
}

impl NArgs for GoCode {
    fn compute(node: &Node, stats: &mut Stats) {
        if Self::is_func(node) {
            compute_go_args(node, &mut stats.fn_nargs);
            return;
        }

        if Self::is_closure(node) {
            compute_go_args(node, &mut stats.closure_nargs);
        }
    }
}

fn compute_kotlin_func_args(node: &Node, nargs: &mut usize) {
    if let Some(params) = node
        .children()
        .find(|c| c.kind_id() == Kotlin::FunctionValueParameters)
    {
        params.act_on_child(&mut |n| {
            if n.kind_id() == Kotlin::Parameter {
                *nargs += 1;
            }
        });
    }
}

fn compute_kotlin_lambda_args(node: &Node, nargs: &mut usize) {
    // Lambda parameters are plain identifiers or destructuring patterns separated
    // by commas; there is no typed `Parameter` wrapper node (unlike function
    // value parameters), so a negative COMMA filter is the correct predicate here.
    if let Some(params) = node
        .children()
        .find(|c| c.kind_id() == Kotlin::LambdaParameters)
    {
        params.act_on_child(&mut |n| {
            if n.kind_id() != Kotlin::COMMA {
                *nargs += 1;
            }
        });
    }
}

impl NArgs for KotlinCode {
    fn compute(node: &Node, stats: &mut Stats) {
        if Self::is_func(node) {
            compute_kotlin_func_args(node, &mut stats.fn_nargs);
            return;
        }

        if Self::is_closure(node) {
            if node.kind_id() == Kotlin::LambdaLiteral {
                compute_kotlin_lambda_args(node, &mut stats.closure_nargs);
            } else {
                compute_kotlin_func_args(node, &mut stats.closure_nargs);
            }
        }
    }
}

fn compute_lua_args(node: &Node, nargs: &mut usize) {
    let Some(params) = node.child_by_field_name("parameters") else {
        return;
    };
    *nargs += params
        .children()
        .filter(|c| matches!(c.kind_id().into(), Lua::Identifier | Lua::VarargExpression))
        .count();
}

impl NArgs for LuaCode {
    fn compute(node: &Node, stats: &mut Stats) {
        if Self::is_func(node) {
            compute_lua_args(node, &mut stats.fn_nargs);
        } else if Self::is_closure(node) {
            compute_lua_args(node, &mut stats.closure_nargs);
        }
    }
}

fn compute_tcl_args(node: &Node, nargs: &mut usize) {
    let Some(params) = node.child_by_field_name("arguments") else {
        return;
    };
    *nargs += params
        .children()
        .filter(|c| c.kind_id() == Tcl::Argument)
        .count();
}

impl NArgs for TclCode {
    fn compute(node: &Node, stats: &mut Stats) {
        if Self::is_func(node) {
            compute_tcl_args(node, &mut stats.fn_nargs);
        }
    }
}

implement_metric_trait!(
    [NArgs],
    PythonCode,
    MozjsCode,
    JavascriptCode,
    TypescriptCode,
    TsxCode,
    RustCode,
    PreprocCode,
    CcommentCode,
    JavaCode,
    PerlCode,
    BashCode,
    PhpCode,
    CsharpCode
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

    #[test]
    fn python_no_functions_and_closures() {
        check_metrics::<PythonParser>("a = 42", "foo.py", |metric| {
            // 0 functions + 0 closures
            insta::assert_json_snapshot!(
                metric.nargs,
                @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 0.0,
                      "average_functions": 0.0,
                      "average_closures": 0.0,
                      "total": 0.0,
                      "average": 0.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn rust_no_functions_and_closures() {
        check_metrics::<RustParser>("let a = 42;", "foo.rs", |metric| {
            // 0 functions + 0 closures
            insta::assert_json_snapshot!(
                metric.nargs,
                @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 0.0,
                      "average_functions": 0.0,
                      "average_closures": 0.0,
                      "total": 0.0,
                      "average": 0.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn cpp_no_functions_and_closures() {
        check_metrics::<CppParser>("int a = 42;", "foo.cpp", |metric| {
            // 0 functions + 0 closures
            insta::assert_json_snapshot!(
                metric.nargs,
                @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 0.0,
                      "average_functions": 0.0,
                      "average_closures": 0.0,
                      "total": 0.0,
                      "average": 0.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn javascript_no_functions_and_closures() {
        check_metrics::<JavascriptParser>("var a = 42;", "foo.js", |metric| {
            // 0 functions + 0 closures
            insta::assert_json_snapshot!(
                metric.nargs,
                @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 0.0,
                      "average_functions": 0.0,
                      "average_closures": 0.0,
                      "total": 0.0,
                      "average": 0.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn python_single_function() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 if a:
                     return a",
            "foo.py",
            |metric| {
                // 1 function
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 2.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_single_function() {
        check_metrics::<RustParser>(
            "fn f(a: bool, b: usize) {
                 if a {
                     return a;
                }
             }",
            "foo.rs",
            |metric| {
                // 1 function
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 2.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_single_function() {
        check_metrics::<CppParser>(
            "int f(int a, int b) {
                 if (a) {
                     return a;
                }
             }",
            "foo.c",
            |metric| {
                // 1 function
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 2.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_single_function() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 return a * b;
             }",
            "foo.js",
            |metric| {
                // 1 function
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 2.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_single_lambda() {
        check_metrics::<PythonParser>("bar = lambda a: True", "foo.py", |metric| {
            // 1 lambda
            insta::assert_json_snapshot!(
                metric.nargs,
                @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 1.0,
                      "average_functions": 0.0,
                      "average_closures": 1.0,
                      "total": 1.0,
                      "average": 1.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 1.0,
                      "closures_max": 1.0
                    }"###
            );
        });
    }

    #[test]
    fn rust_single_closure() {
        check_metrics::<RustParser>("let bar = |i: i32| -> i32 { i + 1 };", "foo.rs", |metric| {
            // 1 lambda
            insta::assert_json_snapshot!(
                metric.nargs,
                @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 1.0,
                      "average_functions": 0.0,
                      "average_closures": 1.0,
                      "total": 1.0,
                      "average": 1.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
            );
        });
    }

    #[test]
    fn cpp_single_lambda() {
        check_metrics::<CppParser>(
            "auto bar = [](int x, int y) -> int { return x + y; };",
            "foo.cpp",
            |metric| {
                // 1 lambda
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 2.0,
                      "average_functions": 0.0,
                      "average_closures": 2.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 2.0,
                      "closures_max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_single_closure() {
        check_metrics::<JavascriptParser>("function (a, b) {return a + b};", "foo.js", |metric| {
            // 1 lambda
            insta::assert_json_snapshot!(
                metric.nargs,
                @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 2.0,
                      "average_functions": 0.0,
                      "average_closures": 2.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 2.0
                    }"###
            );
        });
    }

    #[test]
    fn python_functions() {
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
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 4.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 4.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );

        check_metrics::<PythonParser>(
            "def f(a, b):
                 if a:
                     return a
            def f(a, b, c):
                 if b:
                     return b",
            "foo.py",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 5.0,
                      "total_closures": 0.0,
                      "average_functions": 2.5,
                      "average_closures": 0.0,
                      "total": 5.0,
                      "average": 2.5,
                      "functions_min": 0.0,
                      "functions_max": 3.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_functions() {
        check_metrics::<RustParser>(
            "fn f(a: bool, b: usize) {
                 if a {
                     return a;
                }
             }
             fn f1(a: bool, b: usize) {
                 if a {
                     return a;
                }
             }",
            "foo.rs",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 4.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 4.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );

        check_metrics::<RustParser>(
            "fn f(a: bool, b: usize) {
                 if a {
                     return a;
                }
             }
             fn f1(a: bool, b: usize, c: usize) {
                 if a {
                     return a;
                }
             }",
            "foo.rs",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 5.0,
                      "total_closures": 0.0,
                      "average_functions": 2.5,
                      "average_closures": 0.0,
                      "total": 5.0,
                      "average": 2.5,
                      "functions_min": 0.0,
                      "functions_max": 3.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_functions() {
        check_metrics::<CppParser>(
            "int f(int a, int b) {
                 if (a) {
                     return a;
                }
             }
             int f1(int a, int b) {
                 if (a) {
                     return a;
                }
             }",
            "foo.c",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 4.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 4.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );

        check_metrics::<CppParser>(
            "int f(int a, int b) {
                 if (a) {
                     return a;
                }
             }
             int f1(int a, int b, int c) {
                 if (a) {
                     return a;
                }
             }",
            "foo.c",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 5.0,
                      "total_closures": 0.0,
                      "average_functions": 2.5,
                      "average_closures": 0.0,
                      "total": 5.0,
                      "average": 2.5,
                      "functions_min": 0.0,
                      "functions_max": 3.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_functions() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 return a * b;
             }
             function f1(a, b) {
                 return a * b;
             }",
            "foo.js",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 4.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 4.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );

        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 return a * b;
             }
             function f1(a, b, c) {
                 return a * b;
             }",
            "foo.js",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 5.0,
                      "total_closures": 0.0,
                      "average_functions": 2.5,
                      "average_closures": 0.0,
                      "total": 5.0,
                      "average": 2.5,
                      "functions_min": 0.0,
                      "functions_max": 3.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
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
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 3.0,
                      "total_closures": 2.0,
                      "average_functions": 1.5,
                      "average_closures": 1.0,
                      "total": 5.0,
                      "average": 1.25,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_nested_functions() {
        check_metrics::<RustParser>(
            "fn f(a: i32, b: i32) -> i32 {
                 fn foo(a: i32) -> i32 {
                     return a;
                 }
                 let bar = |a: i32, b: i32| -> i32 { a + 1 };
                 let bar1 = |b: i32| -> i32 { b + 1 };
                 return bar(foo(a), a);
             }",
            "foo.rs",
            |metric| {
                // 2 functions + 2 lambdas = 4
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 3.0,
                      "total_closures": 3.0,
                      "average_functions": 1.5,
                      "average_closures": 1.5,
                      "total": 6.0,
                      "average": 1.5,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn cpp_nested_functions() {
        check_metrics::<CppParser>(
            "int f(int a, int b, int c) {
                 auto foo = [](int x) -> int { return x; };
                 auto bar = [](int x, int y) -> int { return x + y; };
                 return bar(foo(a), a);
             }",
            "foo.cpp",
            |metric| {
                // 1 functions + 2 lambdas = 3
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 3.0,
                      "total_closures": 3.0,
                      "average_functions": 3.0,
                      "average_closures": 1.5,
                      "total": 6.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 3.0,
                      "closures_min": 0.0,
                      "closures_max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_zero_args() {
        check_metrics::<GoParser>(
            "package main
            func f() {}",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 0.0,
                      "average_functions": 0.0,
                      "average_closures": 0.0,
                      "total": 0.0,
                      "average": 0.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_multiple_args() {
        check_metrics::<GoParser>(
            "package main
            func f(a int, b string, c bool) {}",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 3.0,
                      "total_closures": 0.0,
                      "average_functions": 3.0,
                      "average_closures": 0.0,
                      "total": 3.0,
                      "average": 3.0,
                      "functions_min": 0.0,
                      "functions_max": 3.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_method_excludes_receiver() {
        check_metrics::<GoParser>(
            "package main
            type T struct{}
            func (t *T) Greet(name string) string {
                return name
            }",
            "foo.go",
            |metric| {
                // Receiver is in a separate `receiver` field and is not counted.
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 1.0,
                      "total_closures": 0.0,
                      "average_functions": 1.0,
                      "average_closures": 0.0,
                      "total": 1.0,
                      "average": 1.0,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_variadic() {
        check_metrics::<GoParser>(
            "package main
            func f(args ...int) {}",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 1.0,
                      "total_closures": 0.0,
                      "average_functions": 1.0,
                      "average_closures": 0.0,
                      "total": 1.0,
                      "average": 1.0,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_grouped_params() {
        check_metrics::<GoParser>(
            "package main
            func f(a, b int, c string) {}",
            "foo.go",
            |metric| {
                // `a, b int` is one parameter_declaration with two `name`
                // children — semantically two parameters.
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 3.0,
                      "total_closures": 0.0,
                      "average_functions": 3.0,
                      "average_closures": 0.0,
                      "total": 3.0,
                      "average": 3.0,
                      "functions_min": 0.0,
                      "functions_max": 3.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_func_literal_args() {
        check_metrics::<GoParser>(
            "package main
            var f = func(x, y int) int { return x + y }",
            "foo.go",
            |metric| {
                // Closure with grouped params: `x, y int` -> 2 closure args.
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 2.0,
                      "average_functions": 0.0,
                      "average_closures": 2.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_nested_functions() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 function foo(a, c) {
                     return a;
                 }
                 var bar = function (a, b) {return a + b};
                 function (a) {return a};
                 return bar(foo(a), a);
             }",
            "foo.js",
            |metric| {
                // 3 functions + 1 lambdas = 4
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 6.0,
                      "total_closures": 1.0,
                      "average_functions": 2.0,
                      "average_closures": 1.0,
                      "total": 7.0,
                      "average": 1.75,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn perl_no_functions_and_closures() {
        check_metrics::<PerlParser>(
            "my $x = 1;
             print $x;",
            "foo.pl",
            |metric| {
                // Cross-check via nom that no spurious sub/closure was
                // recognised — symmetric with the other `perl_*` nargs
                // tests, and would catch a regression that miscounted
                // `print` (or similar) as a function.
                assert_eq!(metric.nom.functions_sum(), 0.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 0.0,
                  "total_closures": 0.0,
                  "average_functions": 0.0,
                  "average_closures": 0.0,
                  "total": 0.0,
                  "average": 0.0,
                  "functions_min": 0.0,
                  "functions_max": 0.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_single_function() {
        // Perl args arrive via `@_` rather than as formal parameters in the
        // `sub` signature, so nargs is always 0. To make sure the test still
        // discriminates "function parsed" from "function silently dropped",
        // also assert nom recognised exactly one function.
        check_metrics::<PerlParser>(
            "sub greet {
                my ($name) = @_;
                print \"hi $name\";
            }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 0.0,
                  "total_closures": 0.0,
                  "average_functions": 0.0,
                  "average_closures": 0.0,
                  "total": 0.0,
                  "average": 0.0,
                  "functions_min": 0.0,
                  "functions_max": 0.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_single_closure() {
        // Same caveat as `perl_single_function`: closures take their
        // arguments through `@_`, so nargs stays 0. Assert via nom that the
        // anonymous function was actually identified as a closure.
        check_metrics::<PerlParser>(
            "my $f = sub {
                my ($x) = @_;
                return $x + 1;
            };",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 0.0);
                assert_eq!(metric.nom.closures_sum(), 1.0);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 0.0,
                  "total_closures": 0.0,
                  "average_functions": 0.0,
                  "average_closures": 0.0,
                  "total": 0.0,
                  "average": 0.0,
                  "functions_min": 0.0,
                  "functions_max": 0.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_multiple_functions() {
        // Same caveat as `perl_single_function`. Assert nom counted both
        // top-level subs so the test fails if either sub is dropped.
        check_metrics::<PerlParser>(
            "sub a { return 1; }
             sub b {
                 my ($x, $y) = @_;
                 return $x + $y;
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 0.0,
                  "total_closures": 0.0,
                  "average_functions": 0.0,
                  "average_closures": 0.0,
                  "total": 0.0,
                  "average": 0.0,
                  "functions_min": 0.0,
                  "functions_max": 0.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_nested_closure() {
        // Same caveat as `perl_single_function`. Assert nom recognised one
        // outer sub plus one nested closure.
        check_metrics::<PerlParser>(
            "sub outer {
                my $inner = sub { return 42; };
                return $inner->();
            }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1.0);
                assert_eq!(metric.nom.closures_sum(), 1.0);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 0.0,
                  "total_closures": 0.0,
                  "average_functions": 0.0,
                  "average_closures": 0.0,
                  "total": 0.0,
                  "average": 0.0,
                  "functions_min": 0.0,
                  "functions_max": 0.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_no_functions() {
        check_metrics::<JavaParser>(
            "class Foo {
                 int x = 42;
                 String name = \"hello\";
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                    {
                      "total_functions": 0.0,
                      "total_closures": 0.0,
                      "average_functions": 0.0,
                      "average_closures": 0.0,
                      "total": 0.0,
                      "average": 0.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }
                    "#
                );
            },
        );
    }

    #[test]
    fn java_single_method() {
        check_metrics::<JavaParser>(
            "class Foo {
                 void greet(String name, int count) {
                     return;
                 }
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 2.0,
                  "total_closures": 0.0,
                  "average_functions": 2.0,
                  "average_closures": 0.0,
                  "total": 2.0,
                  "average": 2.0,
                  "functions_min": 0.0,
                  "functions_max": 2.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_multiple_methods() {
        check_metrics::<JavaParser>(
            "class Foo {
                 void a(int x) {
                     return;
                 }
                 void b(int x, int y, int z) {
                     return;
                 }
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 4.0,
                  "total_closures": 0.0,
                  "average_functions": 2.0,
                  "average_closures": 0.0,
                  "total": 4.0,
                  "average": 2.0,
                  "functions_min": 0.0,
                  "functions_max": 3.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_constructor_args() {
        check_metrics::<JavaParser>(
            "class Foo {
                 Foo(String name, int age) {
                     return;
                 }
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 2.0,
                  "total_closures": 0.0,
                  "average_functions": 2.0,
                  "average_closures": 0.0,
                  "total": 2.0,
                  "average": 2.0,
                  "functions_min": 0.0,
                  "functions_max": 2.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_lambda_args() {
        check_metrics::<JavaParser>(
            "class Foo {
                 void run() {
                     Runnable r = (int a, int b) -> a + b;
                 }
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 0.0,
                  "total_closures": 2.0,
                  "average_functions": 0.0,
                  "average_closures": 2.0,
                  "total": 2.0,
                  "average": 1.0,
                  "functions_min": 0.0,
                  "functions_max": 0.0,
                  "closures_min": 0.0,
                  "closures_max": 2.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_no_functions() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 int x = 42;
                 string Name = \"hello\";
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                    {
                      "total_functions": 0.0,
                      "total_closures": 0.0,
                      "average_functions": 0.0,
                      "average_closures": 0.0,
                      "total": 0.0,
                      "average": 0.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }
                    "#
                );
            },
        );
    }

    #[test]
    fn csharp_single_method() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 void Greet(string name, int count) {
                     return;
                 }
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 2.0,
                  "total_closures": 0.0,
                  "average_functions": 2.0,
                  "average_closures": 0.0,
                  "total": 2.0,
                  "average": 2.0,
                  "functions_min": 0.0,
                  "functions_max": 2.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_multiple_methods() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 void A(int x) {
                     return;
                 }
                 void B(int x, int y, int z) {
                     return;
                 }
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 4.0,
                  "total_closures": 0.0,
                  "average_functions": 2.0,
                  "average_closures": 0.0,
                  "total": 4.0,
                  "average": 2.0,
                  "functions_min": 0.0,
                  "functions_max": 3.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_constructor_args() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 public Foo(string name, int age) {
                     return;
                 }
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 2.0,
                  "total_closures": 0.0,
                  "average_functions": 2.0,
                  "average_closures": 0.0,
                  "total": 2.0,
                  "average": 2.0,
                  "functions_min": 0.0,
                  "functions_max": 2.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_lambda_args() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 void Run() {
                     System.Func<int, int, int> f = (int a, int b) => a + b;
                 }
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "total_functions": 0.0,
                  "total_closures": 2.0,
                  "average_functions": 0.0,
                  "average_closures": 2.0,
                  "total": 2.0,
                  "average": 1.0,
                  "functions_min": 0.0,
                  "functions_max": 0.0,
                  "closures_min": 0.0,
                  "closures_max": 2.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn tsx_function_and_arrow() {
        check_metrics::<TsxParser>(
            "function add(a: number, b: number): number {
                 return a + b;
             }
             const multiply = (x: number, y: number) => x * y;",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 4.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 4.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_typed_and_optional_params() {
        check_metrics::<TypescriptParser>(
            "function format(value: number, prefix?: string, suffix?: string): string {
                 return (prefix ?? '') + value.toString() + (suffix ?? '');
             }
             const identity = (x: number): number => x;",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 4.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 4.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 3.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_single_function() {
        check_metrics::<MozjsParser>(
            "function f(a, b) {
                 return a * b;
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 2.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_closure_args() {
        check_metrics::<MozjsParser>("function (a, b) {return a + b};", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.nargs,
                @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 2.0,
                      "average_functions": 0.0,
                      "average_closures": 2.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 2.0
                    }"###
            );
        });
    }

    // Regression tests for issue #77: bare-identifier arrow functions
    // (`x => x`) use the singular `parameter` field instead of the plural
    // `parameters` field, and were previously counted as nargs=0.
    //
    // `nargs_total` is used so the assertion is independent of whether the
    // arrow function is classified as a function or a closure (this depends
    // on its enclosing context — e.g. a `VariableDeclarator` ancestor makes
    // it a function).

    #[test]
    fn javascript_bare_arrow_function() {
        check_metrics::<JavascriptParser>("const f = x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn javascript_async_bare_arrow_function() {
        check_metrics::<JavascriptParser>("const f = async x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn javascript_parenthesized_arrow_function() {
        check_metrics::<JavascriptParser>("const f = (x) => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn javascript_multi_parenthesized_arrow_function() {
        check_metrics::<JavascriptParser>("const f = (x, y) => x + y;", "foo.js", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 2.0);
        });
    }

    #[test]
    fn typescript_bare_arrow_function() {
        check_metrics::<TypescriptParser>("const f = x => x;", "foo.ts", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn typescript_async_bare_arrow_function() {
        check_metrics::<TypescriptParser>("const f = async x => x;", "foo.ts", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn typescript_parenthesized_arrow_function() {
        check_metrics::<TypescriptParser>("const f = (x: number) => x;", "foo.ts", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn typescript_multi_parenthesized_arrow_function() {
        check_metrics::<TypescriptParser>(
            "const f = (x: number, y: number) => x + y;",
            "foo.ts",
            |metric| {
                assert_eq!(metric.nargs.nargs_total(), 2.0);
            },
        );
    }

    #[test]
    fn tsx_bare_arrow_function() {
        check_metrics::<TsxParser>("const f = x => x;", "foo.tsx", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn tsx_async_bare_arrow_function() {
        check_metrics::<TsxParser>("const f = async x => x;", "foo.tsx", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn tsx_parenthesized_arrow_function() {
        check_metrics::<TsxParser>("const f = (x: number) => x;", "foo.tsx", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn tsx_multi_parenthesized_arrow_function() {
        check_metrics::<TsxParser>(
            "const f = (x: number, y: number) => x + y;",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.nargs.nargs_total(), 2.0);
            },
        );
    }

    #[test]
    fn mozjs_bare_arrow_function() {
        check_metrics::<MozjsParser>("const f = x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn mozjs_async_bare_arrow_function() {
        check_metrics::<MozjsParser>("const f = async x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn mozjs_parenthesized_arrow_function() {
        check_metrics::<MozjsParser>("const f = (x) => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 1.0);
        });
    }

    #[test]
    fn mozjs_multi_parenthesized_arrow_function() {
        check_metrics::<MozjsParser>("const f = (x, y) => x + y;", "foo.js", |metric| {
            assert_eq!(metric.nargs.nargs_total(), 2.0);
        });
    }

    #[test]
    fn kotlin_nargs_functions_and_closures() {
        check_metrics::<KotlinParser>(
            "fun add(a: Int, b: Int): Int {
                val transform = { x: Int, y: Int -> x + y }
                return transform(a, b)
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 2.0,
                      "total_closures": 2.0,
                      "average_functions": 2.0,
                      "average_closures": 2.0,
                      "total": 4.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 2.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn lua_no_functions_and_closures() {
        check_metrics::<LuaParser>("local x = 1", "foo.lua", |metric| {
            // No functions or closures: both halves are zero.
            assert_eq!(metric.nargs.fn_args_sum(), 0.0);
            assert_eq!(metric.nargs.closure_args_sum(), 0.0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn lua_single_function() {
        check_metrics::<LuaParser>("function f(a, b) return a + b end", "foo.lua", |metric| {
            // f(a, b) → fn_args_sum 2, no closures.
            assert_eq!(metric.nargs.fn_args_sum(), 2.0);
            assert_eq!(metric.nargs.closure_args_sum(), 0.0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn lua_single_closure() {
        check_metrics::<LuaParser>(
            "local f = function(a, b) return a + b end",
            "foo.lua",
            |metric| {
                // Anonymous `function(a, b)` bound via `local` → closure_args_sum 2.
                assert_eq!(metric.nargs.fn_args_sum(), 0.0);
                assert_eq!(metric.nargs.closure_args_sum(), 2.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn lua_functions() {
        check_metrics::<LuaParser>(
            "function f(a) return a end
function g(x, y, z) return x + y + z end",
            "foo.lua",
            |metric| {
                // f(a)=1 + g(x,y,z)=3 → fn_args_sum 4.
                assert_eq!(metric.nargs.fn_args_sum(), 4.0);
                assert_eq!(metric.nargs.closure_args_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn lua_vararg_function() {
        // `...` is a vararg_expression node and counts as one argument.
        check_metrics::<LuaParser>("function f(a, ...) return a end", "foo.lua", |metric| {
            // a + ... → fn_args_sum 2.
            assert_eq!(metric.nargs.fn_args_sum(), 2.0);
            assert_eq!(metric.nargs.closure_args_sum(), 0.0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn lua_colon_method_nargs() {
        // Colon syntax: `self` is implicit and NOT in the `parameters` node.
        // Only explicit params (a, b) are counted.
        check_metrics::<LuaParser>(
            "function obj:method(a, b) return a + b end",
            "foo.lua",
            |metric| {
                // Only explicit a, b → fn_args_sum 2 (implicit self excluded).
                assert_eq!(metric.nargs.fn_args_sum(), 2.0);
                assert_eq!(metric.nargs.closure_args_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn tcl_no_functions() {
        check_metrics::<TclParser>("set x 1", "foo.tcl", |metric| {
            // Bare `set` command, no procs → both halves zero.
            assert_eq!(metric.nargs.fn_args_sum(), 0.0);
            assert_eq!(metric.nargs.closure_args_sum(), 0.0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn tcl_single_function() {
        check_metrics::<TclParser>("proc f {a b} { puts $a }", "foo.tcl", |metric| {
            // proc f {a b} → fn_args_sum 2.
            assert_eq!(metric.nargs.fn_args_sum(), 2.0);
            assert_eq!(metric.nargs.closure_args_sum(), 0.0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn tcl_single_function_no_args() {
        check_metrics::<TclParser>("proc f {} { puts hello }", "foo.tcl", |metric| {
            // proc f {} → empty arg list, fn_args_sum 0.
            assert_eq!(metric.nargs.fn_args_sum(), 0.0);
            assert_eq!(metric.nargs.closure_args_sum(), 0.0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn tcl_functions() {
        check_metrics::<TclParser>(
            "proc f {a b} { puts $a }
proc g {x y z} { puts $x }",
            "foo.tcl",
            |metric| {
                // f(a,b)=2 + g(x,y,z)=3 → fn_args_sum 5.
                assert_eq!(metric.nargs.fn_args_sum(), 5.0);
                assert_eq!(metric.nargs.closure_args_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn tcl_nested_functions() {
        check_metrics::<TclParser>(
            "proc outer {a} {
    proc inner {x y} { puts $x }
    inner $a $a
}",
            "foo.tcl",
            |metric| {
                // outer(a)=1 + inner(x,y)=2 → fn_args_sum 3.
                assert_eq!(metric.nargs.fn_args_sum(), 3.0);
                assert_eq!(metric.nargs.closure_args_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn tcl_args_vararg() {
        // `args` is the Tcl variadic catch-all; it counts as one argument.
        check_metrics::<TclParser>("proc f {a b args} { puts $a }", "foo.tcl", |metric| {
            // a + b + args → fn_args_sum 3 (variadic is one slot).
            assert_eq!(metric.nargs.fn_args_sum(), 3.0);
            assert_eq!(metric.nargs.closure_args_sum(), 0.0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn tcl_default_arg() {
        // `{name default}` is a single argument with a default value.
        check_metrics::<TclParser>(
            "proc greet {{name World} greeting} {
    puts \"$greeting, $name!\"
}",
            "foo.tcl",
            |metric| {
                // {name World} counts as one slot + greeting → fn_args_sum 2.
                assert_eq!(metric.nargs.fn_args_sum(), 2.0);
                assert_eq!(metric.nargs.closure_args_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn kotlin_zero_args() {
        check_metrics::<KotlinParser>("fun f(): Int { return 42 }", "foo.kt", |metric| {
            // fun f() → empty parameter list, fn_args_sum 0.
            assert_eq!(metric.nargs.fn_args_sum(), 0.0);
            assert_eq!(metric.nargs.closure_args_sum(), 0.0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn kotlin_single_arg() {
        check_metrics::<KotlinParser>(
            "fun double(x: Int): Int { return x * 2 }",
            "foo.kt",
            |metric| {
                // double(x) → fn_args_sum 1.
                assert_eq!(metric.nargs.fn_args_sum(), 1.0);
                assert_eq!(metric.nargs.closure_args_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn kotlin_multiple_args() {
        check_metrics::<KotlinParser>(
            "fun add(a: Int, b: Int, c: Int): Int { return a + b + c }",
            "foo.kt",
            |metric| {
                // add(a, b, c) → fn_args_sum 3.
                assert_eq!(metric.nargs.fn_args_sum(), 3.0);
                assert_eq!(metric.nargs.closure_args_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn kotlin_default_args() {
        check_metrics::<KotlinParser>(
            "fun greet(name: String = \"World\", greeting: String = \"Hello\"): String {
                 return \"$greeting, $name!\"
             }",
            "foo.kt",
            |metric| {
                // Defaults still count as parameter slots → fn_args_sum 2.
                assert_eq!(metric.nargs.fn_args_sum(), 2.0);
                assert_eq!(metric.nargs.closure_args_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn kotlin_empty_lambda() {
        // Two lambdas in the same function body: one with two explicit parameters
        // (proving the lambda path is taken and args are counted), and one with an
        // explicit empty parameter list `{ -> expr }` (proving
        // `compute_kotlin_lambda_args` returns 0 for it without crashing or
        // accidentally counting tokens inside the arrow expression).
        // If the grammar fails to parse either lambda, `total_closures` would be
        // lower than 2, making the snapshot unambiguous.
        check_metrics::<KotlinParser>(
            "fun f() {
                 val two = { x: Int, y: Int -> x + y }
                 val zero = { -> 42 }
             }",
            "foo.kt",
            |metric| {
                // Outer fun f() has 0 params; two lambdas counted as closures:
                // {x, y -> ...} contributes 2, {-> 42} contributes 0 →
                // closure_args_sum 2 across two closure entries.
                assert_eq!(metric.nargs.fn_args_sum(), 0.0);
                assert_eq!(metric.nargs.closure_args_sum(), 2.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn kotlin_anonymous_function() {
        // `fun(x: Int, y: Int) = x + y` — anonymous function expression.
        // The grammar surfaces it as an `AnonymousFunction` node, which routes
        // through `compute_kotlin_func_args` (not the lambda path).
        check_metrics::<KotlinParser>(
            "val add = fun(x: Int, y: Int): Int = x + y",
            "foo.kt",
            |metric| {
                // Anonymous fun(x, y) is classified as a closure → closure_args_sum 2.
                assert_eq!(metric.nargs.fn_args_sum(), 0.0);
                assert_eq!(metric.nargs.closure_args_sum(), 2.0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[test]
    fn php_no_functions_and_closures() {
        check_metrics::<PhpParser>("<?php $a = 42;", "foo.php", |metric| {
            insta::assert_json_snapshot!(
                metric.nargs,
                @r###"
                {
                  "total_functions": 0.0,
                  "total_closures": 0.0,
                  "average_functions": 0.0,
                  "average_closures": 0.0,
                  "total": 0.0,
                  "average": 0.0,
                  "functions_min": 0.0,
                  "functions_max": 0.0,
                  "closures_min": 0.0,
                  "closures_max": 0.0
                }"###
            );
        });
    }

    #[test]
    fn php_single_function() {
        // Two parameters in a regular function.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a, int $b): bool {
                if ($a) { return $a; }
                return false;
            }",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 2.0,
                      "total_closures": 0.0,
                      "average_functions": 2.0,
                      "average_closures": 0.0,
                      "total": 2.0,
                      "average": 2.0,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_single_closure() {
        // Anonymous function with 2 params + arrow function with 1 param.
        // Each is a separate closure space.
        check_metrics::<PhpParser>(
            "<?php
            $f = function (int $a, int $b) { return $a + $b; };
            $g = fn (int $x) => $x * 2;",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 0.0,
                      "total_closures": 3.0,
                      "average_functions": 0.0,
                      "average_closures": 1.5,
                      "total": 3.0,
                      "average": 1.5,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_functions() {
        // Two top-level functions, 1 + 2 args.
        check_metrics::<PhpParser>(
            "<?php
            function a(int $x): int { return $x; }
            function b(int $x, int $y): int { return $x + $y; }",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 3.0,
                      "total_closures": 0.0,
                      "average_functions": 1.5,
                      "average_closures": 0.0,
                      "total": 3.0,
                      "average": 1.5,
                      "functions_min": 0.0,
                      "functions_max": 2.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_nested_functions() {
        // PHP cannot define nested named functions inside a function body
        // syntactically, but a class with methods exhibits the same shape:
        // a top-level scope plus inner function-spaces.
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public function outer(int $a): int {
                    $f = function (int $b) use ($a) { return $a + $b; };
                    return $f($a);
                }
            }",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r###"
                    {
                      "total_functions": 1.0,
                      "total_closures": 1.0,
                      "average_functions": 1.0,
                      "average_closures": 1.0,
                      "total": 2.0,
                      "average": 1.0,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }
}
