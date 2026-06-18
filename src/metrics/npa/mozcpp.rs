//! `Npa` implementation for Mozcpp.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Npa for MozcppCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Mozcpp::*;

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
                            id == Mozcpp::Public || id == Mozcpp::Protected || id == Mozcpp::Private
                        })
                        .is_some_and(|tok| tok.kind_id() == Mozcpp::Public);
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
