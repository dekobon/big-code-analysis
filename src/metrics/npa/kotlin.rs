//! `Npa` implementation for Kotlin.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

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

// The `val` / `var` parameters of `class`'s primary constructor, if it
// has one: `class_declaration → primary_constructor → class_parameters
// → class_parameter`. A parameter without the keyword is a plain
// constructor argument.
fn kotlin_parameter_properties<'a>(class: &Node<'a>) -> impl Iterator<Item = Node<'a>> {
    use Kotlin::*;
    class
        .first_child(|id| id == PrimaryConstructor)
        .and_then(|ctor| ctor.first_child(|id| id == ClassParameters))
        .into_iter()
        .flat_map(|params| params.children())
        // role-bypass: allow(DeclKeyword, DocTarget) — whether the parameter declares a
        // property, a question about the parameter, not whether the token is a
        // declaration keyword or the parameter a doc target
        .filter(|p| {
            p.kind_id() == ClassParameter
                && p.children()
                    .any(|c| matches!(c.kind_id().into(), Val | Var))
        })
}

impl Npa for KotlinCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Kotlin::*;

        match node.kind_id().into() {
            // A `ClassParameter` carrying `val` / `var` is a Kotlin
            // primary-constructor parameter property — an attribute of
            // the class, counted once toward it. Parameters without
            // `val`/`var` are plain constructor arguments, not
            // attributes. Read at the class declaration rather than at
            // each parameter: the primary constructor opens a function
            // space of its own (#1828), so a parameter is visited inside
            // that space and a count there would make the property the
            // constructor's — the same reason TypeScript scans a
            // constructor's parameter properties from the class body.
            // role-bypass: allow(Container) — which declaration holds the
            // constructor, not whether it opens a space
            ClassDeclaration => {
                for param in kotlin_parameter_properties(node) {
                    stats.class_na += 1;
                    if kotlin_is_public(&param) {
                        stats.class_npa += 1;
                    }
                }
            }
            // Every `ClassBody` we visit attributes its direct
            // `property_declaration` children to whichever func_space is
            // currently on the state stack. A companion object, a nested
            // class and a nested interface each open their own class-like
            // space (handled by `spaces.rs`), so their attributes are
            // counted there and reach the outer space only through its
            // sums.
            ClassBody => kotlin_count_body_properties(node, code, ancestors, stats),
            _ => {}
        }
    }
}

// The properties declared in a class or interface body.
// tree-sitter-kotlin elides the `class_member_declaration` and
// `declaration` rule layers when those rules are pure forwarding
// choices, so property declarations appear as direct children of
// `class_body`. Each member is asked under its own chain, the body's
// plus the body. One visibility rule serves both buckets: an interface
// property is public by default and may be `private` when it has an
// accessor body (the compiler's
// `compiler/testData/diagnostics/tests/modifiers/privateInInterface.kt`).
fn kotlin_count_body_properties<'a>(
    body: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    use crate::roles::kotlin::member;
    let is_interface = kotlin_class_body_is_interface(body, ancestors);
    let under = ancestors.of_children(body);
    let members = under.get();
    for prop in body.children().filter(|c| member::owns(c, code, members)) {
        let attrs = kotlin_count_property_attrs(&prop);
        let public = if kotlin_is_public(&prop) { attrs } else { 0 };
        if is_interface {
            stats.interface_na += attrs;
            stats.interface_npa += public;
        } else {
            stats.class_na += attrs;
            stats.class_npa += public;
        }
    }
}
