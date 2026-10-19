//! Perl: telling the `and` and `not` operators, the file tests, the
//! unary minus and bareword calls from an auto-quoted key that the
//! grammar lexes as one of them; and reading the POD in the data
//! section after `__END__`, which the grammar leaves as one leaf.

use crate::Perl;
use crate::lang_helpers::doc_comment::pod_comment_region;
use crate::node::{Ancestors, Node};

/// One classified run of rows in a Perl data section, as reported by
/// [`perl_data_section_rows`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerlDataSectionRows {
    /// A POD block, `first..=last`. `documentation` is false for a block
    /// that is nothing but `=begin comment` / `=for comment` regions.
    Pod {
        /// The block's opening command row.
        first: usize,
        /// Its `=cut` row, or its last non-blank row when no `=cut`
        /// closes it.
        last: usize,
        /// Whether the block documents anything.
        documentation: bool,
    },
    /// A non-blank row outside every POD block: the section's data.
    Data(usize),
}

/// Classifies the rows of a `data_not_for_compiler` node — the text
/// after `__END__` or `__DATA__`, which the grammar holds as one opaque
/// leaf — in one pass over `section`, its bytes. `start_row` is the
/// marker's row, where the node starts; the rest of that row is never
/// reported, since the marker makes the row code and no line there
/// starts with `=`.
///
/// A POD block is read as `Pod::Simple` reads one (`BlackBox.pm`, the
/// parser `perldoc` runs): it opens on a line matching
/// `^=[a-zA-Z][a-zA-Z0-9]*(\s|$)` other than `=cut`, so `=1` opens
/// nothing, and closes on a line matching `^=cut` — its own opening line
/// included, so `=cutting` is a block of one row — or at the end of the
/// file. Blank rows outside a block are reported as nothing.
pub fn perl_data_section_rows(
    section: &[u8],
    start_row: usize,
    mut visit: impl FnMut(PerlDataSectionRows),
) {
    // bca: suppress(halstead) — one line-at-a-time POD state machine:
    // open, close on the opening line itself, extend and data share each
    // row's offsets, and splitting them would scatter Pod::Simple's
    // fall-through across helpers that each re-derive the row.
    // The open block's first row, the byte offset of that row, and its
    // last non-blank row so far.
    let mut block: Option<(usize, usize, usize)> = None;
    let mut lines = section.split(|&byte| byte == b'\n');
    let mut offset = lines.next().map_or(0, |marker_rest| marker_rest.len() + 1);
    for (row, line) in (start_row.saturating_add(1)..).zip(lines) {
        let end = offset + line.len();
        let blank = line.trim_ascii().is_empty();
        // `Pod::Simple` falls through from opening a block to its pod
        // mode, whose `^=cut` test then reads the opening line too.
        let opens = block.is_none() && opens_pod(line);
        if opens {
            block = Some((row, offset, row));
        }
        match block {
            Some((first, start, _)) if line.starts_with(b"=cut") => {
                visit(pod_rows(section, first, start..end, row));
                block = None;
            }
            _ if opens || blank => {}
            Some((first, start, _)) => block = Some((first, start, row)),
            None => visit(PerlDataSectionRows::Data(row)),
        }
        offset = end + 1;
    }
    if let Some((first, start, last)) = block {
        visit(pod_rows(section, first, start..section.len(), last));
    }
}

fn pod_rows(
    section: &[u8],
    first: usize,
    bytes: std::ops::Range<usize>,
    last: usize,
) -> PerlDataSectionRows {
    PerlDataSectionRows::Pod {
        first,
        last,
        documentation: !pod_comment_region(section.get(bytes).unwrap_or_default()),
    }
}

/// `^=[a-zA-Z][a-zA-Z0-9]*(\s|$)`, `=cut` aside: a line that opens a
/// POD block outside one. Perl's `\s` takes a vertical tab, which
/// `is_ascii_whitespace` does not.
fn opens_pod(line: &[u8]) -> bool {
    let Some(rest) = line.strip_prefix(b"=") else {
        return false;
    };
    let word = rest
        .iter()
        .position(|byte| !byte.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    let (command, after) = rest.split_at(word);
    command.first().is_some_and(u8::is_ascii_alphabetic)
        && command != b"cut"
        && after
            .first()
            .is_none_or(|byte| byte.is_ascii_whitespace() || *byte == b'\x0b')
}

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
pub fn perl_not_is_key<'a>(not: &Node<'a>, parent: &Node<'a>) -> bool {
    parent.is_error()
        || parent
            .children_after(not)
            // The parser marks the `ERROR` it wraps a skipped `=>` in
            // as an extra, so only comments are passed over here.
            .find(|child| child.is_error() || !child.as_tree_sitter().is_extra())
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
///   cannot take a `=>` as its operand, so the `ERROR` decides — when
///   the `=>` is on the test's own line. Perl quotes `-x` only when
///   nothing but spaces and tabs separate it from the `=>`: across a
///   newline or a comment (`(-f # c⏎ => 7)`) it stays the file test on
///   `$_`, the list `(-f, 7)`, and the grammar recovers the same
///   `ERROR` regardless. A comment runs to the end of its line, so the
///   row alone tells the two apart. The `-and` / `-not` and `-foo`
///   keys are quoted across either, as Perl does.
///
/// The subscript form of the second shape (`$h{-x}`) recovers into an
/// `ERROR` that drops the `-x` token altogether, so nothing is left to
/// bill. A key whose letter is no file-test letter (`-name`, `-height`)
/// is a third shape, a `-` applied to the bareword, which
/// [`perl_bareword_is_key`] covers. Remove this, and every call site,
/// once the pin carries the upstream fix.
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
                operand.is_child(Perl::FatComma as u16) && operand.start_row() == test.start_row()
            } else {
                is_glued_word(&operand, test)
                    && is_plain_bareword(&operand)
                    && ancestors
                        .iter(test)
                        .next()
                        .is_some_and(|(key, above)| is_key_position(&key, above))
            }
        })
}

/// Whether `node` is the word of a `-bareword` key that the enclosing
/// file test bills whole, so it bills nothing of its own. `ancestors`
/// is `node`'s chain.
#[must_use]
pub(crate) fn perl_is_dash_key_word<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.iter(node).next().is_some_and(|(test, above)| {
        test.kind_id() == Perl::FileHandleOperator as u16
            && is_glued_word(node, &test)
            && perl_file_test_is_dash_key(&test, above)
    })
}

/// Whether `word`, a Perl `call_expression_with_bareword` whose
/// ancestor chain is `ancestors`, is an auto-quoted hash key rather than
/// a call.
///
/// FIXME(#1571 upstream): tree-sitter-perl 1.1.2 parses every bareword
/// key as a call. Perl quotes a word of letters, digits and underscores
/// before `=>` (perlop, "Comma Operator") and alone in a hash subscript
/// (perldata, "Scalar value constructors"), a leading `-` included, so
/// `(name => 1)`, `(-name => 1)`, `$h{-name}` and `$h{-size}` hold the
/// strings `"name"`, `"-name"`, `"-name"` and `"-size"`, and call
/// nothing. Three shapes reach here:
///
/// - the bareword itself before a `=>` (`$h{name}` is an `identifier`
///   with no call around it, so the subscript form never arises);
/// - the operand of a `-` whose `unary_expression` is the key —
///   before a `=>`, or alone in a subscript;
/// - the word of a `-bareword` key the grammar reads as a file test
///   (`-size`), which `perl_is_dash_key_word` decides.
///
/// A package-qualified word (`Foo::Bar => 1`) is not quoted by `=>`
/// and stays a call, as does a word with arguments (`name() => 1`),
/// which the grammar wraps in a call of its own. Remove this, and every
/// call site, once the pin carries the upstream fix.
#[must_use]
pub fn perl_bareword_is_key<'a>(word: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    if perl_is_dash_key_word(word, ancestors) {
        return true;
    }
    if !is_plain_bareword(word) {
        return false;
    }
    match ancestors.iter(word).next() {
        Some((unary, above)) if dash_operator(&unary).is_some() => is_key_position(&unary, above),
        _ => is_key_position(word, ancestors),
    }
}

/// Whether `word`, a Perl `call_expression_with_bareword` whose ancestor
/// chain is `ancestors`, is a call site of its own.
///
/// The grammar nests the callee of every call that takes arguments as a
/// bareword call: `foo(1)`, `foo 1` and `sort { … } @x` each wrap a
/// `call_expression_with_bareword` spelling `foo` or `sort`, so the
/// wrapper is the call and the word inside it is its name. The word is
/// kept for the spellings with no wrapper — `foo;`, `&foo;`, and the
/// `foo(1` that error recovery leaves without one — so the wrapper
/// cannot be dropped instead. Nor is an auto-quoted key a call
/// ([`perl_bareword_is_key`]). ABC's branches and
/// `Checker::is_call_with_code` both ask this, so the two count the same
/// calls.
#[must_use]
pub fn perl_bareword_is_call<'a>(word: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    !ancestors.parent(word).is_some_and(|parent| {
        matches!(
            parent.kind_id().into(),
            Perl::CallExpressionWithSpacedArgs
                | Perl::CallExpressionWithSub
                | Perl::CallExpressionWithArgsWithBrackets
                | Perl::CallExpressionWithVariable
                | Perl::CallExpressionRecursive
        )
    }) && !perl_bareword_is_key(word, ancestors)
}

/// Whether `dash`, a Perl `-` token whose ancestor chain is
/// `ancestors`, is the sign of an auto-quoted key rather than a unary
/// minus: a `-and` / `-not` key ([`perl_dash_signs_keyword_key`]) or a
/// `-bareword` key ([`perl_bareword_is_key`]). A key's sign is part of
/// the string, so it is no operator.
#[must_use]
pub(crate) fn perl_dash_signs_key<'a>(dash: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    perl_dash_signs_keyword_key(dash, ancestors)
        || ancestors.iter(dash).next().is_some_and(|(parent, above)| {
            parent.kind_id() == Perl::UnaryExpression as u16
                && next_non_extra(dash, ancestors).is_some_and(|word| is_plain_bareword(&word))
                && is_key_position(&parent, above)
        })
}

/// The `-` that signs the `-bareword` key `identifier` spells, or
/// `None` when `identifier`, a Perl `identifier` whose ancestor chain is
/// `ancestors`, is no such key's word. The key bills as one operand
/// spelled from the `-` to the word's end, so `(-name => 1)` bills
/// `-name`, the spelling of its quoted twin's contents.
#[must_use]
pub(crate) fn perl_bareword_key_sign<'a>(
    identifier: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    let mut up = ancestors.iter(identifier);
    let (_, word_ancestors) = up
        .next()
        .filter(|(word, _)| word.kind_id() == Perl::CallExpressionWithBareword as u16)?;
    let (unary, _) = up.next()?;
    let dash = dash_operator(&unary)?;
    // `word` and `dash` are siblings, so they share an ancestry.
    perl_dash_signs_key(&dash, word_ancestors).then_some(dash)
}

/// Whether `dash`, a Perl `-` token whose ancestor chain is
/// `ancestors`, is the sign of an auto-quoted `-and` or `-not` key
/// (`$h{-not}`, `(-and => 1)`), which the grammar splits into the `-`
/// and the keyword token. [`perl_and_is_operator`] and
/// [`perl_not_is_key`] decide whether the keyword is a key, and a key's
/// sign is part of the string whether or not a space separates the two.
#[must_use]
pub fn perl_dash_signs_keyword_key<'a>(dash: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    // A sign is a prefix `-`, whose parent is the subscript's `ERROR` or
    // a `unary_expression`. An infix `-` is a real subtraction even
    // before an auto-quoted key (`(1 - and => 2)` subtracts the string),
    // and is by far the commonest `-`, so it is answered first.
    let Some(parent) = ancestors
        .parent(dash)
        .filter(|parent| parent.kind_id() != Perl::BinaryExpression as u16)
    else {
        return false;
    };
    // `$h{-not}` keeps the keyword beside the `-`; `(-not => 1)` nests
    // it as the first child of the `-`'s operand.
    next_non_extra(dash, ancestors).is_some_and(|next| {
        keyword_is_key(&next, &parent)
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
    let mut up = ancestors.iter(key);
    up.next().is_some_and(|(_, parent_ancestors)| {
        let beside = ancestors
            .siblings_before(key)
            .find(|sibling| !sibling.as_tree_sitter().is_extra());
        // A `-` beside the key shares its chain; the `operator` of the
        // expression the key leads is a sibling of that expression.
        if let Some(dash) = beside.filter(|node| node.kind_id() == Perl::DASH as u16) {
            return perl_dash_signs_keyword_key(&dash, ancestors);
        }
        up.next().is_some_and(|(outer, _)| {
            dash_operator(&outer)
                .is_some_and(|dash| perl_dash_signs_keyword_key(&dash, parent_ancestors))
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

// Whether `key`, whose ancestor chain is `ancestors`, is a key where
// Perl auto-quotes one: the whole of a hash subscript, which the grammar
// may wrap in a `binary_expression` with no other child but comments,
// or the last thing before a `=>`. Every auto-quoted key shape asks this
// one question, of the node that spans its whole key.
//
// `=>` quotes the word immediately to its left whatever expression that
// word closes (perlop, "Comma Operator"), so `(1 - name => 2)` subtracts
// the string `"name"`. The grammar nests such a word as the last operand
// of its expression, so the test climbs while the node is its parent's
// last child: the sibling after the first ancestor that has one is the
// token after the word. A key that stopped being the whole of its node
// on the way up is no subscript key (`$h{1 - name}` is not quoted).
fn is_key_position<'a>(key: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    let (mut node, mut node_ancestors) = (*key, ancestors);
    let mut whole = true;
    for (parent, above) in ancestors.iter(key) {
        if whole
            && matches!(
                parent.kind_id().into(),
                Perl::HashAccessVariableSimple | Perl::HashAccessVariable
            )
        {
            return true;
        }
        // As in `perl_not_is_key`, only comments are passed over: the
        // grammar wraps a stray `,` (`(name, => 1)`) in an `ERROR` it
        // marks as an extra, and that `,`, not the word, is then the
        // token the `=>` quotes, which is nothing. A `-foo, =>` file-test
        // key stops here too and reads as the grammar does (a file test
        // on `oo`), as every `-bareword` outside key position does; Perl
        // itself reads one as a string or a negated call by whether a sub
        // of that name is visible, which no single file says.
        if let Some(next) = node_ancestors
            .siblings_after(&node)
            .find(|sibling| sibling.is_error() || !sibling.as_tree_sitter().is_extra())
        {
            return next.kind_id() == Perl::FatComma as u16;
        }
        whole &= parent.kind_id() == Perl::BinaryExpression as u16 && non_extra_count(&parent) == 1;
        (node, node_ancestors) = (parent, above);
    }
    false
}

// Whether `word` is a call of a bare word and nothing else: one
// `identifier`, no package qualifier, no arguments. Only such a word is
// auto-quoted.
fn is_plain_bareword(word: &Node) -> bool {
    if word.kind_id() != Perl::CallExpressionWithBareword as u16 {
        return false;
    }
    // Every call's name is one of these, so the common shape — one
    // child, no comment inside the call — is read off two fields rather
    // than through a cursor over the children.
    match word.child_count() {
        1 => word
            .child(0)
            .is_some_and(|child| child.kind_id() == Perl::Identifier as u16),
        _ => word
            .children()
            .filter(|child| !child.as_tree_sitter().is_extra())
            .map(|child| child.kind_id())
            .eq([Perl::Identifier as u16]),
    }
}

// The `-` of `node` when it is a unary minus — the `unary_expression` a
// `-bareword` key whose letter is no file-test letter parses as — and
// `None` for any other node, a subtraction's `binary_expression`
// included.
fn dash_operator<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    if node.kind_id() != Perl::UnaryExpression as u16 {
        return None;
    }
    node.child_by_field_name("operator")
        .filter(|op| op.kind_id() == Perl::DASH as u16)
}

fn non_extra_count(node: &Node) -> usize {
    node.children()
        .filter(|child| !child.as_tree_sitter().is_extra())
        .count()
}

// The sibling after `node`, whose ancestor chain is `ancestors`,
// comments aside. Every key of a wide `(-a => 1, -b => 2, …)` list or
// a flat `ERROR` asks, so it is found without a scan from the parent's
// start (#1545, #1584).
fn next_non_extra<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> Option<Node<'a>> {
    ancestors
        .siblings_after(node)
        .find(|sibling| !sibling.as_tree_sitter().is_extra())
}

fn first_non_extra<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    node.children()
        .find(|child| !child.as_tree_sitter().is_extra())
}
