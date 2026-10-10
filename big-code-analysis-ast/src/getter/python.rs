//! `Getter` implementation for Python.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

/// The role of a `{` or `:`, which Python spells both as syntax and
/// inside an f-string's replacement field.
///
/// As syntax each is an operator (#1486). The interpolation opener
/// (`f"{x}"`, and a nested `{w}` inside a format spec) is spelling, not
/// an operation — the six interpolating languages settled that in #1314
/// and Python follows them. The `:` that opens a format spec
/// (`f"{x:>10}"`) introduces the format mini-language, whose text is
/// string content and counts as nothing, so its delimiter does not
/// count either.
fn brace_or_colon_role(kind: Python, parent: Option<Python>) -> TokenRole {
    use Python::*;

    match (kind, parent) {
        (LBRACE, Some(Interpolation | FormatExpression)) | (COLON, Some(FormatSpecifier)) => {
            TokenRole::Unknown
        }
        _ => TokenRole::Operator,
    }
}

impl Getter for PythonCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Python::FunctionDefinition => SpaceKind::Function,
            Python::ClassDefinition => SpaceKind::Class,
            Python::Module => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Python::*;

        let kind: Python = node.kind_id().into();
        // Bracket openers, `;` and `:` are vocabulary like every other
        // getter's (#1395, #1486); only the f-string replacement-field
        // spellings are withdrawn. An early return rather than an arm,
        // because the match below is one rustfmt declines to format.
        if matches!(kind, LBRACE | COLON) {
            let parent = ancestors.parent(node).map(|p| p.kind_id().into());
            return brace_or_colon_role(kind, parent);
        }

        match kind {
            // The `not` / `in` / `is` leaf tokens are operators on their own
            // (`not x`, `a in b`, `a is b`, `for x in y`), but the grammar
            // also nests them inside the compound `not in` (Notin) and
            // `is not` (Isnot) nodes. When the leaf's parent is one of those
            // compounds, the compound itself is classified as the single
            // operator below, so the leaf must yield Unknown — otherwise
            // `a not in b` would count `not` + `in` as two operators (#413).
            Not | In | Is => match ancestors.parent(node).map(|p| p.kind_id().into()) {
                Some(Notin | Isnot) => TokenRole::Unknown,
                _ => TokenRole::Operator,
            },
            Import | DOT | From | COMMA | As | STAR | GTGT | Assert | COLONEQ | Return | Def
            | Del | Raise | Pass | Break | Continue | If | Elif | Else | Async | For
            | While | Try | Except | Finally | With | DASHGT | EQ | Global | Nonlocal | Exec
            | AT | And | Or | PLUS | DASH | SLASH | PERCENT | SLASHSLASH | STARSTAR | PIPE
            | AMP | CARET | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | LTGT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | ATEQ | SLASHSLASHEQ | PERCENTEQ | STARSTAREQ
            | GTGTEQ | LTLTEQ | AMPEQ | CARETEQ | PIPEEQ | Yield | Print | LPAREN | LBRACK
            | SEMI
            // `not in` / `is not` compounds count as one operator each; the
            // inner Not/In/Is leaves are suppressed by the parent-guard arm
            // above (#413).
            | Notin | Isnot
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
            // `lambda`: count only the keyword token (Lambda3=73), not the
            // Lambda/Lambda2 expression nodes that wrap it, to avoid the same
            // node+keyword double count fixed for await (#413).
            | Lambda3 => {
                TokenRole::Operator
            }
            Identifier | Integer | Float | True | False | None => TokenRole::Operand,
            String => {
                // Docstring / module-level string statement: an `ExpressionStatement`
                // whose only child is the string. Skip those.
                let mut climb = ancestors.iter(node);
                let Some((parent, _)) = climb.next() else {
                    return TokenRole::Unknown;
                };
                if parent.kind_id() == ExpressionStatement && parent.child_count() == 1 {
                    return TokenRole::Unknown;
                }
                // Implicit-concatenation docstring (`"""doc""" "more"`): the
                // adjacent literals are wrapped in a `concatenated_string`,
                // which is itself the sole child of the docstring
                // `ExpressionStatement`. Without this arm each fragment would
                // count as a separate operand, making the docstring's N2
                // contribution depend on how many literals it was split into
                // (#695). Suppress every fragment of such a docstring.
                if parent.kind_id() == ConcatenatedString
                    && climb.next().is_some_and(|(grandparent, _)| {
                        grandparent.kind_id() == ExpressionStatement
                            && grandparent.child_count() == 1
                    })
                {
                    return TokenRole::Unknown;
                }
                // Regression #191: an f-string wraps `Interpolation` children
                // whose inner expressions are walked and counted separately.
                // Skip the wrapping literal to avoid double-counting (same
                // pattern as #180 for Bash/Elixir and #184 for PHP).
                Self::string_operand_type(node, &[Interpolation as u16])
            }
            _ => TokenRole::Unknown,
        }
    }

    get_operator!(Python);
}
