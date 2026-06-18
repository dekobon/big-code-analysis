//! `Npm` implementation for Kotlin.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Re-uses the visibility helper from the `Npa` impl. Kotlin's default
// visibility is `public`, the opposite of Java's

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
