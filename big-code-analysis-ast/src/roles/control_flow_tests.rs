//! The control-flow rows — `Branch`, `Switch`, `Loop`, `Catch`,
//! `ElseIf`, `Exit` — pinned independently of the consumers that read
//! them (#1697): for each language, a fixture spelling every owner and
//! payer kind, and a hand-written table of what each row answers per
//! node, as the consumers answered before they read the rows. A kind
//! absent from the table answers all-false; every declared owner and
//! payer must be met by the fixture, and every defensive alias must
//! stay declared and unseen (`owners_covered`'s shape, #1698).
//!
//! Every `ElseIf` strategy (`grammar-dispatch.md` §4 lists them:
//! parent clause, previous sibling, Go's parent `if`, the dedicated
//! clause, Python's sole statement, and `absent`) gets the issue's three
//! shapes where its grammar has them: a chain (`if / else if / else`),
//! a genuinely nested `if` inside an `else` block, and the language's
//! own clause form — and the row must tell the first from the second.

use std::collections::BTreeSet;

use crate::test_support::for_each_node_with_chain;
use crate::{Ancestors, Checker, Node};

/// What one node answers across the six rows, one slot per projection
/// a consumer reads: each row's `owns_kind` and `pays_at_leaf_kind`,
/// except `Exit`, which every consumer reads at the owner. The slots are
/// named in `slot`, which is what the tables spell.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
type Answers = [bool; 11];

/// The slot of each row projection in [`Answers`].
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
mod slot {
    pub(super) const BRANCH: usize = 0;
    pub(super) const BRANCH_LEAF: usize = 1;
    pub(super) const SWITCH: usize = 2;
    pub(super) const SWITCH_LEAF: usize = 3;
    pub(super) const LOOP: usize = 4;
    pub(super) const LOOP_LEAF: usize = 5;
    pub(super) const CATCH: usize = 6;
    pub(super) const CATCH_LEAF: usize = 7;
    pub(super) const ELSE_IF: usize = 8;
    pub(super) const ELSE_IF_LEAF: usize = 9;
    pub(super) const EXIT: usize = 10;
}
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
use slot::*;

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
const NONE: Answers = [false; 11];

/// A `(row answers)` reader over a language's six row modules, so the
/// per-language test is a table and a fixture, nothing else.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
macro_rules! answers {
    ($roles:path) => {{
        use $roles::*;
        |node: &Node<'_>, code: &[u8], ancestors: Ancestors<'_, '_>| -> Answers {
            let kind = node.kind_id();
            [
                branch::owns_kind(kind, node, code, ancestors),
                branch::pays_at_leaf_kind(kind, node, code, ancestors),
                switch::owns_kind(kind, node, code, ancestors),
                switch::pays_at_leaf_kind(kind, node, code, ancestors),
                r#loop::owns_kind(kind, node, code, ancestors),
                r#loop::pays_at_leaf_kind(kind, node, code, ancestors),
                catch::owns_kind(kind, node, code, ancestors),
                catch::pays_at_leaf_kind(kind, node, code, ancestors),
                else_if::owns_kind(kind, node, code, ancestors),
                else_if::pays_at_leaf_kind(kind, node, code, ancestors),
                exit::owns_kind(kind, node, code, ancestors),
            ]
        }
    }};
}

/// Every declared owner and payer of the six rows, for the fixture
/// guard.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
macro_rules! declared {
    ($roles:path) => {{
        use $roles::*;
        [
            branch::OWNERS,
            branch::PAYERS,
            switch::OWNERS,
            switch::PAYERS,
            r#loop::OWNERS,
            r#loop::PAYERS,
            catch::OWNERS,
            catch::PAYERS,
            else_if::OWNERS,
            else_if::PAYERS,
            exit::OWNERS,
            exit::PAYERS,
        ]
    }};
}

/// Walks `source`, answers the six rows for every node whose kind the
/// `expected` table names (or that any row answers non-false for), and
/// compares against the table in walk order. `defensive` kinds must be
/// declared and never emitted.
///
/// A listed kind pins an all-false *negative* only when the node is
/// named: Ruby spells its `if` keyword token and its `if` statement with
/// one `kind()` string, and the token is a negative nobody means to
/// list. An anonymous node still appears whenever a row answers for it
/// (the keyword payers).
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
fn check<C: Checker + crate::LanguageInfo>(
    source: &[u8],
    read: impl Fn(&Node<'_>, &[u8], Ancestors<'_, '_>) -> Answers,
    expected: &[(&str, Answers)],
    declared: &[&[u16]],
    defensive: &[u16],
) {
    let mut seen_kinds = BTreeSet::new();
    let mut got: Vec<(String, Answers)> = Vec::new();
    for_each_node_with_chain::<C>(source, |node, chain| {
        seen_kinds.insert(node.kind_id());
        let answers = read(node, source, Ancestors::known(chain));
        let listed = node.is_named() && expected.iter().any(|(k, _)| *k == node.kind());
        if listed || answers != NONE {
            got.push((node.kind().to_owned(), answers));
        }
    });
    let want: Vec<(String, Answers)> = expected
        .iter()
        .map(|(k, a)| ((*k).to_owned(), *a))
        .collect();
    pretty_assertions::assert_eq!(got, want);
    for &k in defensive {
        assert!(
            declared.iter().copied().flatten().any(|&d| d == k),
            "defensive kind {k} is no longer declared"
        );
        assert!(
            !seen_kinds.contains(&k),
            "defensive kind {k} is now emitted; its row needs a look"
        );
    }
    for &k in declared.iter().copied().flatten() {
        if !defensive.contains(&k) {
            assert!(seen_kinds.contains(&k), "fixture never emits kind {k}");
        }
    }
}

/// The answers with the slots in `set` true — the table's shorthand:
/// `at(&[BRANCH, BRANCH_LEAF])` is a plain `Branch` owner (owns and pays
/// at the leaf), `at(&[BRANCH, ELSE_IF])` the chained `if` a `pays []`
/// `ElseIf` row owns beside `Branch` and pays nowhere.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
const fn at(set: &[usize]) -> Answers {
    let mut a = NONE;
    let mut i = 0;
    while i < set.len() {
        a[set[i]] = true;
        i += 1;
    }
    a
}

#[cfg(feature = "java")]
mod java {
    use super::*;
    use crate::{Java, JavaCode};

    // Plain rows pay at both projections; split rows pay at the keyword.
    // The chained `if` (previous sibling `else`) is a `Branch` owner the
    // `ElseIf` row also owns and never pays at; the `if` nested in an
    // `else { … }` block is a `Branch` owner only. `switch` statement and
    // expression share the `switch_expression` kind; `for`, enhanced
    // `for`, `while` and `do` each own, and the `for` / `while` tokens
    // pay — the `while` of `do … while` too (#284).
    #[test]
    fn rows_answer_the_table() {
        let source = b"class A {\n  int f(int a, java.util.List<Integer> xs) {\n    if (a == 1) { return 1; } else if (a == 2) { return 2; } else { if (a == 3) { throw new RuntimeException(); } }\n    for (int i = 0; i < 3; i++) { }\n    for (Integer x : xs) { }\n    while (a > 0) { a--; }\n    do { a++; } while (a < 0);\n    switch (a) { case 1: return 1; default: break; }\n    int y = switch (a) { case 2 -> 2; default -> { yield 0; } };\n    try { } catch (RuntimeException e) { } catch (Exception e) { } finally { }\n    return y;\n  }\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, ELSE_IF])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("throw_statement", at(&[EXIT])),
            ("for_statement", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("enhanced_for_statement", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("while_statement", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("do_statement", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("switch_expression", at(&[SWITCH])),
            ("switch", at(&[SWITCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("switch_expression", at(&[SWITCH])),
            ("switch", at(&[SWITCH_LEAF])),
            ("yield_statement", at(&[EXIT])),
            ("catch_clause", at(&[CATCH])),
            ("catch", at(&[CATCH_LEAF])),
            ("catch_clause", at(&[CATCH])),
            ("catch", at(&[CATCH_LEAF])),
            ("return_statement", at(&[EXIT])),
        ];
        check::<JavaCode>(
            source,
            answers!(crate::roles::java),
            expected,
            &declared!(crate::roles::java),
            &[],
        );
    }

    /// The `Checker` spelling forwards to the row with no bytes, which
    /// is exact because the row reads none; the chained `if` is told
    /// from the nested one by its previous sibling.
    #[test]
    fn is_else_if_is_the_row_and_tells_a_chain_from_a_nested_if() {
        let source = b"class A { void f(int a) { if (a == 1) { } else if (a == 2) { } else { if (a == 3) { } } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            if node.kind_id() == Java::IfStatement as u16 {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    JavaCode::is_else_if(node, ancestors),
                    crate::roles::java::else_if::owns(node, source, ancestors),
                ));
            }
        });
        assert_eq!(seen, [(false, false), (true, true), (false, false)]);
        assert_eq!(
            (
                crate::roles::java::else_if::TEXT_GATED,
                crate::roles::java::branch::TEXT_GATED
            ),
            (false, false)
        );
    }

    /// The body occurrence cognitive nests at is the `switch_block`,
    /// the owner's child; no row claims it.
    #[test]
    fn switch_body_is_the_owners_child_and_no_rows() {
        use crate::roles::java::{is_switch_body_kind, switch};
        let source = b"class A { int f(int a) { switch (a) { default: return 0; } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            if is_switch_body_kind(node.kind_id()) {
                let parent = Ancestors::known(chain)
                    .parent(node)
                    .expect("a block has a parent");
                seen.push((
                    parent.kind().to_owned(),
                    switch::owns(&parent, source, Ancestors::known(&chain[..chain.len() - 1])),
                ));
            }
        });
        assert_eq!(seen, [("switch_expression".to_owned(), true)]);
        assert!(!switch::lists_kind(Java::SwitchBlock as u16));
        // The owner projects to its `switch` keyword, the row's payer.
        let mut projected = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, _| {
            if node.kind_id() == Java::SwitchExpression as u16 {
                projected.push(switch::project(node).map(|k| k.kind().to_owned()));
            }
        });
        assert_eq!(projected, [Some("switch".to_owned())]);
    }
}

/// The JS family: four grammars with one kind spelling, so one expected
/// table serves all four; the row *ids* differ per dialect, which the
/// per-dialect `declared!` guard checks. Keyword payers (`if`, `for`,
/// `while`, `catch`); `for … of` and `for … in` are one
/// `for_in_statement`; the `else if` is an `if_statement` under an
/// `else_clause` (a `Branch` owner the `ElseIf` row also owns and never
/// pays at), the nested one under a block is `Branch`'s only; the
/// optional-binding `catch { }` is the same clause.
#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
macro_rules! js_family_rows_answer_the_table {
    ($name:ident, $code:ident, $roles:path) => {
        #[test]
        fn $name() {
            let source = b"function f(a, xs) {\n  if (a === 1) { return 1; } else if (a === 2) { return 2; } else { if (a === 3) { throw new Error(); } }\n  for (let i = 0; i < 3; i++) { }\n  for (const x of xs) { }\n  for (const k in xs) { }\n  while (a > 0) { a--; }\n  do { a++; } while (a < 0);\n  switch (a) { case 1: return 1; default: break; }\n  try { } catch (e) { } finally { }\n  try { } catch { }\n}\nfunction* g() { yield 1; yield* [2]; }\n";
            let expected: &[(&str, Answers)] = &[
                ("if_statement", at(&[BRANCH])),
                ("if", at(&[BRANCH_LEAF])),
                ("return_statement", at(&[EXIT])),
                ("if_statement", at(&[BRANCH, ELSE_IF])),
                ("if", at(&[BRANCH_LEAF])),
                ("return_statement", at(&[EXIT])),
                ("if_statement", at(&[BRANCH])),
                ("if", at(&[BRANCH_LEAF])),
                ("throw_statement", at(&[EXIT])),
                ("for_statement", at(&[LOOP])),
                ("for", at(&[LOOP_LEAF])),
                ("for_in_statement", at(&[LOOP])),
                ("for", at(&[LOOP_LEAF])),
                ("for_in_statement", at(&[LOOP])),
                ("for", at(&[LOOP_LEAF])),
                ("while_statement", at(&[LOOP])),
                ("while", at(&[LOOP_LEAF])),
                ("do_statement", at(&[LOOP])),
                ("while", at(&[LOOP_LEAF])),
                ("switch_statement", at(&[SWITCH, SWITCH_LEAF])),
                ("return_statement", at(&[EXIT])),
                ("catch_clause", at(&[CATCH])),
                ("catch", at(&[CATCH_LEAF])),
                ("catch_clause", at(&[CATCH])),
                ("catch", at(&[CATCH_LEAF])),
                ("yield_expression", at(&[EXIT])),
                ("yield_expression", at(&[EXIT])),
            ];
            check::<crate::$code>(source, answers!($roles), expected, &declared!($roles), &[]);
        }
    };
}

#[cfg(feature = "javascript")]
mod javascript {
    use super::*;
    use crate::{Javascript, JavascriptCode};

    js_family_rows_answer_the_table!(
        rows_answer_the_table,
        JavascriptCode,
        crate::roles::javascript
    );

    #[test]
    fn is_else_if_is_the_row_and_tells_a_chain_from_a_nested_if() {
        let source = b"function f(a) { if (a) { } else if (a) { } else { if (a) { } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavascriptCode>(source, |node, chain| {
            if node.kind_id() == Javascript::IfStatement as u16 {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    JavascriptCode::is_else_if(node, ancestors),
                    crate::roles::javascript::else_if::owns(node, source, ancestors),
                ));
            }
        });
        assert_eq!(seen, [(false, false), (true, true), (false, false)]);
        assert_eq!(
            (
                crate::roles::javascript::else_if::TEXT_GATED,
                crate::roles::javascript::branch::TEXT_GATED
            ),
            (false, false)
        );
    }
}

// test-lang-gates: hand-written(mozjs) — the parser is named only
//     through the `js_family_rows_answer_the_table!` invocation, which
//     the derivation does not expand
#[cfg(feature = "mozjs")]
mod mozjs {
    use super::*;

    js_family_rows_answer_the_table!(rows_answer_the_table, MozjsCode, crate::roles::mozjs);
}

// test-lang-gates: hand-written(typescript) — the two parsers are
//     named only through the `js_family_rows_answer_the_table!`
//     invocations, which the derivation does not expand
#[cfg(feature = "typescript")]
mod typescript {
    use super::*;

    js_family_rows_answer_the_table!(
        rows_answer_the_table,
        TypescriptCode,
        crate::roles::typescript
    );
    js_family_rows_answer_the_table!(tsx_rows_answer_the_table, TsxCode, crate::roles::tsx);
}

/// The C family. C has no exceptions and no range `for`; C++ (and its
/// Mozilla twin, one grammar with different ids) adds `for_range_loop`,
/// `catch_clause` paid at `catch`, `throw`, `co_return` and `co_yield`;
/// ObjC pays its `@catch` clause at the owner (its keyword is `ATcatch`)
/// and spells `@throw` as a `throw_statement`. Keyword payers for `if`
/// / `for` / `while` throughout (#284).
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
macro_rules! cpp_family_rows_answer_the_table {
    ($name:ident, $code:ident, $roles:path) => {
        #[test]
        fn $name() {
            let source = b"#include <vector>\nint f(int a, std::vector<int> v) {\n  if (a == 1) { return 1; } else if (a == 2) { return 2; } else { if (a == 3) { throw 1; } }\n  for (int i = 0; i < 3; i++) { }\n  for (auto x : v) { }\n  while (a > 0) { a--; }\n  do { a++; } while (a < 0);\n  switch (a) { case 1: return 1; default: break; }\n  try { } catch (int e) { } catch (...) { }\n  return 0;\n}\ngenerator g() { co_yield 1; co_return; }\n";
            let expected: &[(&str, Answers)] = &[
                ("if_statement", at(&[BRANCH])),
                ("if", at(&[BRANCH_LEAF])),
                ("return_statement", at(&[EXIT])),
                ("if_statement", at(&[BRANCH, ELSE_IF])),
                ("if", at(&[BRANCH_LEAF])),
                ("return_statement", at(&[EXIT])),
                ("if_statement", at(&[BRANCH])),
                ("if", at(&[BRANCH_LEAF])),
                ("throw_statement", at(&[EXIT])),
                ("for_statement", at(&[LOOP])),
                ("for", at(&[LOOP_LEAF])),
                ("for_range_loop", at(&[LOOP])),
                ("for", at(&[LOOP_LEAF])),
                ("while_statement", at(&[LOOP])),
                ("while", at(&[LOOP_LEAF])),
                ("do_statement", at(&[LOOP])),
                ("while", at(&[LOOP_LEAF])),
                ("switch_statement", at(&[SWITCH, SWITCH_LEAF])),
                ("return_statement", at(&[EXIT])),
                ("catch_clause", at(&[CATCH])),
                ("catch", at(&[CATCH_LEAF])),
                ("catch_clause", at(&[CATCH])),
                ("catch", at(&[CATCH_LEAF])),
                ("return_statement", at(&[EXIT])),
                ("co_yield_statement", at(&[EXIT])),
                ("co_return_statement", at(&[EXIT])),
            ];
            check::<crate::$code>(source, answers!($roles), expected, &declared!($roles), &[]);
        }
    };
}

#[cfg(feature = "c")]
mod c {
    use super::*;
    use crate::{C, CCode};

    #[test]
    fn rows_answer_the_table() {
        let source = b"int f(int a) {\n  if (a == 1) { return 1; } else if (a == 2) { return 2; } else { if (a == 3) { return 3; } }\n  for (int i = 0; i < 3; i++) { }\n  while (a > 0) { a--; }\n  do { a++; } while (a < 0);\n  switch (a) { case 1: return 1; default: break; }\n  return 0;\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, ELSE_IF])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("for_statement", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("while_statement", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("do_statement", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("switch_statement", at(&[SWITCH, SWITCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("return_statement", at(&[EXIT])),
        ];
        check::<CCode>(
            source,
            answers!(crate::roles::c),
            expected,
            &declared!(crate::roles::c),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_row_and_tells_a_chain_from_a_nested_if() {
        let source = b"int f(int a) { if (a) { } else if (a) { } else { if (a) { } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<CCode>(source, |node, chain| {
            if node.kind_id() == C::IfStatement as u16 {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    CCode::is_else_if(node, ancestors),
                    crate::roles::c::else_if::owns(node, source, ancestors),
                ));
            }
        });
        assert_eq!(seen, [(false, false), (true, true), (false, false)]);
        assert_eq!(
            (
                crate::roles::c::else_if::TEXT_GATED,
                crate::roles::c::catch::OWNERS
            ),
            (false, &[] as &[u16])
        );
    }
}

#[cfg(feature = "cpp")]
mod cpp {
    use super::*;
    use crate::{Cpp, CppCode};

    cpp_family_rows_answer_the_table!(rows_answer_the_table, CppCode, crate::roles::cpp);

    #[test]
    fn is_else_if_is_the_row_and_tells_a_chain_from_a_nested_if() {
        let source = b"int f(int a) { if (a) { } else if (a) { } else { if (a) { } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<CppCode>(source, |node, chain| {
            if node.kind_id() == Cpp::IfStatement as u16 {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    CppCode::is_else_if(node, ancestors),
                    crate::roles::cpp::else_if::owns(node, source, ancestors),
                ));
            }
        });
        assert_eq!(
            (seen, crate::roles::cpp::else_if::TEXT_GATED),
            (vec![(false, false), (true, true), (false, false)], false)
        );
    }
}

// test-lang-gates: hand-written(mozcpp) — the parser is named only
//     through the `cpp_family_rows_answer_the_table!` invocation
#[cfg(feature = "mozcpp")]
mod mozcpp {
    use super::*;

    cpp_family_rows_answer_the_table!(rows_answer_the_table, MozcppCode, crate::roles::mozcpp);
}

#[cfg(feature = "objc")]
mod objc {
    use super::*;
    use crate::ObjcCode;

    #[test]
    fn rows_answer_the_table() {
        let source = b"#import <Foundation/Foundation.h>\n@implementation A\n- (int)f:(int)a xs:(NSArray *)xs {\n  if (a == 1) { return 1; } else if (a == 2) { return 2; } else { if (a == 3) { @throw [NSException new]; } }\n  for (int i = 0; i < 3; i++) { }\n  for (id x in xs) { }\n  while (a > 0) { a--; }\n  do { a++; } while (a < 0);\n  switch (a) { case 1: return 1; default: break; }\n  @try { } @catch (NSException *e) { } @finally { }\n  return 0;\n}\n@end\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, ELSE_IF])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("throw_statement", at(&[EXIT])),
            ("for_statement", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("for_statement", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("while_statement", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("do_statement", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("switch_statement", at(&[SWITCH, SWITCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("catch_clause", at(&[CATCH, CATCH_LEAF])),
            ("return_statement", at(&[EXIT])),
        ];
        check::<ObjcCode>(
            source,
            answers!(crate::roles::objc),
            expected,
            &declared!(crate::roles::objc),
            &[],
        );
    }
}

/// Groovy: Java's shape — keyword payers, `switch_expression` paid at
/// `switch` with the body a no-row's, `else if` told by the previous
/// `else` token.
#[cfg(feature = "groovy")]
mod groovy {
    use super::*;
    use crate::{Groovy, GroovyCode};

    #[test]
    fn rows_answer_the_table() {
        let source = b"class A {\n  int f(int a, List xs) {\n    if (a == 1) { return 1 } else if (a == 2) { return 2 } else { if (a == 3) { throw new RuntimeException() } }\n    for (int i = 0; i < 3; i++) { }\n    for (x in xs) { }\n    while (a > 0) { a-- }\n    do { a++ } while (a < 0)\n    switch (a) { case 1: return 1; default: break }\n    int y = switch (a) { case 2 -> 2; default -> { yield 0 } }\n    try { } catch (RuntimeException e) { } finally { }\n    return y\n  }\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, ELSE_IF])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("throw_statement", at(&[EXIT])),
            ("for_statement", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("for_in_statement", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("while_statement", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("do_while_statement", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("switch_expression", at(&[SWITCH])),
            ("switch", at(&[SWITCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("switch_expression", at(&[SWITCH])),
            ("switch", at(&[SWITCH_LEAF])),
            ("yield_statement", at(&[EXIT])),
            ("catch_clause", at(&[CATCH])),
            ("catch", at(&[CATCH_LEAF])),
            ("return_statement", at(&[EXIT])),
        ];
        check::<GroovyCode>(
            source,
            answers!(crate::roles::groovy),
            expected,
            &declared!(crate::roles::groovy),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_row_and_the_switch_body_is_the_owners_child() {
        use crate::roles::groovy::{is_switch_body_kind, switch};
        let source = b"class A { void f(int a) { if (a) { } else if (a) { } else { if (a) { } }; switch (a) { default: break } } }\n";
        let mut seen = Vec::new();
        let mut bodies = Vec::new();
        for_each_node_with_chain::<GroovyCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            if node.kind_id() == Groovy::IfStatement as u16 {
                seen.push((
                    GroovyCode::is_else_if(node, ancestors),
                    crate::roles::groovy::else_if::owns(node, source, ancestors),
                ));
            }
            if is_switch_body_kind(node.kind_id()) {
                let parent = ancestors.parent(node).expect("a block has a parent");
                bodies.push(switch::owns(
                    &parent,
                    source,
                    Ancestors::known(&chain[..chain.len() - 1]),
                ));
            }
        });
        assert_eq!(
            (seen, bodies, crate::roles::groovy::else_if::TEXT_GATED),
            (
                vec![(false, false), (true, true), (false, false)],
                vec![true],
                false
            )
        );
        assert!(!switch::lists_kind(Groovy::SwitchBlock as u16));
    }
}

/// C#: statement-paying rows; `switch` statement and expression both
/// owned; `throw` as a statement and an expression; `yield`.
#[cfg(feature = "csharp")]
mod csharp {
    use super::*;
    use crate::{Csharp, CsharpCode};

    #[test]
    fn rows_answer_the_table() {
        let source = b"using System;\nclass C {\n  int F(int a, int[] xs) {\n    if (a == 1) { return 1; } else if (a == 2) { return 2; } else { if (a == 3) { throw new Exception(); } }\n    for (int i = 0; i < 3; i++) { }\n    foreach (var x in xs) { }\n    while (a > 0) { a--; }\n    do { a++; } while (a < 0);\n    switch (a) { case 1: return 1; default: break; }\n    int y = a switch { 1 => 1, _ => 0 };\n    try { } catch (Exception e) { } finally { }\n    string s = null ?? throw new Exception();\n    return y;\n  }\n  System.Collections.Generic.IEnumerable<int> G() { yield return 1; yield break; }\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, BRANCH_LEAF, ELSE_IF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])),
            ("throw_statement", at(&[EXIT])),
            ("for_statement", at(&[LOOP, LOOP_LEAF])),
            ("foreach_statement", at(&[LOOP, LOOP_LEAF])),
            ("while_statement", at(&[LOOP, LOOP_LEAF])),
            ("do_statement", at(&[LOOP, LOOP_LEAF])),
            ("switch_statement", at(&[SWITCH, SWITCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("switch_expression", at(&[SWITCH, SWITCH_LEAF])),
            ("catch_clause", at(&[CATCH, CATCH_LEAF])),
            ("throw_expression", at(&[EXIT])),
            ("return_statement", at(&[EXIT])),
            ("yield_statement", at(&[EXIT])),
            ("yield_statement", at(&[EXIT])),
        ];
        check::<CsharpCode>(
            source,
            answers!(crate::roles::csharp),
            expected,
            &declared!(crate::roles::csharp),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_row_and_tells_a_chain_from_a_nested_if() {
        let source = b"class C { void F(int a) { if (a == 1) { } else if (a == 2) { } else { if (a == 3) { } } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<CsharpCode>(source, |node, chain| {
            if node.kind_id() == Csharp::IfStatement as u16 {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    CsharpCode::is_else_if(node, ancestors),
                    crate::roles::csharp::else_if::owns(node, source, ancestors),
                ));
            }
        });
        assert_eq!(
            (seen, crate::roles::csharp::else_if::TEXT_GATED),
            (vec![(false, false), (true, true), (false, false)], false)
        );
    }
}

/// Kotlin: expression-paying rows; `when` is the switch; `else if` told
/// by the previous `else` token.
#[cfg(feature = "kotlin")]
mod kotlin {
    use super::*;
    use crate::{Kotlin, KotlinCode};

    #[test]
    fn rows_answer_the_table() {
        let source = b"fun f(a: Int, xs: List<Int>): Int {\n  if (a == 1) { return 1 } else if (a == 2) { return 2 } else { if (a == 3) { throw RuntimeException() } }\n  for (x in xs) { }\n  while (a > 0) { }\n  do { } while (a < 0)\n  val y = when (a) { 1 -> 1; else -> 0 }\n  try { } catch (e: RuntimeException) { } finally { }\n  return y\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if_expression", at(&[BRANCH, BRANCH_LEAF])),
            ("return_expression", at(&[EXIT])),
            ("if_expression", at(&[BRANCH, BRANCH_LEAF, ELSE_IF])),
            ("return_expression", at(&[EXIT])),
            ("if_expression", at(&[BRANCH, BRANCH_LEAF])),
            ("throw_expression", at(&[EXIT])),
            ("for_statement", at(&[LOOP, LOOP_LEAF])),
            ("while_statement", at(&[LOOP, LOOP_LEAF])),
            ("do_while_statement", at(&[LOOP, LOOP_LEAF])),
            ("when_expression", at(&[SWITCH, SWITCH_LEAF])),
            ("catch_block", at(&[CATCH, CATCH_LEAF])),
            ("return_expression", at(&[EXIT])),
        ];
        check::<KotlinCode>(
            source,
            answers!(crate::roles::kotlin),
            expected,
            &declared!(crate::roles::kotlin),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_row_and_tells_a_chain_from_a_nested_if() {
        let source =
            b"fun f(a: Int) { if (a == 1) { } else if (a == 2) { } else { if (a == 3) { } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<KotlinCode>(source, |node, chain| {
            if node.kind_id() == Kotlin::IfExpression as u16 {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    KotlinCode::is_else_if(node, ancestors),
                    crate::roles::kotlin::else_if::owns(node, source, ancestors),
                ));
            }
        });
        assert_eq!(
            (seen, crate::roles::kotlin::else_if::TEXT_GATED),
            (vec![(false, false), (true, true), (false, false)], false)
        );
    }
}

/// Go: statement-paying rows; one `for_statement` for every loop
/// spelling; three switch-like statements; `else if` is an `if` whose
/// parent is the outer `if`; `panic(…)` by callee text, `foo.panic()` not.
#[cfg(feature = "go")]
mod go {
    use super::*;
    use crate::{Go, GoCode};

    #[test]
    fn rows_answer_the_table() {
        let source = b"package p\nfunc f(a int, xs []int, c chan int, x interface{}, g struct{ panic func() }) int {\n\tif a == 1 { return 1 } else if a == 2 { return 2 } else { if a == 3 { panic(\"x\") } }\n\tfor i := 0; i < 3; i++ { }\n\tfor _, v := range xs { _ = v }\n\tfor { break }\n\tswitch a { case 1: return 1; default: }\n\tswitch x.(type) { case int: return 1; default: }\n\tselect { case <-c: ; default: }\n\tg.panic()\n\treturn 0\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, BRANCH_LEAF, ELSE_IF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])),
            ("call_expression", at(&[EXIT])), // panic("x")
            ("for_statement", at(&[LOOP, LOOP_LEAF])),
            ("for_statement", at(&[LOOP, LOOP_LEAF])),
            ("for_statement", at(&[LOOP, LOOP_LEAF])),
            ("expression_switch_statement", at(&[SWITCH, SWITCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("type_switch_statement", at(&[SWITCH, SWITCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("select_statement", at(&[SWITCH, SWITCH_LEAF])),
            ("call_expression", NONE), // g.panic(): a selector, a user function
            ("return_statement", at(&[EXIT])),
        ];
        check::<GoCode>(
            source,
            answers!(crate::roles::go),
            expected,
            &declared!(crate::roles::go),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_row_and_tells_a_chain_from_a_nested_if() {
        let source = b"package p\nfunc f(a int) { if a == 1 { } else if a == 2 { } else { if a == 3 { } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<GoCode>(source, |node, chain| {
            if node.kind_id() == Go::IfStatement as u16 {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    GoCode::is_else_if(node, ancestors),
                    crate::roles::go::else_if::owns(node, source, ancestors),
                ));
            }
        });
        assert_eq!(
            (
                seen,
                crate::roles::go::else_if::TEXT_GATED,
                crate::roles::go::exit::TEXT_GATED,
                crate::roles::go::catch::OWNERS
            ),
            (
                vec![(false, false), (true, true), (false, false)],
                false,
                true,
                &[] as &[u16]
            )
        );
    }
}

/// Bash: statement-paying rows; `elif` is a dedicated clause paying for
/// itself; `return` / `exit` are commands told by name, `echo exit` not.
#[cfg(feature = "bash")]
mod bash {
    use super::*;
    use crate::BashCode;

    #[test]
    fn rows_answer_the_table() {
        let source = b"f() {\n  if [ \"$1\" = 1 ]; then return 1; elif [ \"$1\" = 2 ]; then return 2; else if [ \"$1\" = 3 ]; then exit 3; fi; fi\n  for x in a b; do :; done\n  for ((i = 0; i < 3; i++)); do :; done\n  while true; do break; done\n  until false; do break; done\n  case \"$1\" in 1) return 1 ;; *) echo exit ;; esac\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])),
            ("command", at(&[EXIT])), // return 1 (`[ … ]` is a test_command)
            ("elif_clause", at(&[ELSE_IF, ELSE_IF_LEAF])),
            ("command", at(&[EXIT])), // return 2
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])),
            ("command", at(&[EXIT])), // exit 3
            ("for_statement", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE), // :
            ("c_style_for_statement", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE),
            ("while_statement", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE), // true
            ("command", NONE), // break
            ("while_statement", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE), // false
            ("command", NONE), // break
            ("case_statement", at(&[SWITCH, SWITCH_LEAF])),
            ("command", at(&[EXIT])), // return 1
            ("command", NONE),        // echo exit
        ];
        check::<BashCode>(
            source,
            answers!(crate::roles::bash),
            expected,
            &declared!(crate::roles::bash),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_clause_and_the_exit_row_reads_bytes() {
        let source = b"if a; then :; elif b; then :; else if c; then :; fi; fi\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<BashCode>(source, |node, chain| {
            if matches!(node.kind(), "if_statement" | "elif_clause") {
                seen.push((
                    node.kind(),
                    BashCode::is_else_if(node, Ancestors::known(chain)),
                ));
            }
        });
        assert_eq!(
            (
                seen,
                crate::roles::bash::else_if::TEXT_GATED,
                crate::roles::bash::exit::TEXT_GATED
            ),
            (
                vec![
                    ("if_statement", false),
                    ("elif_clause", true),
                    ("if_statement", false)
                ],
                false,
                true
            )
        );
    }
}

/// Lua: statement-paying rows; `elseif` is a dedicated statement paying
/// for itself; `error(…)` / `os.exit(…)` by callee text, `myError()` not.
#[cfg(feature = "lua")]
mod lua {
    use super::*;
    use crate::LuaCode;

    #[test]
    fn rows_answer_the_table() {
        let source = b"local function f(a, xs)\n  if a == 1 then return 1 elseif a == 2 then return 2 else if a == 3 then error(\"x\") end end\n  for i = 1, 3 do end\n  for k, v in pairs(xs) do end\n  while a > 0 do a = a - 1 end\n  repeat a = a + 1 until a > 0\n  os.exit(1)\n  myError()\n  return 0\nend\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("elseif_statement", at(&[ELSE_IF, ELSE_IF_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])),
            ("function_call", at(&[EXIT])), // error("x")
            ("for_statement", at(&[LOOP, LOOP_LEAF])),
            ("for_statement", at(&[LOOP, LOOP_LEAF])),
            ("function_call", NONE), // pairs(xs)
            ("while_statement", at(&[LOOP, LOOP_LEAF])),
            ("repeat_statement", at(&[LOOP, LOOP_LEAF])),
            ("function_call", at(&[EXIT])), // os.exit(1)
            ("function_call", NONE),        // myError()
            ("return_statement", at(&[EXIT])),
        ];
        check::<LuaCode>(
            source,
            answers!(crate::roles::lua),
            expected,
            &declared!(crate::roles::lua),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_clause() {
        let source = b"if a then elseif b then else if c then end end\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<LuaCode>(source, |node, chain| {
            if matches!(node.kind(), "if_statement" | "elseif_statement") {
                seen.push((
                    node.kind(),
                    LuaCode::is_else_if(node, Ancestors::known(chain)),
                ));
            }
        });
        assert_eq!(
            (
                seen,
                crate::roles::lua::switch::OWNERS,
                crate::roles::lua::catch::OWNERS
            ),
            (
                vec![
                    ("if_statement", false),
                    ("elseif_statement", true),
                    ("if_statement", false)
                ],
                &[] as &[u16],
                &[] as &[u16]
            )
        );
    }
}

/// PHP: statement-paying rows; both `else if` spellings (#529) — the
/// one-word clause pays for itself, the two-word nested `if` is owned
/// and pays nothing; `switch` and `match`; `exit` a statement, `die(…)`
/// a call left out.
#[cfg(feature = "php")]
mod php {
    use super::*;
    use crate::{Php, PhpCode};

    #[test]
    fn rows_answer_the_table() {
        let source = b"<?php\nfunction f($a, $xs) {\n  if ($a == 1) { return 1; } elseif ($a == 2) { return 2; } else if ($a == 3) { return 3; } else { if ($a == 4) { throw new Exception(); } }\n  for ($i = 0; $i < 3; $i++) { }\n  foreach ($xs as $x) { }\n  while ($a > 0) { $a--; }\n  do { $a++; } while ($a < 0);\n  switch ($a) { case 1: return 1; default: break; }\n  $y = match ($a) { 1 => 1, default => 0 };\n  try { } catch (Exception $e) { } finally { }\n  exit(1);\n  die(\"x\");\n  return $y;\n}\nfunction g() { yield 1; }\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("else_if_clause", at(&[ELSE_IF, ELSE_IF_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, BRANCH_LEAF, ELSE_IF])), // two-word `else if`
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, BRANCH_LEAF])), // nested under `else { … }`
            ("throw_expression", at(&[EXIT])),
            ("for_statement", at(&[LOOP, LOOP_LEAF])),
            ("foreach_statement", at(&[LOOP, LOOP_LEAF])),
            ("while_statement", at(&[LOOP, LOOP_LEAF])),
            ("do_statement", at(&[LOOP, LOOP_LEAF])),
            ("switch_statement", at(&[SWITCH, SWITCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("match_expression", at(&[SWITCH, SWITCH_LEAF])),
            ("catch_clause", at(&[CATCH, CATCH_LEAF])),
            ("exit_statement", at(&[EXIT])),
            ("return_statement", at(&[EXIT])),
            ("yield_expression", at(&[EXIT])),
        ];
        // `else_if_clause` has two ids; the pinned grammar emits only
        // `ElseIfClause` — for the brace form, the `:` … `endif`
        // alternative syntax and the bare-statement body alike (dumped).
        // `ElseIfClause2` stays declared as a defensive alias (lesson 34),
        // as the Checker always listed it.
        check::<PhpCode>(
            source,
            answers!(crate::roles::php),
            expected,
            &declared!(crate::roles::php),
            &[Php::ElseIfClause2 as u16],
        );
    }

    #[test]
    fn is_else_if_is_the_row_for_both_spellings() {
        let source = b"<?php\nif ($a) { } elseif ($b) { } else if ($c) { } else { if ($d) { } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<PhpCode>(source, |node, chain| {
            if matches!(node.kind(), "if_statement" | "else_if_clause") {
                seen.push((
                    node.kind(),
                    PhpCode::is_else_if(node, Ancestors::known(chain)),
                ));
            }
        });
        assert_eq!(
            (seen, crate::roles::php::else_if::TEXT_GATED),
            (
                vec![
                    ("if_statement", false),
                    ("else_if_clause", true),
                    ("if_statement", true),
                    ("if_statement", false),
                ],
                false
            )
        );
    }
}

/// Ruby: statement-paying rows, block and modifier forms alike; `elsif`
/// a clause paying for itself; `rescue` under four kinds; `raise` /
/// `exit` / `exit!` by callee with no receiver or `Kernel`, `obj.exit`
/// not. The `if` / `unless` / … keyword tokens share their statement's
/// `kind()` string and are anonymous, so the table lists only the
/// statements.
#[cfg(feature = "ruby")]
mod ruby {
    use super::*;
    use crate::{Ruby, RubyCode};

    #[test]
    fn rows_answer_the_table() {
        let source = b"def f(a, xs)\n  if a == 1 then return 1 elsif a == 2 then return 2 else (if a == 3 then raise \"x\" end) end\n  unless a then exit 1 end\n  return 4 if a\n  return 5 unless a\n  while a > 0 do a -= 1 end\n  until a > 0 do a += 1 end\n  for x in xs do end\n  a += 1 while a < 3\n  a -= 1 until a < 1\n  case a when 1 then 1 else 0 end\n  case a\n  in Integer => n\n    1\n  else\n    0\n  end\n  begin; Kernel.exit 2; rescue ArgumentError => e; rescue => e; end\n  x = (Kernel.raise \"y\" rescue 0)\n  obj.exit\n  exit!\nend\n";
        let expected: &[(&str, Answers)] = &[
            ("if", at(&[BRANCH, BRANCH_LEAF])),
            ("return", at(&[EXIT])),
            ("elsif", at(&[ELSE_IF, ELSE_IF_LEAF])),
            ("return", at(&[EXIT])),
            ("if", at(&[BRANCH, BRANCH_LEAF])),
            ("call", at(&[EXIT])), // raise "x"
            ("unless", at(&[BRANCH, BRANCH_LEAF])),
            ("call", at(&[EXIT])), // exit 1
            ("if_modifier", at(&[BRANCH, BRANCH_LEAF])),
            ("return", at(&[EXIT])),
            ("unless_modifier", at(&[BRANCH, BRANCH_LEAF])),
            ("return", at(&[EXIT])),
            ("while", at(&[LOOP, LOOP_LEAF])),
            ("until", at(&[LOOP, LOOP_LEAF])),
            ("for", at(&[LOOP, LOOP_LEAF])),
            ("while_modifier", at(&[LOOP, LOOP_LEAF])),
            ("until_modifier", at(&[LOOP, LOOP_LEAF])),
            ("case", at(&[SWITCH, SWITCH_LEAF])),
            ("case_match", at(&[SWITCH, SWITCH_LEAF])),
            ("call", at(&[EXIT])), // Kernel.exit 2
            ("rescue", at(&[CATCH, CATCH_LEAF])),
            ("rescue", at(&[CATCH, CATCH_LEAF])),
            ("rescue_modifier", at(&[CATCH, CATCH_LEAF])),
            ("call", at(&[EXIT])), // Kernel.raise "y"
            ("call", NONE),        // obj.exit
            ("call", at(&[EXIT])), // exit!
        ];
        // `rescue_modifier` has three ids, `return` two and `call` four
        // visible (a fifth is the hidden `_call`);
        // the pinned grammar emits only `RescueModifier3` (statement,
        // assignment, argument and `return` position alike — nine
        // spellings dumped), only `Return2` (with and without a value, as
        // a statement and as an operand) and only `Call3` (eighteen call
        // spellings dumped; the `NullSafe` row records the same). The
        // others stay declared as defensive aliases (lesson 34), as the
        // consumers always listed them.
        check::<RubyCode>(
            source,
            answers!(crate::roles::ruby),
            expected,
            &declared!(crate::roles::ruby),
            &[
                Ruby::RescueModifier as u16,
                Ruby::RescueModifier2 as u16,
                Ruby::Return as u16,
                Ruby::Call as u16,
                Ruby::Call2 as u16,
                Ruby::Call4 as u16,
            ],
        );
    }

    #[test]
    fn is_else_if_is_the_clause() {
        let source = b"if a then elsif b then else (if c then end) end\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<RubyCode>(source, |node, chain| {
            if node.is_named() && matches!(node.kind(), "if" | "elsif") {
                seen.push((
                    node.kind(),
                    RubyCode::is_else_if(node, Ancestors::known(chain)),
                ));
            }
        });
        assert_eq!(
            (seen, crate::roles::ruby::exit::TEXT_GATED),
            (vec![("if", false), ("elsif", true), ("if", false)], true)
        );
    }
}

/// iRules: statement-paying rows with dedicated kinds (unlike Tcl);
/// `try`'s `on error` / `trap` handler groups are catches; `return` /
/// `error` are commands told by name (`::return` too), `puts error` not.
#[cfg(feature = "irules")]
mod irules {
    use super::*;
    use crate::IrulesCode;

    #[test]
    fn rows_answer_the_table() {
        let source = b"when HTTP_REQUEST {\n    if {[HTTP::uri] eq \"/a\"} { return } elseif {[HTTP::uri] eq \"/b\"} { return } else { if {1} { error \"x\" } }\n    for {set i 0} {$i < 3} {incr i} { }\n    foreach x [list a b] { }\n    while {1} { break }\n    dict for {k v} [list] { }\n    switch -glob -- [HTTP::uri] { \"/x*\" { return } default { } }\n    catch { error \"y\" } msg\n    try { error \"z\" } on error {m o} { } trap {} {m o} { } finally { }\n    ::return\n    puts error\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if", at(&[BRANCH, BRANCH_LEAF])),
            ("command", NONE),        // [HTTP::uri]
            ("command", at(&[EXIT])), // return
            ("elseif", at(&[ELSE_IF, ELSE_IF_LEAF])),
            ("command", NONE),        // [HTTP::uri]
            ("command", at(&[EXIT])), // return
            ("if", at(&[BRANCH, BRANCH_LEAF])),
            ("command", at(&[EXIT])), // error "x"
            ("for", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE), // incr i (`set i 0` is the start word)
            ("foreach", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE), // list a b
            ("while", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE), // break
            ("dict_for", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE), // list
            ("switch", at(&[SWITCH, SWITCH_LEAF])),
            ("command", NONE),        // [HTTP::uri]
            ("command", at(&[EXIT])), // return
            ("catch", at(&[CATCH, CATCH_LEAF])),
            ("command", at(&[EXIT])), // error "y"
            ("command", at(&[EXIT])), // error "z"
            ("on_handler", at(&[CATCH, CATCH_LEAF])),
            ("trap_handler", at(&[CATCH, CATCH_LEAF])),
            ("command", at(&[EXIT])), // ::return
            ("command", NONE),        // puts error
        ];
        check::<IrulesCode>(
            source,
            answers!(crate::roles::irules),
            expected,
            &declared!(crate::roles::irules),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_clause() {
        let source = b"when HTTP_REQUEST { if {1} { } elseif {2} { } else { if {3} { } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<IrulesCode>(source, |node, chain| {
            if node.is_named() && matches!(node.kind(), "if" | "elseif") {
                seen.push((
                    node.kind(),
                    IrulesCode::is_else_if(node, Ancestors::known(chain)),
                ));
            }
        });
        assert_eq!(
            (seen, crate::roles::irules::exit::TEXT_GATED),
            (vec![("if", false), ("elseif", true), ("if", false)], true)
        );
    }
}

/// Python: keyword payers (#284), with the `if` keyword's three positions
/// told apart — the statement's pays `Branch`, the ternary's is declined
/// by the row's gate (the `conditional_expression` pays instead), the
/// comprehension filter's pays `Branch` as the decision it is; the
/// comprehension `for` clause is a `Loop` owner; `elif` is a clause
/// paying at its keyword, and the sole `if` of an `else:` block is the
/// `ElseIf` row's gated second owner, a `Branch` owner too and paying
/// nothing more. The `match` keyword is contextual and no payer.
#[cfg(feature = "python")]
mod python {
    use super::*;
    use crate::{Python, PythonCode};

    #[test]
    fn rows_answer_the_table() {
        let source = b"def f(a, xs):\n    if a == 1:\n        return 1\n    elif a == 2:\n        return 2\n    else:\n        if a == 3:\n            raise ValueError()\n    assert a\n    for x in xs:\n        pass\n    while a:\n        break\n    y = [x for x in xs if x]\n    z = a if a else 0\n    match a:\n        case 1:\n            return 1\n        case _:\n            pass\n    try:\n        pass\n    except ValueError:\n        pass\n    else:\n        pass\n    yield 1\n";
        let expected: &[(&str, Answers)] = &[
            ("if_statement", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("elif_clause", at(&[ELSE_IF])),
            ("elif", at(&[ELSE_IF_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("if_statement", at(&[BRANCH, ELSE_IF])), // sole `if` of the `else:` block
            ("if", at(&[BRANCH_LEAF])),
            ("raise_statement", at(&[EXIT])),
            ("assert_statement", at(&[BRANCH])),
            ("assert", at(&[BRANCH_LEAF])),
            ("for_statement", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("while_statement", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("for_in_clause", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("if", at(&[BRANCH_LEAF])),       // the comprehension filter's
            ("conditional_expression", NONE), // the `Ternary` row's, not these
            ("match_statement", at(&[SWITCH, SWITCH_LEAF])),
            ("return_statement", at(&[EXIT])),
            ("except_clause", at(&[CATCH])),
            ("except", at(&[CATCH_LEAF])),
            ("yield", at(&[EXIT])),
        ];
        check::<PythonCode>(
            source,
            answers!(crate::roles::python),
            expected,
            &declared!(crate::roles::python),
            &[],
        );
    }

    /// The Checker answers only for the chained `if`: `Branch ∧ ElseIf`,
    /// so the `elif_clause` the row also owns stays false, as every
    /// caller of `is_else_if` has always had it. The `else:` block's
    /// shape is `else_clause → block → if_statement` (#276).
    #[test]
    fn is_else_if_is_the_rows_intersection() {
        let source = b"if a:\n    pass\nelif b:\n    pass\nelse:\n    if c:\n        pass\nif d:\n    pass\nelse:\n    x = 1\n    if e:\n        pass\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            let kind: Python = node.kind_id().into();
            if matches!(kind, Python::IfStatement | Python::ElifClause) {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    node.kind(),
                    PythonCode::is_else_if(node, ancestors),
                    crate::roles::python::else_if::owns(node, source, ancestors),
                ));
            }
        });
        assert_eq!(
            (
                seen,
                crate::roles::python::else_if::TEXT_GATED,
                crate::roles::python::branch::TEXT_GATED
            ),
            (
                vec![
                    ("if_statement", false, false),
                    ("elif_clause", false, true), // the row owns it; the Checker never did
                    ("if_statement", true, true), // sole `if` under `else:`
                    ("if_statement", false, false),
                    ("if_statement", false, false), // a sibling makes it a nest
                ],
                false,
                false
            )
        );
    }
}

/// Rust: keyword payers (#284); `let … else` is a `pays_itself` owner
/// of `Branch` (no keyword of its own); `if let` and `while let` are the
/// plain `if` / `while` kinds; a match guard's `if` is `Branch`'s payer
/// and `Guard`'s alike; `else if` is an `if_expression` under an
/// `else_clause`, the C-family shape; `?` and `return` are the exits.
#[cfg(feature = "rust")]
mod rust {
    use super::*;
    use crate::{Rust, RustCode};

    #[test]
    fn rows_answer_the_table() {
        let source = b"fn f(a: i32, o: Option<i32>, xs: &[i32]) -> Result<i32, ()> {\n    if a == 1 { return Ok(1); } else if a == 2 { return Ok(2); } else { if a == 3 { return Err(()); } }\n    if let Some(v) = o { let _ = v; }\n    let Some(w) = o else { return Ok(0) };\n    for _x in xs { }\n    while a > 0 { break; }\n    while let Some(_) = o { break; }\n    loop { break; }\n    let r = match a { 1 => 1, n if n > 1 => 2, _ => 0 };\n    let q = o.ok_or(())?;\n    Ok(w + r + q)\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if_expression", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_expression", at(&[EXIT])),
            ("if_expression", at(&[BRANCH, ELSE_IF])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_expression", at(&[EXIT])),
            ("if_expression", at(&[BRANCH])),
            ("if", at(&[BRANCH_LEAF])),
            ("return_expression", at(&[EXIT])),
            ("if_expression", at(&[BRANCH])), // `if let`
            ("if", at(&[BRANCH_LEAF])),
            ("let_declaration", NONE), // `let _ = v;`: no `else`
            ("let_declaration", at(&[BRANCH, BRANCH_LEAF])), // `let … else`
            ("return_expression", at(&[EXIT])),
            ("for_expression", at(&[LOOP])),
            ("for", at(&[LOOP_LEAF])),
            ("while_expression", at(&[LOOP])),
            ("while", at(&[LOOP_LEAF])),
            ("while_expression", at(&[LOOP])), // `while let`
            ("while", at(&[LOOP_LEAF])),
            ("loop_expression", at(&[LOOP])),
            ("loop", at(&[LOOP_LEAF])),
            ("let_declaration", NONE),
            ("match_expression", at(&[SWITCH, SWITCH_LEAF])),
            ("if", at(&[BRANCH_LEAF])), // the guard's
            ("let_declaration", NONE),
            ("try_expression", at(&[EXIT])),
        ];
        // `MatchArm2` is the `match_arm`'s second id, the `CaseArm` row's
        // affair; nothing here is defensive.
        check::<RustCode>(
            source,
            answers!(crate::roles::rust),
            expected,
            &declared!(crate::roles::rust),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_row_and_tells_a_chain_from_a_nested_if() {
        let source = b"fn f(a: i32) { if a == 1 { } else if a == 2 { } else { if a == 3 { } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<RustCode>(source, |node, chain| {
            if node.kind_id() == Rust::IfExpression as u16 {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    RustCode::is_else_if(node, ancestors),
                    crate::roles::rust::else_if::owns(node, source, ancestors),
                ));
            }
        });
        assert_eq!(
            (
                seen,
                crate::roles::rust::else_if::TEXT_GATED,
                crate::roles::rust::catch::OWNERS
            ),
            (
                vec![(false, false), (true, true), (false, false)],
                false,
                &[] as &[u16]
            )
        );
    }
}

#[cfg(feature = "perl")]
mod perl {
    use super::*;
    use crate::{Perl, PerlCode};

    // Perl counts statements, so every row but `ElseIf` is plain (owner
    // and leaf agree), and `elsif_clause` — a direct child of the
    // `if_statement`, never a nested `if` — is the `ElseIf` row's one
    // owner, paying for itself. The block forms and the statement-
    // modifier forms are each one owner. `die` / `exit` are bareword
    // calls told by text; the auto-quoted key `(die => 1)`, `Carp::croak`
    // and `$obj->die` are not exits.
    #[test]
    fn rows_answer_the_table() {
        let source = b"sub f { my $a = shift;\n  if ($a == 1) { return 1 } elsif ($a == 2) { return 2 } else { if ($a == 3) { die \"x\" } }\n  unless ($a) { exit 1 }\n  return 4 if $a; return 5 unless $a;\n  while ($a) { last } until ($a) { last }\n  for my $i (1..2) { } for (my $j = 0; $j < 2; $j++) { }\n  $a++ while $a < 3; $a-- until $a < 1; print for 1..2;\n  CORE::die \"y\"; CORE::exit 2; Carp::croak \"z\"; my %h = (die => 1); $obj->die;\n}\n";
        let (branch, lp, elsif) = (
            at(&[BRANCH, BRANCH_LEAF]),
            at(&[LOOP, LOOP_LEAF]),
            at(&[ELSE_IF, ELSE_IF_LEAF]),
        );
        // In walk order. A bareword call that is not `die` / `exit`
        // (`shift`, `print`, `Carp::croak`, `$obj->die`'s callee is an
        // identifier, not listed) answers all-false and is listed so a
        // widened text gate shows. The modifier forms wrap the other way
        // round from the block forms: `return 4 if $a` is a
        // `return_expression` holding the `if_simple_statement`.
        let expected: &[(&str, Answers)] = &[
            ("call_expression_with_bareword", NONE), // shift
            ("if_statement", branch),
            ("return_expression", at(&[EXIT])),
            ("elsif_clause", elsif),
            ("return_expression", at(&[EXIT])),
            ("if_statement", branch),
            ("call_expression_with_bareword", at(&[EXIT])), // die
            ("unless_statement", branch),
            ("call_expression_with_bareword", at(&[EXIT])), // exit
            ("return_expression", at(&[EXIT])),
            ("if_simple_statement", branch),
            ("return_expression", at(&[EXIT])),
            ("unless_simple_statement", branch),
            ("while_statement", lp),
            ("until_statement", lp),
            ("for_statement_2", lp),
            ("for_statement_1", lp),
            ("while_simple_statement", lp),
            ("until_simple_statement", lp),
            ("call_expression_with_bareword", NONE), // print
            ("for_simple_statement", lp),
            ("call_expression_with_bareword", at(&[EXIT])), // CORE::die
            ("call_expression_with_bareword", at(&[EXIT])), // CORE::exit
            ("call_expression_with_bareword", NONE),        // Carp::croak
            ("call_expression_with_bareword", NONE),        // (die => 1): a key
        ];
        check::<PerlCode>(
            source,
            answers!(crate::roles::perl),
            expected,
            &declared!(crate::roles::perl),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_clause_and_the_rows_read_no_bytes() {
        let source = b"sub f { if ($a) { } elsif ($b) { } else { if ($c) { } } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<PerlCode>(source, |node, chain| {
            let k: Perl = node.kind_id().into();
            if matches!(k, Perl::IfStatement | Perl::ElsifClause) {
                seen.push((
                    node.kind(),
                    PerlCode::is_else_if(node, Ancestors::known(chain)),
                ));
            }
        });
        assert_eq!(
            seen,
            [
                ("if_statement", false),
                ("elsif_clause", true),
                ("if_statement", false)
            ]
        );
        assert_eq!(
            (
                crate::roles::perl::else_if::TEXT_GATED,
                crate::roles::perl::exit::TEXT_GATED,
                crate::roles::perl::switch::OWNERS
            ),
            (false, true, &[] as &[u16])
        );
    }
}

/// Tcl: `if` / `elseif` / `foreach` / `while` / `catch` / `try` have
/// kinds of their own and pay at the node; `switch`, `for` and the four
/// exits are generic `command`s told by their leading word, read once
/// per command per walk. A split-form `switch` owns nothing (#467); a
/// `try` owns a decision only with an `on error` handler (#1266);
/// `error` in argument position is not a command (#1270).
#[cfg(feature = "tcl")]
mod tcl {
    use super::*;
    use crate::TclCode;

    #[test]
    fn rows_answer_the_table() {
        let source = b"proc f {a} {\n    if {$a eq \"x\"} { return 1 } elseif {$a eq \"y\"} { return 2 } else { if {1} { error \"e\" } }\n    for {set i 0} {$i < 3} {incr i} { }\n    foreach x [list a b] { }\n    while {1} { break }\n    switch -glob -- $a { \"x*\" { return 3 } default { } }\n    switch $a \"x\" { throw E m } default { }\n    catch { error \"y\" } msg\n    try { exit 1 } on error {m o} { } finally { }\n    try { puts error } finally { }\n    ::return\n}\n";
        let expected: &[(&str, Answers)] = &[
            ("if", at(&[BRANCH, BRANCH_LEAF])),
            ("command", at(&[EXIT])), // return 1
            ("elseif", at(&[ELSE_IF, ELSE_IF_LEAF])),
            ("command", at(&[EXIT])), // return 2
            ("if", at(&[BRANCH, BRANCH_LEAF])),
            ("command", at(&[EXIT])),            // error "e"
            ("command", at(&[LOOP, LOOP_LEAF])), // for …
            ("command", NONE),                   // $i < 3 (a braced script word)
            ("command", NONE),                   // incr i
            ("foreach", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE), // list a b
            ("while", at(&[LOOP, LOOP_LEAF])),
            ("command", NONE),                       // break
            ("command", at(&[SWITCH, SWITCH_LEAF])), // switch -glob -- $a { … }
            ("command", NONE), // the arm list's one command: "x*" { return 3 } default { }
            ("command", at(&[EXIT])), // return 3
            ("command", NONE), // switch $a "x" { … } default { } — split form
            ("command", at(&[EXIT])), // throw E m
            ("catch", at(&[CATCH, CATCH_LEAF])),
            ("command", at(&[EXIT])), // error "y"
            ("try", at(&[CATCH, CATCH_LEAF])),
            ("command", at(&[EXIT])), // exit 1
            ("try", NONE),            // no handler: `finally` alone decides nothing
            ("command", NONE),        // puts error
            ("command", at(&[EXIT])), // ::return
        ];
        check::<TclCode>(
            source,
            answers!(crate::roles::tcl),
            expected,
            &declared!(crate::roles::tcl),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_the_clause_and_the_command_rows_read_bytes() {
        let source = b"if {1} { } elseif {2} { } else { if {3} { } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<TclCode>(source, |node, chain| {
            if node.is_named() && matches!(node.kind(), "if" | "elseif") {
                seen.push((
                    node.kind(),
                    TclCode::is_else_if(node, Ancestors::known(chain)),
                ));
            }
        });
        assert_eq!(
            (
                seen,
                crate::roles::tcl::else_if::TEXT_GATED,
                crate::roles::tcl::switch::TEXT_GATED,
                crate::roles::tcl::r#loop::TEXT_GATED,
                crate::roles::tcl::exit::TEXT_GATED,
            ),
            (
                vec![("if", false), ("elseif", true), ("if", false)],
                false,
                true,
                true,
                true
            )
        );
    }
}

/// Elixir: every control construct is a `call` told by its target's
/// text (#275), so `Branch`, `Switch`, `Loop` and `Exit` each own the
/// one kind under a text gate and pay at the node; `rescue` / `catch`
/// arms are kinds of their own. A remote `Kernel.exit` / `M.throw` has
/// a `dot` target and is no exit; a `quote`d `if` is still a branch
/// (the template's body is read as code); `ElseIf` is absent.
#[cfg(feature = "elixir")]
mod elixir {
    use super::*;
    use crate::ElixirCode;

    #[test]
    fn rows_answer_the_table() {
        let source = b"defmodule M do\n  def f(a) do\n    if a, do: throw(:x), else: raise(\"y\")\n    unless a do\n      reraise(\"z\", [])\n    end\n    for x <- [1], do: exit(:n)\n    while a do\n      g()\n    end\n    case a do\n      1 -> :a\n      _ -> :b\n    end\n    cond do\n      a -> :a\n      true -> :b\n    end\n    with {:ok, v} <- g() do\n      v\n    end\n    try do\n      g()\n    rescue\n      e -> e\n    catch\n      :throw, v -> v\n    end\n    receive do\n      m -> m\n    after\n      0 -> :t\n    end\n    Kernel.exit(:remote)\n    M.throw(:remote)\n    quote do\n      if unquote(a), do: 1\n    end\n  end\nend\n";
        let (branch, switch, lp) = (
            at(&[BRANCH, BRANCH_LEAF]),
            at(&[SWITCH, SWITCH_LEAF]),
            at(&[LOOP, LOOP_LEAF]),
        );
        let expected: &[(&str, Answers)] = &[
            ("call", NONE),        // defmodule
            ("call", NONE),        // def
            ("call", NONE),        // f(a)
            ("call", branch),      // if
            ("call", at(&[EXIT])), // throw
            ("call", at(&[EXIT])), // raise
            ("call", branch),      // unless
            ("call", at(&[EXIT])), // reraise
            ("call", lp),          // for
            ("call", at(&[EXIT])), // exit
            ("call", lp),          // while
            ("call", NONE),        // g()
            ("call", switch),      // case
            ("call", switch),      // cond
            ("call", switch),      // with
            ("call", NONE),        // g()
            ("call", switch),      // try
            ("call", NONE),        // g()
            ("rescue_block", at(&[CATCH, CATCH_LEAF])),
            ("catch_block", at(&[CATCH, CATCH_LEAF])),
            ("call", switch), // receive
            ("call", NONE),   // Kernel.exit
            ("call", NONE),   // M.throw
            ("call", NONE),   // quote
            ("call", branch), // if, inside the template
            ("call", NONE),   // unquote(a)
        ];
        check::<ElixirCode>(
            source,
            answers!(crate::roles::elixir),
            expected,
            &declared!(crate::roles::elixir),
            &[],
        );
    }

    #[test]
    fn is_else_if_is_never_and_the_call_rows_read_bytes() {
        let source = b"if a do\n  1\nelse\n  if b do\n    2\n  end\nend\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            if node.kind() == "call" {
                seen.push(ElixirCode::is_else_if(node, Ancestors::known(chain)));
            }
        });
        assert_eq!(
            (
                seen,
                crate::roles::elixir::branch::TEXT_GATED,
                crate::roles::elixir::switch::TEXT_GATED,
                crate::roles::elixir::r#loop::TEXT_GATED,
                crate::roles::elixir::catch::TEXT_GATED,
                crate::roles::elixir::exit::TEXT_GATED,
            ),
            (vec![false, false], true, true, true, false, true)
        );
    }
}
