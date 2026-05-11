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
use crate::langs::*;
use crate::macros::{csharp_var_decl_kinds, csharp_var_declarator_kinds, implement_metric_trait};
use crate::node::Node;
use crate::*;

/// The `Npa` metric.
///
/// This metric counts the number of public attributes
/// of classes/interfaces.
#[derive(Clone, Debug, Default)]
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

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("npa", 9)?;
        st.serialize_field("classes", &self.class_npa_sum())?;
        st.serialize_field("interfaces", &self.interface_npa_sum())?;
        st.serialize_field("class_attributes", &self.class_na_sum())?;
        st.serialize_field("interface_attributes", &self.interface_na_sum())?;
        st.serialize_field("classes_average", &self.class_cda())?;
        st.serialize_field("interfaces_average", &self.interface_cda())?;
        st.serialize_field("total", &self.total_npa())?;
        st.serialize_field("total_attributes", &self.total_na())?;
        st.serialize_field("average", &self.total_cda())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "classes: {}, interfaces: {}, class_attributes: {}, interface_attributes: {}, classes_average: {}, interfaces_average: {}, total: {}, total_attributes: {}, average: {}",
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
    pub fn class_npa(&self) -> f64 {
        self.class_npa as f64
    }

    /// Returns the number of interface public attributes in a space.
    #[inline]
    #[must_use]
    pub fn interface_npa(&self) -> f64 {
        self.interface_npa as f64
    }

    /// Returns the number of class attributes in a space.
    #[inline]
    #[must_use]
    pub fn class_na(&self) -> f64 {
        self.class_na as f64
    }

    /// Returns the number of interface attributes in a space.
    #[inline]
    #[must_use]
    pub fn interface_na(&self) -> f64 {
        self.interface_na as f64
    }

    /// Returns the number of class public attributes sum in a space.
    #[inline]
    #[must_use]
    pub fn class_npa_sum(&self) -> f64 {
        self.class_npa_sum as f64
    }

    /// Returns the number of interface public attributes sum in a space.
    #[inline]
    #[must_use]
    pub fn interface_npa_sum(&self) -> f64 {
        self.interface_npa_sum as f64
    }

    /// Returns the number of class attributes sum in a space.
    #[inline]
    #[must_use]
    pub fn class_na_sum(&self) -> f64 {
        self.class_na_sum as f64
    }

    /// Returns the number of interface attributes sum in a space.
    #[inline]
    #[must_use]
    pub fn interface_na_sum(&self) -> f64 {
        self.interface_na_sum as f64
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
        self.class_npa_sum() / self.class_na_sum as f64
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
        // For the Java language it's not necessary to compute the metric value
        // The metric value in Java can only be 1.0 or f64:NAN
        if self.interface_npa_sum == self.interface_na_sum && self.interface_npa_sum != 0 {
            1.0
        } else {
            self.interface_npa_sum() / self.interface_na_sum()
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
        self.total_npa() / self.total_na()
    }

    /// Returns the total number of public attributes in a space.
    #[inline]
    #[must_use]
    pub fn total_npa(&self) -> f64 {
        self.class_npa_sum() + self.interface_npa_sum()
    }

    /// Returns the total number of attributes in a space.
    #[inline]
    #[must_use]
    pub fn total_na(&self) -> f64 {
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
}

/// Per-language counting of public attributes.
pub trait Npa
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    fn compute(node: &Node, stats: &mut Stats);
}

impl Npa for JavaCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Java::*;

        // Enables the `Npa` metric if computing stats of a class space
        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            ClassBody => {
                stats.class_na += node
                    .children()
                    .filter(|node| matches!(node.kind_id().into(), FieldDeclaration))
                    .map(|declaration| {
                        let attributes = declaration
                            .children()
                            .filter(|n| matches!(n.kind_id().into(), VariableDeclarator))
                            .count();
                        // The first child node contains the list of attribute modifiers
                        // There are several modifiers that may be part of a field declaration
                        // Source: https://docs.oracle.com/javase/tutorial/reflect/member/fieldModifiers.html
                        if declaration.child(0).is_some_and(|modifiers| {
                            // Looks for the `public` keyword in the list of attribute modifiers
                            matches!(modifiers.kind_id().into(), Modifiers)
                                && modifiers.first_child(|id| id == Public).is_some()
                        }) {
                            stats.class_npa += attributes;
                        }
                        attributes
                    })
                    .sum::<usize>();
            }
            // Every field declaration in the body of an interface is implicitly public, static, and final
            // Source: https://docs.oracle.com/javase/specs/jls/se7/html/jls-9.html
            InterfaceBody => {
                // Children nodes are filtered because Java interfaces
                // can contain constants but also methods and nested types
                // Source: https://docs.oracle.com/javase/tutorial/java/IandI/createinterface.html
                stats.interface_na += node
                    .children()
                    .filter(|node| matches!(node.kind_id().into(), ConstantDeclaration))
                    .flat_map(|node| node.children())
                    .filter(|node| matches!(node.kind_id().into(), VariableDeclarator))
                    .count();
                stats.interface_npa = stats.interface_na;
            }
            _ => {}
        }
    }
}

// C# uses individual `Modifier` nodes (not wrapped under a single
// `modifiers` node like Java); detecting `public` requires scanning
// every Modifier child of the declaration for a `public` keyword.
pub(crate) fn csharp_is_explicit_public(declaration: &Node) -> bool {
    declaration.children().any(|child| {
        matches!(child.kind_id().into(), Csharp::Modifier)
            && child.first_child(|id| id == Csharp::Public).is_some()
    })
}

impl Npa for CsharpCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Csharp::*;

        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        // Class / struct / record / interface bodies all share
        // `DeclarationList`; the parent kind disambiguates.
        if !matches!(node.kind_id().into(), DeclarationList) {
            return;
        }
        let Some(parent_kind) = node.parent().map(|p| p.kind_id().into()) else {
            return;
        };
        match parent_kind {
            // For `RecordDeclaration`, only explicit body fields are
            // counted. The implicit `parameter_list` of a positional
            // record (`record Person(string Name, int Age);`) is not
            // walked here — its parameters become auto-generated public
            // properties at the IL level, but modelling them would
            // require synthesizing nodes that don't appear in the AST.
            ClassDeclaration | StructDeclaration | RecordDeclaration => {
                for declaration in node
                    .children()
                    .filter(|c| matches!(c.kind_id().into(), FieldDeclaration))
                {
                    let attributes = csharp_count_field_declarators(&declaration);
                    stats.class_na += attributes;
                    if csharp_is_explicit_public(&declaration) {
                        stats.class_npa += attributes;
                    }
                }
            }
            // C# 8+ interfaces can declare fields with explicit modifiers
            // (rare); members declared without an explicit modifier default
            // to public, mirroring Java's interface convention.
            InterfaceDeclaration => {
                for declaration in node
                    .children()
                    .filter(|c| matches!(c.kind_id().into(), FieldDeclaration))
                {
                    let attributes = csharp_count_field_declarators(&declaration);
                    stats.interface_na += attributes;
                    stats.interface_npa = stats.interface_na;
                }
            }
            _ => {}
        }
    }
}

// Count `VariableDeclarator`s nested under any aliased `VariableDeclaration`
// inside a C# `FieldDeclaration`. Both kinds emit two aliased `kind_id`s
// each; the macros centralize the alias union (lesson #2).
fn csharp_count_field_declarators(field_decl: &Node) -> usize {
    field_decl
        .children()
        .filter(|c| matches!(c.kind_id().into(), csharp_var_decl_kinds!()))
        .flat_map(|c| c.children())
        .filter(|c| matches!(c.kind_id().into(), csharp_var_declarator_kinds!()))
        .count()
}

// PHP's strict-explicit visibility rule (mirroring Java's pattern): a
// declaration is treated as public only when it carries an explicit
// `public` modifier. Modifier-less declarations — deprecated for
// properties since PHP 8 and merely conventional for methods — are NOT
// counted, even though PHP semantically defaults methods to public.
pub(crate) fn php_is_explicit_public(declaration: &Node) -> bool {
    declaration.children().any(|child| {
        matches!(child.kind_id().into(), Php::VisibilityModifier)
            && child.first_child(|id| id == Php::Public).is_some()
    })
}

impl Npa for PhpCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Php::*;

        // Enables the `Npa` metric if computing stats of a class-like space.
        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            // Class / trait / anonymous-class / interface bodies all share
            // the `DeclarationList` kind; the parent kind disambiguates.
            DeclarationList => {
                let Some(parent_kind) = node.parent().map(|p| p.kind_id().into()) else {
                    return;
                };
                match parent_kind {
                    ClassDeclaration | TraitDeclaration | AnonymousClass => {
                        for declaration in node
                            .children()
                            .filter(|c| matches!(c.kind_id().into(), PropertyDeclaration))
                        {
                            let attributes = declaration
                                .children()
                                .filter(|c| matches!(c.kind_id().into(), PropertyElement))
                                .count();
                            stats.class_na += attributes;
                            if php_is_explicit_public(&declaration) {
                                stats.class_npa += attributes;
                            }
                        }
                    }
                    // Interfaces cannot declare properties but can declare
                    // class constants, which are implicitly public.
                    InterfaceDeclaration => {
                        let count: usize = node
                            .children()
                            .filter(|c| {
                                matches!(c.kind_id().into(), ConstDeclaration | ConstDeclaration2)
                            })
                            .map(|decl| {
                                decl.children()
                                    .filter(|n| {
                                        matches!(n.kind_id().into(), ConstElement | ConstElement2)
                                    })
                                    .count()
                            })
                            .sum();
                        stats.interface_na += count;
                        stats.interface_npa = stats.interface_na;
                    }
                    _ => {}
                }
            }
            // Enum cases are public read-only constants of the enum.
            EnumDeclarationList => {
                let count = node
                    .children()
                    .filter(|c| matches!(c.kind_id().into(), EnumCase))
                    .count();
                stats.class_na += count;
                stats.class_npa += count;
            }
            _ => {}
        }
    }
}

// Kotlin's grammar models classes and interfaces under a single
// `class_declaration` node; the `class` / `interface` keyword child
// disambiguates. A `ClassBody` belongs to an interface iff its parent
// `class_declaration` has an `interface` keyword child.
pub(crate) fn kotlin_class_body_is_interface(class_body: &Node) -> bool {
    class_body.parent().is_some_and(|p| {
        matches!(p.kind_id().into(), Kotlin::ClassDeclaration)
            && p.first_child(|id| id == Kotlin::Interface).is_some()
    })
}

// Counts how many `VariableDeclaration`s a Kotlin `PropertyDeclaration`
// introduces. Kotlin allows destructuring (`val (a, b) = pair`) via
// `MultiVariableDeclaration`; each leaf binding counts as one attribute.
// Empty multi-variable destructurings cannot occur in well-formed Kotlin,
// but a defensive `.max(1)` keeps `property_declaration` at ≥1 attribute
// (matches the C# accessor-counting fallback).
fn kotlin_count_property_attrs(decl: &Node) -> usize {
    use Kotlin::*;
    decl.children()
        .map(|c| match c.kind_id().into() {
            VariableDeclaration => 1,
            MultiVariableDeclaration => c
                .children()
                .filter(|n| matches!(n.kind_id().into(), VariableDeclaration))
                .count(),
            _ => 0,
        })
        .sum::<usize>()
        .max(1)
}

// Kotlin's default visibility is `public`. A declaration is non-public
// only when it carries an explicit `private` / `protected` / `internal`
// modifier under its `Modifiers` child. Returns `true` for missing
// `Modifiers`, missing `VisibilityModifier`, or an explicit `public`
// modifier.
pub(crate) fn kotlin_is_public(decl: &Node) -> bool {
    let Some(modifiers) = decl.first_child(|id| id == Kotlin::Modifiers) else {
        return true;
    };
    let Some(visibility) = modifiers.first_child(|id| id == Kotlin::VisibilityModifier) else {
        return true;
    };
    // The visibility modifier holds exactly one keyword child; absence or
    // an explicit `public` both mean public.
    visibility
        .first_child(|id| {
            matches!(
                id.into(),
                Kotlin::Private | Kotlin::Protected | Kotlin::Internal
            )
        })
        .is_none()
}

impl Npa for KotlinCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Kotlin::*;

        // Enables the `Npa` metric for both class and interface spaces
        // (and `object` singletons, which `Getter` reports as `Class`).
        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            // A `ClassParameter` carrying `val` / `var` is a Kotlin
            // primary-constructor parameter property — counts once toward
            // the enclosing class. Parameters without `val`/`var` are plain
            // constructor arguments, not attributes.
            ClassParameter => {
                if node
                    .children()
                    .any(|c| matches!(c.kind_id().into(), Val | Var))
                {
                    stats.class_na += 1;
                    if kotlin_is_public(node) {
                        stats.class_npa += 1;
                    }
                }
            }
            // Every `ClassBody` we visit attributes its direct
            // `property_declaration` children to whichever func_space is
            // currently on the state stack. Companion objects are not
            // func_spaces, so companion `val`/`var` declarations land on
            // the enclosing class — matching Kotlin's "static members"
            // semantics. Nested class / interface bodies start a new
            // func_space (handled by `spaces.rs`), so they do NOT leak
            // attributes into their outer space.
            ClassBody => {
                let is_interface = kotlin_class_body_is_interface(node);
                // tree-sitter-kotlin elides the `class_member_declaration`
                // and `declaration` rule layers when those rules are pure
                // forwarding choices, so property declarations appear as
                // direct children of `class_body`.
                for prop in node
                    .children()
                    .filter(|c| matches!(c.kind_id().into(), PropertyDeclaration))
                {
                    let attrs = kotlin_count_property_attrs(&prop);
                    if is_interface {
                        stats.interface_na += attrs;
                        // Interface members are always public.
                        stats.interface_npa += attrs;
                    } else {
                        stats.class_na += attrs;
                        if kotlin_is_public(&prop) {
                            stats.class_npa += attrs;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

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
// `accessibility_modifier` adds one to the enclosing class's `na`
// (and to `npa` when the modifier is `public` or absent). The
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
        fn compute(node: &Node, stats: &mut Stats) {
            use $lang::*;

            if Self::is_func_space(node) && stats.is_disabled() {
                stats.is_class_space = true;
            }

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
                                        .first_child(|id| id == $lang::AccessibilityModifier)
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

impl Npa for TypescriptCode {
    ts_npa_compute!(Typescript);
}

impl Npa for TsxCode {
    ts_npa_compute!(Tsx);
}

implement_metric_trait!(
    Npa,
    PythonCode,
    MozjsCode,
    JavascriptCode,
    RustCode,
    CppCode,
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
    use crate::tools::check_metrics;

    use super::*;

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
                    @r###"
                    {
                      "classes": 8.0,
                      "interfaces": 0.0,
                      "class_attributes": 16.0,
                      "interface_attributes": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 8.0,
                      "total_attributes": 16.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 20.0,
                      "interfaces": 0.0,
                      "class_attributes": 40.0,
                      "interface_attributes": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 20.0,
                      "total_attributes": 40.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 20.0,
                      "interfaces": 0.0,
                      "class_attributes": 40.0,
                      "interface_attributes": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 20.0,
                      "total_attributes": 40.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 20.0,
                      "interfaces": 0.0,
                      "class_attributes": 40.0,
                      "interface_attributes": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 20.0,
                      "total_attributes": 40.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 12.0,
                      "interfaces": 0.0,
                      "class_attributes": 24.0,
                      "interface_attributes": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 12.0,
                      "total_attributes": 24.0,
                      "average": 0.5
                    }"###
                );
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
                    @r###"
                    {
                      "classes": 9.0,
                      "interfaces": 0.0,
                      "class_attributes": 18.0,
                      "interface_attributes": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 9.0,
                      "total_attributes": 18.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 10.0,
                      "interfaces": 0.0,
                      "class_attributes": 16.0,
                      "interface_attributes": 0.0,
                      "classes_average": 0.625,
                      "interfaces_average": null,
                      "total": 10.0,
                      "total_attributes": 16.0,
                      "average": 0.625
                    }"###
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
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 0.0,
                      "class_attributes": 6.0,
                      "interface_attributes": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 3.0,
                      "total_attributes": 6.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 0.0,
                      "class_attributes": 3.0,
                      "interface_attributes": 0.0,
                      "classes_average": 1.0,
                      "interfaces_average": null,
                      "total": 3.0,
                      "total_attributes": 3.0,
                      "average": 1.0
                    }"###
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
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 0.0,
                      "class_attributes": 3.0,
                      "interface_attributes": 0.0,
                      "classes_average": 1.0,
                      "interfaces_average": null,
                      "total": 3.0,
                      "total_attributes": 3.0,
                      "average": 1.0
                    }"###
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
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 0.0,
                      "class_attributes": 5.0,
                      "interface_attributes": 0.0,
                      "classes_average": 0.6,
                      "interfaces_average": null,
                      "total": 3.0,
                      "total_attributes": 5.0,
                      "average": 0.6
                    }"###
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
                    @r###"
                    {
                      "classes": 0.0,
                      "interfaces": 3.0,
                      "class_attributes": 0.0,
                      "interface_attributes": 3.0,
                      "classes_average": null,
                      "interfaces_average": 1.0,
                      "total": 3.0,
                      "total_attributes": 3.0,
                      "average": 1.0
                    }"###
                );
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
                assert_eq!(metric.npa.class_npa_sum(), 8.0);
                assert_eq!(metric.npa.class_na_sum(), 16.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 12.0);
                assert_eq!(metric.npa.class_na_sum(), 18.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 4.0);
                assert_eq!(metric.npa.class_na_sum(), 5.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 4.0);
                assert_eq!(metric.npa.class_na_sum(), 5.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 4.0);
                assert_eq!(metric.npa.class_na_sum(), 7.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                assert_eq!(metric.npa.class_na_sum(), 5.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 4.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn csharp_interface() {
        // EC14 — interface members default to public; all fields count.
        check_metrics::<CsharpParser>(
            "interface I {
                static int A = 1;
                static string B = \"hello\";
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0.0);
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
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
    fn php_enum_cases() {
        // Enum cases are public read-only constants.
        check_metrics::<PhpParser>(
            "<?php
            enum Color {
                case Red;
                case Green;
                case Blue;
            }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
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
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
            assert_eq!(metric.npa.class_na_sum(), 1.0);
            assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 4.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
                assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn kotlin_interface_attributes() {
        // Interface members are implicitly public; all properties count
        // toward `interface_npa` and `interface_na`.
        check_metrics::<KotlinParser>(
            "interface I {
                val a: Int
                val b: String
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npa.interface_npa_sum(), 2.0);
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 0.0);
                insta::assert_json_snapshot!(metric.npa);
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
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                assert_eq!(metric.npa.class_na_sum(), 4.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
                assert_eq!(metric.npa.class_na_sum(), 0.0);
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
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
            assert_eq!(metric.npa.class_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 4.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_interface_property_signatures() {
        // Interface property signatures count as implicitly-public
        // attributes; method signatures are not attributes.
        check_metrics::<TypescriptParser>(
            "interface I {
                a: number;
                b: string;
                m(): void;
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npa.interface_npa_sum(), 2.0);
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 0.0);
                insta::assert_json_snapshot!(metric.npa);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn typescript_multiple_classes_and_interface() {
        check_metrics::<TypescriptParser>(
            "class A { x: number = 0; }
             class B { private y: number = 0; }
             interface I { z: number; }",
            "foo.ts",
            |metric| {
                // A: 1 npa / 1 na (public). B: 0 npa / 1 na (private).
                // I: 1 interface_npa / 1 interface_na.
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.interface_npa_sum(), 1.0);
                assert_eq!(metric.npa.interface_na_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npa);
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
                assert_eq!(metric.npa.class_npa_sum(), 4.0);
                assert_eq!(metric.npa.class_na_sum(), 4.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    // TSX parity tests — mirror the TS rules to confirm the shared helper
    // expansion behaves identically on the TSX grammar.

    #[test]
    fn tsx_empty_class_no_attributes() {
        check_metrics::<TsxParser>("class C {}", "foo.tsx", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
            assert_eq!(metric.npa.class_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_interface_property_signatures() {
        check_metrics::<TsxParser>(
            "interface I {
                a: number;
                b: string;
                m(): void;
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.interface_npa_sum(), 2.0);
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npa);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_generic_class_attributes() {
        check_metrics::<TsxParser>("class Box<T> { value: T; }", "foo.tsx", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 1.0);
            assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn tsx_multiple_classes_and_interface() {
        check_metrics::<TsxParser>(
            "class A { x: number = 0; }
             class B { private y: number = 0; }
             interface I { z: number; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.interface_npa_sum(), 1.0);
                assert_eq!(metric.npa.interface_na_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }
}
