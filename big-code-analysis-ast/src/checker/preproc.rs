//! `Checker` implementation for the C preprocessor.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for PreprocCode {
    // The one `Comment` row of `roles/preproc.rs`; no `DocComment` row (#1699).
    impl_comment_from_rows!(preproc);

    // The `FuncOpener` and `Container` rows of `roles/preproc.rs` are
    // absent: the pass opens no space and names no function.
    impl_byte_less_func_space!(preproc);

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::preproc::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func_with_code<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::preproc::func_opener::is_callable(node, code, ancestors)
    }

    // The `Call` row is absent: the pass names no call.
    impl_row_predicates!(preproc, call, is_call, is_call_with_code);
    impl_row_predicates!(preproc, string_lit, is_string, is_string_with_code);
}
