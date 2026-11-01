//! `Checker` implementation for the C preprocessor.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for PreprocCode {
    // The one `Comment` row of `roles/preproc.rs`; no `DocComment` row (#1699).
    impl_comment_from_rows!(preproc);

    impl_row_predicates!(preproc, string_lit, is_string, is_string_with_code);
}
