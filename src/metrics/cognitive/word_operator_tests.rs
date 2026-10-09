//! A word-spelled logical operator continues the boolean sequence of
//! its symbol in cognitive complexity (#1530).
//!
//! Ruby, PHP, Perl, Elixir and iRules spell conjunction and disjunction
//! both as `&&` / `||` and as `and` / `or`, and their grammars give the
//! two spellings distinct kinds. Keyed on the raw kind, a mixed chain
//! such as `a && b and c` scored two sequences where `a && b && c`
//! scores one. Every row pairs the mixed spelling with its unmixed twin,
//! which is the oracle: the expected value is the twin's, and the twin
//! is asserted too so the row cannot pass by both drifting together.
//!
//! The rows that switch operation (`a || b and c`) stay at two
//! sequences: precedence decides which operator nests under which, and
//! the walk sees that through the parse. `xor`, `??` and `//` are
//! operations of their own and must not join an `&&` / `||` sequence.

use std::cell::Cell;

use crate::test_support::check_func_space;
use crate::*;

/// `(twin, mixed, words, expected)`: an unmixed expression, the same
/// expression with some operators word-spelled, how many word-operator
/// tokens `mixed` must carry, and the cognitive sum both must score.
type Row = (&'static str, &'static str, usize, u64);

/// Scores every row inside `wrap`, after proving each mixed fixture
/// carries its word operators — a fixture that silently parsed `and` as
/// an identifier would otherwise pass while asserting nothing.
fn assert_rows<P: MetricSuite>(
    path: &str,
    wrap: impl Fn(&str) -> String,
    words: &[u16],
    rows: &[Row],
) {
    assert!(!rows.is_empty(), "table asserted nothing");
    for &(twin, mixed, want_words, expected) in rows {
        let src = wrap(mixed);
        let parser = P::new(src.as_bytes().to_vec(), std::path::Path::new(path), None);
        let found = parser
            .root()
            .preorder()
            .filter(|n| words.contains(&n.kind_id()))
            .count();
        assert_eq!(found, want_words, "`{mixed}` lost its word operators");
        for body in [twin, mixed] {
            let got = Cell::new(u64::MAX);
            check_func_space::<P, _>(&wrap(body), path, |space| {
                got.set(space.metrics.cognitive.cognitive_sum());
            });
            assert_eq!(got.get(), expected, "`{body}` must score like `{twin}`");
        }
    }
}

// Ruby's `and` / `or` bind looser than `&&` / `||`, so `a || b and c`
// is `(a || b) and c` — the twin is spelled with explicit parentheses.
#[cfg(feature = "ruby")]
#[test]
fn ruby_word_operators_continue_their_symbol_sequence() {
    let asg = |e: &str| format!("def f(a, b, c)\n  x = ({e})\nend\n");
    let cond = |e: &str| format!("def f(a, b, c)\n  if {e}\n    1\n  end\nend\n");
    let words = [Ruby::And as u16, Ruby::Or as u16];
    let rows: [Row; 6] = [
        ("a && b && c", "a && b and c", 1, 1),
        ("a && b && c", "a and b && c", 1, 1),
        ("a || b || c", "a || b or c", 1, 1),
        ("a || b || c", "a or b || c", 1, 1),
        ("(a || b) && c", "a || b and c", 1, 2),
        ("!a && b", "not a and b", 1, 1),
    ];
    assert_rows::<RubyParser>("foo.rb", asg, &words, &rows);
    // Inside an `if` every row pays the `if`'s +1 on top.
    let rows = rows.map(|(t, m, w, e)| (t, m, w, e + 1));
    assert_rows::<RubyParser>("foo.rb", cond, &words, &rows);
}

#[cfg(feature = "php")]
#[test]
fn php_word_operators_continue_their_symbol_sequence() {
    let asg = |e: &str| format!("<?php\nfunction f($a, $b, $c) {{\n  $x = ({e});\n}}\n");
    let words = [Php::And as u16, Php::Or as u16, Php::Xor as u16];
    let rows: [Row; 6] = [
        ("$a && $b && $c", "$a && $b and $c", 1, 1),
        ("$a && $b && $c", "$a and $b && $c", 1, 1),
        ("$a || $b || $c", "$a || $b or $c", 1, 1),
        ("($a || $b) && $c", "$a || $b and $c", 1, 2),
        // `xor` is an operation of its own: it neither joins nor is
        // joined by a conjunction.
        ("$a ?? $b && $c", "$a xor $b && $c", 1, 2),
        ("$a ?? $b ?? $c", "$a xor $b xor $c", 2, 1),
    ];
    assert_rows::<PhpParser>("foo.php", asg, &words, &rows);
}

// tree-sitter-perl parses low-precedence `and` as a `unary_expression`
// and `or` as a `binary_expression`, so the `and` rows also prove the
// `UnaryExpression` arm: without it `not $a and $b` scored 0.
#[cfg(feature = "perl")]
#[test]
fn perl_word_operators_continue_their_symbol_sequence() {
    let asg = |e: &str| format!("sub f {{\n  my $x = ({e});\n}}\n");
    let words = [Perl::And as u16, Perl::Or as u16];
    let rows: [Row; 7] = [
        ("$a && $b", "$a and $b", 1, 1),
        ("$a && $b && $c", "$a && $b and $c", 1, 1),
        ("$a && $b && $c", "$a and $b && $c", 1, 1),
        ("$a || $b || $c", "$a || $b or $c", 1, 1),
        ("($a || $b) && $c", "$a || $b and $c", 1, 2),
        ("!$a && $b", "not $a and $b", 1, 1),
        // `//` is defined-or, an operation of its own.
        ("$a // $b || $c", "$a // $b or $c", 1, 2),
    ];
    assert_rows::<PerlParser>("foo.pl", asg, &words, &rows);
}

// Elixir's `and` / `or` require a boolean left operand where `&&` / `||`
// accept any truthy value; to a reader they are the same operation.
#[cfg(feature = "elixir")]
#[test]
fn elixir_word_operators_continue_their_symbol_sequence() {
    let asg = |e: &str| format!("defmodule M do\n  def f(a, b, c) do\n    x = {e}\n  end\nend\n");
    let words = [Elixir::And as u16, Elixir::Or as u16];
    let rows: [Row; 5] = [
        ("a && b && c", "a && b and c", 1, 1),
        ("a && b && c", "a and b && c", 1, 1),
        ("a || b || c", "a || b or c", 1, 1),
        ("a || b && c", "a || b and c", 1, 2),
        ("a && b || c", "a and b or c", 2, 2),
    ];
    assert_rows::<ElixirParser>("foo.ex", asg, &words, &rows);
}

// iRules `and` / `or` are plain aliases of `&&` / `||`, as in C++.
#[cfg(feature = "irules")]
#[test]
fn irules_word_operators_continue_their_symbol_sequence() {
    let cond =
        |e: &str| format!("when HTTP_REQUEST {{\n  if {{ {e} }} {{\n    log local0. x\n  }}\n}}\n");
    let words = [Irules::And as u16, Irules::Or as u16];
    // Each row's sum includes the `if`'s +1.
    let rows: [Row; 4] = [
        ("$a && $b && $c", "$a && $b and $c", 1, 2),
        ("$a && $b && $c", "$a and $b && $c", 1, 2),
        ("$a || $b || $c", "$a || $b or $c", 1, 2),
        ("$a && $b || $c", "$a and $b or $c", 2, 3),
    ];
    assert_rows::<IrulesParser>("foo.irule", cond, &words, &rows);
}
