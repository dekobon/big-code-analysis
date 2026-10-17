//! Byte-level helpers for the documentation rules (#1597).

use crate::node::Node;

/// The bytes of `node`'s own span, or nothing when the span is outside
/// `code` — a recovery token can point past the source.
#[inline]
#[must_use]
pub(crate) fn node_bytes<'a>(node: &Node, code: &'a [u8]) -> &'a [u8] {
    code.get(node.start_byte()..node.end_byte()).unwrap_or(&[])
}
