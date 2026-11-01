//! `Checker` implementation for Lua.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{
    ldoc_opener, lua_long_comment, node_bytes, run_continues_to,
};

/// What the text of the block `first` opens holds, read over the run
/// `first` starts, as `leading_doc_run` delimits it.
#[derive(Default)]
struct LdocText {
    /// Anything but dashes and whitespace: LDoc discards a block that is
    /// empty once its comment markers are stripped (`parse.lua`).
    text: bool,
    /// A line opening with an `@` tag, as `luadoc_tag` (`^%s*@(%w+)`)
    /// reads one after the markers — so `---@class` is a tag and an
    /// address in prose (`dev@example.com`) is not.
    tag: bool,
}

fn lua_block_text<'a>(first: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> LdocText {
    let mut found = LdocText::default();
    let mut member = *first;
    let mut after = ancestors.siblings_after(first);
    loop {
        for line in node_bytes(&member, code).split(|&byte| byte == b'\n') {
            let body = ldoc_line_body(line);
            found.text |= body.iter().any(|&byte| {
                !matches!(byte, b'-' | b'[' | b']' | b'=') && !byte.is_ascii_whitespace()
            });
            found.tag |=
                matches!(body.trim_ascii_start(), [b'@', tag, ..] if tag.is_ascii_alphanumeric());
        }
        match after.next() {
            Some(next) if run_continues_to::<LuaCode>(&member, &next, code) => member = next,
            _ => return found,
        }
    }
}

/// A comment line with its leading whitespace and comment dashes
/// stripped: the text LDoc reads.
fn ldoc_line_body(line: &[u8]) -> &[u8] {
    let line = line.trim_ascii_start();
    let dashes = line.iter().take_while(|&&byte| byte == b'-').count();
    line.get(dashes..).unwrap_or_default()
}

/// Whether the block `first` opens is the file's first LDoc block, which
/// LDoc takes as the module's documentation whatever follows it: it sits
/// at the top level, no earlier block with text opens a run, and the file
/// opens with a comment or a shebang — when its first token is code and
/// there is no `module()` call, LDoc reads nothing from the file at all,
/// while after a `#!` line it skips straight to the first comment
/// (`parse.lua`). The backward scan stops at an earlier qualifying block,
/// and every block that reaches it qualifies, so the scans of a file cover
/// disjoint stretches and cost `O(siblings)` in all.
fn lua_first_ldoc_block<'a>(first: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    // A fast path, not a separate rule: every other container opens with a
    // token (`if`, `do`, `{`), and the grammar parks a comment above a
    // body's first statement after it, so the first-token test below would
    // reject a nested block too — after a scan this skips.
    // role-bypass: allow(Container) — where the first comment sits,
    // not whether the chunk opens a space
    if ancestors
        .parent(first)
        .is_none_or(|parent| parent.kind_id() != Lua::Chunk)
    {
        return false;
    }
    let mut earliest = *first;
    let mut before = ancestors.siblings_before(first).peekable();
    while let Some(sibling) = before.next() {
        // `run_continues_to` asks only whether `sibling` continues a run
        // from `above`, so a comment directly under code needs the
        // comment test too: it opens its run.
        let opens_run = before.peek().is_none_or(|above| {
            !(LuaCode::is_comment(above) && run_continues_to::<LuaCode>(above, &sibling, code))
        });
        // role-bypass: allow(Comment, DocRunOpener) — the backward scan for an
        // earlier block: run traversal over the rows, not membership
        if opens_run
            && sibling.kind_id() == Lua::Comment
            && ldoc_opener(node_bytes(&sibling, code))
            && lua_block_text(&sibling, code, ancestors).text
        {
            return false;
        }
        earliest = sibling;
    }
    // role-bypass: allow(Comment) — whether the file opens with a comment or
    // a shebang, LDoc's first-token rule; traversal, not membership
    matches!(earliest.kind_id().into(), Lua::Comment | Lua::HashBangLine)
}

/// The first statement of a `block`, which stands for it: the grammar parks
/// a comment that precedes the first statement of an `if`, `do` or loop
/// body outside the `block`.
fn lua_block_first_statement<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    (node.kind_id() == Lua::Block)
        .then(|| node.children().find(Node::is_named))
        .flatten()
}

impl Checker for LuaCode {
    // The `Comment` row of `roles/lua.rs` (#1699); no `DocComment`.
    impl_comment_from_rows!(lua);

    // The `DocRunOpener` row of `roles/lua.rs` is the opener candidate: a
    // `---` comment where LDoc reads one — outside a function body (a
    // `---` block in a body, LuaLS's `---@type` on a local or an LDoc
    // `@fixme`, annotates the code there rather than documenting it, as
    // PHP's in-body `@var` does) and not continuing a `--` group. The
    // content decision needs the sibling after the run and stays here:
    // LDoc (`parse.lua`) discards a block with no text, keeps one that is
    // the file's first, that carries a tag, or that an item follows —
    // the `DocTarget` row — and drops any other with a "definition cannot
    // be parsed" warning. Unlike LDoc, a tagged block counts without an
    // item or a name to attach to.
    fn is_doc_run_opener<'a>(
        first: &Node<'a>,
        next: Option<&Node<'a>>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        if !crate::roles::lua::doc_run_opener::owns(first, code, ancestors) {
            return false;
        }
        // role-bypass: allow(DocRunOpener, DocTarget) — the content decision
        // over the run and the item after it: context the row cannot see,
        // since it needs `next`; not membership
        let text = lua_block_text(first, code, ancestors);
        let item = next.map(|next| lua_block_first_statement(next).unwrap_or(*next));
        text.text
            && (item
                .as_ref()
                .is_some_and(|item| crate::roles::lua::doc_target::owns(item, code, ancestors))
                || text.tag
                || lua_first_ldoc_block(first, code, ancestors))
    }

    fn ends_doc_run(node: &Node, code: &[u8]) -> bool {
        // role-bypass: allow(Comment) — which comment form ends a run (a long
        // comment's closing newline is not part of it); run traversal, not
        // membership
        node.kind_id() == Lua::Comment && lua_long_comment(node_bytes(node, code))
    }

    // The space tree is declared in `roles/lua.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain, so the
    // byte-less spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::lua::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::lua::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::lua::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/lua.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(lua, call, is_call, is_call_with_code);
    impl_row_predicates!(lua, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        // NOTE: `impl NArgs for LuaCode` overrides `compute` with a positive
        // filter on `Identifier | VarargExpression` and never calls `is_non_arg`.
        // This implementation satisfies the trait contract but is unused for NArgs.
        matches!(
            node.kind_id().into(),
            Lua::LPAREN | Lua::COMMA | Lua::RPAREN
        )
    }

    // The `ElseIf` row of `roles/lua.rs`: the dedicated `elseif_statement`
    // itself is the else-if, never a nested `if` (#1697).
    impl_else_if_from_row!(lua);
}
