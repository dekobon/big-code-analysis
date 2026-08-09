//! `Npm` implementation for Mozilla C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Npm for MozcppCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Mozcpp::*;

        if !matches!(node.kind_id().into(), FieldDeclarationList) {
            return;
        }
        let Some(parent) = ancestors.parent(node) else {
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
                    current_is_public = child
                        .first_child(|id| {
                            id == Mozcpp::Public || id == Mozcpp::Protected || id == Mozcpp::Private
                        })
                        .is_some_and(|tok| tok.kind_id() == Mozcpp::Public);
                }
                // Inline-defined member function (with a body): regular
                // methods, constructors, destructors, operator overloads,
                // and conversion operators all share these aliased
                // `function_definition` kind-ids.
                FunctionDefinition | FunctionDefinition2 | FunctionDefinition3
                | FunctionDefinition4 => {
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
                //
                // The shared `cpp_declares_function` helper walks the
                // declarator subtree (including `declaration` wrappers)
                // so all four shapes collapse into one arm. The guard
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
                FieldDeclaration | Declaration | Declaration2 | Declaration3 | Declaration4
                | TemplateDeclaration
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
