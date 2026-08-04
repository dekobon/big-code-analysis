//! `Npm` implementation for C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Npm for CppCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Cpp::*;

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
                            id == Cpp::Public || id == Cpp::Protected || id == Cpp::Private
                        })
                        .is_some_and(|tok| tok.kind_id() == Cpp::Public);
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
                // Declaration-only member function. The wrapping node
                // varies by shape:
                // - `field_declaration > function_declarator` for
                //   ordinary forward-declared methods (incl. pure
                //   virtual `= 0` and `Foo* operator->()` wrapped in
                //   `pointer_declarator`).
                // - `declaration > function_declarator` for
                //   constructors / destructors (no return type).
                // - `template_declaration > declaration >
                //   function_declarator` for templated member fns.
                //
                // The shared `cpp_has_function_declarator` helper walks
                // the declarator subtree (including `declaration`
                // wrappers) so all three shapes collapse into one arm;
                // the guard avoids counting non-method declarations
                // (e.g. nested type aliases) under the same parent.
                FieldDeclaration | Declaration | Declaration2 | Declaration3 | Declaration4
                | TemplateDeclaration
                    if super::npa::cpp_has_function_declarator(&child) =>
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
