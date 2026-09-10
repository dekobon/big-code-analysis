//! `Checker` implementation for C comments.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for CcommentCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Ccomment::Comment
    }

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    impl_simple_is_string!(Ccomment, StringLiteral, RawStringLiteral);
}
