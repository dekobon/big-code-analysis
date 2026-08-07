//! `Loc` implementation for Objc.
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

impl Loc for ObjcCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Objc::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            // ObjC has no raw string literals. `instance_variables` is
            // the `{ … }` ivar block of an `@interface` /
            // `@implementation`; like `DeclarationList` it is a brace
            // container, not a logical line.
            DeclarationList | FieldDeclarationList | InstanceVariables | TranslationUnit => {}
            // A `string_literal` continued across rows can span multiple rows;
            // credit every spanned row to PLOC to match Python's #415 decision
            // (#778). ObjC has no raw string literals.
            StringLiteral => {
                add_multiline_string_ploc(node, ancestors, stats, start, end);
            }
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // C's statement set plus the ObjC `@throw` / `@try` /
            // `@synchronized` statements (each a dedicated node). There
            // is no `@autoreleasepool` node — only the `@autoreleasepool`
            // keyword token — so its inner statements are counted
            // individually and the block header adds no logical line.
            WhileStatement
            | SwitchStatement
            | CaseStatement
            | IfStatement
            | ForStatement
            | ReturnStatement
            | BreakStatement
            | ContinueStatement
            | GotoStatement
            | ThrowStatement
            | TryStatement
            | SynchronizedStatement
            | ExpressionStatement
            | ExpressionStatement2
            | LabeledStatement
            | StatementIdentifier => {
                stats.lloc.count_logical_line();
            }
            Declaration => {
                // A declaration in a `for`/`while`/`if` *header* (not its
                // braced body) is part of that statement's logical line,
                // so it must not add a second one. Mirrors the C / C++
                // gating exactly.
                if node.count_specific_ancestors::<ObjcCode>(
                    ancestors,
                    |node| {
                        matches!(
                            node.kind_id().into(),
                            WhileStatement | ForStatement | IfStatement
                        )
                    },
                    |node| node.kind_id() == CompoundStatement,
                ) == 0
                {
                    stats.lloc.count_logical_line();
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);

                // tree-sitter-objc inherits tree-sitter-cpp's unexpanded
                // macro handling: a single `PreprocArg` node spans the
                // whole macro argument, so every line it covers is PLOC.
                if let PreprocArg = node.kind_id().into() {
                    (node.start_row().saturating_add(1)..=node.end_row()).for_each(|line| {
                        stats.ploc.lines.insert(line);
                    });
                }
            }
        }
    }
}
