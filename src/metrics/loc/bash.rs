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
            // covered only by the wrapper.
            //
            // `HeredocBody` / `HeredocBody2` stay listed **alongside** the
            // wrapper rather than being replaced by it — section 6's "narrow
            // with a gate, never by deletion". `node-types.json` does list
            // `heredoc_body` only as a child of `heredoc_redirect`, but that
            // describes the well-formed grammar and says nothing about error
            // recovery, where tree-sitter-bash emits an **orphan** body with
            // no wrapper anywhere. A single-line compound carrying a heredoc
            // is the shape, and all three spellings are valid, executable
            // Bash:
            //
            //     f() { cat <<EOT; }        if true; then cat <<EOT; fi
            //     body                      x
            //     EOT                       EOT
            //
            // `bca dump` on the first gives an `{ERROR}` root whose direct
            // child is `{heredoc_body:218} from (2, 1) to (4, 1)`. Dropping
            // the body kinds left that node to the leaf-gated catch-all,
            // which credits only its start row, so the terminator row fell
            // to `blank` — #1412's own symptom, reintroduced. Keeping both
            // costs nothing where the wrapper does exist: the body's row
            // range is contained in the wrapper's, and `insert`,
            // `insert_range` and `check_comment_ends_on_code_line` are all
            // idempotent. #1398 was an unterminated-heredoc ERROR shape in
            // this same arm, so recovery trees were already known to reach
            // it.
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
            String | RawString | AnsiCString | HeredocBody | HeredocBody2 => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
                add_string_interior_ploc(node, stats, start);
            }
            // The wrapper, whose interior starts where the *body* does and
            // not one row below the `<<` (#1443).
            //
            // `heredoc_redirect` spans the command-line prefix as well as
            // the literal, and the grammar lets that prefix cross rows: a
            // `pipeline` is one of its children. Crediting from
            // `start + 1`, as the arm above does for a literal that *is*
            // its own span, then bills every prefix row as code —
            // including a blank or comment-only one.
            //
            // The first body row is one past the last row any non-body
            // child occupies, which collapses to `start + 1` for the
            // ordinary single-row prefix, so this is the same arithmetic
            // everywhere except the shape it exists for.
            //
            // **What is and is not reachable here — measured, because
            // the first reading of it was wrong.** A multi-row prefix
            // *is* valid Bash whenever the continuation carries content:
            // `cat <<EOT | \` + `  grep x`, and the `&&` form, both pass
            // `bash -n`. What is unreachable is a **blank or
            // comment-only** prefix row, which is the shape this bound
            // exists for: bash begins the body on the line after the one
            // carrying `<<`, so a bare `cat <<EOT |` + newline makes the
            // next line the body and the terminator is never found, and
            // a `\` continuation splices the rows rather than leaving a
            // blank one. All four spellings — bare and `\`-continued,
            // blank row and comment row — are rejected by `bash -n`,
            // while tree-sitter parses each as a pipeline inside the
            // wrapper.
            //
            // So the bound changes nothing for the valid multi-row
            // prefixes above (their continuation rows carry leaves the
            // catch-all credits) and corrects the malformed trees `bca`
            // is still asked to measure (#1398's argument).
            //
            // One valid shape *does* move, and it is not this arm's bug
            // to own: a continuation row holding only `\` has no leaf,
            // so nothing credits it and it now reads blank. That is a
            // general Bash gap, not a heredoc one — `echo a | \` + `\` +
            // `  grep b` reports it outside any heredoc too — which the
            // old blanket range happened to mask in this one position.
            // Tracked as #1445; do not paper over it here.
            HeredocRedirect => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
                stats.ploc.lines.insert_range(
                    heredoc_body_first_row(node, start),
                    node.end_line().saturating_sub(1),
                );
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

/// The first row of `redirect`'s heredoc body: one past the last row any
/// of its non-body children occupies.
///
/// `heredoc_redirect` covers the command-line prefix (`<<`, the marker,
/// and whatever the grammar hangs off the rest of the line) as well as
/// the literal, so the literal's own rows start below all of them. The
/// prefix is single-row in every runnable spelling, where this returns
/// `start + 1` and the caller behaves exactly as the sibling arm does.
///
/// Reading the *body* node's start row instead would be wrong for the
/// shape #1412 is about: `heredoc_body`'s span begins at the first body
/// row that has text and collapses to zero width when the body is empty
/// throughout, so it cannot say where the body *begins*. The prefix can,
/// because it is bounded by the row the marker sits on.
///
/// The floor keeps the range clear of the opening row the caller has
/// already credited, and it is **inert today**: every `heredoc_redirect`
/// the grammar emits carries the `<<` token, which starts on `start` at
/// a column above 0 and so ends at `start + 1` or later. Deleting the
/// floor fails no test — measured, 0 of 3,394 — which is why the
/// `debug_assert!` is here rather than a comment claiming the shape
/// cannot arise. It checks the premise on every heredoc of every walk,
/// so a grammar that ever emits a body-only wrapper reports that
/// directly instead of silently crediting rows above the literal.
fn heredoc_body_first_row(redirect: &Node, start: usize) -> usize {
    let after_prefix = redirect
        .children()
        .filter(|child| {
            !matches!(
                child.kind_id().into(),
                Bash::HeredocBody | Bash::HeredocBody2 | Bash::HeredocContent | Bash::HeredocEnd
            )
        })
        .map(|child| child.end_line())
        .max();

    debug_assert!(
        after_prefix.is_some_and(|row| row > start),
        "a heredoc_redirect at row {start} has no non-body child below its opening row"
    );

    after_prefix.unwrap_or(0).max(start.saturating_add(1))
}
