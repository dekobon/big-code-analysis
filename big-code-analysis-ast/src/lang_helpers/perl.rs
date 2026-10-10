//! Perl: telling the `and` and `not` operators, and the file tests,
//! from an auto-quoted key that the grammar lexes as one of them.

use crate::Perl;
use crate::node::{Ancestors, Node};

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

/// Whether `not`, a Perl `not` token whose parent is `parent`, is an
/// auto-quoted hash key rather than the prefix operator.
///
/// FIXME(#1541 upstream): the `not` sibling of #1539 — see
/// [`perl_and_is_operator`]. tree-sitter-perl 1.1.2 lexes a bareword
/// `not` before `=>` or alone in a hash subscript as the keyword, and
/// recovers into one of two shapes:
///
/// - a subscript (`$h{not}`, `$r->{not}`) gives an `ERROR` node holding
///   the token;
/// - a fat-comma key (`(not => 1)`, `{ not => 1 }`) gives the
///   `unary_expression` a real `not 1` gets, with an `ERROR` wrapping
///   the `=>` as the token's next sibling;
/// - a fat-comma key after an earlier keyword key in the same list
///   (`f(and => 1, not => 2)`) gives a `not` applied to a zero-width
///   bareword the recovery invents, with the `=>` beside the whole
///   `unary_expression`. A real `not` always has a written operand.
///
/// `and`'s test does not carry over: `not` is a prefix operator, so it
/// is the first child of its expression whether it is a key or not. The
/// recovery node is the only signal, so this reads it directly. A real
/// `not` that lands inside an `ERROR` for an unrelated reason is billed
/// as a key too; that takes input the grammar already failed to parse.
/// `or` is unaffected — the grammar reads it as an identifier in both
/// positions. Remove this, and every call site, once the pin carries the
/// upstream fix.
#[must_use]
pub fn perl_not_is_key(not: &Node, parent: &Node) -> bool {
    parent.is_error()
        || parent
            .children()
            // The parser marks the `ERROR` it wraps a skipped `=>` in
            // as an extra, so only comments are passed over here.
            .filter(|child| child.is_error() || !child.as_tree_sitter().is_extra())
            .skip_while(|child| child.id() != not.id())
            .nth(1)
            .is_some_and(|next| {
                (next.is_error() && next.is_child(Perl::FatComma as u16))
                    || next.start_byte() == next.end_byte()
            })
}

/// Whether `test`, a Perl `file_handle_operator` whose ancestor chain is
/// `ancestors`, is an auto-quoted `-bareword` key rather than a file
/// test.
///
/// FIXME(#1545 upstream): tree-sitter-perl 1.1.2 lexes the leading `-X`
/// of a dash-prefixed bareword key as a file test. Perl auto-quotes
/// `-foo` before `=>` and alone in a hash subscript — the Tk, CGI and
/// Getopt option-hash idiom — so `$h{-foo}` and `(-text => 1)` hold the
/// strings `"-foo"` and `"-text"`, and no operator. The grammar emits
/// two file-test shapes, which this recognises:
///
/// - `-` and a letter glued to more word characters (`-foo`) becomes a
///   file test on the bareword `oo`. Perl never reads that as a file
///   test, since one needs a non-word character after its letter, so
///   only the position decides: the test is the whole `key` of a hash
///   subscript, or the left of a `=>`. Elsewhere `-foo` is a negated
///   call or a string, and stays as the grammar reads it.
/// - a key that *is* a file test (`(-x => 1)`) swallows the `=>` and
///   the value, wrapping the `=>` in an `ERROR`. A real file test
///   cannot take a `=>` as its operand, so the `ERROR` decides.
///
/// The subscript form of the second shape (`$h{-x}`) recovers into an
/// `ERROR` that drops the `-x` token altogether, so nothing is left to
/// bill. A key whose letter is no file-test letter (`-name`, `-height`)
/// is a third shape, a `-` applied to the bareword, which this does not
/// cover: it still bills the `-` and the word, and ABC counts the word
/// as a call, as every bareword key does. Remove this, and every call
/// site, once the pin carries the upstream fix.
#[must_use]
pub(crate) fn perl_file_test_is_dash_key<'a>(
    test: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    test.children()
        // As in `perl_not_is_key`: the `ERROR` is an extra.
        .find(|child| child.is_error() || !child.as_tree_sitter().is_extra())
        .is_some_and(|operand| {
            if operand.is_error() {
                operand.is_child(Perl::FatComma as u16)
            } else {
                is_glued_word(&operand, test) && is_key_position(test, ancestors)
            }
        })
}

/// Whether `node` is the word of a `-bareword` key that the enclosing
/// file test bills whole, so it bills nothing of its own. `ancestors`
/// is `node`'s chain.
#[must_use]
pub fn perl_is_dash_key_word<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.iter(node).next().is_some_and(|(test, above)| {
        test.kind_id() == Perl::FileHandleOperator as u16
            && is_glued_word(node, &test)
            && perl_file_test_is_dash_key(&test, above)
    })
}

/// Whether `dash`, a Perl `-` token whose parent is `parent`, is the
/// sign of an auto-quoted `-and` or `-not` key (`$h{-not}`,
/// `(-and => 1)`), which the grammar splits into the `-` and the keyword
/// token. [`perl_and_is_operator`] and [`perl_not_is_key`] decide
/// whether the keyword is a key, and a key's sign is part of the string
/// whether or not a space separates the two.
#[must_use]
pub fn perl_dash_signs_keyword_key(dash: &Node, parent: &Node) -> bool {
    // A sign is a prefix `-`, whose parent is the subscript's `ERROR` or
    // a `unary_expression`. An infix `-` is a real subtraction even
    // before an auto-quoted key (`(1 - and => 2)` subtracts the string),
    // and is by far the commonest `-`, so it is answered first.
    if parent.kind_id() == Perl::BinaryExpression as u16 {
        return false;
    }
    // `$h{-not}` keeps the keyword beside the `-`; `(-not => 1)` nests
    // it as the first child of the `-`'s operand.
    next_non_extra(parent, dash).is_some_and(|next| {
        keyword_is_key(&next, parent)
            || first_non_extra(&next).is_some_and(|keyword| keyword_is_key(&keyword, &next))
    })
}

/// Whether `key`, an `and` / `not` key token whose ancestor chain is
/// `ancestors`, is signed by a `-` (`$h{-not}`, `(- and => 1)`), read off
/// the tree as [`perl_dash_signs_keyword_key`] reads it: the `-` sits
/// beside the key, or is the operator of the expression the key leads.
/// The bytes cannot say it: an infix `-` (`$x - and => 2`) or a comment
/// ending in one also precedes a key, and a comment can sit between a
/// sign and its key.
#[must_use]
pub fn perl_key_is_signed<'a>(key: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    let mut up = ancestors.iter(key).map(|(ancestor, _)| ancestor);
    let Some(parent) = up.next() else {
        return false;
    };
    let beside = parent
        .children()
        .filter(|sibling| !sibling.as_tree_sitter().is_extra())
        .take_while(|sibling| sibling.id() != key.id())
        .last();
    if let Some(dash) = beside.filter(|node| node.kind_id() == Perl::DASH as u16) {
        return perl_dash_signs_keyword_key(&dash, &parent);
    }
    up.next().is_some_and(|outer| {
        outer.child_by_field_name("operator").is_some_and(|dash| {
            dash.kind_id() == Perl::DASH as u16 && perl_dash_signs_keyword_key(&dash, &outer)
        })
    })
}

fn keyword_is_key(token: &Node, parent: &Node) -> bool {
    match token.kind_id().into() {
        Perl::And => !perl_and_is_operator(token, parent),
        Perl::Not => perl_not_is_key(token, parent),
        _ => false,
    }
}

// The width of a file-test operator token (`-e`, `-x`, …): the grammar
// lexes it as one hidden anonymous `/-[rwxo…]/` token, a `-` and one
// letter, so no node carries its end byte.
const FILE_TEST_TOKEN_LEN: usize = 2;

// Whether `word` is the bareword the grammar split off a `-bareword`
// key: a word glued to the file-test token starts right after it.
fn is_glued_word(word: &Node, test: &Node) -> bool {
    word.kind_id() == Perl::CallExpressionWithBareword as u16
        && word.start_byte() == test.start_byte() + FILE_TEST_TOKEN_LEN
}

// Whether the `unary_expression` around `test` is the whole key of a
// hash subscript, which the grammar wraps in a `binary_expression` with
// no other child but comments, or is followed by a `=>`.
fn is_key_position<'a>(test: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    let mut up = ancestors.iter(test).map(|(ancestor, _)| ancestor);
    let mut key = up.next();
    let mut parent = up.next();
    if parent
        .is_some_and(|p| p.kind_id() == Perl::BinaryExpression as u16 && non_extra_count(&p) == 1)
    {
        key = parent;
        parent = up.next();
    }
    key.zip(parent).is_some_and(|(key, parent)| {
        matches!(
            parent.kind_id().into(),
            Perl::HashAccessVariableSimple | Perl::HashAccessVariable
        ) || next_non_extra(&parent, &key)
            .is_some_and(|next| next.kind_id() == Perl::FatComma as u16)
    })
}

fn non_extra_count(node: &Node) -> usize {
    node.children()
        .filter(|child| !child.as_tree_sitter().is_extra())
        .count()
}

// The sibling after `child` in `parent`, comments aside.
fn next_non_extra<'a>(parent: &Node<'a>, child: &Node) -> Option<Node<'a>> {
    parent
        .children()
        .filter(|sibling| !sibling.as_tree_sitter().is_extra())
        .skip_while(|sibling| sibling.id() != child.id())
        .nth(1)
}

fn first_non_extra<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    node.children()
        .find(|child| !child.as_tree_sitter().is_extra())
}
