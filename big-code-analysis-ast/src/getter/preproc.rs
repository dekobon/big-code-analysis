//! `Getter` implementation for the C preprocessor.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for PreprocCode {
    // Every row is `absent`, so this answers `Unknown` for every node;
    // reading the rows rather than inheriting a default is what lets
    // the trait require the method of every language (#1693).
    keyword_roles!(preproc);

    // No row of `roles/preproc.rs` opens a space.
    impl_byte_less_space_kind!(preproc);

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::preproc::opens_space(node, code, ancestors)
    }
}
