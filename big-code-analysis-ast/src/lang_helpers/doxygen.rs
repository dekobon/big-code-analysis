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
/// fenced block, an HTML comment, or a format block (`\code`,
/// `\verbatim`, `\htmlonly`, `\dot`, …). Commands match case sensitively,
/// as Doxygen's command table does. Quoted text and `<![CDATA[` sections,
/// which Doxygen also reads past, are read for commands here.
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
/// starts at `s[at]`: a ```` ``` ```` fence (to the next fence or the end
/// of the block), a backtick span closed before the next blank line
/// (Markdown's `processCodeSpan`), an HTML comment (to its `-->` or the
/// end of the block), a format block (to its end command or the end of
/// the block), or an escaped `\\` / `@@`.
fn literal_end(s: &[u8], at: usize) -> Option<usize> {
    let rest = &s[at..];
    if let Some(fenced) = rest.strip_prefix(b"```") {
        let close = fenced.windows(3).position(|w| w == b"```");
        return Some(close.map_or(s.len(), |close| at + 6 + close));
    }
    if let Some(span) = rest.strip_prefix(b"`") {
        return code_span_close(span).map(|close| at + close + 2);
    }
    if rest.starts_with(b"<!--") {
        return Some(html_comment_end(s, at + 4));
    }
    if matches!(rest, [b'@', b'@', ..] | [b'\\', b'\\', ..]) {
        return Some(at + 2);
    }
    let end: &[u8] = match doxygen_command(s, at)? {
        b"code" => b"endcode",
        b"icode" => b"endicode",
        b"verbatim" => b"endverbatim",
        b"iverbatim" => b"endiverbatim",
        b"iliteral" => b"endiliteral",
        b"htmlonly" => b"endhtmlonly",
        b"latexonly" => b"endlatexonly",
        b"xmlonly" => b"endxmlonly",
        b"rtfonly" => b"endrtfonly",
        b"manonly" => b"endmanonly",
        b"docbookonly" => b"enddocbookonly",
        b"dot" => b"enddot",
        b"msc" => b"endmsc",
        b"startuml" => b"enduml",
        _ => return None,
    };
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

/// A preprocessor conditional or one of its branches, in every C-family
/// grammar and every alias of the rule: the nodes Doxygen's preprocessor
/// reads through to the declarations inside.
#[must_use]
pub(crate) fn preproc_conditional(node: &Node) -> bool {
    matches!(
        node.kind(),
        "preproc_if" | "preproc_ifdef" | "preproc_else" | "preproc_elif" | "preproc_elifdef"
    )
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
    // Post-order: a conditional is answered once its nested ones are.
    let mut stack = vec![(*node, false)];
    while let Some((conditional, expanded)) = stack.pop() {
        if memo.get(&conditional).is_some() {
            continue;
        }
        if expanded {
            conditionals_read::record();
            let parent = [conditional];
            let below = chain_below(ancestors, &parent);
            let declares = conditional.children().any(|child| {
                if preproc_conditional(&child) {
                    memo.get(&child) == Some(true)
                } else {
                    doxygen_entry::<T>(&child, below)
                }
            });
            memo.set(&conditional, declares);
            continue;
        }
        stack.push((conditional, true));
        stack.extend(
            conditional
                .children()
                .filter(|child| preproc_conditional(child) && memo.get(child).is_none())
                .map(|child| (child, false)),
        );
    }
    memo.get(node) == Some(true)
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
pub(crate) fn doxygen_container(node: &Node) -> bool {
    preproc_conditional(node)
        || matches!(
            node.kind(),
            "translation_unit"
                | "template_parameter_list"
                | "declaration_list"
                | "field_declaration_list"
                | "enumerator_list"
                | "class_interface"
                | "class_implementation"
                | "protocol_declaration"
                | "qualified_protocol_interface_declaration"
                | "instance_variables"
        )
}

/// A sibling Doxygen reads past to the entry after it: `#include`,
/// `#import`, `#pragma` and the other directives that declare nothing, an
/// empty statement, a `static_assert`, a namespace alias, an access
/// specifier, a preprocessor conditional that holds no entry, a parse
/// error holding no entry ([`no_entry_inside`]), and a statement the
/// grammar had to end itself ([`unterminated`]).
#[must_use]
pub(crate) fn doxygen_skips<T: DoxygenRow>(node: &Node, ancestors: Ancestors) -> bool {
    match node.kind() {
        // `alone_macro` is Mozilla's C++ grammar reading a lone `FOO` line.
        "preproc_include"
        | "preproc_call"
        | "static_assert_declaration"
        | "namespace_alias_definition"
        | "access_specifier"
        | "alone_macro"
        | ":"
        | ";" => true,
        "expression_statement" => !statement_is_macro_call(node),
        "ERROR" => no_entry_inside::<T>(node, ancestors),
        _ => preproc_conditional(node) && !conditional_declares::<T>(node, ancestors),
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
/// `conditional_declares` gate is never reached through this fn —
/// [`conditional_declares`]'s own post-order walk asks this of a
/// conditional's non-conditional children only and reads its memo for
/// the rest, so the two defences are independent and either alone
/// keeps the walk from re-entering itself. Code the grammar could not parse,
/// often behind a macro Doxygen's preprocessor would expand, is given
/// the benefit of the doubt, as before #1613, rather than dropping the
/// block on a parse failure; an error holding no entry is read past
/// (`doxygen_skips`).
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
fn doxygen_entry<T: DoxygenRow>(node: &Node, ancestors: Ancestors) -> bool {
    if node.is_error() {
        return !no_entry_inside::<T>(node, ancestors);
    }
    !preproc_conditional(node) && T::doc_target_row(node, &[], ancestors)
}

/// The `DocTarget` row of a C-family dialect, as the one function the
/// Doxygen helpers read targets through; implemented by
/// `impl_doxygen_row!` (`checker/doxygen_doc_binding.rs`) beside the
/// Checker that composes it. Only the row: no helper bounded on this
/// reads a `Checker` method, so a `Checker` supertrait would be a bound
/// nothing uses.
pub(crate) trait DoxygenRow {
    /// The dialect's `doc_target::owns`.
    fn doc_target_row<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool;
}

/// Whether a C-family doc comment binds to what follows it: one among a
/// container's entries outside every function body that neither documents
/// the member before it (`/**<`) nor stands alone (`@file`, `@class X`).
/// The others keep the reading of where they sit: in a body, Doxygen's
/// in-body description (#1609); in a declaration, that declaration's.
#[must_use]
pub(crate) fn doxygen_binds_forward<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let bytes = node_bytes(comment, code);
    !doxygen_member_after(bytes)
        && !doxygen_standalone(bytes)
        && ancestors
            .parent(comment)
            .is_some_and(|parent| doxygen_container(&parent))
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
    ];

    /// Each row is what Doxygen 1.13.2 made of the block above a
    /// declaration: an entry of its own (standalone), or the declaration's
    /// documentation.
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
                .filter(preproc_conditional)
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

    /// `doxygen_entry` must test `preproc_conditional` *before* reading
    /// the row: the row's conditional owners are gated by
    /// `conditional_declares`, so the other order would run that walk
    /// from inside an entry question about a conditional. Two defences
    /// keep the walk from re-entering itself — this order, and the
    /// walk asking `doxygen_entry` of non-conditional children only —
    /// and when it had the first alone, reversing it cost `2^depth - 1`
    /// reads on the uncached path (4,095 at this depth) against `depth`.
    /// Pinned by count on the uncached path: asking the entry question
    /// of the outermost conditional must read **nothing** — it is no
    /// entry whatever it holds, and the reversed order reads the nest
    /// once — and asking `conditional_declares` of it reads each level
    /// once (#1699).
    #[cfg(feature = "c")]
    #[test]
    fn doxygen_entry_never_reenters_the_conditional_walk() {
        use crate::node::Tree;
        const DEPTH: usize = 12;
        let source = format!(
            "{}int x;\n{}",
            "#ifdef A\n".repeat(DEPTH),
            "#endif\n".repeat(DEPTH)
        );
        let tree = Tree::new::<crate::langs::CCode>(source.as_bytes());
        let outermost = tree
            .get_root()
            .preorder()
            .find(preproc_conditional)
            .expect("a nest");
        let uncached = Ancestors::known(&[]);
        let before = conditionals_read::observed();
        assert!(!doxygen_entry::<crate::langs::CCode>(&outermost, uncached));
        assert_eq!(
            conditionals_read::observed() - before,
            0,
            "an entry question reads no nest"
        );
        let before = conditionals_read::observed();
        assert!(conditional_declares::<crate::langs::CCode>(
            &outermost, uncached
        ));
        assert_eq!(
            conditionals_read::observed() - before,
            DEPTH,
            "one walk, each level once"
        );
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
