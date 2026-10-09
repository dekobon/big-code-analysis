//! Bash: the logical connectives and the string comparison of a
//! `[ … ]` / `[[ … ]]` test.

use crate::Bash;
use crate::node::{Ancestors, Node};

/// The `&&` / `||` a `[ … ]` connective stands for: `Some(AMPAMP)` for
/// `-a` and `Some(PIPEPIPE)` for `-o`, when `op`, a child of `parent`,
/// is one.
///
/// tree-sitter-bash gives every `-x` operator the one kind
/// `test_operator`, so the text decides. Only a binary position joins
/// two tests: the unary `-a FILE` (a file-exists test) has a
/// `unary_expression` parent and is a comparison like `-f FILE`. Every
/// expression kind is matched by name, not by `kind_id`, because the
/// grammar aliases `binary_expression` five times (grammar-dispatch §1).
///
/// The `test` builtin spelling (`test "$a" = 1 -a "$b" = 2`) is out of
/// reach: the grammar parses its arguments as plain `word`s, so neither
/// its comparisons nor its connectives exist as nodes, and it scores as
/// one command.
#[must_use]
pub fn bash_test_connective(op: &Node, parent: &Node, code: &[u8]) -> Option<Bash> {
    if op.kind_id() != Bash::TestOperator as u16 || parent.kind() != "binary_expression" {
        return None;
    }
    match code.get(op.start_byte()..op.end_byte())? {
        b"-a" => Some(Bash::AMPAMP),
        b"-o" => Some(Bash::PIPEPIPE),
        _ => None,
    }
}

/// Whether `eq`, a Bash `=` token, is the string comparison of a
/// `[ … ]` / `[[ … ]]` test rather than an assignment.
///
/// Both spellings are a `binary_expression`: `[ "$a" = 1 ]` compares
/// and `(( x = 1 ))` assigns. Climbing out of the test's expression
/// tree must therefore reach the `test_command` itself. The climb
/// passes through `binary_expression` because tree-sitter-bash nests
/// `=` wrongly — `[ "$a" = 1 -a "$b" = 2 ]` parses as
/// `"$a" = (1 -a "$b") = 2` — and through the unary and parenthesized
/// forms (`[[ ! ( "$a" = 1 ) ]]`). A `variable_assignment`'s `=` has no
/// expression parent, so the climb stops at once there.
#[must_use]
pub fn bash_eq_is_comparison<'a>(eq: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .iter(eq)
        .map(|(ancestor, _)| ancestor)
        .find(|ancestor| {
            !matches!(
                ancestor.kind(),
                "binary_expression" | "unary_expression" | "parenthesized_expression"
            )
        })
        .is_some_and(|ancestor| ancestor.kind_id() == Bash::TestCommand as u16)
}
