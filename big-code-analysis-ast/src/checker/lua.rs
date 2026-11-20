//! `Checker` implementation for Lua.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::documentation::budget::{NoBudget, WorkBudget, infallible};
use crate::documentation::byte_scan as scan;
use crate::lang_helpers::doc_comment::{lua_long_comment, node_bytes};

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

impl LdocText {
    fn read_with_budget<B: WorkBudget>(
        &mut self,
        member: &Node<'_>,
        code: &[u8],
        budget: &B,
    ) -> Result<(), B::Error> {
        for line in scan::lines(node_bytes(member, code), budget) {
            budget.step()?;
            let body = ldoc_line_body(line?, budget)?;
            self.text |= scan::position(
                body,
                |byte| !matches!(byte, b'-' | b'[' | b']' | b'=') && !byte.is_ascii_whitespace(),
                budget,
            )?
            .is_some();
            self.tag |= matches!(scan::trim_start(body, budget)?, [b'@', tag, ..] if tag.is_ascii_alphanumeric());
        }
        Ok(())
    }
}

fn lua_block_text<'a>(first: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> LdocText {
    infallible(lua_block_text_with_budget(
        first, code, ancestors, &NoBudget,
    ))
}

fn lua_block_text_with_budget<'a, B: WorkBudget>(
    first: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    budget: &B,
) -> Result<LdocText, B::Error> {
    let mut found = LdocText::default();
    let mut member = *first;
    let mut after = ancestors.siblings_after_with_budget(first, budget)?;
    loop {
        budget.step()?;
        found.read_with_budget(&member, code, budget)?;
        if let Some(next) = after.next()
            && crate::lang_helpers::doc_comment::run_continues_with_budget::<
                crate::documentation::recognition::NativeRules<LuaCode>,
                _,
            >(&member, &next, code, budget)?
        {
            member = next;
        } else {
            return Ok(found);
        }
    }
}

/// A comment line with its leading whitespace and comment dashes
/// stripped: the text LDoc reads.
fn ldoc_line_body<'s, B: WorkBudget>(line: &'s [u8], budget: &B) -> Result<&'s [u8], B::Error> {
    let line = scan::trim_start(line, budget)?;
    let dashes = scan::position(line, |byte| byte != b'-', budget)?.unwrap_or(line.len());
    Ok(&line[dashes..])
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
    lua_first_block_in(first, code, ancestors, None).unwrap_or(false)
}

fn lua_first_block_in<'a>(
    first: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    source: Option<&crate::documentation::SourceContext<'a>>,
) -> Result<bool, crate::documentation::UnresolvedReason> {
    infallible(lua_first_block_with_budget(
        first, code, ancestors, source, &NoBudget,
    ))
}

fn lua_first_block_with_budget<'a, B: WorkBudget>(
    first: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    source: Option<&crate::documentation::SourceContext<'a>>,
    budget: &B,
) -> Result<Result<bool, crate::documentation::UnresolvedReason>, B::Error> {
    // bca: suppress(halstead) -- the same reverse first-block decision serves legacy and source, with source-only gap barriers.
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
        return Ok(Ok(false));
    }
    let mut earliest = *first;
    let mut before = ancestors
        .siblings_before_with_budget(first, budget)?
        .peekable();
    while let Some(sibling) = before.next() {
        budget.step()?;
        if let Some(source) = source
            && !source.covers_with_budget(sibling.start_byte()..earliest.end_byte(), budget)?
        {
            return Ok(Err(crate::documentation::UnresolvedReason::ExcludedGap));
        }
        if let Some(source) = source
            && let Some(above) = before.peek()
            && !source.covers_with_budget(above.start_byte()..sibling.end_byte(), budget)?
        {
            return Ok(Err(crate::documentation::UnresolvedReason::ExcludedGap));
        }
        // `run_continues_to` asks only whether `sibling` continues a run
        // from `above`, so a comment directly under code needs the
        // comment test too: it opens its run.
        let opens_run = if let Some(above) = before.peek() {
            !(LuaCode::is_comment(above)
                && crate::lang_helpers::doc_comment::run_continues_with_budget::<
                    crate::documentation::recognition::NativeRules<LuaCode>,
                    _,
                >(above, &sibling, code, budget)?)
        } else {
            true
        };
        // role-bypass: allow(Comment, DocRunOpener) — the backward scan for an
        // earlier block: run traversal over the rows, not membership
        if opens_run
            && sibling.kind_id() == Lua::Comment
            && crate::lang_helpers::doc_text::ldoc_opener_with_budget(
                node_bytes(&sibling, code),
                budget,
            )?
            && lua_block_text_with_budget(&sibling, code, ancestors, budget)?.text
        {
            return Ok(Ok(false));
        }
        earliest = sibling;
    }
    if let Some(source) = source
        && !source.covers_with_budget(0..earliest.start_byte(), budget)?
    {
        return Ok(Err(crate::documentation::UnresolvedReason::ExcludedGap));
    }
    // role-bypass: allow(Comment) — whether the file opens with a comment or
    // a shebang, LDoc's first-token rule; traversal, not membership
    Ok(Ok(matches!(
        earliest.kind_id().into(),
        Lua::Comment | Lua::HashBangLine
    )))
}

/// The first statement of a `block`, which stands for it: the grammar parks
/// a comment that precedes the first statement of an `if`, `do` or loop
/// body outside the `block`.
fn lua_block_first_statement<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    infallible(lua_block_first_with_budget(node, &NoBudget))
}

fn lua_block_first_with_budget<'a, B: WorkBudget>(
    node: &Node<'a>,
    budget: &B,
) -> Result<Option<Node<'a>>, B::Error> {
    if node.kind_id() == Lua::Block {
        for child in node.children() {
            budget.step()?;
            if child.is_named() {
                return Ok(Some(child));
            }
        }
    }
    Ok(None)
}

pub(crate) fn target_with_budget<B: WorkBudget>(
    node: &Node<'_>,
    budget: &B,
) -> Result<bool, B::Error> {
    if let Some(item) = lua_block_first_with_budget(node, budget)? {
        crate::roles::lua::target_with_budget(&item, budget)
    } else {
        crate::roles::lua::target_with_budget(node, budget)
    }
}

pub(crate) fn source_opener_with_budget<'a, B: WorkBudget>(
    run: &crate::documentation::streaming::positional::RunContext<'_, 'a>,
    ancestors: Ancestors<'a, '_>,
    budget: &B,
) -> Result<
    Result<
        Option<crate::documentation::streaming::positional::OpenerReason>,
        crate::documentation::UnresolvedReason,
    >,
    B::Error,
> {
    use crate::documentation::streaming::positional::OpenerReason;
    let Some(first) = run.members.first() else {
        return Ok(Ok(None));
    };
    if let Some(above) = ancestors.siblings_before_with_budget(first, budget)?.next()
        && LuaCode::is_comment(&above)
        && let Some(source) = run.source
        && !source.covers_with_budget(above.start_byte()..first.end_byte(), budget)?
    {
        return Ok(Err(crate::documentation::UnresolvedReason::ExcludedGap));
    }
    if !crate::roles::lua::opens_with_budget(first, run.code, ancestors, budget)? {
        return Ok(Ok(None));
    }
    let mut text = LdocText::default();
    for member in run.members {
        budget.step()?;
        text.read_with_budget(member, run.code, budget)?;
    }
    if !text.text {
        return Ok(Ok(None));
    }
    match lua_first_block_with_budget(first, run.code, ancestors, run.source, budget)? {
        Ok(true) => return Ok(Ok(Some(OpenerReason::FirstFile))),
        Err(reason) => return Ok(Err(reason)),
        Ok(false) => {}
    }
    if text.tag {
        return Ok(Ok(Some(OpenerReason::Tagged)));
    }
    if let Some(next) = run.below
        && target_with_budget(&next, budget)?
    {
        return Ok(Ok(Some(OpenerReason::FollowingItem)));
    }
    Ok(Ok(None))
}

impl Checker for LuaCode {
    const SOURCE_POSITIONAL_DOCS: bool = true;

    fn is_source_doc_target<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        let _ = (code, ancestors);
        infallible(target_with_budget(node, &NoBudget))
    }

    fn source_doc_run_opener<'a>(
        run: &crate::documentation::streaming::positional::RunContext<'_, 'a>,
        ancestors: Ancestors<'a, '_>,
    ) -> Result<
        Option<crate::documentation::streaming::positional::OpenerReason>,
        crate::documentation::UnresolvedReason,
    > {
        infallible(source_opener_with_budget(run, ancestors, &NoBudget))
    }
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
    impl_byte_less_func_space!(lua);

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::lua::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func_with_code<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::lua::func_opener::is_callable(node, code, ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::lua::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/lua.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(lua, call, is_call, is_call_with_code);
    impl_row_predicates!(lua, string_lit, is_string, is_string_with_code);

    // No `is_non_arg`: `impl NArgs for LuaCode` counts a parameter list
    // positively (`Identifier` and `VarargExpression`), so the trait's
    // negative filter is never consulted and the default `false` stands.

    // The `ElseIf` row of `roles/lua.rs`: the dedicated `elseif_statement`
    // itself is the else-if, never a nested `if` (#1697).
    impl_else_if_from_row!(lua);
}
