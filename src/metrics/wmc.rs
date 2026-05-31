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

/// The `Wmc` metric.
///
/// This metric sums the cyclomatic complexities of all the methods defined in a class.
/// The `Wmc` (Weighted Methods per Class) is an object-oriented metric for classes.
///
/// Original paper and definition:
/// <https://www.researchgate.net/publication/3187649_Kemerer_CF_A_metric_suite_for_object_oriented_design_IEEE_Trans_Softw_Eng_206_476-493>
#[derive(Debug, Clone, Default)]
pub struct Stats {
    cyclomatic: f64,
    // Cumulative cyclomatic carried by descendant Class / Interface
    // spaces (anonymous classes, nested object literals, …). A method
    // that *contains* a nested class must not fold that class's
    // complexity into its enclosing class's WMC — the nested class is
    // its own WMC scope and already counts those methods. Tracking the
    // nested-class cyclomatic lets `merge` subtract it from a Function's
    // contribution, preventing double-attribution (#463).
    nested_class_cyclomatic: f64,
    class_wmc: f64,
    interface_wmc: f64,
    class_wmc_sum: f64,
    interface_wmc_sum: f64,
    space_kind: SpaceKind,
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("wmc", 3)?;
        st.serialize_field("classes", &self.class_wmc_sum())?;
        st.serialize_field("interfaces", &self.interface_wmc_sum())?;
        st.serialize_field("total", &self.total_wmc())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "classes: {}, interfaces: {}, total: {}",
            self.class_wmc_sum(),
            self.interface_wmc_sum(),
            self.total_wmc()
        )
    }
}

impl Stats {
    /// Merges a second `Wmc` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        use SpaceKind::*;

        // Rolls a child space's cyclomatic into the enclosing class /
        // interface WMC, subtracting any nested-class complexity so it is
        // not double-counted (#463). See the per-arm rationale below.
        match other.space_kind {
            // A method contributes its own cyclomatic minus the
            // complexity already claimed by nested class / interface
            // spaces it contains. Those nested classes form their own WMC
            // scope (their members roll up via `class_wmc_sum`), so the
            // method must not re-add them. Its nested-class total also
            // bubbles up so an *ancestor* method can exclude this whole
            // subtree in turn.
            Function => {
                let own_cyclomatic = other.cyclomatic - other.nested_class_cyclomatic;
                match self.space_kind {
                    Class => self.class_wmc += own_cyclomatic,
                    Interface => self.interface_wmc += own_cyclomatic,
                    _ => {}
                }
                self.nested_class_cyclomatic += other.nested_class_cyclomatic;
            }
            // A nested Class / Interface space (e.g. an anonymous class)
            // contributes its *cumulative* cyclomatic — which already
            // subsumes any classes nested inside it — so we record only
            // `other.cyclomatic` here, never also its
            // `nested_class_cyclomatic` (that would double-count the
            // inner classes, see #463 nested-anonymous case).
            Class | Interface => self.nested_class_cyclomatic += other.cyclomatic,
            _ => {}
        }

        self.class_wmc_sum += other.class_wmc_sum;
        self.interface_wmc_sum += other.interface_wmc_sum;
    }

    /// Returns the `Wmc` metric value of the classes in a space.
    #[inline]
    #[must_use]
    pub fn class_wmc(&self) -> f64 {
        self.class_wmc
    }

    /// Returns the `Wmc` metric value of the interfaces in a space.
    #[inline]
    #[must_use]
    pub fn interface_wmc(&self) -> f64 {
        self.interface_wmc
    }

    /// Returns the sum of the `Wmc` metric values of the classes in a space.
    #[inline]
    #[must_use]
    pub fn class_wmc_sum(&self) -> f64 {
        self.class_wmc_sum
    }

    /// Returns the sum of the `Wmc` metric values of the interfaces in a space.
    #[inline]
    #[must_use]
    pub fn interface_wmc_sum(&self) -> f64 {
        self.interface_wmc_sum
    }

    /// Returns the total `Wmc` metric value in a space.
    #[inline]
    #[must_use]
    pub fn total_wmc(&self) -> f64 {
        self.class_wmc_sum() + self.interface_wmc_sum()
    }

    // Accumulates the `Wmc` metric values
    // of classes and interfaces into the sums
    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.class_wmc_sum += self.class_wmc;
        self.interface_wmc_sum += self.interface_wmc;
    }

    // Checks if the `Wmc` metric is disabled
    #[inline]
    pub(crate) fn is_disabled(&self) -> bool {
        matches!(self.space_kind, SpaceKind::Function | SpaceKind::Unknown)
    }
}

#[doc(hidden)]
/// Per-language computation of weighted methods per class.
pub trait Wmc
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats);
}

// Shared WMC compute for languages with class / interface / function /
// unit space kinds (Java, C#, Kotlin). Records the space kind once and,
// for function spaces, captures the cyclomatic sum so the aggregator can
// roll it into the enclosing class / interface.
fn class_interface_compute(
    space_kind: SpaceKind,
    cyclomatic: &cyclomatic::Stats,
    stats: &mut Stats,
) {
    use SpaceKind::*;

    if let Unit | Class | Interface | Function = space_kind {
        if stats.space_kind == Unknown {
            stats.space_kind = space_kind;
        }
        // Record the cumulative cyclomatic for Function spaces (the
        // method's WMC contribution) and for Class / Interface spaces
        // (so an ancestor method can subtract a nested class's
        // complexity from its own contribution — see `merge`, #463).
        if let Function | Class | Interface = space_kind {
            stats.cyclomatic = cyclomatic.cyclomatic_sum();
        }
    }
}

impl Wmc for JavaCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

impl Wmc for GroovyCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

impl Wmc for CsharpCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

// Kotlin's `class_declaration` becomes either `Class` or `Interface` via
// `Getter::get_space_kind` (the keyword child disambiguates). `object`
// singletons map to `Class`. Function spaces (top-level `fun`, member
// `fun`, secondary constructors, lambdas, anonymous functions) contribute
// their cyclomatic to the enclosing class / interface. `companion_object`
// is not a `func_space`, so its members fold into the surrounding class —
// matching Kotlin's "static members" semantics.
impl Wmc for KotlinCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

impl Wmc for PhpCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        use SpaceKind::*;

        // Anonymous classes, enums, and traits all map to `Class` via
        // `Getter::get_space_kind`, so a single `Class` arm covers them.
        if let Unit | Class | Interface | Function = space_kind {
            if stats.space_kind == Unknown {
                stats.space_kind = space_kind;
            }
            // Record cyclomatic for Function spaces (the method's WMC
            // contribution) and for Class / Interface spaces (so an
            // ancestor method can exclude a nested class's complexity —
            // see `merge`, #463; matters for PHP `AnonymousClass` nested
            // inside a method).
            if let Function | Class | Interface = space_kind {
                stats.cyclomatic = cyclomatic.cyclomatic_sum();
            }
        }
    }
}

// Python WMC. The shared `class_interface_compute` already does the
// right thing for the four space kinds Python produces:
// - `Unit` (module-level — receives WMC totals from top-level
//   classes, mirroring the Java unit-space aggregation).
// - `Class` (every `ClassDefinition`).
// - `Function` (every `FunctionDefinition`; captures the
//   per-function cyclomatic sum that the aggregator rolls up into
//   the enclosing class).
// - `Unknown` (anything else — skipped).
//
// Lambdas (`Lambda`) are not `is_func` and therefore do not open a
// `Function` space, so they correctly do *not* contribute to WMC
// — they are anonymous expressions, not methods.
impl Wmc for PythonCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

// Rust WMC. Rust's `Impl` / `Trait` space kinds map onto the OO
// "class" / "interface" concept for WMC purposes: every `impl` block
// is a class, every `trait` is an interface, and each `function_item`
// inside contributes its cyclomatic complexity to the surrounding
// space.
//
// `class_interface_compute` is reused after mapping the space kind:
// the Wmc `Stats.space_kind` field is the recipient that
// `Stats::merge` keys off when rolling per-function cyclomatics into
// the parent. Mapping to `Class` / `Interface` means the existing
// merge logic produces the right numbers without touching the shared
// helpers.
//
// Multiple `impl Foo` blocks each open their own Impl space and
// accumulate independently; their `class_wmc_sum` values are merged
// into the parent space (Unit) during finalisation, so the
// file-level `class_wmc_sum` is the sum of cyclomatic complexity
// across every impl block in the file.
impl Wmc for RustCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        let mapped = match space_kind {
            SpaceKind::Impl => SpaceKind::Class,
            SpaceKind::Trait => SpaceKind::Interface,
            other => other,
        };
        class_interface_compute(mapped, cyclomatic, stats);
    }
}

// C++ WMC. C++'s `class_specifier` and `struct_specifier` both map to
// classes from the OO-metric perspective — `struct` and `class` differ
// only in default visibility, not in their ability to hold methods.
// The `Getter::get_space_kind` impl emits `SpaceKind::Struct` for
// `struct_specifier`, so we collapse it onto `Class` before delegating
// to `class_interface_compute`.
//
// `SpaceKind::Namespace` is intentionally dropped — namespaces are not
// classes; their member functions are free functions and do not
// contribute to a per-class WMC. The Unit space still accumulates the
// per-class sums for file-level reporting.
impl Wmc for CppCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        let mapped = match space_kind {
            SpaceKind::Struct => SpaceKind::Class,
            other => other,
        };
        class_interface_compute(mapped, cyclomatic, stats);
    }
}

// TypeScript / TSX both expose `class_declaration`,
// `abstract_class_declaration` (mapped to `SpaceKind::Class` in
// `getter.rs`) and `interface_declaration` (`SpaceKind::Interface`).
// Method bodies live in `method_definition` and `arrow_function`
// function spaces; their cyclomatic sums roll into the enclosing
// class / interface via `class_interface_compute`. Abstract method
// signatures (`abstract_method_signature`) have no body and so
// contribute zero to WMC, matching Java's `abstract` method rule.
impl Wmc for TypescriptCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

impl Wmc for TsxCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

// Ruby's `Class` and `SingletonClass` map to `SpaceKind::Class` via
// `Getter::get_space_kind`; `Module` maps to `SpaceKind::Namespace`
// and so does not contribute a `Wmc` bucket of its own. Every Ruby
// `Method` / `SingletonMethod` is a `SpaceKind::Function` whose
// cyclomatic sum rolls into the enclosing class via
// `class_interface_compute`. Ruby has no interface construct, so the
// `Interface` arm is unreachable but harmless.
impl Wmc for RubyCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

// JavaScript / Mozjs WMC. JS classes (`class_declaration`,
// `class_expression`) both map to `SpaceKind::Class` in `getter.rs`,
// and method bodies are `method_definition` / `arrow_function`
// function spaces — the same shape as TS/Java. `class_interface_compute`
// rolls per-function cyclomatic sums into the enclosing class.
impl Wmc for JavascriptCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

impl Wmc for MozjsCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

// Go WMC is intentionally a no-op. Go has no `class` syntactic
// construct; methods are declared as `MethodDeclaration` nodes
// attached to a receiver type that lives elsewhere as a `StructType`.
// The Wmc trait signature receives only the per-space `SpaceKind` and
// cyclomatic stats — it cannot tell which `Function` space corresponds
// to a `MethodDeclaration` (receiver method) versus a free-standing
// `FunctionDeclaration`, and the FuncSpace tree exposes no
// per-receiver "class" space to attribute methods to. Implementing
// Wmc correctly per the issue's "methods grouped by receiver = class"
// rule would require either a new `SpaceKind::Struct` variant for Go
// receiver methods or a richer trait signature, both of which are
// out of scope. See the issue body's explicit option (a): "keep
// scoring zero with a documented reason".

// Elixir WMC. Classes (`defmodule`) and Functions (`def` / `defp` /
// `defmacro` / `defmacrop`) are detected by source-aware Checker /
// Getter dispatch (#275), so the FuncSpace tree already carries the
// right `SpaceKind`s. Each method-defining macro opens a Function
// space whose cyclomatic complexity rolls into its enclosing
// `defmodule` Class via the shared aggregator. `defp` (private) is
// included — it is still a *method* of the class even though it is
// not part of the public API (the npm-style "public" filter belongs
// in `Npm`, not `Wmc`).
impl Wmc for ElixirCode {
    fn compute(space_kind: SpaceKind, cyclomatic: &cyclomatic::Stats, stats: &mut Stats) {
        class_interface_compute(space_kind, cyclomatic, stats);
    }
}

// Default no-op `Wmc` impls. Audited in #188. See the rationale block
// on `implement_metric_trait!(Npa, …)` in `src/metrics/npa.rs` — Wmc
// classification mirrors Npa one-for-one (Wmc = sum-of-cyclomatic-
// per-method, so it requires the same per-language class / method
// detection plumbing).
implement_metric_trait!(
    Wmc,
    PreprocCode,
    CcommentCode,
    GoCode,
    PerlCode,
    BashCode,
    LuaCode,
    TclCode
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
    use crate::tools::{assert_child_space_kind, check_func_space, check_metrics};

    use super::*;

    #[test]
    fn java_single_class() {
        check_metrics::<JavaParser>(
            "public class Example { // wmc = 13

                public boolean m1(boolean a, boolean b) { // +1
                    boolean r = false;
                    if (a && b == a || b) { // +3
                        r = true;
                    }
                    return r;
                }

                public boolean m2(int n) { // +1
                    for (int i = 0; i < n; i++) { // +1
                        int j = n;
                        while (j > i) { // +1
                            j--;
                        }
                    }
                    return (n % 2 == 0) ? true : false; // +1
                }

                public int m3(int x, int y, int z) { // +1
                    int ret;
                    try {
                        z = x/y + y/x;
                    } catch (ArithmeticException e) { // +1
                        z = (x == 0) ? -1 : -2; // +1
                    }
                    switch (z) {
                        case -1: // +1
                            ret = y * y;
                            break;
                        case -2: // +1
                            ret = x * x;
                            break;
                        default:
                            ret = x + y;
                    }
                    return ret;
                }
            }",
            "foo.java",
            |metric| {
                // 1 class
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 13.0,
                      "interfaces": 0.0,
                      "total": 13.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn groovy_single_class() {
        // WMC = sum of method cyclomatic complexities for the class.
        check_metrics::<GroovyParser>(
            "class Example {
                boolean m1(boolean a, boolean b) {
                    boolean r = false
                    if (a && b == a || b) {
                        r = true
                    }
                    return r
                }
                boolean m2(int n) {
                    for (int i = 0; i < n; i++) {
                        int j = n
                        while (j > i) {
                            j--
                        }
                    }
                    return (n % 2 == 0) ? true : false
                }
            }",
            "foo.groovy",
            |metric| {
                // m1: entry(1) + if(1) + &&(1) + ||(1) = 4
                // m2: entry(1) + for(1) + while(1) + ternary(1) = 4
                // WMC = 4 + 4 = 8
                assert_eq!(metric.wmc.class_wmc_sum(), 8.0);
            },
        );
    }

    #[test]
    fn groovy_empty_class() {
        check_metrics::<GroovyParser>("class Empty {}", "foo.groovy", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
        });
    }

    #[test]
    fn groovy_class_with_single_method() {
        check_metrics::<GroovyParser>(
            "class A {
                void foo() {
                    println 'hi'
                }
            }",
            "foo.groovy",
            |metric| {
                // single method has entry +1 = 1
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_multiple_classes() {
        check_metrics::<GroovyParser>(
            "class A {
                void f() { if (true) {} }
            }
            class B {
                void g() {}
            }",
            "foo.groovy",
            |metric| {
                // A.f: 1 + 1 (if) = 2, B.g: 1 → total = 3
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_class_with_branching_methods() {
        check_metrics::<GroovyParser>(
            "class Calc {
                int abs(int x) {
                    if (x < 0) {
                        return -x
                    }
                    return x
                }
                int sign(int x) {
                    if (x > 0) return 1
                    if (x < 0) return -1
                    return 0
                }
            }",
            "foo.groovy",
            |metric| {
                // abs: 1 + 1 (if) = 2; sign: 1 + 2 (two ifs) = 3 → 5
                assert_eq!(metric.wmc.class_wmc_sum(), 5.0);
            },
        );
    }

    #[test]
    fn groovy_interface_wmc_is_zero() {
        // Interfaces declare method signatures with no body — wmc = 0.
        check_metrics::<GroovyParser>(
            "interface I {
                void a()
                void b()
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
            },
        );
    }

    #[test]
    fn groovy_static_nested_class() {
        // Mirrors `java_static_nested_class`: nested classes get
        // their own WMC space tied to their parent class's scope.
        check_metrics::<GroovyParser>(
            "class TopLevelClass {
                static class StaticNestedClass {
                    private void m() {
                        println 'Test'
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // TopLevelClass(0) + StaticNestedClass(1 = entry only).
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
            },
        );
    }

    #[test]
    #[ignore = "dekobon Groovy grammar v1 does not yet support inner classes inside class bodies"]
    fn groovy_nested_inner_classes_wmc() {
        // Three nested classes each with one trivial method.
        // Mirrors `java_nested_inner_classes` (wmc.rs flavor).
        check_metrics::<GroovyParser>(
            "class X {
                void a() {}
                class Y {
                    void b() {}
                    class Z {
                        void c() {}
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // 3 classes, each with one method => 1 + 1 + 1 = 3.
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_local_inner_class() {
        // A class declared inside a method body. WMC counts its method
        // like any other class, and the local class is its own WMC scope:
        // `Local.l` (entry 1 + if 1 = 2) must not also fold into
        // `Outer.m`'s contribution to `Outer`. `Outer.m` itself = 1, so
        // total = 1 + 2 = 3 with no double-attribution (#463; the prior
        // value of 6 double-counted the local class's complexity into both
        // scopes, compounded by the Groovy grammar wrapping `m`'s body in a
        // `closure`).
        check_metrics::<GroovyParser>(
            "class Outer {
                void m() {
                    class Local {
                        void l() {
                            if (true) {}
                        }
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
            },
        );
    }

    #[test]
    #[ignore = "dekobon Groovy grammar v1 does not yet support anonymous inner classes (`new T() { … }`)"]
    fn groovy_anonymous_inner_class_wmc() {
        // `new Runnable() { ... }` anonymous inner class. WMC
        // includes the inner's method bodies.
        check_metrics::<GroovyParser>(
            "abstract class Base {
                abstract void m1()
            }
            class Top {
                void m() {
                    def b = new Base() {
                        void m1() {
                            for (int i = 0; i < 5; i++) {
                                println(i)
                            }
                        }
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // Base.m1(1) + Top.m(1) + anonymous.m1(1+for(1)) = 4
                assert_eq!(metric.wmc.class_wmc_sum(), 4.0);
            },
        );
    }

    #[test]
    fn groovy_lambda_expression_wmc() {
        // Lambdas inside a method body don't form their own class
        // space, but the surrounding methods still count toward WMC.
        check_metrics::<GroovyParser>(
            "class Top {
                void m1() {
                    def list = [1, 2, 3]
                    list.each { n -> println(n) }
                }
                void m2() {
                    if (true) {}
                }
            }",
            "foo.groovy",
            |metric| {
                // m1(1) + m2(1 + if(1)) = 3
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_single_interface_wmc() {
        // Default methods inside an interface contribute to WMC.
        // Mirrors `java_single_interface`.
        check_metrics::<GroovyParser>(
            "interface Example {
                default boolean m1(boolean a, boolean b) {
                    return (a && b == a || b)
                }
                default int m2(int n) {
                    return (n != 0) ? 1/n : n
                }
                void m3()
            }",
            "foo.groovy",
            |metric| {
                // m1(1 + && + ||) + m2(1 + ternary) + m3(1) = 6
                assert_eq!(metric.wmc.interface_wmc_sum(), 6.0);
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
            },
        );
    }

    #[test]
    #[ignore = "dekobon Groovy grammar v1 does not yet support inner classes inside interface bodies"]
    fn groovy_class_in_interface() {
        // Inner class inside an interface — its methods count
        // toward `class_wmc`, not `interface_wmc`.
        check_metrics::<GroovyParser>(
            "interface Outer {
                void api()
                class Inner {
                    void f() {
                        if (true) {}
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // Outer interface: api(1) = 1; Inner class: f(1+if) = 2.
                assert_eq!(metric.wmc.interface_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    // Regression for issue #280: Groovy enum bodies fold method-level
    // cyclomatic into `class_wmc_sum` just like Java.
    #[test]
    fn groovy_enum_wmc_aggregates_method_complexity() {
        check_metrics::<GroovyParser>(
            "enum Status {
                ACTIVE, INACTIVE;
                public int code(int n) {
                    if (n > 0) { return n }
                    return 0
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    // Mirror of `java_annotation_type_opens_interface_space_with_zero_wmc`
    // — verifies #280 wired `Groovy::AnnotationTypeDeclaration` into
    // `is_func_space` (the structural check) while keeping
    // `interface_wmc_sum` at 0 because elements are not method
    // declarations. The structural assertion is what distinguishes a
    // working fix from a vacuous one (see the Java sibling for the
    // rationale).
    #[test]
    #[ignore = "dekobon Groovy grammar v1 does not support annotation type elements with `default` values"]
    fn groovy_annotation_type_opens_interface_space_with_zero_wmc() {
        check_func_space::<GroovyParser, _>(
            "public @interface Marker {
                String value() default \"\";
                int priority() default 0;
            }",
            "foo.groovy",
            |func_space| {
                assert_eq!(func_space.metrics.wmc.interface_wmc_sum(), 0.0);
                assert_child_space_kind(&func_space, "Marker", SpaceKind::Interface);
            },
        );
    }

    // Constructors are considered as methods
    // Reference: https://pdepend.org/documentation/software-metrics/weighted-method-count.html
    #[test]
    fn java_multiple_classes() {
        check_metrics::<JavaParser>(
            "public class MainClass { // wmc = 3
                private int a;
                public MainClass() { // +1
                    a = 0;
                }
                public void setA(int n) { // +1
                    a = n;
                }
                public int getA() { // +1
                    return a;
                }
            }

            class TopLevelClass { // wmc = 2
                private int b;
                public TopLevelClass() { // +1
                    b = 0;
                }
                public int getB() { // +1
                    return b;
                }
            }",
            "foo.java",
            |metric| {
                // 2 classes (3 + 2)
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 5.0,
                      "interfaces": 0.0,
                      "total": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_static_nested_class() {
        check_metrics::<JavaParser>(
            "public class TopLevelClass { // wmc = 0
                public static class StaticNestedClass { // wmc = 1
                    private void m() { // +1
                        System.out.println(\"Test\");
                    }
                }
            }",
            "foo.java",
            |metric| {
                // 2 classes (0 + 2)
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 1.0,
                      "interfaces": 0.0,
                      "total": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_nested_inner_classes() {
        check_metrics::<JavaParser>(
            "public class TopLevelClass { // wmc = 2
                private int a;

                class InnerClassBefore { // wmc = 1
                    private boolean b = (a % 2 == 0) ? true : false;
                    public boolean getB() { // +1
                        return b;
                    }
                }

                public TopLevelClass(int n) { // +1
                    if (a != n) { // +1
                        a = n;
                    }
                }

                class InnerClassAfter { // wmc = 2
                    private int c = a;

                    public int getC() { // +1
                        return c;
                    }
                    public void setC(int n) { // +1
                        c = n;
                    }

                    class InnerClass1 { // wmc = 1
                        private int p1;
                        class InnerClass2 { // wmc = 1
                            private int p2;
                            public int getP2() { // +1
                                return p2;
                            }
                            class InnerClass3 { // wmc = 2
                                private int p3;
                                public int getP3() { // +1
                                    return p3;
                                }
                                public void setP3(int n) { // +1
                                    p3 = n;
                                }
                            }
                        }
                        public void setP1(int n) { // +1
                            p1 = n;
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                // 6 classes (2 + 1 + 2 + 1 + 1 + 2)
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 9.0,
                      "interfaces": 0.0,
                      "total": 9.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_local_inner_class() {
        check_metrics::<JavaParser>(
            "import java.util.LinkedList;
            import java.util.List;

            public final class FinalClass { // wmc = 1 (test only)
                private int a = 1;
                public void test() { // +1
                    final List<String> localList = new LinkedList<String>();

                    class LocalInnerClass { // wmc = 2 (print only)
                        private int b = (a == 1) ? 1 : 0; // field init, not a method
                        public void print() { // +1
                            for ( String s : localList ) { // +1
                                System.out.println(s);
                            }
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                // Two classes: `FinalClass` (its only method `test` = 1)
                // and the local `LocalInnerClass` (its only method `print`
                // = base 1 + for 1 = 2). The ternary in `b`'s initializer
                // is class-body cyclomatic, not a method, so it does not
                // count toward WMC. `LocalInnerClass` is its own WMC scope,
                // so its complexity must NOT also fold into `test`'s
                // contribution to `FinalClass` — total = 1 + 2 = 3, with no
                // double-attribution (#463; previously the local class's
                // complexity was double-counted into both scopes, giving an
                // inflated 7).
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 0.0,
                      "total": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_anonymous_inner_class() {
        check_metrics::<JavaParser>(
            "abstract class AbstractClass { // wmc = 1
                abstract void m1(); // +1
            }
            public class TopLevelClass{ // wmc = 3
                public void m(){ // +1
                    AbstractClass ac1 = new AbstractClass() {
                        void m1() { // +1
                            for (int i = 0; i < 5; i++) { // +1
                                System.out.println(\"Test 1: \" + i);
                            }
                        }
                    };
                    ac1.m1();
                }
            }",
            "foo.java",
            |metric| {
                // 2 classes (1 + 3)
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 4.0,
                      "interfaces": 0.0,
                      "total": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_nested_anonymous_inner_classes() {
        check_metrics::<JavaParser>(
            "abstract class AbstractClass{ // wmc = 2
                abstract void m1(); // +1
                abstract void m2(); // +1
            }
            public class TopLevelClass{ // wmc = 6
                public void m(){ // +1

                    AbstractClass ac1 = new AbstractClass() {
                        void m1() { // +1
                            for (int i = 0; i < 5; i++) { // +1
                                System.out.println(\"Test 1: \" + i);
                            }
                        }
                        void m2() { // +1
                            AbstractClass ac2 = new AbstractClass() {
                                void m1() { // +1
                                    System.out.println(\"Test A\");
                                }
                                void m2() { // +1
                                    System.out.println(\"Test B\");
                                }
                            };
                            ac2.m2();
                            System.out.println(\"Test 2\");
                        }
                    };
                    ac1.m1();
                }
            }",
            "foo.java",
            |metric| {
                // 2 classes (2 + 6)
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 8.0,
                      "interfaces": 0.0,
                      "total": 8.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_lambda_expression() {
        check_metrics::<JavaParser>(
            "import java.util.ArrayList;

            public class TopLevelClass { // wmc = 2
                private ArrayList<Integer> numbers;

                public void m1() { // +1
                    numbers = new ArrayList<Integer>();
                    numbers.add(1);
                    numbers.add(2);
                    numbers.add(3);
                }

                public void m2() { // +1
                    numbers.forEach( (n) -> { System.out.println(n); } );
                }
            }",
            "foo.java",
            |metric| {
                // 1 class
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 2.0,
                      "interfaces": 0.0,
                      "total": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_single_interface() {
        check_metrics::<JavaParser>(
            "interface Example { // wmc = 6
                default boolean m1(boolean a, boolean b) { // +1
                    return (a && b == a || b); // +2
                }
                default int m2(int n) { // +1
                    return (n != 0) ? 1/n : n; // +1
                };
                void m3(); // +1
            }",
            "foo.java",
            |metric| {
                // 1 interface
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 0.0,
                      "interfaces": 6.0,
                      "total": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_multiple_interfaces() {
        check_metrics::<JavaParser>(
            "interface FirstInterface { // wmc = 1
                int a = 0;
                default int getA() { // +1
                    return a;
                }
            }

            interface SecondInterface { // wmc = 2
                void setB(int n); // +1
                int getB(); // +1
            }",
            "foo.java",
            |metric| {
                // 2 interfaces (1 + 2)
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 0.0,
                      "interfaces": 3.0,
                      "total": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_nested_inner_interfaces() {
        check_metrics::<JavaParser>(
            "interface TopLevelInterface { // wmc = 1
                interface InnerInterfaceBefore { // wmc = 1
                    void m1(); // +1
                }

                void m2(); // +1

                interface InnerInterfaceAfter { // wmc = 2
                    void m3(); // +1
                    interface InnerInterface { // wmc = 1
                        void m4(); // +1
                    }
                    void m5(); // +1
                }
            }",
            "foo.java",
            |metric| {
                // 4 interfaces (1 + 1 + 2 + 1)
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 0.0,
                      "interfaces": 5.0,
                      "total": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_class_in_interface() {
        check_metrics::<JavaParser>(
            "interface TopLevelInterface { // wmc = 2
                int getA(); // +1
                boolean getB(); // +1

                class InnerClass { // wmc = 2
                    float c;
                    double d;
                    float getC() { // +1
                        return c;
                    }
                    double getD() { // +1
                        return d;
                    }
                }
            }",
            "foo.java",
            |metric| {
                // 1 class 1 interface
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 2.0,
                      "interfaces": 2.0,
                      "total": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_interface_in_class() {
        check_metrics::<JavaParser>(
            "class TopLevelClass { // wmc = 2
                int a;
                boolean b;
                int getA() { // +1
                    return a;
                }
                boolean getB() { // +1
                    return b;
                }

                interface InnerInterface { // wmc = 2
                    float getC(); // +1
                    double getD(); // +1
                }
            }",
            "foo.java",
            |metric| {
                // 1 class 1 interface
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                    {
                      "classes": 2.0,
                      "interfaces": 2.0,
                      "total": 4.0
                    }"###
                );
            },
        );
    }

    // Regression for issue #280: Java `EnumDeclaration` opens a class
    // space, so method-level cyclomatic complexity inside the enum
    // body folds into `class_wmc_sum`.
    #[test]
    fn java_enum_wmc_aggregates_method_complexity() {
        check_metrics::<JavaParser>(
            "enum Status {
                ACTIVE, INACTIVE;
                public int code(int n) {        // entry +1
                    if (n > 0) {                // if +1
                        return n;
                    }
                    return 0;
                }
            }",
            "foo.java",
            |metric| {
                // 1 enum (class), 1 method with cyclomatic = 2.
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    // Regression for issue #280: Java `RecordDeclaration` is treated as
    // a class space; methods inside its explicit body contribute to
    // WMC.
    #[test]
    fn java_record_wmc_aggregates_method_complexity() {
        check_metrics::<JavaParser>(
            "record Point(int x, int y) {
                public int describe() {         // entry +1
                    return (x == 0) ? 0 : 1;    // ternary +1
                }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    // Regression for issue #280: Java `AnnotationTypeDeclaration` must
    // open a `SpaceKind::Interface` FuncSpace (the `is_func_space`
    // change) AND must not aggregate WMC because annotation type
    // elements parse as `AnnotationTypeElementDeclaration`, not
    // `MethodDeclaration`, so no `Function` space is opened for them
    // and their entry cyclomatic is not folded into
    // `interface_wmc_sum`. Asserting only `interface_wmc_sum == 0`
    // would pass vacuously even if `AnnotationTypeDeclaration` were
    // dropped from `is_func_space` (the FuncSpace tree would simply
    // omit the annotation type space, and `0 == 0` would still hold);
    // the structural check on `space.kind` is what catches that
    // regression.
    #[test]
    fn java_annotation_type_opens_interface_space_with_zero_wmc() {
        check_func_space::<JavaParser, _>(
            "@interface Marker {
                String value() default \"\";
                int priority() default 0;
            }",
            "foo.java",
            |func_space| {
                assert_eq!(func_space.metrics.wmc.interface_wmc_sum(), 0.0);
                // Without `AnnotationTypeDeclaration` in `is_func_space`,
                // the file-level Unit would have zero child spaces here.
                assert_child_space_kind(&func_space, "Marker", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn csharp_single_class() {
        check_metrics::<CsharpParser>(
            "public class Example {
                public bool M1(bool a, bool b) {
                    bool r = false;
                    if (a && b == a || b) {
                        r = true;
                    }
                    return r;
                }
                public int M2(int n) {
                    for (int i = 0; i < n; i++) {
                        int j = n;
                        while (j > i) {
                            j--;
                        }
                    }
                    return (n % 2 == 0) ? 1 : 0;
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 8.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_multiple_classes() {
        check_metrics::<CsharpParser>(
            "public class A {
                private int a;
                public A() { a = 0; }
                public void SetA(int n) { a = n; }
                public int GetA() { return a; }
            }
            class B {
                private int b;
                public B() { b = 0; }
                public int GetB() { return b; }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 5.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_static_nested_class() {
        check_metrics::<CsharpParser>(
            "public class Outer {
                public static class Nested {
                    private void M() {
                        System.Console.WriteLine(\"Test\");
                    }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_nested_inner_classes() {
        check_metrics::<CsharpParser>(
            "public class Outer {
                private int a;
                public class Inner {
                    public int GetX() { return 0; }
                    public class Innermost {
                        public int GetY() { return 1; }
                    }
                }
                public int GetA() { return a; }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_local_inner_class() {
        // C# uses local functions instead of Java's local classes.
        check_metrics::<CsharpParser>(
            "public class A {
                public int M(int x) {
                    int Local(int y) {
                        if (y > 0) return y;
                        return -y;
                    }
                    return Local(x);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_anonymous_inner_class() {
        check_metrics::<CsharpParser>(
            "public class A {
                public void Run() {
                    System.Action f = delegate(int x) {
                        if (x > 0) System.Console.WriteLine(x);
                    };
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_nested_anonymous_inner_classes() {
        check_metrics::<CsharpParser>(
            "public class A {
                public void Run() {
                    System.Action f = delegate(int x) {
                        System.Action g = delegate(int y) {
                            if (y > 0) System.Console.WriteLine(y);
                        };
                    };
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 4.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_lambda_expression() {
        check_metrics::<CsharpParser>(
            "public class A {
                public void Run() {
                    System.Func<int, int> f = x => x > 0 ? x : -x;
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_indexer_wmc() {
        // A bodied indexer folds its accessor complexities into the
        // enclosing class. Before #464 the `indexer_declaration` node
        // itself ALSO opened a method space, folding an extra entry on
        // top of get=1 + set=1 (`class_wmc_sum == 3`). The correct sum is
        // 2 — one unit of complexity per accessor — matching the npm path.
        check_metrics::<CsharpParser>(
            "class A {
                private int[] _d;
                public int this[int i] { get => _d[i]; set => _d[i] = value; }
            }",
            "foo.cs",
            |metric| {
                // expected: get (cyclomatic 1) + set (cyclomatic 1) = 2;
                // no extra entry from the IndexerDeclaration node.
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_expression_bodied_indexer_wmc() {
        // The accessor-less expression-bodied form (`this[int i] => _d[i];`)
        // has no `accessor_declaration` child, so the #464 gate keeps the
        // IndexerDeclaration node itself opening a single method space —
        // it must stay at 1, not regress to 0 (mirrors npm `.max(1)`).
        check_metrics::<CsharpParser>(
            "class A {
                private int[] _d;
                public int this[int i] => _d[i];
            }",
            "foo.cs",
            |metric| {
                // expected: one implicit getter (cyclomatic 1) = 1.
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_property_wmc() {
        // A bodied property folds its accessor complexities into the
        // enclosing class. The `property_declaration` node must NOT open an
        // extra method space on top of get=1 + set=1 (the property analogue
        // of the #464 double-count). The correct sum is 2.
        check_metrics::<CsharpParser>(
            "class A {
                private int _w;
                public int W { get => _w; set => _w = value; }
            }",
            "foo.cs",
            |metric| {
                // expected: get (cyclomatic 1) + set (cyclomatic 1) = 2;
                // no extra entry from the PropertyDeclaration node (#472).
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
            },
        );
    }

    #[test]
    fn csharp_expression_bodied_property_wmc() {
        // The accessor-less expression-bodied form (`int W => _w;`) has no
        // `accessor_declaration` child, so the #472 gate lets the
        // PropertyDeclaration node itself open a single method space — it
        // must be 1, not 0 as before the fix (mirrors npm `.max(1)`).
        check_metrics::<CsharpParser>(
            "class A {
                private int _w;
                public int W => _w;
            }",
            "foo.cs",
            |metric| {
                // expected: one implicit getter (cyclomatic 1) = 1.
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
            },
        );
    }

    #[test]
    fn csharp_single_interface() {
        check_metrics::<CsharpParser>(
            "public interface I {
                int GetA();
                int GetB();
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_multiple_interfaces() {
        check_metrics::<CsharpParser>(
            "public interface I1 { int GetA(); }
            public interface I2 { bool GetB(); float GetC(); }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_nested_inner_interfaces() {
        check_metrics::<CsharpParser>(
            "public interface I1 {
                int GetA();
                public interface I2 {
                    bool GetB();
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_class_in_interface() {
        check_metrics::<CsharpParser>(
            "public interface I {
                int GetA();
                public class Helper {
                    public int M() { return 0; }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn csharp_interface_in_class() {
        check_metrics::<CsharpParser>(
            "class Outer {
                int a;
                bool b;
                public int GetA() { return a; }
                public bool GetB() { return b; }
                public interface InnerI {
                    float GetC();
                    double GetD();
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn php_no_classes() {
        check_metrics::<PhpParser>(
            "<?php function f(): int { return 1; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_one_class_simple() {
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public function a(): int { return 1; }
                public function b(): int { return 2; }
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_one_class_with_loops() {
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public function f(int $n): int {
                    $sum = 0;
                    for ($i = 0; $i < $n; $i++) {
                        $sum += $i;
                    }
                    return $sum;
                }
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_one_class_with_branches() {
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public function f(int $x): int {
                    if ($x > 0) {
                        return 1;
                    }
                    if ($x < 0) {
                        return -1;
                    }
                    return 0;
                }
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_class_with_methods_only() {
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public function a(): void {}
                public function b(): void {}
                public function c(): void {}
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_multiple_classes() {
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public function f(int $x): int {
                    if ($x > 0) { return 1; }
                    return 0;
                }
            }
            class B {
                public function g(int $x): int {
                    return $x;
                }
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_anonymous_class() {
        check_metrics::<PhpParser>(
            "<?php
            $obj = new class {
                public function f(int $x): int {
                    if ($x > 0) { return 1; }
                    return 0;
                }
            };",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_class_with_static_methods() {
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public static function f(int $x): int {
                    if ($x > 0) { return 1; }
                    return 0;
                }
                public static function g(): int { return 1; }
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_interface_wmc() {
        check_metrics::<PhpParser>(
            "<?php
            interface I {
                public function a(): void;
                public function b(): int;
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_trait_wmc() {
        check_metrics::<PhpParser>(
            "<?php
            trait T {
                public function f(int $x): int {
                    if ($x > 0) { return 1; }
                    return 0;
                }
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_enum_with_methods() {
        check_metrics::<PhpParser>(
            "<?php
            enum Color {
                case Red;
                case Green;
                public function label(): string {
                    return match ($this) {
                        Color::Red => 'r',
                        Color::Green => 'g',
                    };
                }
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_class_inside_namespace() {
        check_metrics::<PhpParser>(
            "<?php
            namespace App;
            class A {
                public function f(int $x): int {
                    if ($x > 0) { return 1; }
                    return 0;
                }
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    #[test]
    fn php_class_complex() {
        check_metrics::<PhpParser>(
            "<?php
            class Calc {
                public function add(int $a, int $b): int {
                    if ($a > 0 && $b > 0) {
                        return $a + $b;
                    }
                    return 0;
                }
                public function loop(int $n): int {
                    $s = 0;
                    for ($i = 0; $i < $n; $i++) {
                        if ($i % 2 === 0) { $s += $i; }
                    }
                    return $s;
                }
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.wmc),
        );
    }

    // --- Kotlin WMC tests -------------------------------------------------
    //
    // Reference: Kotlin `class_declaration` carries either a `class` or
    // `interface` keyword child; the getter routes the former to
    // `SpaceKind::Class` and the latter to `SpaceKind::Interface`. Member
    // function cyclomatic complexity accumulates into the enclosing
    // class/interface bucket, mirroring the Java impl.

    #[test]
    fn kotlin_empty_class() {
        // Empty class — no methods, WMC = 0.
        check_metrics::<KotlinParser>("class Empty {}", "foo.kt", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
            assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }

    #[test]
    fn kotlin_single_class() {
        // wmc = 1 (method base) + 1 (if) + 1 (explicit when arm; `else`
        // skipped per #282) = 3
        check_metrics::<KotlinParser>(
            "class C {
                fun m(x: Int): Int {       // +1
                    if (x > 0) {           // +1
                        return x
                    }
                    return when (x) {
                        0 -> 0             // +1 (WhenEntry)
                        else -> -x         // skipped (else is default)
                    }
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_multiple_classes() {
        // A: constructor 1 + setA 1 + getA 1 = 3
        // B: constructor 1 + getB 1 = 2
        check_metrics::<KotlinParser>(
            "class A {
                private var a: Int = 0
                constructor(n: Int) { a = n }   // +1
                fun setA(n: Int) { a = n }      // +1
                fun getA(): Int = a             // +1
            }
            class B {
                private var b: Int = 0
                constructor(n: Int) { b = n }   // +1
                fun getB(): Int = b             // +1
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 5.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_nested_class() {
        // Outer: 0 methods. Nested: m(): +1
        check_metrics::<KotlinParser>(
            "class Outer {
                class Nested {
                    fun m() { println(\"hi\") }   // +1
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_inner_class() {
        // `inner class` differs semantically (captures outer reference) but
        // structurally still opens a new class space.
        check_metrics::<KotlinParser>(
            "class Outer {
                fun outerM() {}                    // +1
                inner class Inner {
                    fun innerM() {}                // +1
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_data_class() {
        // `data class` synthesizes copy/equals/hashCode/toString at
        // compile time, but only user-written methods are counted —
        // compiler-generated members are not user code.
        check_metrics::<KotlinParser>(
            "data class Point(val x: Int, val y: Int) {
                fun manhattan(): Int = kotlin.math.abs(x) + kotlin.math.abs(y)  // +1
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_object_singleton() {
        // `object` declarations are singletons; the getter routes them to
        // `SpaceKind::Class` so their methods count as class methods.
        check_metrics::<KotlinParser>(
            "object Util {
                fun add(a: Int, b: Int): Int = a + b   // +1
                fun gtZero(n: Int): Boolean {          // +1
                    return n > 0
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_companion_object() {
        // A `companion object` opens its own Class space, exactly like a
        // named `object` declaration (#431). The companion's `mk` and the
        // enclosing class's `get` are each attributed to their own Class
        // space; the file-level `class_wmc_sum` aggregates both (1 + 1 = 2)
        // with no member lost or double-counted.
        check_metrics::<KotlinParser>(
            "class Holder {
                val instance: Int = 1
                fun get(): Int = instance               // +1
                companion object {
                    val SCALE: Int = 10
                    fun mk(): Holder = Holder()         // +1
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_companion_object_opens_class_space() {
        // Structural guard for #431: a named `companion object` must open
        // its own Class space, mirroring the named-object handling, rather
        // than folding its members into the enclosing class. Reverting the
        // `CompanionObject` arm in `get_space_kind` / `is_func_space` drops
        // the `Companion` child space, failing this assertion.
        check_func_space::<KotlinParser, _>(
            "class Holder {
                fun get(): Int = 1
                companion object Companion {
                    fun mk(): Holder = Holder()
                }
            }",
            "foo.kt",
            |func_space| {
                let holder = func_space
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("Holder"))
                    .expect("expected a child FuncSpace named \"Holder\"");
                // The companion is a Class space nested inside Holder, not a
                // sibling at the file level and not absorbed into Holder.
                assert_child_space_kind(holder, "Companion", crate::SpaceKind::Class);
                let companion = holder
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("Companion"))
                    .expect("expected a child FuncSpace named \"Companion\"");
                // `mk` is attributed to the companion space (wmc = 1), and
                // Holder's roll-up totals get + mk = 2 with no double-count
                // (the file-level aggregate stays 2, not 3).
                assert_eq!(companion.metrics.wmc.class_wmc_sum(), 1.0);
                assert_eq!(holder.metrics.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    #[test]
    fn kotlin_object_literal_opens_class_space() {
        // Structural guard for #463: an anonymous `object : T { ... }`
        // (`object_literal`) must open its own Class space, exactly like a
        // named `object` or `companion object`, rather than folding its
        // members into the enclosing function. Reverting the
        // `ObjectLiteral` arm in `get_space_kind` / `is_func_space`
        // attributes `run` and `helper` to `Holder.get`, failing the
        // structural assertions below (verified by revert).
        check_func_space::<KotlinParser, _>(
            "class Holder {
                fun get(): Int {
                    val r = object : Runnable {
                        override fun run() {}
                        fun helper(): Int = 42
                    }
                    return 1
                }
            }",
            "foo.kt",
            |func_space| {
                let holder = func_space
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("Holder"))
                    .expect("expected a child FuncSpace named \"Holder\"");
                let get = holder
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("get"))
                    .expect("expected a child FuncSpace named \"get\"");
                // The anonymous object opens a Class space nested inside
                // `get`, named `<anonymous>` (default `get_func_space_name`).
                assert_child_space_kind(get, "<anonymous>", crate::SpaceKind::Class);
                let anon = get
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("<anonymous>"))
                    .expect("expected a child FuncSpace named \"<anonymous>\"");
                // `run` and `helper` are attributed to the anonymous space
                // (its two child Function spaces), NOT to `get`. `get`'s
                // only direct child is the anonymous space — there are no
                // stray `run` / `helper` siblings folded into `get`.
                assert_eq!(
                    get.spaces.len(),
                    1,
                    "get's only child is the anonymous class"
                );
                let anon_methods = anon
                    .spaces
                    .iter()
                    .filter(|s| s.kind == crate::SpaceKind::Function)
                    .count();
                assert_eq!(
                    anon_methods, 2,
                    "run + helper attributed to the anonymous class"
                );
                // Issue's core requirement: members are *removed* from the
                // enclosing method, not merely added to the anonymous space.
                // `get`'s own function count is just itself; reverting the
                // space-opening arm folds run/helper back in, lifting it to 3.
                assert_eq!(
                    get.metrics.nom.functions(),
                    1.0,
                    "get owns only itself; run/helper are not folded in"
                );
                // The anonymous class rolls up both methods' WMC (run +
                // helper = 2). These two methods are accounted for in the
                // anonymous space, not double-counted into `get`.
                assert_eq!(anon.metrics.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    #[test]
    fn java_anonymous_class_opens_space() {
        // #463: a Java anonymous class (`new Runnable() { ... }`) opens its
        // own Class space so its members are attributed to it, not the
        // enclosing method. A plain `new Object()` (no `class_body` child)
        // and a lambda (`() -> {}`, a distinct `lambda_expression`) must
        // NOT open a Class space — the gate is on the `class_body` child.
        // Reverting the `ObjectCreationExpression` gate drops the anonymous
        // Class space and re-attributes `run` to `m`, failing this test.
        check_func_space::<JavaParser, _>(
            "class C {
                void m() {
                    Runnable r = new Runnable() {
                        public void run() {}
                    };
                    Object o = new Object();
                    Runnable l = () -> {};
                }
            }",
            "C.java",
            |func_space| {
                let c = func_space
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("C"))
                    .expect("expected a child FuncSpace named \"C\"");
                let m = c
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("m"))
                    .expect("expected a child FuncSpace named \"m\"");
                // Exactly one Class child under `m`: the anonymous class.
                // Plain `new Object()` and the lambda must not over-open;
                // the lambda is a Function space (Java tags
                // `LambdaExpression` as Function), so count Class children.
                let anon_classes: Vec<_> = m
                    .spaces
                    .iter()
                    .filter(|s| s.kind == crate::SpaceKind::Class)
                    .collect();
                assert_eq!(anon_classes.len(), 1, "exactly one anonymous Class space");
                assert_eq!(anon_classes[0].name.as_deref(), Some("<anonymous>"));
                // `run` is attributed to the anonymous class (its single
                // child Function space), not to `m`.
                let anon_methods = anon_classes[0]
                    .spaces
                    .iter()
                    .filter(|s| s.kind == crate::SpaceKind::Function)
                    .count();
                assert_eq!(anon_methods, 1, "run attributed to the anonymous class");
                assert_eq!(anon_classes[0].metrics.wmc.class_wmc_sum(), 1.0);
            },
        );
    }

    #[test]
    fn java_lambda_opens_no_class_space() {
        // Guard against mis-detection (#463): a Java lambda is a
        // `lambda_expression`, NOT an `object_creation_expression`, so it
        // must never trip the anonymous-class gate. It opens a Function
        // space (existing behaviour), never a Class space.
        check_func_space::<JavaParser, _>(
            "class C {
                void m() {
                    Runnable l = () -> { int x = 1; };
                }
            }",
            "C.java",
            |func_space| {
                let c = func_space
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("C"))
                    .expect("expected a child FuncSpace named \"C\"");
                let m = c
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("m"))
                    .expect("expected a child FuncSpace named \"m\"");
                let class_children = m
                    .spaces
                    .iter()
                    .filter(|s| s.kind == crate::SpaceKind::Class)
                    .count();
                assert_eq!(class_children, 0, "a lambda must not open a Class space");
            },
        );
    }

    #[test]
    fn groovy_anonymous_class_models_body_as_closure() {
        // #463 upstream-grammar note: the pinned dekobon Groovy grammar
        // does NOT attach an anonymous-class body to its
        // `object_creation_expression`. It parses `new Runnable()` as a
        // bare constructor call and the trailing `{ ... }` as a separate
        // `closure`, which already opens a Function space. So Groovy gets
        // no Class space for an anonymous class (unlike Java), but its
        // members are still NOT mis-attributed to the enclosing method —
        // they land in the closure's Function space. This pins that
        // behaviour so a future grammar bump that starts modelling
        // `class_body` here is caught and the Groovy `get_space_kind` arm
        // can be revisited.
        check_func_space::<GroovyParser, _>(
            "class C {
                void m() {
                    def r = new Runnable() {
                        void run() {}
                    }
                }
            }",
            "C.groovy",
            |func_space| {
                let c = func_space
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("C"))
                    .expect("expected a child FuncSpace named \"C\"");
                let m = c
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("m"))
                    .expect("expected a child FuncSpace named \"m\"");
                // No Class space (grammar limitation), but `run` lands in a
                // nested Function space (the closure), not in `m` itself.
                let class_children = m
                    .spaces
                    .iter()
                    .filter(|s| s.kind == crate::SpaceKind::Class)
                    .count();
                assert_eq!(
                    class_children, 0,
                    "Groovy grammar models the body as a closure, not a class"
                );
                assert_eq!(m.metrics.nom.functions(), 1.0, "`m` itself, not `run`");
                let nested_funcs: f64 = m.spaces.iter().map(|s| s.metrics.nom.functions()).sum();
                assert_eq!(
                    nested_funcs, 1.0,
                    "`run` is attributed to the nested closure space"
                );
            },
        );
    }

    #[test]
    fn kotlin_interface_simple() {
        // Interface methods all contribute to the interface bucket.
        check_metrics::<KotlinParser>(
            "interface I {
                fun work(): Int                         // +1
                fun describe(): String                  // +1
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_interface_with_default_method() {
        // Default method with control flow counts its full cyclomatic.
        check_metrics::<KotlinParser>(
            "interface I {
                fun abs(n: Int): Int {                   // +1
                    return if (n < 0) -n else n          // +1 if
                }
                fun pure(): Int                          // +1
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_override_function() {
        // `override fun` is structurally just a `function_declaration` with
        // an `override` modifier — counts like any other method.
        check_metrics::<KotlinParser>(
            "open class Base {
                open fun greet(): String = \"hi\"        // +1
            }
            class Sub : Base() {
                override fun greet(): String = \"yo\"    // +1
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_secondary_constructor() {
        // Secondary constructors are explicit `secondary_constructor`
        // nodes; they count as methods.
        check_metrics::<KotlinParser>(
            "class C {
                private var a: Int = 0
                constructor(n: Int) {                    // +1
                    a = n
                }
                constructor(n: Int, m: Int) {            // +1
                    a = n + m
                }
                fun get(): Int = a                       // +1
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_init_block() {
        // `init` blocks are anonymous initializers, not function spaces;
        // they do not add to WMC directly. The class still has whatever
        // methods it declares.
        check_metrics::<KotlinParser>(
            "class C(val n: Int) {
                init {                                   // not counted
                    require(n >= 0) { \"n must be non-negative\" }
                }
                fun get(): Int = n                       // +1
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_top_level_function_excluded() {
        // Top-level `fun` and `val` belong to the `Unit` space, not a class
        // space — they must not contribute to any class metric.
        check_metrics::<KotlinParser>(
            "fun freeFunction(): Int = 42
            val freeVal: Int = 0
            class C { fun m(): Int = 1 }                 // +1
            ",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_extension_function_excluded() {
        // Extension functions look syntactically like methods but the
        // grammar parses them as top-level `function_declaration` with a
        // receiver-type prefix; they belong to the `Unit` space, not a
        // class. Class still gets +1 for its declared method.
        check_metrics::<KotlinParser>(
            "fun List<Int>.sum2(): Int = this.size       // top-level
            class C { fun m(): Int = 1 }                 // +1
            ",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_generic_class() {
        // Generic class with two methods.
        check_metrics::<KotlinParser>(
            "class Box<T>(val value: T) {
                fun get(): T = value                     // +1
                fun mapTo(f: (T) -> T): T = f(value)     // +1
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_class_in_interface() {
        // Nested class inside an interface: the inner class is a class
        // space (its method counts toward classes_wmc), and the interface
        // is the outer.
        check_metrics::<KotlinParser>(
            "interface Outer {
                fun work(): Int                          // +1 (interface)
                class Helper {
                    fun help(): Int = 0                  // +1 (class)
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn kotlin_interface_in_class() {
        // Inverse of the prior test.
        check_metrics::<KotlinParser>(
            "class Outer {
                fun work(): Int = 1                      // +1 (class)
                interface Sub {
                    fun help(): Int                      // +1 (interface)
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    // --- TypeScript / TSX WMC tests --------------------------------------
    //
    // Each class method contributes its cyclomatic complexity to the
    // enclosing class's WMC. Arrow function class members behave as
    // methods. Interface method signatures have no bodies and add zero
    // (matching Java's abstract-method rule).

    #[test]
    fn typescript_class_wmc_single_method() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(): number { return 1; }       // cyclomatic 1
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_class_wmc_two_methods() {
        check_metrics::<TypescriptParser>(
            "class C {
                a(): number { return 1; }       // +1
                b(x: number): number {          // +2 (if branch)
                    if (x > 0) return x;
                    return 0;
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_class_wmc_with_branches() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(x: number): number {
                    if (x > 0) {                // +1
                        return 1;
                    } else if (x < 0) {         // +1
                        return -1;
                    }
                    return 0;
                }                                // base 1
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_class_wmc_arrow_field() {
        // Arrow-function class fields contribute their cyclomatic to
        // the enclosing class — they are function spaces.
        check_metrics::<TypescriptParser>(
            "class C {
                arrow = (x: number) => {
                    if (x > 0) return x;        // +1
                    return 0;
                };                              // base 1
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_class_wmc_with_loops() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(xs: number[]): number {
                    let total = 0;
                    for (const x of xs) {       // +1
                        total += x;
                    }
                    return total;
                }                                // base 1
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_abstract_class_wmc() {
        // Abstract method signatures have no body — contribute 0.
        check_metrics::<TypescriptParser>(
            "abstract class C {
                abstract a(): void;             // signature only, 0
                m(): number { return 1; }       // +1
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_interface_wmc_zero() {
        // Interface method signatures have no bodies → 0 WMC.
        check_metrics::<TypescriptParser>(
            "interface I {
                a(): void;
                b(): number;
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_constructor_wmc() {
        // Constructor counts as a method; its cyclomatic adds to the
        // class WMC.
        check_metrics::<TypescriptParser>(
            "class C {
                x: number;
                constructor(n: number) {
                    if (n > 0) {                // +1
                        this.x = n;
                    } else {
                        this.x = 0;
                    }
                }                                // base 1
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_getter_setter_wmc() {
        // Getter and setter each contribute 1 (base).
        check_metrics::<TypescriptParser>(
            "class C {
                _x: number = 0;
                get x(): number { return this._x; }
                set x(v: number) { this._x = v; }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_multiple_classes_wmc_independent() {
        check_metrics::<TypescriptParser>(
            "class A { m(): number { return 1; } }
             class B {
                m(x: number): number {
                    if (x > 0) return x;        // +1
                    return 0;
                }                                // base 1
             }",
            "foo.ts",
            |metric| {
                // A: 1 + B: 2 = 3 total.
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_class_wmc_with_ternary_and_logical() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(x: number, y: number): number {
                    return x > 0 && y > 0      // +1 (ternary) +1 (&&)
                        ? x + y
                        : 0;
                }                                // base 1
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn typescript_generic_class_wmc() {
        check_metrics::<TypescriptParser>(
            "class Box<T> {
                value: T;
                set(v: T): void { this.value = v; }
                get(): T { return this.value; }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    // TSX parity

    #[test]
    fn tsx_class_wmc_single_method() {
        check_metrics::<TsxParser>(
            "class C { m(): number { return 1; } }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_class_wmc_two_methods() {
        check_metrics::<TsxParser>(
            "class C {
                a(): number { return 1; }
                b(x: number): number {
                    if (x > 0) return x;
                    return 0;
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_class_wmc_with_branches() {
        check_metrics::<TsxParser>(
            "class C {
                m(x: number): number {
                    if (x > 0) return 1;
                    else if (x < 0) return -1;
                    return 0;
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_class_wmc_arrow_field() {
        check_metrics::<TsxParser>(
            "class C {
                arrow = (x: number) => {
                    if (x > 0) return x;
                    return 0;
                };
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_class_wmc_with_loops() {
        check_metrics::<TsxParser>(
            "class C {
                m(xs: number[]): number {
                    let total = 0;
                    for (const x of xs) { total += x; }
                    return total;
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_abstract_class_wmc() {
        check_metrics::<TsxParser>(
            "abstract class C {
                abstract a(): void;
                m(): number { return 1; }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_interface_wmc_zero() {
        check_metrics::<TsxParser>(
            "interface I { a(): void; b(): number; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_constructor_wmc() {
        check_metrics::<TsxParser>(
            "class C {
                x: number;
                constructor(n: number) {
                    if (n > 0) this.x = n;
                    else this.x = 0;
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_getter_setter_wmc() {
        check_metrics::<TsxParser>(
            "class C {
                _x: number = 0;
                get x(): number { return this._x; }
                set x(v: number) { this._x = v; }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_multiple_classes_wmc_independent() {
        check_metrics::<TsxParser>(
            "class A { m(): number { return 1; } }
             class B {
                m(x: number): number {
                    if (x > 0) return x;
                    return 0;
                }
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_class_wmc_with_ternary_and_logical() {
        check_metrics::<TsxParser>(
            "class C {
                m(x: number, y: number): number {
                    return x > 0 && y > 0 ? x + y : 0;
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn tsx_generic_class_wmc() {
        check_metrics::<TsxParser>(
            "class Box<T> {
                value: T;
                set(v: T): void { this.value = v; }
                get(): T { return this.value; }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    // --- Ruby WMC tests ---------------------------------------------------
    //
    // Reference: Ruby `Class` and `SingletonClass` map to `SpaceKind::Class`
    // via `Getter::get_space_kind`; `Module` is a `SpaceKind::Namespace`
    // and does not contribute to WMC. Method cyclomatic complexities
    // accumulate into the enclosing class via `class_interface_compute`.

    #[test]
    fn ruby_no_classes() {
        // File with only a top-level method — no class space, WMC = 0.
        check_metrics::<RubyParser>("def foo\n  1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
            assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }

    #[test]
    fn ruby_empty_class() {
        // Class with no methods → wmc = 0.
        check_metrics::<RubyParser>("class Foo\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }

    #[test]
    fn ruby_one_class_simple() {
        // Two methods, each with cyclomatic = 1 (the method base) → wmc = 2.
        check_metrics::<RubyParser>(
            "class A\n  def a\n    1\n  end\n  def b\n    2\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn ruby_one_class_with_branch() {
        // One method with cyclomatic 1 (base) + 1 (if) = 2.
        check_metrics::<RubyParser>(
            "class A\n  def f(x)\n    if x > 0\n      1\n    else\n      0\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn ruby_one_class_with_loop() {
        // One method with cyclomatic 1 (base) + 1 (while) = 2.
        check_metrics::<RubyParser>(
            "class A\n  def f(n)\n    while n > 0\n      n -= 1\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn ruby_singleton_method_included() {
        // Mix of regular and singleton (`def self.x`) methods, both
        // contribute to the class WMC.
        check_metrics::<RubyParser>(
            "class A\n  def f\n    1\n  end\n  def self.g\n    2\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn ruby_singleton_class_methods_included() {
        // Methods inside `class << self` belong to the enclosing class
        // (singleton class is a `SpaceKind::Class` of its own).
        check_metrics::<RubyParser>(
            "class A\n  class << self\n    def s\n      1\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                // Two class spaces: outer A (wmc 0, no methods) and the
                // singleton class with its single method (wmc 1).
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn ruby_multiple_classes() {
        // Each class contributes its method-cyclomatic sum to the rollup.
        check_metrics::<RubyParser>(
            "class A\n  def f(x)\n    if x > 0\n      1\n    end\n  end\nend\nclass B\n  def g\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                // A: 2 (base + if). B: 1 (base only). Sum = 3.
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn ruby_module_only() {
        // Module is a `Namespace` space — does NOT contribute to WMC even
        // though the body has methods.
        check_metrics::<RubyParser>(
            "module M\n  def f\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn ruby_class_with_inheritance() {
        // `class A < B` inherits — irrelevant to WMC, which depends only on
        // the method bodies inside this class.
        check_metrics::<RubyParser>(
            "class A < B\n  def f\n    1\n  end\n  def g\n    2\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn ruby_class_with_visibility_keywords() {
        // Visibility keywords do NOT affect WMC — every method body
        // contributes regardless of `private` / `protected`.
        check_metrics::<RubyParser>(
            "class A\n  def a\n    1\n  end\n  private\n  def b\n    1\n  end\n  protected\n  def c\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn ruby_class_complex() {
        // Class with two methods whose cyclomatic sums combine.
        // `add`: base(1) + `if`(1) + `&&`(1) = 3.
        // `loop`: base(1) + `while`(1) + `if`(1) = 3.
        // Class WMC = 6.
        check_metrics::<RubyParser>(
            "class Calc\n  def add(a, b)\n    if a > 0 && b > 0\n      a + b\n    end\n  end\n  def loop(n)\n    s = 0\n    while n > 0\n      if n.even?\n        s += n\n      end\n      n -= 1\n    end\n    s\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 6.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    // ---------------------------------------------------------------
    // Default-impl placeholder smoke tests (audited in #188).
    //
    // Each test feeds a class / struct with multiple branchy methods
    // to a language whose `Wmc` is currently the default no-op. The
    // assertion pins the current 0 value; when the real impl lands
    // the assertion will fire and force a test update.
    // ---------------------------------------------------------------

    // --- Python WMC ---------------------------------------------------

    #[test]
    fn python_empty_class_zero_wmc() {
        check_metrics::<PythonParser>("class C:\n    pass\n", "foo.py", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
            assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }

    #[test]
    fn python_single_method_wmc_one() {
        // Single straight-line method → cyclomatic 1 → WMC 1.
        check_metrics::<PythonParser>(
            "class C:\n    def m(self):\n        return 1\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn python_method_with_if_adds_to_wmc() {
        // Cyclomatic: 1 (base) + 1 (if) = 2. WMC = 2.
        check_metrics::<PythonParser>(
            "class C:\n    def m(self, x):\n        if x > 0:\n            return 1\n        return 0\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn python_multiple_methods_wmc_sums() {
        // method1 cyclomatic 1, method2 cyclomatic 2 (if), method3
        // cyclomatic 3 (if + for). WMC sum = 1 + 2 + 3 = 6.
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   def m1(self):\n\
             \x20       return 1\n\
             \x20   def m2(self, x):\n\
             \x20       if x:\n\
             \x20           return 1\n\
             \x20       return 0\n\
             \x20   def m3(self, xs):\n\
             \x20       for x in xs:\n\
             \x20           if x:\n\
             \x20               return x\n\
             \x20       return None\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 6.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn python_top_level_function_does_not_contribute_to_class_wmc() {
        // Top-level function lives in the module/unit space, not in a
        // class space — class_wmc stays at 0.
        check_metrics::<PythonParser>(
            "def f(x):\n    if x:\n        return 1\n    return 0\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn python_multiple_classes_wmc_independent() {
        // Each class accumulates its own methods' cyclomatic. The
        // file-level class_wmc_sum is the sum of every class's WMC.
        // A.m1 (1) + B.m2 (2 — has an if) = 3.
        check_metrics::<PythonParser>(
            "class A:\n\
             \x20   def m1(self):\n\
             \x20       return 1\n\
             class B:\n\
             \x20   def m2(self, x):\n\
             \x20       if x:\n\
             \x20           return 1\n\
             \x20       return 0\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn rust_empty_unit_zero_wmc() {
        check_metrics::<RustParser>("", "empty.rs", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
            assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }

    #[test]
    fn rust_single_impl_method_wmc_one() {
        // Single straight-line method → cyclomatic 1 → WMC 1.
        check_metrics::<RustParser>(
            "struct Foo;\nimpl Foo { fn m(&self) -> i32 { 1 } }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn rust_method_with_if_adds_to_wmc() {
        // Cyclomatic: 1 (base) + 1 (if) = 2. WMC = 2.
        check_metrics::<RustParser>(
            "struct Foo;\n\
             impl Foo {\n\
             \x20   fn m(&self, x: i32) -> i32 {\n\
             \x20       if x > 0 { 1 } else { 0 }\n\
             \x20   }\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn rust_multiple_methods_wmc_sums() {
        // m1 cyclomatic 1, m2 cyclomatic 2 (if), m3 cyclomatic 3 (if
        // inside for). WMC = 1 + 2 + 3 = 6.
        check_metrics::<RustParser>(
            "struct Foo;\n\
             impl Foo {\n\
             \x20   fn m1(&self) -> i32 { 1 }\n\
             \x20   fn m2(&self, x: i32) -> i32 { if x > 0 { 1 } else { 0 } }\n\
             \x20   fn m3(&self, xs: &[i32]) -> i32 {\n\
             \x20       for x in xs { if *x > 0 { return *x; } }\n\
             \x20       0\n\
             \x20   }\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 6.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn rust_multiple_impls_wmc_aggregate() {
        // Two `impl` blocks for Foo, each contributing 1 method with
        // cyclomatic 1. Unit-level class_wmc_sum = 2.
        check_metrics::<RustParser>(
            "struct Foo;\n\
             impl Foo { fn m1(&self) {} }\n\
             impl Foo { fn m2(&self) {} }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn rust_trait_default_method_contributes_to_interface_wmc() {
        // A trait method with a default body — `area` is a function
        // space inside the trait. Cyclomatic = 1 → interface_wmc = 1.
        // The signature-only `draw` has no body and contributes
        // nothing.
        check_metrics::<RustParser>(
            "trait T { fn draw(&self); fn area(&self) -> f64 { 0.0 } }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.wmc.interface_wmc_sum(), 1.0);
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn rust_top_level_function_does_not_contribute_to_class_wmc() {
        // Free `fn f()` opens a Function space but no class/trait
        // surrounds it. The Unit space is not a class space, so
        // class_wmc_sum stays at 0.
        check_metrics::<RustParser>(
            "fn f(x: i32) -> i32 { if x > 0 { 1 } else { 0 } }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    // ----- Go -----

    #[test]
    fn go_wmc_is_zero_documented_limitation() {
        // Go's flat space model does not expose per-receiver class
        // spaces, and the Wmc trait signature receives only a
        // `SpaceKind` (Function for both `MethodDeclaration` and
        // free `FunctionDeclaration`). Implementing receiver-grouped
        // WMC would require space-model changes that are out of
        // scope for this fix; per the issue's option (a), the metric
        // stays at zero with a documented reason. This test pins
        // that behaviour so any future Wmc work for Go has to update
        // it deliberately.
        check_metrics::<GoParser>(
            "package main\n\
             type Foo struct{}\n\
             func (f Foo) M(x int) int { if x > 0 { return 1 } else { return 0 } }\n\
             func (f Foo) N() {}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    // ----- Elixir -----

    // Issue #275: Elixir's `def` / `defp` declarations parse as
    // `Call` nodes whose `target` Identifier text spells the
    // keyword. The source-aware Checker / Getter dispatch promotes
    // them to Function spaces inside the surrounding `defmodule`
    // Class. WMC then aggregates cyclomatic per method into the
    // class via the shared `class_interface_compute` aggregator.
    #[test]
    fn elixir_wmc_aggregates_def_methods() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def m(x) do\n    if x > 0 do\n      1\n    else\n      0\n    end\n  end\n  def n, do: :ok\nend\n",
            "foo.ex",
            |metric| {
                // m: entry(1) + if(1) = 2; n: entry(1) = 1 → wmc = 3.
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(
                    metric.wmc,
                    @r###"
                {
                  "classes": 3.0,
                  "interfaces": 0.0,
                  "total": 3.0
                }"###
                );
            },
        );
    }

    #[test]
    fn elixir_wmc_def_plus_defp_counts_both() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def pub_one, do: 1\n  defp priv_one, do: 1\nend\n",
            "foo.ex",
            |metric| {
                // Both `def` and `defp` are methods of the class — npm
                // distinguishes public vs private, wmc does not.
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    #[test]
    fn elixir_wmc_defmacro_counts() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  defmacro stuff(x) do\n    if x > 0, do: :pos, else: :neg\n  end\nend\n",
            "foo.ex",
            |metric| {
                // defmacro is a method; body has entry(1) + if(1) = 2.
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    #[test]
    fn elixir_wmc_multiple_clauses_each_a_method() {
        // Each `def f(...)` head is a Call with its own Function
        // space, so multiple clauses for the same name each count.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(0), do: :zero\n  def f(_), do: :other\nend\n",
            "foo.ex",
            |metric| {
                // Two clauses, entry(1) each → wmc = 2.
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    #[test]
    fn elixir_wmc_nested_defmodule_isolates() {
        check_metrics::<ElixirParser>(
            "defmodule Outer do\n  def o, do: 1\n  defmodule Inner do\n    def i, do: 1\n  end\nend\n",
            "foo.ex",
            |metric| {
                // Outer.o(1) + Inner.i(1) → file-level sum is 2.
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
            },
        );
    }

    #[test]
    fn elixir_wmc_user_macro_not_classified_as_method() {
        // A user-defined `defmacro custom_def`, then invoking
        // `custom_def foo, do: ...` must NOT be classified as a
        // method — the literal-text comparison in
        // `elixir_call_keyword` only matches the four built-in
        // method-defining macros. The `def unquote(name)` inside the
        // `quote do … end` block is also rejected (it is a code
        // template emitted on macro expansion, not a real definition
        // of any method of `Foo`); `elixir_is_inside_quote_block`
        // filters it out, keeping `Wmc` aligned with `Npm` (#310).
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  defmacro custom_def(name, body) do\n    quote do\n      def unquote(name), do: unquote(body)\n    end\n  end\n  custom_def foo, do: 1\nend\n",
            "foo.ex",
            |metric| {
                // Only the `defmacro custom_def` itself is a method
                // of `Foo`. Body cyclomatic: entry(1) → wmc = 1.
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
            },
        );
    }

    #[test]
    fn elixir_wmc_quoted_defs_do_not_inflate_method_count() {
        // Regression test for #310: previously, every `def` lexically
        // present in the source was promoted to a Function space and
        // counted toward `Wmc`, even when nested inside `quote do …
        // end` (a metaprogramming template that does not declare
        // methods of the enclosing module). That made `Wmc` disagree
        // with `Npm`'s direct-children classification.
        //
        // Here `Foo` has exactly one real method (the `defmacro
        // multi`); the three quoted `def`s inside its body are not
        // methods of `Foo`. `Wmc` should now agree.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  defmacro multi do\n    quote do\n      def a, do: 1\n      def b, do: 2\n      defp c, do: 3\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // Only `defmacro multi` is a method of Foo: entry(1)
                // → wmc = 1.
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
            },
        );
    }

    // ----- C++ -----

    #[test]
    fn cpp_empty_unit_zero_wmc() {
        // No code → no class spaces → wmc = 0. Wires up the trait.
        check_metrics::<CppParser>("", "empty.cpp", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
            assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }

    #[test]
    fn cpp_single_method_wmc_one() {
        // One method with no control flow → cyclomatic = 1 → wmc = 1.
        check_metrics::<CppParser>("class Foo { public: void m() {} };", "foo.cpp", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }

    #[test]
    fn cpp_method_with_if_adds_to_wmc() {
        // One method with one `if` → cyclomatic = 2 → wmc = 2.
        check_metrics::<CppParser>(
            "class Foo {\n\
                 public:\n\
                     int m(int x) {\n\
                         if (x > 0) { return 1; }\n\
                         return 0;\n\
                     }\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn cpp_struct_wmc_maps_to_class() {
        // `struct` opens a `SpaceKind::Struct` space — the C++ Wmc
        // impl maps it to `Class` so the same `class_wmc_sum`
        // accumulator receives the cyclomatic of struct methods.
        check_metrics::<CppParser>(
            "struct Foo {\n\
                 int m(int x) {\n\
                     if (x > 0) { return 1; }\n\
                     return 0;\n\
                 }\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn cpp_free_function_does_not_contribute_to_class_wmc() {
        // A top-level function is not inside any class — its
        // cyclomatic complexity must NOT contribute to class_wmc_sum.
        // The `Unit` space is mapped through `class_interface_compute`
        // unchanged; only `Function` spaces inside a `Class` /
        // `Struct` propagate up.
        check_metrics::<CppParser>(
            "int free_fn(int x) { if (x > 0) { return 1; } return 0; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
                assert_eq!(metric.wmc.interface_wmc_sum(), 0.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn cpp_multiple_methods_wmc_sums() {
        // Two methods, one with `if` (cyclomatic 2), one without
        // (cyclomatic 1). class_wmc_sum = 3.
        check_metrics::<CppParser>(
            "class Foo {\n\
                 public:\n\
                     int a(int x) { if (x > 0) { return 1; } return 0; }\n\
                     int b() { return 42; }\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 3.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn cpp_multiple_classes_wmc_aggregate() {
        // File-level rollup: Foo has wmc 1, Bar has wmc 1. Unit
        // class_wmc_sum = 2.
        check_metrics::<CppParser>(
            "class Foo { public: void a() {} };\nstruct Bar { void b() {} };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn javascript_empty_unit_zero_wmc() {
        check_metrics::<JavascriptParser>("", "empty.js", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 0.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }

    #[test]
    fn javascript_single_method_wmc_one() {
        // Class with a single straight-line method has wmc = 1 (the
        // method's cyclomatic) rolling into the class space.
        check_metrics::<JavascriptParser>("class Foo { a() { return 1; } }", "foo.js", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }

    #[test]
    fn javascript_method_with_if_adds_to_wmc() {
        // Method body with an `if` has cyclomatic = 2 → class_wmc = 2.
        check_metrics::<JavascriptParser>(
            "class Foo { a(x) { if (x > 0) return 1; return 0; } }",
            "foo.js",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn javascript_free_function_does_not_contribute_to_class_wmc() {
        // Top-level functions are not class methods; their
        // cyclomatic does not roll into a class.
        check_metrics::<JavascriptParser>(
            "function f(x) { if (x > 0) return 1; return 0; }\nclass Foo { a() { return 1; } }",
            "foo.js",
            |metric| {
                // Only the class method contributes.
                assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn javascript_multiple_classes_wmc_aggregate() {
        // File-level rollup: Foo has wmc 1, Bar has wmc 1. Unit
        // class_wmc_sum = 2.
        check_metrics::<JavascriptParser>(
            "class Foo { a() { return 1; } }\nclass Bar { b() { return 1; } }",
            "foo.js",
            |metric| {
                assert_eq!(metric.wmc.class_wmc_sum(), 2.0);
                insta::assert_json_snapshot!(metric.wmc);
            },
        );
    }

    #[test]
    fn mozjs_single_method_wmc_one() {
        check_metrics::<MozjsParser>("class Foo { a() { return 1; } }", "foo.js", |metric| {
            assert_eq!(metric.wmc.class_wmc_sum(), 1.0);
            insta::assert_json_snapshot!(metric.wmc);
        });
    }
}
