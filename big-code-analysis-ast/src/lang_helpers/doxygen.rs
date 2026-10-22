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
pub(crate) fn conditional_declares(node: &Node, ancestors: Ancestors) -> bool {
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
            let declares = conditional.children().any(|child| {
                doxygen_entry(&child)
                    || (preproc_conditional(&child) && memo.get(&child) == Some(true))
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
pub(crate) fn doxygen_skips(node: &Node, ancestors: Ancestors) -> bool {
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
        "expression_statement" => node.child_count() == 1 || unterminated(node),
        "ERROR" => no_entry_inside(node),
        _ => preproc_conditional(node) && !conditional_declares(node, ancestors),
    }
}

/// A parse error Doxygen reads no entry from: one holding, past comments
/// and nested parse errors, at most one named node that is not itself an
/// entry — a stray `}`, `@end` in a `.h` read as C or C++, `Q_OBJECT`, or
/// a macro call missing its `;`. A larger one, such as an Objective-C
/// `@interface` in a `.h`, is an entry.
fn no_entry_inside(error: &Node) -> bool {
    let mut named = error
        .children()
        .filter(|child| child.is_named() && !child.is_error() && child.kind() != "comment");
    match (named.next(), named.next()) {
        (None, _) => true,
        (Some(only), None) => !doxygen_entry(&only),
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

/// What Doxygen documents from a doc block before it: a declaration or a
/// definition, a macro, a typedef or alias, a namespace, a `using`, a
/// `friend`, a template (a forward-declared one included), a type with a
/// body, an enumerator, an Objective-C container, property, method or
/// instance variable, a macro call at file or class scope (which Doxygen
/// reads as a function), and a preprocessor conditional holding any of
/// these. A forward declaration of a type (`struct S;`, `@class C;`,
/// `@protocol P;`), an explicit instantiation, `@synthesize` / `@dynamic`
/// and a closing `}` or `@end` take none; Doxygen drops the block.
#[must_use]
pub(crate) fn doxygen_target(node: &Node, ancestors: Ancestors) -> bool {
    doxygen_entry(node) || (preproc_conditional(node) && conditional_declares(node, ancestors))
}

/// [`doxygen_target`] short of a preprocessor conditional.
fn doxygen_entry(node: &Node) -> bool {
    let has_body = |specifier: &Node| specifier.child_by_field_name("body").is_some();
    match node.kind() {
        "declaration"
        | "function_definition"
        | "type_definition"
        | "preproc_def"
        | "preproc_function_def"
        | "linkage_specification"
        | "enumerator"
        | "namespace_definition"
        | "using_declaration"
        | "alias_declaration"
        | "friend_declaration"
        | "template_declaration"
        | "class_interface"
        | "class_implementation"
        | "protocol_declaration"
        | "qualified_protocol_interface_declaration"
        | "property_declaration"
        | "method_declaration"
        | "method_definition"
        | "instance_variable" => true,
        // Code the grammar could not parse, often behind a macro Doxygen's
        // preprocessor would expand: given the benefit of the doubt, as
        // before #1613, rather than dropping the block on a parse failure.
        // One holding no entry is read past (`doxygen_skips`).
        "ERROR" => !no_entry_inside(node),
        "struct_specifier" | "union_specifier" | "enum_specifier" | "class_specifier" => {
            has_body(node)
        }
        // `struct In;` in a class body declares no member.
        "field_declaration" => {
            node.child_by_field_name("declarator").is_some()
                || node.child_by_field_name("type").is_none_or(|t| {
                    !matches!(
                        t.kind(),
                        "struct_specifier"
                            | "union_specifier"
                            | "enum_specifier"
                            | "class_specifier"
                    ) || has_body(&t)
                })
        }
        // Outside a body a statement is a macro call (`FOO(bar);`), which
        // Doxygen documents as a function, or code the grammar misread —
        // an Objective-C method in a `.h` read as C++ — that Doxygen's own
        // scanner still reads as the entry it is. The empty statement is
        // read past (`doxygen_skips`).
        "expression_statement" => node.child_count() > 1 && !unterminated(node),
        "implementation_definition" => node
            .child(0)
            .is_some_and(|d| d.kind() != "property_implementation"),
        _ => false,
    }
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
                assert_eq!(conditional_declares(level, walk), declares, "{bottom}");
            }
            let memoised = conditionals_read::observed() - before;

            let before = conditionals_read::observed();
            for level in &nest {
                assert_eq!(
                    conditional_declares(level, Ancestors::known(&[])),
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
