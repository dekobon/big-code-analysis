//! An auto-quoted `and` or `not` key scores like any other key, and the
//! `and` and `not` operators still score as operators (#1539, #1541).
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
    let out = Cell::new(([0; 3], [0; 4]));
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
