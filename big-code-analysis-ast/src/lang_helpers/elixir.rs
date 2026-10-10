//! Elixir: keyword `Call` identity (`def`, `defmodule`, `quote`, …),
//! and applied versus named operators.

use crate::Elixir;
use crate::node::{Ancestors, Node};

/// Reads the text of the `target` field of an Elixir `Call` node.
///
/// Most of Elixir's control-flow constructs (`if`, `unless`, `for`,
/// `while`, `case`, `cond`, `with`, `try`) and method-defining macros
/// (`def`, `defp`, `defmacro`, …) parse as `Call` nodes whose `target`
/// is an `Identifier` whose source text spells the keyword. The
/// `Cyclomatic` and `Exit` impls already follow this pattern; this
/// helper centralises the byte-text lookup so `Cognitive` and `Abc`
/// can share it.
///
/// Returns `None` for Calls whose target is not a simple identifier
/// (e.g. `Module.func(…)` parses as `RemoteCallWithParentheses` with
/// the dotted name as target) or when the bytes are not valid UTF-8.
#[inline]
#[must_use]
pub fn elixir_call_keyword<'a>(node: &'a Node<'a>, code: &'a [u8]) -> Option<&'a str> {
    if node.kind_id() != Elixir::Call as u16 {
        return None;
    }
    let target = node.child_by_field_name("target")?;
    if target.kind_id() != Elixir::Identifier as u16 {
        return None;
    }
    target.utf8_text(code)
}

/// Method-defining macros (`def`, `defp`, `defmacro`, `defmacrop`). The set
/// is duplicated across checker, getter, and several metric impls
/// because each consults it from a different trait surface; centralising
/// the literal here keeps future additions (e.g. `defguard`) consistent.
#[inline]
#[must_use]
pub fn elixir_is_method_macro(kw: &str) -> bool {
    matches!(kw, "def" | "defp" | "defmacro" | "defmacrop")
}

/// Class-defining macro (`defmodule`). Paired with [`elixir_is_method_macro`]
/// where a caller needs both ("any space-opening declaration").
#[inline]
#[must_use]
pub fn elixir_is_class_macro(kw: &str) -> bool {
    kw == "defmodule"
}

/// Returns true when `node` is lexically nested inside the `do_block` of a
/// `quote do … end` Call (Elixir's metaprogramming template). A `def` /
/// `defp` / `defmacro` / `defmacrop` inside `quote` does not define a
/// method of any enclosing module — the syntax tree is a code template
/// emitted later, when the surrounding macro is invoked. Treating those
/// quoted Calls as methods inflates `Wmc` and disagrees with `Npm`'s
/// direct-children classification (#310).
///
/// Walks the ancestor chain looking for a `quote` Call ancestor. Stops at
/// the first match (true) or at the root (false). Each step is a single
/// `child_by_field_name("target")` + identifier byte compare, so the cost
/// is O(steps) when `ancestors` is known — with `Ancestors::unknown` each
/// step additionally pays `Node::parent`'s O(depth) (#1084).
#[inline]
#[must_use]
pub fn elixir_is_inside_quote_block<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors
        .iter(node)
        .any(|(n, _)| elixir_call_keyword(&n, code) == Some("quote"))
}

/// Iterates the direct-child `Call` nodes inside the `do_block` of an
/// Elixir Call (typically a `defmodule`). Used by `Npm` / `Npa` to scan
/// a module body for method-defining macros / `defstruct` without
/// descending into nested modules. Yields no items when the Call has
/// no `do_block`.
#[inline]
pub fn elixir_do_block_call_children<'a>(
    node: &'a Node<'a>,
) -> impl Iterator<Item = Node<'a>> + 'a {
    node.children()
        .filter(|child| child.kind_id() == Elixir::DoBlock as u16)
        .flat_map(|do_block| do_block.children())
        .filter(|stmt| stmt.kind_id() == Elixir::Call as u16)
}

/// The `binary_operator` that applies `op`, an operator token, or `None`
/// when the token is not applied there.
///
/// An operator token also *names* the operator: `&==/2`, `&and/2` and
/// `Kernel.||(a, b)` hold it inside an `operator_identifier`, and the
/// single-token `atom` / `keyword` spell it too. None of those is a
/// `binary_operator` with operands to compare or chain, and under error
/// recovery the parent can be an `ERROR` with no operands, so every
/// decision metric keys on this parent rather than on the token (#1531,
/// #1534). A remote call that names the operator still *applies* it,
/// though; see [`elixir_remote_call_applies`].
///
/// Matched by rule name rather than by `kind_id`: the grammar aliases
/// `binary_operator` to three ids (`Elixir::BinaryOperator` through
/// `BinaryOperator3`), and a name comparison stays correct when a bump
/// adds a fourth (grammar-dispatch §1).
#[must_use]
pub fn elixir_applying_operator<'a>(
    op: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    ancestors
        .parent(op)
        .filter(|parent| parent.kind() == "binary_operator")
}

/// Whether `op`, an operator token, is the name of a remote call on
/// `Kernel` with arguments: `Kernel.||(a, b)`, or `x |> Kernel.||(y)`,
/// which the pipe rewrites to it. Such a call expands the `Kernel` macro
/// exactly as `a || b` does, so it short-circuits and decides as the
/// operator does, where a capture (`&Kernel.||/2`, a call with no
/// arguments) only names it. It has no `binary_operator`, so it carries no
/// `left` / `right` operands either.
///
/// The receiver must spell `Kernel` (or `Elixir.Kernel`): a user module
/// may define `||/2` as an ordinary function, and `Foo.||(a, b)` neither
/// short-circuits nor decides.
///
/// Matched by rule name: `dot` and `arguments` are aliased to several
/// ids each (grammar-dispatch §1).
#[must_use]
pub fn elixir_remote_call_applies<'a>(
    op: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let mut up = ancestors.iter(op).map(|(node, _)| node);
    up.next()
        .is_some_and(|name| name.kind() == "operator_identifier")
        && up.next().is_some_and(|dot| {
            dot.kind() == "dot"
                && dot.child_by_field_name("left").is_some_and(|receiver| {
                    matches!(
                        code.get(receiver.start_byte()..receiver.end_byte()),
                        Some(b"Kernel" | b"Elixir.Kernel")
                    )
                })
        })
        && up.next().is_some_and(|call| {
            call.kind() == "call" && call.children().any(|child| child.kind() == "arguments")
        })
}
