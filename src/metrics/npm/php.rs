//! `Npm` implementation for PHP.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

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
