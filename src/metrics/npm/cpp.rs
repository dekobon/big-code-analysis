//! `Npm` implementation for C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

/// The C++ method walk, one body for the two dialects, as
/// `cpp_family_cognitive!` is for cognitive: Mozcpp is a vendored fork
/// with its own kind ids, pinned equal by `tests/parity/cpp_mozcpp_parity.rs`.
macro_rules! cpp_family_npm {
    ($code:ty, $lang:ident, $func_opener:path) => {
        impl Npm for $code {
            fn compute<'a>(
                node: &Node<'a>,
                code: &'a [u8],
                ancestors: Ancestors<'a, '_>,
                stats: &mut Stats,
            ) {
                use $func_opener as func_opener;
                use $lang::*;

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
                        // Inline-defined member function (with a body): the
                        // `FuncOpener` row's callables — regular methods,
                        // constructors, destructors, operator overloads and
                        // conversion operators all share the aliased
                        // `function_definition` kind-ids; asked under its own
                        // chain, the list's plus the list.
                        _ if func_opener::is_callable(&child, code, members) => {
                            stats.class_nm += 1;
                            if current_is_public {
                                stats.class_npm += 1;
                            }
                        }
                        // Member function reached through a wrapper node
                        // rather than surfacing as a `function_definition`
                        // directly. The wrapper varies by shape:
                        // - `field_declaration > function_declarator` for
                        //   ordinary forward-declared methods (incl. pure
                        //   virtual `= 0` and `Foo* operator->()` wrapped in
                        //   `pointer_declarator`).
                        // - `declaration > function_declarator` for
                        //   constructors / destructors (no return type).
                        // - `template_declaration > declaration >
                        //   function_declarator` for a templated member fn
                        //   declared without a body.
                        // - `template_declaration > function_definition` for a
                        //   templated member fn *with* an inline body. This
                        //   shape was missed until #1258, leaving `npm`
                        //   disagreeing with `nom` and `wmc` on the same class.
                        // - `declaration > operator_cast` for a conversion
                        //   operator declared without a body, and
                        //   `template_declaration > declaration >
                        //   operator_cast` for its templated form. Neither
                        //   carries a `function_declarator` anywhere in its
                        //   subtree. Missed until #1298, which left both forms
                        //   invisible to `npm` *and* `npa`.
                        //
                        // The shared `cpp_declares_function` helper walks the
                        // declarator subtree (including `declaration` wrappers)
                        // so every shape above collapses into one arm. The guard
                        // is what keeps the `template_declaration` payloads
                        // that are *not* member functions out: a nested
                        // templated class (a `type_specifier` the helper does
                        // not descend into), an `alias_declaration`, a
                        // templated static data member (a `declaration` with
                        // no function declarator), and a `friend_declaration`,
                        // whose function is not a member of this class. The
                        // list is not closed: both grammars also admit a
                        // nested `template_declaration` as a child, which the
                        // helper declines because the two-header form it would
                        // spell is not valid C++ at class scope.
                        // role-bypass: allow(Member, DocTarget) — the row's member read
                        // as a bodiless *method* by its declarator, beside the
                        // non-member wrappers; the attribute consumer reads the
                        // same row the other way, and whether each takes a doc
                        // comment is neither's question
                        FieldDeclaration | Declaration | Declaration2 | Declaration3
                        | Declaration4 | TemplateDeclaration
                            if super::npa::cpp_declares_function(&child) =>
                        {
                            stats.class_nm += 1;
                            if current_is_public {
                                stats.class_npm += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    };
}
pub(super) use cpp_family_npm;

cpp_family_npm!(CppCode, Cpp, crate::roles::cpp::func_opener);
