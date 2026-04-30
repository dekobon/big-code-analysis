use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use crate::checker::Checker;
use crate::macros::implement_metric_trait;

use crate::*;

/// The `Nom` metric suite.
#[derive(Clone, Debug)]
pub struct Stats {
    functions: usize,
    closures: usize,
    functions_sum: usize,
    closures_sum: usize,
    functions_min: usize,
    functions_max: usize,
    closures_min: usize,
    closures_max: usize,
    space_count: usize,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            functions: 0,
            closures: 0,
            functions_sum: 0,
            closures_sum: 0,
            functions_min: usize::MAX,
            functions_max: 0,
            closures_min: usize::MAX,
            closures_max: 0,
            space_count: 1,
        }
    }
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("nom", 10)?;
        st.serialize_field("functions", &self.functions_sum())?;
        st.serialize_field("closures", &self.closures_sum())?;
        st.serialize_field("functions_average", &self.functions_average())?;
        st.serialize_field("closures_average", &self.closures_average())?;
        st.serialize_field("total", &self.total())?;
        st.serialize_field("average", &self.average())?;
        st.serialize_field("functions_min", &self.functions_min())?;
        st.serialize_field("functions_max", &self.functions_max())?;
        st.serialize_field("closures_min", &self.closures_min())?;
        st.serialize_field("closures_max", &self.closures_max())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "functions: {}, \
             closures: {}, \
             functions_average: {}, \
             closures_average: {}, \
             total: {} \
             average: {} \
             functions_min: {} \
             functions_max: {} \
             closures_min: {} \
             closures_max: {}",
            self.functions_sum(),
            self.closures_sum(),
            self.functions_average(),
            self.closures_average(),
            self.total(),
            self.average(),
            self.functions_min(),
            self.functions_max(),
            self.closures_min(),
            self.closures_max(),
        )
    }
}

impl Stats {
    /// Merges a second `Nom` metric suite into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.functions_min = self.functions_min.min(other.functions_min);
        self.functions_max = self.functions_max.max(other.functions_max);
        self.closures_min = self.closures_min.min(other.closures_min);
        self.closures_max = self.closures_max.max(other.closures_max);
        self.functions_sum += other.functions_sum;
        self.closures_sum += other.closures_sum;
        self.space_count += other.space_count;
    }

    /// Counts the number of function definitions in a scope
    #[inline(always)]
    pub fn functions(&self) -> f64 {
        // Only function definitions are considered, not general declarations
        self.functions as f64
    }

    /// Counts the number of closures in a scope
    #[inline(always)]
    pub fn closures(&self) -> f64 {
        self.closures as f64
    }

    /// Return the sum metric for functions
    #[inline(always)]
    pub fn functions_sum(&self) -> f64 {
        // Only function definitions are considered, not general declarations
        self.functions_sum as f64
    }

    /// Return the sum metric for closures
    #[inline(always)]
    pub fn closures_sum(&self) -> f64 {
        self.closures_sum as f64
    }

    /// Returns the average number of function definitions over all spaces
    #[inline(always)]
    pub fn functions_average(&self) -> f64 {
        self.functions_sum() / self.space_count as f64
    }

    /// Returns the average number of closures over all spaces
    #[inline(always)]
    pub fn closures_average(&self) -> f64 {
        self.closures_sum() / self.space_count as f64
    }

    /// Returns the average number of function definitions and closures over all spaces
    #[inline(always)]
    pub fn average(&self) -> f64 {
        self.total() / self.space_count as f64
    }

    /// Counts the number of function definitions in a scope
    #[inline(always)]
    pub fn functions_min(&self) -> f64 {
        // Only function definitions are considered, not general declarations
        self.functions_min as f64
    }

    /// Counts the number of closures in a scope
    #[inline(always)]
    pub fn closures_min(&self) -> f64 {
        self.closures_min as f64
    }
    /// Counts the number of function definitions in a scope
    #[inline(always)]
    pub fn functions_max(&self) -> f64 {
        // Only function definitions are considered, not general declarations
        self.functions_max as f64
    }

    /// Counts the number of closures in a scope
    #[inline(always)]
    pub fn closures_max(&self) -> f64 {
        self.closures_max as f64
    }
    /// Returns the total number of function definitions and
    /// closures in a scope
    #[inline(always)]
    pub fn total(&self) -> f64 {
        self.functions_sum() + self.closures_sum()
    }
    #[inline(always)]
    pub(crate) fn compute_sum(&mut self) {
        self.functions_sum += self.functions;
        self.closures_sum += self.closures;
    }
    #[inline(always)]
    pub(crate) fn compute_minmax(&mut self) {
        self.functions_min = self.functions_min.min(self.functions);
        self.functions_max = self.functions_max.max(self.functions);
        self.closures_min = self.closures_min.min(self.closures);
        self.closures_max = self.closures_max.max(self.closures);
        self.compute_sum();
    }
}

pub trait Nom
where
    Self: Checker,
{
    fn compute(node: &Node, stats: &mut Stats) {
        if Self::is_func(node) {
            stats.functions += 1;
            return;
        }
        if Self::is_closure(node) {
            stats.closures += 1;
        }
    }
}

implement_metric_trait!(
    [Nom],
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
    TclCode
);

#[cfg(test)]
mod tests {
    use crate::tools::check_metrics;

    use super::*;

    #[test]
    fn python_nom() {
        check_metrics::<PythonParser>(
            "def a():
                 pass
             def b():
                 pass
             def c():
                 pass
             x = lambda a : a + 42",
            "foo.py",
            |metric| {
                // Number of spaces = 4
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 3.0,
                      "closures": 1.0,
                      "functions_average": 0.75,
                      "closures_average": 0.25,
                      "total": 4.0,
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

    #[test]
    fn rust_nom() {
        check_metrics::<RustParser>(
            "mod A { fn foo() {}}
             mod B { fn foo() {}}
             let closure = |i: i32| -> i32 { i + 42 };",
            "foo.rs",
            |metric| {
                // Number of spaces = 4
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 2.0,
                      "closures": 1.0,
                      "functions_average": 0.5,
                      "closures_average": 0.25,
                      "total": 3.0,
                      "average": 0.75,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_nom() {
        check_metrics::<CppParser>(
            "int foo();

             int foo() {
                 return 0;
             }",
            "foo.c",
            |metric| {
                // Number of spaces = 2
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 1.0,
                      "average": 0.5,
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
    fn cpp_nom() {
        check_metrics::<CppParser>(
            "struct A {
                 void foo(int) {}
                 void foo(double) {}
             };
             int b = [](int x) -> int { return x + 42; };",
            "foo.cpp",
            |metric| {
                // Number of spaces = 4
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 2.0,
                      "closures": 1.0,
                      "functions_average": 0.5,
                      "closures_average": 0.25,
                      "total": 3.0,
                      "average": 0.75,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_nom() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 function foo(a) {
                     return a;
                 }
                 var bar = (function () {
                     var counter = 0;
                     return function () {
                         counter += 1;
                         return counter
                     }
                 })();
                 return bar(foo(a), a);
             }",
            "foo.js",
            |metric| {
                // Number of spaces = 5
                // functions: f, foo, bar
                // closures:  return function ()
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 3.0,
                      "closures": 1.0,
                      "functions_average": 0.6,
                      "closures_average": 0.2,
                      "total": 4.0,
                      "average": 0.8,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_call_nom() {
        check_metrics::<JavascriptParser>(
            "add_task(async function test_safe_mode() {
                 gAppInfo.inSafeMode = true;
             });",
            "foo.js",
            |metric| {
                // Number of spaces = 2
                // functions: test_safe_mode
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 1.0,
                      "average": 0.5,
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
    fn javascript_assignment_nom() {
        check_metrics::<JavascriptParser>(
            "AnimationTest.prototype.enableDisplay = function(element) {};",
            "foo.js",
            |metric| {
                // Number of spaces = 2
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 1.0,
                      "average": 0.5,
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
    fn javascript_labeled_nom() {
        check_metrics::<JavascriptParser>(
            "toJSON: function() {
                 return this.inspect(true);
             }",
            "foo.js",
            |metric| {
                // Number of spaces = 2
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 1.0,
                      "average": 0.5,
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
    fn javascript_labeled_arrow_nom() {
        check_metrics::<JavascriptParser>(
            "const dimConverters = {
                pt: x => x,
             };",
            "foo.js",
            |metric| {
                // Number of spaces = 2
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 1.0,
                      "average": 0.5,
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
    fn javascript_pair_nom() {
        check_metrics::<JavascriptParser>(
            "return {
                 initialize: function(object) {
                     this._object = object.toObject();
                 },
             }",
            "foo.js",
            |metric| {
                // Number of spaces = 2
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 1.0,
                      "average": 0.5,
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
    fn javascript_unnamed_nom() {
        check_metrics::<JavascriptParser>(
            "Ajax.getTransport = Try.these(
                 function() {
                     return function(){ return new XMLHttpRequest()}
                 }
             );",
            "foo.js",
            |metric| {
                // Number of spaces = 3
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 0.0,
                      "closures": 2.0,
                      "functions_average": 0.0,
                      "closures_average": 0.6666666666666666,
                      "total": 2.0,
                      "average": 0.6666666666666666,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_arrow_nom() {
        check_metrics::<JavascriptParser>(
            "var materials = [\"Hydrogen\"];
             materials.map(material => material.length);
             let add = (a, b)  => a + b;",
            "foo.js",
            |metric| {
                // Number of spaces = 3
                // Functions: add
                // Closures: material.map
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 1.0,
                      "functions_average": 0.3333333333333333,
                      "closures_average": 0.3333333333333333,
                      "total": 2.0,
                      "average": 0.6666666666666666,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_arrow_assignment_nom() {
        check_metrics::<JavascriptParser>("sink.onPull = () => { };", "foo.js", |metric| {
            // Number of spaces = 2
            insta::assert_json_snapshot!(
                metric.nom,
                @r###"
                    {
                      "functions": 1.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 1.0,
                      "average": 0.5,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn javascript_arrow_new_nom() {
        check_metrics::<JavascriptParser>(
            "const response = new Promise(resolve => channel.port1.onmessage = resolve);",
            "foo.js",
            |metric| {
                // Number of spaces = 2
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 0.0,
                      "closures": 1.0,
                      "functions_average": 0.0,
                      "closures_average": 0.5,
                      "total": 1.0,
                      "average": 0.5,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_arrow_call_nom() {
        check_metrics::<JavascriptParser>(
            "let notDisabled = TestUtils.waitForCondition(
                 () => !backbutton.hasAttribute(\"disabled\")
             );",
            "foo.js",
            |metric| {
                // Number of spaces = 2
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 0.0,
                      "closures": 1.0,
                      "functions_average": 0.0,
                      "closures_average": 0.5,
                      "total": 1.0,
                      "average": 0.5,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_nom() {
        check_metrics::<JavaParser>(
            "class A {
                public void foo(){
                    return;
                }
                public void bar(){
                    return;
                }
            }",
            "foo.java",
            |metric| {
                // Number of spaces = 4
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 2.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 2.0,
                      "average": 0.5,
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
    fn go_top_level_funcs() {
        check_metrics::<GoParser>(
            "package main
            func a() {}
            func b() {}
            func c() {}",
            "foo.go",
            |metric| {
                // Number of spaces = 4 (file unit + 3 funcs).
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 3.0,
                      "closures": 0.0,
                      "functions_average": 0.75,
                      "closures_average": 0.0,
                      "total": 3.0,
                      "average": 0.75,
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
    fn go_method_declaration() {
        check_metrics::<GoParser>(
            "package main
            type T struct{}
            func (r *T) M() {}",
            "foo.go",
            |metric| {
                // method_declaration is counted as a function.
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 1.0,
                      "average": 0.5,
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
    fn go_func_literal_is_closure() {
        check_metrics::<GoParser>(
            "package main
            var f = func() {}",
            "foo.go",
            |metric| {
                // func_literal increments closure count, not function count.
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 0.0,
                      "closures": 1.0,
                      "functions_average": 0.0,
                      "closures_average": 0.5,
                      "total": 1.0,
                      "average": 0.5,
                      "functions_min": 0.0,
                      "functions_max": 0.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_nested_closures() {
        check_metrics::<GoParser>(
            "package main
            func f() {
                inner := func() {
                    deeper := func() {}
                    _ = deeper
                }
                _ = inner
            }",
            "foo.go",
            |metric| {
                // 1 function (f) + 2 closures (inner, deeper).
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 2.0,
                      "functions_average": 0.25,
                      "closures_average": 0.5,
                      "total": 3.0,
                      "average": 0.75,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_closure_nom() {
        check_metrics::<JavaParser>(
            "interface printable{
                void print();
              }

              interface IntFunc {
                int func(int n);
              }

              class Printer implements printable{
                public void print(){System.out.println(\"Hello\");}

                public static void main(String args[]){
                  Printer  obj = new Printer();
                  obj.print();
                  IntFunc meaning = (i) -> i + 42;
                  int i = meaning.func(1);
                }
              }",
            "foo.java",
            |metric| {
                // Number of spaces = 8
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 4.0,
                      "closures": 1.0,
                      "functions_average": 0.5,
                      "closures_average": 0.125,
                      "total": 5.0,
                      "average": 0.625,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn perl_nom() {
        check_metrics::<PerlParser>(
            "sub a { 1 }
             sub b { 2 }
             my $c = sub { 3 };
             sub outer {
                 my $inner = sub { 4 };
                 return $inner;
             }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r#"
                {
                  "functions": 3.0,
                  "closures": 2.0,
                  "functions_average": 0.5,
                  "closures_average": 0.3333333333333333,
                  "total": 5.0,
                  "average": 0.8333333333333334,
                  "functions_min": 0.0,
                  "functions_max": 1.0,
                  "closures_min": 0.0,
                  "closures_max": 1.0
                }
                 "#
                );
            },
        );
    }

    #[test]
    fn tsx_named_and_arrow_functions() {
        check_metrics::<TsxParser>(
            "function greet(name: string): string {
                 return `Hello, ${name}`;
             }
             const add = (a: number, b: number) => a + b;
             const log = () => { console.log('done'); };",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 3.0,
                      "closures": 0.0,
                      "functions_average": 0.75,
                      "closures_average": 0.0,
                      "total": 3.0,
                      "average": 0.75,
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
    fn typescript_named_arrow_and_class_methods() {
        check_metrics::<TypescriptParser>(
            "function compute(x: number): number {
                 return x * 2;
             }
             const double = (n: number): number => n * 2;
             class Calculator {
                 add(a: number, b: number): number {
                     return a + b;
                 }
             }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 3.0,
                      "closures": 0.0,
                      "functions_average": 0.6,
                      "closures_average": 0.0,
                      "total": 3.0,
                      "average": 0.6,
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
    fn mozjs_nom() {
        check_metrics::<MozjsParser>(
            "function f(a, b) {
                 function foo(a) {
                     return a;
                 }
                 var bar = (function () {
                     var counter = 0;
                     return function () {
                         counter += 1;
                         return counter
                     }
                 })();
                 return bar(foo(a), a);
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 3.0,
                      "closures": 1.0,
                      "functions_average": 0.6,
                      "closures_average": 0.2,
                      "total": 4.0,
                      "average": 0.8,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_arrow_and_method() {
        check_metrics::<MozjsParser>(
            "let add = (a, b) => a + b;
             class Counter {
                 increment() {
                     this.count++;
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 2.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 2.0,
                      "average": 0.5,
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
    fn kotlin_nom_class_with_methods() {
        check_metrics::<KotlinParser>(
            "class Calculator {
                fun add(a: Int, b: Int): Int {
                    return a + b
                }
                fun subtract(a: Int, b: Int): Int {
                    return a - b
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 2.0,
                      "closures": 0.0,
                      "functions_average": 0.5,
                      "closures_average": 0.0,
                      "total": 2.0,
                      "average": 0.5,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn lua_nom() {
        check_metrics::<LuaParser>(
            "function greet(name)
  return \"hello \" .. name
end

local add = function(a, b)
  return a + b
end

local function outer()
  local inner = function()
    return 42
  end
  return inner()
end",
            "foo.lua",
            |metric| {
                // 2 named functions (greet, outer), 2 closures (add, inner)
                insta::assert_json_snapshot!(metric.nom, @r###"
                    {
                      "functions": 2.0,
                      "closures": 2.0,
                      "functions_average": 0.4,
                      "closures_average": 0.4,
                      "total": 4.0,
                      "average": 0.8,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 1.0
                    }
                    "###);
            },
        );
    }

    #[test]
    fn bash_nom() {
        check_metrics::<BashParser>(
            "#!/bin/bash
foo() {
    echo 'hello'
}
bar() {
    echo 'world'
}
foo
bar",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r#"
                    {
                      "functions": 2.0,
                      "closures": 0.0,
                      "functions_average": 0.6666666666666666,
                      "closures_average": 0.0,
                      "total": 2.0,
                      "average": 0.6666666666666666,
                      "functions_min": 0.0,
                      "functions_max": 1.0,
                      "closures_min": 0.0,
                      "closures_max": 0.0
                    }
                    "#
                );
            },
        );
    }

    #[test]
    fn tcl_nom() {
        check_metrics::<TclParser>(
            "proc foo {a} { puts $a }
proc bar {x y} { puts $x }
foo 1
bar 2 3",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn tcl_nested_nom() {
        check_metrics::<TclParser>(
            "proc outer {a} {
    proc inner {x} { puts $x }
    inner $a
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn typescript_class_methods() {
        check_metrics::<TypescriptParser>(
            "class Calc {
             add(a: number, b: number): number { return a + b; }
             sub(a: number, b: number): number { return a - b; }
         }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn typescript_arrow_and_function() {
        check_metrics::<TypescriptParser>(
            "function f(): number { return 1; }
         const g = (): number => 2;
         const h = (x: number): number => x * 2;",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn tsx_class_methods() {
        check_metrics::<TsxParser>(
            "class Calc {
             add(a: number, b: number): number { return a + b; }
             sub(a: number, b: number): number { return a - b; }
         }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn tsx_arrow_and_function() {
        check_metrics::<TsxParser>(
            "function f(): number { return 1; }
         const g = (): number => 2;
         const h = (x: number): number => x * 2;",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn bash_multiple_functions_nom() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    echo hello
}
g() {
    echo world
}",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn bash_nested_functions_nom() {
        check_metrics::<BashParser>(
            "#!/bin/bash
outer() {
    inner() {
        echo inner
    }
    inner
}",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn mozjs_nested_function_nom() {
        check_metrics::<MozjsParser>(
            "function outer() {
             function inner() {
                 return 1;
             }
             return inner();
         }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn mozjs_class_methods_nom() {
        check_metrics::<MozjsParser>(
            "class Calc {
             add(a, b) { return a + b; }
             sub(a, b) { return a - b; }
         }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn mozjs_iife_nom() {
        check_metrics::<MozjsParser>(
            "(function() {
             var x = 1;
             return x;
         })();",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn kotlin_class_methods_nom() {
        check_metrics::<KotlinParser>(
            "class Calc {
             fun add(a: Int, b: Int): Int = a + b
             fun sub(a: Int, b: Int): Int = a - b
         }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn kotlin_lambda_nom() {
        check_metrics::<KotlinParser>(
            "fun f(list: List<Int>): Int {
             val double = { x: Int -> x * 2 }
             return list.sumOf(double)
         }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }
}
