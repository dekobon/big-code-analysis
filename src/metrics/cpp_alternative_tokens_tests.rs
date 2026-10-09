//! The ISO C++ alternative tokens score exactly like the symbols they
//! spell, in cyclomatic, cognitive and Halstead (#1522).
//!
//! `not`, `compl`, `and`, `or`, `bitand`, `bitor`, `xor`, `not_eq`,
//! `and_eq`, `or_eq` and `xor_eq` are the same tokens as `!`, `~`, `&&`,
//! `||`, `&`, `|`, `^`, `!=`, `&=`, `|=` and `^=` ([lex.digraph]), but
//! tree-sitter-cpp and the vendored tree-sitter-mozcpp give each a kind
//! of its own. Every metric dispatch that listed only the symbol scored
//! the alternative as nothing: `b and c` was no decision, no boolean
//! sequence and no operator. ABC is covered beside its own tests
//! (`cpp_alternative_tokens_score_like_symbols` in `abc.rs`).
//!
//! One table drives every metric so the spellings cannot drift apart
//! per metric, and each row first proves its fixture carries the
//! alternative token: tree-sitter-c reads the same text as an
//! identifier, so a fixture that silently stopped parsing it as the
//! token would otherwise pass while asserting nothing.
//!
//! Mozcpp owns no file extension, so nothing else routes to it; its
//! tests here are the only coverage its clones of these arms have.

use std::cell::Cell;

use crate::test_support::{assert_fixture_spells, check_func_space};
use crate::*;

/// The kind ids of one grammar's alternative tokens, in the order of
/// [`SPELLINGS`].
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
type AltKinds = [u16; 11];

/// `(symbolic, alternative, token)`: one statement spelled both ways,
/// and the alternative token it carries, in the order [`AltKinds`]
/// lists the kinds.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
const SPELLINGS: [(&str, &str, &str); 11] = [
    ("int x = !a;", "int x = not a;", "not"),
    ("int x = ~a;", "int x = compl a;", "compl"),
    ("int x = a && b;", "int x = a and b;", "and"),
    ("int x = a || b;", "int x = a or b;", "or"),
    ("int x = a & b;", "int x = a bitand b;", "bitand"),
    ("int x = a | b;", "int x = a bitor b;", "bitor"),
    ("int x = a ^ b;", "int x = a xor b;", "xor"),
    ("int x = a != b;", "int x = a not_eq b;", "not_eq"),
    ("a &= b;", "a and_eq b;", "and_eq"),
    ("a |= b;", "a or_eq b;", "or_eq"),
    ("a ^= b;", "a xor_eq b;", "xor_eq"),
];

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn wrap(body: &str) -> String {
    format!("void f(int a, int b) {{\n  {body}\n}}\n")
}

/// `metric` read off the root space of `body` wrapped in a function.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn measure<P: MetricSuite, T: Copy + Default>(body: &str, metric: impl Fn(&FuncSpace) -> T) -> T {
    let out = Cell::new(T::default());
    check_func_space::<P, _>(&wrap(body), "foo.cpp", |space| out.set(metric(&space)));
    out.get()
}

/// `(kind, count, spelling)`: a token a fixture must carry.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
type Token = (u16, usize, &'static str);

/// Proves `body` carries each token, so a row cannot pass by parsing
/// its alternative as an identifier.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn assert_spells<P: MetricSuite>(body: &str, kinds: &[Token]) {
    assert_fixture_spells::<P>(&wrap(body), "foo.cpp", kinds);
}

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn halstead_counts(space: &FuncSpace) -> [u64; 4] {
    let h = &space.metrics.halstead;
    [
        h.unique_operators(),
        h.total_operators(),
        h.unique_operands(),
        h.total_operands(),
    ]
}

/// Every alternative bills one operator, as its symbol does. Before
/// #1522 each fell into `Unknown`, so the alternative fixture reported
/// one unique and one total operator fewer than the symbolic one.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn assert_halstead_matches_symbols<P: MetricSuite>(kinds: AltKinds) {
    for ((sym_body, alt_body, token), kind) in SPELLINGS.into_iter().zip(kinds) {
        assert_spells::<P>(alt_body, &[(kind, 1, token)]);
        let sym = measure::<P, _>(sym_body, halstead_counts);
        // `f`, `a`, `b` (and `x` where declared) are the operands; the
        // operator side always includes `void`, `()`, `{}`, `,`, `;`
        // and the operator under test, so 6 is a floor no row can meet
        // without counting it.
        assert!(sym[0] >= 6, "`{sym_body}`: implausible n1 {sym:?}");
        assert_eq!(
            measure::<P, _>(alt_body, halstead_counts),
            sym,
            "`{alt_body}` must bill [n1, N1, n2, N2] like `{sym_body}`"
        );
    }
}

/// `and` and `&&` are two distinct operators in n1 and one each in N1.
///
/// Halstead keys every non-primitive operator by `kind_id`, and `bca
/// ops` names operators by kind, so folding `and` into `&&` in the
/// metric alone would make the two stores disagree about one source.
/// That is the same rule every other distinct-kind spelling follows; a
/// change to it should be deliberate, so it is pinned here.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn assert_mixed_spellings_are_distinct_operators<P: MetricSuite>(and: u16) {
    let mixed = "int x = a && b and a;";
    assert_spells::<P>(mixed, &[(and, 1, "and")]);
    let alike = measure::<P, _>("int x = a && b && a;", halstead_counts);
    let mixed = measure::<P, _>(mixed, halstead_counts);
    assert_eq!(mixed[0], alike[0] + 1, "n1: `and` is a second operator");
    assert_eq!(mixed[1..], alike[1..], "N1, n2, N2 are unchanged");
}

/// `and` / `or` are short-circuit decisions like `&&` / `||`.
/// Expected sums count the unit and function spaces' base of 1 each.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn assert_cyclomatic_matches_symbols<P: MetricSuite>(and: u16, or: u16) {
    let rows = [
        ("bool x = a && b;", "bool x = a and b;", (and, 1, "and"), 3),
        ("if (a || b) {}", "if (a or b) {}", (or, 1, "or"), 4),
    ];
    for (symbolic, alternative, token, expected) in rows {
        assert_spells::<P>(alternative, &[token]);
        for body in [symbolic, alternative] {
            let got = measure::<P, _>(body, |s| s.metrics.cyclomatic.cyclomatic_sum());
            assert_eq!(got, expected, "`{body}` must score like `{symbolic}`");
        }
    }
}

/// `and` / `or` open and continue boolean sequences like `&&` / `||`,
/// and a chain mixing the two spellings of one operator is one
/// sequence. The mixed rows are what distinguish keying the spelling
/// to its symbol from merely matching it: listed as a kind of its own,
/// `a and b && a` scored two sequences against `a && b && a`'s one.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn assert_cognitive_matches_symbols<P: MetricSuite>(and: u16, or: u16) {
    let (and, or) = ((and, 1, "and"), (or, 1, "or"));
    let rows: [(&str, &str, &[Token], u64); 6] = [
        ("bool x = a && b;", "bool x = a and b;", &[and], 1),
        ("if (a || b) {}", "if (a or b) {}", &[or], 2),
        ("bool x = a && b && a;", "bool x = a and b && a;", &[and], 1),
        ("bool x = a && b && a;", "bool x = a && b and a;", &[and], 1),
        ("bool x = a || b || a;", "bool x = a or b || a;", &[or], 1),
        (
            "bool x = a && b || a;",
            "bool x = a and b or a;",
            &[and, or],
            2,
        ),
    ];
    for (symbolic, alternative, tokens, expected) in rows {
        assert_spells::<P>(alternative, tokens);
        for body in [symbolic, alternative] {
            let got = measure::<P, _>(body, |s| s.metrics.cognitive.cognitive_sum());
            assert_eq!(got, expected, "`{body}` must score like `{symbolic}`");
        }
    }
}

#[cfg(feature = "cpp")]
mod cpp {
    use super::*;

    const KINDS: AltKinds = [
        Cpp::Not as u16,
        Cpp::Compl as u16,
        Cpp::And as u16,
        Cpp::Or as u16,
        Cpp::Bitand as u16,
        Cpp::Bitor as u16,
        Cpp::Xor as u16,
        Cpp::NotEq as u16,
        Cpp::AndEq as u16,
        Cpp::OrEq as u16,
        Cpp::XorEq as u16,
    ];

    #[test]
    fn halstead_bills_alternatives_like_symbols() {
        assert_halstead_matches_symbols::<CppParser>(KINDS);
    }

    #[test]
    fn halstead_keeps_and_distinct_from_ampamp() {
        assert_mixed_spellings_are_distinct_operators::<CppParser>(Cpp::And as u16);
    }

    #[test]
    fn cyclomatic_scores_alternatives_like_symbols() {
        assert_cyclomatic_matches_symbols::<CppParser>(Cpp::And as u16, Cpp::Or as u16);
    }

    #[test]
    fn cognitive_scores_alternatives_like_symbols() {
        assert_cognitive_matches_symbols::<CppParser>(Cpp::And as u16, Cpp::Or as u16);
    }
}

#[cfg(feature = "mozcpp")]
mod mozcpp {
    use super::*;

    const KINDS: AltKinds = [
        Mozcpp::Not as u16,
        Mozcpp::Compl as u16,
        Mozcpp::And as u16,
        Mozcpp::Or as u16,
        Mozcpp::Bitand as u16,
        Mozcpp::Bitor as u16,
        Mozcpp::Xor as u16,
        Mozcpp::NotEq as u16,
        Mozcpp::AndEq as u16,
        Mozcpp::OrEq as u16,
        Mozcpp::XorEq as u16,
    ];

    #[test]
    fn halstead_bills_alternatives_like_symbols() {
        assert_halstead_matches_symbols::<MozcppParser>(KINDS);
    }

    #[test]
    fn halstead_keeps_and_distinct_from_ampamp() {
        assert_mixed_spellings_are_distinct_operators::<MozcppParser>(Mozcpp::And as u16);
    }

    #[test]
    fn cyclomatic_scores_alternatives_like_symbols() {
        assert_cyclomatic_matches_symbols::<MozcppParser>(Mozcpp::And as u16, Mozcpp::Or as u16);
    }

    #[test]
    fn cognitive_scores_alternatives_like_symbols() {
        assert_cognitive_matches_symbols::<MozcppParser>(Mozcpp::And as u16, Mozcpp::Or as u16);
    }
}
