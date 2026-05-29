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

#[doc(hidden)]
/// Per-language counting of public attributes.
pub trait Npa
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
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats);
}

// Java and Groovy share their grammar tokens for class/interface
// bodies, so `Npa::compute` differs only by the language enum.
// `impl_npa_java_like!` emits the same body against each enum
// (issue #280).
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
            fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
                use $lang::*;

                if Self::is_func_space(node) && stats.is_disabled() {
                    stats.is_class_space = true;
                }

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

impl_npa_java_like!(JavaCode, Java);

// Groovy uses the dekobon grammar, which models class/interface/trait/
// annotation-type/record bodies as a single `class_body` node and
// flattens modifiers as direct children of the declaration (the
// `_modifier` rule is hidden — no `Modifiers` wrapper). That rules out
// the Java macro, so an explicit impl is required.
//
// `def field` at class scope parses as a `FieldDeclaration` with `Def`
// in the modifier slot and no `Public`, so it's correctly excluded from
// `class_npa` unless explicitly annotated `public` — consistent with
// Groovy's access semantics (we conservatively follow Java).
impl Npa for GroovyCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Groovy::*;

        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            ClassBody | EnumBody => {
                let is_interface_like = groovy_body_is_interface_like(node);

                for declaration in node
                    .children()
                    .filter(|n| matches!(n.kind_id().into(), FieldDeclaration))
                {
                    let attributes = declaration
                        .children()
                        .filter(|n| matches!(n.kind_id().into(), VariableDeclarator))
                        .count();
                    if is_interface_like {
                        stats.interface_na += attributes;
                        stats.interface_npa += attributes;
                    } else {
                        stats.class_na += attributes;
                        if groovy_has_explicit_public(&declaration) {
                            stats.class_npa += attributes;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// Distinguishes interface-like containers (interface, trait, annotation
// type) — whose members are implicitly public — from class-like
// containers (class, enum, record) that need an explicit `public`
// modifier. The dekobon grammar models all of these bodies as
// `class_body`, so the discriminant lives on the parent. Shared with
// `impl Npm for GroovyCode` (`metrics::npm`).
pub(crate) fn groovy_body_is_interface_like(body: &Node) -> bool {
    use Groovy::*;
    body.parent().is_some_and(|p| {
        matches!(
            p.kind_id().into(),
            InterfaceDeclaration | TraitDeclaration | AnnotationTypeDeclaration
        )
    })
}

// Detects an explicit `public` modifier on a class member declaration.
// The dekobon grammar flattens the `_modifier` rule, so modifier
// tokens appear as direct children of the declaration — no `Modifiers`
// wrapper to descend into. Shared with `impl Npm for GroovyCode`.
pub(crate) fn groovy_has_explicit_public(declaration: &Node) -> bool {
    declaration.first_child(|id| id == Groovy::Public).is_some()
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
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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

// Counts the number of symbol arguments passed to an `attr_accessor` /
// `attr_reader` / `attr_writer` macro `Call` node. `attr_accessor :a,
// :b, :c` exposes three attributes; an `attr_*` call with no arguments
// is ill-formed Ruby but defensively returns zero rather than one.
pub(crate) fn ruby_attr_macro_symbol_count(call: &Node) -> usize {
    use Ruby::*;

    call.children()
        .find(|c| matches!(c.kind_id().into(), ArgumentList | ArgumentList2))
        .map_or(0, |args| {
            args.children()
                .filter(|c| {
                    matches!(
                        c.kind_id().into(),
                        SimpleSymbol | DelimitedSymbol | HashKeySymbol | BareSymbol
                    )
                })
                .count()
        })
}

// Ruby class-body visibility state. `private` / `public` / `protected`
// keywords flip this flag for every subsequent declaration in the same
// body until another marker overrides them. The default at the top of
// every class body is `Public`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RubyVisibility {
    Public,
    Private,
    Protected,
}

// Recognises a bare visibility-keyword `identifier` child of a Ruby
// class body (`private` / `public` / `protected` with no arguments).
// tree-sitter-ruby emits the keyword-form as a literal `identifier`
// token; the argument-form (`private :foo`, `private def bar`) is a
// `Call` node instead and does NOT flip the body-wide flag.
pub(crate) fn ruby_visibility_marker(node: &Node, source: &[u8]) -> Option<RubyVisibility> {
    if !matches!(node.kind_id().into(), Ruby::Identifier) {
        return None;
    }
    match node.utf8_text(source)? {
        "private" => Some(RubyVisibility::Private),
        "public" => Some(RubyVisibility::Public),
        "protected" => Some(RubyVisibility::Protected),
        _ => None,
    }
}

// Identifies the `attr_*` macro family on a Ruby `Call` node. Each
// macro takes a list of attribute symbols and synthesises the matching
// reader / writer / accessor methods on the enclosing class.
pub(crate) fn ruby_attr_macro_name(call: &Node, source: &[u8]) -> Option<&'static str> {
    let ident = call
        .children()
        .find(|c| matches!(c.kind_id().into(), Ruby::Identifier))?;
    match ident.utf8_text(source)? {
        "attr_accessor" => Some("attr_accessor"),
        "attr_reader" => Some("attr_reader"),
        "attr_writer" => Some("attr_writer"),
        _ => None,
    }
}

// Walks the direct children of a Ruby class / singleton-class body
// (`BodyStatement` under `Class` / `SingletonClass`) tallying:
// - class-scope assignments to `@var` (`InstanceVariable`) and
//   `@@var` (`ClassVariable`) — one attribute per assignment, regardless
//   of whether the RHS is a constant or another expression.
// - `attr_accessor` / `attr_reader` / `attr_writer` macros — one
//   attribute per symbol argument.
//
// Visibility flags follow Ruby's keyword-marker convention: a bare
// `private` / `public` / `protected` identifier flips the default for
// every subsequent declaration in the body. The default visibility at
// the top of every class body is `public`. The argument-form of those
// keywords (`private :foo`, `private def x`) does not flip the body-
// wide flag — matching Ruby's runtime behaviour.
//
// Attribute assignments to instance/class variables are visible only
// via the methods that wrap them, so the visibility flag at the point
// of declaration is what `npa` should reflect.
pub(crate) fn ruby_walk_class_body(body: &Node, source: &[u8], stats: &mut Stats) {
    use Ruby::*;

    let mut visibility = RubyVisibility::Public;
    for child in body.children() {
        if let Some(marker) = ruby_visibility_marker(&child, source) {
            visibility = marker;
            continue;
        }
        match child.kind_id().into() {
            Assignment | Assignment2 => {
                let Some(lhs) = child.children().next() else {
                    continue;
                };
                if matches!(lhs.kind_id().into(), InstanceVariable | ClassVariable) {
                    stats.class_na += 1;
                    if visibility == RubyVisibility::Public {
                        stats.class_npa += 1;
                    }
                }
            }
            Call | Call2 | Call3 | Call4 if ruby_attr_macro_name(&child, source).is_some() => {
                let count = ruby_attr_macro_symbol_count(&child);
                stats.class_na += count;
                if visibility == RubyVisibility::Public {
                    stats.class_npa += count;
                }
            }
            _ => {}
        }
    }
}

impl Npa for RubyCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        use Ruby::*;

        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        if !matches!(node.kind_id().into(), BodyStatement | BodyStatement2) {
            return;
        }
        let Some(parent_kind) = node.parent().map(|p| p.kind_id().into()) else {
            return;
        };
        if !matches!(parent_kind, Class | SingletonClass) {
            return;
        }
        ruby_walk_class_body(node, code, stats);
    }
}

impl Npa for PhpCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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

// Python attribute counting.
//
// Python has two flavours of class attributes:
// 1. Class-level (a.k.a. static): direct assignments inside the class
//    body — `class C: x = 1` or `class C: x: int = 1`.
// 2. Instance attributes: `self.x = …` assigned inside any method
//    body, conventionally inside `__init__`.
//
// Python has no visibility keyword. The PEP-8 convention `_x` for
// "internal" and `__x` for "name-mangled private" is purely advisory
// and not represented in the AST. `Npa::compute` is also called
// without access to the source bytes (only the `Node`), so reading
// the identifier text is not possible from this trait. We therefore
// treat every class attribute as public — `class_npa == class_na` —
// matching the Python ethos of "consenting adults". Documented as
// part of the trait contract for Python.
//
// Strategy: when the visitor hits a `ClassDefinition`, walk the body
// once and tally both class-level assignments and the `self.X = …`
// targets introduced by any method body. Counting on the
// `ClassDefinition` node (not its enclosed function spaces) keeps the
// attribution local to the surrounding class space, even though
// `self.X = …` lives inside a child `FunctionDefinition` space whose
// own `npa` stats are not class spaces.
impl Npa for PythonCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        use Python::*;

        // Gate on `ClassDefinition` specifically: `is_func_space` is
        // also true for `Module` / `FunctionDefinition`, which would
        // over-eagerly mark every space as a class space.
        if !matches!(node.kind_id().into(), ClassDefinition) {
            return;
        }

        // Mark the current space as a class space so the metric is
        // emitted (otherwise it is suppressed by `is_disabled`).
        if stats.is_disabled() {
            stats.is_class_space = true;
        }

        let Some(body) = python_class_body(node) else {
            return;
        };

        // Counts of distinct class attributes (class-level + self.*).
        // `self.x` may appear in several methods — and in different
        // branches of the same method — but per Fitzpatrick's intent
        // each *attribute* counts once. We deduplicate by the
        // attribute identifier text (read via the `code` bytes
        // widened into the trait by #219), so:
        //   class C:
        //       def __init__(self): self.value = None
        //       def reset(self):    self.value = None
        // counts `value` once, not twice. Closes #215.
        //
        // The class-level and instance passes share one `seen` set so a
        // class default and an instance write of the same name collapse:
        //   class C:
        //       x = 1                       # class default
        //       def __init__(self): self.x = 2   # instance shadows it
        // counts `x` once (#412 dedup). Typical Python classes declare
        // under a dozen attributes; `with_capacity(8)` covers the common
        // case without a rehash and costs negligibly when fewer.
        let mut seen: std::collections::HashSet<&[u8]> =
            std::collections::HashSet::with_capacity(8);
        python_collect_class_level_attrs(&body, code, &mut seen);
        python_collect_unique_self_attrs(&body, code, &mut seen);
        let total = seen.len();

        stats.class_na += total;
        // No visibility keyword in Python — every attribute is "public".
        stats.class_npa += total;
    }
}

// Rust attribute counting.
//
// Rust's "class" maps to a `struct` plus its `impl` blocks. Since each
// `impl` block opens its own func_space (`SpaceKind::Impl`), the
// natural place to record attributes per "class" is at the impl space
// and at the struct itself:
//
// 1. `StructItem`: every direct child in the struct's
//    `field_declaration_list` (named fields) or
//    `ordered_field_declaration_list` (tuple-struct positional fields)
//    is one attribute. Because `struct_item` is NOT a func_space, the
//    fields are attributed to whichever func_space is on the stack
//    when the StructItem is visited (typically `Unit`). The enclosing
//    space is marked as a class space so the npa metric is emitted.
//
// 2. `ImplItem`: every `ConstItem` and `StaticItem` direct child of the
//    impl's `declaration_list` is one associated attribute. These
//    accumulate on the Impl space (which is itself a class-style
//    func_space).
//
// 3. `TraitItem`: every `ConstItem`, `StaticItem`, and `AssociatedType`
//    direct child of the trait's `declaration_list` is one attribute.
//    Trait members are always visible to implementers, so they are
//    counted as public (`interface_npa == interface_na`), mirroring
//    Java's interface-body rule.
//
// Limitations (documented):
// - Multiple `impl Foo` blocks each open their own Impl space and
//   accumulate independently. Their `_sum` accumulators roll up to
//   the parent during finalisation, so the file-level
//   `class_npa_sum` is the sum across every impl.
// - Struct fields are attributed to the enclosing func_space (usually
//   Unit), not to a per-struct space. Two structs in the same module
//   therefore contribute to the same `class_na` bucket on that Unit.
//   This matches the issue's intent of "count struct fields + impl
//   associated consts" without inventing a synthetic per-struct
//   space.
// - Enum variants are NOT counted as attributes (they are sum-type
//   tags, not data fields), mirroring Kotlin's `enum_class_body`
//   treatment.
impl Npa for RustCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Rust::*;

        // Mark Impl / Trait spaces as class spaces so the metric is
        // emitted on them.
        if matches!(node.kind_id().into(), ImplItem | TraitItem) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            // Counted on the StructItem so each struct's fields are
            // tallied exactly once. The enclosing func_space (Unit or
            // nested) is the recipient — marking it a class space
            // makes the npa metric visible.
            StructItem => {
                let mut attrs = 0;
                let mut public_attrs = 0;
                for body in node.children() {
                    match body.kind_id().into() {
                        // Named-field struct: each `field_declaration`
                        // is one attribute. Visibility is the
                        // `visibility_modifier` first child.
                        FieldDeclarationList => {
                            for field in body
                                .children()
                                .filter(|c| matches!(c.kind_id().into(), FieldDeclaration))
                            {
                                attrs += 1;
                                if rust_item_is_public(&field) {
                                    public_attrs += 1;
                                }
                            }
                        }
                        // Tuple struct: the field count is positional.
                        // The grammar emits each field as either a
                        // type-bearing node (`primitive_type`,
                        // `type_identifier`, `generic_type`, ...) or a
                        // `visibility_modifier` followed by such a
                        // node. We count one attribute per non-token
                        // child that is not a delimiter, comma, or
                        // visibility modifier.
                        OrderedFieldDeclarationList => {
                            let (count, public) = rust_count_tuple_struct_fields(&body);
                            attrs += count;
                            public_attrs += public;
                        }
                        _ => {}
                    }
                }
                if attrs > 0 {
                    if stats.is_disabled() {
                        stats.is_class_space = true;
                    }
                    stats.class_na += attrs;
                    stats.class_npa += public_attrs;
                }
            }
            // Associated const/static declared in an `impl` block.
            // The current top-of-stack is the Impl space (because we
            // are inside its body), so attribution lands there.
            ConstItem | StaticItem => {
                let Some(parent) = node.parent() else {
                    return;
                };
                let Some(grand) = parent.parent() else {
                    return;
                };
                match grand.kind_id().into() {
                    ImplItem if matches!(parent.kind_id().into(), DeclarationList) => {
                        stats.class_na += 1;
                        if rust_item_is_public(node) {
                            stats.class_npa += 1;
                        }
                    }
                    TraitItem if matches!(parent.kind_id().into(), DeclarationList) => {
                        stats.interface_na += 1;
                        stats.interface_npa = stats.interface_na;
                    }
                    _ => {}
                }
            }
            // `type Foo;` inside a trait body is an associated type —
            // a placeholder bound that the implementer must supply.
            // Counted as an interface attribute, public by default.
            AssociatedType => {
                let Some(parent) = node.parent() else {
                    return;
                };
                let Some(grand) = parent.parent() else {
                    return;
                };
                if matches!(grand.kind_id().into(), TraitItem)
                    && matches!(parent.kind_id().into(), DeclarationList)
                {
                    stats.interface_na += 1;
                    stats.interface_npa = stats.interface_na;
                }
            }
            _ => {}
        }
    }
}

// Go attribute counting.
//
// Go has no `class` concept; struct types declared at file scope
// (`type Foo struct { … }`) play that role. Methods live separately
// as `MethodDeclaration` nodes attached to a receiver type. Because
// `StructType` is NOT a func_space (per `Checker::is_func_space`),
// the iterator visits it with the enclosing func_space's stats
// (typically the file-level `Unit`). Each direct `FieldDeclaration`
// child of the struct's `FieldDeclarationList` counts as one
// attribute, including embedded types (an embedded type parses as a
// `FieldDeclaration` with no name field, just a type — still one
// attribute per the issue spec).
//
// Visibility note: Go exports identifiers whose first character is
// uppercase. The `Npa::compute` trait signature does not include the
// source byte slice, so reading the identifier text from the node
// alone is not possible. We therefore treat every counted attribute
// as public (`class_npa == class_na`), matching the choice Python's
// Npm makes when no visibility token is present in the AST. The
// alternative — adding a `code: &[u8]` parameter to the trait — is a
// cross-language API change out of scope for this fix.
//
// Limitations:
// - Struct fields are attributed to the enclosing func_space (the
//   file's `Unit`, or a local function space for `type T struct{…}`
//   declared inside a function body). Multiple structs at the same
//   level contribute to the same `class_na` bucket. This mirrors the
//   Rust impl's "fields land on the enclosing space" approach.
// - Interface methods (`interface { Foo() }`) are not attributes —
//   they are method signatures, counted by Npm under
//   `interface_nm`, not by Npa.
impl Npa for GoCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Go as G;

        if !matches!(node.kind_id().into(), G::StructType) {
            return;
        }

        // The struct body is the `field_declaration_list` direct
        // child. An empty struct (`struct{}`) has the list with no
        // FieldDeclaration children → 0 attributes.
        let Some(body) = node
            .children()
            .find(|c| matches!(c.kind_id().into(), G::FieldDeclarationList))
        else {
            return;
        };

        let attrs = body
            .children()
            .filter(|c| matches!(c.kind_id().into(), G::FieldDeclaration))
            .count();

        if attrs == 0 {
            return;
        }

        if stats.is_disabled() {
            stats.is_class_space = true;
        }
        stats.class_na += attrs;
        // Visibility cannot be detected without the source bytes;
        // every field is treated as public (see module-level note).
        stats.class_npa += attrs;
    }
}

impl Npa for CppCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Cpp::*;

        // Mark class / struct spaces as class spaces so the metric is
        // emitted on them.
        if matches!(node.kind_id().into(), ClassSpecifier | StructSpecifier) && stats.is_disabled()
        {
            stats.is_class_space = true;
        }

        if !matches!(node.kind_id().into(), FieldDeclarationList) {
            return;
        }
        let Some(parent) = node.parent() else {
            return;
        };
        // C++ `class` defaults to private; `struct` defaults to public.
        let mut current_is_public = match parent.kind_id().into() {
            ClassSpecifier => false,
            StructSpecifier => true,
            _ => return,
        };

        for child in node.children() {
            match child.kind_id().into() {
                AccessSpecifier => {
                    // Update the current visibility to the access
                    // specifier's keyword. `protected` is bucketed with
                    // `private` for `npa` purposes (matches Java's
                    // "non-public" treatment), so any keyword other
                    // than `public` flips us back to private.
                    current_is_public = child
                        .first_child(|id| {
                            id == Cpp::Public || id == Cpp::Protected || id == Cpp::Private
                        })
                        .is_some_and(|tok| tok.kind_id() == Cpp::Public);
                }
                FieldDeclaration => {
                    // Member functions surface as `field_declaration`
                    // when declared without a body. They are counted
                    // by `Npm`, not as attributes — detect them by
                    // their `function_declarator` and skip.
                    if cpp_has_function_declarator(&child) {
                        continue;
                    }
                    // Data field — count every `field_identifier` in
                    // the declarator subtree. Pointer (`int* p`),
                    // array (`int a[N]`), and plain (`int x`) forms
                    // all reduce to one or more `field_identifier`
                    // leaves; the comma-separated form `int b, c`
                    // adds them as siblings.
                    let count = cpp_count_field_identifiers(&child);
                    stats.class_na += count;
                    if current_is_public {
                        stats.class_npa += count;
                    }
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn cpp_has_function_declarator(node: &Node) -> bool {
    use Cpp::*;
    node.children().any(|child| match child.kind_id().into() {
        FunctionDeclarator | FunctionDeclarator2 | FunctionDeclarator3 => true,
        // Recurse through declarator wrappers that can sit above the
        // function_declarator (`Foo* operator->()`,
        // `template<...> T fn();`, constructor / destructor
        // `declaration`s inside a class body).
        PointerDeclarator | PointerDeclarator2 | ReferenceDeclarator | ReferenceDeclarator2
        | ReferenceDeclarator3 | ReferenceDeclarator4 | Declaration | Declaration2
        | Declaration3 | Declaration4 => cpp_has_function_declarator(&child),
        _ => false,
    })
}

pub(crate) fn cpp_count_field_identifiers(node: &Node) -> usize {
    use Cpp::*;
    let mut count = 0;
    for child in node.children() {
        match child.kind_id().into() {
            FieldIdentifier => count += 1,
            PointerDeclarator | PointerDeclarator2 | ArrayDeclarator | ArrayDeclarator2
            | ArrayDeclarator3 | InitDeclarator | ReferenceDeclarator | ReferenceDeclarator2
            | ReferenceDeclarator3 | ReferenceDeclarator4 => {
                count += cpp_count_field_identifiers(&child);
            }
            _ => {}
        }
    }
    count
}

// Counts positional fields inside an `ordered_field_declaration_list`
// (tuple struct). Each non-token child that is a type node represents
// one field. A leading `visibility_modifier` may decorate the field;
// counts that field as public. Returns `(total_count, public_count)`.
fn rust_count_tuple_struct_fields(list: &Node) -> (usize, usize) {
    use Rust::*;

    let mut total = 0;
    let mut public = 0;
    let mut pending_pub = false;
    for child in list.children() {
        match child.kind_id().into() {
            // Open / close parens and comma separators — skipped.
            LPAREN | RPAREN | COMMA => {
                pending_pub = false;
            }
            // `pub` / `pub(crate)` / `pub(super)` / ... — applies to
            // the next type child.
            VisibilityModifier => {
                pending_pub = true;
            }
            // `attribute_item` decorates the next field but does not
            // contribute to visibility. Skip without resetting the
            // pending-pub flag. `line_comment` / `block_comment` may
            // sit between fields (e.g. `pub struct Foo(/* x */ i32);`)
            // and similarly must not count as a field.
            AttributeItem | LineComment | BlockComment => {}
            // Any other child is treated as a positional field type
            // (primitive_type, type_identifier, generic_type,
            // reference_type, tuple_type, ...). One increment per
            // type child.
            _ => {
                total += 1;
                if pending_pub {
                    public += 1;
                }
                pending_pub = false;
            }
        }
    }
    (total, public)
}

// Returns `true` if `pat` contains exactly one `UNDERSCORE` token
// (identified by `underscore_id`) and no other named children.
// Anonymous tokens such as a leading `|` in a Rust or-pattern
// (`| _ => ...`) are skipped — they do not change the semantic
// meaning of the pattern.
//
// Shared between languages whose `default:`-equivalent wildcard
// pattern is a single `_`:
//   - Rust `match_pattern` (`Cyclomatic` and `Abc` for `RustCode`)
//   - Python `case_pattern` (`Abc` for `PythonCode`)
//
// The Rust caller passes its grammar's `UNDERSCORE` kind id; Python
// passes its own. Guard handling is the caller's responsibility —
// in Rust the guard is a sibling inside `match_pattern` and so adds
// a named child here (this helper returns `false`); in Python the
// guard is an `if_clause` sibling on the enclosing `case_clause`,
// so the caller must check the surrounding node separately.
pub(crate) fn pattern_is_bare_underscore(pat: &Node, underscore_id: u16) -> bool {
    let mut found_underscore = false;
    for child in pat.children() {
        if child.kind_id() == underscore_id {
            if found_underscore {
                return false;
            }
            found_underscore = true;
        } else if child.is_named() {
            return false;
        }
        // else: anonymous non-`_` token (like `|`) — skip.
    }
    found_underscore
}

// Returns `true` iff a Python `case_clause` should count as a
// non-trivial decision: either the pattern is not a bare `_`, or
// the clause carries an `if`-guard (`case _ if g:`).
//
// Shared between the `Cyclomatic` and `Abc` implementations for
// `PythonCode`. The bare wildcard without a guard is Python's
// `default:`-equivalent and is filtered out, matching Rust's bare-`_`
// MatchArm rule and Java/C#'s `default:` rule.
//
// `underscore_id` is the grammar's `Python::UNDERSCORE` kind id,
// passed in so the helper does not assume a particular module-path
// to the language enum.
pub(crate) fn python_case_clause_counts(node: &Node, underscore_id: u16) -> bool {
    let mut bare_underscore = false;
    for child in node.children() {
        match child.kind_id().into() {
            Python::IfClause => return true,
            Python::CasePattern => {
                bare_underscore = pattern_is_bare_underscore(&child, underscore_id);
                if !bare_underscore {
                    return true;
                }
            }
            _ => {}
        }
    }
    !bare_underscore
}

// Returns true if `node`'s first child is a `visibility_modifier`
// containing the `pub` keyword. Matches Rust's "public-only-when-`pub`"
// model — `pub(crate)` / `pub(super)` / `pub(in path)` are also
// `visibility_modifier` and count as public for ABC purposes
// (`pub(crate)` is still "public to its crate"); only the absence of
// `pub` means private.
pub(crate) fn rust_item_is_public(node: &Node) -> bool {
    node.children()
        .any(|c| c.kind_id() == Rust::VisibilityModifier)
}

// Single normalization point for Python's aliased `block` kind_ids.
//
// tree-sitter-python lists two `kind_id`s that both stringify to
// `"block"`: `Block` (135, the hidden `_block` supertype) and
// `Block2` (160, the concrete production). Empirically only `Block2`
// is ever emitted for real block bodies (function, class, if/for,
// while/try/with), so `Block` is dead today — but a future grammar
// bump could promote the supertype to a concrete node. Routing every
// "is this a block?" check through here means such a bump is handled
// at one site instead of silently undercounting at several (issue
// #419; lesson 2 / 34 / 56 in docs/development/lessons_learned.md).
pub(crate) fn python_is_block(node: &Node) -> bool {
    matches!(node.kind_id().into(), Python::Block | Python::Block2)
}

// Returns the `block` body child of a `ClassDefinition` if present.
// `ClassDefinition` children are: `class` keyword, identifier,
// optional type-parameters, optional argument-list (base classes),
// `:`, block. The body is always the final child.
fn python_class_body<'a>(class_def: &Node<'a>) -> Option<Node<'a>> {
    class_def.children().find(python_is_block)
}

// Collects the names bound by class-level attribute assignments into
// `seen`. Walks direct `ExpressionStatement` children of the class
// body; for each contained `Assignment` carrying an `=` token
// (excluding bare type-only annotations like `x: int`, which parse as
// `Assignment` without an `=` — these declare a type but bind nothing),
// every bound *name* contributes one attribute. This counts each name,
// not each statement, so `a = b = 3` (chained) and `p, q = 1, 2`
// (unpacking) each contribute two attributes — mirroring Java's
// per-`VariableDeclarator` counting (#412 (c)). Names are deduplicated
// against the shared `seen` set so a class default `x = 1` and an
// instance `self.x = 2` count `x` once (instance shadows the class
// default — #412 dedup).
fn python_collect_class_level_attrs<'a>(
    body: &Node<'a>,
    code: &'a [u8],
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    use Python::*;

    for stmt in body.children() {
        if stmt.kind_id() != ExpressionStatement {
            continue;
        }
        for child in stmt.children() {
            if child.kind_id() == Assignment && child.first_child(|id| id == EQ).is_some() {
                python_collect_bound_names_from_target(&child, code, seen);
            }
        }
    }
}

// Collects the simple-identifier names bound by a class-level
// `Assignment` target into `seen`, following chained `=` assignments
// and unpacking targets:
//   x = 1            → {x}
//   a = b = 3        → {a, b}   (nested Assignment in the value)
//   p, q = 1, 2      → {p, q}   (pattern_list / list_pattern target)
// Only simple-name bindings contribute here; an attribute target
// (`obj.x = …`) at class level is not a simple name binding and is
// ignored (the self/cls instance-attribute pass handles those).
// Walks an assignment / destructuring `target`, invoking `collect` on
// every leaf binding element. Recurses through nested unpacking patterns
// (`pattern_list` / `expression_list` / `tuple_pattern` / `list_pattern`)
// so `(a, (b, c)) = …` and `self.a, (self.b, self.c) = …` yield every
// bound element, not just the top-level ones. Non-pattern nodes —
// including punctuation children (commas, brackets) — are handed to
// `collect`, which filters by kind, exactly as the previous flat loop did.
fn python_walk_target_elements<'a>(target: &Node<'a>, collect: &mut impl FnMut(&Node<'a>)) {
    match target.kind_id().into() {
        // `tuple_pattern` / `list_pattern` each carry two aliased kind_ids:
        // the hidden supertype (`TuplePattern` 168 / `ListPattern` 167,
        // never emitted) and the live node the grammar actually produces
        // for `(a, b) = …` / `[a, b] = …` (`TuplePattern2` 179 /
        // `ListPattern2` 180). Matching only the supertype alias silently
        // dropped every parenthesized/bracketed unpacking target, so
        // enumerate both aliases per the hidden-alias discipline (#419).
        Python::PatternList
        | Python::ExpressionList
        | Python::TuplePattern
        | Python::TuplePattern2
        | Python::ListPattern
        | Python::ListPattern2 => {
            for element in target.children() {
                python_walk_target_elements(&element, collect);
            }
        }
        _ => collect(target),
    }
}

fn python_collect_bound_names_from_target<'a>(
    assignment: &Node<'a>,
    code: &'a [u8],
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    let Some(target) = assignment.child(0) else {
        return;
    };
    // Every simple-name binding contributes one attribute, including names
    // nested inside an unpacking pattern. An attribute target (`obj.x = …`)
    // at class level is not a simple name binding and is ignored (the
    // self/cls instance-attribute pass handles those).
    python_walk_target_elements(&target, &mut |element| {
        if element.kind_id() == Python::Identifier
            && let Some(name) = code.get(element.start_byte()..element.end_byte())
        {
            seen.insert(name);
        }
    });
    // Chained `a = b = 3`: the right operand is itself a nested
    // `Assignment`, whose own target binds another name. Recurse so
    // every link in the chain is counted.
    if let Some(value) = assignment.child(assignment.child_count().saturating_sub(1))
        && value.kind_id() == Python::Assignment
    {
        python_collect_bound_names_from_target(&value, code, seen);
    }
}

// Collects the unique `self.<attr>` / `cls.<attr>` instance-attribute
// names bound anywhere in the class's method bodies into `seen`. Walks
// every method body once. Deduplicating by identifier text fixes #215:
// re-binding `self.x` across methods or branches no longer inflates the
// count. Sharing the `seen` set with the class-level pass also dedups
// across the two (#412 dedup).
fn python_collect_unique_self_attrs<'a>(
    body: &Node<'a>,
    code: &'a [u8],
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    for stmt in body.children() {
        if let Some(func) = python_unwrap_function(&stmt) {
            python_collect_self_attrs_in_subtree(&func, code, seen);
        }
    }
}

fn python_collect_self_attrs_in_subtree<'a>(
    root: &Node<'a>,
    code: &'a [u8],
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    use Python::*;

    let mut stack: Vec<Node<'a>> = Vec::with_capacity(32);
    for child in root.children() {
        stack.push(child);
    }
    while let Some(node) = stack.pop() {
        // Boundary: do not descend into nested classes, functions, or
        // lambdas. Their attributes belong to their inner scope.
        if matches!(
            node.kind_id().into(),
            FunctionDefinition | ClassDefinition | DecoratedDefinition | Lambda
        ) {
            continue;
        }

        if node.kind_id() == Assignment {
            python_collect_self_attrs_from_target(&node, code, seen);
        }

        for child in node.children() {
            stack.push(child);
        }
    }
}

// Collects `self.<attr>` / `cls.<attr>` names bound by an `Assignment`'s
// target into `seen`. Handles the single-attribute shape (`self.a = 1`),
// flat unpacking (`self.a, self.b = …`, #412 (b)), and nested unpacking
// (`self.a, (self.b, self.c) = …`) uniformly via the shared
// `python_walk_target_elements` recursion. A chained
// `self.a = self.b = 1` is handled by the caller's subtree walk: the
// nested `Assignment` in the value is visited as its own `Assignment`.
fn python_collect_self_attrs_from_target<'a>(
    assignment: &Node<'a>,
    code: &'a [u8],
    seen: &mut std::collections::HashSet<&'a [u8]>,
) {
    let Some(target) = assignment.child(0) else {
        return;
    };
    // Collect only the `self`/`cls` attribute elements; unpacking may mix
    // them with foreign targets (`self.a, x = …`), which are filtered out
    // here because they are not `self`/`cls` attributes.
    python_walk_target_elements(&target, &mut |element| {
        if element.kind_id() == Python::Attribute
            && let Some(name) = python_self_attr_name_bytes(element, code)
        {
            seen.insert(name);
        }
    });
}

// Conventional receiver names that denote the enclosing object inside
// a method body: `self` for instance methods, `cls` for classmethods.
// We match the receiver bytes against these literals rather than
// resolving the enclosing function's first parameter. The pragmatic
// choice (per #412): reading source bytes and matching `self`/`cls` is
// clearly better than the prior structural-only proxy (which counted
// ANY `obj.x = …` as an instance attribute) and covers the
// overwhelming majority of real code. A non-conventionally-named first
// parameter is rare enough that under-counting it is preferable to the
// over-count of treating every foreign-object write as an attribute.
const PYTHON_SELF_RECEIVERS: [&[u8]; 2] = [b"self", b"cls"];

// Returns the byte slice for the attribute identifier of a
// `self.<attr>` / `cls.<attr>` Attribute node, or `None` when the
// receiver is not a self/cls alias.
//
// `attr` is an `Attribute` node. Its first child is the receiver:
//   self.x       → receiver is Identifier "self"      → counts
//   db.x         → receiver is Identifier "db"        → foreign, skip
//   self.f.g     → receiver is itself an Attribute    → skip
// The last named child is the attribute identifier (the `.` and the
// preceding receiver are siblings; the identifier comes last). Only a
// direct `self.<name> = …` introduces an attribute of the class;
// `self.f.g = …` writes attribute `g` on `self.f`, not a new attribute
// of the class, so the nested-Attribute receiver is intentionally
// rejected. Borrows directly from `code` so the returned slice is the
// canonical dedup key — two `self.value` writes share the same key.
fn python_self_attr_name_bytes<'a>(attr: &Node<'a>, code: &'a [u8]) -> Option<&'a [u8]> {
    // Fully-qualified `Python::*` names — this function deliberately
    // does NOT `use Python::*;` so unqualified `None` keeps its
    // `Option` meaning rather than being shadowed by `Python::None`.
    let receiver = attr.child(0)?;
    if receiver.kind_id() != Python::Identifier {
        return None;
    }
    let receiver_bytes = code.get(receiver.start_byte()..receiver.end_byte())?;
    if !PYTHON_SELF_RECEIVERS.contains(&receiver_bytes) {
        return None;
    }
    // The trailing identifier is the last `Identifier` child of the
    // Attribute node; `.last()` walks the children once and yields it.
    let id = attr
        .children()
        .filter(|c| c.kind_id() == Python::Identifier)
        .last()?;
    // Guard the degenerate single-identifier case: only count when the
    // attribute name is a distinct Identifier from the receiver.
    if id.start_byte() == receiver.start_byte() {
        return None;
    }
    code.get(id.start_byte()..id.end_byte())
}

fn python_unwrap_function<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    // Use fully-qualified names here: `use Python::*` would shadow
    // `Option::None` with `Python::None` and break the last arm.
    match node.kind_id().into() {
        Python::FunctionDefinition => Some(*node),
        Python::DecoratedDefinition => node
            .children()
            .find(|c| c.kind_id() == Python::FunctionDefinition),
        _ => None,
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
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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
            ClassParameter
                if node
                    .children()
                    .any(|c| matches!(c.kind_id().into(), Val | Var)) =>
            {
                stats.class_na += 1;
                if kotlin_is_public(node) {
                    stats.class_npa += 1;
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
        fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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
// property-identifier text. The `Npa::compute` trait signature
// does not carry source bytes, so prototype-shaped attributes are
// intentionally not counted by this impl. Modern ES2015+ class
// syntax — the dominant style — is unaffected; legacy prototype-
// only files under-report. A follow-up that widens the trait
// signature to `(node, code, stats)` would unlock prototype
// detection (see `Abc::compute` for the existing pattern).

macro_rules! js_npa_compute {
    ($lang:ident) => {
        fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
            use $lang::*;

            if Self::is_func_space(node) && stats.is_disabled() {
                stats.is_class_space = true;
            }

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

impl Npa for JavascriptCode {
    js_npa_compute!(Javascript);
}

impl Npa for MozjsCode {
    js_npa_compute!(Mozjs);
}

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
    PreprocCode,
    CcommentCode,
    PerlCode,
    BashCode,
    LuaCode,
    TclCode
);

// Elixir Npa (#275). `defmodule` is treated as a class via source-aware
// Checker dispatch; `defstruct` is its closest analog to a field-set
// declaration. When entering a `defmodule` Class space we look for a
// direct-child `defstruct` Call in the `do_block` and count its
// field arguments. Three syntactic forms are accepted, matching the
// Elixir docs (https://hexdocs.pm/elixir/Kernel.html#defstruct/1):
//
// - `defstruct [:a, :b]` — bracketed list of atoms.
// - `defstruct a: 1, b: 2` — bare keyword list (the most common form).
// - `defstruct [a: 1, b: 2]` — bracketed keyword list.
//
// All fields are counted as public (`class_npa`); Elixir struct fields
// have no Java-style visibility modifier and the runtime exposes every
// field via pattern matching and `Map.get/2`.
impl Npa for ElixirCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        use crate::metrics::cognitive::{elixir_call_keyword, elixir_do_block_call_children};

        if !stats.is_disabled() || !Self::is_func_space_with_code(node, code) {
            return;
        }
        if !matches!(elixir_call_keyword(node, code), Some("defmodule")) {
            return;
        }

        stats.is_class_space = true;

        for stmt in elixir_do_block_call_children(node) {
            if matches!(elixir_call_keyword(&stmt, code), Some("defstruct")) {
                let fields = count_defstruct_fields(&stmt);
                stats.class_na += fields;
                stats.class_npa += fields;
            }
        }
    }
}

// Counts the field entries of an Elixir `defstruct` Call's arguments.
// `defstruct` accepts three syntactic forms:
//   * `defstruct [:a, :b]` — a `List` of atoms.
//   * `defstruct a: 1, b: 2` — a bare `Keywords` keyword list, which
//     in the tree-sitter-elixir grammar appears directly inside
//     `Arguments` without an extra wrapper.
//   * `defstruct [a: 1, b: 2]` — a `List` wrapping a `Keywords`.
// We descend through the `Arguments` / `List` / `Keywords` wrapper
// nodes (skipping the leading `target` Identifier that names the
// macro itself) and tally `Atom` leaves (bare-list form) and `Pair`s
// (keyword form). `defstruct nil` and an empty `defstruct` correctly
// return 0.
fn count_defstruct_fields(call: &Node) -> usize {
    use Elixir as E;

    // `Arguments` is the wrapper around the macro's positional
    // arguments. `List` is the bracketed form. Keyword pairs without
    // brackets appear directly inside `Arguments` (no `Keywords`
    // wrapper) in the tree-sitter-elixir grammar. The leading
    // `target` Identifier is never one of these kinds, so no
    // explicit target-skip filter is needed.
    call.children()
        .filter(|child| matches!(child.kind_id().into(), E::Arguments | E::List | E::Keywords))
        .map(|child| count_field_entries(&child))
        .sum()
}

fn count_field_entries(node: &Node) -> usize {
    use Elixir as E;

    node.children()
        .map(|child| match child.kind_id().into() {
            // Bare-list form (`defstruct [:a, :b]`): each atom is a
            // field. Keyword form (`defstruct a: 1, b: 2`): each
            // `Pair` is a field.
            E::Atom | E::QuotedAtom | E::Atom2 | E::Pair => 1,
            // A `List` or `Keywords` may wrap the entries one level
            // deeper (`defstruct [a: 1, b: 2]` puts a `List` inside
            // `Arguments`, which then contains a `Keywords`).
            E::List | E::Keywords => count_field_entries(&child),
            _ => 0,
        })
        .sum()
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
    use crate::tools::{assert_child_space_kind, check_func_space, check_metrics};

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
    fn groovy_no_attributes() {
        check_metrics::<GroovyParser>("class A { void foo() {} }", "foo.groovy", |metric| {
            assert_eq!(metric.npa.total_na(), 0.0);
            assert_eq!(metric.npa.total_npa(), 0.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
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
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
                assert_eq!(metric.npa.interface_npa_sum(), 2.0);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
            },
        );
    }

    #[test]
    fn groovy_no_attributes_in_unit_scope() {
        check_metrics::<GroovyParser>("int x = 1", "foo.groovy", |metric| {
            assert_eq!(metric.npa.total_na(), 0.0);
        });
    }

    #[test]
    fn groovy_multiple_classes() {
        check_metrics::<GroovyParser>(
            "class A { public int a }
            class B { public int b }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 5.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 6.0);
                assert_eq!(metric.npa.class_npa_sum(), 4.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(func_space.metrics.npa.interface_na_sum(), 2.0);
                assert_eq!(func_space.metrics.npa.interface_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(func_space.metrics.npa.interface_na_sum(), 2.0);
                assert_eq!(func_space.metrics.npa.interface_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 0.0);
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npa);
                assert_child_space_kind(&func_space, "I", SpaceKind::Interface);
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
                assert_eq!(metric.npa.interface_npa_sum(), 2.0);
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 0.0);
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
                assert_eq!(metric.npa.interface_npa_sum(), 2.0);
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 0.0);
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
        check_func_space::<TypescriptParser, _>(
            "class A { x: number = 0; }
             class B { private y: number = 0; }
             interface I { z: number; }",
            "foo.ts",
            |func_space| {
                let metric = &func_space.metrics;
                // A: 1 npa / 1 na (public). B: 0 npa / 1 na (private).
                // I: 1 interface_npa / 1 interface_na.
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.interface_npa_sum(), 1.0);
                assert_eq!(metric.npa.interface_na_sum(), 1.0);
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
        check_func_space::<TsxParser, _>(
            "interface I {
                a: number;
                b: string;
                m(): void;
            }",
            "foo.tsx",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npa.interface_npa_sum(), 2.0);
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
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
        check_func_space::<TsxParser, _>(
            "class A { x: number = 0; }
             class B { private y: number = 0; }
             interface I { z: number; }",
            "foo.tsx",
            |func_space| {
                let metric = &func_space.metrics;
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.interface_npa_sum(), 1.0);
                assert_eq!(metric.npa.interface_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
                assert_eq!(metric.npa.class_na_sum(), 0.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn ruby_instance_variable_attribute() {
        // Bare `@x = …` at class scope is one public attribute.
        check_metrics::<RubyParser>("class A\n  @x = 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 1.0);
            assert_eq!(metric.npa.class_na_sum(), 1.0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn ruby_class_variable_attribute() {
        // `@@y = …` at class scope is one attribute.
        check_metrics::<RubyParser>("class A\n  @@y = 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 1.0);
            assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 4.0);
                assert_eq!(metric.npa.class_na_sum(), 4.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
                assert_eq!(metric.npa.class_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
                assert_eq!(metric.npa.class_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
            assert_eq!(metric.npa.interface_na_sum(), 0.0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn python_class_level_assignments_are_attributes() {
        // Two class-level `=` assignments → 2 attributes, all public
        // (Python has no visibility keyword).
        check_metrics::<PythonParser>("class C:\n    x = 1\n    y = 2\n", "foo.py", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 2.0);
            assert_eq!(metric.npa.class_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 0.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn python_module_level_assignments_not_attributes() {
        // `x = 1` at module scope is not a class attribute.
        check_metrics::<PythonParser>("x = 1\ny = 2\nclass C:\n    a = 3\n", "foo.py", |metric| {
            // Only `a = 3` lives in the class space.
            assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 5.0);
                assert_eq!(metric.npa.class_npa_sum(), 5.0);
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
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn rust_empty_unit_no_attributes() {
        check_metrics::<RustParser>("", "empty.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
            assert_eq!(metric.npa.interface_na_sum(), 0.0);
            assert_eq!(metric.npa.interface_npa_sum(), 0.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn rust_tuple_struct_fields_are_attributes() {
        // Tuple-struct field counting is positional. `Bar(pub i32,
        // String)` → 2 fields, 1 public.
        check_metrics::<RustParser>("struct Bar(pub i32, String);", "foo.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 2.0);
            assert_eq!(metric.npa.class_npa_sum(), 1.0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn rust_unit_struct_has_no_attributes() {
        // `struct Empty;` is a unit struct (no fields). 0 attributes.
        check_metrics::<RustParser>("struct Empty;", "foo.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn rust_empty_struct_body_has_no_attributes() {
        // `struct Empty {}` is named-field with zero fields.
        check_metrics::<RustParser>("struct Empty { }", "foo.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_na_sum(), 4.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
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
                assert_eq!(metric.npa.interface_na_sum(), 2.0);
                assert_eq!(metric.npa.interface_npa_sum(), 2.0);
                assert_eq!(metric.npa.class_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 0.0);
                assert_eq!(metric.npa.interface_na_sum(), 0.0);
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
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn go_empty_struct_has_no_attributes() {
        // `type Empty struct{}` has an empty FieldDeclarationList →
        // 0 fields → npa stays disabled.
        check_metrics::<GoParser>("package main\ntype Empty struct{}\n", "foo.go", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn go_struct_fields_are_attributes() {
        // Three named fields: `X int`, `y string`, `Z float64` → 3
        // attributes. Visibility is by identifier case in Go, but the
        // trait signature does not give us source bytes, so every
        // field is counted as public: class_npa == class_na.
        check_metrics::<GoParser>(
            "package main\ntype Foo struct { X int; y string; Z float64 }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn go_grouped_struct_fields_each_count() {
        // `X, Y int` parses as ONE field_declaration with two name
        // identifiers — counted as 1 attribute per the
        // "FieldDeclaration is the unit" rule. The trailing `Z` is a
        // separate field_declaration → 2 attributes total. This
        // mirrors Rust's per-FieldDeclaration counting.
        check_metrics::<GoParser>(
            "package main\ntype Point struct { X, Y int; Z float64 }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn go_embedded_type_counts_as_attribute() {
        // `io.Reader` and `*Foo` are embedded types — field
        // declarations with no name, just a type. Each is one
        // attribute per the issue spec ("Embedded types: a field
        // with no name, just a type — count as one field"). Plus
        // `n int` → 3 attributes total.
        check_metrics::<GoParser>(
            "package main\nimport \"io\"\ntype Bar struct { io.Reader; *Foo; n int }\ntype Foo struct {}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn go_multiple_structs_aggregate_at_unit() {
        // Two structs declared at file scope each contribute their
        // fields to the same Unit space (no per-receiver class
        // grouping in Go). `Foo` has 1 field, `Bar` has 2 → total
        // class_na_sum = 3.
        check_metrics::<GoParser>(
            "package main\ntype Foo struct { x int }\ntype Bar struct { a int; b string }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 0.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
            },
        );
    }

    #[test]
    fn elixir_npa_defstruct_atom_list() {
        check_metrics::<ElixirParser>(
            "defmodule User do\n  defstruct [:name, :age, :email]\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
            },
        );
    }

    #[test]
    fn elixir_npa_defstruct_bracketed_keyword_list() {
        check_metrics::<ElixirParser>(
            "defmodule User do\n  defstruct [name: nil, age: 0]\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2.0);
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
            },
        );
    }

    #[test]
    fn elixir_npa_defstruct_single_field() {
        check_metrics::<ElixirParser>(
            "defmodule Box do\n  defstruct value: nil\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
            },
        );
    }

    #[test]
    fn elixir_npa_no_defstruct_is_zero() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def m, do: :ok\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 0.0);
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
            },
        );
    }

    // ----- C++ -----

    #[test]
    fn cpp_empty_unit_no_attributes() {
        // No code → no class spaces → npa = 0. Establishes the trait
        // is wired and the per-language compute is reachable.
        check_metrics::<CppParser>("", "empty.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn cpp_empty_class_no_attributes() {
        // `class Foo {};` has no fields. Marked as class space (npa
        // becomes visible) but counts stay at 0.
        check_metrics::<CppParser>("class Foo {};", "foo.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
            assert_eq!(metric.npa.class_na_sum(), 1.0);
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn cpp_struct_default_public_visibility() {
        // `struct` defaults to public — opposite of `class`. The same
        // field counts once and is public.
        check_metrics::<CppParser>("struct Bar { int value_; };", "foo.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 1.0);
            assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                assert_eq!(metric.npa.class_npa_sum(), 0.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                // Struct → all public.
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                // Public: Foo::a (1) + Bar::c (1) = 2.
                assert_eq!(metric.npa.class_npa_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[test]
    fn javascript_empty_unit_no_attributes() {
        // Wires up the trait and ensures no spurious attribute counts
        // on an empty file.
        check_metrics::<JavascriptParser>("", "empty.js", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[test]
    fn javascript_empty_class_no_attributes() {
        // A class with no body and no fields has zero attributes.
        check_metrics::<JavascriptParser>("class Foo {}", "foo.js", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0.0);
            assert_eq!(metric.npa.class_npa_sum(), 0.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 1.0);
                assert_eq!(metric.npa.class_npa_sum(), 1.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
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
                assert_eq!(metric.npa.class_na_sum(), 3.0);
                assert_eq!(metric.npa.class_npa_sum(), 3.0);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }
}
