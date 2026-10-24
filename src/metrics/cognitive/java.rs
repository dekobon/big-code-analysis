//! `Cognitive` implementation for Java.
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

impl Cognitive for JavaCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use crate::roles::java::{short_circuit, ternary};
        use Java::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // A `binary_expression` the `ShortCircuit` row owns: its
            // `&&` / `||` token — the row's payer — joins or opens a
            // boolean sequence (#1694).
            _ if short_circuit::owns(node, code, ancestors) => {
                compute_booleans_with(node, stats, |id| short_circuit::PAYERS.contains(&id));
            }
            ForStatement | EnhancedForStatement | WhileStatement | DoStatement | SwitchBlock
            | CatchClause => {
                increase_nesting(stats, &mut nesting);
            }
            // The `ternary_expression` the `Ternary` row owns (#1695).
            _ if ternary::owns(node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `Else` here is the `else` keyword token, which the grammar
            // also emits for the `else` of an `else if` — so this arm
            // covers both.
            Else => {
                increment_by_one(stats);
            }
            // Per SonarSource Cognitive Complexity §B2, labeled `break LABEL`
            // and `continue LABEL` each add +1 for breaking the structured
            // control flow. Plain `break;` / `continue;` are not penalized.
            BreakStatement | ContinueStatement if node.is_child(Identifier as u16) => {
                increment_by_one(stats);
            }
            LambdaExpression => {
                nesting.lambda += 1;
            }
            // At a (possibly nested) method / constructor boundary, reset
            // structural nesting to zero and bump the function-depth
            // surcharge when this declaration is itself nested inside
            // another — matching Rust and the 9-of-13 sibling
            // families. Without this, a method declared inside a control
            // construct (Java local / member classes) inherited the
            // enclosing nesting and every nested method missed the
            // SonarSource B-nesting amplification (#696).
            //
            // A record's compact constructor is its own kind
            // (`compact_constructor_declaration`) rather than a
            // `constructor_declaration`, so it needs listing in both the
            // arm and the `stops` set — otherwise its body's control flow
            // is charged to the enclosing class space (#1160).
            // `static { … }` joins them for the same reason (#1184): it
            // opens a `FuncSpace`, so without this arm an initialiser
            // written inside a nested class inherited the enclosing
            // nesting.
            MethodDeclaration
            | ConstructorDeclaration
            | CompactConstructorDeclaration
            | StaticInitializer => {
                enter_function_boundary(
                    &mut nesting,
                    node,
                    ancestors,
                    &[
                        MethodDeclaration,
                        ConstructorDeclaration,
                        CompactConstructorDeclaration,
                        StaticInitializer,
                    ],
                );
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
