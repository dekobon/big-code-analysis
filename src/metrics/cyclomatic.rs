use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use crate::checker::Checker;
use crate::macros::implement_metric_trait;
use crate::*;

/// The `Cyclomatic` metric.
#[derive(Debug, Clone)]
pub struct Stats {
    cyclomatic_sum: f64,
    cyclomatic: f64,
    n: usize,
    cyclomatic_max: f64,
    cyclomatic_min: f64,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            cyclomatic_sum: 0.,
            cyclomatic: 1.,
            n: 1,
            cyclomatic_max: 0.,
            cyclomatic_min: f64::MAX,
        }
    }
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("cyclomatic", 4)?;
        st.serialize_field("sum", &self.cyclomatic_sum())?;
        st.serialize_field("average", &self.cyclomatic_average())?;
        st.serialize_field("min", &self.cyclomatic_min())?;
        st.serialize_field("max", &self.cyclomatic_max())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "sum: {}, average: {}, min: {}, max: {}",
            self.cyclomatic_sum(),
            self.cyclomatic_average(),
            self.cyclomatic_min(),
            self.cyclomatic_max()
        )
    }
}

impl Stats {
    /// Merges a second `Cyclomatic` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        //Calculate minimum and maximum values
        self.cyclomatic_max = self.cyclomatic_max.max(other.cyclomatic_max);
        self.cyclomatic_min = self.cyclomatic_min.min(other.cyclomatic_min);

        self.cyclomatic_sum += other.cyclomatic_sum;
        self.n += other.n;
    }

    /// Returns the `Cyclomatic` metric value
    pub fn cyclomatic(&self) -> f64 {
        self.cyclomatic
    }
    /// Returns the sum
    pub fn cyclomatic_sum(&self) -> f64 {
        self.cyclomatic_sum
    }

    /// Returns the `Cyclomatic` metric average value
    ///
    /// This value is computed dividing the `Cyclomatic` value for the
    /// number of spaces.
    pub fn cyclomatic_average(&self) -> f64 {
        self.cyclomatic_sum() / self.n as f64
    }
    /// Returns the `Cyclomatic` maximum value
    pub fn cyclomatic_max(&self) -> f64 {
        self.cyclomatic_max
    }
    /// Returns the `Cyclomatic` minimum value
    pub fn cyclomatic_min(&self) -> f64 {
        self.cyclomatic_min
    }
    #[inline(always)]
    pub(crate) fn compute_sum(&mut self) {
        self.cyclomatic_sum += self.cyclomatic;
    }
    #[inline(always)]
    pub(crate) fn compute_minmax(&mut self) {
        self.cyclomatic_max = self.cyclomatic_max.max(self.cyclomatic);
        self.cyclomatic_min = self.cyclomatic_min.min(self.cyclomatic);
        self.compute_sum();
    }
}

pub trait Cyclomatic
where
    Self: Checker,
{
    fn compute(node: &Node, stats: &mut Stats);
}

impl Cyclomatic for PythonCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Python::*;

        match node.kind_id().into() {
            If | Elif | For | While | Except | With | Assert | And | Or => {
                stats.cyclomatic += 1.;
            }
            Else if node.has_ancestors(
                |node| matches!(node.kind_id().into(), ForStatement | WhileStatement),
                |node| node.kind_id() == ElseClause,
            ) =>
            {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for MozjsCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Mozjs::*;

        match node.kind_id().into() {
            If | For | While | Case | Catch | TernaryExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for JavascriptCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Javascript::*;

        match node.kind_id().into() {
            If | For | While | Case | Catch | TernaryExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for TypescriptCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Typescript::*;

        match node.kind_id().into() {
            If | For | While | Case | Catch | TernaryExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for TsxCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Tsx::*;

        match node.kind_id().into() {
            If | For | While | Case | Catch | TernaryExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for RustCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Rust::*;

        match node.kind_id().into() {
            If | For | While | Loop | MatchArm | MatchArm2 | TryExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for CppCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Cpp::*;

        match node.kind_id().into() {
            If | For | While | Case | Catch | ConditionalExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for JavaCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Java::*;

        match node.kind_id().into() {
            If | For | While | Case | Catch | TernaryExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for GoCode {
    fn compute(node: &Node, stats: &mut Stats) {
        // Aliased because `Go::Go` (the `go` keyword variant) collides with
        // the bare enum name in pattern position under `use Go::*;`.
        use Go as G;

        match node.kind_id().into() {
            G::IfStatement
            | G::ForStatement
            | G::ExpressionCase
            | G::TypeCase
            | G::CommunicationCase
            | G::AMPAMP
            | G::PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for PerlCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Perl as P;

        match node.kind_id().into() {
            // Branching control-flow constructs
            P::IfStatement
            | P::UnlessStatement
            | P::ElsifClause
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
            | P::ForStatement2
            | P::WhenSimpleStatement
            // Postfix conditional / loop forms (`do_thing() if cond;`)
            | P::IfSimpleStatement
            | P::UnlessSimpleStatement
            | P::WhileSimpleStatement
            | P::UntilSimpleStatement
            | P::ForSimpleStatement
            // Short-circuit boolean operators and ternary
            | P::AMPAMP
            | P::PIPEPIPE
            | P::SLASHSLASH
            | P::And
            | P::Or
            | P::TernaryExpression => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for KotlinCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Kotlin::*;

        match node.kind_id().into() {
            IfExpression | ForStatement | WhileStatement | DoWhileStatement | WhenEntry
            | CatchBlock | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for LuaCode {
    fn compute(node: &Node, stats: &mut Stats) {
        match node.kind_id().into() {
            Lua::IfStatement
            | Lua::ElseifStatement
            | Lua::ForStatement
            | Lua::WhileStatement
            | Lua::RepeatStatement
            | Lua::And
            | Lua::Or => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for PhpCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Php::*;

        match node.kind_id().into() {
            IfStatement
            | ElseIfClause
            | ElseIfClause2
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | CaseStatement
            | MatchConditionalExpression
            | ConditionalExpression
            | CatchClause
            | AMPAMP
            | PIPEPIPE
            | And
            | Or
            | Xor
            | QMARKQMARK => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

implement_metric_trait!(Cyclomatic, PreprocCode, CcommentCode);

impl Cyclomatic for BashCode {
    fn compute(node: &Node, stats: &mut Stats) {
        match node.kind_id().into() {
            // Control flow: +1 each (WhileStatement covers both while and until;
            // ForStatement covers both for and select)
            Bash::IfStatement
            | Bash::ElifClause
            | Bash::ForStatement
            | Bash::CStyleForStatement
            | Bash::WhileStatement
            | Bash::CaseStatement
            // Case arms: +1 each
            | Bash::CaseItem
            | Bash::CaseItem2
            // Logical operators: count the tokens directly so we catch
            // both command-level (inside List) and expression-level
            // (inside [[ ]] / (( ))) uses.
            | Bash::AMPAMP
            | Bash::PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for TclCode {
    fn compute(node: &Node, stats: &mut Stats) {
        match node.kind_id().into() {
            Tcl::If
            | Tcl::Elseif
            | Tcl::Foreach
            | Tcl::While
            | Tcl::Catch
            | Tcl::TernaryExpr
            | Tcl::AMPAMP
            | Tcl::PIPEPIPE => {
                stats.cyclomatic += 1.;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::check_metrics;

    use super::*;

    #[test]
    fn python_simple_function() {
        check_metrics::<PythonParser>(
            "def f(a, b): # +2 (+1 unit space)
                if a and b:  # +2 (+1 and)
                   return 1
                if c and d: # +2 (+1 and)
                   return 1",
            "foo.py",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_1_level_nesting() {
        check_metrics::<PythonParser>(
            "def f(a, b): # +2 (+1 unit space)
                if a:  # +1
                    for i in range(b):  # +1
                        return 1",
            "foo.py",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_1_level_nesting() {
        check_metrics::<RustParser>(
            "fn f() { // +2 (+1 unit space)
                 if true { // +1
                     match true {
                         true => println!(\"test\"), // +1
                         false => println!(\"test\"), // +1
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_switch() {
        check_metrics::<CppParser>(
            "void f() { // +2 (+1 unit space)
                 switch (1) {
                     case 1: // +1
                         printf(\"one\");
                         break;
                     case 2: // +1
                         printf(\"two\");
                         break;
                     case 3: // +1
                         printf(\"three\");
                         break;
                     default:
                         printf(\"all\");
                         break;
                 }
             }",
            "foo.c",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_real_function() {
        check_metrics::<CppParser>(
            "int sumOfPrimes(int max) { // +2 (+1 unit space)
                 int total = 0;
                 OUT: for (int i = 1; i <= max; ++i) { // +1
                   for (int j = 2; j < i; ++j) { // +1
                       if (i % j == 0) { // +1
                          continue OUT;
                       }
                   }
                   total += i;
                 }
                 return total;
            }",
            "foo.c",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_unit_before() {
        check_metrics::<CppParser>(
            "
            int a=42;
            if(a==42) //+2(+1 unit space)
            {

            }
            if(a==34) //+1
            {

            }
            int sumOfPrimes(int max) { // +1
                 int total = 0;
                 OUT: for (int i = 1; i <= max; ++i) { // +1
                   for (int j = 2; j < i; ++j) { // +1
                       if (i % j == 0) { // +1
                          continue OUT;
                       }
                   }
                   total += i;
                 }
                 return total;
            }",
            "foo.c",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 3.5,
                      "min": 3.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    /// Test to handle the case of min and max when merge happen before the final value of one module are set.
    /// In this case the min value should be 3 because the unit space has 2 branches and a complexity of 3
    /// while the function sumOfPrimes has a complexity of 4.
    #[test]
    fn c_unit_after() {
        check_metrics::<CppParser>(
            "
            int sumOfPrimes(int max) { // +1
                 int total = 0;
                 OUT: for (int i = 1; i <= max; ++i) { // +1
                   for (int j = 2; j < i; ++j) { // +1
                       if (i % j == 0) { // +1
                          continue OUT;
                       }
                   }
                   total += i;
                 }
                 return total;
            }

            int a=42;
            if(a==42) //+2(+1 unit space)
            {

            }
            if(a==34) //+1
            {

            }",
            "foo.c",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 3.5,
                      "min": 3.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_simple_class() {
        check_metrics::<JavaParser>(
            "
            public class Example { // +2 (+1 unit space)
                int a = 10;
                boolean b = (a > 5) ? true : false; // +1
                boolean c = b && true; // +1

                public void m1() { // +1
                    if (a % 2 == 0) { // +1
                        b = b || c; // +1
                    }
                }
                public void m2() { // +1
                    while (a > 3) { // +1
                        m1();
                        a--;
                    }
                }
            }",
            "foo.java",
            |metric| {
                // nspace = 4 (unit, class and 2 methods)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 9.0,
                      "average": 2.25,
                      "min": 1.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_real_class() {
        check_metrics::<JavaParser>(
            "
            public class Matrix { // +2 (+1 unit space)
                private int[][] m = new int[5][5];

                public void init() { // +1
                    for (int i = 0; i < m.length; i++) { // +1
                        for (int j = 0; j < m[i].length; j++) { // +1
                            m[i][j] = i * j;
                        }
                    }
                }
                public int compute(int i, int j) { // +1
                    try {
                        return m[i][j] / m[j][i];
                    } catch (ArithmeticException e) { // +1
                        return -1;
                    } catch (ArrayIndexOutOfBoundsException e) { // +1
                        return -2;
                    }
                }
                public void print(int result) { // +1
                    switch (result) {
                        case -1: // +1
                            System.out.println(\"Division by zero\");
                            break;
                        case -2: // +1
                            System.out.println(\"Wrong index number\");
                            break;
                        default:
                            System.out.println(\"The result is \" + result);
                    }
                }
            }",
            "foo.java",
            |metric| {
                // nspace = 5 (unit, class and 3 methods)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 2.2,
                      "min": 1.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_simple_function() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) { // +2 (+1 unit space)
                 if (a) { // +1
                     return a;
                 } else if (b) { // +1
                     return b;
                 }
                 return 0;
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_switch() {
        check_metrics::<JavascriptParser>(
            "function f() { // +2 (+1 unit space)
                 switch (x) {
                     case 1: // +1
                         console.log(\"one\");
                         break;
                     case 2: // +1
                         console.log(\"two\");
                         break;
                     case 3: // +1
                         console.log(\"three\");
                         break;
                     default:
                         console.log(\"other\");
                         break;
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_simple_function() {
        check_metrics::<GoParser>(
            "package main
            func f() {}",
            "foo.go",
            |metric| {
                // nspace = 2 (file unit + func), each base 1.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 1.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_if_else() {
        check_metrics::<GoParser>(
            "package main
            func f(x bool) { // +2 (+1 unit)
                if x { // +1
                } else {
                }
            }",
            "foo.go",
            |metric| {
                // `else` clause attaches to the same if_statement node and is
                // not counted again.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_else_if_chain() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) { // +2 (+1 unit)
                if x > 0 { // +1
                } else if x < 0 { // +1 (nested if_statement)
                } else if x == 0 { // +1 (nested if_statement)
                } else {
                }
            }",
            "foo.go",
            |metric| {
                // tree-sitter-go represents `else if` as a nested
                // if_statement under the parent's `else` clause; each nested
                // if contributes +1.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_for_loop() {
        check_metrics::<GoParser>(
            "package main
            func f() { // +2 (+1 unit)
                for i := 0; i < 10; i++ { // +1
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_for_range() {
        check_metrics::<GoParser>(
            "package main
            func f(xs []int) { // +2 (+1 unit)
                for _, v := range xs { // +1
                    _ = v
                }
            }",
            "foo.go",
            |metric| {
                // range_clause is a child of for_statement; only the
                // for_statement contributes.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_switch() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) { // +2 (+1 unit)
                switch x {
                case 1: // +1
                case 2: // +1
                default: // not counted
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_type_switch() {
        check_metrics::<GoParser>(
            "package main
            func f(x interface{}) { // +2 (+1 unit)
                switch x.(type) {
                case int: // +1
                case string: // +1
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_select() {
        check_metrics::<GoParser>(
            "package main
            func f(c1, c2 chan int) { // +2 (+1 unit)
                select {
                case <-c1: // +1
                case <-c2: // +1
                default: // not counted
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_logical_operators() {
        check_metrics::<GoParser>(
            "package main
            func f(a, b, c bool) { // +2 (+1 unit)
                if a && b || c { // +1 if, +1 &&, +1 ||
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_defer_and_go_do_not_count() {
        check_metrics::<GoParser>(
            "package main
            func f() { // +2 (+1 unit)
                defer cleanup()
                go work()
            }",
            "foo.go",
            |metric| {
                // defer_statement and go_statement are not branches.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 1.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    // As reported here:
    // https://github.com/sebastianbergmann/php-code-coverage/issues/607
    // An anonymous class declaration is not considered when computing the Cyclomatic Complexity metric for Java
    // Only the complexity of the anonymous class content is considered for the computation
    #[test]
    fn java_anonymous_class() {
        check_metrics::<JavaParser>(
            "
            abstract class A { // +2 (+1 unit space)
                public abstract boolean m1(int n); // +1
                public abstract boolean m2(int n); // +1
            }
            public class B { // +1

                public void test() { // +1
                    A a = new A() {
                        public boolean m1(int n) { // +1
                            if (n % 2 == 0) { // +1
                                return true;
                            }
                            return false;
                        }
                        public boolean m2(int n) { // +1
                            if (n % 5 == 0) { // +1
                                return true;
                            }
                            return false;
                        }
                    };
                }
            }",
            "foo.java",
            |metric| {
                // nspace = 8 (unit, 2 classes and 5 methods)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 10.0,
                      "average": 1.25,
                      "min": 1.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn perl_nested_control_flow() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                for my $i (1..10) { # +1 for_statement_2
                    if ($i % 2) { # +1 if_statement
                        print $i;
                    }
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_postfix_conditionals() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                return 1 if $_[0]; # +1 if_simple_statement
                return 0 unless $_[1]; # +1 unless_simple_statement
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_unless_and_until() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                unless ($x) { # +1 unless_statement
                    print 'a';
                }
                until ($n == 0) { # +1 until_statement
                    $n--;
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_logical_operators_and_ternary() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                my $x = $a && $b; # +1 (&&)
                my $y = $c || $d; # +1 (||)
                my $z = $e // $f; # +1 (//)
                my $t = $g ? 1 : 0; # +1 ternary
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 6.0,
                  "average": 3.0,
                  "min": 1.0,
                  "max": 5.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_word_logical_operators() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                my $x = $a and $b; # +1 (and)
                my $y = $c or $d; # +1 (or)
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_foreach_loop() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                foreach my $i (@list) { # +1 for_statement_2
                    print $i;
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic, @r#"
                {
                  "sum": 3.0,
                  "average": 1.5,
                  "min": 1.0,
                  "max": 2.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_else_does_not_count_but_elsif_does() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                if ($x) { # +1 if_statement
                    print 'a';
                } elsif ($y) { # +1 elsif_clause
                    print 'b';
                } else {
                    print 'c';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0
                }
                 "#
                );
            },
        );
    }

    #[test]
    fn tsx_simple_function() {
        check_metrics::<TsxParser>(
            "function f(a: number, b: number) { // +2 (+1 unit space)
                 if (a > 0) { // +1
                     return a;
                 } else if (b > 0) { // +1
                     return b;
                 }
                 return 0;
             }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_if_else_and_switch() {
        check_metrics::<TypescriptParser>(
            "function classify(value: number): string {
                 if (value < 0) { // +1
                     return 'negative';
                 } else if (value === 0) { // +1
                     return 'zero';
                 }
                 switch (value) {
                     case 1: // +1
                         return 'one';
                     case 2: // +1
                         return 'two';
                     default:
                         return 'other';
                 }
             }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_if_else_and_switch() {
        check_metrics::<MozjsParser>(
            "function f(x) { // +2 (+1 unit space)
                 if (x > 0) { // +1
                     return 1;
                 } else if (x < 0) { // +1
                     return -1;
                 }
                 switch (x) {
                     case 0: // +1
                         return 0;
                     case 42: // +1
                         return 42;
                     default:
                         return -2;
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn kotlin_cyclomatic_mixed() {
        check_metrics::<KotlinParser>(
            "class Calc {
                fun compute(x: Int, y: Int): Int {
                    if (x > 0) {            // +1
                        for (i in 1..x) {   // +1
                            println(i)
                        }
                    }
                    when (y) {
                        1 -> println(\"one\")   // +1 (WhenEntry)
                        2 -> println(\"two\")   // +1
                        else -> println(\"?\") // +1
                    }
                    val ok = x > 0 && y > 0  // +1
                    try {
                        println(x / y)
                    } catch (e: Exception) { // +1
                        println(\"err\")
                    }
                    return x + y
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 10.0,
                      "average": 3.3333333333333335,
                      "min": 1.0,
                      "max": 8.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn lua_1_level_nesting() {
        // chunk: base=1; f: base=1 + for=1 + if=1 = 3; sum=4
        check_metrics::<LuaParser>(
            "local function f(t)
  for i = 1, #t do
    if t[i] > 0 then
      return t[i]
    end
  end
  return 0
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic, @r###"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0
                }
                "###);
            },
        );
    }

    #[test]
    fn lua_elseif_branches() {
        // chunk: base=1; classify: base=1 + if=1 + elseif=1 + elseif=1 = 4
        // else does NOT add a branch; sum=5
        check_metrics::<LuaParser>(
            "local function classify(x)
  if x > 0 then
    return 1
  elseif x < 0 then
    return -1
  elseif x == 0 then
    return 0
  else
    return 0
  end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic, @r###"
                {
                  "sum": 5.0,
                  "average": 2.5,
                  "min": 1.0,
                  "max": 4.0
                }
                "###);
            },
        );
    }

    #[test]
    fn lua_logical_operators() {
        // chunk: base=1; f: base=1 + if=1 + and=1 + or=1 = 4; sum=5
        check_metrics::<LuaParser>(
            "local function f(a, b, c)
  if a and b or c then
    return 1
  end
  return 0
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic, @r###"
                {
                  "sum": 5.0,
                  "average": 2.5,
                  "min": 1.0,
                  "max": 4.0
                }
                "###);
            },
        );
    }

    #[test]
    fn bash_nested_control_flow() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    if [ $1 -eq 1 ]; then
        for i in 1 2 3; do
            echo $i
        done
    elif [ $1 -eq 2 ]; then
        echo 'two'
    fi
}",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    {".sum" => insta::rounded_redaction(2)}
                );
            },
        );
    }

    #[test]
    fn tcl_1_level_nesting() {
        // chunk: base=1; f: base=1 + while=1 + if=1 = 3; sum=4
        check_metrics::<TclParser>(
            "proc f {x} {
    while {$x > 0} {
        if {$x > 10} {
            set x [expr {$x - 1}]
        }
    }
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tcl_elseif_branch() {
        // if=1, elseif=1; else does NOT add a branch; sum=3 (chunk base=1)
        check_metrics::<TclParser>(
            "proc f {x} {
    if {$x > 10} {
        puts big
    } elseif {$x > 5} {
        puts medium
    } else {
        puts small
    }
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tcl_logical_operators() {
        // &&=1 and ||=1 inside expr; sum=3 (chunk=1, proc base=1, &&=1, ||=1)
        check_metrics::<TclParser>(
            "proc f {x y z} {
    if {$x > 0 && $y > 0 || $z > 0} {
        puts ok
    }
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tcl_catch_branch() {
        // `catch` command adds +1 (conditional handler); `try` does NOT add a branch.
        // source_file(1) + proc_space(base=1 + catch=1 = 2) = sum=3
        check_metrics::<TclParser>(
            "proc f {} {
    catch {
        expr {1 / 0}
    } msg
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tcl_try_no_branch() {
        // `try` is NOT a conditional construct; it does not add cyclomatic complexity.
        // Only the base counts: source_file(1) + proc_space(base=1) = sum=2, average=1.
        check_metrics::<TclParser>(
            "proc f {} {
    try {
        expr {1 / 0}
    } finally {
        puts done
    }
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 1.0,
                      "max": 1.0
                    }
                    "#
                );
            },
        );
    }

    #[test]
    fn mozjs_for_loop() {
        check_metrics::<MozjsParser>(
            "function f(n) { // +2 (+1 unit)
             var s = 0;
             for (var i = 0; i < n; i++) { // +1
                 s += i;
             }
             return s;
         }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn mozjs_logical_operators() {
        check_metrics::<MozjsParser>(
            "function f(a, b, c) { // +2 (+1 unit)
             if (a && b || c) { // +1 if, +1 &&, +1 ||
                 return 1;
             }
             return 0;
         }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn mozjs_while_loop() {
        check_metrics::<MozjsParser>(
            "function f(n) { // +2 (+1 unit)
             var i = 0;
             while (i < n) { // +1
                 i++;
             }
             return i;
         }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn bash_while_loop() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    local n=$1
    while [ $n -gt 0 ]; do
        echo $n
        n=$((n - 1))
    done
}",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn bash_case_statement() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    case $1 in
        start) echo starting ;;
        stop)  echo stopping ;;
        *)     echo unknown  ;;
    esac
}",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn bash_simple_function() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    echo hello
}",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn kotlin_for_loop() {
        check_metrics::<KotlinParser>(
            "fun sum(n: Int): Int {  // +2 (+1 unit)
             var s = 0
             for (i in 1..n) {  // +1
                 s += i
             }
             return s
         }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn kotlin_while_loop() {
        check_metrics::<KotlinParser>(
            "fun countdown(n: Int): Int { // +2 (+1 unit)
             var i = n
             while (i > 0) { // +1
                 i--
             }
             return i
         }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn kotlin_logical_operators() {
        check_metrics::<KotlinParser>(
            "fun check(a: Boolean, b: Boolean, c: Boolean): Boolean { // +2 (+1 unit)
             return a && b || c  // +1 &&, +1 ||
         }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn typescript_for_loop() {
        check_metrics::<TypescriptParser>(
            "function sum(n: number): number { // +2 (+1 unit)
             let s = 0;
             for (let i = 0; i < n; i++) { // +1
                 s += i;
             }
             return s;
         }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn typescript_while_loop() {
        check_metrics::<TypescriptParser>(
            "function countdown(n: number): number { // +2 (+1 unit)
             let i = n;
             while (i > 0) { // +1
                 i--;
             }
             return i;
         }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn typescript_logical_operators() {
        check_metrics::<TypescriptParser>(
            "function check(a: boolean, b: boolean, c: boolean): boolean { // +2 (+1 unit)
             return a && b || c;  // +1 &&, +1 ||
         }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn typescript_try_catch() {
        check_metrics::<TypescriptParser>(
            "function safe(x: number): number { // +2 (+1 unit)
             try {
                 return 1 / x;
             } catch (e) { // +1
                 return 0;
             }
         }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_for_loop() {
        check_metrics::<TsxParser>(
            "function sum(n: number): number { // +2 (+1 unit)
             let s = 0;
             for (let i = 0; i < n; i++) { // +1
                 s += i;
             }
             return s;
         }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_while_loop() {
        check_metrics::<TsxParser>(
            "function countdown(n: number): number { // +2 (+1 unit)
             let i = n;
             while (i > 0) { // +1
                 i--;
             }
             return i;
         }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_logical_operators() {
        check_metrics::<TsxParser>(
            "function check(a: boolean, b: boolean, c: boolean): boolean { // +2 (+1 unit)
             return a && b || c;  // +1 &&, +1 ||
         }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_try_catch() {
        check_metrics::<TsxParser>(
            "function safe(x: number): number { // +2 (+1 unit)
             try {
                 return 1 / x;
             } catch (e) { // +1
                 return 0;
             }
         }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_switch() {
        check_metrics::<TsxParser>(
            "function describe(x: number): string { // +2 (+1 unit)
             switch (x) {
                 case 1: // +1
                     return 'one';
                 case 2: // +1
                     return 'two';
                 default:
                     return 'other';
             }
         }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn php_1_level_nesting() {
        // Mirrors java_simple_class' if-inside-method shape:
        // unit (+1) + function (+1) + if (+1) + && (+1) = sum 4.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $a, int $b): bool {
                if ($a > 0 && $b > 0) {
                    return true;
                }
                return false;
            }",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_match_expression() {
        // Each `match_conditional_expression` arm (+1) but the default arm
        // does NOT add a branch (mirrors switch/case Java semantics).
        check_metrics::<PhpParser>(
            "<?php
            function color(string $c): int {
                return match ($c) {
                    'red' => 1,
                    'green' => 2,
                    'blue' => 3,
                    default => 0,
                };
            }",
            "foo.php",
            |metric| {
                // unit (+1) + function (+1) + 3 match arms (+3) = sum 5.
                // Default arm contributes 0.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_null_coalescing() {
        // `??` adds 1 (treated as a short-circuit branch). `??=` is an
        // augmented assignment, NOT a binary `??` — does not double-count.
        check_metrics::<PhpParser>(
            "<?php
            function pick($x, $y) {
                $a = $x ?? $y;
                $a ??= 0;
                return $a;
            }",
            "foo.php",
            |metric| {
                // unit (+1) + function (+1) + ?? (+1) = sum 3.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }
}
