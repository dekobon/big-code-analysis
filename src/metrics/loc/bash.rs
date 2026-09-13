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
            // for eighteen other languages and #415 took for Python.
            //
            // `String` looks covered without an arm and is not: the grammar
            // emits one `string_content` child per row that *has* text, so
            // `"a\n\nb"` credits rows 1 and 3 through those leaves and leaves
            // row 2 blank. Every other language routes the whole literal and
            // counts an empty interior row as code, so Bash does too.
            // `RawString` (`'…'`) and `AnsiCString` (`$'…'`) are childless,
            // so the leaf-gated `_` arm below reached only their opening
            // row. `TranslatedString` (`$"…"`) needs no arm: it wraps a
            // `String` that this one already covers.
            //
            // The heredoc is routed through its **wrapper**,
            // `heredoc_redirect`, rather than through `heredoc_body`, for
            // `.claude/rules/grammar-dispatch.md` section 6's keeper rule —
            // the same call #1396 made for PHP. `heredoc_body`'s span
            // excludes a leading empty body row and collapses to zero width
            // when the body is empty throughout, so those rows sat inside no
            // node at all and `blank = sloc - ploc - cloc` claimed them
            // (#1412). `bca dump` on `cat <<EOT\n\nEOT\n`:
            //
            //     {heredoc_redirect:213} from (1, 5) to (3, 4)
            //       {<<:36}              from (1, 5) to (1, 7)
            //       {heredoc_start:152}  from (1, 7) to (1, 10)
            //       {heredoc_body:218}   from (3, 1) to (3, 1)
            //       {heredoc_end:156}    from (3, 1) to (3, 4)
            //
            // The body node is on the *terminator's* row and empty; row 2 is
            // covered only by the wrapper. `HeredocBody` / `HeredocBody2`
            // are dropped rather than kept alongside it, because the wrapper
            // is a strict superset in every spelling: tree-sitter-bash
            // 0.25.1's `node-types.json` lists `heredoc_body` as a child of
            // `heredoc_redirect` and of nothing else, and the wrapper runs
            // from `<<` on the command row to the end of `heredoc_end`, so
            // its interior range already covers every body row and the
            // terminator. Dumped and confirmed for `<<`, `<<-`, quoted
            // (`<<'EOT'`), empty, unterminated, and heredocs inside a
            // function, subshell, pipeline, command substitution and
            // `&&` list.
            //
            // Routing the wrapper also fixes a body that *has* text:
            // `heredoc_content` is itself multi-row (`(64, 1)` to
            // `(65, 24)` in the corpus's `generate-pc.sh`), so the
            // leaf-gated catch-all credited only its first row.
            //
            // A knowing divergence from section 7's parity cross-walk:
            // `BashCode::is_string` (`big-code-analysis-ast/src/checker/bash.rs`)
            // does **not** list `HeredocRedirect`, and must not — a
            // redirection is an operator, not a string literal, and
            // `find string` / Halstead operand classification would be wrong
            // to report one. This arm answers a different question, "which
            // physical rows hold source text", and the wrapper is the node
            // that answers it. PHP's arm agrees with its `is_string` because
            // there the keeper (`heredoc`) *is* the literal; here it is not.
            //
            // This arm owns its opening row, which is why it calls
            // `add_string_interior_ploc` rather than the parent-gated
            // `add_multiline_string_ploc` its siblings use. That
            // gate *skips* the opening row when the parent starts on it,
            // which is safe only where the catch-all credits every node's
            // start row — and Bash's, below, is leaf-gated, so a container
            // parent contributes nothing. A childless `raw_string` /
            // `ansi_c_string` is then the only node covering its own row,
            // and the skip deletes it: `'ls'` alone in a file reported
            // `ploc 0, blank 1`. Re-derived for `heredoc_redirect`: its
            // parent `redirected_statement` starts on the same row, so the
            // gate would skip there too — harmless only because the `<<`
            // token is a leaf on that row and the catch-all picks it up.
            // Depending on that is a second rule for no gain, so the
            // wrapper takes the unconditional form its siblings do.
            String | RawString | AnsiCString | HeredocRedirect => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
                add_string_interior_ploc(node, stats, start);
            }
            // An assignment standing as a statement of its own is one
            // logical line — but only then.
            //
            // `VariableAssignment2` is the id observed parse trees
            // actually carry for `variable_assignment`, in every
            // position: at top level, in a `compound_statement` or
            // `subshell` or `do_group`, under a `declaration_command`
            // (`local n=5`, `export A=1`, `declare -a B=(1 2)`,
            // `readonly C=3`) and as a `command`'s environment prefix
            // (`X=1 cmd`). The unsuffixed `VariableAssignment` below is
            // never emitted, so listing only it scored `a=1` zero
            // logical lines (`.claude/rules/grammar-dispatch.md` §1);
            // it stays as a defensive arm rather than being swapped.
            //
            // The last two positions need the parent gate. Both
            // `declaration_command` and `command` are counted as
            // logical lines by the arm below, and the assignment is
            // *part* of that line rather than another one, so listing
            // the kind ungated billed `local n=5` twice (§5's
            // container/leaf double count). Gated rather than dropped
            // per §6: the standalone spelling has no other arm to fall
            // back on.
            VariableAssignment2
                if !ancestors.parent(node).is_some_and(|p| {
                    matches!(p.kind_id().into(), DeclarationCommand | Command)
                }) =>
            {
                stats.lloc.count_logical_line();
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
