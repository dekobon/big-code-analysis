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
    python_is_bare_string_parenthesis, python_is_bare_string_statement, python_sphinx_doc_comment,
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
//
// Where the run is a bare-string statement's line — its `\` ends the
// statement's row, or the run sits inside the string's parentheses
// (#1610) — it is, with no code before the string on it, what the rows
// after the run make it (`classify_python_run`): the comment the
// string's rows are (#415) unless a `;` starts a statement there.
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
// A `\` after a bare-string statement continues that statement's logical
// line, whose rows are comment rows (#415) — unless code precedes the
// string on it (`x = 1; """doc""" \`), where the run is that code's as
// after any statement. With none, the line holds no code yet and is what
// the rows after the run make it, as the C family's `BareRowIndex` reads
// a `/* … */` before a `\` (#1818).
//
// A run that starts a line — after no `\`, or after one a `#` comment
// swallowed — can start it inside a bare string's parentheses (#1610),
// where it is the string's line as much as a run its `\` opens (third
// re-review of #1819): every path asks `python_on_bare_strings_line`
// and reads the line through `python_bare_string_run`.
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
        Some(_) if in_comment => python_starting_run(node, code, first, ancestors),
        Some(last) if python_on_bare_strings_line(node, code, Some(last), ancestors) => {
            python_bare_string_run(node, code, first, ancestors)
        }
        Some(_) => BareRow::Code,
        None => python_starting_run(node, code, first, ancestors),
    }
}

// What a run starting a logical line is: the bare string's line, where
// it starts it inside the string's parentheses (#1610), else what the
// token at `first` makes it.
fn python_starting_run<'a>(
    node: &Node<'a>,
    code: &[u8],
    first: usize,
    ancestors: Ancestors<'a, '_>,
) -> BareRow {
    if python_on_bare_strings_line(node, code, None, ancestors) {
        python_bare_string_run(node, code, first, ancestors)
    } else {
        forward_python_run(node, code, first, ancestors)
    }
}

// Whether the run headed by `node` is on a bare string statement's
// logical line with no code before the string on it: the string is
// found from the `\` at `backslash` ending the statement's row, which
// the run continues, or with none from the parentheses the run sits
// inside. Code before the string makes the run that code's, as after
// any statement.
fn python_on_bare_strings_line<'a>(
    node: &Node<'a>,
    code: &[u8],
    backslash: Option<usize>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    match backslash {
        Some(last) => python_backslash_after_bare_string(node, code, last, ancestors)
            .is_some_and(|line| !python_code_before_string(node, code, &line, ancestors)),
        None => python_run_inside_bare_string_parentheses(node, code, ancestors),
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

// What a run on a bare string statement's logical line, with no code
// before the string on it (`python_on_bare_strings_line`), is: what the
// token at `first` makes that line. A comment leaves it the string's; a
// `;` starts another statement on it, code; an empty row leaves it as it
// is, the Blank reading `GapScan::classify` maps to Comment too. A
// string there concatenates and takes the `\` with it (no node);
// anything else is a SyntaxError — except the string's own parentheses
// (#1610), which no C-family line has: a `)` closes the string, and the
// line is what follows it; a `(` before the string (#1830) opens it, and
// the line is the string's.
fn python_bare_string_run<'a>(
    node: &Node<'a>,
    code: &[u8],
    first: usize,
    ancestors: Ancestors<'a, '_>,
) -> BareRow {
    match forward_python_run(node, code, first, ancestors) {
        BareRow::Blank => BareRow::Comment,
        BareRow::Code => python_past_string_parentheses(node, code, first, ancestors),
        BareRow::Comment => BareRow::Comment,
    }
}

// Whether the run headed by `node` sits inside a bare string statement's
// parentheses (#1610), before or after the string, with no code before
// the string on its logical line. The run's parent is one of the
// statement's `parenthesized_expression`s, and the answer is that
// parenthesis's alone: every run it holds is on the string's line. So it
// is asked of the parenthesis, memoised per parenthesis under the walk's
// cache, and what it reads at the run's own level is nothing — before
// the string there are only the string's `(`, comments and `\` splices,
// which `python_code_among` skips, so the levels above the parenthesis
// answer, climbed from it as `python_code_before_string` climbs from a
// run. Reading the holder off the siblings before the run and climbing
// from there walked every earlier run and comment in the parenthesis,
// once per run, quadratic in the runs it holds (fourth re-review of
// #1819; `loc/python-separated-bare-runs` prices it).
//
// Which side of the string the run is on, the memo need not say: the
// caller reads the line forward from the run's own next token, which no
// other run shares, and only the token differs — a comment, an empty
// row or the string's `)` after it; a comment, an empty row or an inner
// parenthesis of the string's before it (#1830,
// `python_past_string_parentheses`). Until #1830 a run before the string
// made `python_is_bare_string_parenthesis` read the `\` as the
// parenthesis's value and answer no; `loc/python-bare-runs-before-string`
// prices the string's parentheses asked once per run across a wide
// prefix of them.
fn python_run_inside_bare_string_parentheses<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let Some((parent, chain)) = ancestors.iter(node).next() else {
        return false;
    };
    ancestors.memoised_answer(NodeQuestion::BareStringParenthesisLine, &parent, || {
        python_is_bare_string_parenthesis(&parent, chain)
            && !python_code_before_node(code, &parent, chain)
    })
}

// What the line a run headed by `node` joins at `first` holds past the
// bare string's closing parentheses there, when the token at `first` is
// one: a comment, or where they end the row, the string's own line;
// code otherwise, the token at `first` included when it is no such
// parenthesis. Each parenthesis is a sibling of the run or of one of its
// ancestors — the run sits inside the innermost it closes — so the walk
// steps up a level where one runs out, at most the 200 CPython nests.
//
// A run before the string (`( \` + `\` + `("""doc""")`, #1830) joins an
// inner parenthesis of the string's instead — between the string's `(`
// and the string only its parentheses, comments and `\` splices are
// grammatical, and a `\` right before the string itself leaves no node
// (#1825), so the string is never the token here. The inner parenthesis
// is stepped over whole, as a `)` is, and the row is read on past it:
// `("doc")); x=1` holds code after the string's closing parentheses as
// `); x=1` after the string does.
fn python_past_string_parentheses<'a>(
    node: &Node<'a>,
    code: &[u8],
    first: usize,
    ancestors: Ancestors<'a, '_>,
) -> BareRow {
    let mut after = first;
    let mut level = Some((*node, ancestors));
    while let Some((at, chain)) = level {
        let tokens = chain
            .siblings_after(&at)
            .filter(|sibling| sibling.kind_id() != Python::LineContinuation);
        for sibling in tokens {
            // A `)` closes the string's line; an inner `(…)` before the
            // string opens it (#1830). Either is stepped over.
            let strings_own = sibling.start_byte() == after
                && matches!(
                    sibling.kind_id().into(),
                    Python::RPAREN | Python::ParenthesizedExpression
                )
                && python_is_bare_string_parenthesis(&sibling, chain);
            if !strings_own {
                return python_token_row(&sibling);
            }
            after = past_line_padding(code, sibling.end_byte());
            if at_row_end(code, after) {
                return BareRow::Comment;
            }
        }
        level = chain.iter(&at).next();
    }
    BareRow::Code
}

// What a line whose next token is `token` is: a comment where `token` is
// a `#` comment, which runs to the row's end, code otherwise.
fn python_token_row(token: &Node) -> BareRow {
    if PYTHON_COMMENTS.lists(token.as_tree_sitter()) {
        BareRow::Comment
    } else {
        BareRow::Code
    }
}

// A bare string statement's line that a run is on, as
// `python_backslash_after_bare_string` finds it from the `\` ending the
// statement's row: `holder` is the sibling before the run whose last
// token the `\` follows — the string itself, a concatenation, a
// parenthesis or the statement — and `deepest`, where the statement
// lies below the holder, the deepest node on the descent to it that has
// siblings before it, with its parent — the level whose tokens are
// nearest the statement.
struct BareStringBackslash<'a> {
    holder: Node<'a>,
    deepest: Option<(Node<'a>, Node<'a>)>,
}

// The `\` at byte `last`, the non-bare `line_continuation` the run
// headed by `node` continues, when it ends a bare string statement's row
// — a row of comment rows (#415) that the run then extends. The token
// before the `\` is the last of the sibling before the run: the string
// itself, a concatenation's fragment or an inner parenthesis when the
// `\` sits inside the parentheses of a parenthesised bare string
// (`("""doc""" \` + `\` + `)`, `(("""doc""") \`, #1610), or the
// string's own `(` when the run comes before the string (`( \` + `\` +
// `"""doc"""`, #1830); otherwise the statement, or a `def` or `if` whose
// body ends with it, which the descent along last children reaches, as
// it cannot through a chain the walk has not built.
fn python_backslash_after_bare_string<'a>(
    node: &Node<'a>,
    code: &[u8],
    last: usize,
    ancestors: Ancestors<'a, '_>,
) -> Option<BareStringBackslash<'a>> {
    let holder = ancestors
        .siblings_before(node)
        .find(|sibling| sibling.kind_id() != Python::LineContinuation as u16)
        .filter(|holder| past_horizontal_space(code, holder.end_byte()) == last)?;
    // The holder shares the run's parent, so the run's chain is its own.
    let inside = match holder.kind_id().into() {
        Python::String | Python::ConcatenatedString => python_is_bare_string(&holder, ancestors),
        // The string's `(` holds the `\` ending its own row (#1830).
        Python::ParenthesizedExpression | Python::LPAREN => {
            python_is_bare_string_parenthesis(&holder, ancestors)
        }
        _ => false,
    };
    if inside {
        return Some(BareStringBackslash {
            holder,
            deepest: None,
        });
    }
    let mut deepest = None;
    let mut current = holder;
    while current.end_byte() == holder.end_byte() {
        if python_is_bare_string_statement(&current) {
            return Some(BareStringBackslash { holder, deepest });
        }
        let Some(child) = current
            .child_count()
            .checked_sub(1)
            .and_then(|index| current.child(index))
        else {
            break;
        };
        if current.child_count() > 1 {
            deepest = Some((current, child));
        }
        current = child;
    }
    None
}

// Whether code precedes the bare string statement whose `\` `line`
// records on its logical line, so the run is that code's — the question
// the C family's `BareRowIndex` settles for a bare row (#1818). The
// nearest token before the statement decides, at the deepest
// level that has one: the descent's, then the holder's (its siblings
// before, continuing the run's), then each of the run's ancestors' among
// its own siblings; a level with no token before its node leaves the
// question to the one above, and the root answers no. Each level is a
// seek among its siblings, so a run head costs its depth at most, and
// one on a line that holds a token stops at the first level.
fn python_code_before_string<'a>(
    node: &Node<'a>,
    code: &[u8],
    line: &BareStringBackslash<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    if let Some((parent, child)) = line.deepest
        && let Some(answer) = python_code_among(code, parent.children_before(&child), &child, None)
    {
        return answer;
    }
    let before_holder = ancestors
        .siblings_before(node)
        .skip_while(|sibling| sibling.id() != line.holder.id())
        .skip(1);
    if let Some(answer) = python_code_among(code, before_holder, &line.holder, Some(ancestors)) {
        return answer;
    }
    ancestors
        .iter(node)
        .next()
        .is_some_and(|(parent, chain)| python_code_before_node(code, &parent, chain))
}

// Whether code precedes `node` on its logical line: among its own
// siblings, or at the deepest level above it that has a token before
// its node. The levels `python_code_before_string` climbs from the
// run's parent once the run's own level is read, and every level a
// parenthesis's memoised answer reads — the parenthesis's inside, where
// nothing before the string can be code, is never one of them.
//
// The levels above `node` are folded down the walk's chain rather than
// climbed. A parenthesis whose own level holds only comments and `\`
// splices before it defers to the level above, and so does that one:
// asked from each of `n` nested parentheses, the climb read every level
// above each, `Σ` of their extras, quadratic in `n` under CPython's
// 200-level cap (#1863; `loc/python-deep-paren-extra-prefix` prices it).
fn python_code_before_node<'a>(code: &[u8], node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    python_code_at_level(code, node, ancestors).unwrap_or_else(|| {
        ancestors.nearest_level_answer(
            node,
            LevelLane::CodeBeforeOnLine,
            code,
            python_code_at_level,
        )
    })
}

// `python_code_among` for `level` among its own siblings: one level of
// `python_code_before_node`'s reading, a function of the level alone.
fn python_code_at_level<'a>(
    code: &[u8],
    level: &Node<'a>,
    chain: Ancestors<'a, '_>,
) -> Option<bool> {
    python_line_levels_read::record();
    python_code_among(code, chain.siblings_before(level), level, Some(chain))
}

// Levels `python_code_before_node` reads for a token before their node.
// The fold and the climb answer the same, so only this tells a walk that
// reads each level once from one that reads every level above every
// parenthesis (#1863).
crate::observation::counter!(python_line_levels_read);

// Whether the nearest of `siblings` (nearest first) that is neither a
// `\` splice nor the bare string's own parenthesis ends on `next`'s
// logical line — only padding and splices between the two, a splice the
// scanner eats before a string start (#1825) read like any other — and
// so is code before the string; `None` when the level has no such
// token. A `#` comment is skipped too: inside the string's parentheses
// the newline after it is an NL, not a NEWLINE — the logical line runs
// on past it as past the `(` (`x=1; ( # c` + `"""doc"""`), so the token
// before it, or the level above, answers; and it never ends a line by
// itself anywhere, since the padding read from any token before it
// stops at its `#` — outside parentheses the `:` before every block is
// such a token, so the level above is never asked. Reading it as the
// line's end answered no for the code before the `(` (second re-review
// of #1819). `parens` is the siblings' chain at a level where one of
// them can be the string's parenthesis — the run sits inside it, so the
// holder's level and above — and `None` on the descent below the holder,
// which stops at the statement, above its parentheses.
fn python_code_among<'a>(
    code: &[u8],
    siblings: impl Iterator<Item = Node<'a>>,
    next: &Node<'a>,
    parens: Option<Ancestors<'a, '_>>,
) -> Option<bool> {
    let mut tokens = siblings.skip_while(|sibling| match sibling.kind_id().into() {
        Python::LineContinuation | Python::Comment => true,
        Python::LPAREN | Python::RPAREN => {
            parens.is_some_and(|chain| python_is_bare_string_parenthesis(sibling, chain))
        }
        _ => false,
    });
    tokens
        .next()
        .map(|token| past_line_padding(code, token.end_byte()) == next.start_byte())
}

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
    // Not the non-extra skip of #1572 (`is_extra() && !is_error()`): a
    // comment is an extra too, and is what this looks for.
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
