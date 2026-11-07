//! Doxygen's attachment rule for C, C++, Mozilla C++ and Objective-C
//! (#1613): which doc blocks document the entry after them, which stand
//! alone, and which document nothing. Read from `commentscan.l` and
//! checked by running Doxygen 1.13.2 (`EXTRACT_ALL=NO`, XML output). The
//! rule reads kind names, which the four grammars share and number
//! differently.

use crate::checker::Checker;
use crate::lang_helpers::doc_comment::{node_bytes, outside_function_bodies};
use crate::node::{Ancestors, MemoQuestion, Node, NodeMemo};

/// A Doxygen block written after the member it documents: `/**<`,
/// `/*!<`, `///<` or `//!<`. It documents the code before it, so it never
/// binds forward (#1613).
#[inline]
#[must_use]
pub(crate) fn doxygen_member_after(s: &[u8]) -> bool {
    [&b"/**<"[..], b"/*!<", b"///<", b"//!<"]
        .iter()
        .any(|marker| s.starts_with(marker))
}

/// A Doxygen block that stands alone: one carrying a command that
/// `commentscan.l` turns into an entry of its own
/// (`makeStructuralIndicator`), so it documents that entry rather than the
/// code after it, and documents it even with nothing after it (#1613).
/// `\file`, `\mainpage`, `\page`, `\dir`, `\example`, `\defgroup`,
/// `\addtogroup`, `\weakgroup`, `\name` and `\package` always do; the
/// commands that name an entity (`\fn`, `\class`, `\def`, …) only with an
/// argument on their line, since without one Doxygen warns and documents
/// the declaration after the block. `\brief`, `\ingroup` and the rest do
/// not. Checked against Doxygen 1.13.2: a command counts wherever it
/// sits in the text, even inside `<tt>` or after a word (`foo@file`), but
/// not escaped (`\\file`, `@@file`), in a mail address, a backtick span, a
/// fenced block, an HTML comment, a format block (`\code`,
/// `\verbatim`, `\htmlonly`, `\dot`, …), text quoted with `"` on one
/// line, or a `<![CDATA[` section (`commentscan.l`, the `<Comment>`
/// rules for quoted text and `<![CDATA[`; the last two from the source,
/// #1646). Commands match case sensitively, as Doxygen's command table
/// does.
#[must_use]
pub(crate) fn doxygen_standalone(s: &[u8]) -> bool {
    const ALWAYS: &[&[u8]] = &[
        b"file",
        b"mainpage",
        b"page",
        b"dir",
        b"example",
        b"defgroup",
        b"addtogroup",
        b"weakgroup",
        b"name",
        b"package",
    ];
    const NAMED: &[&[u8]] = &[
        b"fn",
        b"var",
        b"def",
        b"typedef",
        b"property",
        b"overload",
        b"enum",
        b"struct",
        b"union",
        b"class",
        b"namespace",
        b"interface",
        b"protocol",
        b"concept",
        b"idlexcept",
        b"module",
    ];
    let mut at = 0;
    while at < s.len() {
        if let Some(end) = literal_end(s, at) {
            at = end;
            continue;
        }
        let Some(word) = doxygen_command(s, at) else {
            at += 1;
            continue;
        };
        at += 1 + word.len();
        if ALWAYS.contains(&word) || (NAMED.contains(&word) && names_something(&s[at..])) {
            return true;
        }
    }
    false
}

/// Where the text Doxygen does not read for commands ends, when such text
/// starts at `s[at]`: a backtick span closed before the next blank line
/// (Markdown's `processCodeSpan`), text quoted with `"` and closed on
/// the same line — the scanner's quoted-text rule is `"\""[^"\n]*"\""`,
/// so an unclosed `"` is one character and the command after it is read
/// (#1646) — an escaped `\\` / `@@`, a fence, an HTML comment or a
/// `<![CDATA[` section ([`delimited_end`]; the comment is the one of
/// the three that is not a plain open/close pair, since a `-->` whose
/// `>` follows a third `-` does not close it), or a format block
/// ([`format_block_end`]). The
/// fence is asked before the span so ```` ``` ```` is never read as a
/// backtick span opened on an empty one.
fn literal_end(s: &[u8], at: usize) -> Option<usize> {
    let rest = &s[at..];
    if let Some(end) = delimited_end(s, at) {
        return Some(end);
    }
    if let Some(span) = rest.strip_prefix(b"`") {
        return code_span_close(span).map(|close| at + close + 2);
    }
    if let Some(quoted) = rest.strip_prefix(b"\"") {
        return quoted_text_close(quoted).map(|close| at + close + 2);
    }
    if matches!(rest, [b'@', b'@', ..] | [b'\\', b'\\', ..]) {
        return Some(at + 2);
    }
    format_block_end(s, at)
}

/// Where a literal that runs to a closing delimiter or the end of the
/// block ends, when one opens at `s[at]`: a ```` ``` ```` fence, an HTML
/// comment, or a `<![CDATA[` section.
fn delimited_end(s: &[u8], at: usize) -> Option<usize> {
    let rest = &s[at..];
    if rest.starts_with(b"<!--") {
        return Some(html_comment_end(s, at + 4));
    }
    let (open, close): (&[u8], &[u8]) = if rest.starts_with(b"```") {
        (b"```", b"```")
    } else if rest.starts_with(b"<![CDATA[") {
        (b"<![CDATA[", b"]]>")
    } else {
        return None;
    };
    let body = at + open.len();
    let found = s[body..].windows(close.len()).position(|w| w == close);
    Some(found.map_or(s.len(), |found| body + found + close.len()))
}

/// The offset in `quoted` of the `"` closing a quoted text whose opening
/// `"` precedes `quoted`, or `None` when a newline or the end of the
/// comment comes first.
fn quoted_text_close(quoted: &[u8]) -> Option<usize> {
    quoted
        .iter()
        .position(|&b| b == b'"' || b == b'\n')
        .filter(|&close| quoted[close] == b'"')
}

/// The format blocks, each opening command beside the one that closes it.
const FORMAT_BLOCKS: &[(&[u8], &[u8])] = &[
    (b"code", b"endcode"),
    (b"icode", b"endicode"),
    (b"verbatim", b"endverbatim"),
    (b"iverbatim", b"endiverbatim"),
    (b"iliteral", b"endiliteral"),
    (b"htmlonly", b"endhtmlonly"),
    (b"latexonly", b"endlatexonly"),
    (b"xmlonly", b"endxmlonly"),
    (b"rtfonly", b"endrtfonly"),
    (b"manonly", b"endmanonly"),
    (b"docbookonly", b"enddocbookonly"),
    (b"dot", b"enddot"),
    (b"msc", b"endmsc"),
    (b"startuml", b"enduml"),
];

/// Where a format block opened by the command at `s[at]` ends: after its
/// end command, or at the end of the comment when it has none.
fn format_block_end(s: &[u8], at: usize) -> Option<usize> {
    let word = doxygen_command(s, at)?;
    let (_, end) = FORMAT_BLOCKS.iter().find(|(open, _)| *open == word)?;
    Some(block_end(s, at, end))
}

/// The offset in `span` of the backtick closing a code span whose opening
/// backtick precedes `span`, or `None` when a blank line (one holding
/// nothing but whitespace and a leading `*`) or the end of the comment
/// comes first.
fn code_span_close(span: &[u8]) -> Option<usize> {
    let mut start = 0;
    for (index, line) in span.split(|&b| b == b'\n').enumerate() {
        if let Some(close) = line.iter().position(|&b| b == b'`') {
            return Some(start + close);
        }
        let text = line.trim_ascii();
        if index > 0
            && text
                .strip_prefix(b"*")
                .unwrap_or(text)
                .trim_ascii()
                .is_empty()
        {
            return None;
        }
        start += line.len() + 1;
    }
    None
}

/// Where an HTML comment whose text starts at `s[from]` ends: after its
/// `-->`, or at the end of the comment when it has none. Doxygen 1.13.2
/// closes it on neither `--->` nor `--!>`.
fn html_comment_end(s: &[u8], from: usize) -> usize {
    (from..s.len())
        .find(|&at| s[at..].starts_with(b"-->") && (at == from || s[at - 1] != b'-'))
        .map_or(s.len(), |at| at + 3)
}

/// Where a format block opened at `s[at]` ends: after its `end` command,
/// or at the end of the comment when it has none.
fn block_end(s: &[u8], at: usize, end: &[u8]) -> usize {
    (at + 1..s.len())
        .find(|&after| doxygen_command(s, after) == Some(end))
        .map_or(s.len(), |after| after + 1 + end.len())
}

/// Whether a naming command has an argument: anything on the rest of its
/// line but the comment's closing `*/`.
fn names_something(after: &[u8]) -> bool {
    let line = after.split(|&b| b == b'\n').next().unwrap_or_default();
    let line = line.trim_ascii();
    !line
        .strip_suffix(b"*/")
        .unwrap_or(line)
        .trim_ascii()
        .is_empty()
}

/// The command word at `s[at]` when a Doxygen command starts there: `@` or
/// `\` followed by letters, the word ending at a non-word byte. An `@` in
/// a mail address (`a@file.com`) is not a command.
fn doxygen_command(s: &[u8], at: usize) -> Option<&[u8]> {
    if !matches!(s.get(at), Some(b'@' | b'\\')) {
        return None;
    }
    let rest = &s[at + 1..];
    let len = rest
        .iter()
        .take_while(|b| b.is_ascii_alphabetic() || **b == b'_')
        .count();
    if len == 0 {
        return None;
    }
    let mail = s[at] == b'@'
        && at > 0
        && s[at - 1].is_ascii_alphanumeric()
        && rest
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
            .skip(len)
            .any(|&b| b == b'.');
    (!mail).then(|| &rest[..len])
}

/// A preprocessor conditional or one of its branches, under any alias of
/// the rule: the nodes Doxygen's preprocessor reads through to the
/// declarations inside. Read by id from the dialect's `DocTarget`
/// conditional owners ([`DoxygenRow::conditional_kind`]), since the four
/// grammars number them differently (#1795).
#[must_use]
pub(crate) fn preproc_conditional<T: DoxygenRow>(node: &Node) -> bool {
    T::conditional_kind(node.kind_id())
}

// Each conditional `conditional_declares` reads the children of. The memo
// changes no answer, so only this count tells a memoised walk from a
// quadratic one.
crate::observation::counter!(conditionals_read);

/// Whether a preprocessor conditional holds, in any branch and at any
/// depth, an entry ([`doxygen_entry`]). Doxygen documents the first
/// declaration of the branch its preprocessor takes; which branch that is
/// depends on macros this walk cannot evaluate, so any declaration in any
/// branch stands in for it (#1613). A conditional holding none is read
/// past, as Doxygen reads past `#ifdef X` `#endif`. Iterative, so a deep
/// nest cannot exhaust the stack.
///
/// A doc comment above each level of a nest asks this of every level, and
/// each answer reads the whole nest below it, so unmemoised the nest costs
/// `O(depth²)`. Under a walk that lent a [`FoldCache`](crate::FoldCache)
/// every conditional's answer is stored, the nested ones' included, so
/// each is read once per walk; any other caller reads the nest afresh.
#[must_use]
pub(crate) fn conditional_declares<T: DoxygenRow>(node: &Node, ancestors: Ancestors) -> bool {
    let unshared = NodeMemo::default();
    let memo = ancestors
        .node_memo(MemoQuestion::ConditionalDeclares)
        .unwrap_or(&unshared);
    if let Some(answer) = memo.get(node) {
        return answer;
    }
    // Post-order: a conditional's answer is complete once each nested one
    // has folded its own in. `answers[i]` holds the answer, so far, of the
    // `i`th conditional read.
    let mut answers = Vec::new();
    let mut stack = vec![Visit::Read {
        conditional: *node,
        outer: None,
    }];
    while let Some(visit) = stack.pop() {
        match visit {
            Visit::Read { conditional, outer } => {
                conditionals_read::record();
                let own = answers.len();
                stack.push(Visit::Answer {
                    conditional,
                    outer,
                    own,
                });
                answers.push(children_declare::<T>(
                    &conditional,
                    ancestors,
                    memo,
                    |nested| {
                        stack.push(Visit::Read {
                            conditional: nested,
                            outer: Some(own),
                        });
                    },
                ));
            }
            Visit::Answer {
                conditional,
                outer,
                own,
            } => {
                memo.set(&conditional, answers[own]);
                if let Some(outer) = outer {
                    answers[outer] |= answers[own];
                }
            }
        }
    }
    memo.get(node) == Some(true)
}

/// A step of [`conditional_declares`]'s walk over a nest.
enum Visit<'a> {
    /// Read the conditional's children ([`children_declare`]).
    Read {
        conditional: Node<'a>,
        /// The enclosing conditional's index in the walk's `answers`;
        /// `None` for the conditional asked about.
        outer: Option<usize>,
    },
    /// Store the conditional's answer, every nested one folded in by now,
    /// and fold it into the enclosing conditional's.
    Answer {
        conditional: Node<'a>,
        outer: Option<usize>,
        /// This conditional's own index in `answers`.
        own: usize,
    },
}

/// Whether a child of `conditional` already answers
/// [`conditional_declares`] for it: an entry, or a nested conditional
/// the memo answers `true` for. Each nested conditional the memo has no
/// answer for is handed to `defer`, so the walk reads it and folds its
/// answer in later. Each child is classified once, here (#1795).
fn children_declare<'a, T: DoxygenRow>(
    conditional: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    memo: &NodeMemo,
    mut defer: impl FnMut(Node<'a>),
) -> bool {
    let parent = [*conditional];
    let below = chain_below(ancestors, &parent);
    let mut declares = false;
    for child in conditional.children() {
        if preproc_conditional::<T>(&child) {
            match memo.get(&child) {
                Some(answer) => declares |= answer,
                None => defer(child),
            }
        } else if !declares {
            declares = non_conditional_entry::<T>(&child, below);
        }
    }
    declares
}

/// The context a conditional's children are asked with: a chain of one
/// entry, the conditional itself, so a gate reading `parent` takes the
/// conditional and not the nest's parent — with the walk's fold cache
/// carried, no sibling cache, and no allocation (the entry is the
/// caller's local). It is `parent` and nothing more: a chain of the
/// real depth would cost a copy of the prefix per conditional, which
/// across independent conditionals at growing depths is quadratic, and
/// no C-family `DocTarget` gate reads past `parent` — one that folded
/// or counted depth through this one-entry chain would answer off the
/// wrong depth, so the first such gate must widen this, not reuse it.
/// With no chain known (`Ancestors::unknown`) the gate climbs, which is
/// exact and allocation-free as well.
fn chain_below<'a, 'c>(
    ancestors: Ancestors<'a, 'c>,
    parent: &'c [Node<'a>; 1],
) -> Ancestors<'a, 'c> {
    if ancestors.is_known() {
        ancestors.below(parent)
    } else {
        ancestors
    }
}

// Doxygen's scanner keeps every doc block it meets until the next entry
// and gives them all to it, so a stretch keeps every doc comment
// (`KeptDocs::All`); what follows is which siblings are entries, which it
// reads past, and where a doc block binds at all.

/// The kinds [`doxygen_container`] names besides the preprocessor
/// conditionals. By kind string, not id: one compare covers every alias
/// of a rule in the four dialects, which number them differently
/// (grammar-dispatch §1). Nothing at compile time ties a string to a
/// grammar, so `doxygen_kind_strings_name_an_emitted_kind`
/// (`roles/doc_tests/doxygen.rs`) holds each entry to the dialects
/// whose parser emits it (#1834).
pub(crate) const DOXYGEN_CONTAINERS: &[&str] = &[
    "translation_unit",
    "template_parameter_list",
    "declaration_list",
    "field_declaration_list",
    "enumerator_list",
    "class_interface",
    "class_implementation",
    "protocol_declaration",
    "qualified_protocol_interface_declaration",
    "instance_variables",
];

/// A node whose children Doxygen reads as a sequence of entries, so a doc
/// comment among them documents the next one: a file, a namespace or
/// `extern "C"` body, a class or enum body, an Objective-C container and a
/// preprocessor branch. A template parameter list holds no entry, so a
/// doc comment in one documents nothing. A doc comment anywhere else sits
/// in a declaration — after `static`, an attribute or a template head, on
/// a parameter — and documents that declaration where it sits (#1597).
/// That includes an initializer list, where Doxygen documents nothing:
/// the grammars read Apple's `typedef NS_ENUM(…) { … }` as one, and there
/// each doc comment documents an enumerator.
#[must_use]
pub(crate) fn doxygen_container<T: DoxygenRow>(node: &Node) -> bool {
    preproc_conditional::<T>(node) || DOXYGEN_CONTAINERS.contains(&node.kind())
}

/// The kinds [`doxygen_skips`] always reads past, by kind string and
/// pinned to the grammars as [`DOXYGEN_CONTAINERS`] is.
pub(crate) const DOXYGEN_SKIP_KINDS: &[&str] = &[
    "preproc_include",
    "preproc_call",
    "static_assert_declaration",
    "namespace_alias_definition",
    "access_specifier",
    // Mozilla's C++ grammar reading a lone `FOO` line.
    "alone_macro",
    ":",
    ";",
];

/// A sibling Doxygen reads past to the entry after it: `#include`,
/// `#import`, `#pragma` and the other directives that declare nothing, an
/// empty statement, a `static_assert`, a namespace alias, an access
/// specifier, a preprocessor conditional that holds no entry, a parse
/// error holding no entry ([`no_entry_inside`]), and a statement the
/// grammar had to end itself ([`unterminated`]).
#[must_use]
pub(crate) fn doxygen_skips<T: DoxygenRow>(node: &Node, ancestors: Ancestors) -> bool {
    match node.kind() {
        kind if DOXYGEN_SKIP_KINDS.contains(&kind) => true,
        "expression_statement" => !statement_is_macro_call(node),
        "ERROR" => no_entry_inside::<T>(node, ancestors),
        _ => preproc_conditional::<T>(node) && !conditional_declares::<T>(node, ancestors),
    }
}

/// A parse error Doxygen reads no entry from: one holding, past comments
/// and nested parse errors, at most one named node that is not itself an
/// entry — a stray `}`, `@end` in a `.h` read as C or C++, `Q_OBJECT`, or
/// a macro call missing its `;`. A larger one, such as an Objective-C
/// `@interface` in a `.h`, is an entry. The child is asked with a chain
/// ending in the error, as a conditional's children are
/// ([`chain_below`]), not with the error's own, under which its
/// `parent` would be the error's (lesson 103).
pub(crate) fn no_entry_inside<T: DoxygenRow>(error: &Node, ancestors: Ancestors) -> bool {
    let mut named = error
        .children()
        .filter(|child| child.is_named() && !child.is_error() && child.kind() != "comment");
    match (named.next(), named.next()) {
        (None, _) => true,
        (Some(only), None) => {
            let parent = [*error];
            !doxygen_entry::<T>(&only, chain_below(ancestors, &parent))
        }
        _ => false,
    }
}

/// A statement whose `;` the grammar had to invent: `FOO`, `42`, the
/// `end` of `@end` in a `.h` read as C or C++, or a stray `@end` in
/// Objective-C. Doxygen reads no entry from it and documents the entry
/// after it; `FOO;` with its `;` is a variable to Doxygen.
fn unterminated(statement: &Node) -> bool {
    statement.children().last().is_some_and(|c| c.is_missing())
}

/// Whether an `expression_statement` outside a body is a macro call
/// (`FOO(bar);`), which Doxygen documents as a function, or code the
/// grammar misread — an Objective-C method in a `.h` read as C++ — that
/// Doxygen's own scanner still reads as the entry it is: not the empty
/// statement, and not one the grammar had to end itself. The gate the
/// `DocTarget` rows read for the statement kinds, and the complement of
/// what `doxygen_skips` reads past.
#[must_use]
pub(crate) fn statement_is_macro_call(statement: &Node) -> bool {
    statement.child_count() > 1 && !unterminated(statement)
}

/// What Doxygen documents from a doc block before it: the dialect's
/// `DocTarget` row (through [`DoxygenRow`], so this stays one fn for the
/// four dialects' tables), plus the one clause no row can spell — a parse
/// error holding an entry. The `ERROR` kind is the generated enum's
/// sentinel, which no row may own (`utils/role_tables.py` strips it),
/// so recovery is composed here, as the named context the Checker reads
/// (#1699).
#[must_use]
pub(crate) fn doxygen_target<'a, T: DoxygenRow>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    // role-bypass: allow(DocTarget) — the `ERROR` sentinel holding an
    // entry: recovery, not a kind a row can own
    T::doc_target_row(node, code, ancestors)
        || (node.is_error() && !no_entry_inside::<T>(node, ancestors))
}

/// What Doxygen documents from a doc block before it, short of a
/// preprocessor conditional — the *entry*: the dialect's `DocTarget`
/// row (`roles/<lang>.rs`, the C-family fragments in
/// `roles/c_family.rs`) less the conditional owners, plus a parse error
/// holding an entry. The conditional test comes *first*, so a
/// conditional is no entry here whatever it holds and the row's
/// [`conditional_declares`] gate is never reached through this fn. Each
/// path to the row makes that test on its own, and neither covers the
/// other: this fn's, which the walk reaches through an error's sole
/// child ([`no_entry_inside`]), and [`children_declare`]'s, which asks
/// [`non_conditional_entry`] of a conditional's other children only.
/// Code the grammar could not parse, often behind a macro Doxygen's
/// preprocessor would expand, is given the benefit of the doubt, as
/// before #1613, rather than dropping the block on a parse failure; an
/// error holding no entry is read past (`doxygen_skips`).
fn doxygen_entry<T: DoxygenRow>(node: &Node, ancestors: Ancestors) -> bool {
    !preproc_conditional::<T>(node) && non_conditional_entry::<T>(node, ancestors)
}

/// [`doxygen_entry`] of a node its caller has already found is no
/// conditional: the row, or a parse error holding an entry (an `ERROR`
/// is never a conditional kind).
///
/// The row is asked with no bytes, which is exact because no C-family
/// `DocTarget` gate reads any — `impl_doxygen_row!` in
/// `checker/doxygen_doc_binding.rs` asserts `!TEXT_GATED` at build time
/// — and with the caller's chain, which the remaining gates ignore (a
/// field, the children, the memo). It reads the *row*, through
/// `T::doc_target_row`, not `T::is_doc_target`: the Checker composes the
/// `ERROR` clause this fn already handles, and a dialect that someday
/// hand-writes a byte-reading `is_doc_target` would otherwise slip past
/// the row's assertion.
fn non_conditional_entry<T: DoxygenRow>(node: &Node, ancestors: Ancestors) -> bool {
    if node.is_error() {
        return !no_entry_inside::<T>(node, ancestors);
    }
    T::doc_target_row(node, &[], ancestors)
}

/// The `DocTarget` row of a C-family dialect, and its conditional owners
/// as a kind test, as the functions the Doxygen helpers read targets and
/// conditionals through; implemented by `impl_doxygen_row!`
/// (`checker/doxygen_doc_binding.rs`) beside the Checker that composes
/// it. Not a `Checker` supertrait: of the helpers bounded on this, only
/// [`doxygen_binds_forward`] reads a `Checker` method, and it names both
/// bounds.
pub(crate) trait DoxygenRow {
    /// The dialect's `doc_target::owns`.
    fn doc_target_row<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool;
    /// The dialect's `is_preproc_conditional_kind`: the row's conditional
    /// owners, gates aside, which `doc_target::lists_kind` cannot tell
    /// from the entries it lists beside them.
    fn conditional_kind(kind: u16) -> bool;
}

/// Whether a C-family doc comment binds to what follows it: one among a
/// container's entries outside every function body that neither documents
/// the member before it (`/**<`) nor stands alone (`@file`, `@class X`).
/// The others keep the reading of where they sit: in a body, Doxygen's
/// in-body description (#1609); in a declaration, that declaration's.
#[must_use]
pub(crate) fn doxygen_binds_forward<'a, T: Checker + DoxygenRow>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let bytes = node_bytes(comment, code);
    !doxygen_member_after(bytes)
        && !doxygen_standalone(bytes)
        && ancestors
            .parent(comment)
            .is_some_and(|parent| doxygen_container::<T>(&parent))
        && outside_function_bodies::<T>(comment, ancestors)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blocks Doxygen 1.13.2 made an entry of their own.
    const STANDALONE: &[&[u8]] = &[
        b"/** @file */",
        b"/*! \\file x.c */",
        b"/// @file",
        b"/**\n *@file\n */",
        b"/**\n * text\n * @file\n */",
        b"/** <tt>@file</tt> */",
        b"/** foo@file here */",
        b"/** @mainpage */",
        b"/** @page p Title */",
        b"/** @dir */",
        b"/** @example */",
        b"/** @defgroup */",
        b"/** @addtogroup g */",
        b"/** @weakgroup */",
        b"/** @name */",
        b"/** @package */",
        b"/** @class X */",
        b"/** @struct S\n * d */",
        b"/** @def M */",
        b"/** @fn int f(void) */",
        b"/** d @fn int f(void) */",
        b"/** @namespace n */",
        b"/** @concept K */",
        b"/**\n * @code\n * x\n * @endcode\n * @file\n */",
        b"/**\n * ```\n * x\n * ```\n * @file\n */",
        b"/**\n * @verbatim x @endverbatim @file\n */",
        b"/**\n * @htmlonly x @endhtmlonly\n * @file\n */",
        b"/**\n * <!-- x -->\n * @file\n */",
        b"/** `a` @file x `b` */",
        b"/** `a\n *\n * @file x\n * b` */",
        b"/** `a\n * @file x\n */",
        b"/** @defgroup g.x G */",
        b"/** (@defgroup g.x G) */",
        b"/** a\\defgroup g.x G */",
        b"/** a\\defgroup.g G */",
        b"/** x @defgroup.g G */",
        b"/** @var v */",
        b"/** @typedef t */",
        b"/** @property p */",
        b"/** @overload void f(int) */",
        b"/** @enum e */",
        b"/** @union u */",
        b"/** @interface I */",
        b"/** @protocol P */",
        b"/** @idlexcept E */",
        b"/** @module m */",
        b"/** @icode x @endicode @file */",
        b"/** @iverbatim x @endiverbatim @file */",
        b"/** @iliteral x @endiliteral @file */",
        b"/** @latexonly x @endlatexonly @file */",
        b"/** @xmlonly x @endxmlonly @file */",
        b"/** @rtfonly x @endrtfonly @file */",
        b"/** @manonly x @endmanonly @file */",
        b"/** @docbookonly x @enddocbookonly @file */",
        b"/** @dot x @enddot @file */",
        b"/** @msc x @endmsc @file */",
        b"/** @startuml x @enduml @file */",
        b"/** <!-- x -->@file */",
        // From `commentscan.l`'s `<Comment>` rules (#1646): a quote
        // closes only on its own line, and a CDATA section ends at `]]>`.
        b"/** \"@file */",
        b"/** \"a\"\n * @file\n */",
        b"/** \"x\n * @file\" */",
        b"/** \"\"@file */",
        b"/** <![CDATA[ x ]]> @file */",
        b"/** <![CDATA[ x ]]>@file */",
    ];

    /// Blocks Doxygen 1.13.2 gave to the declaration after them.
    const ATTACHED: &[&[u8]] = &[
        b"/** d */",
        b"/** @brief d */",
        b"/** @ingroup g */",
        b"/** @internal d */",
        b"/** @copyright d */",
        b"/** @File */",
        b"/** @class */",
        b"/** @fn\n * d */",
        b"/** @struct */",
        b"/** \\\\file x */",
        b"/** @@file x */",
        b"/** a@file.com */",
        b"/** `@file` */",
        b"/**\n * ```\n * @file\n * ```\n */",
        b"/**\n * @code\n * @file\n * @endcode\n */",
        b"/**\n * @verbatim\n * @file\n * @endverbatim\n */",
        b"/** @filex */",
        b"/** `a\n * @file x\n * b` */",
        b"/** `\n * @file x\n * b` */",
        b"/** a@file-x.com */",
        b"/** @file_x */",
        b"/**\n * @verbatim\n * @endcode @file\n * @endverbatim\n */",
        b"/**\n * @htmlonly @file @endhtmlonly\n */",
        b"/**\n * @htmlonly[block] @file @endhtmlonly\n */",
        b"/**\n * @htmlonly\n * @endlatexonly @file\n * @endhtmlonly\n */",
        b"/**\n * @latexonly\n * @file\n */",
        b"/** @xmlonly @file @endxmlonly */",
        b"/** @rtfonly @file @endrtfonly */",
        b"/** @manonly @file @endmanonly */",
        b"/** @docbookonly @file @enddocbookonly */",
        b"/** @dot @file @enddot */",
        b"/** @msc @file @endmsc */",
        b"/** @startuml @file @enduml */",
        b"/** @icode @file @endicode */",
        b"/** @iverbatim @file @endiverbatim */",
        b"/** @iliteral @file @endiliteral */",
        b"/**\n * <!-- @file -->\n */",
        b"/**\n * <!-- x --->\n * @file\n */",
        b"/**\n * <!-- x --!>\n * @file\n */",
        b"/**\n * <!-- x\n * @file\n */",
        // From `commentscan.l`'s `<Comment>` rules (#1646): quoted text
        // on one line and a CDATA section, closed or running to the end
        // of the block, are read past.
        b"/** \"@file\" */",
        b"/** a \"b\" \"@file\" c */",
        b"/** \"@file x\" \"@class X\" */",
        b"/** <![CDATA[ @file ]]> */",
        b"/** <![CDATA[ @file */",
        b"/**\n * <![CDATA[\n * @file\n * ]]>\n */",
    ];

    /// Each row is what Doxygen 1.13.2 made of the block above a
    /// declaration: an entry of its own (standalone), or the declaration's
    /// documentation. The quoted-text and `<![CDATA[` rows are read from
    /// `commentscan.l`'s `<Comment>` rules rather than from a run (#1646).
    #[test]
    fn doxygen_standalone_follows_commentscan() {
        crate::test_support::assert_classifies(doxygen_standalone, STANDALONE, ATTACHED);
    }

    /// Asked of every level of a nest, outermost first as the walk asks,
    /// the memo reads each conditional once and answers as the unmemoised
    /// search does — `true` with a declaration at the bottom, `false`
    /// without. The unmemoised count is the `O(depth²)` the memo removes
    /// (#1613).
    #[cfg(feature = "c")]
    #[test]
    fn conditional_declares_reads_each_conditional_once_per_walk() {
        use crate::node::{FoldCache, Tree};
        const DEPTH: usize = 40;
        for (bottom, declares) in [("int x;\n", true), ("#include <x.h>\n", false)] {
            let source = format!(
                "{}{bottom}{}",
                "/** d */\n#ifdef A\n".repeat(DEPTH),
                "#endif\n".repeat(DEPTH)
            );
            let tree = Tree::new::<crate::langs::CCode>(source.as_bytes());
            let nest: Vec<_> = tree
                .get_root()
                .preorder()
                .filter(|node| preproc_conditional::<crate::langs::CCode>(node))
                .collect();
            assert_eq!(nest.len(), DEPTH, "{bottom}");

            let cache = FoldCache::default();
            let walk = Ancestors::known(&[]).with_folds(&cache);
            let before = conditionals_read::observed();
            for level in &nest {
                assert_eq!(
                    conditional_declares::<crate::langs::CCode>(level, walk),
                    declares,
                    "{bottom}"
                );
            }
            let memoised = conditionals_read::observed() - before;

            let before = conditionals_read::observed();
            for level in &nest {
                assert_eq!(
                    conditional_declares::<crate::langs::CCode>(level, Ancestors::known(&[])),
                    declares,
                    "{bottom}"
                );
            }
            let unmemoised = conditionals_read::observed() - before;

            assert_eq!(memoised, DEPTH, "{bottom}");
            if !declares {
                assert_eq!(unmemoised, DEPTH * (DEPTH + 1) / 2, "{bottom}");
            }
        }
    }

    /// Every conditional of each nest answers as its children say — an
    /// entry, or a nested conditional that declares — asked outermost
    /// first (each nested answer folded in by the walk), innermost first
    /// (each one the memo's) and unmemoised. The first shape, an entry
    /// before a nested conditional declaring nothing, is the one that
    /// tells a fold from an overwrite at both fold sites (#1795); the
    /// others pin the remaining orders. The answers are in preorder, by
    /// hand: the `#elif` branch nests in the `#if` and the `#else` in the
    /// `#elif`.
    #[cfg(feature = "c")]
    #[test]
    fn conditional_declares_folds_in_every_child() {
        use crate::langs::CCode;
        use crate::node::{FoldCache, Tree};
        let shapes: &[(&str, &[bool])] = &[
            (
                "#ifdef A\nint x;\n#ifdef B\n#endif\n#endif\n",
                &[true, false],
            ),
            (
                "#ifdef A\n#ifdef B\n#endif\nint x;\n#endif\n",
                &[true, false],
            ),
            (
                "#ifdef A\n#ifdef B\nint x;\n#endif\n#include <y.h>\n#endif\n",
                &[true, true],
            ),
            (
                "#if A\n#ifdef B\n#endif\n#elif C\nint x;\n#else\n#ifdef D\n#endif\n#endif\n",
                &[true, false, true, false, false],
            ),
        ];
        for &(shape, answers) in shapes {
            let tree = Tree::new::<CCode>(shape.as_bytes());
            let nest: Vec<_> = tree
                .get_root()
                .preorder()
                .filter(|node| preproc_conditional::<CCode>(node))
                .zip(answers.iter().copied())
                .collect();
            assert_eq!(nest.len(), answers.len(), "{shape}");
            for order in [nest.clone(), nest.iter().rev().copied().collect()] {
                let cache = FoldCache::default();
                let walk = Ancestors::known(&[]).with_folds(&cache);
                for (level, declares) in &order {
                    let row = level.start_row();
                    let answer = conditional_declares::<CCode>(level, walk);
                    assert_eq!(answer, *declares, "{shape}: row {row}");
                    let answer = conditional_declares::<CCode>(level, Ancestors::known(&[]));
                    assert_eq!(answer, *declares, "{shape}: row {row}, unmemoised");
                }
            }
        }
    }

    /// Each path to the row tests `preproc_conditional` *before* reading
    /// it: the row's conditional owners are gated by
    /// `conditional_declares`, so the other order would run that walk
    /// from inside a question about a conditional. Pinned by count on the
    /// uncached path, one assertion per path. The entry question
    /// (`doxygen_entry`) of the outermost conditional reads **nothing** —
    /// it is no entry whatever it holds, and the reversed order reads the
    /// nest once. `conditional_declares` of it reads each level once,
    /// where `children_declare` asking the row first costs `2^depth - 1`
    /// (4,095 at this depth) on the nest declaring nothing; on the nest
    /// with an entry each re-entered walk answers `true` and ends the
    /// level, so that order reads `depth` there and only the other nest
    /// tells (#1699, #1795).
    #[cfg(feature = "c")]
    #[test]
    fn doxygen_entry_never_reenters_the_conditional_walk() {
        use crate::node::Tree;
        const DEPTH: usize = 12;
        for (bottom, declares) in [("int x;\n", true), ("#include <x.h>\n", false)] {
            let source = format!(
                "{}{bottom}{}",
                "#ifdef A\n".repeat(DEPTH),
                "#endif\n".repeat(DEPTH)
            );
            let tree = Tree::new::<crate::langs::CCode>(source.as_bytes());
            let outermost = tree
                .get_root()
                .preorder()
                .find(|node| preproc_conditional::<crate::langs::CCode>(node))
                .expect("a nest");
            // Both uncached contexts: a known chain, under which the walk
            // hands each child a one-entry chain, and no chain at all (a
            // `find` / `count` consumer), under which the walk hands the
            // climbing context through unchanged.
            for uncached in [Ancestors::known(&[]), Ancestors::unknown()] {
                let before = conditionals_read::observed();
                assert!(!doxygen_entry::<crate::langs::CCode>(&outermost, uncached));
                assert_eq!(
                    conditionals_read::observed() - before,
                    0,
                    "{bottom}: an entry question reads no nest"
                );
                let before = conditionals_read::observed();
                assert_eq!(
                    conditional_declares::<crate::langs::CCode>(&outermost, uncached),
                    declares,
                    "{bottom}"
                );
                assert_eq!(
                    conditionals_read::observed() - before,
                    DEPTH,
                    "{bottom}: one walk, each level once"
                );
            }
        }
    }

    /// `no_entry_inside` asks the row of an error's sole named child
    /// with a chain ending in the *error* — the child's parent — not
    /// the error's own chain, under which `parent` would be the error's
    /// parent (lesson 103). No C-family gate reads the chain today, so
    /// the production rows cannot tell; a row that checks `parent`
    /// against the tree does. `Q_OBJECT` then `int after;` recovers as
    /// a declaration whose `ERROR` holds one `identifier`.
    #[cfg(feature = "c")]
    #[test]
    fn error_child_is_asked_with_the_error_as_its_parent() {
        use crate::node::Tree;
        use std::cell::Cell;
        thread_local! { static ASKED: Cell<usize> = const { Cell::new(0) }; }
        struct ParentChecked;
        impl DoxygenRow for ParentChecked {
            fn doc_target_row<'a>(node: &Node<'a>, _: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
                assert_eq!(
                    ancestors.parent(node).map(|p| p.id()),
                    node.parent().map(|p| p.id()),
                    "{} asked with a chain not its own",
                    node.kind()
                );
                ASKED.with(|asked| asked.set(asked.get() + 1));
                false
            }
            // The fixture is C, so C's conditionals.
            fn conditional_kind(kind: u16) -> bool {
                <crate::langs::CCode as DoxygenRow>::conditional_kind(kind)
            }
        }
        let source = b"int ok;\nQ_OBJECT\nint after;\n";
        let tree = Tree::new::<crate::langs::CCode>(source);
        let root = tree.get_root();
        let mut chain = vec![root];
        let mut stack: Vec<(Node, usize)> = root.children().map(|c| (c, 1)).collect();
        let mut errors = 0;
        while let Some((node, depth)) = stack.pop() {
            chain.truncate(depth);
            if node.is_error() {
                errors += 1;
                assert!(no_entry_inside::<ParentChecked>(
                    &node,
                    Ancestors::known(&chain)
                ));
            }
            chain.push(node);
            stack.extend(node.children().map(|c| (c, depth + 1)));
        }
        assert_eq!(errors, 1, "one recovery error in the fixture");
        assert_eq!(ASKED.with(Cell::get), 1, "the sole child was asked once");
    }

    #[test]
    fn doxygen_member_after_takes_the_four_markers() {
        for after in [&b"/**< d */"[..], b"/*!< d */", b"///< d", b"//!< d"] {
            assert!(doxygen_member_after(after));
        }
        for before in [&b"/** d */"[..], b"/// d", b"//! d", b"/*! d */"] {
            assert!(!doxygen_member_after(before));
        }
    }
}
