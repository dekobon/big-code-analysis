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

use crate::checker::{Checker, csharp_accessor_count};
use crate::langs::*;
use crate::macros::implement_metric_trait;
use crate::metrics::npa::{accessibility_ratio, python_is_block, ts_member_is_public};
use crate::node::Node;
use crate::*;

/// The `Npm` metric.
///
/// This metric counts the number of public methods
/// of classes/interfaces.
///
/// Emitted on container spaces — [`SpaceKind::Class`], `Struct`,
/// `Trait`, `Impl`, `Namespace`, `Interface` — and on the
/// [`SpaceKind::Unit`] file root that rolls them up. Never on a
/// [`SpaceKind::Function`] space, which owns no members of its own.
///
/// Since [#1203] that holds by construction rather than by convention:
/// the space's own kind is the only input, so no language can disagree
/// with it in either direction. [`Wmc`](crate::wmc::Stats) decides the
/// same way. A language with no class-shaped construct at all — C, Bash,
/// Lua, Perl, Tcl — emits no block anywhere rather than an all-zero one
/// on each file root.
///
/// The rule governs the *block*, not the counts behind it: those roll up
/// through every enclosing space regardless, so a type declared inside a
/// function body is reported by the nearest enclosing container, or by
/// the file root when there is none.
///
/// [#1203]: https://github.com/dekobon/big-code-analysis/issues/1203
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Stats {
    class_npm: usize,
    interface_npm: usize,
    class_nm: usize,
    interface_nm: usize,
    class_npm_sum: usize,
    interface_npm_sum: usize,
    class_nm_sum: usize,
    interface_nm_sum: usize,
    space_kind: SpaceKind,
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "classes: {}, interfaces: {}, class_methods: {}, interface_methods: {}, class_coa: {}, interface_coa: {}, total: {}, total_methods: {}, coa: {}",
            self.class_npm_sum(),
            self.interface_npm_sum(),
            self.class_nm_sum(),
            self.interface_nm_sum(),
            self.class_coa(),
            self.interface_coa(),
            self.total_npm(),
            self.total_nm(),
            self.total_coa()
        )
    }
}

impl Stats {
    /// Merges a second `Npm` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.class_npm_sum += other.class_npm_sum;
        self.interface_npm_sum += other.interface_npm_sum;
        self.class_nm_sum += other.class_nm_sum;
        self.interface_nm_sum += other.interface_nm_sum;
    }

    /// Returns the number of class public methods in a space.
    #[inline]
    #[must_use]
    pub fn class_npm(&self) -> u64 {
        self.class_npm as u64
    }

    /// Returns the number of interface public methods in a space.
    #[inline]
    #[must_use]
    pub fn interface_npm(&self) -> u64 {
        self.interface_npm as u64
    }

    /// Returns the number of class methods in a space.
    #[inline]
    #[must_use]
    pub fn class_nm(&self) -> u64 {
        self.class_nm as u64
    }

    /// Returns the number of interface methods in a space.
    #[inline]
    #[must_use]
    pub fn interface_nm(&self) -> u64 {
        self.interface_nm as u64
    }

    /// Returns the number of class public methods sum in a space.
    #[inline]
    #[must_use]
    pub fn class_npm_sum(&self) -> u64 {
        self.class_npm_sum as u64
    }

    /// Returns the number of interface public methods sum in a space.
    #[inline]
    #[must_use]
    pub fn interface_npm_sum(&self) -> u64 {
        self.interface_npm_sum as u64
    }

    /// Returns the number of class methods sum in a space.
    #[inline]
    #[must_use]
    pub fn class_nm_sum(&self) -> u64 {
        self.class_nm_sum as u64
    }

    /// Returns the number of interface methods sum in a space.
    #[inline]
    #[must_use]
    pub fn interface_nm_sum(&self) -> u64 {
        self.interface_nm_sum as u64
    }

    /// Returns the class `Coa` metric value
    ///
    /// The `Class Operation Accessibility` metric value for a class
    /// is computed by dividing the `Npm` value of the class
    /// by the total number of methods defined in the class.
    ///
    /// This metric is an adaptation of the `Classified Operation Accessibility` (`COA`)
    /// security metric for not classified methods.
    /// Paper: <https://ieeexplore.ieee.org/abstract/document/5381538>
    #[inline]
    #[must_use]
    pub fn class_coa(&self) -> f64 {
        accessibility_ratio(self.class_npm_sum() as f64, self.class_nm_sum() as f64)
    }

    /// Returns the interface `Coa` metric value
    ///
    /// The `Class Operation Accessibility` metric value for an interface
    /// is computed by dividing the `Npm` value of the interface
    /// by the total number of methods defined in the interface.
    ///
    /// This metric is an adaptation of the `Classified Operation Accessibility` (`COA`)
    /// security metric for not classified methods.
    /// Paper: <https://ieeexplore.ieee.org/abstract/document/5381538>
    #[inline]
    #[must_use]
    pub fn interface_coa(&self) -> f64 {
        // Java interface methods are implicitly public, so when every counted
        // method is public (`npm == nm != 0`) the ratio is exactly 1.0 and the
        // division is skipped. The empty case falls through to
        // `accessibility_ratio`, which is guarded to return a finite 0.0 (not
        // `NaN`) for a zero denominator (#438).
        if self.interface_npm_sum == self.interface_nm_sum && self.interface_npm_sum != 0 {
            1.0
        } else {
            accessibility_ratio(
                self.interface_npm_sum() as f64,
                self.interface_nm_sum() as f64,
            )
        }
    }

    /// Returns the total `Coa` metric value
    ///
    /// The total `Class Operation Accessibility` metric value
    /// is computed by dividing the total `Npm` value
    /// by the total number of methods.
    ///
    /// This metric is an adaptation of the `Classified Operation Accessibility` (`COA`)
    /// security metric for not classified methods.
    /// Paper: <https://ieeexplore.ieee.org/abstract/document/5381538>
    #[inline]
    #[must_use]
    pub fn total_coa(&self) -> f64 {
        accessibility_ratio(self.total_npm() as f64, self.total_nm() as f64)
    }

    /// Returns the total number of public methods in a space.
    #[inline]
    #[must_use]
    pub fn total_npm(&self) -> u64 {
        self.class_npm_sum() + self.interface_npm_sum()
    }

    /// Returns the total number of methods in a space.
    #[inline]
    #[must_use]
    pub fn total_nm(&self) -> u64 {
        self.class_nm_sum() + self.interface_nm_sum()
    }

    // Accumulates the number of class and interface
    // public and not public methods into the sums
    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.class_npm_sum += self.class_npm;
        self.interface_npm_sum += self.interface_npm;
        self.class_nm_sum += self.class_nm;
        self.interface_nm_sum += self.interface_nm;
    }

    /// Records the kind of the space these stats describe, which is the
    /// sole input to [`Self::is_disabled`].
    ///
    /// Called once per space from the walker's finalize step, beside the
    /// equivalent `wmc` call. Left unset — and so reported disabled — for
    /// a language whose `HAS_MEMBERS` is `false`.
    #[inline]
    pub(crate) fn set_space_kind(&mut self, kind: SpaceKind) {
        self.space_kind = kind;
    }

    // Checks if the `Npm` metric is disabled
    #[inline]
    pub(crate) fn is_disabled(&self) -> bool {
        !self.space_kind.is_member_scope()
    }
}

/// The direct children of `node` that `C` classifies as functions.
///
/// The class-body arms below all ask the same question — "which of this
/// body's children are methods?" — and share one reason for answering it
/// with [`Ancestors::unknown`]: a chain is a borrowed slice, so it cannot
/// be extended by `node` without allocating one per body. Nothing is lost,
/// because every grammar that reaches this helper (Java, Groovy, Kotlin,
/// PHP) decides `is_func` from the node's own kind and never asks for an
/// ancestor (#1088).
fn direct_child_funcs<'a, C: Checker>(node: &Node<'a>) -> impl Iterator<Item = Node<'a>> {
    node.children()
        .filter(|child| C::is_func(child, Ancestors::unknown()))
}

#[doc(hidden)]
/// Per-language counting of public methods.
pub(crate) trait Npm
where
    Self: Checker,
{
    /// Whether this language has any construct that owns members.
    ///
    /// `false` only for the no-op impls — grammars with no class-shaped
    /// construct at all (C, Bash, Perl, Lua, Tcl, iRules, and the two
    /// comment/preprocessor grammars), where the metric could report
    /// nothing but zeros. The walker consults it before recording a
    /// space kind, so those languages emit no block rather than an
    /// all-zero one on every file root (#1203). `wmc` gets the same
    /// outcome from its no-op `compute`, which never records a kind.
    const HAS_MEMBERS: bool = true;

    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    ///
    /// `code` is the raw source-bytes buffer; languages whose visibility
    /// rules are encoded in identifier text (Ruby's keyword-style
    /// `private` / `public` / `protected`) read identifier text from
    /// it. Languages whose visibility rules are encoded purely in
    /// distinct token kinds (Java's `Public` / `Private`, PHP's
    /// `VisibilityModifier`) ignore the parameter.
    ///
    /// `ancestors` is the chain the walker descended through. The
    /// C-family, C#, PHP, Ruby, Rust, Kotlin, and Groovy impls read a
    /// parent from it, because their grammars give a class body, an
    /// interface body, and (for Rust) a free item the same node kind and
    /// leave the enclosing declaration to disambiguate. Reaching that
    /// declaration with [`Node::parent`] costs `O(depth)` per node
    /// (#1096).
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    );
}

// `impl_npm_java_like!` was introduced for Java and Groovy, whose
// grammar tokens for class / interface bodies matched closely enough that
// `Npm::compute` differed only by the language enum (mirrors
// `impl_npa_java_like!` in `npa.rs`; issue #280). Groovy has since moved
// to a hand-written impl — the dekobon grammar flattens modifiers, see
// `npm/groovy.rs` — so this expands against Java alone. It is kept in
// macro form because the next Java-shaped grammar can reuse it.
//
// `ClassBody` covers class and record explicit bodies;
// `EnumBodyDeclarations` is the optional declarations block inside
// `EnumBody` (after the enum constants) and may contain method
// declarations. Both share the same Java public-method detection rule.
//
// `InterfaceBody`: all methods in an interface are implicitly public
// (https://docs.oracle.com/javase/tutorial/java/IandI/interfaceDef.html).
// `AnnotationTypeBody`: annotation type elements are abstract public
// methods at the bytecode level and obey the same rule.
macro_rules! impl_npm_java_like {
    ($code:ty, $lang:ident) => {
        impl Npm for $code {
            fn compute<'a>(
                node: &Node<'a>,
                _code: &'a [u8],
                _ancestors: Ancestors<'a, '_>,
                stats: &mut Stats,
            ) {
                use $lang::*;

                match node.kind_id().into() {
                    ClassBody | EnumBodyDeclarations => {
                        for method in direct_child_funcs::<Self>(node) {
                            stats.class_nm += 1;
                            // The first child node contains the list of method modifiers.
                            // Source: https://docs.oracle.com/javase/tutorial/reflect/member/methodModifiers.html
                            if let Some(modifiers) = method.child(0)
                                && matches!(modifiers.kind_id().into(), Modifiers)
                                && modifiers.first_child(|id| id == Public).is_some()
                            {
                                stats.class_npm += 1;
                            }
                        }
                    }
                    InterfaceBody => {
                        stats.interface_nm += direct_child_funcs::<Self>(node).count();
                        stats.interface_npm = stats.interface_nm;
                    }
                    AnnotationTypeBody => {
                        stats.interface_nm += node
                            .children()
                            .filter(|n| {
                                matches!(n.kind_id().into(), AnnotationTypeElementDeclaration)
                            })
                            .count();
                        stats.interface_npm = stats.interface_nm;
                    }
                    _ => {}
                }
            }
        }
    };
}

// TypeScript / TSX share the same OOP node shape, so we expand the
// same compute logic into both impls via `ts_npm_compute!`.
//
// What counts as a class method:
// - `method_definition` direct children of `class_body` (regular
//   instance methods, static methods, abstract method
//   implementations, getters/setters/constructors). Each counts as
//   one method — getter and setter each count separately, matching
//   their distinct accessor semantics. Method overloads in TS share
//   a single `method_definition` body (signature-only overloads are
//   `method_signature` nodes inside a class body — those are
//   declaration-only and we do not count them).
// - `public_field_definition` whose initializer is an
//   `arrow_function` (or `function_expression`). These are class
//   members written as `foo = () => {}` and behave as methods.
// - `abstract_method_signature` direct children of `class_body`
//   (abstract method declarations on abstract classes).
//
// Interface decision: `method_signature`, `abstract_method_signature`,
// and `construct_signature` direct children of `interface_body` count
// toward `interface_npm` / `interface_nm`. Interface members are
// implicitly public.
//
// Method overload signatures inside a class (`method_signature` as a
// direct child of `class_body`) are NOT counted — they are
// type-system declarations whose implementation is the `method_definition`
// they precede. Counting them would double-count overloaded methods.
macro_rules! ts_npm_compute {
    ($lang:ident) => {
        fn compute<'a>(
            node: &Node<'a>,
            _code: &'a [u8],
            _ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use $lang::*;

            match node.kind_id().into() {
                ClassBody => {
                    for member in node.children() {
                        match member.kind_id().into() {
                            MethodDefinition | AbstractMethodSignature => {
                                stats.class_nm += 1;
                                if ts_member_is_public!($lang, member) {
                                    stats.class_npm += 1;
                                }
                            }
                            // Field-as-arrow-function (`foo = () => …`) is a
                            // class method written as a field initializer.
                            PublicFieldDefinition
                                if member
                                    .first_child(|id| {
                                        id == $lang::ArrowFunction
                                            || id == $lang::FunctionExpression
                                    })
                                    .is_some() =>
                            {
                                stats.class_nm += 1;
                                if ts_member_is_public!($lang, member) {
                                    stats.class_npm += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                InterfaceBody => {
                    let count = node
                        .children()
                        .filter(|c| {
                            matches!(
                                c.kind_id().into(),
                                MethodSignature | AbstractMethodSignature | ConstructSignature
                            )
                        })
                        .count();
                    stats.interface_nm += count;
                    stats.interface_npm = stats.interface_nm;
                }
                _ => {}
            }
        }
    };
}

// JavaScript / Mozjs class methods. JS has no `accessibility_modifier`
// — every class member is public, so each method maps 1:1 to both
// `nm` and `npm`. Two shapes count:
//
//   1. `method_definition` direct children of `class_body`
//      (regular methods, getters/setters, the constructor — all share
//      the same kind id in the JS grammar).
//   2. `field_definition` whose initializer is an `arrow_function` or
//      `function_expression` (method written as a field initializer:
//      `foo = () => {}`).
//
// Prototype methods (`Foo.prototype.bar = function() {}`) would also
// qualify, but detecting them requires matching the `prototype`
// property text. The `Npm::compute` trait does not carry source
// bytes, so prototype-shaped methods are intentionally not counted.
// Modern ES2015+ class syntax is unaffected.
macro_rules! js_npm_compute {
    ($lang:ident) => {
        fn compute<'a>(
            node: &Node<'a>,
            _code: &'a [u8],
            _ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use $lang::*;

            if !matches!(node.kind_id().into(), ClassBody) {
                return;
            }

            for member in node.children() {
                match member.kind_id().into() {
                    MethodDefinition => {
                        stats.class_nm += 1;
                        stats.class_npm += 1;
                    }
                    FieldDefinition
                        if member
                            .first_child(|id| {
                                id == $lang::ArrowFunction || id == $lang::FunctionExpression
                            })
                            .is_some() =>
                    {
                        stats.class_nm += 1;
                        stats.class_npm += 1;
                    }
                    _ => {}
                }
            }
        }
    };
}

// Per-language `Npm` impls live in sibling modules. The `mod`
// declarations sit after the local `macro_rules!` so textual macro
// scoping reaches the child files (mirrors `getter.rs` and
// `metrics::abc`).
mod cpp;
mod csharp;
mod elixir;
mod go;
mod groovy;
mod java;
mod javascript;
mod kotlin;
mod mozcpp;
mod mozjs;
mod objc;
mod php;
mod python;
mod ruby;
mod rust;
mod tsx;
mod typescript;

// Default no-op `Npm` impls. Audited in #188. See the rationale block
// on `implement_metric_trait!(Npa, …)` in `src/metrics/npa.rs` — Npm
// classification mirrors Npa one-for-one (same set of "has classes?"
// questions, same follow-up issues).

implement_metric_trait!(
    Npm,
    CCode,
    PreprocCode,
    CcommentCode,
    PerlCode,
    BashCode,
    LuaCode,
    TclCode,
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
    use crate::test_support::{
        assert_child_space_kind, check_func_space_only_shim, check_metrics_only_shim, child_space,
    };

    use super::*;

    check_metrics_only_shim!(check_metrics, Npm);
    check_func_space_only_shim!(check_func_space, Npm);
    // `Npm` alongside the two metrics that count the same C++ members
    // through independent walks: `Nom` (one per function space) and
    // `Wmc` (each member's cyclomatic, rolled into the class). #1258
    // was invisible to the `Npm`-only shim precisely because nothing
    // asserted the three agree.
    check_metrics_only_shim!(check_metrics_with_nom_wmc, Npm, Nom, Wmc);
    // `Npm` alongside `Npa`, for members a bug counted as *neither*.
    // Asserting one metric alone cannot tell "now counted correctly"
    // apart from "moved to the other counter" — #1298's conversion
    // operators read as absent from both, so both must be pinned.
    check_metrics_only_shim!(check_metrics_with_npa, Npm, Npa);

    #[test]
    fn java_constructors() {
        check_metrics::<JavaParser>(
            "class X {
                X() {}
                private X(int a) {}
                protected X(int a, int b) {}
                public X(int a, int b, int c) {}    // +1
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 1,
                  "interface_npm_sum": 0,
                  "class_methods": 4,
                  "interface_methods": 0,
                  "class_coa": 0.25,
                  "interface_coa": 0.0,
                  "total": 1,
                  "total_methods": 4,
                  "coa": 0.25
                }
                "#
                );
            },
        );
    }

    #[test]
    fn groovy_no_methods() {
        check_metrics::<GroovyParser>("class A { int x = 1 }", "foo.groovy", |metric| {
            assert_eq!(metric.npm.total_nm(), 0);
        });
    }

    #[test]
    fn groovy_public_methods() {
        check_metrics::<GroovyParser>(
            "class A {
                public void m1() {}
                public int m2() { return 0 }
                private void m3() {}
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    #[test]
    fn groovy_interface_methods_implicitly_public() {
        // Asserting only the body-walker `interface_*_sum` totals
        // would pass vacuously if `InterfaceDeclaration` were dropped
        // from `GroovyCode::is_func_space`. The structural
        // `assert_child_space_kind` call catches that revert by
        // requiring the interface to actually open an `Interface`
        // FuncSpace.
        check_func_space::<GroovyParser, _>(
            "interface I {
                void a()
                int b()
            }",
            "foo.groovy",
            |func_space| {
                let metric = &func_space.metrics;
                // Interface methods are implicitly public.
                assert_eq!(metric.npm.interface_nm_sum(), 2);
                assert_eq!(metric.npm.interface_npm_sum(), 2);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    // Regression for issue #280: Groovy mirrors Java's enum / record /
    // annotation method counting.
    #[test]
    fn groovy_enum_counts_methods() {
        check_metrics::<GroovyParser>(
            "enum Status {
                ACTIVE, INACTIVE;
                public int code() { return 0 }
                private void reset() {}
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
            },
        );
    }

    #[test]
    #[ignore = "dekobon Groovy grammar v1 does not support annotation type elements with `default` values; the trailing `default \"\"`/`default 0` make the body fail to parse"]
    fn groovy_annotation_type_counts_elements() {
        // The Groovy tree-sitter grammar parses `@interface` only when
        // preceded by a modifier and when each element ends in `;` (it
        // inherits the Java parser's strictness). This source shape
        // produces a clean `annotation_type_declaration` →
        // `annotation_type_body` → `annotation_type_element_declaration`
        // tree. Mirror of `java_annotation_type_counts_elements` — the
        // body-walker count is identical whether or not Groovy's
        // `AnnotationTypeDeclaration` is wired into `is_func_space`,
        // so the structural `check_func_space` assertion is what
        // catches a revert.
        check_func_space::<GroovyParser, _>(
            "public @interface Marker {
                String value() default \"\";
                int priority() default 0;
            }",
            "foo.groovy",
            |func_space| {
                assert_eq!(func_space.metrics.npm.interface_nm_sum(), 2);
                assert_eq!(func_space.metrics.npm.interface_npm_sum(), 2);
                assert_child_space_kind(&func_space, "Marker", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn groovy_constructors() {
        check_metrics::<GroovyParser>(
            "class X {
                X() {}
                private X(int a) {}
                protected X(int a, int b) {}
                public X(int a, int b, int c) {}
            }",
            "foo.groovy",
            |metric| {
                // 4 constructors total, 1 public
                assert_eq!(metric.npm.class_nm_sum(), 4);
                assert_eq!(metric.npm.class_npm_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_no_methods_in_unit_scope() {
        check_metrics::<GroovyParser>("int x = 1", "foo.groovy", |metric| {
            assert_eq!(metric.npm.total_nm(), 0);
        });
    }

    #[test]
    fn groovy_multiple_classes_methods() {
        check_metrics::<GroovyParser>(
            "class A { public void a() {} }
            class B { public void b() {} }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    #[test]
    fn groovy_methods_returning_primitive_types() {
        // Mirror of `java_methods_returning_primitive_types`. Each
        // method declaration is counted regardless of return type;
        // `public` modifier promotes to NPM.
        check_metrics::<GroovyParser>(
            "class X {
                public byte a() {}
                public int b() {}
                public double c() {}
                public boolean d() {}
                byte e() {}
                int f() {}
            }",
            "foo.groovy",
            |metric| {
                // 6 methods, 4 public.
                assert_eq!(metric.npm.class_nm_sum(), 6);
                assert_eq!(metric.npm.class_npm_sum(), 4);
            },
        );
    }

    #[test]
    fn groovy_methods_with_generic_types() {
        // Methods with generic parameter/return types.
        check_metrics::<GroovyParser>(
            "class X {
                public List<String> a() {}
                public Map<String, Integer> b() {}
                List<Integer> c() {}
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    #[test]
    fn groovy_method_modifiers() {
        // Modifier ordering doesn't matter — what matters is
        // whether the `Modifiers` block contains `Public`. Mirrors
        // `java_method_modifiers`.
        check_metrics::<GroovyParser>(
            "abstract class X {
                public static void a() {}
                static public void b() {}
                public final void c() {}
                final public void d() {}
                protected static void e() {}
                static protected void f() {}
                abstract public void g()
                abstract void h()
            }",
            "foo.groovy",
            |metric| {
                // 8 methods, 5 public.
                assert_eq!(metric.npm.class_nm_sum(), 8);
                assert_eq!(metric.npm.class_npm_sum(), 5);
            },
        );
    }

    #[test]
    #[ignore = "dekobon Groovy grammar v1 does not yet support inner classes inside class bodies"]
    fn groovy_nested_inner_classes() {
        // Each nested `class` declaration is its own class space.
        // Mirrors `java_nested_inner_classes`.
        check_metrics::<GroovyParser>(
            "class X {
                public void a() {}
                class Y {
                    public void b() {}
                    class Z {
                        public void c() {}
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // 3 classes, 3 public methods (one per class).
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 3);
            },
        );
    }

    #[test]
    #[ignore = "dekobon Groovy grammar v1 does not yet support anonymous inner classes (`new T() { … }`)"]
    fn groovy_anonymous_inner_class() {
        // Anonymous inner class via `new T() { ... }`. Its methods
        // are counted in a separate class space.
        check_metrics::<GroovyParser>(
            "class X {
                public Runnable r = new Runnable() {
                    public void run() {}
                    void helper() {}
                }
            }",
            "foo.groovy",
            |metric| {
                // Inner anonymous: 2 methods (run + helper), 1 public
                // (run). Outer X has no methods.
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_interfaces_and_class() {
        // Mixed interfaces + class. Interface methods are
        // implicitly public; class methods need explicit `public`.
        // Mirrors `java_interfaces_and_class`. Structural
        // `assert_child_space_kind` guards against an
        // `InterfaceDeclaration` revert (see #311).
        check_func_space::<GroovyParser, _>(
            "interface X {
                void a()
            }
            interface Y extends X {
                void b()
                void c()
            }
            class Z implements Y {
                public void a() {}
                public void b() {}
                public void c() {}
                void d() {}
                void e() {}
            }",
            "foo.groovy",
            |func_space| {
                let metric = &func_space.metrics;
                // Interfaces: 3 total methods (a, b, c), all 3 public.
                assert_eq!(metric.npm.interface_nm_sum(), 3);
                assert_eq!(metric.npm.interface_npm_sum(), 3);
                // Class Z: 5 methods, 3 public (a, b, c — d, e are
                // package-private).
                assert_eq!(metric.npm.class_nm_sum(), 5);
                assert_eq!(metric.npm.class_npm_sum(), 3);
                assert_child_space_kind(&func_space, "X", SpaceKind::Interface);
                assert_child_space_kind(&func_space, "Y", SpaceKind::Interface);
                assert_child_space_kind(&func_space, "Z", SpaceKind::Class);
            },
        );
    }

    #[test]
    fn java_methods_returning_primitive_types() {
        check_metrics::<JavaParser>(
            "class X {
                public byte a() {}      // +1
                public short b() {}     // +1
                public int c() {}       // +1
                public long d() {}      // +1
                public float e() {}     // +1
                public double f() {}    // +1
                public boolean g() {}   // +1
                public char h() {}      // +1
                byte i() {}
                short j() {}
                int k() {}
                long l() {}
                float m() {}
                double n() {}
                boolean o() {}
                char p() {}
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 8,
                  "interface_npm_sum": 0,
                  "class_methods": 16,
                  "interface_methods": 0,
                  "class_coa": 0.5,
                  "interface_coa": 0.0,
                  "total": 8,
                  "total_methods": 16,
                  "coa": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_methods_returning_arrays() {
        check_metrics::<JavaParser>(
            "class X {
                public byte[] a() {}    // +1
                public short[] b() {}   // +1
                public int[] c() {}     // +1
                public long[] d() {}    // +1
                public float[] e() {}   // +1
                public double[] f() {}  // +1
                public boolean[] g() {} // +1
                public char[] h() {}    // +1
                byte[] i() {}
                short[] j() {}
                int[] k() {}
                long[] l() {}
                float[] m() {}
                double[] n() {}
                boolean[] o() {}
                char[] p() {}
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 8,
                  "interface_npm_sum": 0,
                  "class_methods": 16,
                  "interface_methods": 0,
                  "class_coa": 0.5,
                  "interface_coa": 0.0,
                  "total": 8,
                  "total_methods": 16,
                  "coa": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_methods_returning_objects() {
        check_metrics::<JavaParser>(
            "class X {
                public Integer[] a() {} // +1
                public Integer b() {}   // +1
                public String[] c() {}  // +1
                public String d() {}    // +1
                public Y[] e() {}       // +1
                public Y f() {}         // +1
                Integer[] g() {}
                Integer h() {}
                String[] i() {}
                String j() {}
                Y[] k() {}
                Y l() {}
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 6,
                  "interface_npm_sum": 0,
                  "class_methods": 12,
                  "interface_methods": 0,
                  "class_coa": 0.5,
                  "interface_coa": 0.0,
                  "total": 6,
                  "total_methods": 12,
                  "coa": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_methods_with_generic_types() {
        check_metrics::<JavaParser>(
            "class X {
                public <T, S extends T> void a(T x, S y) {} // +1
                public <T, S> int b(T x, S y) {}            // +1
                public <T> boolean c(T x) {}                // +1
                public <T> ArrayList<T> d() {}              // +1
                public Y<String> e() {}                     // +1
                <T, S extends T> void f(T x, S y) {}
                <T, S> int g(T x, S y) {}
                <T> boolean h(T x) {}
                <T> ArrayList<T> i() {}
                Y<String> j() {}
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 5,
                  "interface_npm_sum": 0,
                  "class_methods": 10,
                  "interface_methods": 0,
                  "class_coa": 0.5,
                  "interface_coa": 0.0,
                  "total": 5,
                  "total_methods": 10,
                  "coa": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_method_modifiers() {
        check_metrics::<JavaParser>(
            "abstract class X {
                public static final synchronized strictfp void a() {}   // +1
                static public final synchronized strictfp void b() {}   // +1
                static final public synchronized strictfp void c() {}   // +1
                static final synchronized public strictfp void d() {}   // +1
                static final synchronized strictfp public void e() {}   // +1
                protected static final synchronized native void f();
                static protected final synchronized native void g();
                static final protected synchronized native void h();
                static final synchronized protected native void i();
                static final synchronized native protected void j();
                abstract public void k();                               // +1
                abstract void l();
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 6,
                  "interface_npm_sum": 0,
                  "class_methods": 12,
                  "interface_methods": 0,
                  "class_coa": 0.5,
                  "interface_coa": 0.0,
                  "total": 6,
                  "total_methods": 12,
                  "coa": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_classes() {
        check_metrics::<JavaParser>(
            "class X {
                public void a() {}  // +1
                public void b() {}  // +1
                private void c() {}
            }
            class Y {
                private void d() {}
                private void e() {}
                public void f() {}  // +1
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 3,
                  "interface_npm_sum": 0,
                  "class_methods": 6,
                  "interface_methods": 0,
                  "class_coa": 0.5,
                  "interface_coa": 0.0,
                  "total": 3,
                  "total_methods": 6,
                  "coa": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_nested_inner_classes() {
        check_metrics::<JavaParser>(
            "class X {
                public void a() {}          // +1
                class Y {
                    public void b() {}      // +1
                    class Z {
                        public void c() {}  // +1
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 3,
                  "interface_npm_sum": 0,
                  "class_methods": 3,
                  "interface_methods": 0,
                  "class_coa": 1.0,
                  "interface_coa": 0.0,
                  "total": 3,
                  "total_methods": 3,
                  "coa": 1.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_local_inner_classes() {
        check_metrics::<JavaParser>(
            "class X {
                public void a() {                   // +1
                    class Y {
                        public void b() {           // +1
                            class Z {
                                public void c() {}  // +1
                            }
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 3,
                  "interface_npm_sum": 0,
                  "class_methods": 3,
                  "interface_methods": 0,
                  "class_coa": 1.0,
                  "interface_coa": 0.0,
                  "total": 3,
                  "total_methods": 3,
                  "coa": 1.0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_anonymous_inner_classes() {
        check_metrics::<JavaParser>(
            "abstract class X {
                public abstract void a();   // +1
            }
            abstract class Y {
                abstract void b();
            }
            class Z {
                public void c(){            // +1
                    X x = new X() {
                        @Override
                        public void a() {}  // +1
                    };
                    Y y = new Y() {
                        @Override
                        void b() {}
                    };
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 3,
                  "interface_npm_sum": 0,
                  "class_methods": 5,
                  "interface_methods": 0,
                  "class_coa": 0.6,
                  "interface_coa": 0.0,
                  "total": 3,
                  "total_methods": 5,
                  "coa": 0.6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_interface() {
        check_metrics::<JavaParser>(
            "interface X {
                public int a(); // +1
                boolean b();    // +1
                void c();       // +1
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 0,
                  "interface_npm_sum": 3,
                  "class_methods": 0,
                  "interface_methods": 3,
                  "class_coa": 0.0,
                  "interface_coa": 1.0,
                  "total": 3,
                  "total_methods": 3,
                  "coa": 1.0
                }
                "#
                );
            },
        );
    }

    // Regression for issue #280: Java enum bodies hold methods after
    // the constants. The Npm body walker recognises
    // `EnumBodyDeclarations` and treats it like `ClassBody`.
    #[test]
    fn java_enum_counts_methods() {
        check_metrics::<JavaParser>(
            "enum Status {
                ACTIVE, INACTIVE;
                public int code() { return 0; }     // +1 public
                private void reset() {}             // not public
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
            },
        );
    }

    // Regression for issue #280: Java records can declare methods in
    // their explicit body; they share `ClassBody`'s walker.
    #[test]
    fn java_record_counts_methods() {
        check_metrics::<JavaParser>(
            "record Point(int x, int y) {
                public int sum() { return x + y; }
                public Point() { this(0, 0); }
            }",
            "foo.java",
            |metric| {
                // `JavaCode::is_func` accepts both `MethodDeclaration`
                // and `ConstructorDeclaration`, so the body contributes
                // one method (`sum`) plus one explicit constructor
                // (`Point()`) = 2 total, both annotated `public`.
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    /// The same for #1160, which added `CompactConstructorDeclaration` to
    /// `JavaCode::is_func`: a record's compact constructor joins the body
    /// walker's method count on the same footing as the canonical
    /// spelling. The `public` modifier check reads `child(0)`, and the
    /// compact form carries its optional `modifiers` node in that same
    /// slot, so the visibility half transfers unchanged.
    ///
    /// `half` is the control that keeps the two sums apart — without a
    /// non-public member, `class_nm_sum == class_npm_sum` and a bug that
    /// counted every member as public would still pass.
    #[test]
    fn java_record_counts_a_compact_constructor_as_a_method() {
        check_metrics::<JavaParser>(
            "record R(int a, int b) {
                public R { }
                private int half() { return a / 2; }
                public int sum() { return a + b; }
            }",
            "foo.java",
            |metric| {
                // Compact constructor + `half` + `sum` = 3 methods, of
                // which the constructor and `sum` are public. Pre-fix the
                // compact constructor was not a function at all, so these
                // read 2 and 1.
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    #[test]
    fn java_annotation_type_counts_elements() {
        // Asserting only the body-walker counts (`interface_nm_sum`,
        // `interface_npm_sum`) would pass vacuously if
        // `AnnotationTypeDeclaration` were dropped from
        // `JavaCode::is_func_space`: with no `SpaceKind::Interface`
        // opened, the file-level Unit would still report 2.0 for both
        // sums (the body walker counts `AnnotationTypeElementDeclaration`
        // regardless of the surrounding space). The `check_func_space`
        // assertion catches that revert by requiring the annotation
        // type to actually open an `Interface` FuncSpace.
        check_func_space::<JavaParser, _>(
            "@interface Marker {
                String value() default \"\";
                int priority() default 0;
            }",
            "foo.java",
            |func_space| {
                assert_eq!(func_space.metrics.npm.interface_nm_sum(), 2);
                assert_eq!(func_space.metrics.npm.interface_npm_sum(), 2);
                assert_child_space_kind(&func_space, "Marker", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn java_interfaces_and_class() {
        check_metrics::<JavaParser>(
            "interface X {
                void a();           // +1
            }
            interface Y extends X {
                void b();           // +1
                void c();           // +1
            }
            class Z implements Y {
                @Override
                public void a() {}  // +1
                @Override
                public void b() {}  // +1
                @Override
                public void c() {}  // +1
                void d() {}
                void e() {}
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r#"
                {
                  "class_npm_sum": 3,
                  "interface_npm_sum": 3,
                  "class_methods": 5,
                  "interface_methods": 3,
                  "class_coa": 0.6,
                  "interface_coa": 1.0,
                  "total": 6,
                  "total_methods": 8,
                  "coa": 0.75
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_constructors() {
        check_metrics::<CsharpParser>(
            "class A {
                public A() {}
                public A(int x) {}
                A(int x, int y) {}
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_methods_returning_primitive_types() {
        check_metrics::<CsharpParser>(
            "class A {
                public int M1() { return 1; }
                public bool M2() { return true; }
                public double M3() { return 0.0; }
                int M4() { return 0; }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_methods_returning_arrays() {
        check_metrics::<CsharpParser>(
            "class A {
                public int[] M1() { return new int[0]; }
                public string[] M2() { return new string[0]; }
                int[] M3() { return new int[0]; }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_methods_returning_objects() {
        check_metrics::<CsharpParser>(
            "class Point { }
             class A {
                public Point M1() { return new Point(); }
                public string M2() { return \"\"; }
                Point M3() { return new Point(); }
             }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_methods_with_generic_types() {
        check_metrics::<CsharpParser>(
            "class A {
                public System.Collections.Generic.List<int> M1() { return null; }
                public System.Collections.Generic.Dictionary<string, int> M2() { return null; }
                System.Collections.Generic.List<string> M3() { return null; }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_method_modifiers() {
        check_metrics::<CsharpParser>(
            "class A {
                public void M1() {}
                private void M2() {}
                protected void M3() {}
                internal void M4() {}
                public static void M5() {}
                public virtual void M6() {}
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_classes() {
        check_metrics::<CsharpParser>(
            "class A {
                public void M1() {}
                public void M2() {}
                void M3() {}
            }
            class B {
                public int N() { return 0; }
                int Hidden() { return 0; }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_nested_inner_classes() {
        check_metrics::<CsharpParser>(
            "class Outer {
                public void M() {}
                void Hidden() {}
                public class Inner {
                    public void N() {}
                    void HiddenN() {}
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_property_accessors() {
        // EC7 — each property accessor (get/set/init) counts as a method.
        // `W` is an expression-bodied property — no AccessorList, just an
        // ArrowExpressionClause — and exercises the `.max(1)` fallback in
        // `csharp_count_member` that keeps such properties at 1 method.
        check_metrics::<CsharpParser>(
            "class A {
                int _w;
                public int X { get; set; }
                public int Y { get; }
                public int Z { get; init; }
                public int W => _w;
                int Hidden { get; set; }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_narrowed_accessor_visibility() {
        // #783 — a C# accessor inherits the member's visibility unless it
        // narrows it with its own `private` / `protected` modifier. A
        // narrowed accessor still counts as a method (nm) but is NOT a
        // public method (npm). Members exercised:
        //   X  public { get; private set; }   nm 2, npm 1 (get only)
        //   Idx public this[...] { get; protected set; } nm 2, npm 1
        //   Y  public { get; set; }           nm 2, npm 2 (unchanged guard)
        //   W  public { get; }                nm 1, npm 1 (auto-property)
        //   Z  public => 0                    nm 1, npm 1 (expression body)
        //   P  (no modifier) { get; set; }    nm 2, npm 0 (private member)
        // expected nm  = 2 + 2 + 2 + 1 + 1 + 2 = 10
        // expected npm = 1 + 1 + 2 + 1 + 1 + 0 = 6
        check_metrics::<CsharpParser>(
            "class A {
                public int X { get; private set; }
                public int this[int i] { get; protected set; }
                public int Y { get; set; }
                public int W { get; }
                public int Z => 0;
                int P { get; set; }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 10, "all accessors count as nm");
                assert_eq!(
                    metric.npm.class_npm_sum(),
                    6,
                    "narrowed private/protected accessors are not public methods"
                );
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn csharp_local_functions() {
        // Local functions inside a method body are nested function spaces;
        // they don't count toward the enclosing class's NoM/NPM. The
        // private sibling `Hidden` ensures the visibility gate is also
        // exercised: nm should be 2 (Outer + Hidden), npm should be 1
        // (only Outer is `public`). If the local function leaked into
        // the enclosing class's count, nm would be 3.
        check_metrics::<CsharpParser>(
            "class A {
                public void Outer() {
                    void Local() {}
                    Local();
                }
                private void Hidden() {}
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2, "Local must not leak");
                assert_eq!(metric.npm.class_npm_sum(), 1, "only Outer is public");
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn csharp_interface() {
        // EC14 — interface methods default to public.
        check_metrics::<CsharpParser>(
            "interface I {
                int M1();
                bool M2();
                int X { get; set; }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn csharp_interfaces_and_class() {
        check_metrics::<CsharpParser>(
            "interface I1 { int M1(); }
            interface I2 { bool M2(); float M3(); }
            class A {
                public void M() {}
                void Hidden() {}
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_no_class_methods() {
        check_metrics::<PhpParser>(
            "<?php class A { public int $x = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_one_public_method() {
        check_metrics::<PhpParser>(
            "<?php class A { public function f(): void {} }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_one_private_method() {
        check_metrics::<PhpParser>(
            "<?php class A { private function f(): void {} }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_one_protected_method() {
        check_metrics::<PhpParser>(
            "<?php class A { protected function f(): void {} }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_mixed_visibility_methods() {
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public function a(): void {}
                public function b(): void {}
                private function c(): void {}
                protected function d(): void {}
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_static_public_method() {
        check_metrics::<PhpParser>(
            "<?php class A { public static function f(): void {} }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_abstract_method() {
        check_metrics::<PhpParser>(
            "<?php abstract class A { abstract public function f(): void; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_final_public_method() {
        check_metrics::<PhpParser>(
            "<?php class A { final public function f(): void {} }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_interface_methods() {
        // Interface methods are implicitly public.
        check_metrics::<PhpParser>(
            "<?php
            interface I {
                public function a(): void;
                public function b(): int;
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_enum_methods() {
        // Enum can declare public methods (PHP 8.1+).
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
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_trait_methods() {
        check_metrics::<PhpParser>(
            "<?php
            trait T {
                public function a(): void {}
                private function b(): void {}
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    #[test]
    fn php_no_explicit_visibility_method_excluded() {
        // Methods without explicit visibility (which PHP treats as public)
        // are NOT counted under the strict-explicit rule.
        check_metrics::<PhpParser>(
            "<?php class A { function f(): void {} }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npm),
        );
    }

    // --- Kotlin NPM tests -------------------------------------------------

    #[test]
    fn kotlin_empty_class_no_methods() {
        check_metrics::<KotlinParser>("class C {}", "foo.kt", |metric| {
            assert_eq!(metric.npm.class_npm_sum(), 0);
            assert_eq!(metric.npm.class_nm_sum(), 0);
            assert_eq!(metric.npm.interface_nm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn kotlin_public_methods_default() {
        // Kotlin default visibility is public — no modifier means public.
        check_metrics::<KotlinParser>(
            "class C {
                fun a() {}
                fun b(): Int = 0
                fun c(x: Int): Int = x
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 3);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_private_method() {
        check_metrics::<KotlinParser>(
            "class C {
                fun a() {}                  // public
                private fun b() {}          // private
                fun c() {}                  // public
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_protected_internal_methods() {
        check_metrics::<KotlinParser>(
            "open class C {
                protected fun a() {}
                internal fun b() {}
                public fun c() {}
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_secondary_constructor_counts() {
        // Secondary constructors are explicit `secondary_constructor`
        // nodes; they count as methods (matching the Java rule).
        check_metrics::<KotlinParser>(
            "class C {
                private var a: Int = 0
                constructor(n: Int) { a = n }
                constructor(n: Int, m: Int) { a = n + m }
                fun get(): Int = a
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 3);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_companion_object_methods() {
        // Companion object methods fold into the enclosing class (static
        // members).
        check_metrics::<KotlinParser>(
            "class Holder {
                fun memberFn() {}
                companion object {
                    fun staticFn() {}
                    private fun secret() {}
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_data_class_methods() {
        // `data class` compiler-generated members are NOT counted —
        // only user-written `fun` declarations.
        check_metrics::<KotlinParser>(
            "data class Point(val x: Int, val y: Int) {
                fun manhattan(): Int = x + y
                private fun internal_(): Int = 0
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_object_singleton_methods() {
        check_metrics::<KotlinParser>(
            "object Util {
                fun add(a: Int, b: Int): Int = a + b
                private fun helper(): Int = 0
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_interface_methods() {
        check_func_space::<KotlinParser, _>(
            "interface I {
                fun work(): Int
                fun describe(): String
            }",
            "foo.kt",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npm.interface_npm_sum(), 2);
                assert_eq!(metric.npm.interface_nm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 0);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn kotlin_interface_with_default_method() {
        check_func_space::<KotlinParser, _>(
            "interface I {
                fun abs(n: Int): Int {
                    return if (n < 0) -n else n
                }
                fun pure(): Int
            }",
            "foo.kt",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npm.interface_npm_sum(), 2);
                assert_eq!(metric.npm.interface_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn kotlin_override_fun_counts() {
        check_metrics::<KotlinParser>(
            "open class Base {
                open fun greet(): String = \"hi\"
            }
            class Sub : Base() {
                override fun greet(): String = \"yo\"
                private fun secret() {}
            }",
            "foo.kt",
            |metric| {
                // Base: 1 method (public).
                // Sub: 2 methods — override (public, no visibility modifier
                //   so default public) + private secret.
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_nested_class_methods() {
        check_metrics::<KotlinParser>(
            "class Outer {
                fun outerM() {}
                class Nested {
                    fun nestedM() {}
                    private fun nestedSecret() {}
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_inner_class_methods() {
        check_metrics::<KotlinParser>(
            "class Outer {
                fun outerM() {}
                inner class Inner {
                    fun innerM() {}
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_top_level_function_excluded() {
        // Top-level `fun` belongs to `Unit`, not any class.
        check_metrics::<KotlinParser>(
            "fun freeFn() {}
class C {
    fun m() {}
}",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_extension_function_excluded() {
        // Extension functions parse as top-level `function_declaration`
        // with a receiver-type prefix; they belong to the `Unit` space.
        check_metrics::<KotlinParser>(
            "fun List<Int>.sum2(): Int = this.size
class C {
    fun m() {}
}",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_class_in_interface() {
        // Interface with nested class — methods count to the right
        // bucket. Structural `assert_child_space_kind` guards both
        // the outer interface and the nested class against
        // `is_func_space` reverts (see #311).
        check_func_space::<KotlinParser, _>(
            "interface Outer {
                fun work(): Int
                class Helper {
                    fun help() {}
                }
            }",
            "foo.kt",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npm.interface_npm_sum(), 1);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "Outer", SpaceKind::Interface);
                let outer = func_space
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("Outer"))
                    .expect("Outer FuncSpace");
                assert_child_space_kind(outer, "Helper", SpaceKind::Class);
            },
        );
    }

    #[test]
    fn kotlin_interface_in_class() {
        // Class with nested interface — methods count to the right
        // bucket. Structural `assert_child_space_kind` guards both
        // the outer class and the nested interface against
        // `is_func_space` reverts (see #311).
        check_func_space::<KotlinParser, _>(
            "class Outer {
                fun work() {}
                interface Sub {
                    fun help(): Int
                }
            }",
            "foo.kt",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.interface_npm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "Outer", SpaceKind::Class);
                let outer = func_space
                    .spaces
                    .iter()
                    .find(|s| s.name.as_deref() == Some("Outer"))
                    .expect("Outer FuncSpace");
                assert_child_space_kind(outer, "Sub", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn kotlin_init_block_not_a_method() {
        // `init` blocks are anonymous initializers — they are not
        // function declarations and don't count toward `nm`/`npm`.
        check_metrics::<KotlinParser>(
            "class C(val n: Int) {
                init { require(n >= 0) }
                fun get(): Int = n
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    // --- TypeScript / TSX NPM tests --------------------------------------
    //
    // TypeScript class methods are `method_definition` direct children of
    // `class_body` (regular methods, static methods, constructors,
    // getters, setters). Each `method_definition` counts once.
    // `abstract_method_signature` (abstract method declaration with no
    // body) is also counted. A `public_field_definition` whose value is
    // an `arrow_function` is a class method written as a field
    // initializer and counts once. Method overload signatures
    // (`method_signature` as class_body children) are NOT counted —
    // the implementation `method_definition` is the canonical method.
    // Interface methods (`method_signature`, `abstract_method_signature`,
    // `construct_signature`) count as implicitly-public interface
    // methods.

    #[test]
    fn typescript_empty_class_no_methods() {
        check_metrics::<TypescriptParser>("class C {}", "foo.ts", |metric| {
            assert_eq!(metric.npm.class_npm_sum(), 0);
            assert_eq!(metric.npm.class_nm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn typescript_default_public_methods() {
        check_metrics::<TypescriptParser>(
            "class C {
                a(): void {}
                b(): number { return 0; }
                c(x: number): number { return x; }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 3);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_method_visibility() {
        check_metrics::<TypescriptParser>(
            "class C {
                public a(): void {}
                private b(): void {}
                protected c(): void {}
                d(): void {}
            }",
            "foo.ts",
            |metric| {
                // public + default-public = 2 npm; 4 nm.
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 4);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_static_methods() {
        check_metrics::<TypescriptParser>(
            "class C {
                static a(): void {}
                public static b(): void {}
                private static c(): void {}
            }",
            "foo.ts",
            |metric| {
                // a (default public) + b (public) = 2 npm.
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_constructor_counts_as_method() {
        // The constructor is a `method_definition` — one method.
        check_metrics::<TypescriptParser>(
            "class C {
                constructor(public x: number) {}
                m(): void {}
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_getter_setter_each_count_once() {
        // `get x()` and `set x(v)` are distinct `method_definition`
        // nodes — each counts as one method.
        check_metrics::<TypescriptParser>(
            "class C {
                private _x: number = 0;
                get x(): number { return this._x; }
                set x(v: number) { this._x = v; }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_arrow_field_counts_as_method() {
        // `foo = () => {}` is a class method.
        check_metrics::<TypescriptParser>(
            "class C {
                a: number = 0;
                arrow = () => this.a;
                private secret = () => this.a;
            }",
            "foo.ts",
            |metric| {
                // 2 methods (arrow public, secret private). 1 field.
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_method_overload_counts_once() {
        // Only the implementation `method_definition` counts; the two
        // signature-only `method_signature` overloads do not.
        check_metrics::<TypescriptParser>(
            "class C {
                m(x: number): void;
                m(x: string): void;
                m(x: any): void {}
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_abstract_class_methods() {
        // Abstract method signatures count; concrete methods count; both
        // contribute to `nm`. `public` abstract method is public.
        check_metrics::<TypescriptParser>(
            "abstract class C {
                abstract a(): void;
                public abstract b(): number;
                protected abstract c(): void;
                public m(): void {}
                private n(): void {}
            }",
            "foo.ts",
            |metric| {
                // a (default public abstract), b (public), m (public) = 3 npm.
                // c (protected), n (private) demoted. Total nm = 5.
                assert_eq!(metric.npm.class_npm_sum(), 3);
                assert_eq!(metric.npm.class_nm_sum(), 5);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_interface_methods() {
        // Interface method signatures are implicitly public.
        check_func_space::<TypescriptParser, _>(
            "interface I {
                a(): void;
                b(x: number): number;
                c: string;
            }",
            "foo.ts",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npm.interface_npm_sum(), 2);
                assert_eq!(metric.npm.interface_nm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 0);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn typescript_generic_class_methods() {
        check_metrics::<TypescriptParser>(
            "class Box<T> {
                value: T;
                set(v: T): void { this.value = v; }
                get(): T { return this.value; }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_multiple_classes_and_interface() {
        check_func_space::<TypescriptParser, _>(
            "class A { m(): void {} }
             class B { private h(): void {} }
             interface I { p(): number; }",
            "foo.ts",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.interface_npm_sum(), 1);
                assert_eq!(metric.npm.interface_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "A", SpaceKind::Class);
                assert_child_space_kind(&func_space, "B", SpaceKind::Class);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    // TSX parity

    #[test]
    fn tsx_empty_class_no_methods() {
        check_metrics::<TsxParser>("class C {}", "foo.tsx", |metric| {
            assert_eq!(metric.npm.class_npm_sum(), 0);
            assert_eq!(metric.npm.class_nm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn tsx_default_public_methods() {
        check_metrics::<TsxParser>(
            "class C {
                a(): void {}
                b(): number { return 0; }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_method_visibility() {
        check_metrics::<TsxParser>(
            "class C {
                public a(): void {}
                private b(): void {}
                protected c(): void {}
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_static_methods() {
        check_metrics::<TsxParser>(
            "class C {
                static a(): void {}
                private static b(): void {}
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_constructor_counts_as_method() {
        check_metrics::<TsxParser>(
            "class C {
                constructor() {}
                m(): void {}
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_getter_setter_each_count_once() {
        check_metrics::<TsxParser>(
            "class C {
                private _x: number = 0;
                get x(): number { return this._x; }
                set x(v: number) { this._x = v; }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_arrow_field_counts_as_method() {
        check_metrics::<TsxParser>(
            "class C {
                arrow = () => 1;
                private secret = () => 2;
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_method_overload_counts_once() {
        check_metrics::<TsxParser>(
            "class C {
                m(x: number): void;
                m(x: string): void;
                m(x: any): void {}
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_abstract_class_methods() {
        check_metrics::<TsxParser>(
            "abstract class C {
                abstract a(): void;
                public m(): void {}
                private n(): void {}
            }",
            "foo.tsx",
            |metric| {
                // a (default public) + m (public) = 2 npm; 3 nm.
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_interface_methods() {
        check_func_space::<TsxParser, _>(
            "interface I {
                a(): void;
                b(): number;
            }",
            "foo.tsx",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npm.interface_npm_sum(), 2);
                assert_eq!(metric.npm.interface_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn tsx_generic_class_methods() {
        check_metrics::<TsxParser>(
            "class Box<T> { value: T; set(v: T): void { this.value = v; } }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_multiple_classes_and_interface() {
        check_func_space::<TsxParser, _>(
            "class A { m(): void {} }
             class B { private h(): void {} }
             interface I { p(): number; }",
            "foo.tsx",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.interface_npm_sum(), 1);
                assert_eq!(metric.npm.interface_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "A", SpaceKind::Class);
                assert_child_space_kind(&func_space, "B", SpaceKind::Class);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    // --- Ruby NPM tests ---------------------------------------------------
    //
    // Ruby methods default to public. Visibility keywords (`private`,
    // `public`, `protected`) appear as bare `identifier` nodes in the
    // class body and flip the default for every subsequent declaration.
    // The argument-form (`private :foo`, `private def x`) is a `call`
    // node and does NOT change the body-wide flag.

    #[test]
    fn ruby_no_class_methods() {
        check_metrics::<RubyParser>("def foo\n  1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.npm.class_npm_sum(), 0);
            assert_eq!(metric.npm.class_nm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn ruby_one_public_method() {
        // No visibility keyword → default public.
        check_metrics::<RubyParser>(
            "class A\n  def f\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_one_private_method() {
        // Bare `private` flips visibility for `f`.
        check_metrics::<RubyParser>(
            "class A\n  private\n  def f\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 0);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_one_protected_method() {
        check_metrics::<RubyParser>(
            "class A\n  protected\n  def f\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 0);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_mixed_visibility_methods() {
        // `a` is public (default). `b` is private. `c` is public again
        // because the explicit `public` keyword resets the flag. `d` is
        // protected.
        check_metrics::<RubyParser>(
            "class A\n  def a\n    1\n  end\n  private\n  def b\n    1\n  end\n  public\n  def c\n    1\n  end\n  protected\n  def d\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 4);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_singleton_method_is_counted() {
        // `def self.x` and plain `def x` both count; default is public.
        check_metrics::<RubyParser>(
            "class A\n  def self.f\n    1\n  end\n  def g\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_singleton_class_methods() {
        // `class << self` opens a separate class space whose methods
        // count there. Outer class A has 0 methods.
        check_metrics::<RubyParser>(
            "class A\n  class << self\n    def s\n      1\n    end\n    def t\n      2\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_argument_form_visibility_does_not_flip() {
        // `private :y` is a `call` node (argument form). It does NOT
        // change the body-wide visibility, so `z` declared after it
        // remains public.
        check_metrics::<RubyParser>(
            "class A\n  def y\n    1\n  end\n  private :y\n  def z\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_multiple_classes() {
        check_metrics::<RubyParser>(
            "class A\n  def a\n    1\n  end\nend\nclass B\n  private\n  def b\n    1\n  end\n  def c\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                // A: 1 public method. B: 0 public, 2 total. Sum = 1/3.
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_module_methods_not_counted() {
        // `Module` is `Namespace`, not `Class` — its methods do not
        // contribute to NPM.
        check_metrics::<RubyParser>(
            "module M\n  def f\n    1\n  end\n  def g\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 0);
                assert_eq!(metric.npm.class_nm_sum(), 0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_class_with_inheritance() {
        // Inheritance does not change method counts.
        check_metrics::<RubyParser>(
            "class A < B\n  def f\n    1\n  end\n  def g\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_visibility_resets_between_classes() {
        // Each class body starts in default-public state regardless of
        // the previous body's trailing visibility.
        check_metrics::<RubyParser>(
            "class A\n  private\n  def a\n    1\n  end\nend\nclass B\n  def b\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                // A: 0 public, B: 1 public.
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_empty_class_no_methods() {
        check_metrics::<RubyParser>("class Empty\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.npm.class_npm_sum(), 0);
            assert_eq!(metric.npm.class_nm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    // ---------------------------------------------------------------
    // Default-impl placeholder smoke tests (audited in #188).
    //
    // Each test feeds a class / struct with public methods to a
    // language whose `Npm` is currently the default no-op. The
    // assertion pins the current 0 value with a TODO pointing at the
    // follow-up issue — when the real impl lands the assertion will
    // fire and force a test update.
    // ---------------------------------------------------------------

    // --- Python NPM ---------------------------------------------------

    #[test]
    fn python_empty_class_no_methods() {
        check_metrics::<PythonParser>("class C:\n    pass\n", "foo.py", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0);
            assert_eq!(metric.npm.class_npm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn python_class_methods_count() {
        // 3 `def`s inside the class body → 3 methods, all public.
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   def __init__(self):\n\
             \x20       pass\n\
             \x20   def m(self):\n\
             \x20       pass\n\
             \x20   def n(self):\n\
             \x20       pass\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn python_decorated_methods_count() {
        // `@property`, `@staticmethod`, `@classmethod`, custom
        // decorators all wrap a FunctionDefinition in
        // DecoratedDefinition. Each wrapper still counts as one method.
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   @property\n\
             \x20   def p(self):\n\
             \x20       return 1\n\
             \x20   @staticmethod\n\
             \x20   def s():\n\
             \x20       return 2\n\
             \x20   @classmethod\n\
             \x20   def c(cls):\n\
             \x20       return 3\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn python_async_method_counts() {
        // `async def m` parses as a FunctionDefinition with an Async
        // keyword child — still a method.
        check_metrics::<PythonParser>(
            "class C:\n    async def m(self):\n        return 1\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn python_nested_class_methods_independent() {
        // Outer.method belongs to Outer; Inner.inner_method belongs
        // to Inner; class_nm_sum aggregates across the file.
        check_metrics::<PythonParser>(
            "class Outer:\n\
             \x20   def method(self):\n\
             \x20       pass\n\
             \x20   class Inner:\n\
             \x20       def inner_method(self):\n\
             \x20           pass\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn python_module_level_function_is_not_method() {
        // `def f()` outside any class is a top-level function, not a
        // method.
        check_metrics::<PythonParser>(
            "def f():\n    pass\nclass C:\n    def m(self):\n        pass\n",
            "foo.py",
            |metric| {
                // Only `C.m` is a class method.
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn python_dunder_methods_count() {
        // `__init__`, `__repr__`, `__eq__` are dunder methods — public
        // by convention.
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   def __init__(self):\n\
             \x20       pass\n\
             \x20   def __repr__(self):\n\
             \x20       return 'C'\n\
             \x20   def __eq__(self, other):\n\
             \x20       return True\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn rust_empty_unit_no_methods() {
        check_metrics::<RustParser>("", "empty.rs", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0);
            assert_eq!(metric.npm.class_npm_sum(), 0);
            assert_eq!(metric.npm.interface_nm_sum(), 0);
            assert_eq!(metric.npm.interface_npm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn rust_impl_methods_count() {
        // 3 `fn`s in `impl Foo` body. `pub new` and `pub process` are
        // public; `helper` is private. → class_nm=3, class_npm=2.
        check_metrics::<RustParser>(
            "struct Foo;\n\
             impl Foo {\n\
             \x20   pub fn new() -> Self { Foo }\n\
             \x20   fn helper(&self) -> i32 { 0 }\n\
             \x20   pub fn process(&self) -> i32 { 0 }\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn rust_pub_self_is_private() {
        // Regression for #460. `pub(self)` / `pub(in self)` restrict to
        // the current module — semantically private, like no modifier.
        // Only the forms that widen visibility beyond the module count
        // as public: `pub`, `pub(crate)`, `pub(super)`, `pub(in <path>)`.
        // → 6 methods, 4 public (b, d, e, f); a, a2, c excluded.
        // Pre-fix the `pub(self)`/`pub(in self)` pair over-counted, so
        // class_npm_sum was 6 (revert-verified).
        check_metrics::<RustParser>(
            "struct S;\n\
             impl S {\n\
             \x20   pub(self) fn a(&self) {}\n\
             \x20   pub(in self) fn a2(&self) {}\n\
             \x20   pub(crate) fn b(&self) {}\n\
             \x20   pub(super) fn d(&self) {}\n\
             \x20   pub(in crate::x) fn e(&self) {}\n\
             \x20   pub fn f(&self) {}\n\
             \x20   fn c(&self) {}\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 7);
                assert_eq!(metric.npm.class_npm_sum(), 4);
            },
        );
    }

    #[test]
    fn rust_trait_methods_count() {
        // `fn draw(&self);` (signature only) + `fn area(&self) -> f64
        // { 0.0 }` (default body) → both are interface methods.
        // Trait methods are always public. → interface_nm=2,
        // interface_npm=2. Structural `assert_child_space_kind`
        // pins the trait FuncSpace against an `is_func_space`
        // revert (see #311).
        check_func_space::<RustParser, _>(
            "trait Drawable {\n\
             \x20   fn draw(&self);\n\
             \x20   fn area(&self) -> f64 { 0.0 }\n\
             }\n",
            "foo.rs",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npm.interface_nm_sum(), 2);
                assert_eq!(metric.npm.interface_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 0);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "Drawable", SpaceKind::Trait);
            },
        );
    }

    #[test]
    fn rust_module_level_function_not_method() {
        // Top-level `fn` is NOT a method. The npa/npm metric on a
        // Unit space stays disabled (no class/interface), so the
        // method count is zero.
        check_metrics::<RustParser>("fn f() {}\nfn g() {}\n", "foo.rs", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0);
            assert_eq!(metric.npm.interface_nm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn rust_multiple_impls_methods_aggregate() {
        // Two `impl Foo` blocks contribute 1 + 1 = 2 methods.
        check_metrics::<RustParser>(
            "struct Foo;\n\
             impl Foo { pub fn m1(&self) {} }\n\
             impl Foo { fn m2(&self) {} }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn rust_trait_impl_block_counts_methods() {
        // `impl Drawable for Foo` is also an `impl_item` — its methods
        // count toward class_nm of the impl. Trait impls and inherent
        // impls are not distinguished at the AST level (both parse as
        // `impl_item`). Structural `assert_child_space_kind` pins the
        // trait FuncSpace against an `is_func_space` revert
        // (see #311).
        check_func_space::<RustParser, _>(
            "struct Foo;\n\
             trait Drawable { fn draw(&self); }\n\
             impl Drawable for Foo { fn draw(&self) {} }\n",
            "foo.rs",
            |func_space| {
                let metric = &func_space.metrics;
                // Trait body: 1 signature method → interface_nm = 1.
                // Impl body: 1 fn `draw` → class_nm = 1.
                assert_eq!(metric.npm.interface_nm_sum(), 1);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
                assert_child_space_kind(&func_space, "Drawable", SpaceKind::Trait);
            },
        );
    }

    // ----- Go -----

    #[test]
    fn go_empty_unit_no_methods() {
        // No receiver methods → npm stays disabled, class_nm_sum = 0.
        check_metrics::<GoParser>("package main\n", "empty.go", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn go_method_declarations_count() {
        // Two `func (r Foo) ...` methods on the same receiver type →
        // class_nm_sum = 2. Go visibility is lexical (issue #458):
        // `DoX` is exported, `doY` is not, so class_npm_sum = 1.
        check_metrics::<GoParser>(
            "package main\n\
             type Foo struct{}\n\
             func (f Foo) DoX() {}\n\
             func (f Foo) doY() {}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn go_free_function_is_not_method() {
        // `func g() {}` has no receiver → NOT a method. class_nm_sum
        // stays at 0. The file has no method either, so npm stays
        // disabled (suppressed from JSON).
        check_metrics::<GoParser>(
            "package main\nfunc g() {}\nfunc h(x int) int { return x }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn go_methods_on_different_receivers_aggregate_at_unit() {
        // Go's flat space model cannot group methods by receiver, so
        // methods on `Foo` and `Bar` aggregate at the file level
        // → class_nm_sum = 3 (1 + 2).
        check_metrics::<GoParser>(
            "package main\n\
             type Foo struct{}\n\
             type Bar struct{}\n\
             func (f Foo) M1() {}\n\
             func (b Bar) M2() {}\n\
             func (b *Bar) M3() {}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn go_interface_methods_count_as_interface_nm() {
        // `interface { Read() error; Close() error }` declares two
        // method signatures → interface_nm = 2, interface_npm = 2.
        // Both names are exported (uppercase first char), so the
        // lexical export rule (issue #471) leaves npm == nm here;
        // `go_interface_methods_respect_export` covers the mixed case.
        //
        // Unlike Java / Kotlin / TS, Go interfaces do *not* open a
        // FuncSpace (`GoCode::is_func_space` only matches
        // `SourceFile` and the function kinds), so there is no
        // `SpaceKind::Interface` child to assert against here — the
        // body walker counts methods directly from the `interface_type`
        // AST node. The failure mode #311 guards against (a vacuous
        // pass when `InterfaceDeclaration` is dropped from
        // `is_func_space`) therefore does not apply to Go.
        check_metrics::<GoParser>(
            "package main\ntype RC interface { Read() error; Close() error }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npm.interface_nm_sum(), 2);
                assert_eq!(metric.npm.interface_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn go_interface_methods_respect_export() {
        // Go's lexical export rule applies to interface method names
        // too (issue #471). `Foo` and `Ünic` (Unicode uppercase first
        // char) are exported; `bar` is not. interface_nm counts all
        // three; interface_npm only the two exported. Revert-verified
        // against the old all-public arm (interface_npm_sum = 3).
        check_metrics::<GoParser>(
            "package main\ntype I interface { Foo(); bar(); Ünic() }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npm.interface_nm_sum(), 3);
                assert_eq!(metric.npm.interface_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn go_pointer_receiver_methods_count() {
        // Pointer-receiver methods (`func (r *Foo) M() {}`) parse as
        // MethodDeclaration the same way as value-receiver methods
        // → class_nm_sum = 2.
        check_metrics::<GoParser>(
            "package main\n\
             type Foo struct{}\n\
             func (f *Foo) Set() {}\n\
             func (f *Foo) Get() int { return 0 }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn go_npm_excludes_unexported() {
        // Mixed exported / unexported methods (issue #458). `Greet`
        // and `Ärger` (Unicode uppercase first char) are exported;
        // `helper` is not. nm counts all three, npm only the two
        // exported. Revert-verified against the old all-public code
        // (which scored class_npm_sum = 3).
        check_metrics::<GoParser>(
            "package main\n\
             type T struct{}\n\
             func (t *T) Greet() {}\n\
             func (t *T) helper() {}\n\
             func (t *T) Ärger() {}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    // ----- Elixir -----

    // Issue #275: Elixir `def` is public, `defp` is private. All
    // count toward `class_nm`; only the public ones bump `class_npm`.
    #[test]
    fn elixir_npm_def_is_public_defp_is_private() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def pub_one, do: 1\n  defp priv_one, do: 1\n  def pub_two(x), do: x\nend\n",
            "foo.ex",
            |metric| {
                // 3 methods, 2 public.
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    #[test]
    fn elixir_npm_defmacro_counts_as_public() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  defmacro pub_macro(x), do: x\n  defmacrop priv_macro(x), do: x\nend\n",
            "foo.ex",
            |metric| {
                // defmacro = public method, defmacrop = private method.
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
            },
        );
    }

    #[test]
    fn elixir_npm_multiple_def_clauses_each_count() {
        // Pattern-match clauses each form their own method head.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(0), do: :zero\n  def f(_), do: :other\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    #[test]
    fn elixir_npm_nested_defmodule_each_class() {
        check_metrics::<ElixirParser>(
            "defmodule Outer do\n  def o, do: 1\n  defmodule Inner do\n    def i, do: 1\n  end\nend\n",
            "foo.ex",
            |metric| {
                // Two classes, one public method each.
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    #[test]
    fn elixir_npm_user_macro_not_classified_as_method() {
        // User-defined `custom_def` is a defmacro (counts) but its
        // invocation `custom_def foo, do: ...` must NOT be classified
        // as a method.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  defmacro custom_def(name, body) do\n    quote do\n      def unquote(name), do: unquote(body)\n    end\n  end\n  custom_def foo, do: 1\nend\n",
            "foo.ex",
            |metric| {
                // Only `defmacro custom_def` is a method of Foo (the
                // inner `def unquote(name)` is wrapped in `quote` so
                // it does not lexically appear as a direct child of
                // the defmodule do_block).
                assert_eq!(metric.npm.class_nm_sum(), 1);
                assert_eq!(metric.npm.class_npm_sum(), 1);
            },
        );
    }

    #[test]
    fn elixir_npm_quoted_defs_do_not_inflate_method_count() {
        // Companion to `wmc::tests::elixir_wmc_quoted_defs_do_not_inflate_method_count`
        // (#310). The three `def` / `defp` calls inside the `quote do
        // … end` template do NOT count as methods of `Foo`. NPM has
        // always behaved this way via its direct-children scan; this
        // test pins the headline values so a future refactor of NPM
        // toward "walk all nested Function spaces" cannot silently
        // re-introduce the WMC/NPM disagreement that #310 fixed.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  defmacro multi do\n    quote do\n      def a, do: 1\n      def b, do: 2\n      defp c, do: 3\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // Only `defmacro multi` is a method (and public).
                assert_eq!(metric.npm.class_nm_sum(), 1);
                assert_eq!(metric.npm.class_npm_sum(), 1);
            },
        );
    }

    /// A `defmodule` inside a `quote` template still opens a class and
    /// still has its methods counted.
    ///
    /// This pins the equivalence the #1088 simplification rests on.
    /// `Npm::compute` used to gate on `is_func_space_with_code` before
    /// checking for the `defmodule` keyword, which cost a source-text
    /// scan per node and — for `def`-shaped calls — an ancestor walk
    /// asking whether the call sat inside a `quote`. That walk's answer
    /// was always discarded: `elixir_is_class_macro` is exactly
    /// `defmodule`, so the keyword check that follows admits precisely
    /// the nodes the gate would have, and rejects every node the walk
    /// was consulted for.
    ///
    /// The quoted `defmodule Inner` is the shape where a *different*
    /// reading of "is this a class space?" would show up: if the
    /// quote-template rule were ever extended to class macros, these
    /// counts would move.
    #[test]
    fn elixir_npm_counts_a_quoted_defmodule_as_a_class() {
        check_metrics::<ElixirParser>(
            "defmodule Outer do\n  defmacro gen do\n    quote do\n      defmodule Inner do\n        def a, do: 1\n        defp b, do: 2\n      end\n    end\n  end\nend\n",
            "outer.ex",
            |metric| {
                // `Outer` contributes `defmacro gen`; the quoted `Inner`
                // contributes `def a` (public) and `defp b` (private).
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    // ----- Objective-C -----

    #[test]
    fn objc_npm() {
        // ObjC has no method-privacy keyword: methods declared in
        // `@interface` are public (interface_npm), and every
        // `@implementation` method counts as public (class_npm) —
        // `privHelper`, defined but never declared, included. A free C
        // function (`cFunc`) defined inside `@implementation` is NOT a
        // method, so `class_nm` stays 3.
        check_metrics::<ObjcParser>(
            "@interface Foo : NSObject\n\
             - (void)pub1;\n\
             - (void)pub2;\n\
             @end\n\
             @implementation Foo\n\
             - (void)pub1 { }\n\
             - (void)pub2 { }\n\
             - (void)privHelper { }\n\
             void cFunc(void) { }\n\
             @end\n",
            "foo.m",
            |metric| {
                assert_eq!(metric.npm.interface_nm_sum(), 2);
                assert_eq!(metric.npm.interface_npm_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 3);
            },
        );
    }

    #[test]
    fn objc_npm_protocol() {
        // A `@protocol`'s methods after an `@optional` / `@required`
        // marker nest under a `qualified_protocol_interface_declaration`;
        // they must still count (regression for the direct-children walk
        // that missed `optDraw`).
        check_metrics::<ObjcParser>(
            "@protocol Drawable <NSObject>\n\
             - (void)draw;\n\
             @optional\n\
             - (void)optDraw;\n\
             @end\n",
            "foo.m",
            |metric| {
                assert_eq!(metric.npm.interface_nm_sum(), 2);
                assert_eq!(metric.npm.interface_npm_sum(), 2);
            },
        );
    }

    // ----- C++ -----

    #[test]
    fn cpp_empty_unit_no_methods() {
        // No code → no class spaces → npm = 0.
        check_metrics::<CppParser>("", "empty.cpp", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0);
            assert_eq!(metric.npm.class_npm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn cpp_class_methods_count() {
        // Two member functions (one defined inline, one declared only).
        // Both count. Defaults to private → class_npm = 0.
        check_metrics::<CppParser>(
            "class Foo {\n\
                 void method1() {}\n\
                 void method2();\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn cpp_constructors_and_destructors_count() {
        // Constructors and destructors are parsed as `declaration`
        // (not `field_declaration`) inside the class body because they
        // have no return type. Both still count as methods.
        check_metrics::<CppParser>(
            "class Foo {\n\
                 public:\n\
                     Foo();\n\
                     ~Foo();\n\
                     void method();\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn cpp_template_methods_count() {
        // `template<typename T> T foo(T x);` parses as
        // `template_declaration` wrapping a `declaration` whose
        // `function_declarator` is reached recursively.
        check_metrics::<CppParser>(
            "class Foo {\n\
                 public:\n\
                     template<typename T> T fn(T x);\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 1);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn cpp_struct_methods_default_public() {
        // `struct` defaults to public visibility. All three methods
        // count as public.
        check_metrics::<CppParser>(
            "struct Foo {\n\
                 void a();\n\
                 void b() {}\n\
                 Foo() {}\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn cpp_free_function_is_not_method() {
        // Top-level function — not inside any class — does not count
        // toward npm. The Unit space is not marked as a class space,
        // so npm stays at zero.
        check_metrics::<CppParser>("void free_fn() {}\n", "foo.cpp", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0);
            assert_eq!(metric.npm.class_npm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn cpp_mixed_visibility_methods() {
        // `class` defaults to private. Public section gets 1 method,
        // protected gets 1 (bucketed as non-public for npm), private
        // gets 1. Total: class_nm = 3, class_npm = 1.
        check_metrics::<CppParser>(
            "class Foo {\n\
                 public: void a();\n\
                 protected: void b();\n\
                 private: void c();\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn cpp_multiple_classes_aggregate_at_unit() {
        // File-level rollup: Foo has 2 methods, Bar has 1. Unit
        // class_nm_sum = 3.
        check_metrics::<CppParser>(
            "class Foo { public: void a(); void b() {} };\n\
             struct Bar { void c(); };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    // The C++ source shared by the `cpp_*` and `mozcpp_*` halves of the
    // #1258 regression pair. `.mozcpp` owns no file extension, so the
    // fork gets no integration-snapshot coverage and its clone of the
    // `TemplateDeclaration` arm can only be pinned against its
    // extension-owning sibling (grammar-dispatch, "sweep the rest").
    const TEMPLATE_METHOD_WITH_BODY: &str = "class C {\n\
         public:\n\
             template<typename T> T get() { return T{}; }\n\
             int plain() { return 1; }\n\
         };";

    // Conversion operators declared *without* a body, shared by the
    // `cpp_*` and `mozcpp_*` halves of the #1298 regression pair for
    // the same no-file-extension reason as above. Neither form has a
    // `function_declarator` or a `function_definition` anywhere in its
    // subtree — the plain one parses as `declaration > operator_cast`
    // and the templated one as `template_declaration > declaration >
    // operator_cast` — so before #1298 both were counted as neither
    // method nor attribute.
    //
    // Deliberately asymmetric on all three axes the arm can get wrong:
    // 2 public conversion operators against 1 private, so an arm that
    // ignored `current_is_public` lands on 3/3 and one that never
    // reached the private section on 2/2; and a real public data
    // member, so `class_na`/`class_npa` are 1/1 rather than the
    // default 0 that a leak into `Npa` would be indistinguishable
    // from.
    const CONVERSION_OPERATORS_WITHOUT_BODIES: &str = "class C {\n\
         public:\n\
             operator float();\n\
             template<typename T> operator T();\n\
             int width;\n\
         private:\n\
             operator double();\n\
         };";

    // Every `template_declaration` payload the C++ grammar admits in
    // class scope that is *not* a member function, per both grammars'
    // `node-types.json`: a nested templated class (`type_specifier`),
    // an `alias_declaration`, a templated static data member
    // (`declaration` with no function declarator), and a
    // `friend_declaration` — whose function is a free function the
    // class merely grants access to, not a member of it.
    //
    // `real()` and `Nested::hidden()` are present so the expected
    // totals are 2/1 rather than 0/0: a fixture that stopped parsing
    // scores the default on every field, and an all-zero expectation
    // cannot tell that apart from the payloads being correctly
    // ignored. That the two differ also exercises the visibility flag,
    // which an all-public fixture would leave pinned.
    //
    // `Nested` deliberately carries a *private* method rather than
    // being empty. Its own class space contributes 1/0 to the subtree
    // sums, so the expectation is 2/1 — and a helper that descended
    // through `class_specifier` *and* `field_declaration_list` into the
    // nested body would count the outer `template_declaration` as well
    // and reach 3/2. (Both arms are needed to break it; adding either
    // alone leaves the recursion one level short. Verified by
    // perturbation.) An empty `Nested` would leave that descent
    // untested in either direction.
    const NON_METHOD_TEMPLATE_PAYLOADS: &str = "class C {\n\
         public:\n\
             template<typename T> class Nested { void hidden() {} };\n\
             template<typename T> using Alias = T;\n\
             template<typename T> static T value;\n\
             template<typename T> friend void amigo() {}\n\
             template<typename T> T real() { return T{}; }\n\
         };";

    #[test]
    fn cpp_template_method_with_inline_body_counts() {
        // A templated member *with a body* parses as
        // `template_declaration > function_definition`, not the
        // `template_declaration > declaration` shape that
        // `cpp_template_methods_count` above pins. Before #1258 the
        // guard could only reach a `function_declarator`, so `get()`
        // scored zero: `npm` said the class had one method while `nom`
        // opened two function spaces and `wmc` weighted two.
        check_metrics_with_nom_wmc::<CppParser>(TEMPLATE_METHOD_WITH_BODY, "foo.cpp", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 2);
            assert_eq!(metric.npm.class_npm_sum(), 2);
            // Both members carry a body, so all three walks must
            // land on 2. Each method's cyclomatic is 1.
            assert_eq!(metric.nom.functions_sum(), 2);
            assert_eq!(metric.wmc.class_wmc_sum(), 2);
        });
    }

    #[test]
    fn cpp_template_method_with_inline_body_respects_visibility() {
        // Deliberately asymmetric — 2 public, 1 private. A template arm
        // that counted methods but ignored `current_is_public` lands on
        // 3/3, and one that never reached the private section lands on
        // 2/2; only the correct arm produces 3/2.
        check_metrics_with_nom_wmc::<CppParser>(
            "class C {\n\
             public:\n\
                 template<typename T> T a() { return T{}; }\n\
                 template<typename T> T b() { return T{}; }\n\
             private:\n\
                 template<typename T> T c() { return T{}; }\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.nom.functions_sum(), 3);
            },
        );
    }

    #[test]
    fn cpp_template_conversion_operator_with_body_counts() {
        // A conversion operator's declarator is an `operator_cast`, so
        // there is no `function_declarator` anywhere in this subtree.
        // This shape is `template_declaration > function_definition >
        // operator_cast`, and `cpp_declares_function` accepts the
        // `function_definition` outright: it is not in the helper's
        // recursion set, so dropping that alternative would score this
        // member zero even though #1298 has since taught the helper to
        // match `operator_cast` one level further down.
        check_metrics_with_nom_wmc::<CppParser>(
            "class C {\n\
             public:\n\
                 template<typename T> operator T() { return T{}; }\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 1);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.nom.functions_sum(), 1);
            },
        );
    }

    #[test]
    fn cpp_conversion_operators_without_bodies_count_as_methods() {
        check_metrics_with_npa::<CppParser>(
            CONVERSION_OPERATORS_WITHOUT_BODIES,
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
                // `width` only. The three conversion operators must
                // not have been swept into `Npa` on the way out of
                // being invisible to both.
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[test]
    fn mozcpp_conversion_operators_without_bodies_count_as_methods() {
        check_metrics_with_npa::<MozcppParser>(
            CONVERSION_OPERATORS_WITHOUT_BODIES,
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    // Function-pointer *data* members, shared by the `cpp_*` and
    // `mozcpp_*` halves of the #1300 regression pair for the same
    // no-file-extension reason as the two fixtures above.
    //
    // `int (*fp)(int);` nests as `field_declaration >
    // function_declarator > parenthesized_declarator >
    // pointer_declarator > field_identifier`, so an unconditional
    // `function_declarator` arm claimed it and both counters were
    // wrong in opposite directions at once: it scored as a method and
    // was skipped as an attribute.
    //
    // Every member is load-bearing:
    // - `plainData` is the control for the unwrapped path.
    // - `fps[4]` puts an `array_declarator` under the parenthesis,
    //   which the widened `cpp_count_field_identifiers` must compose
    //   with rather than stop at.
    // - `operator->()` reaches its `function_declarator` through a
    //   `pointer_declarator`; a gate that declined on any nesting at
    //   all would silently drop it.
    // - the two conversion operators are #1298's shapes, which carry
    //   no `function_declarator` anywhere in their subtree. They pin
    //   that the gate left the `operator_cast` arm they depend on
    //   alone.
    // - `(parenMethod)` and `(operator+)` are the boundary the gate
    //   introduces, and the reason it asks whether the parentheses
    //   interpose an *indirection* rather than merely whether they are
    //   there. Both are ordinary member functions written in the
    //   macro-defence idiom (`int (max)(int, int);`), and a gate that
    //   declined every parenthesised declarator demotes the first to
    //   an attribute and loses the second from both counters, its name
    //   being an `operator_name` the attribute counter does not match.
    // - the two `((doubleParen…))` members are the same distinction one
    //   nesting deeper, and the only fixtures that reach the helper's
    //   recursive `parenthesized_declarator` arm. They cover it in both
    //   directions: without the `*` the member is still a function,
    //   with it a field.
    // - the `private:` section makes public and total differ on both
    //   metrics, so neither pair can be reached by an arm that ignores
    //   `current_is_public`.
    const FUNCTION_POINTER_MEMBERS: &str = "class F {\n\
         public:\n\
             int (*fp)(int);\n\
             int plainData;\n\
             int (*fps[4])(int);\n\
             void realMethod();\n\
             Foo* operator->();\n\
             operator float();\n\
             template<typename T> operator T();\n\
             void (parenMethod)();\n\
             int (operator+)(int);\n\
             void ((doubleParenMethod))();\n\
             int ((*doubleParenFp))(int);\n\
         private:\n\
             int (*privFp)(int);\n\
             void privMethod();\n\
         };";

    // A member function whose *return type* is a function pointer.
    // `int (*getFp(int))(int);` nests like a function-pointer data
    // member for one level longer: the `parenthesized_declarator`
    // holds a `pointer_declarator` wrapping `getFp`'s own
    // `function_declarator`. `fp` sits alongside it so the fixture
    // separates the two readings: the pre-#1300 unconditional arm
    // counts both as methods, and a gate that declined every
    // parenthesised declarator outright counts neither — verified by
    // perturbation, which scores this class 0 methods.
    //
    // Each shape appears once per visibility, so all four expected
    // values are 2/1 rather than the 1/1/1/1 an all-public version
    // would give — which an arm ignoring `current_is_public` would
    // satisfy on both metrics at once.
    const METHOD_RETURNING_FUNCTION_POINTER: &str = "class F {\n\
         public:\n\
             int (*getFp(int))(int);\n\
             int (*fp)(int);\n\
         private:\n\
             int (*privGetFp(int))(int);\n\
             int (*privFp)(int);\n\
         };";

    #[test]
    fn cpp_function_pointer_members_are_attributes_not_methods() {
        check_metrics_with_npa::<CppParser>(FUNCTION_POINTER_MEMBERS, "foo.cpp", |metric| {
            // Everything but the four function-pointer fields:
            // `realMethod`, `operator->`, the two conversion
            // operators, `parenMethod`, `operator+`, and
            // `privMethod`. Before #1300 the function-pointer members
            // inflated this pair to 10/8.
            assert_eq!(metric.npm.class_nm_sum(), 8);
            assert_eq!(metric.npm.class_npm_sum(), 7);
            // `fp`, `plainData`, `fps`, `privFp`. Before #1300 only
            // `plainData` was reachable, leaving 1/1 — and gating the
            // predicate *without* widening the counter leaves it
            // there, because the declined field's `field_identifier`
            // is still buried under the two declarator kinds the
            // counter did not recurse through.
            assert_eq!(metric.npa.class_na_sum(), 5);
            assert_eq!(metric.npa.class_npa_sum(), 4);
        });
    }

    #[test]
    fn mozcpp_function_pointer_members_are_attributes_not_methods() {
        check_metrics_with_npa::<MozcppParser>(FUNCTION_POINTER_MEMBERS, "foo.cpp", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 8);
            assert_eq!(metric.npm.class_npm_sum(), 7);
            assert_eq!(metric.npa.class_na_sum(), 5);
            assert_eq!(metric.npa.class_npa_sum(), 4);
        });
    }

    #[test]
    fn cpp_method_returning_a_function_pointer_stays_a_method() {
        check_metrics_with_npa::<CppParser>(
            METHOD_RETURNING_FUNCTION_POINTER,
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[test]
    fn mozcpp_method_returning_a_function_pointer_stays_a_method() {
        check_metrics_with_npa::<MozcppParser>(
            METHOD_RETURNING_FUNCTION_POINTER,
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[test]
    fn cpp_non_method_template_payloads_are_not_counted() {
        check_metrics_with_nom_wmc::<CppParser>(
            NON_METHOD_TEMPLATE_PAYLOADS,
            "foo.cpp",
            |metric| {
                // `real()` (public, in C) plus `Nested::hidden()`
                // (private, in its own class space).
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                // `nom` additionally counts the friend's body, which is
                // a free function the class merely grants access to and
                // not a member — an arm that leaked through
                // `friend_declaration` would push `class_nm_sum` to 3.
                assert_eq!(metric.nom.functions_sum(), 3);
                // `wmc` agrees with `npm` since #1301: `real()` and
                // `Nested::hidden()` at cyclomatic 1 each. Until then
                // this read 3, weighting the friend's body into the
                // class — the divergence #1258 recorded here and #1301
                // removed. `nom` staying at 3 is what makes the two
                // separable: this fixture pins the metrics that count
                // *members* against the one that counts functions.
                assert_eq!(metric.wmc.class_wmc_sum(), 2);
            },
        );
    }

    #[test]
    fn mozcpp_template_method_with_inline_body_counts() {
        check_metrics_with_nom_wmc::<MozcppParser>(
            TEMPLATE_METHOD_WITH_BODY,
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 2);
                assert_eq!(metric.nom.functions_sum(), 2);
                assert_eq!(metric.wmc.class_wmc_sum(), 2);
            },
        );
    }

    #[test]
    fn mozcpp_non_method_template_payloads_are_not_counted() {
        check_metrics_with_nom_wmc::<MozcppParser>(
            NON_METHOD_TEMPLATE_PAYLOADS,
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                assert_eq!(metric.nom.functions_sum(), 3);
                // See the Cpp mirror above: 3 before #1301.
                assert_eq!(metric.wmc.class_wmc_sum(), 2);
            },
        );
    }

    #[test]
    fn javascript_empty_unit_no_methods() {
        check_metrics::<JavascriptParser>("", "empty.js", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0);
            assert_eq!(metric.npm.class_npm_sum(), 0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn javascript_class_methods_count() {
        // `method_definition` direct children of `class_body` cover
        // regular methods, getters/setters, and constructors. JS has
        // no visibility — all members are public. nm = npm = 4.
        check_metrics::<JavascriptParser>(
            "class Foo {\n\
                 constructor() {}\n\
                 bar() {}\n\
                 get baz() { return 1; }\n\
                 set baz(v) {}\n\
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 4);
                assert_eq!(metric.npm.class_npm_sum(), 4);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn javascript_arrow_field_is_method() {
        // `class Foo { x = () => {} }` is a method written as a field
        // initializer. Both arrow functions and `function`
        // expressions in field position count as methods.
        check_metrics::<JavascriptParser>(
            "class Foo { x = () => {}; y = function() {}; z = 1; }",
            "foo.js",
            |metric| {
                // x + y are methods; z is an attribute.
                assert_eq!(metric.npm.class_nm_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 2);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn javascript_free_function_is_not_method() {
        // Top-level functions and arrow functions outside a class
        // body are not methods.
        check_metrics::<JavascriptParser>(
            "function f() {}\nconst g = () => {};\nclass Foo { h() {} }",
            "foo.js",
            |metric| {
                // Only `h` is a method.
                assert_eq!(metric.npm.class_nm_sum(), 1);
                assert_eq!(metric.npm.class_npm_sum(), 1);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn javascript_multiple_classes_aggregate_at_unit() {
        // File-level rollup: Foo has 2 methods, Bar has 1. Unit
        // class_nm_sum = 3.
        check_metrics::<JavascriptParser>(
            "class Foo { a() {} b() {} }\nclass Bar { c() {} }",
            "foo.js",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npm.class_npm_sum(), 3);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn mozjs_class_methods_count() {
        // Mozjs shares JS's class vocabulary.
        check_metrics::<MozjsParser>(
            "class Foo {\n\
                 constructor() {}\n\
                 bar() {}\n\
                 get baz() { return 1; }\n\
                 set baz(v) {}\n\
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 4);
                assert_eq!(metric.npm.class_npm_sum(), 4);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    // Regression for #438: an empty class has zero methods, so the COA
    // accessors divide 0.0 / 0.0. Before the zero-guard this yielded NaN
    // (serialized to JSON `null`). The defined value is 0.0 — a
    // method-less class exposes no public operations. Asserting
    // `!is_nan()` proves the guard fires; the `== 0.0` checks pin the
    // chosen convention. Exercised across the explicit-visibility OO
    // languages (Java, C#, Kotlin, PHP).
    #[test]
    fn empty_class_coa_is_zero_not_nan() {
        let assert_zero = |metric: crate::CodeMetrics| {
            assert_eq!(metric.npm.class_nm_sum(), 0);
            assert!(!metric.npm.class_coa().is_nan());
            assert!(!metric.npm.total_coa().is_nan());
            assert_eq!(metric.npm.class_coa(), 0.0);
            assert_eq!(metric.npm.total_coa(), 0.0);
        };
        check_metrics::<JavaParser>("class Foo {}", "foo.java", assert_zero);
        check_metrics::<CsharpParser>("class Foo {}", "foo.cs", assert_zero);
        check_metrics::<KotlinParser>("class Foo {}", "foo.kt", assert_zero);
        check_metrics::<PhpParser>("<?php class Foo {}", "foo.php", assert_zero);
    }

    // Regression for #438: an empty interface has zero methods; the
    // existing all-public guard explicitly excludes the empty case
    // (`!= 0`), so without the divisor guard `interface_coa` returned
    // 0.0 / 0.0 = NaN. The defined value is 0.0.
    #[test]
    fn empty_interface_coa_is_zero_not_nan() {
        let assert_zero = |metric: crate::CodeMetrics| {
            assert_eq!(metric.npm.interface_nm_sum(), 0);
            assert!(!metric.npm.interface_coa().is_nan());
            assert_eq!(metric.npm.interface_coa(), 0.0);
        };
        check_metrics::<JavaParser>("interface Foo {}", "foo.java", assert_zero);
        check_metrics::<CsharpParser>("interface Foo {}", "foo.cs", assert_zero);
    }

    // Rounds out `npm`'s public surface — the `Display` impl and the
    // per-space `class_npm` / `class_nm` / `interface_*` accessors —
    // mirroring the `Display` tests the sibling metrics carry.
    #[test]
    fn stats_display_and_per_space_accessors() {
        check_func_space::<JavaParser, _>(
            "public interface I {\n    void p();\n}\n\
             public class C {\n    public void m() {}\n    private void n() {}\n}\n",
            "X.java",
            |unit| {
                // Class C: m public, n private → 1 public of 2 methods.
                // Interface I: one method p.
                assert_eq!(unit.metrics.npm.class_npm_sum(), 1);
                assert_eq!(unit.metrics.npm.class_nm_sum(), 2);
                let rendered = unit.metrics.npm.to_string();
                for fragment in [
                    "classes: 1, interfaces: 1",
                    "class_methods: 2",
                    "interface_methods: 1",
                    "total: 2, total_methods: 3",
                ] {
                    assert!(
                        rendered.contains(fragment),
                        "missing {fragment:?} in {rendered}"
                    );
                }
                // Singular accessors populate only on the owning class /
                // interface space (0 on the file-unit root); assert them where
                // they are nonzero so an always-zero or wrong-field accessor
                // would fail.
                let class = child_space(&unit, "C");
                assert_eq!(class.kind, SpaceKind::Class);
                assert_eq!(class.metrics.npm.class_npm(), 1);
                assert_eq!(class.metrics.npm.class_nm(), 2);
                let iface = child_space(&unit, "I");
                assert_eq!(iface.kind, SpaceKind::Interface);
                assert_eq!(iface.metrics.npm.interface_npm(), 1);
                assert_eq!(iface.metrics.npm.interface_nm(), 1);
            },
        );
    }
}
