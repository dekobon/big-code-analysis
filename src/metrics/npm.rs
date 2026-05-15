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
use crate::macros::implement_metric_trait;
use crate::metrics::npa::ts_member_is_public;
use crate::node::Node;
use crate::*;

/// The `Npm` metric.
///
/// This metric counts the number of public methods
/// of classes/interfaces.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    class_npm: usize,
    interface_npm: usize,
    class_nm: usize,
    interface_nm: usize,
    class_npm_sum: usize,
    interface_npm_sum: usize,
    class_nm_sum: usize,
    interface_nm_sum: usize,
    is_class_space: bool,
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("npm", 9)?;
        st.serialize_field("classes", &self.class_npm_sum())?;
        st.serialize_field("interfaces", &self.interface_npm_sum())?;
        st.serialize_field("class_methods", &self.class_nm_sum())?;
        st.serialize_field("interface_methods", &self.interface_nm_sum())?;
        st.serialize_field("classes_average", &self.class_coa())?;
        st.serialize_field("interfaces_average", &self.interface_coa())?;
        st.serialize_field("total", &self.total_npm())?;
        st.serialize_field("total_methods", &self.total_nm())?;
        st.serialize_field("average", &self.total_coa())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "classes: {}, interfaces: {}, class_methods: {}, interface_methods: {}, classes_average: {}, interfaces_average: {}, total: {}, total_methods: {}, average: {}",
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
    pub fn class_npm(&self) -> f64 {
        self.class_npm as f64
    }

    /// Returns the number of interface public methods in a space.
    #[inline]
    #[must_use]
    pub fn interface_npm(&self) -> f64 {
        self.interface_npm as f64
    }

    /// Returns the number of class methods in a space.
    #[inline]
    #[must_use]
    pub fn class_nm(&self) -> f64 {
        self.class_nm as f64
    }

    /// Returns the number of interface methods in a space.
    #[inline]
    #[must_use]
    pub fn interface_nm(&self) -> f64 {
        self.interface_nm as f64
    }

    /// Returns the number of class public methods sum in a space.
    #[inline]
    #[must_use]
    pub fn class_npm_sum(&self) -> f64 {
        self.class_npm_sum as f64
    }

    /// Returns the number of interface public methods sum in a space.
    #[inline]
    #[must_use]
    pub fn interface_npm_sum(&self) -> f64 {
        self.interface_npm_sum as f64
    }

    /// Returns the number of class methods sum in a space.
    #[inline]
    #[must_use]
    pub fn class_nm_sum(&self) -> f64 {
        self.class_nm_sum as f64
    }

    /// Returns the number of interface methods sum in a space.
    #[inline]
    #[must_use]
    pub fn interface_nm_sum(&self) -> f64 {
        self.interface_nm_sum as f64
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
        self.class_npm_sum() / self.class_nm_sum()
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
        // For the Java language it's not necessary to compute the metric value
        // The metric value in Java can only be 1.0 or f64:NAN
        if self.interface_npm_sum == self.interface_nm_sum && self.interface_npm_sum != 0 {
            1.0
        } else {
            self.interface_npm_sum() / self.interface_nm_sum()
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
        self.total_npm() / self.total_nm()
    }

    /// Returns the total number of public methods in a space.
    #[inline]
    #[must_use]
    pub fn total_npm(&self) -> f64 {
        self.class_npm_sum() + self.interface_npm_sum()
    }

    /// Returns the total number of methods in a space.
    #[inline]
    #[must_use]
    pub fn total_nm(&self) -> f64 {
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

    // Checks if the `Npm` metric is disabled
    #[inline]
    pub(crate) fn is_disabled(&self) -> bool {
        !self.is_class_space
    }
}

/// Per-language counting of public methods.
pub trait Npm
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

impl Npm for JavaCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Java::*;

        // Enables the `Npm` metric if computing stats of a class space
        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            ClassBody => {
                stats.class_nm += node
                    .children()
                    .filter(|node| Self::is_func(node))
                    .map(|method| {
                        // The first child node contains the list of method modifiers
                        // There are several modifiers that may be part of a method declaration
                        // Source: https://docs.oracle.com/javase/tutorial/reflect/member/methodModifiers.html
                        if let Some(modifiers) = method.child(0) {
                            // Looks for the `public` keyword in the list of method modifiers
                            if matches!(modifiers.kind_id().into(), Modifiers)
                                && modifiers.first_child(|id| id == Public).is_some()
                            {
                                stats.class_npm += 1;
                            }
                        }
                    })
                    .count();
            }
            // All methods in an interface are implicitly public
            // Source: https://docs.oracle.com/javase/tutorial/java/IandI/interfaceDef.html
            InterfaceBody => {
                // Children nodes are filtered because Java interfaces
                // can contain methods but also constants and nested types
                // Source: https://docs.oracle.com/javase/tutorial/java/IandI/createinterface.html
                stats.interface_nm += node.children().filter(|node| Self::is_func(node)).count();
                stats.interface_npm = stats.interface_nm;
            }
            _ => {}
        }
    }
}

// Count direct method-like declarations and property / indexer
// accessors (each get/set/init is a method per C# IL semantics).
// Expression-bodied properties (`int W => _w;`) have no AccessorList
// but do define a getter — `.max(1)` keeps them at 1 method.
fn csharp_count_member(member: &Node) -> usize {
    use Csharp::*;
    match member.kind_id().into() {
        MethodDeclaration
        | ConstructorDeclaration
        | DestructorDeclaration
        | OperatorDeclaration
        | ConversionOperatorDeclaration => 1,
        PropertyDeclaration | IndexerDeclaration => member
            .children()
            .filter(|c| matches!(c.kind_id().into(), AccessorList))
            .flat_map(|c| c.children())
            .filter(|c| matches!(c.kind_id().into(), AccessorDeclaration))
            .count()
            .max(1),
        _ => 0,
    }
}

impl Npm for CsharpCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Csharp::*;

        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        if !matches!(node.kind_id().into(), DeclarationList) {
            return;
        }
        let Some(parent_kind) = node.parent().map(|p| p.kind_id().into()) else {
            return;
        };

        match parent_kind {
            ClassDeclaration | StructDeclaration | RecordDeclaration => {
                for member in node.children() {
                    let count = csharp_count_member(&member);
                    stats.class_nm += count;
                    if super::npa::csharp_is_explicit_public(&member) {
                        stats.class_npm += count;
                    }
                }
            }
            // Interface members default to public (matching Java's rule);
            // skip the visibility scan entirely.
            InterfaceDeclaration => {
                for member in node.children() {
                    stats.interface_nm += csharp_count_member(&member);
                }
                stats.interface_npm = stats.interface_nm;
            }
            _ => {}
        }
    }
}

impl Npm for PhpCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Php::*;

        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            DeclarationList => {
                let Some(parent_kind) = node.parent().map(|p| p.kind_id().into()) else {
                    return;
                };
                match parent_kind {
                    ClassDeclaration | TraitDeclaration | AnonymousClass => {
                        for method in node.children().filter(|c| Self::is_func(c)) {
                            stats.class_nm += 1;
                            if super::npa::php_is_explicit_public(&method) {
                                stats.class_npm += 1;
                            }
                        }
                    }
                    // Interface methods are implicitly public.
                    InterfaceDeclaration => {
                        let count = node.children().filter(|c| Self::is_func(c)).count();
                        stats.interface_nm += count;
                        stats.interface_npm = stats.interface_nm;
                    }
                    _ => {}
                }
            }
            // PHP 8.1 enums can declare regular and static methods.
            EnumDeclarationList => {
                for method in node.children().filter(|c| Self::is_func(c)) {
                    stats.class_nm += 1;
                    if super::npa::php_is_explicit_public(&method) {
                        stats.class_npm += 1;
                    }
                }
            }
            _ => {}
        }
    }
}

// Python method counting.
//
// A "method" is a `FunctionDefinition` direct child of a class body
// (the `Block2` under a `ClassDefinition`), including decorated
// methods such as `@property`, `@staticmethod`, `@classmethod` and
// user decorators — those wrap the inner function in a
// `DecoratedDefinition` node, so we unwrap and count once.
//
// Python has no visibility keyword. The PEP-8 convention `_x` for
// "internal" and `__x` for "name-mangled private" is purely advisory
// and not represented in the AST. `Npm::compute` is also called
// without source bytes, so reading the identifier text is not
// possible from this trait. We therefore treat every class method as
// public — `class_npm == class_nm`.
//
// Nested classes and async functions are handled naturally:
// `async def m(self):` still parses as `FunctionDefinition`, so the
// `is_func` check covers it without special-casing. Nested
// `ClassDefinition` children of a class body are skipped here — they
// open their own class space, where their methods will be counted.
impl Npm for PythonCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Python::*;

        // Gate on `ClassDefinition` specifically so the flag is not
        // set on plain function or module spaces.
        if !matches!(node.kind_id().into(), ClassDefinition) {
            return;
        }

        if stats.is_disabled() {
            stats.is_class_space = true;
        }

        let Some(body) = node.children().find(|c| c.kind_id() == Block2) else {
            return;
        };

        // Count direct-child method declarations. Decorated methods
        // appear under a `DecoratedDefinition` wrapper; walk into
        // that wrapper to find the inner `FunctionDefinition`.
        let count = body
            .children()
            .filter(|stmt| match stmt.kind_id().into() {
                FunctionDefinition => true,
                DecoratedDefinition => stmt.children().any(|c| c.kind_id() == FunctionDefinition),
                _ => false,
            })
            .count();

        stats.class_nm += count;
        // No visibility modifier in Python — every method is "public".
        stats.class_npm += count;
    }
}

// Rust method counting.
//
// A "method" in Rust is a `function_item` direct child of an `impl`
// block's `declaration_list`. In a `trait_item`, both `function_item`
// (default-body methods) and `function_signature_item` (signature-only
// methods that implementers must provide) count toward the trait's
// interface methods. Trait methods are always visible to implementers
// and are therefore counted as public, matching Java's interface rule
// (`interface_npm == interface_nm`).
//
// `pub` / `pub(crate)` / `pub(super)` / `pub(in ...)` mark an impl
// method as public; absence of any visibility modifier means private.
// The `pub(crate)` form is intentionally counted as public because
// it's externally callable from the crate's perspective — narrower
// distinctions are tracked by `npa` / `npm` only as a binary public /
// private flag.
impl Npm for RustCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Rust::*;

        // Mark Impl / Trait spaces as class spaces so npm emits.
        if matches!(node.kind_id().into(), ImplItem | TraitItem) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        // A method is a `function_item` or `function_signature_item`
        // whose parent is the `declaration_list` of an `impl` or
        // `trait`. Gating on the kind, parent, and grandparent keeps
        // free-standing functions out of the count without needing to
        // walk the parent list eagerly.
        if !matches!(node.kind_id().into(), FunctionItem | FunctionSignatureItem) {
            return;
        }
        let Some(parent) = node.parent() else {
            return;
        };
        if !matches!(parent.kind_id().into(), DeclarationList) {
            return;
        }
        let Some(grand) = parent.parent() else {
            return;
        };
        match grand.kind_id().into() {
            ImplItem => {
                stats.class_nm += 1;
                if super::npa::rust_item_is_public(node) {
                    stats.class_npm += 1;
                }
            }
            TraitItem => {
                stats.interface_nm += 1;
                stats.interface_npm = stats.interface_nm;
            }
            _ => {}
        }
    }
}

// Re-uses the visibility helper from the `Npa` impl. Kotlin's default
// visibility is `public`, the opposite of Java's

impl Npm for GoCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Go as G;

        match node.kind_id().into() {
            // First-visit pass on the file root: enable npm output if
            // the file declares any receiver methods. Walks only the
            // direct children — Go always declares methods at file
            // scope, so deeper recursion is unnecessary.
            G::SourceFile
                if stats.is_disabled()
                    && node
                        .children()
                        .any(|c| matches!(c.kind_id().into(), G::MethodDeclaration)) =>
            {
                stats.is_class_space = true;
            }
            // Each receiver method contributes one to the per-space
            // count; `compute_sum` rolls it into `class_nm_sum`, and
            // the parent's merge bubbles it up to the Unit. The
            // method's own space is left unmarked so its
            // per-function npm block stays suppressed.
            G::MethodDeclaration => {
                stats.class_nm += 1;
                // Visibility cannot be detected without source bytes;
                // every method is treated as public.
                stats.class_npm += 1;
            }
            // `interface { Foo(); Bar() int }` declares method
            // signatures via `MethodElem` children of an
            // `InterfaceType`. Interfaces have no func_space of
            // their own, so the count lands on the enclosing space
            // (typically Unit). Interface members are always visible
            // to implementers — counted as public per Java's rule.
            G::InterfaceType => {
                let methods = node
                    .children()
                    .filter(|c| matches!(c.kind_id().into(), G::MethodElem))
                    .count();
                if methods == 0 {
                    return;
                }
                if stats.is_disabled() {
                    stats.is_class_space = true;
                }
                stats.interface_nm += methods;
                stats.interface_npm = stats.interface_nm;
            }
            _ => {}
        }
    }
}

impl Npm for CppCode {
    fn compute(node: &Node, stats: &mut Stats) {
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
                    current_is_public = child
                        .first_child(|id| {
                            id == Cpp::Public || id == Cpp::Protected || id == Cpp::Private
                        })
                        .is_some_and(|tok| tok.kind_id() == Cpp::Public);
                }
                // Inline-defined member function (with a body): regular
                // methods, constructors, destructors, operator overloads,
                // and conversion operators all share these aliased
                // `function_definition` kind-ids.
                FunctionDefinition | FunctionDefinition2 | FunctionDefinition3
                | FunctionDefinition4 => {
                    stats.class_nm += 1;
                    if current_is_public {
                        stats.class_npm += 1;
                    }
                }
                // Declaration-only member function. The wrapping node
                // varies by shape:
                // - `field_declaration > function_declarator` for
                //   ordinary forward-declared methods (incl. pure
                //   virtual `= 0` and `Foo* operator->()` wrapped in
                //   `pointer_declarator`).
                // - `declaration > function_declarator` for
                //   constructors / destructors (no return type).
                // - `template_declaration > declaration >
                //   function_declarator` for templated member fns.
                //
                // The shared `cpp_has_function_declarator` helper walks
                // the declarator subtree (including `declaration`
                // wrappers) so all three shapes collapse into one arm;
                // the guard avoids counting non-method declarations
                // (e.g. nested type aliases) under the same parent.
                FieldDeclaration | Declaration | Declaration2 | Declaration3 | Declaration4
                | TemplateDeclaration
                    if super::npa::cpp_has_function_declarator(&child) =>
                {
                    stats.class_nm += 1;
                    if current_is_public {
                        stats.class_npm += 1;
                    }
                }
                _ => {}
            }
        }
    }
}

// Kotlin's default visibility is `public` (unlike Java, which is
// package-private-by-default), so the "no modifier → public" branch is the
// common case.
impl Npm for KotlinCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Kotlin::*;

        // Enables the `Npm` metric for any class-like func_space.
        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        // Each `ClassBody` contributes its direct `FunctionDeclaration`
        // and `SecondaryConstructor` children to whichever func_space is
        // currently on the stack. Companion objects (not a func_space)
        // fold into the enclosing class — companion functions read as
        // static methods on the parent. Nested classes and interfaces
        // open their own func_space, so their members do not bleed into
        // the outer space.
        //
        // Kotlin properties can declare custom `getter` / `setter`
        // blocks, but these are still property accessors, not separate
        // methods (the Kotlin spec is explicit on this), and they are
        // not counted here. `data class` synthesizes
        // `copy` / `equals` / `hashCode` / `toString` at compile time;
        // those are not user code and are also not counted.
        if !matches!(node.kind_id().into(), ClassBody) {
            return;
        }
        let is_interface = super::npa::kotlin_class_body_is_interface(node);
        // tree-sitter-kotlin elides the `class_member_declaration` and
        // `declaration` rule layers, so function declarations and
        // secondary constructors appear as direct children of
        // `class_body`. `Self::is_func` recognises both kinds.
        for func in node.children().filter(|c| Self::is_func(c)) {
            if is_interface {
                stats.interface_nm += 1;
                stats.interface_npm += 1;
            } else {
                stats.class_nm += 1;
                if super::npa::kotlin_is_public(&func) {
                    stats.class_npm += 1;
                }
            }
        }
    }
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
        fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
            use $lang::*;

            if Self::is_func_space(node) && stats.is_disabled() {
                stats.is_class_space = true;
            }

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

impl Npm for TypescriptCode {
    ts_npm_compute!(Typescript);
}

impl Npm for TsxCode {
    ts_npm_compute!(Tsx);
}

// Ruby `Method` and `SingletonMethod` declared directly inside a
// `Class` or `SingletonClass` body count as methods. Visibility flips
// follow the same keyword-marker rule as `Npa`: a bare `private`
// `public` `protected` `Identifier` child of the body changes the
// running visibility for every subsequent declaration. The
// argument-form (`private :foo`, `private def x`) does NOT flip the
// body-wide flag — matching Ruby's runtime semantics.
//
// `Module` bodies are not classes (the getter routes them to
// `SpaceKind::Namespace`); they do not contribute to `Npm` so a
// module-only file reports zero methods.
impl Npm for RubyCode {
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

        let mut visibility = super::npa::RubyVisibility::Public;
        for child in node.children() {
            if let Some(marker) = super::npa::ruby_visibility_marker(&child, code) {
                visibility = marker;
                continue;
            }
            if matches!(child.kind_id().into(), Method | SingletonMethod) {
                stats.class_nm += 1;
                if visibility == super::npa::RubyVisibility::Public {
                    stats.class_npm += 1;
                }
            }
        }
    }
}

// Default no-op `Npm` impls. Audited in #188. See the rationale block
// on `implement_metric_trait!(Npa, …)` in `src/metrics/npa.rs` — Npm
// classification mirrors Npa one-for-one (same set of "has classes?"
// questions, same follow-up issues).
implement_metric_trait!(
    Npm,
    MozjsCode,
    JavascriptCode,
    PreprocCode,
    CcommentCode,
    PerlCode,
    BashCode,
    LuaCode,
    TclCode,
    ElixirCode
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
                    @r###"
                    {
                      "classes": 1.0,
                      "interfaces": 0.0,
                      "class_methods": 4.0,
                      "interface_methods": 0.0,
                      "classes_average": 0.25,
                      "interfaces_average": null,
                      "total": 1.0,
                      "total_methods": 4.0,
                      "average": 0.25
                    }"###
                );
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
                    @r###"
                    {
                      "classes": 8.0,
                      "interfaces": 0.0,
                      "class_methods": 16.0,
                      "interface_methods": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 8.0,
                      "total_methods": 16.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 8.0,
                      "interfaces": 0.0,
                      "class_methods": 16.0,
                      "interface_methods": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 8.0,
                      "total_methods": 16.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 6.0,
                      "interfaces": 0.0,
                      "class_methods": 12.0,
                      "interface_methods": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 6.0,
                      "total_methods": 12.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 5.0,
                      "interfaces": 0.0,
                      "class_methods": 10.0,
                      "interface_methods": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 5.0,
                      "total_methods": 10.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 6.0,
                      "interfaces": 0.0,
                      "class_methods": 12.0,
                      "interface_methods": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 6.0,
                      "total_methods": 12.0,
                      "average": 0.5
                    }"###
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
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 0.0,
                      "class_methods": 6.0,
                      "interface_methods": 0.0,
                      "classes_average": 0.5,
                      "interfaces_average": null,
                      "total": 3.0,
                      "total_methods": 6.0,
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
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 0.0,
                      "class_methods": 3.0,
                      "interface_methods": 0.0,
                      "classes_average": 1.0,
                      "interfaces_average": null,
                      "total": 3.0,
                      "total_methods": 3.0,
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
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 0.0,
                      "class_methods": 3.0,
                      "interface_methods": 0.0,
                      "classes_average": 1.0,
                      "interfaces_average": null,
                      "total": 3.0,
                      "total_methods": 3.0,
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
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 0.0,
                      "class_methods": 5.0,
                      "interface_methods": 0.0,
                      "classes_average": 0.6,
                      "interfaces_average": null,
                      "total": 3.0,
                      "total_methods": 5.0,
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
                public int a(); // +1
                boolean b();    // +1
                void c();       // +1
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.npm,
                    @r###"
                    {
                      "classes": 0.0,
                      "interfaces": 3.0,
                      "class_methods": 0.0,
                      "interface_methods": 3.0,
                      "classes_average": null,
                      "interfaces_average": 1.0,
                      "total": 3.0,
                      "total_methods": 3.0,
                      "average": 1.0
                    }"###
                );
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
                    @r###"
                    {
                      "classes": 3.0,
                      "interfaces": 3.0,
                      "class_methods": 5.0,
                      "interface_methods": 3.0,
                      "classes_average": 0.6,
                      "interfaces_average": 1.0,
                      "total": 6.0,
                      "total_methods": 8.0,
                      "average": 0.75
                    }"###
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
                assert_eq!(metric.npm.class_nm_sum(), 2.0, "Local must not leak");
                assert_eq!(metric.npm.class_npm_sum(), 1.0, "only Outer is public");
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
            assert_eq!(metric.npm.class_npm_sum(), 0.0);
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
            assert_eq!(metric.npm.interface_nm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 3.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 3.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_interface_methods() {
        check_metrics::<KotlinParser>(
            "interface I {
                fun work(): Int
                fun describe(): String
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.interface_npm_sum(), 2.0);
                assert_eq!(metric.npm.interface_nm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 0.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_interface_with_default_method() {
        check_metrics::<KotlinParser>(
            "interface I {
                fun abs(n: Int): Int {
                    return if (n < 0) -n else n
                }
                fun pure(): Int
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.interface_npm_sum(), 2.0);
                assert_eq!(metric.npm.interface_nm_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npm);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_class_in_interface() {
        // Interface with nested class — methods count to the right
        // bucket.
        check_metrics::<KotlinParser>(
            "interface Outer {
                fun work(): Int
                class Helper {
                    fun help() {}
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.interface_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn kotlin_interface_in_class() {
        check_metrics::<KotlinParser>(
            "class Outer {
                fun work() {}
                interface Sub {
                    fun help(): Int
                }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.interface_npm_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npm);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
            assert_eq!(metric.npm.class_npm_sum(), 0.0);
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 3.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 4.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 3.0);
                assert_eq!(metric.npm.class_nm_sum(), 5.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_interface_methods() {
        // Interface method signatures are implicitly public.
        check_metrics::<TypescriptParser>(
            "interface I {
                a(): void;
                b(x: number): number;
                c: string;
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npm.interface_npm_sum(), 2.0);
                assert_eq!(metric.npm.interface_nm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 0.0);
                insta::assert_json_snapshot!(metric.npm);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn typescript_multiple_classes_and_interface() {
        check_metrics::<TypescriptParser>(
            "class A { m(): void {} }
             class B { private h(): void {} }
             interface I { p(): number; }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                assert_eq!(metric.npm.interface_npm_sum(), 1.0);
                assert_eq!(metric.npm.interface_nm_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    // TSX parity

    #[test]
    fn tsx_empty_class_no_methods() {
        check_metrics::<TsxParser>("class C {}", "foo.tsx", |metric| {
            assert_eq!(metric.npm.class_npm_sum(), 0.0);
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_interface_methods() {
        check_metrics::<TsxParser>(
            "interface I {
                a(): void;
                b(): number;
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.interface_npm_sum(), 2.0);
                assert_eq!(metric.npm.interface_nm_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_generic_class_methods() {
        check_metrics::<TsxParser>(
            "class Box<T> { value: T; set(v: T): void { this.value = v; } }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn tsx_multiple_classes_and_interface() {
        check_metrics::<TsxParser>(
            "class A { m(): void {} }
             class B { private h(): void {} }
             interface I { p(): number; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                assert_eq!(metric.npm.interface_npm_sum(), 1.0);
                assert_eq!(metric.npm.interface_nm_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npm);
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
            assert_eq!(metric.npm.class_npm_sum(), 0.0);
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 0.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 0.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 4.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 0.0);
                assert_eq!(metric.npm.class_nm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn ruby_empty_class_no_methods() {
        check_metrics::<RubyParser>("class Empty\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.npm.class_npm_sum(), 0.0);
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
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

    fn assert_npm_default_zero(metric: &crate::CodeMetrics) {
        assert_eq!(metric.npm.class_npm_sum(), 0.0);
        assert_eq!(metric.npm.class_nm_sum(), 0.0);
    }


    // PLACEHOLDER #202: Mozjs `Npm` is unimplemented.
    #[test]
    fn mozjs_npm_placeholder_returns_zero() {
        check_metrics::<MozjsParser>("class A { m1() {} m2() {} }", "foo.js", |metric| {
            assert_npm_default_zero(&metric);
        });
    }

    // PLACEHOLDER #202: JavaScript `Npm` is unimplemented.
    #[test]
    fn javascript_npm_placeholder_returns_zero() {
        check_metrics::<JavascriptParser>("class A { m1() {} m2() {} }", "foo.js", |metric| {
            assert_npm_default_zero(&metric);
        });
    }


    // PLACEHOLDER #204: C++ `Npm` is unimplemented.
    #[test]
    fn cpp_npm_placeholder_returns_zero() {
        check_metrics::<CppParser>(
            "class A { public: void m1() {} void m2() {} };",
            "foo.cpp",
            |metric| assert_npm_default_zero(&metric),
        );
    }


    // --- Python NPM ---------------------------------------------------

    #[test]
    fn python_empty_class_no_methods() {
        check_metrics::<PythonParser>("class C:\n    pass\n", "foo.py", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
            assert_eq!(metric.npm.class_npm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
                assert_eq!(metric.npm.class_npm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
                assert_eq!(metric.npm.class_npm_sum(), 3.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn rust_empty_unit_no_methods() {
        check_metrics::<RustParser>("", "empty.rs", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
            assert_eq!(metric.npm.class_npm_sum(), 0.0);
            assert_eq!(metric.npm.interface_nm_sum(), 0.0);
            assert_eq!(metric.npm.interface_npm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn rust_trait_methods_count() {
        // `fn draw(&self);` (signature only) + `fn area(&self) -> f64
        // { 0.0 }` (default body) → both are interface methods.
        // Trait methods are always public. → interface_nm=2,
        // interface_npm=2.
        check_metrics::<RustParser>(
            "trait Drawable {\n\
             \x20   fn draw(&self);\n\
             \x20   fn area(&self) -> f64 { 0.0 }\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npm.interface_nm_sum(), 2.0);
                assert_eq!(metric.npm.interface_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 0.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn rust_module_level_function_not_method() {
        // Top-level `fn` is NOT a method. The npa/npm metric on a
        // Unit space stays disabled (no class/interface), so the
        // method count is zero.
        check_metrics::<RustParser>("fn f() {}\nfn g() {}\n", "foo.rs", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
            assert_eq!(metric.npm.interface_nm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn rust_trait_impl_block_counts_methods() {
        // `impl Drawable for Foo` is also an `impl_item` — its methods
        // count toward class_nm of the impl. Trait impls and inherent
        // impls are not distinguished at the AST level (both parse as
        // `impl_item`).
        check_metrics::<RustParser>(
            "struct Foo;\n\
             trait Drawable { fn draw(&self); }\n\
             impl Drawable for Foo { fn draw(&self) {} }\n",
            "foo.rs",
            |metric| {
                // Trait body: 1 signature method → interface_nm = 1.
                // Impl body: 1 fn `draw` → class_nm = 1.
                assert_eq!(metric.npm.interface_nm_sum(), 1.0);
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    // ----- Go -----

    #[test]
    fn go_empty_unit_no_methods() {
        // No receiver methods → npm stays disabled, class_nm_sum = 0.
        check_metrics::<GoParser>("package main\n", "empty.go", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
            insta::assert_json_snapshot!(metric.npm);
        });
    }

    #[test]
    fn go_method_declarations_count() {
        // Two `func (r Foo) ...` methods on the same receiver type →
        // class_nm_sum = 2. Visibility cannot be detected from the
        // node alone, so class_npm == class_nm.
        check_metrics::<GoParser>(
            "package main\n\
             type Foo struct{}\n\
             func (f Foo) DoX() {}\n\
             func (f Foo) doY() {}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                assert_eq!(metric.npm.class_npm_sum(), 2.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    #[test]
    fn go_interface_methods_count_as_interface_nm() {
        // `interface { Read() error; Close() error }` declares two
        // method signatures → interface_nm = 2, interface_npm = 2
        // (interface members are always visible to implementers,
        // matching Java's interface rule).
        check_metrics::<GoParser>(
            "package main\ntype RC interface { Read() error; Close() error }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npm.interface_nm_sum(), 2.0);
                assert_eq!(metric.npm.interface_npm_sum(), 2.0);
                assert_eq!(metric.npm.class_nm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    // ----- Elixir -----

    #[test]
    fn elixir_npm_is_zero_documented_limitation() {
        // Elixir's closest analog to a "class method" is `def` /
        // `defp` inside a `defmodule`. Both surface as `Call` nodes
        // with a macro identifier in the `target` field — they are
        // NOT distinct grammar productions. The `Npm` trait signature
        // receives no source bytes, so identifying a Call as `def`
        // / `defp` rather than any other macro is not possible from
        // the trait's inputs.
        //
        // A correct Elixir Npm impl would either need (a) the trait
        // signature to gain `&[u8]` like `Cognitive` / `Abc` did
        // here, or (b) a Checker-level helper that pre-tags `def` /
        // `defp` Calls. Both are cross-cutting changes out of scope
        // for this fix; the metric stays at zero with a documented
        // reason. This test pins the documented omission.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def pub_one, do: 1\n  defp priv_one, do: 1\n  def pub_two(x), do: x\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.npm.class_nm_sum(), 0.0);
                assert_eq!(metric.npm.class_npm_sum(), 0.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }

    // ----- C++ -----

    #[test]
    fn cpp_empty_unit_no_methods() {
        // No code → no class spaces → npm = 0.
        check_metrics::<CppParser>("", "empty.cpp", |metric| {
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
            assert_eq!(metric.npm.class_npm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 2.0);
                assert_eq!(metric.npm.class_npm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
                assert_eq!(metric.npm.class_npm_sum(), 3.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 1.0);
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
                assert_eq!(metric.npm.class_npm_sum(), 3.0);
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
            assert_eq!(metric.npm.class_nm_sum(), 0.0);
            assert_eq!(metric.npm.class_npm_sum(), 0.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
                assert_eq!(metric.npm.class_npm_sum(), 1.0);
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
                assert_eq!(metric.npm.class_nm_sum(), 3.0);
                assert_eq!(metric.npm.class_npm_sum(), 3.0);
                insta::assert_json_snapshot!(metric.npm);
            },
        );
    }
}
