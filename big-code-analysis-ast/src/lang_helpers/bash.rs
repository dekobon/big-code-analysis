//! Bash: the logical connectives and the string comparison of a
//! `[ … ]` / `[[ … ]]` test.

use crate::Bash;
use crate::node::Node;

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

/// How many `=` string comparisons `test`, a `[ … ]` / `[[ … ]]`
/// `test_command`, holds.
///
/// Both spellings of `=` are a `binary_expression`: `[ "$a" = 1 ]`
/// compares and `(( x = 1 ))` assigns. So an `=` compares exactly when
/// the test reaches it through expression nodes alone. The descent
/// passes through `binary_expression` because tree-sitter-bash nests `=`
/// wrongly — `[ "$a" = 1 -a "$b" = 2 ]` parses as
/// `"$a" = (1 -a "$b") = 2` — and through the unary and parenthesized
/// forms (`[[ ! ( "$a" = 1 ) ]]`). It stops at anything else, so the
/// assignment in `[[ $(( x = 1 )) -eq 1 ]]` is not counted.
///
/// Counted from the test down rather than by climbing from each `=` to
/// its test: on the mis-nested spine a climb per `=` is `O(depth)`, and a
/// long `[ … = … -o … ]` made the walk quadratic. The descent is an
/// explicit stack, since that spine is as deep as the test is long.
#[must_use]
pub fn bash_test_eq_count(test: &Node) -> usize {
    let mut count = 0;
    let mut pending = vec![*test];
    while let Some(expression) = pending.pop() {
        for child in expression.children() {
            if child.kind_id() == Bash::EQ as u16 {
                count += 1;
            } else if matches!(
                child.kind(),
                "binary_expression" | "unary_expression" | "parenthesized_expression"
            ) {
                pending.push(child);
            }
        }
    }
    count
}
