//! `Npa` implementation for C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

/// The C++ attribute walk, one body for the two dialects, as
/// `cpp_family_cognitive!` is for cognitive: Mozcpp is a vendored fork
/// with its own kind ids, pinned equal by `tests/parity/cpp_mozcpp_parity.rs`.
macro_rules! cpp_family_npa {
    ($code:ty, $lang:ident, $member:path) => {
        impl Npa for $code {
            fn compute<'a>(
                node: &Node<'a>,
                code: &'a [u8],
                ancestors: Ancestors<'a, '_>,
                stats: &mut Stats,
            ) {
                use $lang::*;
                use $member as member;

                if !matches!(node.kind_id().into(), FieldDeclarationList) {
                    return;
                }
                // A `field_declaration_list` is never the root, so the
                // guard only keeps this total.
                let Some(parent) = ancestors.parent(node) else {
                    return;
                };
                // C++ `class` defaults to private; `struct` defaults to public.
                // role-bypass: allow(Container) — which container's body this is,
                // for the default access; not whether it opens a space
                let mut current_is_public = match parent.kind_id().into() {
                    ClassSpecifier => false,
                    StructSpecifier => true,
                    _ => return,
                };

                let under = ancestors.of_children(node);
                let members = under.get();
                for child in node.children() {
                    match child.kind_id().into() {
                        AccessSpecifier => {
                            // Update the current visibility to the access
                            // specifier's keyword. `protected` is bucketed with
                            // `private` for `npa` purposes (matches Java's
                            // "non-public" treatment), so any keyword other
                            // than `public` flips us back to private.
                            // role-bypass: allow(Modifier) — which access label, not
                            // whether the token is a modifier
                            current_is_public = child
                                .first_child(|id| {
                                    id == $lang::Public
                                        || id == $lang::Protected
                                        || id == $lang::Private
                                })
                                .is_some_and(|tok| tok.kind_id() == $lang::Public);
                        }
                        // The `Member` row's declaration, asked under its own
                        // chain: the list's plus the list. A templated static
                        // data member (`template_declaration > declaration`)
                        // is neither the row's nor counted here, while the
                        // method consumer reads the same wrapper for a
                        // templated member *function*; FIXME(#1299).
                        _ if member::owns(&child, code, members) => {
                            // Member functions surface as `field_declaration`
                            // when declared without a body. They are counted
                            // by `Npm`, not as attributes — detect them by
                            // their declarator subtree and skip. A
                            // `function_declarator` alone does not settle it:
                            // `int (*fp)(int);` is a function-pointer data
                            // member (#1300), which is why the helper reads
                            // the node's `declarator` field rather than its
                            // kind alone.
                            if cpp_declares_function(&child) {
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
    };
}
pub(super) use cpp_family_npa;

cpp_family_npa!(CppCode, Cpp, crate::roles::cpp::member);
