//! `Npm` implementation for Groovy.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Groovy uses the dekobon grammar, which models all class-like bodies
// as `class_body` and flattens modifiers as direct children of the
// declaration. That rules out the Java macro, so an explicit impl is
// required. The `groovy_body_is_interface_like` and
// `groovy_has_explicit_public` helpers live in `metrics::npa` (Groovy
// section) so both impls share the same parent-kind and modifier
// heuristic.
impl Npm for GroovyCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::metrics::npa::{
            groovy_body_is_interface_like, groovy_has_explicit_public, groovy_record_is_public,
        };
        use Groovy::*;

        match node.kind_id().into() {
            // The record header's implicit canonical constructor
            // (`record_header_is_a_method`, #1842), public when
            // `groovy_record_is_public` says the record is.
            // role-bypass: allow(Container) — a traversal step from the
            // record to the list the `FuncOpener` row then judges, not
            // whether the record opens a space
            RecordDeclaration
                if record_header_is_a_method::<Self>(
                    node,
                    crate::roles::groovy::record_header(node),
                    code,
                    ancestors,
                ) =>
            {
                stats.class_nm += 1;
                stats.class_npm += usize::from(groovy_record_is_public(node, ancestors));
            }
            ClassBody | EnumBody => {
                let is_interface_like = groovy_body_is_interface_like(node, ancestors);
                let under = ancestors.of_children(node);

                for method in direct_child_funcs::<Self>(node, code, under.get()) {
                    if is_interface_like {
                        stats.interface_nm += 1;
                        stats.interface_npm += 1;
                    } else {
                        stats.class_nm += 1;
                        if groovy_has_explicit_public(&method) {
                            stats.class_npm += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
