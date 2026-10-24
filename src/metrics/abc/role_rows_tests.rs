//! The `Comparison` and `ShortCircuit` rows feed three independent ABC
//! paths — the token arm, the slot's `scores_itself`, and the chain
//! walker — and a fixture any one of them can score reads the right
//! total whichever is dead (grammar-dispatch §11). Each row below gives
//! one input to one path, so a dropped payer, a projection answering
//! `None` or an owner the slot no longer recognises lands on its own
//! number (#1694):
//!
//! - *slot only*: a bare identifier predicate, which only the slot can
//!   pay — zero here means the slot driver stopped being called;
//! - *comparison in a slot*: the token pays and the slot must not —
//!   two here means `scores_itself` lost the owner;
//! - *comparison outside any slot*: `boolean b = x < y;`, which only
//!   the token arm can pay — zero here means the payer was dropped,
//!   and no slot exists to compensate for it;
//! - *the chain*: each operand is a slot and the token pays nothing,
//!   so `a && b` is two and `a && b < 1` is two as well.
//!
//! Cyclomatic is the oracle beside every row (lesson 6): it reads the
//! same `ShortCircuit` row and none of the `Comparison` one, so a
//! comparison row moves `conditions` alone and a chain row moves both.

use crate::test_support::{function_space, space_verbatim};
use crate::{LANG, Metric, MetricsOptions};

/// For each `(body, conditions, decisions)` row, substitutes the body
/// into the template and asserts the function named `f` scores
/// `conditions` ABC conditions and `decisions` cyclomatic decisions.
#[cfg(feature = "java")]
#[track_caller]
fn assert_rows(lang: LANG, template: &str, rows: &[(&str, u64, u64)]) {
    assert!(template.contains("BODY"), "template has no body slot");
    let options = MetricsOptions::default().with_only(&[Metric::Abc, Metric::Cyclomatic]);
    for &(body, conditions, decisions) in rows {
        let source = template.replace("BODY", body);
        let root = space_verbatim(lang, source.as_bytes(), options);
        let f = function_space(&root, "f");
        assert_eq!(
            (
                f.metrics.abc.conditions(),
                f.metrics.cyclomatic.cyclomatic() - 1
            ),
            (conditions, decisions),
            "(abc.conditions, decisions) of `{body}` in {lang:?}\n  source: {source}"
        );
    }
}

#[test]
#[cfg(feature = "java")]
fn java_rows_feed_each_abc_path_independently() {
    let template = "class A { boolean f(boolean a, boolean b, int x, int y, Object o) { BODY } }\n";
    assert_rows(
        LANG::Java,
        template,
        &[
            // slot only
            ("if (a) { } return a;", 1, 1),
            // comparison in a slot: the token pays, the slot does not
            ("if (x < y) { } return a;", 1, 1),
            ("if (x == y) { } return a;", 1, 1),
            ("if (o instanceof String) { } return a;", 1, 1),
            ("if (o instanceof String s) { } return a;", 1, 1),
            // outside any slot: only the token arm can pay
            ("boolean c = x < y; return c;", 1, 0),
            ("boolean c = x >= y; return c;", 1, 0),
            ("boolean c = o instanceof String; return c;", 1, 0),
            ("boolean c = o instanceof String s; return c;", 1, 0),
            // a generic bracket pays nothing (the `<` / `>` gate)
            ("java.util.List<String> l = null; return a;", 0, 0),
            // the chain: two operand slots, nothing for the token
            ("return a && b;", 2, 1),
            ("return a || b;", 2, 1),
            ("boolean c = a && x < 1; return c;", 2, 1),
            ("if (a && b) { } return a;", 2, 2),
            ("if (a && b || x > 1) { } return a;", 3, 3),
        ],
    );
}
