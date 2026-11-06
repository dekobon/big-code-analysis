//! `Loc` implementation for Python.
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

use super::shared::{
    BareRow, BareRowComments, at_row_end, bare_row_start, continuing_backslash,
    opens_comment_only_line, past_horizontal_space, past_line_padding,
};
use super::*;
use crate::lang_helpers::python::{
    python_documentation_string_run, python_is_bare_statement_value, python_is_bare_string,
    python_is_bare_string_parenthesis, python_sphinx_doc_comment,
};

impl Loc for PythonCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        is_func_space: bool,
    ) {
        // Per-kind LOC dispatch for Python, one arm per node kind.
        use Python::*;

        let start = init(node, stats, is_func_space);

        match node.kind_id().into() {
            // `block` (both aliased kind_ids — `Block` 135, the dead
            // hidden supertype, and `Block2` 160, the only one emitted
            // today) is a structural wrapper that contributes no LOC of
            // its own: its opening row is always shared with the first
            // statement it wraps, which inserts that row via its own
            // arm. Route through `python_is_block` so both aliases
            // no-op identically at one normalization point — previously
            // `Block2` fell through to the `_` arm and re-inserted an
            // already-covered row (a HashSet duplicate); making the
            // no-op explicit is behaviour-preserving (issue #419).
            _ if python_is_block(node) => {}
            // `escape_sequence` and `escape_interpolation` are leaves inside
            // a string's content that spell no code of their own; their row
            // is billed by the string's arm. Left to the catch-all they
            // credited a docstring row to PLOC (#1597).
            StringStart | StringEnd | StringContent | EscapeSequence | EscapeInterpolation
            | Module => {}
            Comment => {
                add_comment_lines::<Self>(node, code, ancestors, stats);
                add_sphinx_doc_comment(node, code, ancestors, stats);
            }
            // An implicitly concatenated bare string (`"""doc""" "more"`)
            // is one string to CPython and to Halstead (#695): bill the
            // whole literal here, once; its fragments are skipped below. A
            // concatenated value falls to the catch-all.
            ConcatenatedString if python_is_bare_statement_value(node, ancestors) => {
                add_bare_string_lines(node, code, ancestors, stats);
            }
            // CPython drops the parentheses around a bare string, so they
            // are part of it (#1610): their rows are its comment rows, and
            // documentation rows when it is, which the string's arm credits.
            // The `parenthesized_expression` itself bills nothing.
            LPAREN | RPAREN if python_is_bare_string_parenthesis(node, ancestors) => {
                add_comment_rows(stats, node.start_row(), node.end_row(), false);
            }
            ParenthesizedExpression if python_is_bare_string_parenthesis(node, ancestors) => {}
            String => {
                if python_is_bare_string(node, ancestors) {
                    // A bare string statement is counted as comment lines
                    // (see issue #415 for the related classification caveat,
                    // intentionally left unchanged), and as documentation
                    // too when it sits where Python reads documentation. A
                    // fragment of a concatenated string was billed by the
                    // arm above.
                    if !ancestors.parent_has_kind(node, ConcatenatedString as u16) {
                        add_bare_string_lines(node, code, ancestors, stats);
                    }
                } else if ancestors.parent_has_kind(node, ExpressionStatement as u16) {
                    // A bare statement's literal that is not a docstring, such
                    // as an element of the discarded tuple `"""a\nb""", x`.
                    // The statement's arm credits no row, so the literal
                    // credits its own opening row instead of trusting
                    // `add_multiline_string_ploc`'s parent gate (#1597).
                    check_comment_ends_on_code_line(stats, start);
                    stats.ploc.lines.insert(start);
                    add_string_interior_ploc(node, stats, start);
                } else {
                    // A non-docstring string literal (assigned or passed as an
                    // argument). Its interior physical rows are real code, not
                    // blank lines (#415). The opening row is inserted only when
                    // the parent statement begins on an earlier row, otherwise
                    // that row is already attributed to the enclosing statement.
                    //
                    // This arm is the rule `add_multiline_string_ploc` was
                    // extracted from (#778), and stayed open-coded until #1423
                    // swept the last copies of the pattern onto the helper. The
                    // parent gate is unchanged — a `string` always has a parent
                    // here, so the helper's `is_none_or` reduces to the same
                    // test — and the interior bound moves from the raw end row
                    // to `Node::end_line`. That is a no-op in Python today: a
                    // `string` node always closes on a `string_end` quote, so
                    // its end column is never 0, and an unterminated literal
                    // parses to a bare `string_start` under an `ERROR` rather
                    // than to a `string` node at all.
                    add_multiline_string_ploc(node, ancestors, stats, start);
                }
            }
            // `match` (PEP 634) and `type` aliases (PEP 695) are real
            // statements that parse cleanly and previously fell through
            // to the `_` arm, contributing 0 logical lines (#462). Count
            // `MatchStatement` like `IfStatement`/`TryStatement` — the
            // construct itself adds one lloc; its `case_clause` children
            // are deliberately omitted, mirroring how `elif_clause`,
            // `else_clause`, `except_clause`, and `finally_clause` are
            // absent here (clauses do not add lloc; the statements they
            // contain do via their own arms). `TypeAliasStatement` is a
            // leaf statement, counted like an assignment expression.
            Statement
            | SimpleStatements
            | ImportStatement
            | FutureImportStatement
            | ImportFromStatement
            | PrintStatement
            | AssertStatement
            | ReturnStatement
            | DeleteStatement
            | RaiseStatement
            | PassStatement
            | BreakStatement
            | ContinueStatement
            | IfStatement
            | ForStatement
            | WhileStatement
            | TryStatement
            | MatchStatement
            | TypeAliasStatement
            | WithStatement
            | GlobalStatement
            | NonlocalStatement
            | ExecStatement
            | ExpressionStatement => {
                stats.lloc.count_logical_line();
            }
            // The grammar keeps `\` + newline as a node where the splicing
            // grammars drop it: a row holding only the `\` is the logical
            // line it belongs to, not code by default (#1819).
            LineContinuation => credit_python_line_continuation(node, code, ancestors, stats),
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

/// Bills a bare string statement's rows: as comment rows always (#415),
/// and as documentation rows (`dloc`) only in a docstring or attribute
/// docstring position (#1597) — so a triple-quoted string wrapping
/// commented-out code is charged by MI like a `#` comment.
fn add_bare_string_lines<'a>(
    value: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    add_comment_rows(stats, value.start_row(), value.end_row(), false);
    python_documentation_string_run(value, code, ancestors, |first, last| {
        stats.dloc.lines.insert_range(first, last);
    });
}

/// Credits a Sphinx `#:` doc comment, which documents the assignment
/// beside or below it, as `dloc`. Its rows are already comment rows, so
/// `dloc <= cloc` holds (#1597).
fn add_sphinx_doc_comment<'a>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    python_sphinx_doc_comment(comment, code, ancestors, |first, last| {
        stats.dloc.lines.insert_range(first, last);
    });
}

// Credits a Python `line_continuation` node's row, when it holds nothing
// but the `\`, as the logical line it belongs to (#1819).
//
// tree-sitter-python keeps the `\` + newline as an `extra` node — `\r\n`
// included, so the row is read from `node.end_byte()` — where the
// splicing grammars `credit_bare_continuation_rows` serves drop it, and
// the catch-all made every such row code. CPython reads it as the
// splicing lexers do: a bare row continuing
// a statement is that statement's code; one continuing nothing starts a
// logical line and is what that line becomes, a comment where its first
// token is a `#` comment (which runs to the row's end, so nothing can
// follow it), blank where it joins an empty row. A `#` comment swallows a
// `\` at its end, so a bare row after `# c \` starts a line of its own.
//
// A run of bare rows is one logical line, so the node on its first row
// classifies and credits the whole run, and every later node in it
// returns at once: consecutive extras between two tokens attach to the
// same parent, so the head's `stats` is the right space for every row.
// A node with code before its `\` credits nothing — the code's own
// tokens cover the row — which is the one observable change for a `\`
// ending a bare-string statement: that row is the string's comment row
// and was also code. `loc/python-long-bare-run` prices the run.
#[inline]
fn credit_python_line_continuation<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    let Some(first) = past_python_bare_run(node, code) else {
        return;
    };
    // The node ends on the row after its own — or on its own for the
    // `\` + NUL spelling, where `past_line_padding` stops at the NUL —
    // and every byte from there to `first` is a `\` splice or horizontal
    // space, so the newlines among them count the run's further rows.
    // When the joined row is empty, `first` is its newline, which is
    // after the last bare row's.
    let start = node.start_row();
    // The run is a few bytes per row; a `bytecount` dependency buys nothing.
    #[allow(clippy::naive_bytecount)]
    let further = code
        .get(node.end_byte()..first)
        .unwrap_or_default()
        .iter()
        .filter(|&&byte| byte == b'\n')
        .count();
    let last = start.saturating_add(further);
    match classify_python_run(node, code, ancestors, first) {
        BareRow::Comment => {
            for row in start..=last {
                // The string arm already made the row code — the run sits
                // in a multi-row f-string's interpolation, whose rows are
                // PLOC (#778) — so it is a comment on a code line, as
                // `add_cloc_lines` files one; filing it comment-only too
                // would subtract it from `blank` twice (`GapScan::credit`).
                if stats.ploc.lines.contains(row) {
                    add_code_comment_line(stats, row);
                } else {
                    add_only_comment_lines(stats, row, row);
                }
            }
        }
        // No `check_comment_ends_on_code_line`: nothing but `\` is on
        // these rows.
        BareRow::Code => stats.ploc.lines.insert_range(start, last),
        BareRow::Blank => {}
    }
}

// The byte past the run of bare `\` rows headed by `node` — the first
// byte of its logical line's next token, or of the row end where it
// joins an empty row — when `node` is a `line_continuation` holding its
// row alone and heads a run: the row before it does not end in a `\`
// that is itself alone on its row. `None` for a node with code before
// it and for a later row of a run, each in O(1): only the head reads
// the run.
fn past_python_bare_run(node: &Node, code: &[u8]) -> Option<usize> {
    let row_start = bare_row_start(code, node.start_byte())?;
    let continues_a_bare_row = continuing_backslash(code, row_start)
        .is_some_and(|last| bare_row_start(code, last).is_some());
    (!continues_a_bare_row).then(|| past_line_padding(code, node.end_byte()))
}

// What the run headed by `node` is: the code it continues when the row
// before it ends in a `\` outside a comment — the non-bare
// `line_continuation` of a statement — or else what the logical line it
// starts becomes, read off the token at `first`, where the line goes on.
//
// A `\` after a bare-string statement continues a line that holds no code
// yet, the string's rows being comment rows (#415), so the run is what
// the rows after it make it, a comment where they add none — as
// `GapScan::classify` reads a C-family `/* … */` before a `\`.
fn classify_python_run<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    first: usize,
) -> BareRow {
    let continued = bare_row_start(code, node.start_byte())
        .and_then(|row_start| continuing_backslash(code, row_start));
    let in_comment =
        continued.is_some_and(|last| python_backslash_in_comment(node, last, ancestors));
    match continued {
        Some(_) if in_comment && PYTHON_COMMENTS.continued => BareRow::Comment,
        // The comment's `\` is its text, so the run starts a line of
        // its own.
        Some(_) if in_comment => forward_python_run(node, code, first, ancestors),
        // The line holds the string, its parentheses and whatever the run
        // joins — a `)`, a comment, a fragment, an empty row — and no
        // code: a token after the string on its line makes it a value,
        // not bare. So the run is comment rows, where `GapScan::classify`
        // maps only a Blank forward reading to Comment: there the joined
        // row can hold code, and here it cannot.
        Some(last) if python_backslash_after_bare_string(node, code, last, ancestors) => {
            BareRow::Comment
        }
        Some(_) => BareRow::Code,
        None => forward_python_run(node, code, first, ancestors),
    }
}

// What the logical line the run headed by `node` starts becomes, read
// off the token at `first`: blank where the row it joins is empty, a
// comment where that token opens a comment-only line, code otherwise.
fn forward_python_run<'a>(
    node: &Node<'a>,
    code: &[u8],
    first: usize,
    ancestors: Ancestors<'a, '_>,
) -> BareRow {
    if at_row_end(code, first) {
        BareRow::Blank
    } else if python_joins_comment(node, code, first, ancestors) {
        BareRow::Comment
    } else {
        BareRow::Code
    }
}

// Whether the `\` at byte `last`, the non-bare `line_continuation` the
// run headed by `node` continues, ends a bare string statement's row —
// a row of comment rows (#415) that the run then extends. The token
// before the `\` is the last of the sibling before the run: the string
// itself when the `\` sits inside the parentheses of a parenthesised bare
// string (`("""doc""" \` + `\` + `)`, #1610) or a concatenation's
// fragment; otherwise the statement, or a `def` or `if` whose body ends
// with it, which the descent along last children reaches, as it cannot
// through a chain the walk has not built.
fn python_backslash_after_bare_string<'a>(
    node: &Node<'a>,
    code: &[u8],
    last: usize,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let Some(holder) = ancestors
        .siblings_before(node)
        .find(|sibling| sibling.kind_id() != Python::LineContinuation as u16)
        .filter(|holder| past_horizontal_space(code, holder.end_byte()) == last)
    else {
        return false;
    };
    if matches!(
        holder.kind_id().into(),
        Python::String | Python::ConcatenatedString
    ) {
        return python_is_bare_string(&holder, ancestors);
    }
    let mut node = Some(holder);
    while let Some(current) = node
        && current.end_byte() == holder.end_byte()
    {
        if python_is_bare_string_statement(&current) {
            return true;
        }
        node = current
            .child_count()
            .checked_sub(1)
            .and_then(|index| current.child(index));
    }
    false
}

// Whether `statement` is an `expression_statement` whose sole child is a
// string, past any parentheses around it: the shape `Checker::is_comment`'s
// bare-string reading and the `String` arm agree on, asked of the node
// alone — the chain-reading helpers answer for a node on the walk's
// chain, and this one is a sibling's descendant.
fn python_is_bare_string_statement(statement: &Node) -> bool {
    if statement.kind_id() != Python::ExpressionStatement || statement.child_count() != 1 {
        return false;
    }
    let mut value = statement.child(0);
    // CPython nests at most 200 parentheses (`MAXLEVEL`); the same cap
    // bounds `python_is_bare_string_parenthesis`'s descent.
    for _ in 0..=PYTHON_MAX_PARENTHESES {
        match value.map(|current| current.kind_id().into()) {
            Some(Python::String | Python::ConcatenatedString) => return true,
            Some(Python::ParenthesizedExpression) => {
                value = value.and_then(|paren| {
                    paren
                        .children()
                        .find(|child| child.is_named() && child.kind_id() != Python::Comment)
                });
            }
            _ => return false,
        }
    }
    false
}

// `MAXLEVEL` of CPython's tokenizer (`Parser/lexer/state.h`): how many
// parentheses one expression may nest in.
const PYTHON_MAX_PARENTHESES: usize = 200;

// Whether byte `last`, the `\` ending the row before `node`'s, is inside
// a comment. The comment may be the sibling before `node` or nested in
// it (a `def`'s last statement's trailing comment, a `decorator`'s), so
// the sibling containing `last` is asked for its descendant at that
// byte, as `GapScan::holder_of` does. One sibling is enough: a `\` on
// the row before a bare row is inside the node ending that row, or in
// padding, and a previous `line_continuation` ending on that row is
// non-bare and was rejected by the head test. The parent stands in only
// when no sibling contains `last`, which valid input never reaches —
// the scanner eats a `\` before a string start, and any other token
// before it is a sibling's — so the lookup is from a sibling, never
// from `module`, whose `descendant_for_byte_range` would scan its
// children from the first.
fn python_backslash_in_comment<'a>(
    node: &Node<'a>,
    last: usize,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let contains = |holder: &Node<'a>| holder.as_tree_sitter().byte_range().contains(&last);
    ancestors
        .siblings_before(node)
        .next()
        .filter(contains)
        .or_else(|| ancestors.parent(node).filter(contains))
        .and_then(|holder| {
            holder
                .as_tree_sitter()
                .descendant_for_byte_range(last, last.saturating_add(1))
        })
        .is_some_and(|covering| PYTHON_COMMENTS.lists(covering))
}

// Whether the token at byte `first`, where the run headed by `node` goes
// on, opens a comment-only line: the first sibling after the run is asked
// as a gap's next child is. A `#` comment is the rest of its row, so the
// line ends with it, and `only_comments_after`'s sibling step is not
// needed: the cursor handed down is rooted at `joined`, so that step
// could never succeed here anyway.
fn python_joins_comment<'a>(
    node: &Node<'a>,
    code: &[u8],
    first: usize,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    // Not #1601's non-extra skip: a comment is an extra too, and is what
    // this looks for.
    ancestors
        .siblings_after(node)
        .find(|sibling| sibling.kind_id() != Python::LineContinuation as u16)
        .is_some_and(|joined| {
            opens_comment_only_line(code, PYTHON_COMMENTS, &joined.cursor(), first)
        })
}

// Python's `#` comment, which swallows a `\` at its end, so a bare row
// after one starts a line of its own (`continued: false`).
const PYTHON_COMMENTS: BareRowComments = BareRowComments {
    is_comment: crate::roles::python::comment::lists_kind,
    continued: false,
};
