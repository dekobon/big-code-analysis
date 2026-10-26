//! `Checker` implementation for the C preprocessor.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for PreprocCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Preproc::Comment
    }

    impl_row_predicates!(preproc, string_lit, is_string, is_string_with_code);
}
