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
             total: {}, \
             average: {}, \
             functions_min: {}, \
             functions_max: {}, \
             closures_min: {}, \
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
    #[inline]
    #[must_use]
    pub fn functions(&self) -> f64 {
        // Only function definitions are considered, not general declarations
        self.functions as f64
    }

    /// Counts the number of closures in a scope
    #[inline]
    #[must_use]
    pub fn closures(&self) -> f64 {
        self.closures as f64
    }

    /// Return the sum metric for functions
    #[inline]
    #[must_use]
    pub fn functions_sum(&self) -> f64 {
        // Only function definitions are considered, not general declarations
        self.functions_sum as f64
    }

    /// Return the sum metric for closures
    #[inline]
    #[must_use]
    pub fn closures_sum(&self) -> f64 {
        self.closures_sum as f64
    }

    /// Returns the average number of function definitions over all spaces
    #[inline]
    #[must_use]
    pub fn functions_average(&self) -> f64 {
        self.functions_sum() / self.space_count as f64
    }

    /// Returns the average number of closures over all spaces
    #[inline]
    #[must_use]
    pub fn closures_average(&self) -> f64 {
        self.closures_sum() / self.space_count as f64
    }

    /// Returns the average number of function definitions and closures over all spaces
    #[inline]
    #[must_use]
    pub fn average(&self) -> f64 {
        self.total() / self.space_count as f64
    }

    /// Counts the number of function definitions in a scope.
    ///
    /// Collapses the `usize::MAX` sentinel that `Stats::default()` plants
    /// into `functions_min` to `0.0`, so a never-observed space
    /// serializes to a meaningful number rather than `1.8446744e19`.
    #[inline]
    #[must_use]
    pub fn functions_min(&self) -> f64 {
        // Only function definitions are considered, not general declarations
        if self.functions_min == usize::MAX {
            0.0
        } else {
            self.functions_min as f64
        }
    }

    /// Counts the number of closures in a scope.
    ///
    /// Same `usize::MAX` sentinel collapse as `functions_min`.
    #[inline]
    #[must_use]
    pub fn closures_min(&self) -> f64 {
        if self.closures_min == usize::MAX {
            0.0
        } else {
            self.closures_min as f64
        }
    }
    /// Counts the number of function definitions in a scope
    #[inline]
    #[must_use]
    pub fn functions_max(&self) -> f64 {
        // Only function definitions are considered, not general declarations
        self.functions_max as f64
    }

    /// Counts the number of closures in a scope
    #[inline]
    #[must_use]
    pub fn closures_max(&self) -> f64 {
        self.closures_max as f64
    }
    /// Returns the total number of function definitions and
    /// closures in a scope
    #[inline]
    #[must_use]
    pub fn total(&self) -> f64 {
        self.functions_sum() + self.closures_sum()
    }
    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.functions_sum += self.functions;
        self.closures_sum += self.closures;
    }
    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.functions_min = self.functions_min.min(self.functions);
        self.functions_max = self.functions_max.max(self.functions);
        self.closures_min = self.closures_min.min(self.closures);
        self.closures_max = self.closures_max.max(self.closures);
        self.compute_sum();
    }
}

#[doc(hidden)]
/// Per-language counting of methods (functions + closures).
pub trait Nom
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
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
    TclCode,
    PhpCode,
    CsharpCode,
    ElixirCode,
    RubyCode,
    GroovyCode,
    IrulesCode
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

    /// Regression for #227: a `Stats::default()` that never sees an
    /// observation must not leak the `usize::MAX` sentinel for
    /// `functions_min` or `closures_min`. Both getters collapse the
    /// sentinel to `0.0` so JSON never emits `1.8446744e19`.
    #[test]
    fn nom_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.functions_min(), 0.0);
        assert_eq!(stats.closures_min(), 0.0);
    }

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
                // The `lambda` is detected as a closure via
                // `PythonCode::is_closure` (widened to accept both
                // aliased kind_ids in #419); pin the count explicitly
                // so a regression in the predicate fails loudly.
                assert_eq!(metric.nom.closures_sum(), 1.0);
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

    /// Free functions and member functions both surface as
    /// `Cpp::FunctionDefinition` and count toward `functions`.  Member
    /// functions are nested inside a struct/class space; the count is on
    /// the function-definition node itself, not on the enclosing scope.
    #[test]
    fn cpp_free_and_member_functions() {
        check_metrics::<CppParser>(
            "int free_fn(int x) { return x; }
             struct S {
                 int member_fn(int x) { return x + 1; }
             };",
            "foo.cpp",
            |metric| {
                // 2 functions: `free_fn`, `S::member_fn`.
                let s = &metric.nom;
                assert_eq!(s.functions_sum(), 2.0);
                assert_eq!(s.closures_sum(), 0.0);
                assert_eq!(s.total(), 2.0);
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    /// `static` member functions still surface as `Cpp::FunctionDefinition`
    /// — the `static` keyword is a storage-class specifier, not a separate
    /// node kind — so they are counted just like non-static members.
    #[test]
    fn cpp_static_member_function() {
        check_metrics::<CppParser>(
            "struct S {
                 static int factory(int x) { return x; }
                 int method(int x) { return x + 1; }
             };",
            "foo.cpp",
            |metric| {
                let s = &metric.nom;
                assert_eq!(s.functions_sum(), 2.0);
                assert_eq!(s.closures_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    /// Constructor and destructor definitions surface as
    /// `Cpp::FunctionDefinition` nodes with a `function_declarator` whose
    /// identifier is the class name (ctor) or `~ClassName` (dtor).  Both
    /// count as functions.
    #[test]
    fn cpp_constructor_and_destructor() {
        check_metrics::<CppParser>(
            "struct S {
                 S() {}
                 ~S() {}
                 int method() { return 0; }
             };",
            "foo.cpp",
            |metric| {
                let s = &metric.nom;
                // 3 functions: S(), ~S(), method.
                assert_eq!(s.functions_sum(), 3.0);
                assert_eq!(s.closures_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    /// Operator overloads surface as `FunctionDefinition` whose declarator
    /// has an `OperatorName` identifier (`operator+`, `operator==`).  Both
    /// inline overloads count toward `functions`.
    #[test]
    fn cpp_operator_overloads() {
        check_metrics::<CppParser>(
            "struct V {
                 int x;
                 V operator+(const V& o) const { return V{x + o.x}; }
                 bool operator==(const V& o) const { return x == o.x; }
             };",
            "foo.cpp",
            |metric| {
                let s = &metric.nom;
                assert_eq!(s.functions_sum(), 2.0);
                assert_eq!(s.closures_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    /// Function-template definition counts as a single function — the
    /// `template<>` prefix wraps a `FunctionDefinition` and does not
    /// produce additional function-definition nodes.
    #[test]
    fn cpp_function_template() {
        check_metrics::<CppParser>(
            "template<typename T>
             T identity(T x) { return x; }",
            "foo.cpp",
            |metric| {
                let s = &metric.nom;
                assert_eq!(s.functions_sum(), 1.0);
                assert_eq!(s.closures_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    /// Class-template member functions defined in-line each count as one
    /// function.  The `template<>` head wraps the class, and the methods
    /// inside it surface as ordinary `FunctionDefinition` nodes.
    #[test]
    fn cpp_class_template_members() {
        check_metrics::<CppParser>(
            "template<typename T>
             struct Box {
                 T value;
                 T get() const { return value; }
                 void set(T v) { value = v; }
             };",
            "foo.cpp",
            |metric| {
                let s = &metric.nom;
                assert_eq!(s.functions_sum(), 2.0);
                assert_eq!(s.closures_sum(), 0.0);
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    /// Lambdas inside a function body count as `closures`, not as
    /// `functions` — Cpp::LambdaExpression is the closure kind.  The
    /// enclosing function adds 1 to `functions`; each lambda adds 1 to
    /// `closures`.
    #[test]
    fn cpp_lambdas_inside_function() {
        check_metrics::<CppParser>(
            "int run() {
                 auto add = [](int a, int b) { return a + b; };
                 auto mul = [](int a, int b) { return a * b; };
                 return add(1, 2) + mul(3, 4);
             }",
            "foo.cpp",
            |metric| {
                let s = &metric.nom;
                // 1 enclosing function + 2 closures.
                assert_eq!(s.functions_sum(), 1.0);
                assert_eq!(s.closures_sum(), 2.0);
                assert_eq!(s.total(), 3.0);
                insta::assert_json_snapshot!(metric.nom);
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

    fn check_returned_object_arrow_nom<T: ParserTrait>(file_name: &str) {
        check_metrics::<T>(
            "function f() { return { foo: x => x }; }",
            file_name,
            |metric| {
                insta::allow_duplicates! {
                    insta::assert_json_snapshot!(
                        metric.nom,
                        @r###"
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
                        }"###
                    );
                }
            },
        );
    }

    #[test]
    fn javascript_returned_object_arrow_nom() {
        check_returned_object_arrow_nom::<JavascriptParser>("foo.js");
    }

    #[test]
    fn mozjs_returned_object_arrow_nom() {
        check_returned_object_arrow_nom::<MozjsParser>("foo.js");
    }

    #[test]
    fn typescript_returned_object_arrow_nom() {
        check_returned_object_arrow_nom::<TypescriptParser>("foo.ts");
    }

    #[test]
    fn tsx_returned_object_arrow_nom() {
        check_returned_object_arrow_nom::<TsxParser>("foo.tsx");
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
    fn csharp_nom() {
        check_metrics::<CsharpParser>(
            "class A {
                public void Foo() {
                    return;
                }
                public void Bar() {
                    return;
                }
                public int X { get; set; }
                public void Outer() {
                    void Local() { return; }
                    Local();
                }
            }",
            "foo.cs",
            |metric| {
                // Methods: Foo, Bar, Outer (=3 explicit)
                // Plus accessors `get`, `set` on X = 2 more functions
                // Plus `Local` local function = 1 more
                // Total functions = 6
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 6.0,
                      "closures": 0.0,
                      "functions_average": 0.75,
                      "closures_average": 0.0,
                      "total": 6.0,
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
    fn csharp_closure_nom() {
        check_metrics::<CsharpParser>(
            "class A {
                public void Run() {
                    System.Func<int, int> f = x => x + 1;
                    System.Action g = delegate(int y) { System.Console.WriteLine(y); };
                }
            }",
            "foo.cs",
            |metric| {
                // 1 method (Run), 1 lambda, 1 anonymous_method = 1 func + 2 closures.
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 2.0,
                      "functions_average": 0.2,
                      "closures_average": 0.4,
                      "total": 3.0,
                      "average": 0.6,
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
    fn csharp_indexer_nom() {
        // A bodied indexer defines two callable accessors (`get`, `set`).
        // Before #464 the `indexer_declaration` node itself ALSO opened a
        // function space, triple-counting the indexer as 3 functions. The
        // correct count is 2 — the accessor count — matching the npm path
        // (`csharp_count_member`) which reports `class_methods == 2`.
        check_metrics::<CsharpParser>(
            "class A {
                private int[] _d;
                public int this[int i] { get => _d[i]; set => _d[i] = value; }
            }",
            "foo.cs",
            |metric| {
                // expected: get + set accessors = 2 functions; the
                // IndexerDeclaration node no longer opens its own space.
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
    fn csharp_expression_bodied_indexer_nom() {
        // An expression-bodied indexer (`this[int i] => _d[i];`) has NO
        // `accessor_declaration` child — it defines a single implicit
        // getter. Removing the IndexerDeclaration entry from is_func /
        // is_func_space outright would drop this to 0; the #464 fix gates
        // the entry on the absence of accessors so this form still counts
        // as 1, matching the npm `.max(1)` fallback.
        check_metrics::<CsharpParser>(
            "class A {
                private int[] _d;
                public int this[int i] => _d[i];
            }",
            "foo.cs",
            |metric| {
                // expected: one implicit getter, no accessor nodes => 1.
                assert_eq!(metric.nom.functions_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 1.0,
                      "closures": 0.0,
                      "functions_average": 0.3333333333333333,
                      "closures_average": 0.0,
                      "total": 1.0,
                      "average": 0.3333333333333333,
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
    fn csharp_property_nom() {
        // A bodied property (`int W { get => _w; set => _w = value; }`)
        // defines two callable accessors. The `property_declaration` node
        // must NOT open its own space on top of them, else it double-counts
        // (the property analogue of #464). The correct count is 2 — the
        // accessor count — matching the npm path which reports 2.
        check_metrics::<CsharpParser>(
            "class A {
                private int _w;
                public int W { get => _w; set => _w = value; }
            }",
            "foo.cs",
            |metric| {
                // expected: get + set accessors = 2 functions; the
                // PropertyDeclaration node defers and opens no space (#472).
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
            },
        );
    }

    #[test]
    fn csharp_auto_property_nom() {
        // An auto-property (`int Y { get; set; }`) still has two
        // `accessor_declaration` children, so it defers to them exactly like
        // a bodied property — the #472 gate must not change this count.
        check_metrics::<CsharpParser>(
            "class A {
                public int Y { get; set; }
            }",
            "foo.cs",
            |metric| {
                // expected: get + set accessors = 2 functions, unchanged.
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
            },
        );
    }

    #[test]
    fn csharp_expression_bodied_property_nom() {
        // An expression-bodied property (`int W => _w;`) has NO
        // `accessor_declaration` child — it defines a single implicit
        // getter via an `arrow_expression_clause`. With no accessor to
        // defer to, the `property_declaration` opened no space at all
        // before #472 (0 functions). The fix gates the entry on the
        // absence of accessors so it counts as 1, matching the npm
        // `.max(1)` fallback.
        check_metrics::<CsharpParser>(
            "class A {
                private int _w;
                public int W => _w;
            }",
            "foo.cs",
            |metric| {
                // expected: one implicit getter, no accessor nodes => 1.
                assert_eq!(metric.nom.functions_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
    fn groovy_nom() {
        check_metrics::<GroovyParser>(
            "class Printer {
                void print() {
                    println 'hello'
                }
                static void main(String[] args) {
                    def p = new Printer()
                    p.print()
                    def doubler = { x -> x * 2 }
                    int r = doubler(21)
                }
            }",
            "foo.groovy",
            |metric| {
                // Two methods declared. The dekobon grammar parses
                // method bodies as `block`, distinct from `closure`, so
                // the explicit `doubler = { x -> x * 2 }` literal is the
                // only closure here (unlike the prior amaanq grammar
                // which mis-parsed each method body as a `closure`
                // node too).
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_nom_function_definition() {
        // `def foo() {}` at top level uses `function_definition`, not
        // `method_declaration`. Nom must count it as a function.
        check_metrics::<GroovyParser>(
            "def greet(name) {
                println(name)
            }
            greet('world')",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1.0);
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
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 3.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 3.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 0.0);
                assert_eq!(metric.nom.closures_sum(), 1.0);
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
                assert_eq!(metric.nom.functions_sum(), 2.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
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
                assert_eq!(metric.nom.functions_sum(), 1.0);
                assert_eq!(metric.nom.closures_sum(), 1.0);
                insta::assert_json_snapshot!(metric.nom);
            },
        );
    }

    #[test]
    fn php_nom() {
        // Top-level function + 2 methods inside a class + 1 anonymous +
        // 1 arrow = 3 functions, 2 closures.
        check_metrics::<PhpParser>(
            "<?php
            function top(): void {}
            class A {
                public function m1(): void {}
                public function m2(): int {
                    $f = function () { return 1; };
                    $g = fn () => 2;
                    return $f() + $g();
                }
            }",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nom,
                    @r###"
                    {
                      "functions": 3.0,
                      "closures": 2.0,
                      "functions_average": 0.42857142857142855,
                      "closures_average": 0.2857142857142857,
                      "total": 5.0,
                      "average": 0.7142857142857143,
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
    fn php_nom_anonymous_class() {
        // Methods inside `new class { … }` count toward the closure-style
        // space mechanism: anonymous_class is its own space and its
        // method_declaration children are counted as functions.
        check_metrics::<PhpParser>(
            "<?php
            function f(): object {
                return new class {
                    public function inner(): int { return 1; }
                };
            }",
            "foo.php",
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

    // Documents Elixir's current default-impl behaviour: `def`/`defp`
    // surface as `Call` nodes whose target is an `Identifier`, and
    // `is_func` returns `false`, so Nom only counts `AnonymousFunction`
    // closures. Two anon functions + zero functions is the load-bearing
    // claim — a future real impl that started counting `def` calls
    // would flip this. The same call-target text-inspection pattern
    // that #179 introduced for `Cyclomatic` would apply here once
    // `Nom::compute` is widened to take the source bytes.
    #[test]
    fn elixir_default_nom_counts_only_anonymous_functions() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def public_fn(x), do: x + 1\n  defp private_fn(x), do: x - 1\n  def with_anon do\n    inc = fn x -> x + 1 end\n    dec = fn x -> x - 1 end\n    {inc, dec}\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 0.0);
                assert_eq!(metric.nom.closures_sum(), 2.0);
            },
        );
    }

    #[test]
    fn stats_display_commas_between_all_fields() {
        let stats = Stats {
            functions: 0,
            closures: 0,
            functions_sum: 3,
            closures_sum: 1,
            functions_min: 0,
            functions_max: 2,
            closures_min: 0,
            closures_max: 1,
            space_count: 2,
        };
        let formatted = format!("{stats}");

        // Every adjacent pair of labels must appear with ", " between the
        // previous field's value and the next label.
        let expected_fragments = [
            "functions: 3, closures: 1",
            "closures: 1, functions_average:",
            "functions_average: 1.5, closures_average:",
            "closures_average: 0.5, total:",
            "total: 4, average:",
            "average: 2, functions_min:",
            "functions_min: 0, functions_max:",
            "functions_max: 2, closures_min:",
            "closures_min: 0, closures_max:",
        ];

        for fragment in expected_fragments {
            assert!(
                formatted.contains(fragment),
                "missing fragment {fragment:?} in: {formatted}"
            );
        }
    }

    #[test]
    fn ruby_nom() {
        // expected: total = 4 (2 methods `add`/`mul` + 1 singleton
        // method `self.factory` + 1 block argument to `each`).
        // `functions` counts only the named `Method` / `SingletonMethod`
        // forms (3); `closures` counts `Block` / `DoBlock` / `Lambda`
        // (1).
        check_metrics::<RubyParser>(
            "class C\n  def add(a, b)\n    a + b\n  end\n  def mul(a, b)\n    a * b\n  end\n  def self.factory\n    new\n  end\nend\n\n[1, 2, 3].each { |x| puts x }\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 3.0);
                assert_eq!(metric.nom.closures_sum(), 1.0);
                assert_eq!(metric.nom.total(), 4.0);
            },
        );
    }

    #[test]
    fn ruby_stabby_lambda_single_closure() {
        // A stabby lambda `->(z) { … }` parses as a `Lambda` node that
        // contains a `Block` for its body. `is_closure` must count the
        // pair as ONE closure, not two (#465). Revert-verified: counting
        // the inner `Block` again yields closures_sum == 2.0.
        check_metrics::<RubyParser>("f = ->(z) { z + 1 }\n", "stabby.rb", |metric| {
            assert_eq!(metric.nom.functions_sum(), 0.0);
            assert_eq!(metric.nom.closures_sum(), 1.0);
        });
    }

    #[test]
    fn ruby_stabby_lambda_multi_statement_single_closure() {
        // A multi-statement body does not change the structure: still one
        // `Lambda` wrapping one `Block`, so still one closure.
        check_metrics::<RubyParser>(
            "f = ->(z) {\n  y = z + 1\n  y * 2\n}\n",
            "stabby_multi.rb",
            |metric| {
                assert_eq!(metric.nom.closures_sum(), 1.0);
            },
        );
    }

    #[test]
    fn ruby_stabby_lambda_do_block_single_closure() {
        // The `do … end` body form of a stabby lambda parses as a `Lambda`
        // wrapping a `DoBlock`; both must collapse to one closure.
        check_metrics::<RubyParser>("f = ->(z) do\n  z + 1\nend\n", "stabby_do.rb", |metric| {
            assert_eq!(metric.nom.closures_sum(), 1.0);
        });
    }

    #[test]
    fn ruby_keyword_lambda_single_closure() {
        // The keyword forms `lambda { }` / `proc { }` parse as a `Call`
        // carrying a `Block` argument (the parent is a `Call`, not a
        // `Lambda`), so they must still count exactly one closure. Guards
        // against the #465 fix regressing the keyword form to zero.
        check_metrics::<RubyParser>(
            "g = lambda { |z| z + 1 }\nh = proc { |z| z + 1 }\n",
            "keyword.rb",
            |metric| {
                assert_eq!(metric.nom.closures_sum(), 2.0);
            },
        );
    }

    /// iRules counts event handlers (`when` / `on` / `trap`) and `proc`
    /// definitions as functions; the language has no closures. A file with
    /// two handlers and one proc reports three functions, zero closures.
    /// Confirms the handlers-as-functions decision end to end.
    #[test]
    fn irules_nom_handlers_and_procs() {
        check_metrics::<IrulesParser>(
            "when CLIENT_ACCEPTED {
    log local0. \"connected\"
}
when HTTP_REQUEST {
    log local0. [HTTP::uri]
}
proc helper { x } {
    return $x
}
",
            "foo.irule",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 3.0);
                assert_eq!(metric.nom.closures_sum(), 0.0);
                assert_eq!(metric.nom.total(), 3.0);
            },
        );
    }
}
