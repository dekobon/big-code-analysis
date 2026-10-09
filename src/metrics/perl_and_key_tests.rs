//! An auto-quoted `and` key scores like any other key, and the
//! low-precedence `and` operator still scores as one (#1539).
//!
//! Perl quotes a bareword before `=>` and alone in a hash subscript, so
//! `$h{and}` and `(and => 1)` hold the string `"and"` and no operator.
//! tree-sitter-perl 1.1.2 lexes that bareword as the keyword anyway,
//! and every metric that keys on the token scored a decision that is
//! not there. `lang_helpers::perl::perl_and_is_operator` tells the two
//! apart; these tests pin the metrics, not the error tree the grammar
//! builds around the key (grammar-dispatch §6).
//!
//! Every row pairs its fixture with a twin that spells the same program
//! without the `and` token — an `or` key, which the grammar reads as a
//! plain identifier, or a `&&` operator — and the twin is the oracle.
//! The headline numbers are asserted outright as well, so a row cannot
//! pass by both spellings drifting together.

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
