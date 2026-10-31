//! `Getter` implementation for Python.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

use crate::lang_helpers::python::{python_is_bare_string, python_is_bare_string_parenthesis};

/// The role of a `{`, `:` or `=`, which Python spells both as syntax
/// and inside an f-string's replacement field.
///
/// As syntax each is an operator (#1486). The interpolation opener
/// (`f"{x}"`, and a nested `{w}` inside a format spec) is spelling, not
/// an operation — the six interpolating languages settled that in #1314
/// and Python follows them. The `:` that opens a format spec
/// (`f"{x:>10}"`) introduces the format mini-language, whose text is
/// string content and counts as nothing, so its delimiter does not
/// count either. The self-documenting `=` (`f"{x=}"`) is a directive of
/// the same field: it asks for the expression's text to be printed and
/// assigns nothing, like the `!r` conversion beside it, which was
/// already unbilled (#1551).
fn replacement_field_role(kind: Python, parent: Option<Python>) -> TokenRole {
    use Python::*;

    match (kind, parent) {
        // role-bypass: allow(StringLit) — token role under an interpolation, not whether the node
        // is a literal
        (LBRACE | EQ, Some(Interpolation | FormatExpression)) | (COLON, Some(FormatSpecifier)) => {
            TokenRole::Unknown
        }
        _ => TokenRole::Operator,
    }
}

impl Getter for PythonCode {
    // The mapping is the `opens` annotations of `roles/python.rs`
    // (#1696); the rows read no bytes and no chain, so the byte-less
    // spelling forwards with none.
    fn get_space_kind(node: &Node) -> SpaceKind {
        Self::get_space_kind_with_code(node, &[], Ancestors::known(&[]))
    }

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::python::opens_space(node, code, ancestors)
    }

    keyword_roles!(python);

    paying_roles!(python, control_flow);

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Python::*;

        let kind: Python = node.kind_id().into();
        // Bracket openers, `;`, `:` and `=` are vocabulary like every
        // other getter's (#1395, #1486); only the f-string
        // replacement-field spellings are withdrawn. An early return
        // rather than an arm, because the match below is one rustfmt
        // declines to format.
        if matches!(kind, LBRACE | COLON | EQ) {
            let parent = ancestors.parent(node).map(|p| p.kind_id().into());
            return replacement_field_role(kind, parent);
        }
        // The parentheses around a bare string are discarded with it,
        // as CPython drops them (#1610).
        if kind == LPAREN && python_is_bare_string_parenthesis(node, ancestors) {
            return TokenRole::Unknown;
        }

        // The comparison operators — the compounds `not in` / `is not`
        // included — and `and` / `or` are not here: the `Comparison`
        // and `ShortCircuit` rows of `roles/python.rs` declare them and
        // `get_paying_role` bills each at its leaf (#1694).
        match kind {
            // The `not` / `in` / `is` leaf tokens are operators on their own
            // (`not x`, `a in b`, `a is b`, `for x in y`), but the grammar
            // also nests them inside the compound `not in` (Notin) and
            // `is not` (Isnot) nodes. When the leaf's parent is one of those
            // compounds, the compound itself is classified as the single
            // operator, so the leaf must yield Unknown — otherwise
            // `a not in b` would count `not` + `in` as two operators (#413).
            // The `Comparison` row pays a bare `in` / `is` only under the
            // `comparison_operator` wrapper, which is the gate the
            // counting consumers need; the `in` of a `for` clause and a
            // comprehension is the spelling the row leaves to this arm.
            // role-bypass: allow(Comparison) — the operator classification of the spellings the
            // row's `under ComparisonOperator` gate leaves out: a loop's `in`
            Not | In | Is => match ancestors.parent(node).map(|p| p.kind_id().into()) {
                Some(Notin | Isnot) => TokenRole::Unknown,
                _ => TokenRole::Operator,
            },
            // The `assert` / `for` / `while` / `except` / `elif` keywords
            // are the `Branch` / `Loop` / `Catch` / `ElseIf` rows' payers
            // and `get_paying_role` bills them (#1697). `if` stays: the
            // `Branch` row's payer gate declines the ternary's `if` so
            // cyclomatic does not count it beside the owner, while
            // Halstead bills that spelling as it always has (`if` and
            // `else` are the ternary's operators); the statement's `if`
            // reaches `get_paying_role` first and never gets here.
            // role-bypass: allow(Exit, Branch) — the operator classification of the `yield`
            // expression the row owns as an exit, and of the `if` spelling the `Branch`
            // row's `not_under_ternary` gate leaves out; neither a projection the row answers
            Import | DOT | From | COMMA | As | STAR | GTGT | COLONEQ | Return
            | Del | Raise | Pass | Break | Continue | If | Else | Async
            | Try | Finally | With | DASHGT | Global | Nonlocal | Exec
            | AT | PLUS | DASH | SLASH | PERCENT | SLASHSLASH | STARSTAR | PIPE
            | AMP | CARET | LTLT | TILDE
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | ATEQ | SLASHSLASHEQ | PERCENTEQ | STARSTAREQ
            | GTGTEQ | LTLTEQ | AMPEQ | CARETEQ | PIPEEQ | Yield | Print | LPAREN | LBRACK
            | SEMI
            // `match` / `case` keyword tokens (Match=26, Case=27), mirroring
            // the cyclomatic metric which already counts each `case` clause
            // and the Rust Halstead which counts `match` (#413).
            | Match | Case
            // `nonlocal` keyword token, for parity with `global` which was
            // already classified (#413).
            // `await`: count only the await-expression node (Await=237);
            // Await2 (keyword token 95) is the nested keyword and was being
            // double-counted, mirroring how `yield` counts only the Yield
            // node and not its keyword leaf (#413).
            | Await
            // `def`, `class` and `lambda`'s keyword token are not here:
            // `roles/python.rs` declares them and `get_keyword_role`
            // bills them (#1693).
            => {
                TokenRole::Operator
            }
            Identifier | Integer | Float | True | False | None => TokenRole::Operand,
            // role-bypass: allow(StringLit) — the operand classification of the literal
            // under the row's `wraps` list, a projection the row does not answer
            String => {
                // A bare string statement's string — its sole value, or a
                // fragment of an implicitly concatenated one — is discarded,
                // so it is not an operand (#695). The LOC walk bills the same
                // strings as comment rows through this predicate, so the two
                // agree on which strings are discarded; whether one is also
                // documentation is `python_documentation_string_run`'s
                // question, which Halstead deliberately does not ask (#1597).
                if python_is_bare_string(node, ancestors) {
                    return TokenRole::Unknown;
                }
                // Regression #191: an f-string wraps `Interpolation` children
                // whose inner expressions are walked and counted separately.
                // Skip the wrapping literal to avoid double-counting (same
                // pattern as #180 for Bash/Elixir and #184 for PHP).
                Self::string_operand_type(node, crate::roles::python::string_lit::WRAPS)
            }
            _ => TokenRole::Unknown,
        }
    }

    get_operator!(Python);
}
