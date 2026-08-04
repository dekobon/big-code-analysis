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

impl Npa for KotlinCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Kotlin::*;

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
                let is_interface = kotlin_class_body_is_interface(node, ancestors);
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
