//! `Cognitive` implementation for C#.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cognitive for CsharpCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        // One arm per construct the grammar can hand us, each a row read or a
        // kind the rows do not own: the count is the construct list, not
        // branching a reader must hold, and the effort is the table's vocabulary
        // (#1695, #1697).
        use crate::roles::csharp::{
            branch, catch, closure, else_if, func_opener, r#loop, short_circuit, switch, ternary,
        };
        use Csharp::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // An `if` the `Branch` row owns opens a level — unless the
            // `ElseIf` row recognises it as the `if` of an `else if`, where
            // the `Else` arm below pays the +1 and no level opens (#1697).
            _ if branch::owns_kind(kind, node, code, ancestors)
                && !else_if::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // Every loop, switch and handler the rows own (#1697).
            _ if r#loop::owns_kind(kind, node, code, ancestors)
                || switch::owns_kind(kind, node, code, ancestors)
                || catch::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // The `conditional_expression` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `else` is an anonymous keyword token. Each occurrence carries
            // a flat +1 for the alternative branch (matches Java's `Else`
            // handling).
            Else => {
                increment_by_one(stats);
            }
            // Per SonarSource Cognitive Complexity §B1, any `goto` (including
            // `goto label`, `goto case x`, `goto default`) is an unstructured
            // jump and adds +1. C#'s grammar does not allow labeled
            // `break`/`continue` (those forms are syntactically rejected), so
            // the only labeled-jump form to handle here is `goto_statement`.
            GotoStatement => {
                increment_by_one(stats);
            }
            // A `binary_expression` the `ShortCircuit` row owns (#1695).
            // The `NullSafe` row's `??` and the compound `??=` are not
            // read: the paper ignores null coalescing (the module header
            // of `cognitive.rs`, #1800). C# has no `&&=` / `||=`, so no
            // assignment arm is left.
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_from(node, stats, short_circuit::PAYERS);
            }
            // A lambda or anonymous method — the `Closure` row — charges
            // the lambda channel and does not reset nesting.
            _ if closure::owns_kind(kind, node, code, ancestors) => {
                nesting.lambda += 1;
            }
            // At a (possibly nested) function boundary — the `FuncOpener`
            // row's callables — reset structural nesting to zero and bump
            // the function-depth surcharge when this declaration is itself
            // nested inside another. C# local functions are the acute
            // case: a local function declared inside an `if` previously
            // inherited `nesting = 1`, inflating every control-flow
            // statement in its body by one (#696). The row lists
            // `local_function_statement` only: the
            // `_local_function_declaration` alias the arm used to name
            // beside it is the hidden rule the parser never emits.
            //
            // The stop list is this consumer's own, not the row's
            // `CALLABLE`: that slice names kinds gates aside, and an
            // indexer or property is a callable only in its accessor-less
            // form — a bodied one's accessors are the callables — so a
            // `get { if … }` under a property would otherwise read its
            // property as an enclosing function and take a depth
            // surcharge it never did (measured on the corpus, #1696).
            _ if func_opener::is_callable_kind(kind, node, code, ancestors) => {
                // role-bypass: allow(FuncOpener) — which enclosing kinds
                // give a nested callable its depth surcharge: the row's
                // ungated callables, not whether a node opens a space
                enter_function_boundary(
                    &mut nesting,
                    node,
                    ancestors,
                    &[
                        MethodDeclaration,
                        ConstructorDeclaration,
                        DestructorDeclaration,
                        OperatorDeclaration,
                        ConversionOperatorDeclaration,
                        AccessorDeclaration,
                        LocalFunctionStatement,
                    ],
                );
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
