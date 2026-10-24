//! `Cognitive` implementation for Groovy.
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

impl Cognitive for GroovyCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        // bca: suppress(cyclomatic, halstead) — exhaustive kind dispatch table
        // One arm per grammar kind, like `CppCode::compute`: the count is
        // the number of node kinds the grammar can hand us, not branching
        // a reader must hold, and the effort is the table's vocabulary;
        // the row reads (#1695) are arms like the kind arms they replaced.
        use crate::roles::groovy::{null_safe, short_circuit, ternary};
        use Groovy::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `for_in_statement` is the dekobon grammar's distinct node
            // for `for (x in xs)` / `for (Foo x : xs)` (the prior amaanq
            // grammar called this `enhanced_for_statement`); `do_while`
            // and `switch_block` keep their familiar names.
            ForStatement | ForInStatement | WhileStatement | DoWhileStatement | SwitchBlock
            | CatchClause => {
                increase_nesting(stats, &mut nesting);
            }
            // The `ternary_expression` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `Else` covers plain `else` blocks *and* the chained
            // `else if` form, because the grammar inlines the
            // `else` token before the nested `if_statement` rather
            // than wrapping it in an `else_clause` node.
            Else => {
                increment_by_one(stats);
            }
            // SonarSource B2: labeled break/continue each +1 for breaking
            // structured control flow. Same shape as Java.
            BreakStatement | ContinueStatement if node.is_child(Identifier as u16) => {
                increment_by_one(stats);
            }
            // A `binary_expression` the `ShortCircuit` row owns (#1695).
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_from(node, stats, short_circuit::PAYERS);
            }
            // Groovy's Elvis `?:` is a short-circuit nullish operator
            // analogous to Kotlin's `?:` (#239) and JS `??`. Per
            // SonarSource B1, a chain of identical short-circuit
            // operators contributes a single boolean-sequence increment
            // — the same rule as `&&` / `||`. The dekobon grammar
            // models Elvis as a distinct `elvis_expression` node
            // rather than a Java-shaped `ternary_expression` with a
            // missing consequence (closes #246): the `NullSafe` row's
            // owner whose payer is `?:`. The row's three safe
            // navigations are no sequence, and their tokens never match
            // the key.
            // role-bypass: allow(NullSafe) — the sequence key, the one of the row's four payers
            // that is a boolean operator
            _ if null_safe::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_with(node, stats, |id| matches!(id.into(), QMARKCOLON));
            }
            // Groovy closures are first-class (`list.each { … }`,
            // `def c = { … }`); control flow nested inside one must pay
            // the same lambda-nesting surcharge as Java's
            // `LambdaExpression`, so the byte-equivalent construct scores
            // identically across languages (lesson #11). The grammar node
            // is `Groovy::Closure` — also recognized by `is_closure` and
            // the `nargs` `closure_parameters` path.
            Closure => {
                nesting.lambda += 1;
            }
            // At a (possibly nested) method / constructor boundary, reset
            // structural nesting to zero and bump the function-depth
            // surcharge when this declaration is itself nested inside
            // another — matching Rust and the 9-of-13 sibling
            // families. Groovy methods can nest inside inner classes; a
            // nested method previously inherited the enclosing nesting and
            // missed the SonarSource B-nesting amplification (#696).
            // `static { … }` — see `java.rs`, which this mirrors (#1184).
            MethodDeclaration | ConstructorDeclaration | StaticInitializer => {
                enter_function_boundary(
                    &mut nesting,
                    node,
                    ancestors,
                    &[MethodDeclaration, ConstructorDeclaration, StaticInitializer],
                );
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
