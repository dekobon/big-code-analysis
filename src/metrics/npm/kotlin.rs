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

// A primary constructor's visibility is written on the constructor
// itself (`class F private constructor(x: Int)`), read by the shared
// helper off its `modifiers`. Two class modifiers cap it: a `sealed`
// class's constructors are `protected` by default and may be written
// only `private` or `protected` (the compiler rejects any other), and
// an `enum class`'s are always `private`. Neither is a public method,
// whatever is written, so the written visibility is not consulted.
fn kotlin_primary_constructor_is_public(class: &Node, ctor: &Node) -> bool {
    let restricted_class = class
        .first_child(|id| id == Kotlin::Modifiers)
        .is_some_and(|mods| {
            mods.children().any(|m| {
                m.kind_id() == Kotlin::ClassModifier
                    // role-bypass: allow(Modifier) — which class modifier,
                    // not whether the token is one
                    && m.first_child(|id| matches!(id.into(), Kotlin::Sealed | Kotlin::Enum))
                        .is_some()
            })
        });
    !restricted_class && super::npa::kotlin_is_public(ctor)
}

// Kotlin's default visibility is `public` (unlike Java, which is
// package-private-by-default), so the "no modifier → public" branch is the
// common case.
impl Npm for KotlinCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Kotlin::*;

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
        // The primary constructor is a child of the declaration, not of
        // the body (#1828): counted here, where the walk is still on the
        // class space — its own function space opens one node later.
        // The grammar hangs it off `class_declaration` only
        // (`object_declaration` has none), and also accepts one under
        // the `interface` keyword — `interface I(x: Int)` — which Kotlin
        // rejects, so that shape is billed to the class bucket as any
        // other and not told apart (grammar-dispatch §6: invalid input,
        // no test).
        // role-bypass: allow(Container) — which declaration holds the
        // constructor, not whether it opens a space
        if node.kind_id() == ClassDeclaration
            && let Some(ctor) = node.first_child(|id| id == PrimaryConstructor)
        {
            stats.class_nm += 1;
            if kotlin_primary_constructor_is_public(node, &ctor) {
                stats.class_npm += 1;
            }
        }
        if !matches!(node.kind_id().into(), ClassBody) {
            return;
        }
        let is_interface = super::npa::kotlin_class_body_is_interface(node, ancestors);
        // tree-sitter-kotlin elides the `class_member_declaration` and
        // `declaration` rule layers, so function declarations and
        // secondary constructors appear as direct children of
        // `class_body`. The callable test recognises both kinds.
        let under = ancestors.of_children(node);
        for func in direct_child_funcs::<Self>(node, code, under.get()) {
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
