//! `Npm` implementation for PHP.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Npm for PhpCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cognitive) — grammar dispatch match, clearest whole
        // The outer match selects the declaration-list kind and the inner one
        // its owning container (class / trait / anonymous class / interface),
        // each arm counting methods under that container's visibility rule.
        // Splitting the arms out would scatter one grammar dispatch across
        // helpers named after nothing but their arm, the same reason every
        // sibling `Abc::compute` carries a marker here.
        use Php::*;

        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            DeclarationList => {
                let Some(parent_kind) = ancestors.parent(node).map(|p| p.kind_id().into()) else {
                    return;
                };
                match parent_kind {
                    ClassDeclaration | TraitDeclaration | AnonymousClass => {
                        for method in direct_child_funcs::<Self>(node) {
                            stats.class_nm += 1;
                            if super::npa::php_is_explicit_public(&method) {
                                stats.class_npm += 1;
                            }
                        }
                    }
                    // Interface methods are implicitly public.
                    InterfaceDeclaration => {
                        let count = direct_child_funcs::<Self>(node).count();
                        stats.interface_nm += count;
                        stats.interface_npm = stats.interface_nm;
                    }
                    _ => {}
                }
            }
            // PHP 8.1 enums can declare regular and static methods.
            EnumDeclarationList => {
                for method in direct_child_funcs::<Self>(node) {
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
