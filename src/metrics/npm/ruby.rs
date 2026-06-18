//! `Npm` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Ruby `Method` and `SingletonMethod` declared directly inside a
// `Class` or `SingletonClass` body count as methods. Visibility flips
// follow the same keyword-marker rule as `Npa`: a bare `private`
// `public` `protected` `Identifier` child of the body changes the
// running visibility for every subsequent declaration. The
// argument-form (`private :foo`, `private def x`) does NOT flip the
// body-wide flag — matching Ruby's runtime semantics.
//
// `Module` bodies are not classes (the getter routes them to
// `SpaceKind::Namespace`); they do not contribute to `Npm` so a
// module-only file reports zero methods.
impl Npm for RubyCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        use Ruby::*;

        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        if !matches!(node.kind_id().into(), BodyStatement | BodyStatement2) {
            return;
        }
        let Some(parent_kind) = node.parent().map(|p| p.kind_id().into()) else {
            return;
        };
        if !matches!(parent_kind, Class | SingletonClass) {
            return;
        }

        let mut visibility = super::npa::RubyVisibility::Public;
        for child in node.children() {
            if let Some(marker) = super::npa::ruby_visibility_marker(&child, code) {
                visibility = marker;
                continue;
            }
            if matches!(child.kind_id().into(), Method | SingletonMethod) {
                stats.class_nm += 1;
                if visibility == super::npa::RubyVisibility::Public {
                    stats.class_npm += 1;
                }
            }
        }
    }
}
