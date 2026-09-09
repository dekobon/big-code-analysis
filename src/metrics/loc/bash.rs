//! `Loc` implementation for Bash.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Loc for BashCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Bash::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // Every Bash literal that can span rows, so its interior rows
            // reach PLOC instead of being mislabelled blank by
            // `blank = sloc - ploc - cloc` (#1260) — the decision #778 took
            // for thirteen other languages and #415 took for Python.
            //
            // `String` looks covered without an arm and is not: the grammar
            // emits one `string_content` child per row that *has* text, so
            // `"a\n\nb"` credits rows 1 and 3 through those leaves and leaves
            // row 2 blank. Every other language routes the whole literal and
            // counts an empty interior row as code, so Bash does too.
            // `RawString` (`'…'`), `AnsiCString` (`$'…'`) and `HeredocBody`
            // are childless, so the leaf-gated `_` arm below reached only
            // their opening row. `TranslatedString` (`$"…"`) needs no arm: it
            // wraps a `String` that this one already covers.
            //
            // `HeredocBody2` is the parser-node symbol observed parse trees
            // actually carry; the duplicate `HeredocBody` entry is a
            // defensive arm that tree-sitter-bash 0.25.1 does not surface —
            // `src/checker/bash.rs` records the same finding for `is_string`
            // and omits it there (`.claude/rules/grammar-dispatch.md`
            // sections 1 and 2).
            //
            // The two rows this arm inserts before delegating are not
            // redundant. `add_multiline_string_ploc` *skips* the opening row
            // when the parent starts on it, which is safe only where the
            // catch-all credits every node's start row — and Bash's, below,
            // is leaf-gated, so a container parent contributes nothing. A
            // childless `raw_string` / `ansi_c_string` is then the only node
            // covering its own row, and the skip deletes it: `'ls'` alone in
            // a file reported `ploc 0, blank 1`. `LineSet::insert` is
            // idempotent, so pre-inserting costs nothing where the helper
            // would have inserted the row anyway.
            String | RawString | AnsiCString | HeredocBody | HeredocBody2 => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
                add_multiline_string_ploc(node, ancestors, stats, start);
            }
            // LLOC: leaf statement nodes. Pipeline, Subshell, and
            // RedirectedStatement are excluded because they wrap inner
            // Command nodes that are already counted here.
            Command | VariableAssignment | DeclarationCommand | UnsetCommand | IfStatement
            | ForStatement | CStyleForStatement | WhileStatement | CaseStatement
            | FunctionDefinition => {
                stats.lloc.count_logical_line();
            }
            _ => {
                if node.child_count() == 0 {
                    check_comment_ends_on_code_line(stats, start);
                    stats.ploc.lines.insert(start);
                }
            }
        }
    }
}
