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
use crate::getter::Getter;
use crate::langs::*;
use crate::macros::{csharp_var_decl_kinds, csharp_var_declarator_kinds, implement_metric_trait};
use crate::metrics::opens_member_scope;
use crate::node::Node;
use crate::*;

/// The `Npa` metric.
///
/// This metric counts the number of public attributes
/// of classes/interfaces.
///
/// Emitted on container spaces and on the file unit that rolls them up,
/// never on a function space — the rule `wmc` also follows, spelled once
/// as `SpaceKind::is_member_scope`. Each language decides *when* to set
/// the flag: the ten that route through `metrics::opens_member_scope`
/// obey the rule for every node, while Python, Rust, C++, Mozcpp, Go,
/// Objective-C and Elixir gate on their own node kinds and may still
/// enable a space the shared predicate would not (Go's file root, for
/// one).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Stats {
    class_npa: usize,
    interface_npa: usize,
    class_na: usize,
    interface_na: usize,
    class_npa_sum: usize,
    interface_npa_sum: usize,
    class_na_sum: usize,
    interface_na_sum: usize,
    is_class_space: bool,
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "classes: {}, interfaces: {}, class_attributes: {}, interface_attributes: {}, class_cda: {}, interface_cda: {}, total: {}, total_attributes: {}, cda: {}",
            self.class_npa_sum(),
            self.interface_npa_sum(),
            self.class_na_sum(),
            self.interface_na_sum(),
            self.class_cda(),
            self.interface_cda(),
            self.total_npa(),
            self.total_na(),
            self.total_cda()
        )
    }
}

impl Stats {
    /// Merges a second `Npa` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.class_npa_sum += other.class_npa_sum;
        self.interface_npa_sum += other.interface_npa_sum;
        self.class_na_sum += other.class_na_sum;
        self.interface_na_sum += other.interface_na_sum;
    }

    /// Returns the number of class public attributes in a space.
    #[inline]
    #[must_use]
    pub fn class_npa(&self) -> u64 {
        self.class_npa as u64
    }

    /// Returns the number of interface public attributes in a space.
    #[inline]
    #[must_use]
    pub fn interface_npa(&self) -> u64 {
        self.interface_npa as u64
    }

    /// Returns the number of class attributes in a space.
    #[inline]
    #[must_use]
    pub fn class_na(&self) -> u64 {
        self.class_na as u64
    }

    /// Returns the number of interface attributes in a space.
    #[inline]
    #[must_use]
    pub fn interface_na(&self) -> u64 {
        self.interface_na as u64
    }

    /// Returns the number of class public attributes sum in a space.
    #[inline]
    #[must_use]
    pub fn class_npa_sum(&self) -> u64 {
        self.class_npa_sum as u64
    }

    /// Returns the number of interface public attributes sum in a space.
    #[inline]
    #[must_use]
    pub fn interface_npa_sum(&self) -> u64 {
        self.interface_npa_sum as u64
    }

    /// Returns the number of class attributes sum in a space.
    #[inline]
    #[must_use]
    pub fn class_na_sum(&self) -> u64 {
        self.class_na_sum as u64
    }

    /// Returns the number of interface attributes sum in a space.
    #[inline]
    #[must_use]
    pub fn interface_na_sum(&self) -> u64 {
        self.interface_na_sum as u64
    }

    /// Returns the class `Cda` metric value
    ///
    /// The `Class Data Accessibility` metric value for a class
    /// is computed by dividing the `Npa` value of the class
    /// by the total number of attributes defined in the class.
    ///
    /// This metric is an adaptation of the `Classified Class Data Accessibility` (`CCDA`)
    /// security metric for not classified attributes.
    /// Paper: <https://ieeexplore.ieee.org/abstract/document/5381538>
    #[inline]
    #[must_use]
    pub fn class_cda(&self) -> f64 {
        accessibility_ratio(self.class_npa_sum() as f64, self.class_na_sum() as f64)
    }

    /// Returns the interface `Cda` metric value
    ///
    /// The `Class Data Accessibility` metric value for an interface
    /// is computed by dividing the `Npa` value of the interface
    /// by the total number of attributes defined in the interface.
    ///
    /// This metric is an adaptation of the `Classified Class Data Accessibility` (`CCDA`)
    /// security metric for not classified attributes.
    /// Paper: <https://ieeexplore.ieee.org/abstract/document/5381538>
    #[inline]
    #[must_use]
    pub fn interface_cda(&self) -> f64 {
        // Java interface fields are implicitly public, so when every counted
        // attribute is public (`npa == na != 0`) the ratio is exactly 1.0 and
        // the division is skipped. The empty case falls through to
        // `accessibility_ratio`, which is guarded to return a finite 0.0 (not
        // `NaN`) for a zero denominator (#438).
        if self.interface_npa_sum == self.interface_na_sum && self.interface_npa_sum != 0 {
            1.0
        } else {
            accessibility_ratio(
                self.interface_npa_sum() as f64,
                self.interface_na_sum() as f64,
            )
        }
    }

    /// Returns the total `Cda` metric value
    ///
    /// The total `Class Data Accessibility` metric value
    /// is computed by dividing the total `Npa` value
    /// by the total number of attributes.
    ///
    /// This metric is an adaptation of the `Classified Class Data Accessibility` (`CCDA`)
    /// security metric for not classified attributes.
    /// Paper: <https://ieeexplore.ieee.org/abstract/document/5381538>
    #[inline]
    #[must_use]
    pub fn total_cda(&self) -> f64 {
        accessibility_ratio(self.total_npa() as f64, self.total_na() as f64)
    }

    /// Returns the total number of public attributes in a space.
    #[inline]
    #[must_use]
    pub fn total_npa(&self) -> u64 {
        self.class_npa_sum() + self.interface_npa_sum()
    }

    /// Returns the total number of attributes in a space.
    #[inline]
    #[must_use]
    pub fn total_na(&self) -> u64 {
        self.class_na_sum() + self.interface_na_sum()
    }

    // Accumulates the number of class and interface
    // public and not public attributes into the sums
    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.class_npa_sum += self.class_npa;
        self.interface_npa_sum += self.interface_npa;
        self.class_na_sum += self.class_na;
        self.interface_na_sum += self.interface_na;
    }

    // Checks if the `Npa` metric is disabled
    #[inline]
    pub(crate) fn is_disabled(&self) -> bool {
        !self.is_class_space
    }

    /// Enables `Npa` on the space `node` opens, when that space is a
    /// member scope — a container, or the file unit that rolls its
    /// containers up (#1197).
    ///
    /// Idempotent by design: `compute` runs once per node, so the first
    /// qualifying node to reach a given space wins and every later call is
    /// a no-op. The languages that gate on a bespoke node-kind set —
    /// Python, Rust, C++, Mozcpp, Go, Objective-C, Elixir — set the flag
    /// themselves and do not route through here.
    #[inline]
    fn enable_for_member_scope<'a, L: Checker + Getter>(
        &mut self,
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) {
        if self.is_disabled() && opens_member_scope::<L>(node, code, ancestors) {
            self.is_class_space = true;
        }
    }
}

// Computes an accessibility ratio (public members / total members),
// guarding the empty case. A class/interface with no attributes or
// methods has no exposed surface, so the defined value is `0.0` rather
// than `0.0 / 0.0 = NaN` (which serializes to JSON `null`). Shared by
// `Npa`'s CDA accessors and `Npm`'s COA accessors (issue #438).
#[inline]
pub(crate) fn accessibility_ratio(public: f64, total: f64) -> f64 {
    if total == 0.0 { 0.0 } else { public / total }
}

#[doc(hidden)]
/// Per-language counting of public attributes.
pub(crate) trait Npa
where
    Self: Checker,
{
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

// `impl_npa_java_like!` was introduced for Java and Groovy, whose
// grammar tokens for class/interface bodies matched closely enough that
// `Npa::compute` differed only by the language enum (issue #280). Groovy
// has since moved to a hand-written impl — the dekobon grammar flattens
// modifiers, see `npa/groovy.rs` — so this expands against Java alone.
// It is kept in macro form because the next Java-shaped grammar can
// reuse it.
//
// `ClassBody` covers classes and records (records reuse `class_body`
// for their explicit declaration body). Record components in
// `formal_parameters` are implicit public final fields, but only
// explicit body members are counted here for parity with C#'s record
// handling (lesson 11). `EnumBodyDeclarations` is the optional
// declarations block inside `EnumBody`, following the enum constants.
// Annotation type bodies hold `ConstantDeclaration`s with the same
// implicit `public static final` rule as interfaces
// (https://docs.oracle.com/javase/specs/jls/se7/html/jls-9.html).
//
// Groovy note: `def field` at class scope is parsed as a
// `FieldDeclaration` with `Def` in the modifiers list (no `Public`),
// so it's correctly excluded from `class_npa` unless explicitly
// annotated `public` — consistent with Groovy's access semantics
// (default class members are package-private under `@CompileStatic`,
// public otherwise; we conservatively follow Java).
macro_rules! impl_npa_java_like {
    ($code:ty, $lang:ident) => {
        impl Npa for $code {
            fn compute<'a>(
                node: &Node<'a>,
                code: &'a [u8],
                ancestors: Ancestors<'a, '_>,
                stats: &mut Stats,
            ) {
                use $lang::*;

                stats.enable_for_member_scope::<Self>(node, code, ancestors);

                match node.kind_id().into() {
                    ClassBody | EnumBodyDeclarations => {
                        for declaration in node
                            .children()
                            .filter(|n| matches!(n.kind_id().into(), FieldDeclaration))
                        {
                            let attributes = declaration
                                .children()
                                .filter(|n| matches!(n.kind_id().into(), VariableDeclarator))
                                .count();
                            stats.class_na += attributes;
                            // The first child node contains the list of
                            // attribute modifiers. Source:
                            // https://docs.oracle.com/javase/tutorial/reflect/member/fieldModifiers.html
                            if declaration.child(0).is_some_and(|modifiers| {
                                matches!(modifiers.kind_id().into(), Modifiers)
                                    && modifiers.first_child(|id| id == Public).is_some()
                            }) {
                                stats.class_npa += attributes;
                            }
                        }
                    }
                    InterfaceBody | AnnotationTypeBody => {
                        stats.interface_na += node
                            .children()
                            .filter(|n| matches!(n.kind_id().into(), ConstantDeclaration))
                            .flat_map(|n| n.children())
                            .filter(|n| matches!(n.kind_id().into(), VariableDeclarator))
                            .count();
                        stats.interface_npa = stats.interface_na;
                    }
                    _ => {}
                }
            }
        }
    };
}

mod shared;
pub(crate) use shared::*;

// TypeScript / TSX share the same OOP node shape: `class_declaration`
// and `abstract_class_declaration` both contain a `class_body`;
// `interface_declaration` contains an `interface_body`. The
// `ts_npa_compute!` macro expands the same compute logic for each enum,
// so TS and TSX cannot drift.
//
// Visibility rule: a `public_field_definition` or `method_definition`
// is considered public unless it carries an explicit
// `accessibility_modifier` child whose only child is `private` or
// `protected`. Default (no modifier) is public, matching TypeScript's
// own semantics.
//
// Parameter properties (`constructor(private x: number)`) are class
// attributes: each `required_parameter` carrying an
// `accessibility_modifier` *or* a bare `readonly` keyword adds one to
// the enclosing class's `na` (and to `npa` when the modifier is
// `public` or absent). `readonly` is a distinct keyword child, not an
// `accessibility_modifier`, so a `readonly`-only parameter property
// (`constructor(readonly b: number)`) is public and must be detected
// separately — matching `readonly` class fields, which already count
// (see `typescript_readonly_field`). `private readonly` carries both
// children but `first_child` matches at most one and we increment once,
// so it is never double-counted. The
// grammar allows accessibility modifiers on parameters of any
// `method_definition`, not only `constructor` — TypeScript itself
// rejects that at type-check time, but accepting any method here
// avoids fragile name-matching against the `constructor` identifier
// (the grammar does not expose a dedicated constructor token).
//
// Interface decision: `property_signature` children of
// `interface_body` count toward `interface_npa` / `interface_na`.
// All interface members are implicitly public (TypeScript spec).
// `index_signature` and `method_signature` are NOT attributes — they
// belong to `npm`.
macro_rules! ts_npa_compute {
    ($lang:ident) => {
        fn compute<'a>(
            node: &Node<'a>,
            code: &'a [u8],
            ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use $lang::*;

            stats.enable_for_member_scope::<Self>(node, code, ancestors);

            match node.kind_id().into() {
                ClassBody => {
                    for member in node.children() {
                        match member.kind_id().into() {
                            // Plain field declaration (`x: T = expr;`, `private x: T;`,
                            // `static x: T = expr;`). Each is one attribute.
                            // Skip fields whose initializer is an arrow function or
                            // function expression — those are methods written as
                            // field initializers and are counted by `npm` instead.
                            PublicFieldDefinition
                                if member
                                    .first_child(|id| {
                                        id == $lang::ArrowFunction
                                            || id == $lang::FunctionExpression
                                    })
                                    .is_none() =>
                            {
                                stats.class_na += 1;
                                if ts_member_is_public!($lang, member) {
                                    stats.class_npa += 1;
                                }
                            }
                            // Parameter properties on any `method_definition`. In
                            // practice these only appear on the constructor.
                            // Scan formal_parameters at the class-body level so
                            // the attribute lands on the class space, not the
                            // method's own function space.
                            MethodDefinition => {
                                let Some(params) =
                                    member.first_child(|id| id == $lang::FormalParameters)
                                else {
                                    continue;
                                };
                                for param in params.children().filter(|c| {
                                    matches!(
                                        c.kind_id().into(),
                                        RequiredParameter | RequiredParameter2
                                    )
                                }) {
                                    if param
                                        .first_child(|id| {
                                            id == $lang::AccessibilityModifier
                                                || id == $lang::Readonly
                                        })
                                        .is_some()
                                    {
                                        stats.class_na += 1;
                                        if ts_member_is_public!($lang, param) {
                                            stats.class_npa += 1;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                InterfaceBody => {
                    let count = node
                        .children()
                        .filter(|c| matches!(c.kind_id().into(), PropertySignature))
                        .count();
                    stats.interface_na += count;
                    stats.interface_npa = stats.interface_na;
                }
                _ => {}
            }
        }
    };
}

// Class members are public unless they declare an explicit
// `accessibility_modifier` whose only child is `private` or `protected`.
// Missing modifier means public, matching TypeScript's spec. The helper
// is a macro rather than a generic function so both TS and TSX expand
// the same code against their own enum without a marker trait.
macro_rules! ts_member_is_public {
    ($lang:ident, $member:expr) => {{
        match $member.first_child(|id| id == $lang::AccessibilityModifier) {
            None => true,
            Some(m) => m
                .first_child(|id| id == $lang::Private || id == $lang::Protected)
                .is_none(),
        }
    }};
}
pub(crate) use ts_member_is_public;

// JavaScript / Mozjs share the same class vocabulary. JS has no
// `accessibility_modifier` — every class member is public, so each
// class field maps 1:1 to both `na` and `npa`.
//
// We count ES2022 class fields (`class Foo { x = 1; }`):
// `field_definition` direct children of `class_body`. Fields whose
// initializer is an `arrow_function` or `function_expression` are
// methods written as field initializers and belong to `Npm`, not
// `Npa`.
//
// Prototype-based attribute assignments (`Foo.prototype.x = 5;`)
// would also be legitimate JS attributes per Fenton's metric
// taxonomy, but detecting them requires matching the `prototype`
// property-identifier text. They are not yet detected by this impl,
// so modern ES2015+ class syntax — the dominant style — is
// unaffected, while legacy prototype-only files under-report. The
// `code` source bytes are already available (bound as `_code`
// below), so implementing prototype detection requires no trait
// signature change (see `Abc::compute` for the existing pattern).

macro_rules! js_npa_compute {
    ($lang:ident) => {
        fn compute<'a>(
            node: &Node<'a>,
            code: &'a [u8],
            ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use $lang::*;

            stats.enable_for_member_scope::<Self>(node, code, ancestors);

            if !matches!(node.kind_id().into(), ClassBody) {
                return;
            }

            for member in node.children() {
                if matches!(member.kind_id().into(), FieldDefinition)
                    && member
                        .first_child(|id| {
                            id == $lang::ArrowFunction || id == $lang::FunctionExpression
                        })
                        .is_none()
                {
                    stats.class_na += 1;
                    stats.class_npa += 1;
                }
            }
        }
    };
}

// Per-language `Npa` impls live in sibling modules. The `mod`
// declarations sit after the local `macro_rules!` so textual macro
// scoping reaches the child files (mirrors `metrics::npm` and
// `metrics::cyclomatic`).
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

// Default no-op `Npa` impls. Audited in #188.
//
// Real defaults (no first-class class / OO grammar construct, so the
// metric is genuinely 0):
//   - PreprocCode, CcommentCode: no executable code.
//   - BashCode: shell has no class concept.
//   - PerlCode, LuaCode, TclCode: prototype / table / package-based
//     OO is convention-only, not a grammar construct the audit treats
//     as class-shaped.
// Elixir Npa is implemented below (#275).
implement_metric_trait!(
    Npa,
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

    check_metrics_only_shim!(check_metrics, Npa);
    check_func_space_only_shim!(check_func_space, Npa);

    #[test]
    fn java_single_attributes() {
        check_metrics::<JavaParser>(
            "class X {
                public byte a;      // +1
                public short b;     // +1
                public int c;       // +1
                public long d;      // +1
                public float e;     // +1
                public double f;    // +1
                public boolean g;   // +1
                public char h;      // +1
                byte i;
                short j;
                int k;
                long l;
                float m;
                double n;
                boolean o;
                char p;
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 8,
                  "interface_npa_sum": 0,
                  "class_attributes": 16,
                  "interface_attributes": 0,
                  "class_cda": 0.5,
                  "interface_cda": 0.0,
                  "total": 8,
                  "total_attributes": 16,
                  "cda": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_multiple_attributes() {
        check_metrics::<JavaParser>(
            "class X {
                public byte a1;                 // +1
                public short b1, b2;            // +2
                public int c1, c2, c3;          // +3
                public long d1, d2, d3, d4;     // +4
                public float e1, e2, e3, e4;    // +4
                public double f1, f2, f3;       // +3
                public boolean g1, g2;          // +2
                public char h1;                 // +1
                byte i1, i2, i3, i4;
                short j1, j2, j3;
                int k1, k2;
                long l1;
                float m1;
                double n1, n2;
                boolean o1, o2, o3;
                char p1, p2, p3, p4;
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 20,
                  "interface_npa_sum": 0,
                  "class_attributes": 40,
                  "interface_attributes": 0,
                  "class_cda": 0.5,
                  "interface_cda": 0.0,
                  "total": 20,
                  "total_attributes": 40,
                  "cda": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_initialized_attributes() {
        check_metrics::<JavaParser>(
            "class X {
                public byte a1 = 1;                             // +1
                public short b1 = 2, b2;                        // +2
                public int c1, c2 = 3, c3;                      // +3
                public long d1 = 4, d2, d3, d4 = 5;             // +4
                public float e1, e2 = 6.0f, e3 = 7.0f, e4;      // +4
                public double f1 = 8.0, f2 = 9.0, f3 = 10.0;    // +3
                public boolean g1 = true, g2;                   // +2
                public char h1 = 'a';                           // +1
                byte i1 = 1, i2 = 2, i3 = 3, i4 = 4;
                short j1 = 5, j2, j3 = 6;
                int k1, k2 = 7;
                long l1 = 8;
                float m1 = 9.0f;
                double n1, n2 = 10.0;
                boolean o1, o2 = false, o3;
                char p1 = 'a', p2 = 'b', p3 = 'c', p4 = 'd';
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 20,
                  "interface_npa_sum": 0,
                  "class_attributes": 40,
                  "interface_attributes": 0,
                  "class_cda": 0.5,
                  "interface_cda": 0.0,
                  "total": 20,
                  "total_attributes": 40,
                  "cda": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_array_attributes() {
        check_metrics::<JavaParser>(
            "class X {
                public byte[] a1, a2, a3, a4;                       // +4
                public short b1[], b2[], b3[];                      // +3
                public int[] c1 = { 1 }, c2;                        // +2
                public long d1[] = { 1 };                           // +1
                public float[] e1 = { 1.0f, 2.0f, 3.0f };           // +1
                public double f1[] = { 1.0, 2.0, 3.0 }, f2[];       // +2
                public boolean[] g1 = new boolean[5], g2, g3;       // +3
                public char[] h1 = new char[5], h2[], h3[], h4[];   // +4
                byte[] i1;
                short j1[], j2[];
                int[] k1, k2, k3 = { 1 };
                long l1[], l2[] = { 1 }, l3[] = { 2 }, l4[];
                float[] m1, m2, m3, m4 = { 1.0f, 2.0f, 3.0f };
                double n1[], n2[] = { 1.0, 2.0, 3.0 }, n3[];
                boolean[] o1, o2 = new boolean[5];
                char[] p1 = new char[5];
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 20,
                  "interface_npa_sum": 0,
                  "class_attributes": 40,
                  "interface_attributes": 0,
                  "class_cda": 0.5,
                  "interface_cda": 0.0,
                  "total": 20,
                  "total_attributes": 40,
                  "cda": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_object_attributes() {
        check_metrics::<JavaParser>(
            "class X {
                public Integer[] a1 = { 1 };                                    // +1
                public Integer b1, b2;                                          // +2
                public String[] c1 = { \"Hello\" }, c2, c3 = { \"World!\" };    // +3
                public String d1[][] = { { \"Hello\" }, { \"World!\" } };       // +1
                public Y[] e1, e2[];                                            // +2
                public Y f1[], f2[][], f3[][][];                                // +3
                Integer[] g1 = { new Integer(1) };
                Integer h1 = new Integer(1), h2 = new Integer(2);
                String[] i1, i2 = { \"Hello World!\" }, i3;
                String j1 = \"Hello World!\";
                Y[] k1[], k2;
                Y l1[][], l2[], l3 = new Y();
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 12,
                  "interface_npa_sum": 0,
                  "class_attributes": 24,
                  "interface_attributes": 0,
                  "class_cda": 0.5,
                  "interface_cda": 0.0,
                  "total": 12,
                  "total_attributes": 24,
                  "cda": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn groovy_no_attributes() {
        check_metrics::<GroovyParser>("class A { void foo() {} }", "foo.groovy", |metric| {
            assert_eq!(metric.npa.total_na(), 0);
            assert_eq!(metric.npa.total_npa(), 0);
        });
    }

    #[test]
    fn groovy_public_attributes() {
        check_metrics::<GroovyParser>(
            "class A {
                public int x
                public String name
                private int hidden
            }",
            "foo.groovy",
            |metric| {
                // 3 total attributes, 2 public
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[test]
    fn groovy_def_attributes_not_public() {
        // `def field` at class scope is a FieldDeclaration whose
        // modifier list contains `Def`, not `Public`. Mirror Java's
        // semantics: only explicit `public` is counted.
        check_metrics::<GroovyParser>(
            "class A {
                def field1
                def field2
            }",
            "foo.groovy",
            |metric| {
                // Both `def` fields parse as FieldDeclarations.
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 0);
            },
        );
    }

    #[test]
    fn groovy_interface_attributes() {
        // Structural `assert_child_space_kind` guards against an
        // `InterfaceDeclaration` revert in `GroovyCode::is_func_space`
        // — see #311.
        check_func_space::<GroovyParser, _>(
            "interface I {
                public static final int A = 1
                public static final int B = 2
            }",
            "foo.groovy",
            |func_space| {
                let metric = &func_space.metrics;
                // Interface fields are implicitly public+static+final.
                assert_eq!(metric.npa.interface_na_sum(), 2);
                assert_eq!(metric.npa.interface_npa_sum(), 2);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn groovy_no_attributes_in_unit_scope() {
        check_metrics::<GroovyParser>("int x = 1", "foo.groovy", |metric| {
            assert_eq!(metric.npa.total_na(), 0);
        });
    }

    #[test]
    fn groovy_multiple_classes() {
        check_metrics::<GroovyParser>(
            "class A { public int a }
            class B { public int b }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[test]
    fn groovy_initialized_attributes() {
        // Mirror of `java_initialized_attributes`: each
        // `variable_declarator` inside a `field_declaration` counts
        // as one attribute, with or without an initializer; `public`
        // modifier promotes them all to NPA.
        check_metrics::<GroovyParser>(
            "class X {
                public int a1 = 1, a2
                public int b1 = 2
                int c1, c2 = 3
            }",
            "foo.groovy",
            |metric| {
                // 5 attributes total, 3 public.
                assert_eq!(metric.npa.class_na_sum(), 5);
                assert_eq!(metric.npa.class_npa_sum(), 3);
            },
        );
    }

    #[test]
    fn groovy_object_attributes() {
        // Object-typed attributes (boxed primitives, user types,
        // String, arrays). Each declarator is one attribute.
        check_metrics::<GroovyParser>(
            "class X {
                public Integer a1
                public String b1 = 'hello'
                public Y[] c1
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
            },
        );
    }

    #[test]
    fn groovy_attribute_modifiers() {
        // Multiple modifier orderings (public/static/final/transient/
        // volatile etc.) must all be detected — what matters for NPA
        // is whether the `Modifiers` block contains `Public`.
        check_metrics::<GroovyParser>(
            "class X {
                public static int a
                static public int b
                public final int c = 1
                final public int d = 2
                private static int e
                int f
            }",
            "foo.groovy",
            |metric| {
                // 6 attributes total, 4 public (regardless of order).
                assert_eq!(metric.npa.class_na_sum(), 6);
                assert_eq!(metric.npa.class_npa_sum(), 4);
            },
        );
    }

    #[test]
    #[ignore = "dekobon Groovy grammar v1 does not yet support inner classes inside class bodies (https://github.com/dekobon/tree-sitter-groovy SPECIFICATION.md §4 — 'Field declarations, static initialisers, and inner classes land later')"]
    fn groovy_nested_inner_classes() {
        // Each nested `class` declaration is its own class space
        // with its own NPA. Mirrors `java_nested_inner_classes`.
        check_metrics::<GroovyParser>(
            "class X {
                public int a
                class Y {
                    public boolean b
                    class Z {
                        public char c
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // 3 classes, 3 public attributes.
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
            },
        );
    }

    #[test]
    fn groovy_array_attributes() {
        // Array-typed attributes. Mirrors `java_array_attributes`.
        check_metrics::<GroovyParser>(
            "class X {
                public int[] a
                public String[] b
                int[] c
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[test]
    fn groovy_anonymous_inner_class() {
        // Object-creation expression containing a `class_body` —
        // anonymous inner class. Its attributes are counted in a
        // separate class space.
        check_metrics::<GroovyParser>(
            "class X {
                public Runnable r = new Runnable() {
                    public int x
                    void run() {}
                }
            }",
            "foo.groovy",
            |metric| {
                // outer X has 1 public attr `r`; inner anonymous
                // has 1 public attr `x` => total 2.
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    // Regression for issue #280: Groovy mirrors Java's enum / record /
    // annotation handling. Record support in the dekobon Groovy grammar
    // lags behind groovyc, but the grammar exposes `record_declaration`
    // and the `Npa` body walker treats it identically.
    #[test]
    fn groovy_enum_counts_explicit_public_fields() {
        check_metrics::<GroovyParser>(
            "enum Status {
                ACTIVE, INACTIVE;
                public int code;
                private int hidden;
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_annotation_type_counts_constants_as_implicit_public() {
        // The dekobon Groovy grammar parses `@interface` like Java
        // (modifier required, statements terminated with `;`). Mirror of
        // `java_annotation_type_counts_constants_as_implicit_public`
        // — the body-walker count is identical whether or not
        // Groovy's `AnnotationTypeDeclaration` is wired into
        // `is_func_space`, so the structural `check_func_space`
        // assertion is what catches a revert.
        check_func_space::<GroovyParser, _>(
            "public @interface Marker {
                int VERSION = 1;
                String NAME = \"x\";
            }",
            "foo.groovy",
            |func_space| {
                assert_eq!(func_space.metrics.npa.interface_na_sum(), 2);
                assert_eq!(func_space.metrics.npa.interface_npa_sum(), 2);
                assert_child_space_kind(&func_space, "Marker", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn java_generic_attributes() {
        check_metrics::<JavaParser>(
            "class X<T, S extends T> {
                public T a1;                            // +1
                public Entry<T, S> b1, b2[];            // +2
                public ArrayList<T> c1, c2, c3;         // +3
                public HashMap<Long, Double> d1, d2;    // +2
                public TreeSet<String> e1;              // +1
                S f1;
                Entry<S, T> g1[], g2;
                ArrayList<S> h1, h2, h3;
                HashMap<Long, Float> i1, i2;
                TreeSet<Entry<S, T>> j1;
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 9,
                  "interface_npa_sum": 0,
                  "class_attributes": 18,
                  "interface_attributes": 0,
                  "class_cda": 0.5,
                  "interface_cda": 0.0,
                  "total": 9,
                  "total_attributes": 18,
                  "cda": 0.5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_attribute_modifiers() {
        check_metrics::<JavaParser>(
            "class X {
                public transient volatile static int a;     // +1
                transient public volatile static int b;     // +1
                transient volatile public static int c;     // +1
                transient volatile static public int d;     // +1
                public transient static final int e = 1;    // +1
                transient public static final int f = 2;    // +1
                transient static public final int g = 3;    // +1
                transient static final public int h = 4;    // +1
                protected transient volatile static int i;
                transient volatile static protected int j;
                private transient volatile static int k;
                transient volatile static private int l;
                transient volatile static int m;
                transient static final int n = 5;
                static public final int o = 6;              // +1
                final public int p = 7;                     // +1
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 10,
                  "interface_npa_sum": 0,
                  "class_attributes": 16,
                  "interface_attributes": 0,
                  "class_cda": 0.625,
                  "interface_cda": 0.0,
                  "total": 10,
                  "total_attributes": 16,
                  "cda": 0.625
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
                public int a;       // +1
                public boolean b;   // +1
                private char c;
            }
            class Y {
                private double d;
                private long e;
                public float f;      // +1
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 3,
                  "interface_npa_sum": 0,
                  "class_attributes": 6,
                  "interface_attributes": 0,
                  "class_cda": 0.5,
                  "interface_cda": 0.0,
                  "total": 3,
                  "total_attributes": 6,
                  "cda": 0.5
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
                public int a;           // +1
                class Y {
                    public boolean b;   // +1
                    class Z {
                        public char c;  // +1
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 3,
                  "interface_npa_sum": 0,
                  "class_attributes": 3,
                  "interface_attributes": 0,
                  "class_cda": 1.0,
                  "interface_cda": 0.0,
                  "total": 3,
                  "total_attributes": 3,
                  "cda": 1.0
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
                public int a;                   // +1
                void x() {
                    class Y {
                        public boolean b;       // +1
                        void y() {
                            class Z {
                                public char c;  // +1
                                void z() {}
                            }
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 3,
                  "interface_npa_sum": 0,
                  "class_attributes": 3,
                  "interface_attributes": 0,
                  "class_cda": 1.0,
                  "interface_cda": 0.0,
                  "total": 3,
                  "total_attributes": 3,
                  "cda": 1.0
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
                public int a;               // +1
            }
            abstract class Y {
                boolean b;
            }
            class Z {
                public char c;              // +1
                public void z(){
                    X x1 = new X() {
                        public double d;    // +1
                    };
                    Y y1 = new Y() {
                        long e;
                    };
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 3,
                  "interface_npa_sum": 0,
                  "class_attributes": 5,
                  "interface_attributes": 0,
                  "class_cda": 0.6,
                  "interface_cda": 0.0,
                  "total": 3,
                  "total_attributes": 5,
                  "cda": 0.6
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
                public int a = 0;           // +1
                static boolean b = false;   // +1
                final char c = ' ';         // +1
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npa,
                    @r#"
                {
                  "class_npa_sum": 0,
                  "interface_npa_sum": 3,
                  "class_attributes": 0,
                  "interface_attributes": 3,
                  "class_cda": 0.0,
                  "interface_cda": 1.0,
                  "total": 3,
                  "total_attributes": 3,
                  "cda": 1.0
                }
                "#
                );
            },
        );
    }

    // Regression for issue #280: Java `EnumDeclaration` must be
    // classified as a class space so `Npa` walks its body and counts
    // explicit public fields declared after the enum constants.
    #[test]
    fn java_enum_counts_explicit_public_fields() {
        check_metrics::<JavaParser>(
            "enum Status {
                ACTIVE, INACTIVE;
                public static final int FLAG = 1;   // implicit static final, still public
                public int code;                    // +1 explicit public
                private int hidden;                 // not public
            }",
            "foo.java",
            |metric| {
                // 1 class space (the enum), 3 total fields, 2 explicit public.
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    // Regression for issue #280: Java `RecordDeclaration` reuses
    // `ClassBody` for its explicit body, so explicit fields declared
    // inside it count. Record components in the parameter list are
    // implicit public final fields at the bytecode level but are NOT
    // counted here, matching the C# precedent (only explicit body
    // members count).
    #[test]
    fn java_record_counts_explicit_body_fields() {
        check_metrics::<JavaParser>(
            "record Point(int x, int y) {
                public static int origin = 0;       // explicit body, public
                private int cached;                 // explicit body, private
            }",
            "foo.java",
            |metric| {
                // Only explicit body fields are counted; the `x` / `y`
                // record components are not.
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[test]
    fn java_annotation_type_counts_constants_as_implicit_public() {
        // Asserting only `interface_na_sum` / `interface_npa_sum`
        // would pass vacuously if `AnnotationTypeDeclaration` were
        // dropped from `JavaCode::is_func_space`: the body walker
        // counts annotation-type constants regardless of the
        // surrounding FuncSpace kind, so the file-level Unit would
        // still report 2.0 for both. The `check_func_space`
        // assertion catches that revert by requiring the annotation
        // type to actually open an `Interface` FuncSpace.
        check_func_space::<JavaParser, _>(
            "@interface Marker {
                int VERSION = 1;        // implicit public static final
                String NAME = \"x\";    // implicit public static final
            }",
            "foo.java",
            |func_space| {
                assert_eq!(func_space.metrics.npa.interface_na_sum(), 2);
                assert_eq!(func_space.metrics.npa.interface_npa_sum(), 2);
                assert_child_space_kind(&func_space, "Marker", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn php_no_class_attributes() {
        check_metrics::<PhpParser>(
            "<?php class A { public function f(): void {} }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn csharp_single_attributes() {
        check_metrics::<CsharpParser>(
            "class X {
                public byte a;
                public short b;
                public int c;
                public long d;
                public float e;
                public double f;
                public bool g;
                public char h;
                byte i;
                short j;
                int k;
                long l;
                float m;
                double n;
                bool o;
                char p;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 8);
                assert_eq!(metric.npa.class_na_sum(), 16);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_multiple_attributes() {
        check_metrics::<CsharpParser>(
            "class X {
                public byte a1;
                public short b1, b2;
                public int c1, c2, c3;
                public long d1, d2, d3, d4;
                public bool g1, g2;
                byte i1, i2, i3, i4;
                int k1, k2;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 12);
                assert_eq!(metric.npa.class_na_sum(), 18);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_initialized_attributes() {
        check_metrics::<CsharpParser>(
            "class X {
                public int a = 1;
                public bool b = true;
                public string c = \"hello\";
                public double d = 3.14;
                int e = 0;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 4);
                assert_eq!(metric.npa.class_na_sum(), 5);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_array_attributes() {
        check_metrics::<CsharpParser>(
            "class X {
                public int[] a;
                public string[] b = new string[5];
                int[] c;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_object_attributes() {
        check_metrics::<CsharpParser>(
            "class Point { public int X, Y; }
             class Shape {
                public Point origin;
                public Point endpoint = new Point();
                Point hidden;
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 4);
                assert_eq!(metric.npa.class_na_sum(), 5);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_generic_attributes() {
        check_metrics::<CsharpParser>(
            "class X {
                public System.Collections.Generic.List<int> a;
                public System.Collections.Generic.Dictionary<string, int> b;
                System.Collections.Generic.List<string> c;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_attribute_modifiers() {
        check_metrics::<CsharpParser>(
            "class X {
                public int a;
                private int b;
                protected int c;
                internal int d;
                public static int e;
                public readonly int f;
                public const int g = 1;
            }",
            "foo.cs",
            |metric| {
                // Modifiers test: 4 of 7 fields are explicitly `public`. The
                // visibility-filter split is the spec.
                assert_eq!(metric.npa.class_npa_sum(), 4);
                assert_eq!(metric.npa.class_na_sum(), 7);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_classes() {
        check_metrics::<CsharpParser>(
            "class A {
                public int a;
                public int b;
                int c;
            }
            class B {
                public string s;
                int n;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 3);
                assert_eq!(metric.npa.class_na_sum(), 5);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_nested_inner_classes() {
        check_metrics::<CsharpParser>(
            "class Outer {
                public int a;
                int b;
                public class Inner {
                    public string s;
                    int n;
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 4);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_struct_attributes() {
        // C#-only: structs declare fields like classes; visibility rule
        // applies the same way (default is private).
        check_metrics::<CsharpParser>(
            "struct Point {
                public int X;
                public int Y;
                int Hidden;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_record_attributes() {
        // C#-only: records can declare body fields just like classes.
        // Positional record properties are not modelled (EC9).
        check_metrics::<CsharpParser>(
            "record Person {
                public string Name;
                int Age;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_interface() {
        // EC14 — interface members default to public; all fields count.
        // Structural `assert_child_space_kind` guards against an
        // `InterfaceDeclaration` revert in `CsharpCode::is_func_space`
        // — see #311.
        check_func_space::<CsharpParser, _>(
            "interface I {
                static int A = 1;
                static string B = \"hello\";
            }",
            "foo.cs",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npa.class_na_sum(), 0);
                assert_eq!(metric.npa.interface_na_sum(), 2);
                // No explicit modifier means default-public: both fields
                // count as public attributes (#780 regression guard).
                assert_eq!(metric.npa.interface_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn csharp_interface_explicit_modifiers() {
        // #780 — C# 8+ permits explicit `private`/`protected` on interface
        // members. Default-public members count toward npa; an explicit
        // private/protected member does not. Here `Hidden` is private, so
        // only `Shown` and the unmodified `Implicit` are public.
        check_metrics::<CsharpParser>(
            "interface I {
                private static int Hidden = 1;
                protected static int AlsoHidden = 2;
                public static int Shown = 3;
                static int Implicit = 4;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.interface_na_sum(), 4);
                // Only `Shown` (explicit public) and `Implicit` (default
                // public) count; `Hidden`/`AlsoHidden` are excluded.
                assert_eq!(metric.npa.interface_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_interface_multi_declarator_modifier() {
        // The visibility modifier applies to every declarator of a field, so
        // the public/private split is per-declaration. `private int a, b;`
        // contributes two attributes, neither public; `int c, d;` (default
        // public) contributes two public attributes.
        check_metrics::<CsharpParser>(
            "interface I {
                private static int a = 1, b = 2;
                static int c = 3, d = 4;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.interface_na_sum(), 4);
                assert_eq!(metric.npa.interface_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn php_one_public_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { public int $x = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_one_private_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { private int $x = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_one_protected_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { protected int $x = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_mixed_visibility_attributes() {
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public int $a = 0;
                public int $b = 0;
                private int $c = 0;
                protected int $d = 0;
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_static_public_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { public static int $x = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_readonly_public_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { public readonly int $x; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_multiple_attributes_per_declaration() {
        // A single property_declaration can declare several
        // property_elements; each counts.
        check_metrics::<PhpParser>(
            "<?php class A { public int $a = 0, $b = 0, $c = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_interface_constants() {
        // Interface constants are implicitly public.
        check_metrics::<PhpParser>(
            "<?php
            interface I {
                const A = 1;
                const B = 2;
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_enum_cases_not_counted() {
        // #781: enum cases are sum-type tags, not data fields, so they
        // contribute zero npa attributes — matching the Java, Kotlin,
        // Rust, and C# convention. Before #781 this enum reported
        // class_na = class_npa = 3 (one per case); it must now be 0.
        check_metrics::<PhpParser>(
            "<?php
            enum Color {
                case Red;
                case Green;
                case Blue;
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0);
                assert_eq!(metric.npa.class_npa_sum(), 0);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn php_enum_const_not_counted() {
        // #781: a PHP enum body may declare `const`s alongside its
        // cases, but class-level `const`s are not counted as attributes
        // outside an enum either (only `PropertyDeclaration` counts), so
        // the enum const is consistently excluded. This enum reports 0.
        check_metrics::<PhpParser>(
            "<?php
            enum Suit: string {
                case Hearts = 'H';
                case Diamonds = 'D';
                const Wild = 'W';
                public function label(): string { return $this->name; }
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0);
                assert_eq!(metric.npa.class_npa_sum(), 0);
            },
        );
    }

    #[test]
    fn php_enum_npa_matches_java_enum_npa() {
        // #781 cross-language parity: an enum whose only members are
        // cases reports the same npa (0) in PHP and Java. The cases are
        // sum-type tags, excluded by both languages. `check_metrics`
        // takes a non-capturing `fn`, so each side asserts the shared
        // target (0, 0) independently; parity follows by transitivity.
        check_metrics::<PhpParser>(
            "<?php
            enum Color {
                case Red;
                case Green;
                case Blue;
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0);
                assert_eq!(metric.npa.class_npa_sum(), 0);
            },
        );
        check_metrics::<JavaParser>(
            "enum Color {
                RED, GREEN, BLUE;
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0);
                assert_eq!(metric.npa.class_npa_sum(), 0);
            },
        );
    }

    #[test]
    fn php_trait_attributes() {
        check_metrics::<PhpParser>(
            "<?php
            trait T {
                public int $a = 0;
                private int $b = 0;
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_no_explicit_visibility_excluded() {
        // PHP 8.x deprecates implicit-public for properties; we follow
        // Java's strict-explicit rule and do NOT count properties without
        // an explicit `public` modifier.
        check_metrics::<PhpParser>("<?php class A { var $x = 0; }", "foo.php", |metric| {
            // The property is excluded from the public-count (npa) because
            // `var` is not an explicit `public` modifier, but still
            // contributes to the total-count (na). This split is the spec.
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.class_na_sum(), 1);
            assert_eq!(metric.npa.interface_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn php_anonymous_class_attributes() {
        // Anonymous classes have their own DeclarationList space and
        // their public properties count. The Npa impl branches on
        // `parent_kind == AnonymousClass` and this test exercises that
        // arm.
        check_metrics::<PhpParser>(
            "<?php
            $obj = new class {
                public int $a = 0;
                private int $b = 0;
            };",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[test]
    fn php_property_promotion_excluded() {
        // Constructor property promotion (PHP 8.0+) declares both a
        // parameter AND a property in one syntax. The promoted property
        // lives under `formal_parameters`, NOT under
        // `declaration_list`, so the current Npa impl naturally
        // excludes it. This is a documented limitation; this test
        // pins the behavior so a future change that starts counting
        // promoted properties has to update the test deliberately.
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public function __construct(public string $x, public int $y) {}
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    // --- Kotlin NPA tests -------------------------------------------------
    //
    // Reference: Kotlin properties (`val` / `var`) declared inside a class
    // body are attributes. Default visibility is `public`. Primary
    // constructor parameters carrying `val` / `var` are parameter
    // properties and count. Companion-object members fold into the
    // enclosing class. Top-level properties belong to the `Unit` space
    // and are excluded.

    #[test]
    fn kotlin_empty_class_no_attributes() {
        check_metrics::<KotlinParser>("class C {}", "foo.kt", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.interface_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn kotlin_public_val_var_default() {
        // Kotlin's default visibility is public — no modifier means public.
        check_metrics::<KotlinParser>(
            "class C {
                val a: Int = 1
                var b: Int = 2
                val c: String = \"hi\"
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 3);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_private_val_var() {
        // Private properties contribute to total `na` but not to `npa`.
        check_metrics::<KotlinParser>(
            "class C {
                val a: Int = 1               // public
                private val b: Int = 2       // not public
                var c: Int = 3               // public
                private var d: Int = 4       // not public
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 4);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_protected_internal_excluded_from_public() {
        check_metrics::<KotlinParser>(
            "open class C {
                protected val a: Int = 1
                internal val b: Int = 2
                public val c: Int = 3        // explicit public
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_primary_constructor_parameter_property() {
        // `val`/`var` on primary constructor parameters declares both a
        // parameter AND a property. Bare `name: Type` parameters are NOT
        // attributes.
        check_metrics::<KotlinParser>(
            "class C(val a: Int, var b: Int, c: Int) {
                val d: Int = c
            }",
            "foo.kt",
            |metric| {
                // a, b, d -> public; c -> not an attribute (no val/var)
                assert_eq!(metric.npa.class_npa_sum(), 3);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_primary_constructor_private_param_property() {
        check_metrics::<KotlinParser>(
            "class C(private val a: Int, val b: Int)",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_secondary_constructor_does_not_add_attrs() {
        // Secondary constructors are methods, not attribute declarations.
        check_metrics::<KotlinParser>(
            "class C {
                private var a: Int = 0
                constructor(n: Int) { a = n }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 0);
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_companion_object_attributes() {
        // Companion-object properties fold into the enclosing class as
        // "static" attributes.
        check_metrics::<KotlinParser>(
            "class Holder {
                val instance: Int = 1
                companion object {
                    val SCALE: Int = 10
                    private val SECRET: Int = 7
                }
            }",
            "foo.kt",
            |metric| {
                // instance (public) + SCALE (public) = 2 public
                // SECRET counts toward total na but not npa
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_data_class_attributes() {
        // `data class` parameters are the canonical positional attributes.
        check_metrics::<KotlinParser>(
            "data class Point(val x: Int, val y: Int)",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_object_singleton_attributes() {
        check_metrics::<KotlinParser>(
            "object Config {
                val DEFAULT: Int = 42
                private val SEED: Int = 0
                var debug: Boolean = false
            }",
            "foo.kt",
            |metric| {
                // DEFAULT, debug -> public; SEED -> not.
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_interface_attributes() {
        // Interface members are implicitly public; all properties count
        // toward `interface_npa` and `interface_na`. Structural
        // `assert_child_space_kind` guards against an
        // `InterfaceDeclaration` revert in `KotlinCode::is_func_space`
        // — see #311.
        check_func_space::<KotlinParser, _>(
            "interface I {
                val a: Int
                val b: String
            }",
            "foo.kt",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npa.interface_npa_sum(), 2);
                assert_eq!(metric.npa.interface_na_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn kotlin_nested_class_attributes() {
        // Each class space has its own attribute count; nested class
        // attributes do not leak into the outer class.
        check_metrics::<KotlinParser>(
            "class Outer {
                val o1: Int = 1
                class Nested {
                    val n1: Int = 1
                    val n2: Int = 2
                }
            }",
            "foo.kt",
            |metric| {
                // 2 classes total — Outer's 1 + Nested's 2 = 3 attributes
                assert_eq!(metric.npa.class_npa_sum(), 3);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_inner_class_attributes() {
        check_metrics::<KotlinParser>(
            "class Outer {
                val o1: Int = 1
                inner class Inner {
                    val i1: Int = 1
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_top_level_properties_excluded() {
        // Top-level `val` belongs to `Unit`, not a class — must not
        // contribute to `class_na`.
        check_metrics::<KotlinParser>(
            "val topVal: Int = 0
            var topVar: Int = 1
            class C { val x: Int = 0 }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_multiple_classes_attributes() {
        check_metrics::<KotlinParser>(
            "class A {
                val a1: Int = 0
                var a2: Int = 0
            }
            class B {
                val b1: Int = 0
                private val b2: Int = 0
            }",
            "foo.kt",
            |metric| {
                // A: 2 public; B: 1 public + 1 private = 2 total, 1 public
                assert_eq!(metric.npa.class_npa_sum(), 3);
                assert_eq!(metric.npa.class_na_sum(), 4);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_class_with_methods_no_attrs() {
        // Methods are not attributes.
        check_metrics::<KotlinParser>(
            "class C {
                fun m1() {}
                fun m2(): Int = 0
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 0);
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    // --- TypeScript / TSX NPA tests --------------------------------------
    //
    // TypeScript class fields are `public_field_definition` direct children
    // of `class_body`. Default visibility is public; an explicit
    // `accessibility_modifier` whose only child is `private`/`protected`
    // demotes a field. Constructor parameter properties
    // (`constructor(private x: number)`) count as class attributes.
    // Fields whose initializer is an arrow function are methods, not
    // attributes. Interface property signatures count as implicitly
    // public attributes.

    #[test]
    fn typescript_empty_class_no_attributes() {
        check_metrics::<TypescriptParser>("class C {}", "foo.ts", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn typescript_default_public_fields() {
        // No accessibility modifier means public.
        check_metrics::<TypescriptParser>(
            "class C {
                a: number = 1;
                b: string = \"\";
                c: boolean = false;
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 3);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_visibility_modifiers() {
        // Public / private / protected. Default public.
        check_metrics::<TypescriptParser>(
            "class C {
                public a: number = 1;
                private b: number = 2;
                protected c: number = 3;
                d: number = 4;
            }",
            "foo.ts",
            |metric| {
                // public + default(public) = 2 npa; total na = 4.
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 4);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_static_fields() {
        // `static` is orthogonal to visibility — the field still counts.
        check_metrics::<TypescriptParser>(
            "class C {
                static a: number = 0;
                public static b: number = 0;
                private static c: number = 0;
            }",
            "foo.ts",
            |metric| {
                // a (default public) + b (public) = 2 npa; c is private.
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_parameter_properties() {
        // Constructor parameter properties are class attributes.
        check_metrics::<TypescriptParser>(
            "class C {
                constructor(public a: number, private b: string, c: boolean) {}
            }",
            "foo.ts",
            |metric| {
                // a, b are parameter properties (modifiered); c is a plain
                // parameter and does NOT count. a is public, b is private.
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_readonly_constructor_param_property() {
        // A bare `readonly` constructor parameter is a public parameter
        // property (regression for #459): `readonly` is a distinct keyword,
        // not an `accessibility_modifier`, yet must count like a `readonly`
        // class field. `e` is a plain parameter and does NOT count.
        // `private readonly d` carries both modifier children but must count
        // exactly once (and as non-public, so it lands in na but not npa).
        check_metrics::<TypescriptParser>(
            "class C {
                constructor(private a: number, readonly b: number, public c: number, e: number, private readonly d: number) {}
            }",
            "foo.ts",
            |metric| {
                // Properties: a (private), b (readonly→public), c (public),
                // d (private readonly). e is not a property. na = 4.
                // npa counts the public ones: b, c. npa = 2.
                assert_eq!(metric.npa.class_na_sum(), 4);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_readonly_field() {
        // `readonly` is a non-visibility modifier — the field still counts
        // and stays public unless paired with private/protected.
        check_metrics::<TypescriptParser>(
            "class C {
                readonly a: number = 1;
                private readonly b: number = 2;
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_abstract_class_attributes() {
        // `abstract_class_declaration` opens its own class space; fields
        // count just like a concrete class.
        check_metrics::<TypescriptParser>(
            "abstract class C {
                public a: number = 1;
                protected b: number = 2;
                abstract m(): void;
            }",
            "foo.ts",
            |metric| {
                // a (public) + b (protected) = 2 attrs; npa = 1.
                // `abstract m()` is a method, not an attribute.
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_arrow_field_is_method_not_attribute() {
        // A field whose initializer is an arrow function is counted by
        // npm, not npa.
        check_metrics::<TypescriptParser>(
            "class C {
                a: number = 0;
                arrow = () => this.a;
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_interface_property_signatures() {
        // Interface property signatures count as implicitly-public
        // attributes; method signatures are not attributes.
        // Structural `assert_child_space_kind` guards against an
        // `InterfaceDeclaration` revert in
        // `TypescriptCode::is_func_space` — see #311.
        check_func_space::<TypescriptParser, _>(
            "interface I {
                a: number;
                b: string;
                m(): void;
            }",
            "foo.ts",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npa.interface_npa_sum(), 2);
                assert_eq!(metric.npa.interface_na_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn typescript_generic_class_attributes() {
        // Type parameters on the class do not contribute attributes.
        check_metrics::<TypescriptParser>(
            "class Box<T, U> {
                value: T;
                other: U;
                constructor(v: T, o: U) { this.value = v; this.other = o; }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_getters_setters_not_attributes() {
        // `get x()` / `set x(v)` are method_definitions, not attributes.
        check_metrics::<TypescriptParser>(
            "class C {
                private _x: number = 0;
                get x(): number { return this._x; }
                set x(v: number) { this._x = v; }
            }",
            "foo.ts",
            |metric| {
                // Only `_x` counts as an attribute (private → not public).
                assert_eq!(metric.npa.class_npa_sum(), 0);
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_multiple_classes_and_interface() {
        check_func_space::<TypescriptParser, _>(
            "class A { x: number = 0; }
             class B { private y: number = 0; }
             interface I { z: number; }",
            "foo.ts",
            |func_space| {
                let metric = &func_space.metrics;
                // A: 1 npa / 1 na (public). B: 0 npa / 1 na (private).
                // I: 1 interface_npa / 1 interface_na.
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.interface_npa_sum(), 1);
                assert_eq!(metric.npa.interface_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
                assert_child_space_kind(&func_space, "A", SpaceKind::Class);
                assert_child_space_kind(&func_space, "B", SpaceKind::Class);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn typescript_nested_class_attributes_independent() {
        // Each class space tracks its own attributes; the outer class's
        // sum gets the inner-class sum via merge. The Outer class has
        // two `public_field_definition` direct children — `a` and the
        // `Inner` static field whose value is a class expression.
        // The class expression itself opens a separate `class` space
        // with its own two fields. Total counted across both spaces:
        // 2 (Outer: a + Inner) + 2 (inner anonymous class: b, c) = 4.
        check_metrics::<TypescriptParser>(
            "class Outer {
                a: number = 0;
                static Inner = class {
                    b: number = 0;
                    c: number = 0;
                };
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 4);
                assert_eq!(metric.npa.class_na_sum(), 4);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    // TSX parity tests — mirror the TS rules to confirm the shared helper
    // expansion behaves identically on the TSX grammar.

    #[test]
    fn tsx_empty_class_no_attributes() {
        check_metrics::<TsxParser>("class C {}", "foo.tsx", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn tsx_default_public_fields() {
        check_metrics::<TsxParser>(
            "class C {
                a: number = 1;
                b: string = \"\";
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_visibility_modifiers() {
        check_metrics::<TsxParser>(
            "class C {
                public a: number = 1;
                private b: number = 2;
                protected c: number = 3;
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_parameter_properties() {
        check_metrics::<TsxParser>(
            "class C {
                constructor(public a: number, private b: string) {}
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_readonly_constructor_param_property() {
        // TSX sibling of `typescript_readonly_constructor_param_property`
        // (#459): a bare `readonly` constructor parameter is a public
        // parameter property; `e` is not a property; `private readonly d`
        // counts once and is non-public.
        check_metrics::<TsxParser>(
            "class C {
                constructor(private a: number, readonly b: number, public c: number, e: number, private readonly d: number) {}
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 4);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_abstract_class_attributes() {
        check_metrics::<TsxParser>(
            "abstract class C {
                public a: number = 1;
                private b: number = 2;
                abstract m(): void;
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_interface_property_signatures() {
        check_func_space::<TsxParser, _>(
            "interface I {
                a: number;
                b: string;
                m(): void;
            }",
            "foo.tsx",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npa.interface_npa_sum(), 2);
                assert_eq!(metric.npa.interface_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn tsx_arrow_field_is_method_not_attribute() {
        check_metrics::<TsxParser>(
            "class C {
                a: number = 0;
                arrow = () => this.a;
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_static_fields() {
        check_metrics::<TsxParser>(
            "class C {
                static a: number = 0;
                private static b: number = 0;
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_readonly_field() {
        check_metrics::<TsxParser>(
            "class C {
                readonly a: number = 1;
                private readonly b: number = 2;
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_generic_class_attributes() {
        check_metrics::<TsxParser>("class Box<T> { value: T; }", "foo.tsx", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 1);
            assert_eq!(metric.npa.class_na_sum(), 1);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn tsx_getters_setters_not_attributes() {
        check_metrics::<TsxParser>(
            "class C {
                private _x: number = 0;
                get x(): number { return this._x; }
                set x(v: number) { this._x = v; }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 0);
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_multiple_classes_and_interface() {
        check_func_space::<TsxParser, _>(
            "class A { x: number = 0; }
             class B { private y: number = 0; }
             interface I { z: number; }",
            "foo.tsx",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.interface_npa_sum(), 1);
                assert_eq!(metric.npa.interface_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
                assert_child_space_kind(&func_space, "A", SpaceKind::Class);
                assert_child_space_kind(&func_space, "B", SpaceKind::Class);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    // --- Ruby NPA tests ---------------------------------------------------
    //
    // Ruby has no field-declaration syntax; class-scope instance and
    // class variables are introduced by direct assignment in the class
    // body (`@var = …`, `@@var = …`). `attr_accessor` / `attr_reader`
    // / `attr_writer` macros synthesise reader/writer pairs and also
    // introduce attributes. Visibility flows from keyword markers as
    // in `Npm`.

    #[test]
    fn ruby_no_class_attributes() {
        check_metrics::<RubyParser>(
            "class A\n  def f\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 0);
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_instance_variable_attribute() {
        // Bare `@x = …` at class scope is one public attribute.
        check_metrics::<RubyParser>("class A\n  @x = 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 1);
            assert_eq!(metric.npa.class_na_sum(), 1);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn ruby_class_variable_attribute() {
        // `@@y = …` at class scope is one attribute.
        check_metrics::<RubyParser>("class A\n  @@y = 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 1);
            assert_eq!(metric.npa.class_na_sum(), 1);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn ruby_attr_accessor_counts_symbols() {
        // `attr_accessor :x, :y, :z` declares three attributes.
        check_metrics::<RubyParser>(
            "class A\n  attr_accessor :x, :y, :z\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 3);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_attr_reader_and_writer() {
        check_metrics::<RubyParser>(
            "class A\n  attr_reader :r1, :r2\n  attr_writer :w\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 3);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_mixed_attributes_and_assignments() {
        check_metrics::<RubyParser>(
            "class A\n  attr_accessor :x, :y\n  @z = 1\n  @@w = 2\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 4);
                assert_eq!(metric.npa.class_na_sum(), 4);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_private_attributes() {
        // Bare `private` flips visibility for the subsequent attr.
        check_metrics::<RubyParser>(
            "class A\n  attr_accessor :pub\n  private\n  attr_accessor :hidden\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_visibility_public_resets_private() {
        // `private` then `public` returns to default-public.
        check_metrics::<RubyParser>(
            "class A\n  attr_reader :a\n  private\n  attr_reader :b\n  public\n  attr_reader :c\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_method_scope_assignments_excluded() {
        // `@x = 1` inside a method does NOT count — it's a method-local
        // instance-variable write, not a class-scope attribute
        // declaration.
        check_metrics::<RubyParser>(
            "class A\n  def init\n    @x = 1\n    @@y = 2\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 0);
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_module_attributes_not_counted() {
        // `module M` is a `Namespace` space — its attr_* macros and
        // class-variable assignments do NOT contribute to NPA.
        check_metrics::<RubyParser>(
            "module M\n  attr_accessor :x\n  @@m = 1\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 0);
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_inheritance_attributes() {
        // Inheritance does not change the attribute count for this class.
        check_metrics::<RubyParser>(
            "class A < B\n  attr_accessor :x\n  @y = 0\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_constant_assignments_excluded() {
        // `CONST = …` at class scope binds a constant, not an
        // attribute; the LHS is a `Constant`, not an
        // `InstanceVariable` / `ClassVariable`.
        check_metrics::<RubyParser>(
            "class A\n  PI = 3.14\n  E = 2.71\n  attr_reader :x\nend\n",
            "foo.rb",
            |metric| {
                // Only `attr_reader :x` counts.
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_multiple_classes_attribute_rollup() {
        check_metrics::<RubyParser>(
            "class A\n  attr_accessor :x\nend\nclass B\n  private\n  attr_accessor :y\nend\n",
            "foo.rb",
            |metric| {
                // A: 1 public attr. B: 0 public, 1 total.
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    // ---------------------------------------------------------------
    // Default-impl placeholder smoke tests (audited in #188).
    //
    // Each test feeds a class / struct with public attributes to a
    // language whose `Npa` is currently the default no-op. The
    // assertion pins the current 0 value with a TODO pointing at the
    // follow-up issue — when the real impl lands the assertion will
    // fire and force a test update, which is the gate.
    // ---------------------------------------------------------------

    // --- Python NPA ---------------------------------------------------

    #[test]
    fn python_empty_class_no_attributes() {
        check_metrics::<PythonParser>("class C:\n    pass\n", "foo.py", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.interface_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn python_class_level_assignments_are_attributes() {
        // Two class-level `=` assignments → 2 attributes, all public
        // (Python has no visibility keyword).
        check_metrics::<PythonParser>("class C:\n    x = 1\n    y = 2\n", "foo.py", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 2);
            assert_eq!(metric.npa.class_npa_sum(), 2);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn python_bare_type_annotation_not_attribute() {
        // `x: int` is a bare annotation (declares a type, binds
        // nothing); only `y: int = 2` actually creates an attribute.
        check_metrics::<PythonParser>(
            "class C:\n    x: int\n    y: int = 2\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn python_self_attributes_in_init() {
        // `self.x` and `self.y` assigned in `__init__` → 2 instance
        // attributes attributed to the class space.
        check_metrics::<PythonParser>(
            "class C:\n    def __init__(self):\n        self.x = 1\n        self.y = 2\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn python_self_attributes_in_nested_control_flow() {
        // `self.z = 1` and `self.z = 2` in if/else now count once —
        // #215 added identifier-text deduplication. Both branches
        // bind the same attribute `z`, so `class_na == 1`.
        check_metrics::<PythonParser>(
            "class C:\n    def __init__(self, flag):\n        if flag:\n            self.z = 1\n        else:\n            self.z = 2\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// Regression #215: `self.value = …` bound in `__init__` and again
    /// in `reset()` should count the attribute exactly once. Before
    /// identifier-text deduplication, each binding inflated
    /// `class_na` by one — the defensive re-init pattern reported 2.
    ///
    /// The two assignments use DIFFERENT right-hand sides (`None`
    /// vs `0`) so a hypothetical byte-content-of-Assignment dedup
    /// (rather than identifier-name dedup) would NOT collapse them.
    /// This pins the rule to the attribute *name*, not the
    /// assignment text.
    #[test]
    fn python_defensive_reinit_self_attribute_counts_once() {
        check_metrics::<PythonParser>(
            "class C:\n    def __init__(self):\n        self.value = None\n    def reset(self):\n        self.value = 0\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// Distinct attribute names still accumulate normally — the
    /// dedup is per-name, not per-method.
    #[test]
    fn python_distinct_self_attributes_count_independently() {
        check_metrics::<PythonParser>(
            "class C:\n    def __init__(self):\n        self.x = 1\n        self.y = 2\n        self.z = 3\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// Annotated `self.x: int = 1` inside a method body parses as
    /// `Assignment(target=Attribute(self, x), type, value)` in
    /// tree-sitter-python — the same node type as plain `self.x = 1`.
    /// The dedup helper must see both forms and treat them as the
    /// same attribute. Regression guard for the review finding on
    /// #215: ensure annotated assignments aren't missed.
    #[test]
    fn python_self_attribute_annotated_assignment_dedupes() {
        check_metrics::<PythonParser>(
            "class C:\n    def __init__(self):\n        self.value: int = 1\n    def reset(self):\n        self.value = 0\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn python_class_level_and_self_attrs_combine() {
        // 1 class-level + 2 instance = 3 total attributes.
        check_metrics::<PythonParser>(
            "class C:\n    counter = 0\n    def __init__(self):\n        self.name = 'a'\n        self.value = 1\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn python_self_attrs_isolated_per_class() {
        // Nested class `Inner` opens its own class space; its
        // `self.z = …` belongs to Inner. The class_na_sum aggregates
        // across class spaces in the file, so we see both attributes
        // (Outer.x + Inner.z) in the unit-level sum; the snapshot
        // pins the per-space breakdown.
        check_metrics::<PythonParser>(
            "class Outer:\n\
             \x20   def __init__(self):\n\
             \x20       self.x = 1\n\
             \x20   class Inner:\n\
             \x20       def __init__(self):\n\
             \x20           self.z = 2\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn python_decorated_methods_do_not_inflate_attrs() {
        // `@property` / `@staticmethod` wrap a `FunctionDefinition` in
        // `DecoratedDefinition`. These contribute methods, not
        // attributes — Npa must stay at 0.
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   @property\n\
             \x20   def p(self):\n\
             \x20       return 1\n\
             \x20   @staticmethod\n\
             \x20   def s():\n\
             \x20       return 2\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn python_module_level_assignments_not_attributes() {
        // `x = 1` at module scope is not a class attribute.
        check_metrics::<PythonParser>("x = 1\ny = 2\nclass C:\n    a = 3\n", "foo.py", |metric| {
            // Only `a = 3` lives in the class space.
            assert_eq!(metric.npa.class_na_sum(), 1);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    /// #412 (a): a write to a *foreign* object's attribute
    /// (`db.connection = …`, `logger.level = …`) is not an attribute of
    /// the class. Only `self.name` — whose receiver is the `self` alias
    /// — counts. The prior structural-only check treated every
    /// `obj.x = …` as an instance attribute, reporting 3.
    #[test]
    fn python_foreign_object_writes_not_attributes() {
        check_metrics::<PythonParser>(
            "class Service:\n\
             \x20   def __init__(self, db, logger):\n\
             \x20       self.name = \"svc\"\n\
             \x20       db.connection = None\n\
             \x20       logger.level = \"INFO\"\n",
            "foo.py",
            |metric| {
                // Only self.name; db.* and logger.* are foreign.
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// #412 (b): tuple-unpacking instance attributes. The target of
    /// `self.a, self.b = 1, 2` is a `pattern_list`, not a single
    /// `Attribute`; the prior code bailed on non-Attribute targets and
    /// missed both `a` and `b`, reporting 1 (only `self.c`).
    #[test]
    fn python_self_attribute_unpacking_counts_each() {
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   def __init__(self):\n\
             \x20       self.a, self.b = 1, 2\n\
             \x20       self.c = 3\n",
            "foo.py",
            |metric| {
                // a, b, c.
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// Nested unpacking of instance attributes: `self.a, (self.b, self.c)
    /// = …` nests a `tuple_pattern` inside the outer `pattern_list`. The
    /// shared `python_walk_target_elements` recursion descends into the
    /// nested pattern so `b` and `c` are counted, not just `a` (review
    /// follow-up to #412 (b); a flat iteration reports 1).
    #[test]
    fn python_self_attribute_nested_unpacking_counts_each() {
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   def __init__(self):\n\
             \x20       self.a, (self.b, self.c) = 1, (2, 3)\n",
            "foo.py",
            |metric| {
                // a, b, c — all three, including the nested b and c.
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
            },
        );
    }

    /// Nested unpacking at class level: `(a, (b, c)) = 1, (2, 3)` nests a
    /// `tuple_pattern` inside the target. Each bound name — including the
    /// nested `b` and `c` — contributes one attribute (review follow-up to
    /// #412 (c); a flat iteration reports 1).
    #[test]
    fn python_class_level_nested_unpacking_counts_each() {
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   (a, (b, c)) = 1, (2, 3)\n",
            "foo.py",
            |metric| {
                // a, b, c.
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
            },
        );
    }

    /// Minimal regression for the hidden-alias bug: a flat *parenthesized*
    /// or *bracketed* class-level unpacking target (`(p, q) = …`,
    /// `[m, n] = …`) parses to the live `tuple_pattern` (179) /
    /// `list_pattern` (180) node, not the bare `pattern_list` the
    /// unparenthesized `p, q = …` form uses. Matching only the hidden
    /// supertype aliases (168 / 167) dropped these entirely; both bound
    /// names must be counted (#419 hidden-alias discipline).
    #[test]
    fn python_class_level_parenthesized_unpacking_counts_each() {
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   (p, q) = 1, 2\n\
             \x20   [m, n] = 3, 4\n",
            "foo.py",
            |metric| {
                // p, q, m, n.
                assert_eq!(metric.npa.class_na_sum(), 4);
                assert_eq!(metric.npa.class_npa_sum(), 4);
            },
        );
    }

    /// #412 (b) edge: unpacking that mixes a self attribute with a
    /// foreign / local target (`self.a, x = …`) counts only the self
    /// attribute.
    #[test]
    fn python_self_attribute_unpacking_skips_non_self_targets() {
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   def __init__(self):\n\
             \x20       self.a, x = 1, 2\n",
            "foo.py",
            |metric| {
                // Only `a`; the bare local `x` is not an attribute.
                assert_eq!(metric.npa.class_na_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// #412 (c): a multi-target class-level assignment binds one
    /// attribute per name. `a = b = 3` (chained) binds two; `p, q = 1,
    /// 2` (unpacking) binds two; with `x = 1` that is five names. The
    /// prior code counted one per `=` statement, reporting 3.
    #[test]
    fn python_class_level_multi_target_counts_each_name() {
        check_metrics::<PythonParser>(
            "class C:\n    x = 1\n    a = b = 3\n    p, q = 1, 2\n",
            "foo.py",
            |metric| {
                // x, a, b, p, q.
                assert_eq!(metric.npa.class_na_sum(), 5);
                assert_eq!(metric.npa.class_npa_sum(), 5);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// #412 (b)/(c): a chained instance assignment `self.a = self.b = 1`
    /// binds both `a` and `b` on `self`. The nested `Assignment` in the
    /// value is visited by the subtree walk, so both are counted.
    #[test]
    fn python_chained_self_assignment_counts_each() {
        check_metrics::<PythonParser>(
            "class C:\n    def __init__(self):\n        self.a = self.b = 1\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// #412 (a): a classmethod binds class attributes through the `cls`
    /// alias; `cls.registry = …` counts, while a foreign `other.thing =
    /// …` write in the same body does not.
    #[test]
    fn python_classmethod_cls_attribute_counts() {
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   @classmethod\n\
             \x20   def make(cls, other):\n\
             \x20       cls.registry = {}\n\
             \x20       other.thing = 1\n",
            "foo.py",
            |metric| {
                // Only cls.registry; other.thing is foreign.
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// #412 (a) edge: a nested-attribute write `self.f.g = 1` sets `g`
    /// on `self.f`; it does NOT introduce a new attribute of the class.
    /// The receiver of the outer Attribute is itself an Attribute
    /// (`self.f`), not the `self` Identifier, so it is rejected.
    #[test]
    fn python_nested_self_attribute_not_counted() {
        check_metrics::<PythonParser>(
            "class C:\n    def __init__(self):\n        self.f.g = 1\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    /// #412 dedup: a class default `x = 1` and an instance write
    /// `self.x = 2` name the same attribute; the instance binding
    /// shadows the class default, so `x` counts once. The class-level
    /// and instance passes share one dedup set.
    #[test]
    fn python_class_default_and_self_attr_dedupe() {
        check_metrics::<PythonParser>(
            "class C:\n    x = 1\n    def __init__(self):\n        self.x = 2\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn rust_empty_unit_no_attributes() {
        check_metrics::<RustParser>("", "empty.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.interface_na_sum(), 0);
            assert_eq!(metric.npa.interface_npa_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn rust_struct_fields_are_attributes() {
        // 3 named fields → class_na = 3. `pub a` and `pub c` are public
        // → class_npa = 2. `b` is private, so it's not in `npa`.
        check_metrics::<RustParser>(
            "struct Foo { pub a: i32, b: String, pub c: bool }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn rust_pub_self_field_is_private() {
        // Regression for #460. A `pub(self)` / `pub(in self)` field
        // restricts to the current module and is private, like no
        // modifier. The widening forms (`pub(crate)`, `pub(super)`,
        // `pub`, `pub(in <path>)`) stay public. → 7 fields, 4 public
        // (b, d, e, f). Pre-fix `a`/`a2` over-counted (class_npa_sum=6,
        // revert-verified). Asserts `pub(super)`/`pub(crate)` are NOT
        // over-suppressed.
        check_metrics::<RustParser>(
            "struct S {\n\
             \x20   pub(self) a: i32,\n\
             \x20   pub(in self) a2: i32,\n\
             \x20   pub(crate) b: i32,\n\
             \x20   pub(super) d: i32,\n\
             \x20   pub(in crate::x) e: i32,\n\
             \x20   pub f: i32,\n\
             \x20   c: i32,\n\
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 7);
                assert_eq!(metric.npa.class_npa_sum(), 4);
            },
        );
    }

    #[test]
    fn rust_pub_self_assoc_const_is_private() {
        // Regression for #460 on the associated-const path. `pub(self)`
        // and `pub(in self)` associated consts are private; `pub(crate)`
        // and `pub` are public. → 4 consts, 2 public (B, D). Pre-fix
        // `A`/`A2` over-counted (class_npa_sum=4, revert-verified).
        check_metrics::<RustParser>(
            "struct Foo;\n\
             impl Foo {\n\
             \x20   pub(self) const A: i32 = 1;\n\
             \x20   pub(in self) const A2: i32 = 2;\n\
             \x20   pub(crate) const B: i32 = 3;\n\
             \x20   pub const D: i32 = 4;\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 4);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[test]
    fn rust_pub_self_tuple_field_is_private() {
        // Regression for #460 on the tuple-struct positional path.
        // `pub(self)` / `pub(in self)` fields are private; `pub(crate)`
        // and bare `pub` stay public. 5 positional fields; only the
        // `pub(crate) i32` and `pub u8` are public → 2 public (the
        // trailing `String` carries no modifier). Pre-fix the two
        // `self`-restricted fields over-counted (class_npa_sum=4,
        // revert-verified).
        check_metrics::<RustParser>(
            "struct Bar(pub(self) i32, pub(in self) i32, pub(crate) i32, pub u8, String);",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 5);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[test]
    fn rust_tuple_struct_fields_are_attributes() {
        // Tuple-struct field counting is positional. `Bar(pub i32,
        // String)` → 2 fields, 1 public.
        check_metrics::<RustParser>("struct Bar(pub i32, String);", "foo.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 2);
            assert_eq!(metric.npa.class_npa_sum(), 1);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn rust_unit_struct_has_no_attributes() {
        // `struct Empty;` is a unit struct (no fields). 0 attributes.
        check_metrics::<RustParser>("struct Empty;", "foo.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn rust_empty_struct_body_has_no_attributes() {
        // `struct Empty {}` is named-field with zero fields.
        check_metrics::<RustParser>("struct Empty { }", "foo.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn rust_impl_associated_consts_are_attributes() {
        // `const X` and `pub const Y` and `static Z` and `pub static W`
        // → 4 associated attributes, 2 public.
        check_metrics::<RustParser>(
            "struct Foo;\n\
             impl Foo {\n\
             \x20   const X: i32 = 1;\n\
             \x20   pub const Y: i32 = 2;\n\
             \x20   static Z: i32 = 3;\n\
             \x20   pub static W: i32 = 4;\n\
             }\n",
            "foo.rs",
            |metric| {
                // The Impl-space class_na is 4; rolled up to Unit
                // class_na_sum it is also 4 (no struct fields in `Foo;`).
                assert_eq!(metric.npa.class_na_sum(), 4);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn rust_trait_consts_and_associated_types_are_attributes() {
        // `const DEFAULT_COLOR` + `type Item` → 2 interface attributes,
        // both public by trait convention. Structural
        // `assert_child_space_kind` pins the trait FuncSpace against
        // an `is_func_space` revert (see #311).
        check_func_space::<RustParser, _>(
            "trait Drawable { const DEFAULT_COLOR: u32; type Item; }",
            "foo.rs",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npa.interface_na_sum(), 2);
                assert_eq!(metric.npa.interface_npa_sum(), 2);
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
                assert_child_space_kind(&func_space, "Drawable", SpaceKind::Trait);
            },
        );
    }

    #[test]
    fn rust_multiple_impls_aggregate() {
        // Two `impl Foo` blocks each have one associated const. The
        // unit-level rollup should be class_na_sum = 2.
        check_metrics::<RustParser>(
            "struct Foo;\n\
             impl Foo { const X: i32 = 1; }\n\
             impl Foo { pub const Y: i32 = 2; }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn rust_module_level_consts_not_attributes() {
        // `const PI: f64 = 3.14;` at file scope is a free-standing
        // constant — NOT a class attribute. Only consts INSIDE an
        // `impl` / `trait` body count.
        check_metrics::<RustParser>(
            "const PI: f64 = 3.14;\nstatic Q: i32 = 0;\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0);
                assert_eq!(metric.npa.interface_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    // ----- Go -----

    #[test]
    fn go_empty_unit_no_attributes() {
        // Package-only file declares no struct → npa stays disabled,
        // class_na_sum = 0.
        check_metrics::<GoParser>("package main\n", "empty.go", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn go_empty_struct_has_no_attributes() {
        // `type Empty struct{}` has an empty FieldDeclarationList →
        // 0 fields → npa stays disabled.
        check_metrics::<GoParser>("package main\ntype Empty struct{}\n", "foo.go", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn go_struct_fields_are_attributes() {
        // Three named fields: `X int`, `y string`, `Z float64` → 3
        // attributes. Go visibility is lexical (issue #458): `X` and
        // `Z` are exported, `y` is not → class_npa_sum = 2.
        check_metrics::<GoParser>(
            "package main\ntype Foo struct { X int; y string; Z float64 }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn go_grouped_struct_fields_each_count() {
        // `X, Y int` declares two field names in one
        // field_declaration; each name is its own attribute
        // (issue #458). With the trailing `Z` → 3 attributes total,
        // all exported → class_npa_sum = 3.
        check_metrics::<GoParser>(
            "package main\ntype Point struct { X, Y int; Z float64 }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn go_embedded_type_counts_as_attribute() {
        // `io.Reader` and `*Foo` are embedded types — field
        // declarations with no name, just a type; the embedded
        // type's base name (`Reader`, `Foo`) is the attribute name
        // and decides its visibility (issue #458). Both are
        // exported; `n int` is not → class_na_sum = 3,
        // class_npa_sum = 2.
        check_metrics::<GoParser>(
            "package main\nimport \"io\"\ntype Bar struct { io.Reader; *Foo; n int }\ntype Foo struct {}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn go_multiple_structs_aggregate_at_unit() {
        // Two structs declared at file scope each contribute their
        // fields to the same Unit space (no per-receiver class
        // grouping in Go). `Foo` has 1 field, `Bar` has 2 → total
        // class_na_sum = 3. All three names are lowercase
        // (unexported), so class_npa_sum = 0 (issue #458).
        check_metrics::<GoParser>(
            "package main\ntype Foo struct { x int }\ntype Bar struct { a int; b string }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn go_top_level_var_const_not_attributes() {
        // Package-level `var` and `const` declarations are NOT
        // struct fields — they are free-standing identifiers.
        // Expected class_na_sum = 0.
        check_metrics::<GoParser>(
            "package main\nvar Counter int\nconst Pi = 3.14\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn go_npa_excludes_unexported() {
        // Issue #458: mixed exported / unexported fields exercising a
        // multi-name declaration (`A, b int`), an embedded field
        // (`io.Reader`), the blank identifier (`_`), and a Unicode
        // uppercase first char (`Ärger`).
        //
        // Names: Name(exp), secret(no), A(exp), b(no), Reader(exp),
        //   _(no), Ärger(exp) → na = 7, npa = 4. Revert-verified
        //   against the old all-public code, which counted every
        //   FieldDeclaration node once (class_npa_sum = class_na_sum
        //   = 6, undercounting the grouped names).
        check_metrics::<GoParser>(
            "package main\nimport \"io\"\n\
             type T struct { Name string; secret int; A, b int; io.Reader; _ int; Ärger bool }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 7);
                assert_eq!(metric.npa.class_npa_sum(), 4);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    // ----- Elixir -----

    // Issue #275: `defstruct` is Elixir's closest analog to a class
    // field-set declaration. We count its field arguments as
    // (public) attributes.
    #[test]
    fn elixir_npa_defstruct_keyword_list() {
        check_metrics::<ElixirParser>(
            "defmodule User do\n  defstruct name: nil, age: 0, email: nil\nend\n",
            "foo.ex",
            |metric| {
                // Three keyword pairs → 3 fields, all public.
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
            },
        );
    }

    #[test]
    fn elixir_npa_defstruct_atom_list() {
        check_metrics::<ElixirParser>(
            "defmodule User do\n  defstruct [:name, :age, :email]\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
            },
        );
    }

    #[test]
    fn elixir_npa_defstruct_bracketed_keyword_list() {
        check_metrics::<ElixirParser>(
            "defmodule User do\n  defstruct [name: nil, age: 0]\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[test]
    fn elixir_npa_defstruct_single_field() {
        check_metrics::<ElixirParser>(
            "defmodule Box do\n  defstruct value: nil\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[test]
    fn elixir_npa_no_defstruct_is_zero() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def m, do: :ok\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0);
                assert_eq!(metric.npa.class_npa_sum(), 0);
            },
        );
    }

    /// The `Npa` half of the #1088 simplification: a `defmodule` inside
    /// a `quote` template still opens a class, so its `defstruct` fields
    /// are still counted.
    ///
    /// Same reasoning as `npm::tests::
    /// elixir_npm_counts_a_quoted_defmodule_as_a_class` — the
    /// `is_func_space_with_code` gate that used to precede the
    /// `defmodule` keyword check could not change the outcome, because
    /// `elixir_is_class_macro` is exactly `defmodule`.
    #[test]
    fn elixir_npa_counts_a_quoted_defmodule_as_a_class() {
        check_metrics::<ElixirParser>(
            "defmodule Outer do\n  defstruct [:x]\n  defmacro gen do\n    quote do\n      defmodule Inner do\n        defstruct [:a, :b]\n      end\n    end\n  end\nend\n",
            "outer.ex",
            |metric| {
                // `Outer` contributes `x`; the quoted `Inner` contributes
                // `a` and `b`. Elixir struct fields are all public.
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
            },
        );
    }

    // ----- Objective-C -----

    #[test]
    fn objc_npa() {
        // `@property` is always a public attribute. Instance variables
        // default to `@protected`; a visibility marker flips the current
        // visibility for the fields that *follow* — including flipping
        // back to non-public — and a multi-declarator `int a, b;` is two
        // attributes. Here:
        //   ivars: `_prot` (default @protected), `_priv` (@private),
        //          `_pub1` + `_pub2` (@public → 2 public), `_prot2`
        //          (@protected, resetting the visibility) → 5 total, 2
        //          public.
        //   properties: `count`, `name` → 2 public.
        // → interface_na = 7, interface_npa = 4. The trailing `@protected`
        // resets visibility, so `_prot2` is NOT public.
        check_metrics::<ObjcParser>(
            "@interface Foo : NSObject {\n\
                 int _prot;\n\
             @private\n\
                 int _priv;\n\
             @public\n\
                 int _pub1, _pub2;\n\
             @protected\n\
                 int _prot2;\n\
             }\n\
             @property (nonatomic) int count;\n\
             @property (copy) NSString *name;\n\
             @end\n",
            "foo.m",
            |metric| {
                assert_eq!(metric.npa.interface_na_sum(), 7);
                assert_eq!(metric.npa.interface_npa_sum(), 4);
            },
        );
    }

    #[test]
    fn objc_npa_protocol() {
        // A `@protocol`'s `@property` after an `@optional` / `@required`
        // marker nests under a `qualified_protocol_interface_declaration`;
        // it must still be counted (regression for the direct-children
        // walk that missed it).
        check_metrics::<ObjcParser>(
            "@protocol Drawable <NSObject>\n\
             @optional\n\
             @property (readonly) int z;\n\
             @end\n",
            "foo.m",
            |metric| {
                assert_eq!(metric.npa.interface_na_sum(), 1);
                assert_eq!(metric.npa.interface_npa_sum(), 1);
            },
        );
    }

    // ----- C++ -----

    #[test]
    fn cpp_empty_unit_no_attributes() {
        // No code → no class spaces → npa = 0. Establishes the trait
        // is wired and the per-language compute is reachable.
        check_metrics::<CppParser>("", "empty.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.class_npa_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn cpp_empty_class_no_attributes() {
        // `class Foo {};` has no fields. Marked as class space (npa
        // becomes visible) but counts stay at 0.
        check_metrics::<CppParser>("class Foo {};", "foo.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.class_npa_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn cpp_class_public_attributes() {
        // `class` defaults to private. `public:` flips visibility →
        // `int a; int b, c;` becomes 3 public attributes (multi-
        // declarator declaration emits one `field_identifier` per
        // name). Total: class_na = 3, class_npa = 3.
        check_metrics::<CppParser>(
            "class Foo { public: int a; int b, c; };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn cpp_class_private_default_visibility() {
        // No access specifier → `class` keeps its default private
        // visibility → `int value_;` counts as 1 attribute but 0 are
        // public. class_na = 1, class_npa = 0.
        check_metrics::<CppParser>("class Foo { int value_; };", "foo.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 1);
            assert_eq!(metric.npa.class_npa_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn cpp_struct_default_public_visibility() {
        // `struct` defaults to public — opposite of `class`. The same
        // field counts once and is public.
        check_metrics::<CppParser>("struct Bar { int value_; };", "foo.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 1);
            assert_eq!(metric.npa.class_npa_sum(), 1);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn cpp_mixed_visibility_sections() {
        // Public section: 1 field. Protected section (bucketed with
        // private for npa): 1 field. Private section: 1 field.
        // class_na = 3, class_npa = 1.
        check_metrics::<CppParser>(
            "class Foo {\n\
                 public: int a;\n\
                 protected: int b;\n\
                 private: int c;\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn cpp_methods_not_counted_as_attributes() {
        // Inline-defined methods (`function_definition`) and
        // declaration-only methods (`field_declaration` containing
        // `function_declarator`) must NOT be counted as attributes.
        // Only the data field `value_` adds to `class_na`.
        check_metrics::<CppParser>(
            "class Foo {\n\
                 public:\n\
                     void method1() {}\n\
                     void method2();\n\
                 private:\n\
                     int value_;\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn cpp_pointer_array_fields_count() {
        // `int* p;` wraps the `field_identifier` inside
        // `pointer_declarator`. `int a[10];` wraps it inside
        // `array_declarator`. Both must be reached by the recursive
        // helper. Plus a plain `int x;` → 3 attributes total.
        check_metrics::<CppParser>(
            "struct S {\n\
                 int* p;\n\
                 int a[10];\n\
                 int x;\n\
             };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                // Struct → all public.
                assert_eq!(metric.npa.class_npa_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn cpp_multiple_classes_aggregate_at_unit() {
        // Two classes in one file. Each contributes to its own
        // class space; the file-level (Unit) class_na_sum aggregates
        // both. Foo has 2 attrs (1 public, 1 private). Bar has 1.
        // Total class_na_sum at Unit = 3.
        check_metrics::<CppParser>(
            "class Foo { public: int a; private: int b; };\nstruct Bar { int c; };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                // Public: Foo::a (1) + Bar::c (1) = 2.
                assert_eq!(metric.npa.class_npa_sum(), 2);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn javascript_empty_unit_no_attributes() {
        // Wires up the trait and ensures no spurious attribute counts
        // on an empty file.
        check_metrics::<JavascriptParser>("", "empty.js", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.class_npa_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn javascript_empty_class_no_attributes() {
        // A class with no body and no fields has zero attributes.
        check_metrics::<JavascriptParser>("class Foo {}", "foo.js", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.class_npa_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn javascript_class_fields_count() {
        // ES2022 class fields: `class Foo { x = 1; y; static z = 2; }`.
        // All three are `field_definition` direct children of
        // `class_body`. JS has no visibility — everything is public.
        // class_na = class_npa = 3.
        check_metrics::<JavascriptParser>(
            "class Foo { x = 1; y; static z = 2; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn javascript_arrow_field_is_method_not_attribute() {
        // `class Foo { x = () => {} }` declares a method, not an
        // attribute. The arrow function initializer makes this an
        // `Npm` member, not an `Npa` member.
        check_metrics::<JavascriptParser>(
            "class Foo { x = () => {}; y = function() {}; z = 1; }",
            "foo.js",
            |metric| {
                // Only `z = 1` is an attribute.
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn javascript_methods_not_counted_as_attributes() {
        // `method_definition` direct children of `class_body` are
        // methods, not fields. They must not show up in `npa`.
        check_metrics::<JavascriptParser>(
            "class Foo { constructor() {} bar() {} get baz() { return 1; } x = 1; }",
            "foo.js",
            |metric| {
                // Only `x = 1` is a true attribute.
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn javascript_multiple_classes_aggregate_at_unit() {
        // Two classes contribute their attribute counts to the
        // Unit-level rollup. Foo has 2 fields; Bar has 1. Total
        // class_na_sum = 3.
        check_metrics::<JavascriptParser>(
            "class Foo { a = 1; b = 2; }\nclass Bar { c = 3; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn mozjs_class_fields_count() {
        // Mozjs shares JS's class vocabulary. Same expectation as the
        // JS parity test above.
        check_metrics::<MozjsParser>(
            "class Foo { x = 1; y; static z = 2; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    // Regression for #438: an empty class has zero attributes, so the
    // CDA accessors divide 0.0 / 0.0. Before the zero-guard this yielded
    // NaN (serialized to JSON `null`). The defined value is 0.0 — an
    // attribute-less class exposes no public surface. Asserting
    // `!is_nan()` proves the guard fires; the `== 0.0` checks pin the
    // chosen convention. Exercised across the explicit-visibility OO
    // languages (Java, C#, Kotlin, PHP).
    #[test]
    fn empty_class_cda_is_zero_not_nan() {
        let assert_zero = |metric: crate::CodeMetrics| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert!(!metric.npa.class_cda().is_nan());
            assert!(!metric.npa.total_cda().is_nan());
            assert_eq!(metric.npa.class_cda(), 0.0);
            assert_eq!(metric.npa.total_cda(), 0.0);
        };
        check_metrics::<JavaParser>("class Foo {}", "foo.java", assert_zero);
        check_metrics::<CsharpParser>("class Foo {}", "foo.cs", assert_zero);
        check_metrics::<KotlinParser>("class Foo {}", "foo.kt", assert_zero);
        check_metrics::<PhpParser>("<?php class Foo {}", "foo.php", assert_zero);
    }

    // Regression for #438: an empty interface has zero attributes; the
    // existing all-public guard explicitly excludes the empty case
    // (`!= 0`), so without the divisor guard `interface_cda` returned
    // 0.0 / 0.0 = NaN. The defined value is 0.0.
    #[test]
    fn empty_interface_cda_is_zero_not_nan() {
        let assert_zero = |metric: crate::CodeMetrics| {
            assert_eq!(metric.npa.interface_na_sum(), 0);
            assert!(!metric.npa.interface_cda().is_nan());
            assert_eq!(metric.npa.interface_cda(), 0.0);
        };
        check_metrics::<JavaParser>("interface Foo {}", "foo.java", assert_zero);
        check_metrics::<CsharpParser>("interface Foo {}", "foo.cs", assert_zero);
    }

    // Rounds out `npa`'s public surface — the `Display` impl and the
    // per-space `class_npa` / `class_na` / `interface_*` accessors —
    // mirroring the `Display` tests the sibling metrics carry.
    #[test]
    fn stats_display_and_per_space_accessors() {
        check_func_space::<JavaParser, _>(
            "public interface I {\n    int K = 1;\n}\n\
             public class C {\n    public int a;\n    private int b;\n}\n",
            "X.java",
            |unit| {
                // Class C: a public, b private → 1 public of 2 attributes.
                // Interface I: one constant K.
                assert_eq!(unit.metrics.npa.class_npa_sum(), 1);
                assert_eq!(unit.metrics.npa.class_na_sum(), 2);
                let rendered = unit.metrics.npa.to_string();
                for fragment in [
                    "classes: 1, interfaces: 1",
                    "class_attributes: 2",
                    "interface_attributes: 1",
                    "total: 2, total_attributes: 3",
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
                assert_eq!(class.metrics.npa.class_npa(), 1);
                assert_eq!(class.metrics.npa.class_na(), 2);
                let iface = child_space(&unit, "I");
                assert_eq!(iface.kind, SpaceKind::Interface);
                assert_eq!(iface.metrics.npa.interface_npa(), 1);
                assert_eq!(iface.metrics.npa.interface_na(), 1);
            },
        );
    }
}
