//! `Checker` implementation for Lua.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{
    documents_where_it_sits, ldoc_opener, lua_long_comment, node_bytes, run_continues_to,
};

/// Whether `node` is an item LDoc's `item_follows` (`lang.lua`) documents:
/// a `function` statement, local or not; an assignment to one plain or
/// dotted name, local (`local x = 1`, not a bare `local x`) or not
/// (`M.x = 1`); a table field with a name (`new = function … end`); or a
/// `return` of a function or a table. `get_fun_name` stops at `,`, `<`
/// and `[`, so `local a, b = …`, `local x <const> = …` and `M[1] = …`
/// name no item.
fn lua_ldoc_item(node: &Node) -> bool {
    // role-bypass: allow(FuncOpener) — doc attachment, not membership:
    // what an LDoc block documents is `DocTarget`'s own row (R09)
    match node.kind_id().into() {
        Lua::FunctionDeclaration | Lua::FunctionDeclaration2 | Lua::FunctionDeclaration3 => true,
        Lua::AssignmentStatement | Lua::AssignmentStatement2 => lua_assigns_one_name(node),
        Lua::VariableDeclaration | Lua::VariableDeclaration2 => node.children().any(|child| {
            matches!(
                child.kind_id().into(),
                Lua::AssignmentStatement | Lua::AssignmentStatement2
            ) && lua_assigns_one_name(&child)
        }),
        // `name = value`; the bracket form starts with `[` and a positional
        // field has no `=`.
        Lua::Field => {
            node.child(0)
                .is_some_and(|name| name.kind_id() == Lua::Identifier)
                && node.child(1).is_some_and(|eq| eq.kind_id() == Lua::EQ)
        }
        Lua::ReturnStatement => node
            .children()
            .find(|child| {
                matches!(
                    child.kind_id().into(),
                    Lua::ExpressionList | Lua::ExpressionList2
                )
            })
            .and_then(|list| list.children().find(Node::is_named))
            .is_some_and(|value| {
                // role-bypass: allow(Closure) — what an assigned value
                // documents as, not whether it is a closure
                matches!(
                    value.kind_id().into(),
                    Lua::FunctionDefinition | Lua::TableConstructor
                )
            }),
        _ => false,
    }
}

/// Whether `assignment`'s target list is a single plain or dotted name.
fn lua_assigns_one_name(assignment: &Node) -> bool {
    let Some(targets) = assignment.child(0) else {
        return false;
    };
    let mut names = targets.children().filter(Node::is_named);
    names.next().is_some_and(|name| lua_is_dotted_name(&name)) && names.next().is_none()
}

/// An identifier, or a `.` index of one (`M.a.b`).
fn lua_is_dotted_name(node: &Node) -> bool {
    node.preorder().filter(Node::is_named).all(|part| {
        matches!(
            part.kind_id().into(),
            Lua::Identifier | Lua::DotIndexExpression | Lua::DotIndexExpression2
        )
    })
}

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
        if opens_run
            && sibling.kind_id() == Lua::Comment
            && ldoc_opener(node_bytes(&sibling, code))
            && lua_block_text(&sibling, code, ancestors).text
        {
            return false;
        }
        earliest = sibling;
    }
    matches!(earliest.kind_id().into(), Lua::Comment | Lua::HashBangLine)
}

/// The trailing comment that heads `first`'s LDoc group, when `first` sits
/// directly under a row that ends in one: LDoc lexes a line comment with
/// its newline and gathers every consecutive comment token into one group
/// whose first token decides whether it is an LDoc block (`parse.lua`), so
/// a `---` row under `x = 1 -- note` belongs to that `--` group. The run
/// `leading_doc_run` hands over leaves such a comment out, as go/doc does,
/// so it is looked up here.
fn lua_group_head_above<'a>(
    first: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    ancestors.siblings_before(first).next().filter(|above| {
        LuaCode::is_comment(above) && run_continues_to::<LuaCode>(above, first, code)
    })
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
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Lua::Comment
    }

    // LDoc documents what a module declares; a `---` block inside a function
    // body — LuaLS's `---@type` on a local, an LDoc `@fixme` — annotates the
    // code there rather than documenting it, as PHP's in-body `@var` does.
    // Elsewhere LDoc (`parse.lua`) discards a block with no text, keeps one
    // that is the file's first, that carries a tag, or that an item
    // follows, and drops any other with a "definition cannot be parsed"
    // warning. Unlike LDoc, a tagged block counts without an item or a name
    // to attach to.
    fn is_doc_run_opener<'a>(
        first: &Node<'a>,
        next: Option<&Node<'a>>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        if !(first.kind_id() == Lua::Comment
            && ldoc_opener(node_bytes(first, code))
            && documents_where_it_sits::<Self>(first, code, ancestors))
            || lua_group_head_above(first, code, ancestors)
                .is_some_and(|head| !ldoc_opener(node_bytes(&head, code)))
        {
            return false;
        }
        let text = lua_block_text(first, code, ancestors);
        let item = next.map(|next| lua_block_first_statement(next).unwrap_or(*next));
        text.text
            && (item.as_ref().is_some_and(lua_ldoc_item)
                || text.tag
                || lua_first_ldoc_block(first, code, ancestors))
    }

    fn ends_doc_run(node: &Node, code: &[u8]) -> bool {
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

    // Lua uses a dedicated elseif_statement node rather than nesting a
    // second if_statement inside the outer one (as Go does).
    impl_is_else_if_clause!(Lua, ElseifStatement);
}
