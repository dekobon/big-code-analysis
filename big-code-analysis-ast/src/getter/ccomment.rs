//! `Getter` implementation for C comments.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for CcommentCode {
    // Every row is `absent`, so this answers `Unknown` for every node;
    // reading the rows rather than inheriting a default is what lets
    // the trait require the method of every language (#1693).
    keyword_roles!(ccomment);

    // No row of `roles/ccomment.rs` opens a space.
    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::ccomment::opens_space(node, code, ancestors)
    }
}
