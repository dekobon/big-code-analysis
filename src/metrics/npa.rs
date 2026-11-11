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
use crate::macros::{csharp_var_decl_kinds, csharp_var_declarator_kinds, implement_metric_trait};
use crate::metrics::MemberScopeExt;
use crate::node::Node;
use crate::*;

/// The `Npa` metric.
///
/// This metric counts the number of public attributes
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
    class_npa: usize,
    interface_npa: usize,
    class_na: usize,
    interface_na: usize,
    class_npa_sum: usize,
    interface_npa_sum: usize,
    class_na_sum: usize,
    interface_na_sum: usize,
    space_kind: SpaceKind,
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

    // Checks if the `Npa` metric is disabled
    #[inline]
    pub(crate) fn is_disabled(&self) -> bool {
        !self.space_kind.is_member_scope()
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
    ($code:ty, $lang:ident, $roles:ident) => {
        impl Npa for $code {
            fn compute<'a>(
                node: &Node<'a>,
                code: &'a [u8],
                ancestors: Ancestors<'a, '_>,
                stats: &mut Stats,
            ) {
                use $crate::roles::$roles::member;
                use $lang::*;

                // Which of a body's children are members is the `Member`
                // row's; which body a member sits in, and whether it is
                // public, is this consumer's reading. A `class_body` or an
                // enum's declarations block holds fields, and an interface
                // or annotation body holds constants (`node-types.json`
                // at the pin admits no other member in either), so the
                // body decides the bucket and the row decides membership.
                // Each member is asked under its own chain — the body's
                // plus the body — so a gate the row grows later reads the
                // right parent; the chain is built inside the body arms,
                // never for the nodes that are no body.
                match node.kind_id().into() {
                    ClassBody | EnumBodyDeclarations => {
                        let under = ancestors.of_children(node);
                        let members = under.get();
                        for declaration in
                            node.children().filter(|n| member::owns(n, code, members))
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
                        let under = ancestors.of_children(node);
                        let members = under.get();
                        stats.interface_na += node
                            .children()
                            .filter(|n| member::owns(n, code, members))
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

mod ruby_body;
mod ruby_ledger;
mod shared;
pub(crate) use python::python_unwrap_function;
pub(crate) use ruby_body::{RubyBodyConsumer, RubyBodyShape, RubyBodyState, RubyOpenCall};
pub(crate) use ruby_ledger::{RubyLedger, ruby_is_auto_private, ruby_leave_visibility_call};
pub(crate) use shared::*;

// Reached by the per-language submodules through `use super::*`.
use crate::lang_helpers::python::python_is_block;

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
    ($lang:ident, $roles:ident) => {
        fn compute<'a>(
            node: &Node<'a>,
            code: &'a [u8],
            ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use $crate::roles::$roles::{func_opener, member};
            use $lang::*;

            // Membership is the `Member` row's (a class field, an
            // interface's property signature); which body the member
            // sits in, whether it is public, and whether a field's
            // initialiser makes it a method rather than an attribute
            // are this consumer's reading. Each member is asked under
            // its own chain — the body's plus the body — which the
            // callable's binding gates climb; built inside the body arms,
            // never for the nodes that are no body.
            match node.kind_id().into() {
                ClassBody => {
                    let under = ancestors.of_children(node);
                    let members = under.get();
                    for member in node.children() {
                        // Plain field declaration (`x: T = expr;`, `private x: T;`,
                        // `static x: T = expr;`). Each is one attribute.
                        // Skip fields whose initializer is an arrow function or
                        // function expression — those are methods written as
                        // field initializers and are counted by `npm` instead.
                        if member::owns(&member, code, members) {
                            if member
                                .first_child(|id| {
                                    id == $lang::ArrowFunction || id == $lang::FunctionExpression
                                })
                                .is_none()
                            {
                                stats.class_na += 1;
                                if ts_member_is_public!($lang, member) {
                                    stats.class_npa += 1;
                                }
                            }
                        } else if func_opener::is_callable(&member, code, members)
                            && let Some(params) =
                                member.first_child(|id| id == $lang::FormalParameters)
                        {
                            // Parameter properties on any method — a callable
                            // opener met as a body's child, the
                            // `method_definition`. In practice these only
                            // appear on the constructor. Scan formal_parameters
                            // at the class-body level so the attribute lands on
                            // the class space, not the method's own function
                            // space.
                            // role-bypass: allow(DocTarget) — a constructor's parameter
                            // properties for the attribute count; not whether each takes a
                            // doc comment
                            for param in params.children().filter(|c| {
                                matches!(c.kind_id().into(), RequiredParameter | RequiredParameter2)
                            }) {
                                if param
                                    .first_child(|id| {
                                        id == $lang::AccessibilityModifier || id == $lang::Readonly
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
                    }
                }
                InterfaceBody => {
                    let under = ancestors.of_children(node);
                    let members = under.get();
                    let count = node
                        .children()
                        .filter(|c| member::owns(c, code, members))
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
// `code` source bytes are already available below, so implementing
// prototype detection requires no trait signature change (see
// `Abc::compute` for the existing pattern).

macro_rules! js_npa_compute {
    ($lang:ident, $roles:ident) => {
        fn compute<'a>(
            node: &Node<'a>,
            code: &'a [u8],
            ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use $crate::roles::$roles::member;
            use $lang::*;

            if !matches!(node.kind_id().into(), ClassBody) {
                return;
            }

            // Membership is the `Member` row's; a field whose initialiser
            // is a function is a method written as a field, this
            // consumer's reading. Each member is asked under its own
            // chain, the body's plus the body.
            let under = ancestors.of_children(node);
            let members = under.get();
            for member in node.children() {
                if member::owns(&member, code, members)
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
    use std::path::PathBuf;

    use crate::test_support::{
        assert_child_space_kind, ast_has_kind_id, check_func_space_only_shim,
        check_metrics_only_shim, child_space,
    };

    use super::*;

    check_metrics_only_shim!(check_metrics, Npa);
    check_func_space_only_shim!(check_func_space, Npa);
    check_metrics_only_shim!(check_metrics_with_npm, Npa, Npm);

    // The static data member templates (#1299), one per declarator
    // shape the pinned grammars give a `template_declaration >
    // declaration` payload: a plain name, an `init_declarator`, an
    // `array_declarator` whose bound `N` is itself an `identifier`, a
    // `pointer_declarator`, the field-less `reference_declarator`, a
    // `function_declarator > parenthesized_declarator > pointer_declarator`
    // function-pointer data member whose parameter name `a` is another
    // `identifier`, and a `requires_clause` before the payload. Every
    // one is valid C++20: `g++ -std=c++20 -fsyntax-only` accepts the
    // fixture below exactly as written, and rejects it with the leading
    // `constexpr int N = 3;` removed. The templates
    // straddle the `private:` label so the new arm is reached under
    // both flags: with every template public, `add_attributes(stats,
    // 1, true)` would pass.
    //
    // The two non-templated fields keep the `field_declaration` arm in
    // the fixture, so the new arm cannot pass by claiming what that one
    // already counts.
    #[cfg(any(feature = "cpp", feature = "mozcpp"))]
    const STATIC_DATA_MEMBER_TEMPLATES: &str = "constexpr int N = 3;\n\
         class G {\n\
         public:\n\
             static int plainStatic;\n\
             template<typename T> static T tplStatic;\n\
             template<class T> static constexpr T v = T(1);\n\
             template<class T> static T arr[N];\n\
             template<class T> static T* ptr;\n\
         private:\n\
             int normalField;\n\
             template<class T> static T& ref;\n\
             template<class T> static int (*fp)(int a);\n\
             template<class T> requires true static T req;\n\
         };";

    // The templated payloads `Npa` must decline and `Npm` decides:
    // a member function declared without a body, a conversion
    // operator, a deleted member function (`= delete`, which the
    // grammar wraps in an `init_declarator` only in the templated
    // form — the batch review found it billed as a data member), a
    // nested class, an alias, and a friend. `Npa` counts the one data
    // member and nothing else; `Npm` counts the three callables. The
    // `tfn` / `conv` / `tdel` triple is the one both consumers read
    // through the same `cpp_declares_function`, so a payload claimed
    // by both moves one per-metric total up and a payload claimed by
    // neither moves one down; the two are asserted apart because a
    // swap would leave their sum at 4.
    #[cfg(any(feature = "cpp", feature = "mozcpp"))]
    const TEMPLATE_PAYLOADS_SPLIT: &str = "struct S {\n\
             template<class T> T tfn();\n\
             template<class T> operator T();\n\
             template<class T> void tdel(T) = delete;\n\
             template<class T> struct Inner {};\n\
             template<class T> using A = T;\n\
             template<class T> friend void fr(T);\n\
             template<class T> static T data;\n\
         };";

    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_static_data_member_templates_are_attributes() {
        // expected: na = 9 (two fields, seven templates), npa = 5
        // (`plainStatic` and the four public templates). Before #1299
        // the templates counted under nothing and this read 2/1.
        check_metrics::<CppParser>(STATIC_DATA_MEMBER_TEMPLATES, "foo.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 9);
            assert_eq!(metric.npa.class_npa_sum(), 5);
        });
    }

    #[cfg(feature = "mozcpp")]
    #[test]
    fn mozcpp_static_data_member_templates_are_attributes() {
        // `mozcpp` owns no file extension, so nothing routes a corpus
        // file to it; the Cpp fixture is replayed here on its own ids.
        check_metrics::<MozcppParser>(STATIC_DATA_MEMBER_TEMPLATES, "foo.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 9);
            assert_eq!(metric.npa.class_npa_sum(), 5);
        });
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_template_payloads_split_between_npa_and_npm() {
        // expected: na = 1 (`data`), nm = 3 (`tfn`, `conv`, `tdel`); a
        // struct, so the public counts equal the totals. `Inner` opens
        // its own class space with nothing in it, so it adds 0 to both
        // sums.
        check_metrics_with_npm::<CppParser>(TEMPLATE_PAYLOADS_SPLIT, "foo.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 1);
            assert_eq!(metric.npa.class_npa_sum(), 1);
            assert_eq!(metric.npm.class_nm_sum(), 3);
            assert_eq!(metric.npm.class_npm_sum(), 3);
        });
    }

    #[cfg(feature = "mozcpp")]
    #[test]
    fn mozcpp_template_payloads_split_between_npa_and_npm() {
        check_metrics_with_npm::<MozcppParser>(TEMPLATE_PAYLOADS_SPLIT, "foo.cpp", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 1);
            assert_eq!(metric.npa.class_npa_sum(), 1);
            assert_eq!(metric.npm.class_nm_sum(), 3);
            assert_eq!(metric.npm.class_npm_sum(), 3);
        });
    }

    // `cpp_count_field_identifiers`'s member-pointer arm (#1718), asked
    // directly. Its one caller hands it a `field_declaration`, and no
    // field spelling reaches the arm under the pinned grammars — the
    // parenthesised one recovers into an `ERROR` over `S::` beside a
    // `pointer_declarator`, the unparenthesised one into a
    // `field_identifier` `S` beside a `bitfield_clause` — so the
    // templated payloads are the only real trees carrying the
    // shape. Rows: a member pointer counts one; one under a global scope
    // and with an array bound `N` (an `identifier`) still counts one; a
    // qualified *type* holds no member pointer and adds nothing.
    #[cfg(any(feature = "cpp", feature = "mozcpp"))]
    fn assert_member_pointer_names_count_once(lang: LANG) {
        let ast = crate::test_support::parse_named(
            lang,
            "foo.cpp",
            "struct S {\n\
                 template<class T> static int (S::*fp)(T);\n\
                 template<class T> static int (::S::*fps[N])(T);\n\
                 template<class T> static std::vector<T> v;\n\
             };",
        );
        let counts: Vec<usize> = ast
            .root_node()
            .descendants_by_kind(&["declaration"])
            .iter()
            .map(cpp_count_field_identifiers)
            .collect();
        assert_eq!(counts, [1, 1, 0]);
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_member_pointer_names_count_once() {
        assert_member_pointer_names_count_once(LANG::Cpp);
    }

    #[cfg(feature = "mozcpp")]
    #[test]
    fn mozcpp_member_pointer_names_count_once() {
        assert_member_pointer_names_count_once(LANG::Mozcpp);
    }

    // A type qualified 20,000 levels deep (`A::A::…::B v;`) is one data
    // member. Every member-pointer arm asks `cpp_member_pointer` about
    // each qualified name a member holds, so the scope chain is walked in
    // a loop: the recursive first draft overflowed a worker's stack at
    // about 11,000 levels, a depth no other metric minds (#1718 review).
    // Run on a small stack so a reintroduced recursion aborts here.
    // Measured with the recursion restored, in the dev profile the tests
    // build: it overflows at about 2,600 levels on this 512 KiB stack
    // (10,800 at the 2 MiB a spawned worker gets, 43,500 at 8 MiB), so
    // `DEPTH` is ~7.7x past it. A future `DEPTH` must stay well above the
    // overflow depth for the stack it sets, or a recursion passes here.
    //
    // expected: one static data member — na = 1, nm = 0.
    #[cfg(any(feature = "cpp", feature = "mozcpp"))]
    fn assert_deeply_qualified_type_is_one_attribute<T: crate::MetricSuite + 'static>() {
        // `A::` levels before the type's last name, far past what the
        // stack below holds at one frame per level.
        const DEPTH: usize = 20_000;
        // A deliberately small test-thread stack.
        const SMALL_STACK: usize = 512 * 1024;
        let source = format!("struct X {{ static {}B v; }};\n", "A::".repeat(DEPTH));
        std::thread::Builder::new()
            .stack_size(SMALL_STACK)
            .spawn(move || {
                check_metrics_with_npm::<T>(&source, "foo.cpp", |metric| {
                    assert_eq!(metric.npa.class_na_sum(), 1);
                    assert_eq!(metric.npm.class_nm_sum(), 0);
                });
            })
            .expect("spawn bounded-stack thread")
            .join()
            .expect("the member-pointer walk must not overflow a 512 KiB stack");
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_deeply_qualified_type_is_one_attribute() {
        assert_deeply_qualified_type_is_one_attribute::<CppParser>();
    }

    #[cfg(feature = "mozcpp")]
    #[test]
    fn mozcpp_deeply_qualified_type_is_one_attribute() {
        assert_deeply_qualified_type_is_one_attribute::<MozcppParser>();
    }

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_no_attributes() {
        check_metrics::<GroovyParser>("class A { void foo() {} }", "foo.groovy", |metric| {
            assert_eq!(metric.npa.total_na(), 0);
            assert_eq!(metric.npa.total_npa(), 0);
        });
    }

    #[cfg(feature = "groovy")]
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

    #[cfg(feature = "groovy")]
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

    #[cfg(feature = "groovy")]
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

    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_no_attributes_in_unit_scope() {
        check_metrics::<GroovyParser>("int x = 1", "foo.groovy", |metric| {
            assert_eq!(metric.npa.total_na(), 0);
        });
    }

    #[cfg(feature = "groovy")]
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

    #[cfg(feature = "groovy")]
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

    #[cfg(feature = "groovy")]
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

    #[cfg(feature = "groovy")]
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

    #[cfg(feature = "groovy")]
    #[test]
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

    #[cfg(feature = "groovy")]
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

    // An anonymous inner class owns its attributes. Since
    // dekobon-tree-sitter-groovy 0.5.0 the body of `new Runnable() { … }`
    // is a `class_body` under its `object_creation_expression`, which
    // opens a `<anonymous>` Class space as Java's does (#463), so `x`
    // counts there and `r` on `X`. The totals alone cannot show that:
    // at 0.3.0 they were also 2 / 2, with the body spilled out of the
    // field behind an `ERROR` and `x` counted on `X` itself, and at
    // 0.4.0 they were 1 / 1 with `x` inside a closure and owned by no
    // class (tree-sitter-groovy#68). The per-space rows are the tell.
    //
    // expected: 2 attributes, both public, in total; one each on `X`
    // and on the anonymous class.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_anonymous_inner_class() {
        use crate::traits::ParserTrait;
        let src = "class X {
                public Runnable r = new Runnable() {
                    public int x
                    void run() {}
                }
            }";
        let parser = GroovyParser::new(
            src.as_bytes().to_vec(),
            std::path::Path::new("foo.groovy"),
            None,
        );
        assert!(
            !parser.root().has_error(),
            "the anonymous class parses clean"
        );
        assert!(
            !ast_has_kind_id(&parser, Groovy::Closure as u16),
            "the body is a class body, not the 0.4.0 trailing closure"
        );
        check_func_space::<GroovyParser, _>(src, "foo.groovy", |space| {
            assert_eq!(space.metrics.npa.class_na_sum(), 2);
            assert_eq!(space.metrics.npa.class_npa_sum(), 2);
            let outer = child_space(&space, "X");
            let anonymous = child_space(outer, "<anonymous>");
            assert_eq!(anonymous.kind, SpaceKind::Class);
            assert_eq!(
                [outer.metrics.npa.class_na(), outer.metrics.npa.class_npa()],
                [1, 1],
                "`r` on `X`"
            );
            assert_eq!(
                [
                    anonymous.metrics.npa.class_na(),
                    anonymous.metrics.npa.class_npa()
                ],
                [1, 1],
                "`x` on the anonymous class"
            );
        });
    }

    // FIXME(tree-sitter-groovy#72): the grammar has no compact
    // constructor. Spelled with a modifier, as Groovy's own documentation
    // spells it (`public Warning { … }`), it parses with no error node
    // as a modifier-only field named after the record followed by an
    // instance initializer `block`, so `npa` counts a public attribute
    // the record does not have. Java's `compact_constructor_declaration`
    // reads 0 / 0 here, and so did dekobon-tree-sitter-groovy 0.3.0,
    // which lost the whole body to error recovery. The correct values
    // are 0 / 0; the ones below are the 0.5.0 parse. The clean-parse
    // assertion and the census are the premise: the bare `Warning { … }`
    // spelling still errors (`groovy_compact_constructor_does_not_parse`
    // in `nargs.rs`), so this is the form no error check can see.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_compact_constructor_with_a_modifier_reads_as_a_public_field() {
        use crate::traits::ParserTrait;
        let src = "record Warning(String message) {
                public Warning {
                    Objects.requireNonNull(message)
                }
            }";
        let parser = GroovyParser::new(
            src.as_bytes().to_vec(),
            std::path::Path::new("foo.groovy"),
            None,
        );
        assert!(
            !parser.root().has_error(),
            "the compact constructor errors now; re-derive the values below"
        );
        assert!(
            ast_has_kind_id(&parser, Groovy::FieldDeclaration as u16),
            "the grammar no longer reads the compact constructor as a field: \
             it may parse it now, so re-derive the values below (0 / 0) and drop the FIXME"
        );
        check_metrics::<GroovyParser>(src, "foo.groovy", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 1);
            assert_eq!(metric.npa.class_npa_sum(), 1);
        });
    }

    // Regression for issue #280: Groovy mirrors Java's enum / record /
    // annotation handling. Record support in the dekobon Groovy grammar
    // lags behind groovyc, but the grammar exposes `record_declaration`
    // and the `Npa` body walker treats it identically.
    #[cfg(feature = "groovy")]
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

    #[cfg(feature = "groovy")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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
    #[cfg(feature = "java")]
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
    #[cfg(feature = "java")]
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

    #[cfg(feature = "java")]
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

    #[cfg(feature = "php")]
    #[test]
    fn php_no_class_attributes() {
        check_metrics::<PhpParser>(
            "<?php class A { public function f(): void {} }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "php")]
    #[test]
    fn php_one_public_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { public int $x = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[cfg(feature = "php")]
    #[test]
    fn php_one_private_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { private int $x = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[cfg(feature = "php")]
    #[test]
    fn php_one_protected_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { protected int $x = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[cfg(feature = "php")]
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

    #[cfg(feature = "php")]
    #[test]
    fn php_static_public_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { public static int $x = 0; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[cfg(feature = "php")]
    #[test]
    fn php_readonly_public_attribute() {
        check_metrics::<PhpParser>(
            "<?php class A { public readonly int $x; }",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.npa),
        );
    }

    #[cfg(feature = "php")]
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

    #[cfg(feature = "php")]
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

    #[cfg(feature = "php")]
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

    #[cfg(feature = "php")]
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

    #[cfg(all(feature = "java", feature = "php"))]
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

    #[cfg(feature = "php")]
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

    #[cfg(feature = "php")]
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

    #[cfg(feature = "php")]
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

    #[cfg(feature = "php")]
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

    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_empty_class_no_attributes() {
        check_metrics::<KotlinParser>("class C {}", "foo.kt", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.interface_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    /// A parameter property is the class's attribute, not its primary
    /// constructor's — the constructor opens a function space of its own
    /// since #1828, inside which each parameter is visited, so the count
    /// is read at the class declaration instead. The class's **own**
    /// counts carry all three (`a`, `b` public; `c` private) and the
    /// constructor's own carry none; the private one keeps `class_npa`
    /// independent of `class_na`. Before the move the class's own
    /// `class_na` read 0 and the constructor's 3, with only the sums
    /// unchanged.
    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_parameter_properties_stay_the_classs() {
        check_func_space::<KotlinParser, _>(
            "class C(val a: Int, var b: Int, private val c: Int, d: Int) {
                val e: Int = d
            }",
            "own.kt",
            |space| {
                let class = child_space(&space, "C");
                let own = |s: &FuncSpace| (s.metrics.npa.class_na(), s.metrics.npa.class_npa());
                assert_eq!(own(class), (4, 3), "a, b, c, e; c is private");
                assert_eq!(own(&class.spaces[0]), (0, 0), "the constructor owns none");
                assert_eq!(class.spaces[0].kind, SpaceKind::Function);
                assert_eq!(
                    (
                        class.metrics.npa.class_na_sum(),
                        class.metrics.npa.class_npa_sum()
                    ),
                    (4, 3)
                );
            },
        );
    }

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "kotlin")]
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

    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_empty_class_no_attributes() {
        check_metrics::<TypescriptParser>("class C {}", "foo.ts", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
    #[test]
    fn tsx_empty_class_no_attributes() {
        check_metrics::<TsxParser>("class C {}", "foo.tsx", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
    #[test]
    fn tsx_generic_class_attributes() {
        check_metrics::<TsxParser>("class Box<T> { value: T; }", "foo.tsx", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 1);
            assert_eq!(metric.npa.class_na_sum(), 1);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "typescript")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_instance_variable_attribute() {
        // Bare `@x = …` at class scope is one public attribute.
        check_metrics::<RubyParser>("class A\n  @x = 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 1);
            assert_eq!(metric.npa.class_na_sum(), 1);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_class_variable_attribute() {
        // `@@y = …` at class scope is one attribute.
        check_metrics::<RubyParser>("class A\n  @@y = 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.npa.class_npa_sum(), 1);
            assert_eq!(metric.npa.class_na_sum(), 1);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
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

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_visibility_call_wrapping_attr_macro_counts_symbols() {
        // `private attr_accessor :b` nests the `attr_accessor` call
        // inside the `private` call, so the macro is never a direct
        // child of the body. Before #1255 its symbols were dropped from
        // `na` entirely rather than counted as private attributes.
        //
        // expected: na = 2 (a, b), npa = 1 (a).
        check_metrics::<RubyParser>(
            "class D\n  attr_accessor :a\n  private attr_accessor :b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_wrapped_attr_macro_reads_the_keyword_not_the_flag() {
        // Seeds the body-wide flag to `private` first, so the assertion
        // can only pass if the wrapped macro reads the *keyword*. With
        // the flag consulted instead, `b` would come out private and
        // npa would be 0.
        //
        // expected: na = 2 (a, b), npa = 1 — `a` is private by the flag,
        // `b` is public because `public attr_reader :b` says so.
        check_metrics::<RubyParser>(
            "class D\n  private\n  attr_reader :a\n  public attr_reader :b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_parenthesised_bare_keyword_flips_the_attribute_flag() {
        // `private()` is the explicit-parens spelling of the bare
        // keyword. It is a `call`, not an `identifier`, so it reaches
        // `Npa` through the visibility-call arm rather than through
        // `ruby_visibility_marker` — and both walkers must agree that it
        // flips the flag (grammar-dispatch rule 7).
        //
        // expected: na = 2 (a, b), npa = 1 (a).
        check_metrics::<RubyParser>(
            "class D\n  attr_accessor :a\n  private()\n  attr_accessor :b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attr_macro_counts_symbol_array_elements() {
        // `attr_writer %i[e f]` passes one argument naming two
        // attributes. Counting arguments rather than the symbols they
        // name reports 1; ignoring the `symbol_array` kind reports 0.
        //
        // expected: na = 3 (a, e, f), npa = 3.
        check_metrics::<RubyParser>(
            "class A\n  attr_accessor :a\n  attr_writer %i[e f]\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 3);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attr_macro_on_another_object_declares_nothing() {
        // `Other.attr_accessor :b` adds an attribute to `Other`, not
        // here. The receiver is a `constant`, so a scan for the leading
        // `identifier` would find `attr_accessor` and count it.
        //
        // `self.(:c)` is `self.call(:c)` spelled without a method name:
        // the `call` node has a `self` receiver and no `method` field,
        // so the callee lookup must yield nothing rather than assume
        // every `self`-received call names one.
        //
        // expected: na = 1 (a), npa = 1.
        check_metrics::<RubyParser>(
            "class A\n  attr_accessor :a\n  Other.attr_accessor :b\n  self.(:c)\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_hash_key_symbol_declares_no_attribute() {
        // Pins the defensive `HashKeySymbol` arm in
        // `ruby_symbol_argument_count` as unreachable at the pinned
        // grammar (grammar-dispatch rule 2). `attr_accessor :x, foo: 1`
        // puts a `hash_key_symbol` inside this very argument list, but
        // wrapped in a `pair` — tree-sitter-ruby 0.23.1 emits it only as
        // a `pair` / `keyword_pattern` key, never as a direct argument.
        //
        // expected: na = 1 (x), npa = 1. A grammar that promoted
        // `hash_key_symbol` to a direct argument would reach the arm and
        // report 2, counting a keyword argument as an attribute.
        let source = "class A\n  attr_accessor :x, foo: 1\nend\n";
        let parser = RubyParser::new(source.as_bytes().to_vec(), &PathBuf::from("foo.rb"), None);
        assert!(
            ast_has_kind_id(&parser, Ruby::HashKeySymbol as u16),
            "the fixture no longer produces a `hash_key_symbol`; the \
             claim below is then vacuous"
        );
        check_metrics::<RubyParser>(source, "foo.rb", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 1);
            assert_eq!(metric.npa.class_npa_sum(), 1);
        });
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_hidden_call_alias_is_not_emitted() {
        // The Ruby `Npm` / `Npa` walkers dispatch on
        // `Call | Call2 | Call3 | Call4`. `Call5` is the hidden `_call`
        // supertype and is deliberately absent; pin that rather than
        // leaving it to a comment (grammar-dispatch rule 2).
        let source =
            "class A\n  attr_accessor :a\n  private attr_accessor :b\n  self.private :a\nend\n";
        let parser = RubyParser::new(source.as_bytes().to_vec(), &PathBuf::from("foo.rb"), None);
        assert!(
            ast_has_kind_id(&parser, Ruby::Call3 as u16),
            "the fixture no longer produces the `Call3` alias; the \
             negative claim below is then vacuous"
        );
        assert!(
            !ast_has_kind_id(&parser, Ruby::Call5 as u16),
            "tree-sitter-ruby now emits the hidden `_call` supertype; \
             add `Call5` to every Ruby `Call` dispatch set"
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_visibility_call_without_a_wrapped_macro_declares_nothing() {
        // The visibility-call arm must not invent attributes out of the
        // symbol form (`private :a`, which names a method) or out of an
        // argument it cannot resolve (`private *SYMS`). Nor may an
        // `attr_*` macro count an argument that is not a symbol at all
        // (`attr_reader name`, whose attribute is known only at
        // runtime).
        //
        // expected: na = 1, npa = 1 — only `attr_accessor :a` declares.
        check_metrics::<RubyParser>(
            "class D\n  attr_accessor :a\n  attr_reader name\n  private :a\n  private *SYMS\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                insta::assert_json_snapshot!(metric.npa);
            },
        );
    }

    // --- Ruby attributes the body reaches through a wrapper (#1719) -------
    //
    // The attribute walk reads the statements the method walk does — the
    // shared body walk in `npa/ruby_body.rs` — so an `attr_*` call or an
    // `@x = …` / `@@x = …` assignment counts wherever it runs at class
    // scope: under a conditional, a loop, a `begin`, a `case` arm, a
    // statement modifier, parentheses, an assignment's right side, or as
    // a call's argument. Until #1719 only the body's direct children and
    // a visibility call's direct arguments were read, and a bare marker
    // was read only as a direct child, with no shadow table. These mirror
    // `npm.rs`'s #1401 block. Every visibility expectation was measured
    // against ruby 3.0.2 with `public_instance_methods(false)` and its
    // siblings (the reader method is the attribute's visibility), the
    // instance and class variables with `instance_variables` /
    // `class_variables`; the oracle script and its output are recorded on
    // #1719. The walk is syntactic and counts every branch, as `nom`
    // does, so a branch Ruby skips still counts.

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attribute_under_a_conditional_counts() {
        // Both arms of an `if`, a `begin` after a `private` marker, and a
        // bare `attr_reader` after it. Measured: `modern` public, `hidden`
        // and `plain` private, `@wrapped` set; `legacy` sits on the branch
        // Ruby did not take. Before #1719: na = 1 (`plain`), npa = 0.
        //
        // expected: na = 5 (modern, legacy, @wrapped, hidden, plain),
        // npa = 2 (modern, legacy).
        check_metrics::<RubyParser>(
            "class Cond\n  if RUBY_VERSION > \"3.0\"\n    attr_reader :modern\n  else\n    attr_reader :legacy\n  end\n  private\n  begin\n    @wrapped = 1\n    attr_reader :hidden\n  end\n  attr_reader :plain\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 5);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attribute_under_each_statement_wrapper_counts() {
        // One declaration per wrapper the walk reads through, each
        // reachable by that wrapper's arm alone: `while`, `until`, `for`
        // (whose body is a `do`), `unless`, the four statement modifiers
        // (the `body` field), `rescue` and `ensure`, `elsif`, a `when`
        // and a `case` `else`, and an `in` clause. Ruby defines the eight
        // on branches it takes, every one public. Before #1719: na = 1.
        //
        // expected: na = 15 (twelve `attr_reader`s, three assignments),
        // npa = 15.
        check_metrics::<RubyParser>(
            "class W\n  while false\n    attr_reader :in_while\n  end\n  until true\n    attr_reader :in_until\n  end\n  for i in [] do\n    attr_reader :in_for\n  end\n  unless false\n    attr_reader :in_unless\n  end\n  attr_reader :early unless false\n  attr_reader :if_mod if true\n  @late = 1 while false\n  @until_mod = 1 until true\n  begin\n    1\n  rescue\n    attr_reader :in_rescue\n  ensure\n    attr_reader :in_ensure\n  end\n  if false\n    2\n  elsif true\n    attr_reader :in_elsif\n  end\n  case 1\n  when 1\n    attr_reader :in_when\n  else\n    @in_case_else = 1\n  end\n  case 2\n  in Integer\n    attr_reader :in_clause\n  end\n  attr_reader :bare\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 15);
                assert_eq!(metric.npa.class_npa_sum(), 15);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attribute_in_parentheses_a_rescue_modifier_or_an_assignment_counts() {
        // A parenthesised statement list; the `rescue` modifier on both
        // sides; the right side of an assignment, plain, through
        // parentheses and chained (`@p = @q = 1` sets both, measured);
        // and `@h = private attr_reader :h`, whose target is one
        // attribute and whose right side declares a private `h`.
        // Measured: `h` private, every other reader Ruby defines public.
        // Before #1719: na = 3 (@p, @h, bare).
        //
        // expected: na = 13 (grouped, rescued, handler, @on_raise, a, @b,
        // plain, @in_parens, @p, @q, @h, h, bare), npa = 12.
        check_metrics::<RubyParser>(
            "class P\n  (attr_reader :grouped)\n  attr_reader :rescued rescue attr_reader :handler\n  raise \"boom\" rescue @on_raise = 1\n  (attr_reader :a; @b = 1)\n  x = attr_reader :plain\n  y = (@in_parens = 1)\n  @p = @q = 1\n  @h = private attr_reader :h\n  attr_reader :bare\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 13);
                assert_eq!(metric.npa.class_npa_sum(), 12);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attribute_in_a_call_argument_counts() {
        // A call's arguments run at class scope: `memoize(attr_reader
        // :a)` defines `a`, `foo(@x = 1)` sets `@x`, and two decorators
        // deep is no different (measured, with `memoize` and `foo`
        // defined on `Module`: `a`, `b` and `bare` public, `@x` set). An
        // ordinary call governs nothing, so each takes the flag. Before
        // #1719: na = 1 (`bare`).
        //
        // expected: na = 4, npa = 4.
        check_metrics::<RubyParser>(
            "class Arg\n  memoize(attr_reader :a)\n  foo(@x = 1)\n  memoize memoize attr_reader :b\n  attr_reader :bare\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 4);
                assert_eq!(metric.npa.class_npa_sum(), 4);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_only_a_plain_ivar_assignment_is_an_attribute() {
        // The scope `npa/ruby.rs` documents: a plain `@x = …` is an
        // attribute, while an operator assignment (`@c ||= 1`, `@d += 1`)
        // and a multiple assignment (`@a, @b = 1, 2`) are not counted,
        // though Ruby sets each variable. The shared walk reads both as
        // wrappers and asks the `Member` row about their target, so it
        // is the row's `is_assignment_target` gate that keeps them out;
        // `ruby_space_tree_rows` pins the gate, this pins npa reading it.
        //
        // expected: na = 1 (`@x`).
        check_metrics::<RubyParser>(
            "class A\n  @c ||= 1\n  @d += 1\n  @a, @b = 1, 2\n  @x = 1\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attribute_in_an_ordinary_call_takes_a_private_flag() {
        // The other half of `ruby_attribute_in_a_call_argument_counts`:
        // an ordinary call governs nothing, so its argument takes the
        // body flag even when that flag is not the default (measured,
        // with `memoize` returning its argument: `a` and `b` private).
        //
        // expected: na = 2, npa = 0.
        check_metrics::<RubyParser>(
            "class P\n  private\n  memoize(attr_reader :a)\n  attr_reader :b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 0);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_shadowed_keyword_does_not_flip_the_attribute_flag() {
        // The issue's second half: after `private = false`, the bare
        // `private` statement is the local, not the marker, so `@x` and
        // `y` stay public — and the method walk already said so of `m`
        // (measured: `A` has `m`, `y` and `y=` public). Before #1719 the
        // attribute walk had no shadow table and read the bare word as a
        // direct child, so npa was 0 while npm was 1. Removing the shadow
        // table fails this.
        //
        // expected: na = 2, npa = 2; nm = 1, npm = 1.
        check_metrics_with_npm::<RubyParser>(
            "class A\n  private = false\n  private\n  @x = 1\n  attr_accessor :y\n  def m; end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                assert_eq!(metric.npm.class_npm_sum(), 1);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_shadowed_keyword_in_a_condition_is_not_a_statement() {
        // The condition-is-not-a-statement control: `if private` reads
        // the local, and the walk reads the `if`'s `then`, never its
        // condition. Measured: `y` public (`x` sits on the branch the
        // falsy local skips). What this pins is that `x` is read under
        // the `if` (na = 1 without the wrapper) and that the later bare
        // `private` is the local (npa = 1 without the shadow table). It
        // cannot discriminate a widened `If` arm that read the condition
        // as a statement: that arm would find the same shadowed local and
        // change nothing, as `is_statement_of`'s comment records.
        //
        // expected: na = 2 (x, y), npa = 2.
        check_metrics::<RubyParser>(
            "class K\n  private = false\n  if private\n    attr_accessor :x\n  end\n  private\n  attr_reader :y\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_marker_inside_a_wrapper_sets_the_attribute_flag() {
        // A marker inside a wrapper's statements runs when the wrapper
        // does: a `public` under `if true` undoes the `private` before
        // it, a `private` in a `begin` demotes what follows, a `public`
        // in a `while` undoes that (measured: `after_branch` public,
        // `after_begin` private, `after_while` public). Each marker moves
        // the flag across npa's public/non-public line, so none of the
        // three can be skipped unseen. Before #1719 all three read
        // private under the first marker.
        //
        // expected: na = 3, npa = 2 (`after_branch`, `after_while`).
        check_metrics::<RubyParser>(
            "class In\n  private\n  if true\n    public\n  end\n  attr_reader :after_branch\n  begin\n    private\n  end\n  attr_reader :after_begin\n  while true\n    public\n    break\n  end\n  attr_reader :after_while\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    /// Asserts each named class's `na` and `npa` sums; the Ruby
    /// re-filing tests (#1399) read several classes off one fixture.
    #[cfg(feature = "ruby")]
    fn assert_class_npa(func_space: &FuncSpace, expected: &[(&str, u64, u64)]) {
        for &(class, na, npa) in expected {
            let npa_stats = &child_space(func_space, class).metrics.npa;
            assert_eq!(npa_stats.class_na_sum(), na, "{class} na");
            assert_eq!(npa_stats.class_npa_sum(), npa, "{class} npa");
        }
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_bare_keyword_in_an_expression_or_argument_is_the_attribute_marker() {
        // Ruby's `private` is a method call wherever it is evaluated:
        // `x = private` and `foo(private)` demote what follows, `y =
        // public` undoes it, and a binding in an argument (`foo(private =
        // 1)`) shadows the word from then on (measured: `U` has
        // `after_assign` private; `E1` has `m` public; `E2` has `d`
        // private). Before #1719 none of the three was read: U npa 3,
        // E2 npa 2.
        //
        // expected: U na 3 / npa 2; E1 na 1 / npa 1; E2 na 2 / npa 1.
        // Asserted per class: in the totals (na 6 / npa 4) a lost
        // argument-position shadow (E1 -1) and a lost argument-position
        // marker (E2 +1) cancel.
        check_func_space::<RubyParser, _>(
            "class U\n  attr_reader :before\n  x = private\n  attr_reader :after_assign\n  y = public\n  attr_reader :after_second\nend\nclass E1\n  foo(private = 1)\n  x = private\n  attr_reader :m\nend\nclass E2\n  attr_reader :a\n  foo(private)\n  attr_reader :d\nend\n",
            "foo.rb",
            |func_space| assert_class_npa(&func_space, &[("U", 3, 2), ("E1", 1, 1), ("E2", 2, 1)]),
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_destructured_for_and_rescue_bindings_shadow_the_attribute_keyword() {
        // Each binding form the walk models shadows the keyword — a
        // destructuring target flat, nested or splatted, a `for` variable,
        // a `rescue =>` variable — so the later `x = private` is the local
        // and `visible` / `v` stay public; an instance-variable target
        // binds no local, so `F`'s `demoted` is private (measured: as
        // listed). `F`'s `@private = 1` is itself an attribute, set
        // public. Not modelled: a pattern-matching binding (`in [Integer
        // => private, *]`, `1 => private`) and a regexp named capture
        // create the local too, and the walk still reads a later bare
        // `private` as the marker there.
        //
        // expected: na = 7 (visible ×4, @private, demoted, v), npa = 6.
        check_metrics::<RubyParser>(
            "class D\n  private, other = false, nil\n  x = private\n  attr_reader :visible\nend\nclass N1\n  (a, private) = [1, false]\n  x = private\n  attr_reader :visible\nend\nclass N2\n  *private = [false]\n  x = private\n  attr_reader :visible\nend\nclass E\n  for private in [false]\n  end\n  y = private\n  attr_reader :visible\nend\nclass F\n  @private = 1\n  z = private\n  attr_reader :demoted\nend\nclass G\n  begin\n    raise \"x\"\n  rescue => private\n  end\n  w = private\n  attr_reader :v\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 7);
                assert_eq!(metric.npa.class_npa_sum(), 6);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attribute_inside_another_space_is_not_the_class_s() {
        // The boundary the walk stops at: a `def` body, a block, a lambda
        // and a nested `class` / `module` body are spaces of their own.
        // `Inner`'s `@v` is `Inner`'s attribute, counted once, under
        // `Inner`; `M` is a namespace, which `npa` does not count. A walk
        // that entered `Inner` would count `@v` twice, and one that
        // entered `M` would count `q`.
        //
        // Ruby does set `@y` on `S` — a block's `self` is the class — but
        // a block opens a space, as it does for `npm`'s `def`, and the
        // walk does not read it (measured: `S.instance_variables` is
        // `[:@y]`). That gap is recorded here rather than asserted.
        //
        // expected: na = 2 (S's `bare`, Inner's `@v`), npa = 2.
        check_metrics::<RubyParser>(
            "class S\n  def inner\n    @x = 1\n  end\n  [1].each do |i|\n    @y = 1\n  end\n  l = -> { @w = 1 }\n  class Inner\n    @v = 1\n  end\n  module M\n    attr_reader :q\n  end\n  attr_reader :bare\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attribute_under_deeply_nested_calls_does_not_recurse() {
        // The attribute walk is the method walk's explicit stack, so an
        // `attr_reader` under 5,000 nested calls costs no Rust stack; run
        // on a small stack so a reintroduced recursion fails here.
        //
        // expected: na = 2 (deep, bare), npa = 2.
        let nested = format!(
            "class C\n  {}attr_reader :deep{}\n  attr_reader :bare\nend\n",
            "f(".repeat(5_000),
            ")".repeat(5_000)
        );
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || {
                check_metrics::<RubyParser>(&nested, "foo.rb", |metric| {
                    assert_eq!(metric.npa.class_na_sum(), 2);
                    assert_eq!(metric.npa.class_npa_sum(), 2);
                });
            })
            .expect("spawn bounded-stack thread")
            .join()
            .expect("the attribute walk must not overflow a 512 KiB stack");
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attr_in_a_wrapper_argument_counts() {
        // A statement wrapper passed as a keyword's argument runs its
        // `attr_*` at class scope, so each is an attribute: the statement
        // list of `private (attr_reader :x)` (the space before the
        // parenthesis makes it one) and a `begin`.
        //
        // Only the count is pinned, as for `def` in npm's
        // `ruby_def_in_a_wrapper_argument_counts`. Ruby hands the
        // wrapper's last value to `private`, so `x` and `y` are private
        // (measured on ruby 3.0.2), while the walk does not resolve that
        // value and both take the body-wide flag — a known gap, not a
        // contract, and so not asserted.
        //
        // expected: na = 3 (x, y, bare).
        check_metrics::<RubyParser>(
            "class A\n  private (attr_reader :x)\n  private(begin\n    attr_reader :y\n  end)\n  attr_reader :bare\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attribute_and_method_walks_agree_on_visibility() {
        // Grammar-dispatch rule 7's parity: one `attr_reader` beside one
        // `def` in each position the walk reads — under a wrapper after a
        // marker inside it, after an expression-position `public`, after
        // a bare `private` that a local shadows, and after an
        // argument-position `protected` — so the two walks must count
        // the same and agree on which are public. The shadowed `private`
        // sits where the flag is public, so reading it as the marker
        // would demote `rd`/`md`. Measured: `ra`/`ma` private, `rb`/`mb`
        // and `rd`/`md` public, `rc`/`mc` protected. Before #1719 the
        // attribute walk read na = 2.
        //
        // expected: na = nm = 4, npa = npm = 2.
        check_metrics_with_npm::<RubyParser>(
            "class Par\n  if true\n    private\n    attr_reader :ra\n    def ma; end\n  end\n  x = public\n  attr_reader :rb\n  def mb; end\n  private = 1\n  private\n  attr_reader :rd\n  def md; end\n  foo(protected)\n  begin\n    attr_reader :rc\n    def mc; end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 4);
                assert_eq!(metric.npm.class_nm_sum(), 4);
                assert_eq!(metric.npa.class_npa_sum(), 2);
                assert_eq!(metric.npm.class_npm_sum(), 2);
            },
        );
    }

    // --- Ruby attributes a later visibility call names (#1399) ----------
    //
    // An attribute is public iff any accessor it generated is public
    // (the rule `npa/ruby.rs` states). `private :b` names the reader
    // `b` alone and `private :b=` the writer, each re-filed through the
    // ledger `npm` re-files its methods with (`npa/ruby_ledger.rs`).
    // Every expectation was measured on ruby 3.0.2 with
    // `public_instance_methods(false)` and its siblings; the oracle
    // script and its output are recorded on #1399. Before #1399 the
    // symbol form re-filed no attribute, so every class here read npa =
    // na; the one that keeps that value does so by the rule.

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_private_symbol_after_attr_accessor_keeps_the_writer_public() {
        // The issue's own fixture. Measured: `b` private, `b=` public —
        // the writer is untouched, so by the rule the attribute is
        // public. Same value as before #1399 (na = 1, npa = 1), now by
        // the fold rather than by omission: folding with "every
        // accessor" instead of "any" reads 0 here. Deleting the `private
        // :b` line cannot move this test — the rule makes the two
        // fixtures read alike — so the fold perturbation is its only
        // anchor (recorded on #1399).
        //
        // expected: na = 1, npa = 1.
        check_metrics::<RubyParser>(
            "class A1\n  attr_accessor :b\n  private :b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_private_symbol_after_attr_reader_demotes_the_attribute() {
        // The value that moves. `attr_reader :b` generates the reader
        // only, and `private :b` names it (measured: `b` private, nothing
        // public), so no accessor is left public. Before #1399: na = 1,
        // npa = 1 (`bca metrics` at 52d9a34d). Deleting the `private :b`
        // line restores 1.
        //
        // expected: na = 1, npa = 0.
        check_metrics::<RubyParser>(
            "class A2\n  attr_reader :b\n  private :b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 0);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_private_symbol_names_the_reader_not_the_writer() {
        // `private :b` names the reader `b`, which `attr_writer :b` never
        // generated — Ruby raises `NameError` there (measured) and
        // leaves `b=` public — so re-filing `b` finds no accessor and
        // the attribute stays public; `private :b=` names the writer
        // and demotes it; `private :b, :b=` names both of an accessor's
        // and demotes it; `private :b=` under `attr_accessor :b` leaves
        // the reader public. Measured per class: A3 public `[b=]`; A4
        // private `[b=]`; A5 private `[b, b=]`; A18 public `[b]`,
        // private `[b=]`. Keying the ledger by the bare name — reading
        // `:b=` as `b` — would demote A3 and promote A4.
        //
        // expected: A3 na 1 / npa 1; A4 1 / 0; A5 1 / 0; A18 1 / 1.
        check_func_space::<RubyParser, _>(
            "class A3\n  attr_writer :b\n  private :b\nend\nclass A4\n  attr_writer :b\n  private :b=\nend\nclass A5\n  attr_accessor :b\n  private :b, :b=\nend\nclass A18\n  attr_accessor :b\n  private :b=\nend\n",
            "foo.rb",
            |func_space| {
                assert_class_npa(
                    &func_space,
                    &[("A3", 1, 1), ("A4", 1, 0), ("A5", 1, 0), ("A18", 1, 1)],
                );
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attr_initialize_reader_is_auto_private() {
        // The reader of `attr_reader :initialize` is one of the names
        // Ruby makes private as it is defined (#1400), as `def
        // initialize` is; the writer is not, so `attr_accessor
        // :initialize` keeps a public `initialize=` and counts public
        // by the rule. The rule loses to a keyword naming the macro
        // (`public attr_reader :initialize`) and to a later `public
        // :initialize`, but not to a class-method keyword, which names
        // no instance accessor (`private_class_method attr_reader
        // :initialize` leaves the reader auto-private, measured), and
        // does not reach a singleton class. Measured per class: A6
        // public `[initialize=]`, private `[initialize]`; A6r and P
        // private `[initialize]`; A6w public `[initialize=]`; S2 and
        // S3 public `[initialize]`; S1's singleton class public
        // `[initialize]`.
        //
        // expected: A6 na 1 / npa 1; A6r 1 / 0; A6w 1 / 1; S2 1 / 1;
        // S3 1 / 1; P 1 / 0; S1 1 / 1 (the `class << self` body is
        // S1's child space, and its attribute is booked on S1).
        check_func_space::<RubyParser, _>(
            "class A6\n  attr_accessor :initialize\nend\nclass A6r\n  attr_reader :initialize\nend\nclass A6w\n  attr_writer :initialize\nend\nclass S2\n  public attr_reader :initialize\nend\nclass S3\n  attr_reader :initialize\n  public :initialize\nend\nclass P\n  private_class_method attr_reader :initialize\nend\nclass S1\n  class << self\n    attr_reader :initialize\n  end\nend\n",
            "foo.rb",
            |func_space| {
                assert_class_npa(
                    &func_space,
                    &[
                        ("A6", 1, 1),
                        ("A6r", 1, 0),
                        ("A6w", 1, 1),
                        ("S2", 1, 1),
                        ("S3", 1, 1),
                        ("P", 1, 0),
                        ("S1", 1, 1),
                    ],
                );
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_symbol_refiles_an_attribute_in_either_direction() {
        // `public :b` after `private :b` re-files the reader back;
        // `protected :b` is non-public, as it is for `npm`; and `public
        // :b` promotes an attribute declared under a `private` flag —
        // the flag is seeded first so the assertion cannot pass by a
        // one-way demotion (measured: A7 and A12 public `[b]`, A8
        // protected `[b]`). `private :a, :b` names two of three readers
        // (measured: A9 public `[c]`). The class-method pair names the
        // singleton family and reaches no accessor: `private_class_method
        // :b` beside `def self.b` leaves the reader public, and
        // `public_class_method :b` under a `private` flag leaves it
        // private (measured on ruby 3.0.2: G public `[b]`, H private
        // `[b]`) — dropping the `targets_singleton` guard in
        // `leave_call` moves both.
        //
        // expected: A7 na 1 / npa 1; A8 1 / 0; A12 1 / 1; A9 3 / 1;
        // G 1 / 1; H 1 / 0.
        check_func_space::<RubyParser, _>(
            "class A7\n  attr_reader :b\n  private :b\n  public :b\nend\nclass A8\n  attr_reader :b\n  protected :b\nend\nclass A12\n  private\n  attr_reader :b\n  public :b\nend\nclass A9\n  attr_reader :a, :b, :c\n  private :a, :b\nend\nclass G\n  attr_reader :b\n  def self.b; end\n  private_class_method :b\nend\nclass H\n  private\n  attr_reader :b\n  def self.b; end\n  public_class_method :b\nend\n",
            "foo.rb",
            |func_space| {
                assert_class_npa(
                    &func_space,
                    &[
                        ("A7", 1, 1),
                        ("A8", 1, 0),
                        ("A12", 1, 1),
                        ("A9", 3, 1),
                        ("G", 1, 1),
                        ("H", 1, 0),
                    ],
                );
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_symbol_refiles_the_attributes_declared_before_it() {
        // Ruby's `private :b` reaches back over the body and only over
        // the body: an `attr_reader :b` written after it is a fresh,
        // public definition (measured: T public `[b]`). The walk counts
        // declarations, not the method table, so both count and the
        // second keeps its own bit — as `npm`'s
        // `ruby_visibility_refiles_every_declaration_of_a_name_in_order`.
        //
        // expected: na = 2, npa = 1.
        check_metrics::<RubyParser>(
            "class T\n  attr_reader :b\n  private :b\n  attr_reader :b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_private_is_the_visibility_call_only_by_its_bytes() {
        // The call is told from a local or a method of the same name by
        // the identifier bytes (grammar-dispatch rule 10), and each
        // spelling of the word here is a different thing: `private = 1`
        // binds a local, which shadows the bare word — the later bare
        // `private` is that local, so `d` stays public — while
        // `private(:b)` with an argument list is still the `Module`
        // method, local or not (measured: L public `[a, c, d, private]`,
        // private `[b]`); and `def private` declares an ordinary method
        // named `private`, which `npm` counts and `npa` ignores.
        //
        // expected: na = 4 (a, b, c, d), npa = 3; nm = 1, npm = 1.
        check_metrics_with_npm::<RubyParser>(
            "class L\n  attr_reader :a, :b, :c\n  private = 1\n  private(:b)\n  private\n  attr_reader :d\n  def private; end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 4);
                assert_eq!(metric.npa.class_npa_sum(), 3);
                assert_eq!(metric.npm.class_nm_sum(), 1);
                assert_eq!(metric.npm.class_npm_sum(), 1);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_visibility_call_with_unresolvable_arguments_changes_no_attribute() {
        // The `npa` twin of `npm`'s
        // `ruby_visibility_call_with_unresolvable_arguments_changes_nothing`
        // (#1399 §2, unmodelled by decision): a splat (`private *SYMS`),
        // a bare identifier (`private foo`), a nested call (`private
        // send(:name_of, :a)`), the empty delimited symbol (`private
        // :""`) and an interpolated string (`private "#{x}"`) name
        // accessors this walk cannot resolve, so none re-files `a`.
        //
        // expected: na = 1, npa = 1.
        check_metrics::<RubyParser>(
            "class N\n  attr_reader :a\n  private *SYMS\n  private foo\n  private send(:name_of, :a)\n  private :\"\"\n  private \"#{x}\"\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_string_argument_names_an_accessor() {
        // `private "b"` is `private :b` (#1399 §3; measured: A10 private
        // `[b]`), in either quote, and `attr_reader "f"` is `attr_reader
        // :f` (measured: public `[f]`) — the macro and the keyword read
        // an argument's names through one helper, so the string spelling
        // declares where it re-files (before #1399 it declared nothing).
        // A string that is not one plain literal names nothing: `"d#{x}"`
        // carries an `interpolation`, `"e\n"` an `escape_sequence` beside
        // its content — Ruby raises `NameError` on it (measured) — so `d`
        // and `e` stay public.
        //
        // expected: na = 5 (b, c, d, e, f), npa = 3 (d, e, f).
        check_metrics::<RubyParser>(
            "class A10\n  attr_reader :b, :c, :d, :e\n  attr_reader \"f\"\n  private \"b\", 'c'\n  private \"d#{x}\"\n  private \"e\\n\"\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 5);
                assert_eq!(metric.npa.class_npa_sum(), 3);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_nested_visibility_calls_compose_over_an_attr_macro() {
        // `private public attr_accessor :x` on ruby 3.1+: `attr_accessor`
        // returns `[:x, :x=]` (3.0+), `public` declares both public and
        // returns its argument, and the outer `private` demotes what it
        // is handed — the names the macro passed up through `public`.
        // The ordinary call in `private memoize attr_reader :y` forwards
        // nothing, so `y` takes the inner keyword's absence: public, the
        // gap `npa/ruby.rs` records. 3.0.2 raises `TypeError` on the
        // nested call, as for `npm`'s
        // `ruby_nested_visibility_calls_compose`; the 3.1 source is
        // `set_visibility` in `vm_method.c`.
        //
        // expected: na = 2 (x, y), npa = 1 (y).
        check_metrics::<RubyParser>(
            "class N\n  private public attr_accessor :x\n  private memoize attr_reader :y\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_attribute_and_method_walks_agree_on_the_symbol_form() {
        // Grammar-dispatch rule 7's parity for the retroactive form: one
        // `attr_reader` beside one `def` under each keyword's symbol
        // spelling — `private :ra, :ma`, `public "rb", "mb"` promoting
        // out of a `private` flag, `protected :rc, :mc` demoting out of
        // a `public` one — so the two walks re-file the same names the
        // same way, and each pair's re-file crosses the flag it was
        // declared under. Measured: `ma` / `ra` private, `mb` / `rb`
        // public, `mc` / `rc` protected.
        //
        // expected: na = nm = 3, npa = npm = 1.
        check_metrics_with_npm::<RubyParser>(
            "class Par\n  attr_reader :ra\n  def ma; end\n  private :ra, :ma\n  private\n  attr_reader :rb\n  def mb; end\n  public \"rb\", \"mb\"\n  public\n  attr_reader :rc\n  def mc; end\n  protected :rc, :mc\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npm.class_nm_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 1);
                assert_eq!(metric.npm.class_npm_sum(), 1);
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

    #[cfg(feature = "python")]
    #[test]
    fn python_empty_class_no_attributes() {
        check_metrics::<PythonParser>("class C:\n    pass\n", "foo.py", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.class_npa_sum(), 0);
            assert_eq!(metric.npa.interface_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "python")]
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

    #[cfg(feature = "python")]
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

    #[cfg(feature = "python")]
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

    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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

    #[cfg(feature = "python")]
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

    #[cfg(feature = "python")]
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

    #[cfg(feature = "python")]
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

    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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

    /// A `self.` attribute under a non-pattern node on the target side —
    /// the receiver of `self.x.y = …`, the index of `a[self.i] = …` —
    /// is a read, not a binding. The walk never hands the inner node to
    /// the `Member` row (it stops at the outer attribute / subscript,
    /// which the row declines by receiver or kind), so this pins the
    /// consumer's traversal; the row's own wrapper-kind check is pinned
    /// with each node's real chain by
    /// `member_declines_a_self_attribute_under_a_non_pattern_node` in
    /// `roles/python.rs`.
    #[cfg(feature = "python")]
    #[test]
    fn python_self_attribute_under_a_non_pattern_node_is_not_bound() {
        check_metrics::<PythonParser>(
            "class C:\n\
             \x20   def __init__(self):\n\
             \x20       self.x.y = 1\n\
             \x20       a[self.i] = 2\n\
             \x20       self.k = 3\n",
            "foo.py",
            |metric| {
                // Only `k`.
                assert_eq!(metric.npa.class_na_sum(), 1);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    /// A target nested thousands of brackets deep parses, and the
    /// element walk overflowed the consumer thread's stack at about
    /// 8,000 levels before it was made iterative (as `binds` was in
    /// #1597). Run on a thread with a small stack so the depth that
    /// used to abort is reached well inside the test's budget; the two
    /// attributes are still counted. expected: `self.a` under 4,000
    /// parentheses and `self.b` beside it → 2, both public.
    #[cfg(feature = "python")]
    #[test]
    fn python_deeply_nested_unpacking_target_does_not_overflow_the_stack() {
        let depth = 4_000;
        let source = format!(
            "class C:\n    def __init__(self):\n        {}self.a{}, self.b = 1, 2\n",
            "(".repeat(depth),
            ")".repeat(depth)
        );
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || {
                check_metrics::<PythonParser>(&source, "foo.py", |metric| {
                    assert_eq!(metric.npa.class_na_sum(), 2);
                    assert_eq!(metric.npa.class_npa_sum(), 2);
                });
            })
            .expect("spawn bounded-stack thread")
            .join()
            .expect("the target walk must not overflow a 512 KiB stack");
    }

    /// #412 (c): a multi-target class-level assignment binds one
    /// attribute per name. `a = b = 3` (chained) binds two; `p, q = 1,
    /// 2` (unpacking) binds two; with `x = 1` that is five names. The
    /// prior code counted one per `=` statement, reporting 3.
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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
    #[cfg(feature = "python")]
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

    #[cfg(feature = "rust")]
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

    #[cfg(feature = "rust")]
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

    #[cfg(feature = "rust")]
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

    /// An anonymous struct's `field_declaration_list` hangs off a
    /// `struct_specifier` that is no space of its own, so its field is
    /// counted — on the unit, where the walk's merge lands it — and the
    /// class's field beside it on the class; both public here, one
    /// struct-default and one explicit.
    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_anonymous_struct_fields_count_on_the_unit() {
        check_metrics::<CppParser>(
            "struct { int a; } anon;\nclass C { public: int b; };\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    /// The `Member` row, not "every named child of the field list",
    /// decides what is a field: a comment and an attribute inside the
    /// body are named children too, and a filter that admitted them
    /// scored 4 here (perturbation-verified).
    #[cfg(feature = "rust")]
    #[test]
    fn rust_struct_body_comment_and_attribute_are_not_fields() {
        check_metrics::<RustParser>(
            "struct S {\n\
             \x20   // a comment\n\
             \x20   #[allow(dead_code)]\n\
             \x20   pub a: i32,\n\
             \x20   b: i32,\n\
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[cfg(feature = "rust")]
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

    #[cfg(feature = "rust")]
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

    #[cfg(feature = "rust")]
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

    #[cfg(feature = "rust")]
    #[test]
    fn rust_tuple_struct_comments_between_fields_are_not_fields() {
        // A comment of either kind between positional fields is skipped
        // without resetting the pending `pub` (the `Comment` row's kind
        // table, #1699): 3 fields, 2 public. Counting a comment as a
        // field reads 5 / 3 — or 4 / 2 with the `pub` reset.
        check_metrics::<RustParser>(
            "struct Bar(pub /* a */ i32, // b\n pub u8, /* c */ String);",
            "foo.rs",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 3);
                assert_eq!(metric.npa.class_npa_sum(), 2);
            },
        );
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_unit_struct_has_no_attributes() {
        // `struct Empty;` is a unit struct (no fields). 0 attributes.
        check_metrics::<RustParser>("struct Empty;", "foo.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_empty_struct_body_has_no_attributes() {
        // `struct Empty {}` is named-field with zero fields.
        check_metrics::<RustParser>("struct Empty { }", "foo.rs", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "rust")]
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

    #[cfg(feature = "rust")]
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

    #[cfg(feature = "rust")]
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

    #[cfg(feature = "rust")]
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

    #[cfg(feature = "go")]
    #[test]
    fn go_empty_unit_no_attributes() {
        // Package-only file declares no struct → npa stays disabled,
        // class_na_sum = 0.
        check_metrics::<GoParser>("package main\n", "empty.go", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "go")]
    #[test]
    fn go_empty_struct_has_no_attributes() {
        // `type Empty struct{}` has an empty FieldDeclarationList →
        // 0 fields → npa stays disabled.
        check_metrics::<GoParser>("package main\ntype Empty struct{}\n", "foo.go", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "go")]
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

    #[cfg(feature = "go")]
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

    /// The `Member` row, not "every named child of the field list",
    /// decides what is a field: a comment inside the body is a named
    /// child too, and a filter that admitted it scored 3 here
    /// (perturbation-verified).
    #[cfg(feature = "go")]
    #[test]
    fn go_struct_body_comment_is_not_a_field() {
        check_metrics::<GoParser>(
            "package p\ntype S struct {\n\t// a comment\n\tA int\n\tb string\n}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.npa.class_na_sum(), 2);
                assert_eq!(metric.npa.class_npa_sum(), 1);
            },
        );
    }

    #[cfg(feature = "go")]
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

    #[cfg(feature = "go")]
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

    #[cfg(feature = "go")]
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

    #[cfg(feature = "go")]
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
    #[cfg(feature = "elixir")]
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

    #[cfg(feature = "elixir")]
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

    #[cfg(feature = "elixir")]
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

    #[cfg(feature = "elixir")]
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

    #[cfg(feature = "elixir")]
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
    #[cfg(feature = "elixir")]
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

    #[cfg(feature = "objc")]
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

    #[cfg(feature = "objc")]
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

    #[cfg(feature = "cpp")]
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

    #[cfg(feature = "cpp")]
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

    #[cfg(feature = "cpp")]
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

    #[cfg(feature = "cpp")]
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

    #[cfg(feature = "cpp")]
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

    #[cfg(feature = "cpp")]
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

    #[cfg(feature = "cpp")]
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

    #[cfg(feature = "cpp")]
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

    #[cfg(feature = "cpp")]
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

    #[cfg(feature = "javascript")]
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

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_empty_class_no_attributes() {
        // A class with no body and no fields has zero attributes.
        check_metrics::<JavascriptParser>("class Foo {}", "foo.js", |metric| {
            assert_eq!(metric.npa.class_na_sum(), 0);
            assert_eq!(metric.npa.class_npa_sum(), 0);
            insta::assert_json_snapshot!(metric.npa);
        });
    }

    #[cfg(feature = "javascript")]
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

    #[cfg(feature = "javascript")]
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

    #[cfg(feature = "javascript")]
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

    #[cfg(feature = "javascript")]
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

    #[cfg(feature = "mozjs")]
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
    #[cfg(all(
        feature = "csharp",
        feature = "java",
        feature = "kotlin",
        feature = "php"
    ))]
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
    #[cfg(all(feature = "csharp", feature = "java"))]
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
    #[cfg(feature = "java")]
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
