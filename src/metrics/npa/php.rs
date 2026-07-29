//! `Npa` implementation for PHP.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Npa for PhpCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Php::*;

        // Enables the `Npa` metric if computing stats of a class-like space.
        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        // Class / trait / anonymous-class / interface bodies all share
        // the `DeclarationList` kind; the parent kind disambiguates.
        //
        // Enum bodies (`EnumDeclarationList`) are deliberately NOT handled
        // and contribute no npa attributes. Enum *cases* are sum-type
        // tags, not data fields, so they are excluded — matching the Java,
        // Kotlin, Rust, and C# convention (see the Rust impl's comment and
        // #781). The only other declarable members are `const`s and
        // methods: PHP enums cannot declare instance properties, and
        // class-level `const`s are not counted as attributes outside an
        // enum either (the `ClassDeclaration` arm below counts only
        // `PropertyDeclaration`), so counting enum `const`s here would be
        // inconsistent with PHP's own class-body rule.
        if !matches!(node.kind_id().into(), DeclarationList) {
            return;
        }
        let Some(parent_kind) = ancestors.parent(node).map(|p| p.kind_id().into()) else {
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
                    .filter(|c| matches!(c.kind_id().into(), ConstDeclaration | ConstDeclaration2))
                    .map(|decl| {
                        decl.children()
                            .filter(|n| matches!(n.kind_id().into(), ConstElement | ConstElement2))
                            .count()
                    })
                    .sum();
                stats.interface_na += count;
                stats.interface_npa = stats.interface_na;
            }
            _ => {}
        }
    }
}
