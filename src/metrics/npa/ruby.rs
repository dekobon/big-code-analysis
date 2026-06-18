//! `Npa` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Npa for RubyCode {
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
        ruby_walk_class_body(node, code, stats);
    }
}
