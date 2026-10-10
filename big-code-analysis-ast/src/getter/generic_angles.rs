//! The generic angle brackets every grammar with generics shares with
//! its comparison operators (#1559).

use crate::{Ancestors, Node};

/// How [`get_operator_spelling`] renders a generic angle-bracket pair,
/// in every language that has one (#1559).
///
/// [`get_operator_spelling`]: crate::Getter::get_operator_spelling
pub(crate) const ANGLE_PAIR: &str = "<>";

/// The kinds one grammar spells its generic angle brackets with: the
/// list nodes whose `<` / `>` delimit type arguments or parameters
/// rather than compare, and the bracket tokens themselves.
///
/// A list's brackets are one bracket pair, billed like `()`, `[]` and
/// `{}` (#1395): the opener carries the pair as [`ANGLE_PAIR`] and the
/// closer bills nothing (#1559). The grammars give the brackets the
/// same kind ids as the comparison operators, so only the parent tells
/// the two apart, and the opener is keyed by text rather than by kind
/// to keep it out of the comparison `<`'s vocabulary entry.
pub(crate) struct GenericAngleKinds {
    /// The list nodes: type-argument, type-parameter and template
    /// lists, plus each grammar's other spelling of a generic list.
    pub(crate) lists: &'static [u16],
    /// The `<` kinds, with any alias the grammar declares for it.
    pub(crate) openers: &'static [u16],
    /// The `>` kinds, likewise.
    pub(crate) closers: &'static [u16],
    /// Whether a list node, given its ancestor chain, holds comparisons
    /// the grammar misparsed as a generic list, whose brackets then keep
    /// billing as the comparisons they are. [`never_misparsed`] for a
    /// grammar with no such shape this can tell from a real list.
    pub(crate) is_misparse: for<'a, 'b> fn(&Node<'a>, Ancestors<'a, 'b>) -> bool,
}

/// The [`GenericAngleKinds::is_misparse`] of a grammar whose generic
/// lists hold no misparse this can tell apart. C++ is one: tree-sitter-cpp
/// reads `a < b || c > (d)` as a template argument list, but so does a
/// valid template call spelled the same, and only name lookup tells them
/// apart.
pub(crate) fn never_misparsed(_: &Node, _: Ancestors) -> bool {
    false
}

impl GenericAngleKinds {
    fn parent_is_list<'a>(&self, node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        ancestors.iter(node).next().is_some_and(|(list, above)| {
            self.lists.contains(&list.kind_id()) && !(self.is_misparse)(&list, above)
        })
    }

    /// Whether `node` closes a generic list, and so bills nothing.
    pub(crate) fn is_closer<'a>(&self, node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        self.closers.contains(&node.kind_id()) && self.parent_is_list(node, ancestors)
    }

    /// [`ANGLE_PAIR`] when `node` opens a generic list, else `None`.
    pub(crate) fn opener_spelling<'a>(
        &self,
        node: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
    ) -> Option<&'static str> {
        (self.openers.contains(&node.kind_id()) && self.parent_is_list(node, ancestors))
            .then_some(ANGLE_PAIR)
    }
}
