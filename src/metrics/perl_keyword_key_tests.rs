//! An auto-quoted `and` or `not` key scores like any other key, and the
//! `and` and `not` operators still score as operators (#1539, #1541).
//! The same holds for a `-bareword` key and the file tests the grammar
//! mistakes it for (#1545).
//!
//! Perl quotes a bareword before `=>` and alone in a hash subscript, so
//! `$h{and}` and `(not => 1)` hold a string key and no operator.
//! tree-sitter-perl 1.1.2 lexes either bareword as the keyword anyway,
//! and every metric that keys on the token scored an operator, or a
//! decision, that is not there. `lang_helpers::perl::perl_and_is_operator`
//! and `perl_not_is_key` tell the two apart; these tests pin the metrics,
//! not the error tree the grammar builds around the key
//! (grammar-dispatch §6).
//!
//! Every row pairs its fixture with a twin that spells the same program
//! without the keyword token — an `or` key, which the grammar reads as a
//! plain identifier, or a `&&` / `!` operator — and the twin is the
//! oracle. The headline numbers are asserted outright as well, so a row
//! cannot pass by both spellings drifting together.

use std::cell::Cell;

use crate::test_support::check_func_space_only;
use crate::*;

/// `[cyclomatic, abc conditions, cognitive]` sums of the root space.
/// The cyclomatic sum carries the unit's and the sub's base of 1 each.
#[cfg(feature = "perl")]
type Decisions = [u64; 3];

/// `[n1, N1, n2, N2]`.
#[cfg(feature = "perl")]
type Halstead = [u64; 4];

#[cfg(feature = "perl")]
fn measure(body: &str) -> (Decisions, Halstead) {
    let ([cyclomatic, _, conditions, cognitive], halstead) = measure_with_branches(body);
    ([cyclomatic, conditions, cognitive], halstead)
}

/// `[cyclomatic, abc branches, abc conditions, cognitive]` sums of the
/// root space, and its Halstead counts.
#[cfg(feature = "perl")]
fn measure_with_branches(body: &str) -> ([u64; 4], Halstead) {
    let out = Cell::new(([0; 4], [0; 4]));
    check_func_space_only::<PerlParser, _>(
        &format!("sub f {{ {body} }}\n"),
        "foo.pl",
        &[
            Metric::Cyclomatic,
            Metric::Abc,
            Metric::Cognitive,
            Metric::Halstead,
        ],
        |space| {
            let m = &space.metrics;
            out.set((
                [
                    m.cyclomatic.cyclomatic_sum(),
                    m.abc.branches_sum(),
                    m.abc.conditions_sum(),
                    m.cognitive.cognitive_sum(),
                ],
                [
                    m.halstead.unique_operators(),
                    m.halstead.total_operators(),
                    m.halstead.unique_operands(),
                    m.halstead.total_operands(),
                ],
            ));
        },
    );
    out.get()
}

/// Every valid spelling of an `and` key scores exactly as an `or` key
/// does, in all four metrics. Before #1539 each subscript scored one
/// extra cyclomatic decision and billed `and` as an operator, and each
/// `=>` key also scored an ABC condition and a cognitive sequence.
#[cfg(feature = "perl")]
#[test]
fn perl_and_key_scores_like_an_or_key() {
    // `(or_twin, and_key, decisions)`.
    let rows: [(&str, &str, Decisions); 7] = [
        ("my %h; return $h{or};", "my %h; return $h{and};", [2, 0, 0]),
        (
            "my %h; return $h{ or };",
            "my %h; return $h{ and };",
            [2, 0, 0],
        ),
        (
            "my $r; return $r->{or};",
            "my $r; return $r->{and};",
            [2, 0, 0],
        ),
        (
            "my %h; if ($h{or}) { return 1; } return 0;",
            "my %h; if ($h{and}) { return 1; } return 0;",
            [3, 1, 1],
        ),
        (
            "my %h = (or => 1); return 1;",
            "my %h = (and => 1); return 1;",
            [2, 0, 0],
        ),
        (
            "my $h = { or => 1 }; return $h;",
            "my $h = { and => 1 }; return $h;",
            [2, 0, 0],
        ),
        ("f(x => 1, or => 2);", "f(x => 1, and => 2);", [2, 0, 0]),
    ];
    for (twin, key, decisions) in rows {
        assert_eq!(
            key.replace("and", "or"),
            twin,
            "`{key}` drifted from its twin"
        );
        let want = measure(twin);
        assert_eq!(want.0, decisions, "`{twin}` moved; re-derive the row");
        assert_eq!(measure(key), want, "`{key}` must score like `{twin}`");
    }
}

/// A list holding an `and` key, assigned in a condition, is one
/// truthiness test: the slot evaluates to the list's last element, so
/// it pays one condition like any other value.
///
/// This row guards cyclomatic, cognitive and Halstead, but not ABC. The
/// ABC count comes out at 1 whether or not the `and` gate in
/// `perl_is_logical_chain` holds. Without the gate, the slot takes the
/// key for a chain and scores 0, and the chain walk then pays the 1 the
/// slot dropped. `perl_and_key_scores_like_an_or_key` guards that gate.
#[cfg(feature = "perl")]
#[test]
fn perl_and_key_in_a_condition_slot_pays_the_slot() {
    let twin = "my %h; if (%h = (or => 1)) { return 1; } return 0;";
    let key = "my %h; if (%h = (and => 1)) { return 1; } return 0;";
    let want = measure(twin);
    assert_eq!(want.0, [3, 1, 1], "`{twin}` moved; re-derive the row");
    assert_eq!(measure(key), want, "`{key}` must score like `{twin}`");
}

/// A real low-precedence `and` keeps scoring as `&&` does — alone,
/// chained, after `not`, and inside the comparison and ternary shapes
/// the #1273 workaround climbs. Halstead is left out: `and` and `&&`
/// are distinct operators, and the ternary twin needs parentheses.
#[cfg(feature = "perl")]
#[test]
fn perl_and_operator_still_scores_like_ampamp() {
    // `(ampamp_twin, and_operator, decisions)`.
    let rows: [(&str, &str, Decisions); 5] = [
        ("return ($a && $b);", "return ($a and $b);", [3, 2, 1]),
        (
            "return ($a && $b && $c);",
            "return ($a and $b and $c);",
            [4, 3, 1],
        ),
        (
            "if (!$a && $b) { return 1; } return 0;",
            "if (not $a and $b) { return 1; } return 0;",
            [4, 2, 2],
        ),
        (
            "return ($a > $b && $c);",
            "return ($a > $b and $c);",
            [3, 2, 1],
        ),
        (
            "return (($a > $b) && ($c ? 1 : 2));",
            "return ($a > $b and $c ? 1 : 2);",
            [4, 3, 2],
        ),
    ];
    for (twin, operator, decisions) in rows {
        assert!(operator.contains(" and "), "`{operator}` lost its `and`");
        let want = measure(twin).0;
        assert_eq!(want, decisions, "`{twin}` moved; re-derive the row");
        assert_eq!(
            measure(operator).0,
            want,
            "`{operator}` must score like `{twin}`"
        );
    }
}

/// Every valid spelling of a `not` key scores exactly as an `or` key
/// does. Before #1541 each billed `not` as an operator where the twin
/// bills an operand, and a `=>` key outside a boolean slot also scored
/// an ABC condition, the peel reading it as `!` applied to its value.
/// The peel must still reach that value, which a negation there proves
/// boolean like any other.
#[cfg(feature = "perl")]
#[test]
fn perl_not_key_scores_like_an_or_key() {
    // `(or_twin, not_key, decisions, halstead)`.
    let rows: [(&str, &str, Decisions, Halstead); 10] = [
        (
            "my %h; return $h{or};",
            "my %h; return $h{not};",
            [2, 0, 0],
            [6, 8, 4, 4],
        ),
        (
            "my %h; return $h{ or };",
            "my %h; return $h{ not };",
            [2, 0, 0],
            [6, 8, 4, 4],
        ),
        (
            "my $r; return $r->{or};",
            "my $r; return $r->{not};",
            [2, 0, 0],
            [7, 10, 3, 4],
        ),
        (
            "my %h = (or => 1); return 1;",
            "my %h = (not => 1); return 1;",
            [2, 0, 0],
            [8, 9, 4, 5],
        ),
        (
            "my $r = { or => 1 }; return 1;",
            "my $r = { not => 1 }; return 1;",
            [2, 0, 0],
            [8, 10, 4, 5],
        ),
        (
            "f(x => 1, or => 2);",
            "f(x => 1, not => 2);",
            [2, 0, 0],
            [6, 7, 5, 6],
        ),
        ("f(or => 1);", "f(not => 1);", [2, 0, 0], [5, 5, 3, 4]),
        (
            "return (or => 1);",
            "return (not => 1);",
            [2, 0, 0],
            [6, 6, 3, 3],
        ),
        // The key's value is still a slot: the `!` proves it boolean.
        ("f(or => !$a);", "f(not => !$a);", [2, 1, 0], [7, 7, 3, 4]),
        (
            "my $y = $c ? 1 : (or => 2);",
            "my $y = $c ? 1 : (not => 2);",
            [3, 2, 1],
            [10, 11, 6, 6],
        ),
    ];
    for (twin, key, decisions, halstead) in rows {
        assert_eq!(
            key.replace("not", "or"),
            twin,
            "`{key}` drifted from its twin"
        );
        let want = measure(twin);
        assert_eq!(
            want,
            (decisions, halstead),
            "`{twin}` moved; re-derive the row"
        );
        assert_eq!(measure(key), want, "`{key}` must score like `{twin}`");
    }
}

/// A real `not` keeps scoring as `!` does, in all four metrics: alone,
/// in a condition, applied to a list, as a call argument the negation
/// proves boolean, ahead of a low-precedence `and`, and as the value of
/// a `not` key, where the key's recovery node sits one level up. `not`
/// and `!` are distinct operators, but each fixture spells only one of
/// them, so the Halstead counts agree too.
#[cfg(feature = "perl")]
#[test]
fn perl_not_operator_still_scores_like_bang() {
    // `(bang_twin, not_operator, decisions, halstead)`.
    let rows: [(&str, &str, Decisions, Halstead); 6] = [
        ("!$a;", "not $a;", [2, 0, 0], [5, 5, 2, 2]),
        (
            "if (!$a) { return 1; } return 0;",
            "if (not $a) { return 1; } return 0;",
            [3, 1, 1],
            [8, 11, 4, 4],
        ),
        ("!($a);", "not($a);", [2, 0, 0], [6, 6, 2, 2]),
        ("f(!$a);", "f(not $a);", [2, 1, 0], [6, 6, 2, 3]),
        (
            "return (!$a && $b);",
            "return (not $a and $b);",
            [3, 2, 1],
            [8, 9, 3, 3],
        ),
        (
            "my %h = (or => !$a); return 1;",
            "my %h = (not => not $a); return 1;",
            [2, 0, 0],
            [10, 11, 5, 5],
        ),
    ];
    for (twin, operator, decisions, halstead) in rows {
        assert!(operator.contains("not"), "`{operator}` lost its `not`");
        let want = measure(twin);
        assert_eq!(
            want,
            (decisions, halstead),
            "`{twin}` moved; re-derive the row"
        );
        assert_eq!(
            measure(operator),
            want,
            "`{operator}` must score like `{twin}`"
        );
    }
}

/// `(quoted_twin, dash_key, [cyclomatic, branches, conditions,
/// cognitive], halstead)` rows for
/// `perl_dash_key_scores_like_its_quoted_twin`.
#[cfg(feature = "perl")]
const DASH_KEY_ROWS: [(&str, &str, [u64; 4], Halstead); 18] = [
    (
        "my %h; return $h{'-foo'};",
        "my %h; return $h{-foo};",
        [2, 0, 0, 0],
        [6, 8, 4, 4],
    ),
    // A comment in the subscript. Its `#` bills as an operator in both
    // spellings (#1549).
    (
        "my %h; return $h{'-foo' # why\n};",
        "my %h; return $h{-foo # why\n};",
        [2, 0, 0, 0],
        [7, 9, 4, 4],
    ),
    (
        "my $r; return $r->{'-foo'};",
        "my $r; return $r->{-foo};",
        [2, 0, 0, 0],
        [7, 10, 3, 4],
    ),
    (
        "my %h; return @h{'-foo'};",
        "my %h; return @h{-foo};",
        [2, 0, 0, 0],
        [5, 7, 4, 4],
    ),
    (
        "my %h; if ($h{'-foo'}) { return 1; } return 0;",
        "my %h; if ($h{-foo}) { return 1; } return 0;",
        [3, 0, 1, 1],
        [8, 13, 6, 6],
    ),
    (
        "my %h = ('-foo' => 1); return 1;",
        "my %h = (-foo => 1); return 1;",
        [2, 0, 0, 0],
        [8, 9, 4, 5],
    ),
    (
        "my $r = { '-foo' => 1 }; return $r;",
        "my $r = { -foo => 1 }; return $r;",
        [2, 0, 0, 0],
        [8, 11, 4, 5],
    ),
    (
        "f('-text' => 1);",
        "f(-text => 1);",
        [2, 1, 0, 0],
        [5, 5, 3, 4],
    ),
    (
        "f('-text' => 1, ext => 2);",
        "f(-text => 1, ext => 2);",
        [2, 2, 0, 0],
        [6, 7, 5, 6],
    ),
    (
        "my %h = ('-x' => 1); return 1;",
        "my %h = (-x => 1); return 1;",
        [2, 0, 0, 0],
        [8, 9, 4, 5],
    ),
    (
        "my %h = ('-x' => 1); my %g = ('-x' => 2);",
        "my %h = (-x => 1); my %g = (-x => 2);",
        [2, 0, 0, 0],
        [7, 12, 6, 7],
    ),
    // The swallowed value is still billed, call and all.
    (
        "f('-x' => foo);",
        "f(-x => foo);",
        [2, 2, 0, 0],
        [5, 5, 3, 4],
    ),
    (
        "my %h; return $h{'-not'};",
        "my %h; return $h{-not};",
        [2, 0, 0, 0],
        [6, 8, 4, 4],
    ),
    (
        "my $r; return $r->{'-and'};",
        "my $r; return $r->{-and};",
        [2, 0, 0, 0],
        [7, 10, 3, 4],
    ),
    (
        "my %h = ('-and' => 1); return 1;",
        "my %h = (-and => 1); return 1;",
        [2, 0, 0, 0],
        [8, 9, 4, 5],
    ),
    (
        "my $r = { '-not' => 1 }; return $r;",
        "my $r = { -not => 1 }; return $r;",
        [2, 0, 0, 0],
        [8, 11, 4, 5],
    ),
    (
        "f('-not' => 1, not => 2);",
        "f(-not => 1, not => 2);",
        [2, 1, 0, 0],
        [6, 7, 5, 6],
    ),
    (
        "f('-and' => 1, and => 2);",
        "f(-and => 1, and => 2);",
        [2, 1, 0, 0],
        [6, 7, 5, 6],
    ),
];

/// Every valid spelling of a `-bareword` key scores exactly as its
/// quoted twin does, in all four metrics (#1545). tree-sitter-perl
/// reads `-foo` as the file test `-f` on a bareword `oo`, `-x => 1` as
/// the file test `-x` swallowing the `=>`, and `-not` as a `-` applied
/// to the keyword. Before the fix each `-foo` billed the operand `oo`
/// and an ABC branch for calling it, and each `-not` / `-and` billed a
/// `-` operator.
///
/// The rows pairing a key with a second key equal to the misparsed
/// word (`ext`, `not`) or to itself (`-x` twice) pin the operand's
/// spelling, which the counts alone cannot see: billed as `ext`, `not`
/// or `-x => 1`, the two keys share or split one vocabulary entry
/// where their twins do not.
#[cfg(feature = "perl")]
#[test]
fn perl_dash_key_scores_like_its_quoted_twin() {
    for (twin, key, decisions, halstead) in DASH_KEY_ROWS {
        assert_eq!(twin.replace('\'', ""), key, "`{key}` drifted from its twin");
        let want = measure_with_branches(twin);
        assert_eq!(
            want,
            (decisions, halstead),
            "`{twin}` moved; re-derive the row"
        );
        assert_eq!(
            measure_with_branches(key),
            want,
            "`{key}` must score like `{twin}`"
        );
    }
}

/// A `-X` that is not a key keeps the reading it had before #1545: a
/// real file test, alone, in a condition, on the `_` stat cache,
/// stacked, and as a subscript's whole expression, where only a word
/// glued to the letter makes a key; a `-bareword` outside key position,
/// including a subscript it is only part of;
/// and a `-` applied to a real `not`. The file test itself is billed as
/// nothing, since the grammar emits its `-X` as a hidden token. Every
/// value is `main`'s, measured before the fix.
#[cfg(feature = "perl")]
#[test]
fn perl_file_test_outside_a_key_keeps_its_reading() {
    let rows: [(&str, [u64; 4], Halstead); 11] = [
        ("my $z = -f $x;", [2, 0, 0, 0], [6, 7, 3, 3]),
        (
            "if (-e $x) { return 1; } return 0;",
            [3, 0, 1, 1],
            [7, 10, 4, 4],
        ),
        ("my $z = -d _;", [2, 1, 0, 0], [6, 6, 3, 3]),
        ("my $z = -f -w $x;", [2, 0, 0, 0], [6, 7, 3, 3]),
        ("my %h; return $h{-f $x};", [2, 0, 0, 0], [6, 9, 4, 4]),
        ("my %h; return $h{-f foo};", [2, 1, 0, 0], [6, 8, 4, 4]),
        ("return -foo;", [2, 1, 0, 0], [4, 4, 2, 2]),
        // Inside a subscript, but not the whole key.
        ("my %h; return $h{$a . -foo};", [2, 1, 0, 0], [7, 10, 5, 5]),
        ("f(-foo, 1);", [2, 2, 0, 0], [5, 5, 3, 4]),
        ("return -not $x;", [2, 0, 0, 0], [7, 7, 2, 2]),
        ("return $a - not $x;", [2, 0, 0, 0], [7, 8, 3, 3]),
    ];
    for (body, decisions, halstead) in rows {
        assert_eq!(
            measure_with_branches(body),
            (decisions, halstead),
            "`{body}` changed its reading"
        );
    }
}
