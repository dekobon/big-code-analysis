//! `Loc` implementation for Csharp.
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

impl Loc for CsharpCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Csharp::*;

        let (start, end) = init(node, stats, is_func_space);
        let kind_id: Csharp = node.kind_id().into();
        match kind_id {
            CompilationUnit => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // A C# verbatim (`@"…"`) or raw (`"""…"""`) string literal can span
            // several rows; credit every spanned row to PLOC to match Python's
            // #415 decision (#778).
            VerbatimStringLiteral | RawStringLiteral => {
                add_multiline_string_ploc(node, ancestors, stats, start);
            }
            BreakStatement | CheckedStatement | ContinueStatement | DoStatement
            | ExpressionStatement | FixedStatement | ForStatement | ForeachStatement
            | GotoStatement | IfStatement | LabeledStatement | LockStatement | ReturnStatement
            | SwitchStatement | ThrowStatement | TryStatement | UnsafeStatement
            | UsingStatement | WhileStatement | YieldStatement => {
                stats.lloc.count_logical_line();
            }
            LocalDeclarationStatement => {
                // Variable declarations inside a `for_statement` init/condition/update
                // (e.g. `for (int i = 0; i < n; i++)`) shouldn't bump LLOC; the
                // surrounding `for_statement` already counts.
                if node.count_specific_ancestors::<CsharpCode>(
                    ancestors,
                    |n| n.kind_id() == ForStatement,
                    |n| n.kind_id() == Block,
                ) == 0
                {
                    stats.lloc.count_logical_line();
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);

                // **No `PreprocArg` arm here, unlike the four C-family
                // siblings — a decided gap, not an oversight (#1430).**
                //
                // C# has a `preproc_arg` (`Csharp::PreprocArg`, 185, no
                // numeric-suffix aliases) and `c.rs` / `cpp.rs` /
                // `mozcpp.rs` / `objc.rs` all route theirs through
                // `add_string_interior_ploc` so a multi-row macro body
                // reaches PLOC. Without the arm a multi-row `preproc_arg`
                // credits only its start row and the rest fall to
                // `blank = sloc - ploc - cloc`. Measured:
                //
                //     #region Big \
                //       section
                //     #endregion
                //     class C {}
                //
                // parses `{preproc_arg:185} from (1, 9) to (2, 10)` and
                // reports `sloc 4, ploc 3, blank 1` — row 1 is text scored
                // as blank.
                //
                // The arm is absent because **no valid C# can reach the
                // shape**. The C# specification terminates a `pp-directive`
                // at the new-line and defines no line continuation, so
                // `#region Big \` ends at the newline and `  section` below
                // it is a syntax error, not an argument row.
                // `tree-sitter-c-sharp` is over-permissive relative to the
                // language here, accepting a backslash continuation the way
                // the C grammar legitimately does. A `preproc_arg` in
                // runnable C# is always single-row, and on a single row the
                // arm would no-op — so adding it could only change the
                // number reported for source that does not compile, where a
                // line count has no correct answer to be right about.
                //
                // This is the opposite call to #1443, which fixed a Bash
                // shape that was also unreachable — a blank prefix row in a
                // heredoc. The difference is what the arm would buy: there,
                // the range was wrong in a way that made the *meaning* wrong
                // ("the literal's rows" included command-prefix rows), and
                // the valid multi-row prefixes were unaffected. Here the
                // meaning is already right, and only source that does not
                // compile can tell the two behaviours apart.
                //
                // If `tree-sitter-c-sharp` ever tightens to match the
                // specification, this note and #1430 both become moot. If
                // C# ever *gains* a continuation, add the arm — it is two
                // lines, and its four siblings are the template.
            }
        }
    }
}
