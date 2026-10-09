//! Perl: telling the low-precedence `and` operator from an auto-quoted
//! `and` key.

use crate::Perl;
use crate::node::Node;

/// Whether `and`, a Perl `and` token whose parent is `parent`, is the
/// low-precedence logical operator.
///
/// FIXME(#1539 upstream): tree-sitter-perl 1.1.2 lexes an auto-quoted
/// bareword `and` as the keyword. Perl quotes a bareword before `=>`
/// and alone inside a hash subscript, so `(and => 1)` and `$h{and}`
/// hold a string key and no operator, yet the grammar emits two
/// shapes that read as one:
///
/// - a subscript (`$h{and}`, `$r->{and}`, `$h{-and}`) recovers into an
///   `ERROR` node holding the token;
/// - a fat-comma key (`(and => 1)`, `{ and => 1 }`, `f(x => 1, and =>
///   2)`) becomes a `unary_expression` whose *first* child is the
///   token — the kind a real `$a and $b` also gets, with `$a` first.
///
/// `and` is an infix operator, so in valid Perl the operator always has
/// a left operand. That is the whole test: the parent is one of the two
/// expression kinds and the token is not its first child, an `extra`
/// aside.
/// `or` and `xor` are unaffected — the grammar reads either as an
/// identifier in both positions. Remove this, and every call site,
/// once the pin carries the upstream fix.
#[must_use]
pub fn perl_and_is_operator(and: &Node, parent: &Node) -> bool {
    matches!(
        parent.kind_id().into(),
        Perl::UnaryExpression | Perl::BinaryExpression
    ) && first_non_extra(parent).is_some_and(|first| first.id() != and.id())
}

/// Whether `expr` is the `unary_expression` tree-sitter-perl builds
/// around an auto-quoted `and` key (`(and => 1)`): its first child,
/// comments aside, is the `and` token itself. See
/// [`perl_and_is_operator`].
#[must_use]
pub fn perl_is_and_key_misparse(expr: &Node) -> bool {
    first_non_extra(expr).is_some_and(|first| first.kind_id() == Perl::And as u16)
}

fn first_non_extra<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    node.children()
        .find(|child| !child.as_tree_sitter().is_extra())
}
