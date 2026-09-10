//! Tcl: the leading word of a `command` node.

use crate::Tcl;
use crate::node::Node;

/// Reads the leading word of a Tcl `command` node when it is a plain
/// `simple_word` (`switch`, `for`, `puts`, …). Returns `None` for any other
/// node kind, for commands whose leading word is computed (`$cmd`, `[cmd]`
/// parse it as `variable_substitution` / `command_substitution`, never
/// statically resolvable to a builtin), and for non-UTF-8 bytes. Shared by
/// the out-of-band control-flow detectors below (grammar-dispatch §10:
/// identity questions read the bytes).
///
/// A *literal* name in a quoted or braced spelling is also unresolved, and
/// that is a deliberate limitation: `"for" {set i 0} {$i < 3} {incr i} {…}`
/// and `{for} …` are legal Tcl that still invoke the builtin, but the
/// grammar parses their name as `quoted_word` / `braced_word` rather than
/// `simple_word`, so they score as plain commands. The `simple_word` gate
/// is what keeps the computed forms out; matching the quoted spellings
/// would mean unquoting the bytes for a style no real Tcl uses.
///
/// Callers dispatch on the returned name so each `command` node resolves it
/// exactly once per metric walk — the helpers below take the resolved
/// identity as a precondition rather than re-deriving it.
#[inline]
#[must_use]
pub fn tcl_command_name<'a>(node: &'a Node<'a>, code: &'a [u8]) -> Option<&'a str> {
    if node.kind_id() != Tcl::Command as u16 {
        return None;
    }
    let name = node.child_by_field_name("name")?;
    if name.kind_id() != Tcl::SimpleWord as u16 {
        return None;
    }
    name.utf8_text(code)
}
