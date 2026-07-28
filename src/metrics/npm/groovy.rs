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
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use crate::metrics::npa::{groovy_body_is_interface_like, groovy_has_explicit_public};
        use Groovy::*;

        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            ClassBody | EnumBody => {
                let is_interface_like = groovy_body_is_interface_like(node);

                for method in direct_child_funcs::<Self>(node) {
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
