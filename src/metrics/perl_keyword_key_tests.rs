//! An auto-quoted `and` or `not` key scores like any other key, and the
//! `and` and `not` operators still score as operators (#1539, #1541).
//! The same holds for a `-bareword` key and the file tests the grammar
//! mistakes it for (#1545), and for every other bareword key, which the
//! grammar reads as a call (#1572).
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
    // `(or_twin, and_key, decisions)`. Three rows read the key's *value*:
    // the ABC peel steps through an `and` key to it, as it does through a
    // `not` key, so a negated value pays its condition. The last is a
    // real subtraction before the key, whose `-` still bills.
    let rows: [(&str, &str, Decisions); 12] = [
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
        ("f(or => !$a);", "f(and => !$a);", [2, 1, 0]),
        (
            "my $y = $c ? 1 : (or => !$a);",
            "my $y = $c ? 1 : (and => !$a);",
            [3, 3, 1],
        ),
        (
            "if (or => $x > 1) { return 1; } return 0;",
            "if (and => $x > 1) { return 1; } return 0;",
            [3, 1, 1],
        ),
        (
            "my @a = (1 - or => 2);",
            "my @a = (1 - and => 2);",
            [2, 0, 0],
        ),
        // The infix `-` is the byte before the first key but signs
        // neither: both keys are spelled `and`.
        (
            "my @a = ($x - or => 2, or => 3);",
            "my @a = ($x - and => 2, and => 3);",
            [2, 0, 0],
        ),
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
    let rows: [(&str, &str, Decisions, Halstead); 13] = [
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
        // A real subtraction before the key: the `-` is infix, no sign,
        // and still bills.
        (
            "my @a = ($x - or => 2);",
            "my @a = ($x - not => 2);",
            [2, 0, 0],
            [9, 9, 5, 5],
        ),
        // A comment ending in `-` before the key signs nothing.
        (
            "my %h = (\n  # -----\n  or => 1,\n  or => 2,\n);",
            "my %h = (\n  # -----\n  not => 1,\n  not => 2,\n);",
            [2, 0, 0],
            [8, 10, 5, 6],
        ),
        // A `not` key after an earlier keyword key parses as `not` applied
        // to a zero-width bareword the recovery invents.
        (
            "f(and => 1, or => 2);",
            "f(and => 1, not => 2);",
            [2, 0, 0],
            [6, 7, 5, 6],
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
const DASH_KEY_ROWS: [(&str, &str, [u64; 4], Halstead); 20] = [
    (
        "my %h; return $h{'-foo'};",
        "my %h; return $h{-foo};",
        [2, 0, 0, 0],
        [6, 8, 4, 4],
    ),
    // A comment in the subscript bills nothing, so this scores as the
    // row above. Its `#` billed an operator until #1549 (7, 9).
    (
        "my %h; return $h{'-foo' # why\n};",
        "my %h; return $h{-foo # why\n};",
        [2, 0, 0, 0],
        [6, 8, 4, 4],
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
    // The `ext` key scored a branch for "calling" it until #1572.
    (
        "f('-text' => 1, 'ext' => 2);",
        "f(-text => 1, ext => 2);",
        [2, 1, 0, 0],
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
    // The ABC peel steps through the sign to the key and on to its
    // value, so a negated value pays its condition, as its twin's does.
    (
        "f('-and' => !$a);",
        "f(-and => !$a);",
        [2, 1, 1, 0],
        [7, 7, 3, 4],
    ),
    (
        "f('-not' => !$a);",
        "f(-not => !$a);",
        [2, 1, 1, 0],
        [7, 7, 3, 4],
    ),
];

/// Each `-bareword` key below scores exactly as its quoted twin does, in
/// all four metrics (#1545). tree-sitter-perl reads `-foo` as the file
/// test `-f` on a bareword `oo`, `-x => 1` as the file test `-x`
/// swallowing the `=>`, and `-not` as a `-` applied to the keyword.
/// Before the fix each `-foo` billed the operand `oo` and an ABC branch
/// for calling it, and each `-not` / `-and` billed a `-` operator. A key
/// whose letter is no file-test letter (`-name`) parses as a `-` applied
/// to a bareword instead, and is pinned by
/// `perl_bareword_key_scores_like_its_quoted_twin`.
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

/// A `-and` / `-not` key keeps its sign when whitespace separates the
/// two: Perl keys `(- not => 1)` as `"-not"`, so it must not share a
/// vocabulary entry with the plain `not` key beside it. The twins'
/// values are `DASH_KEY_ROWS`' for the glued spelling. A `- name` key
/// keeps its sign too (#1572); it is spelled as written, space and all,
/// which the counts cannot see, since only one signed key is present.
#[cfg(feature = "perl")]
#[test]
fn perl_spaced_dash_key_keeps_its_sign() {
    for (twin, key) in [
        ("f('-not' => 1, not => 2);", "f(- not => 1, not => 2);"),
        ("f('-and' => 1, and => 2);", "f(- and => 1, and => 2);"),
        // A comment between the sign and the key leaves it signed.
        (
            "f('-not' => 1, not => 2);",
            "f(- # c\n not => 1, not => 2);",
        ),
        (
            "f('-name' => 1, 'name' => 2);",
            "f(- name => 1, name => 2);",
        ),
        // The slice from the sign carries the comment, so the key is
        // still one operand apart from `name`.
        (
            "f('-name' => 1, 'name' => 2);",
            "f(- # c\n name => 1, name => 2);",
        ),
    ] {
        let want = measure_with_branches(twin);
        assert_eq!(
            want,
            ([2, 1, 0, 0], [6, 7, 5, 6]),
            "`{twin}` moved; re-derive the row"
        );
        assert_eq!(
            measure_with_branches(key),
            want,
            "`{key}` must score like `{twin}`"
        );
    }
}

/// Perl quotes a file-test `-X` before `=>` only across spaces and tabs.
/// Across a newline, or a comment, which runs to one, `-f` stays the
/// file test on `$_`: executed, `(-f # c⏎ => 7)` and `(-f⏎ => 7)` both
/// yield `(undef, 7)`, where `(-f \t => 7)` yields `("-f", 7)`. The
/// grammar recovers the same `ERROR` around the `=>` in every spelling,
/// and #1545 billed each as the key `-f`. Each row's twin is the program
/// Perl runs: the comma list for a file test, the quoted key otherwise.
/// A `-foo` key is quoted either way — `-foo` is the string `"-foo"`
/// whether or not the `=>` quotes it — so its commented and
/// line-broken spellings keep their key reading.
#[cfg(feature = "perl")]
#[test]
fn perl_file_test_key_stops_at_a_line_break() {
    let comma = ([2, 0, 0, 0], [8, 9, 3, 4]);
    let quoted = ([2, 0, 0, 0], [8, 9, 4, 5]);
    let word = ([2, 1, 0, 0], [5, 5, 3, 4]);
    for (twin, want, key) in [
        (
            "my @p = (-f, 7); return @p;",
            comma,
            "my @p = (-f # c\n=> 7); return @p;",
        ),
        (
            "my @p = (-f, 7); return @p;",
            comma,
            "my @p = (-f\n=> 7); return @p;",
        ),
        (
            "my @p = ('-f' => 7); return @p;",
            quoted,
            "my @p = (-f \t => 7); return @p;",
        ),
        ("f('-foo' => 1);", word, "f(-foo # c\n=> 1);"),
        ("f('-foo' => 1);", word, "f(-foo\n=> 1);"),
    ] {
        assert_eq!(
            measure_with_branches(twin),
            want,
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

/// `(-foo, => 1)` pins the grammar's misparse, not Perl's reading.
///
/// Perl reads `-foo` here as the string `-foo`: `perl -e 'my @c =
/// (-foo, => 1); print "@c"'` prints `-foo 1`. tree-sitter-perl instead
/// parses `-foo` as a `file_handle_operator` applied to the bareword
/// `oo`, and the stray `,` ends the key position, so the `4, 4`
/// operands below come from billing `oo`. `main` read the key `-foo`,
/// because its `=>` lookup skipped the `ERROR` the stray `,` recovers
/// into; since #1572 the shape reads as the grammar does, as
/// `(name, => 1)` does (see `is_key_position`). The value is pinned so
/// a change to it is seen, not endorsed as correct.
// FIXME(#1571): bill `-foo` as the one string operand Perl reads.
#[cfg(feature = "perl")]
#[test]
fn perl_dash_key_before_stray_comma_pins_the_grammar_misparse_1571() {
    assert_eq!(
        measure_with_branches("my @c = (-foo, => 1);"),
        ([2, 1, 0, 0], [8, 8, 4, 4]),
        "the grammar's `oo` reading moved; re-derive against #1571"
    );
}

/// `(quoted_twin, bareword_key, [cyclomatic, branches, conditions,
/// cognitive], halstead)` rows for
/// `perl_bareword_key_scores_like_its_quoted_twin`. Every value is the
/// twin's, measured on `main` before #1572.
#[cfg(feature = "perl")]
const BAREWORD_KEY_ROWS: [(&str, &str, [u64; 4], Halstead); 23] = [
    (
        "my %h = ('name' => 1); return 1;",
        "my %h = (name => 1); return 1;",
        [2, 0, 0, 0],
        [8, 9, 4, 5],
    ),
    (
        "my %h = ('-name' => 1); return 1;",
        "my %h = (-name => 1); return 1;",
        [2, 0, 0, 0],
        [8, 9, 4, 5],
    ),
    (
        "my %h; return $h{'-name'};",
        "my %h; return $h{-name};",
        [2, 0, 0, 0],
        [6, 8, 4, 4],
    ),
    (
        "my %h; return $h{ '-name' };",
        "my %h; return $h{ -name };",
        [2, 0, 0, 0],
        [6, 8, 4, 4],
    ),
    (
        "my $r; return $r->{'-name'};",
        "my $r; return $r->{-name};",
        [2, 0, 0, 0],
        [7, 10, 3, 4],
    ),
    (
        "my %h; return @h{'-a'};",
        "my %h; return @h{-a};",
        [2, 0, 0, 0],
        [5, 7, 4, 4],
    ),
    (
        "my %h; if ($h{'-name'}) { return 1; } return 0;",
        "my %h; if ($h{-name}) { return 1; } return 0;",
        [3, 0, 1, 1],
        [8, 13, 6, 6],
    ),
    (
        "my $r = { 'a' => { 'b' => 1 } }; return $r;",
        "my $r = { a => { b => 1 } }; return $r;",
        [2, 0, 0, 0],
        [8, 13, 5, 6],
    ),
    // A named-argument call scores its callee alone.
    (
        "new('name' => $x, 'size' => 3);",
        "new(name => $x, size => 3);",
        [2, 1, 0, 0],
        [7, 8, 6, 6],
    ),
    (
        "foo 'name' => 1;",
        "foo name => 1;",
        [2, 1, 0, 0],
        [4, 4, 4, 4],
    ),
    (
        "$o->m('-name' => 1);",
        "$o->m(-name => 1);",
        [2, 1, 0, 0],
        [7, 7, 5, 5],
    ),
    // Perl quotes a word across a comment or line break before `=>`.
    (
        "my %h = ('name' # c\n => 1); return 1;",
        "my %h = (name # c\n => 1); return 1;",
        [2, 0, 0, 0],
        [8, 9, 4, 5],
    ),
    // The spelling rows: a signed and a plain key are two operands, and
    // two signed keys one, as their twins are. Billed as `name`, the
    // first pair merges (n2 4) and the second still matches; billed as
    // `-` + `name`, both also bill a `-` operator.
    (
        "f('-name' => 1, 'name' => 2);",
        "f(-name => 1, name => 2);",
        [2, 1, 0, 0],
        [6, 7, 5, 6],
    ),
    (
        "f('-name' => 1, '-name' => 2);",
        "f(-name => 1, -name => 2);",
        [2, 1, 0, 0],
        [6, 7, 4, 6],
    ),
    // A builtin's name is quoted like any other word.
    (
        "my %h = ('shift' => 1, 'time' => 2); return 1;",
        "my %h = (shift => 1, time => 2); return 1;",
        [2, 0, 0, 0],
        [9, 11, 6, 7],
    ),
    (
        "my @a = ('a' => 'b' => 1); return @a;",
        "my @a = (a => b => 1); return @a;",
        [2, 0, 0, 0],
        [8, 10, 5, 6],
    ),
    // A bareword *value* is no key, and still scores its call.
    (
        "my @a = ('a' => foo); return @a;",
        "my @a = (a => foo); return @a;",
        [2, 1, 0, 0],
        [8, 9, 4, 5],
    ),
    // `=>` quotes the word right before it whatever expression the word
    // closes, which the grammar nests as that expression's last operand.
    // A subtraction's `-` stays an operator.
    (
        "my @a = (1 - 'name' => 2); return @a;",
        "my @a = (1 - name => 2); return @a;",
        [2, 0, 0, 0],
        [9, 10, 5, 6],
    ),
    (
        "my @a = ($x . 'name' => 2); return @a;",
        "my @a = ($x . name => 2); return @a;",
        [2, 0, 0, 0],
        [10, 11, 5, 6],
    ),
    (
        "my @a = (f(1), 'name' => 2); return @a;",
        "my @a = (f(1), name => 2); return @a;",
        [2, 1, 0, 0],
        [9, 11, 5, 7],
    ),
    // After a keyword or file-test key, whose recovery leaves an
    // `ERROR` in the same list.
    (
        "f(not => 1, 'name' => 2);",
        "f(not => 1, name => 2);",
        [2, 1, 0, 0],
        [6, 7, 5, 6],
    ),
    (
        "f(and => 1, '-name' => 2);",
        "f(and => 1, -name => 2);",
        [2, 1, 0, 0],
        [6, 7, 5, 6],
    ),
    (
        "f('-x' => 1, 'name' => 2);",
        "f(-x => 1, name => 2);",
        [2, 1, 0, 0],
        [6, 7, 5, 6],
    ),
];

/// Each bareword key below scores exactly as its quoted twin does, in
/// all four metrics (#1572). tree-sitter-perl reads `name` before `=>`
/// as a call of `name`, and `-name` there or alone in a subscript as a
/// `-` applied to that call. Before the fix each key scored an ABC
/// branch, and each `-name` billed a `-` operator and the unsigned
/// operand `name`. Cyclomatic and cognitive were never affected, which
/// the decision columns pin.
#[cfg(feature = "perl")]
#[test]
fn perl_bareword_key_scores_like_its_quoted_twin() {
    for (twin, key, decisions, halstead) in BAREWORD_KEY_ROWS {
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

/// A bareword that is no auto-quoted key keeps the call, and a `-` that
/// signs none keeps its operator: a word followed by anything but `=>`,
/// a package-qualified word (`=>` quotes only letters, digits and
/// underscores), a word with arguments, a call inside a subscript, a
/// `-bareword` that is only part of one, a word a stray `,` separates
/// from the `=>` (the grammar hides that `,` in an extra `ERROR`, and
/// the `=>` then quotes the `,`, which is nothing), and a unary minus on
/// a variable. Every value is `main`'s, measured before #1572.
#[cfg(feature = "perl")]
#[test]
fn perl_bareword_outside_a_key_keeps_its_call() {
    let rows: [(&str, [u64; 4], Halstead); 11] = [
        ("foo(-name);", [2, 2, 0, 0], [5, 5, 3, 3]),
        (
            "my @a = (name, => 1); return @a;",
            [2, 1, 0, 0],
            [9, 10, 4, 5],
        ),
        (
            "my @a = (-name, => 1); return @a;",
            [2, 1, 0, 0],
            [10, 11, 4, 5],
        ),
        ("return -name;", [2, 1, 0, 0], [5, 5, 2, 2]),
        ("my @a = (name, 1); return @a;", [2, 1, 0, 0], [8, 9, 4, 5]),
        (
            "my @a = (Foo::Bar => 1); return @a;",
            [2, 1, 0, 0],
            [9, 10, 5, 6],
        ),
        (
            "my @a = (name() => 1); return @a;",
            [2, 1, 0, 0],
            [8, 10, 4, 5],
        ),
        ("my %h; return $h{foo()};", [2, 1, 0, 0], [7, 9, 4, 4]),
        (
            "my %h; return $h{-name . 'x'};",
            [2, 1, 0, 0],
            [8, 10, 5, 5],
        ),
        ("my %h; return $h{1 - name};", [2, 1, 0, 0], [7, 9, 5, 5]),
        ("my $y = -$x; return $y;", [2, 0, 0, 0], [8, 11, 3, 4]),
    ];
    for (body, decisions, halstead) in rows {
        assert_eq!(
            measure_with_branches(body),
            (decisions, halstead),
            "`{body}` changed its reading"
        );
    }
}

/// A file whose tail fails to parse leaves every key, chain and sign in
/// it a direct child of one flat `ERROR` root, and the key and misparse
/// lookups ask for a sibling in that `ERROR` once per child. Both walks
/// lend the `SiblingCache` that answers them from the children they
/// recorded rather than by a scan from the `ERROR`'s start (#1584); a
/// walk that stopped recording trips the debug assertion in the lookup
/// here, since the answers themselves would not change. Every value is
/// `main`'s, measured before #1584: twelve `last` calls, the `name`
/// keys none, and `-a` / `-and` billed whole. `find` and `count` walk
/// with `act_on_node`, which lends the same cache.
#[cfg(feature = "perl")]
#[test]
fn perl_flat_error_reads_siblings_from_the_walk() {
    // The width from which a walk records a node's children.
    use big_code_analysis_ast::node::WIDE_PARENT;
    let body = "{\nlast,\n$a && $b,\n- and,\n-a => 1,\nname => 1,\n$c > $d && $e,\n";
    let source = format!("my @r = ({}", body.repeat(12));
    let ast = Ast::parse(Source::new(LANG::Perl, source.as_bytes())).expect("perl is enabled");
    let root = ast.as_tree_sitter().root_node();
    assert!(
        root.is_error() && root.child_count() > WIDE_PARENT,
        "the fixture must parse to a wide flat ERROR root"
    );

    let space = ast
        .metrics(MetricsOptions::default())
        .expect("metrics yields a root space");
    let (abc, halstead) = (&space.metrics.abc, &space.metrics.halstead);
    assert_eq!(
        (
            abc.branches_sum(),
            abc.conditions_sum(),
            abc.assignments_sum()
        ),
        (12, 48, 1)
    );
    assert_eq!(
        [
            halstead.unique_operators(),
            halstead.total_operators(),
            halstead.unique_operands(),
            halstead.total_operands(),
        ],
        [9, 207, 11, 133]
    );

    let ops = ast.ops().expect("ops yields a root space");
    assert_eq!(
        ops.operands,
        [
            "$a", "$b", "$c", "$d", "$e", "-a", "-and", "1", "@r", "last", "name"
        ]
    );
    assert_eq!(
        ops.operators,
        ["$", "&&", "()", ",", "=", "=>", ">", "my", "{}"]
    );

    // The `call` filter of `find` and `count` asks the same key test of
    // every bareword, through the walk `act_on_node` keeps (#1580): the
    // twelve `last` calls, as ABC counts them, and no `name` key.
    assert_eq!(ast.count(&["call".to_owned()]).0, 12);
}

/// What `bca find --type call` reports on `source`: each node's kind and
/// text, in source order, and the root space's ABC branches. `is_call`
/// and ABC's branch arm share `perl_bareword_is_call`, so the two must
/// count the same calls (grammar-dispatch §7) — Perl has no other kind
/// of ABC branch.
#[cfg(feature = "perl")]
fn perl_calls(source: &str) -> (Vec<(String, String)>, usize) {
    let ast = Ast::parse(Source::new(LANG::Perl, source.as_bytes())).expect("perl is enabled");
    let found = ast
        .find(&["call".to_owned()])
        .expect("find is infallible")
        .iter()
        .map(|node| {
            (
                node.kind().to_owned(),
                source[node.start_byte()..node.end_byte()].to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ast.count(&["call".to_owned()]).0,
        found.len(),
        "`count` and `find` disagree on `{source}`"
    );
    let branches = ast
        .metrics(MetricsOptions::default())
        .expect("metrics yields a root space")
        .metrics
        .abc
        .branches_sum();
    let branches = usize::try_from(branches).expect("a fixture's branch count fits a usize");
    (found, branches)
}

/// #1580's fixture: three auto-quoted keys the grammar parses as
/// bareword calls, and one call whose name it parses as a second. Before
/// the fix `find` reported all five; `foo(1)` is the only call.
#[cfg(feature = "perl")]
#[test]
fn perl_find_call_reports_one_node_per_call_1580() {
    let source = "sub d { my %h = (name => 1, -size => 2); my $y = $h{-name}; return foo(1); }\n";
    let (found, branches) = perl_calls(source);
    assert_eq!(
        found,
        [(
            "call_expression_with_args_with_brackets".to_owned(),
            "foo(1)".to_owned()
        )]
    );
    assert_eq!(branches, 1);
}

/// Each call spelling is one call, reported on the node present for
/// that spelling: the wrapper where the grammar builds one around the
/// name, else the bareword. `foo(1` is the error-recovery shape with no
/// wrapper at all, so its bareword is the call; dropping bareword calls
/// instead of gating them would lose it, and `foo;` and `&foo;` with it
/// (grammar-dispatch §6).
#[cfg(feature = "perl")]
#[test]
fn perl_each_call_spelling_is_one_call_1580() {
    const BRACKETS: &str = "call_expression_with_args_with_brackets";
    const SPACED: &str = "call_expression_with_spaced_args";
    const BAREWORD: &str = "call_expression_with_bareword";
    const METHOD: &str = "method_invocation";
    let rows: [(&str, &[(&str, &str)]); 14] = [
        ("foo(1);", &[(BRACKETS, "foo(1)")]),
        ("foo();", &[(BRACKETS, "foo()")]),
        ("&foo(1);", &[(BRACKETS, "&foo(1)")]),
        ("Foo::bar(1);", &[(BRACKETS, "Foo::bar(1)")]),
        ("foo 1;", &[(SPACED, "foo 1")]),
        ("print \"x\";", &[(SPACED, "print \"x\"")]),
        (
            "sort { $a <=> $b } @x;",
            &[(SPACED, "sort { $a <=> $b } @x")],
        ),
        ("$obj->m(1);", &[(METHOD, "$obj->m(1)")]),
        (
            "&$code(1);",
            &[("call_expression_with_variable", "&$code(1)")],
        ),
        (
            "__SUB__->(1);",
            &[("call_expression_recursive", "__SUB__->(1)")],
        ),
        ("foo;", &[(BAREWORD, "foo")]),
        ("&foo;", &[(BAREWORD, "&foo")]),
        // A bareword argument is a call of its own, not the callee.
        ("foo(bar);", &[(BRACKETS, "foo(bar)"), (BAREWORD, "bar")]),
        ("foo(1", &[(BAREWORD, "foo")]),
    ];
    for (source, expected) in rows {
        let (found, branches) = perl_calls(&format!("{source}\n"));
        let expected = expected
            .iter()
            .map(|&(kind, text)| (kind.to_owned(), text.to_owned()))
            .collect::<Vec<_>>();
        assert_eq!(found, expected, "`{source}`");
        assert_eq!(branches, found.len(), "`{source}`: ABC disagrees");
    }
}

/// An auto-quoted key is a string, so it reports the calls its quoted
/// twin does — none of its own. A word Perl does not quote keeps its
/// call: a package-qualified word, one with arguments, and a value.
/// Every count is checked against ABC's branches as well.
#[cfg(feature = "perl")]
#[test]
fn perl_bareword_key_is_no_call_1580() {
    let rows = [
        ("my %h = (name => 1);", "my %h = ('name' => 1);", 0),
        ("my %h = (-name => 1);", "my %h = ('-name' => 1);", 0),
        ("my %h = (-size => 1);", "my %h = ('-size' => 1);", 0),
        ("my $y = $h{-name};", "my $y = $h{'-name'};", 0),
        ("my $y = $h{-size};", "my $y = $h{'-size'};", 0),
        ("my %h = (1 - name => 2);", "my %h = (1 - 'name' => 2);", 0),
        ("new(name => $x);", "new('name' => $x);", 1),
        ("my %h = (Foo::Bar => 1);", "my %h = (Foo::Bar() => 1);", 1),
        (
            "my %h = (-foo::bar => 1);",
            "my %h = (-foo::bar() => 1);",
            1,
        ),
        ("my %h = (name() => 1);", "my %h = (name(1) => 1);", 1),
        ("my %h = (k => name);", "my %h = ('k' => name());", 1),
    ];
    for (key, twin, calls) in rows {
        let (found, branches) = perl_calls(&format!("{key}\n"));
        let (twin_found, _) = perl_calls(&format!("{twin}\n"));
        assert_eq!(found.len(), calls, "`{key}` reported {found:?}");
        assert_eq!(found.len(), twin_found.len(), "`{key}` against `{twin}`");
        assert_eq!(branches, calls, "`{key}`: ABC disagrees");
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_negated_qualified_calls_keep_their_operand_names() {
    let source = "my %h = (-foo::bar => 1, -foo::baz => 2);";
    let ast = Ast::parse(Source::new(LANG::Perl, source.as_bytes())).expect("perl is enabled");
    assert_eq!(ast.count(&["call".to_owned()]).0, 2);
    let metrics = ast
        .metrics(MetricsOptions::default())
        .expect("metrics has a root");
    assert_eq!(metrics.metrics.abc.branches_sum(), 2);
    assert_eq!(metrics.metrics.halstead.unique_operands(), 6);
    assert_eq!(
        ast.ops().expect("ops has a root").operands,
        ["%h", "1", "2", "bar", "baz", "oo"]
    );
}
