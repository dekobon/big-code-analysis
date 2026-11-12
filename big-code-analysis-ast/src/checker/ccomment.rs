//! `Checker` implementation for C comments.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for CcommentCode {
    // The one `Comment` row of `roles/ccomment.rs`; no `DocComment` row (#1699).
    impl_comment_from_rows!(ccomment);

    // The `FuncOpener` and `Container` rows of `roles/ccomment.rs` are
    // absent: the pass opens no space and names no function.
    impl_byte_less_func_space!(ccomment);

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::ccomment::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func_with_code<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::ccomment::func_opener::is_callable(node, code, ancestors)
    }

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    // The `Call` row is absent: the pass names no call.
    impl_row_predicates!(ccomment, call, is_call, is_call_with_code);
    impl_row_predicates!(ccomment, string_lit, is_string, is_string_with_code);
}
