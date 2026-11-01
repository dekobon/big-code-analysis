//! `Checker` implementation for C comments.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for CcommentCode {
    // The one `Comment` row of `roles/ccomment.rs`; no `DocComment` row (#1699).
    impl_comment_from_rows!(ccomment);

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    impl_row_predicates!(ccomment, string_lit, is_string, is_string_with_code);
}
