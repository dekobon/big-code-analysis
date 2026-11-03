//! Metric-arm parity derived from the role tables (#1700): the root
//! crate's half of `roles/derived_parity_tests.rs`, for the consumers
//! that re-encode a row's membership inside a metric — which that
//! crate cannot reach, since it computes none.
//!
//! Three readings, each against the production context rather than a
//! fixture's:
//!
//! The premise test reads the control-flow fixtures and nothing else,
//! so it is a `corpus_` test: the CI legs that check out no submodule
//! (`features`, `chain-audit`) filter the prefix out, as they do the
//! corpus binaries, and it runs on `test` and `coverage`.
//!
//! - **`rows_own!`'s premise.** ABC's slot peel asks the `Comparison`,
//!   `ShortCircuit`, `Ternary`, `NullSafe` and (Kotlin) `Branch` rows
//!   with no chain, on the stated grounds that no owner gate of those
//!   rows reads one (`abc.rs`). That is a comment; here it is a
//!   measurement on every owner candidate of the corpus: the row under
//!   a wrong chain equals the row under the walk's.
//! - **Elixir's word policy.** The cyclomatic and cognitive arms
//!   dispatch on `ElixirCallTarget`, not on the rows, which are
//!   `text_gated_by` the same word. For every word the enum spells, a
//!   `Call` the rows own decides and nests except where a declared
//!   policy says otherwise — `with` pays no container decision
//!   (#1535), `try` opens no cognitive level (#416) — and a word the
//!   rows reject does neither. The policy set is declared here, not
//!   re-typed per word, and pinned both ways.
//!
//! Not here, and measured before being left out: `abc.conditions ==
//! cyclomatic() - 1` per function space over the control-flow fixtures.
//! The two agree only on the slot shapes `abc/slot_pays_tests.rs`
//! templates — ABC's Rule 5 counts every `else` arm and no loop, while
//! cyclomatic counts every loop and no `else`, so on a function with
//! both the equality fails by design on every language (36 of the 25
//! fixtures' functions at the first run, recorded on #1700). The
//! relation the issue asks for holds per *construct*, which is what the
//! exact `(conditions, decisions)` pairs there pin and what no corpus
//! function isolates; those rows stay, none retired.

use big_code_analysis_ast::test_support::{control_flow_sources, for_each_node_with_sibling_cache};
use big_code_analysis_ast::with_code_tag;

use crate::lang_helpers::elixir::{ElixirCallTarget, elixir_call_target_word};
use crate::*;

/// Every enabled language with a control-flow fixture.
fn languages_with_fixtures() -> Vec<LANG> {
    let langs: Vec<LANG> = LANG::into_enum_iter()
        .filter(LANG::is_enabled)
        .filter(|l| !matches!(l, LANG::Ccomment | LANG::Preproc))
        .collect();
    crate::test_support::assert_fixtures_present(&langs);
    langs
}

// ---------------------------------------------------------------------------
// rows_own!'s premise
// ---------------------------------------------------------------------------

use crate::metrics::abc::rows_own_inventory_of;

/// `comparison` → `Comparison`, `short_circuit` → `ShortCircuit`: the
/// row module name back to the role token the view carries.
fn snake_to_role(module: &str) -> String {
    let mut out = String::with_capacity(module.len());
    let mut upper = true;
    for c in module.trim_start_matches("r#").chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Every `ROWS_OWN` names roles the language's table declares with
/// owners — a module name mistyped in an inventory would otherwise
/// measure nothing and pass.
#[test]
fn rows_own_inventories_name_declared_rows() {
    for lang in LANG::into_enum_iter() {
        let Some(inventory) = rows_own_inventory_of(lang) else {
            continue;
        };
        let table = big_code_analysis_ast::roles::tables()
            .iter()
            .find(|t| t.lang == lang)
            .expect("every language has a table");
        let rows = (table.rows)();
        for module in inventory {
            let role = snake_to_role(module);
            let declared = rows
                .iter()
                .find(|r| r.role() == role)
                .and_then(|r| r.declared().map(|d| !d.owners.is_empty()));
            assert_eq!(
                declared,
                Some(true),
                "{lang:?}: `rows_own_inventory!` names `{module}`, which is not a declared row with owners"
            );
        }
    }
}

/// On every owner candidate of the rows `rows_own!` reads, the row
/// answers the same under the walk's chain and under a chain that is
/// *wrong* — `Ancestors::known(&[])`, which says the node is the root.
/// A gate whose answer depends on its ancestry disagrees between the
/// two on some node; that is what `abc.rs`'s premise forbids, and the
/// audit's probe (a previous-sibling gate on Kotlin's `Branch`) fails
/// here. What this cannot see: a gate that *reads* the chain and
/// answers alike anyway — `parent().is_none_or(…)` admitting a root
/// and a block the same, or a `Node::parent()` climb that ignores the
/// chain handed in — which reads at `O(depth)` under production's
/// `Ancestors::unknown()` without changing an answer. That is a cost
/// question, not a correctness one, and the read-budget tests are
/// where it lives. `unknown()` itself is not the probe: it climbs
/// `Node::parent` and answers exactly, so every gate agrees with it.
#[test]
fn corpus_rows_own_rows_answer_alike_under_a_wrong_chain() {
    assert!(
        big_code_analysis_ast::test_support::control_flow_corpus_present(),
        "control_flow/ not checked out; run `make worktree-setup` — or filter `corpus_` tests"
    );
    let mut asked = 0;
    let mut disagreements = Vec::new();
    for lang in languages_with_fixtures() {
        let Some(inventory) = rows_own_inventory_of(lang) else {
            continue;
        };
        let roles: Vec<String> = inventory.iter().map(|r| snake_to_role(r)).collect();
        let table = big_code_analysis_ast::roles::tables()
            .iter()
            .find(|t| t.lang == lang)
            .expect("every language has a table");
        let rows: Vec<_> = (table.rows)()
            .into_iter()
            .filter_map(|r| r.declared().copied())
            .filter(|r| roles.iter().any(|role| role == r.role))
            .collect();
        let sources = control_flow_sources(lang).expect("asserted present above");
        for (name, bytes) in sources {
            let code = bytes.as_slice();
            with_code_tag!(lang, |C| {
                for_each_node_with_sibling_cache::<C>(code, |node, ancestors| {
                    let kind = node.kind_id();
                    for row in &rows {
                        if !row.owners.contains(&kind) {
                            continue;
                        }
                        asked += 1;
                        let chained = (row.owns_kind)(kind, node, code, ancestors);
                        let rootless = (row.owns_kind)(kind, node, code, Ancestors::known(&[]));
                        if chained != rootless {
                            disagreements.push(format!(
                                "{lang:?} {} on `{}` in {name}: with chain {chained}, as root {rootless}",
                                row.role,
                                node.kind()
                            ));
                        }
                    }
                });
            });
        }
    }
    // A build enabling only a language with no `rows_own!` call site
    // (Go, Bash, JavaScript) asks nothing, and that is not vacuity.
    let any_inventoried = languages_with_fixtures()
        .iter()
        .any(|l| rows_own_inventory_of(*l).is_some());
    assert!(
        asked > 0 || !any_inventoried,
        "no owner candidate of any rows_own! row"
    );
    assert!(
        disagreements.is_empty(),
        "owner gates rows_own! asks with no chain that read it:\n{}",
        disagreements.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Elixir's word policy
// ---------------------------------------------------------------------------

/// What a word the control rows own may decline to do, by the policy
/// the consumers declare in their arms: `with` pays no container
/// decision, its `<-` clauses being the decisions (#1535,
/// `cyclomatic/elixir.rs`); `try` opens no cognitive level, its
/// handlers being the nesting (#416, `cognitive/elixir.rs`). Declared
/// once, here, and pinned both ways below: a word listed must change
/// an assertion, and a word the metrics treat specially must be
/// listed.
#[cfg(feature = "elixir")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ElixirException {
    NoContainerDecision,
    NoCognitiveLevel,
}

#[cfg(feature = "elixir")]
const ELIXIR_WORD_POLICY: &[(ElixirCallTarget, ElixirException, &str)] = &[
    (
        ElixirCallTarget::With,
        ElixirException::NoContainerDecision,
        "#1535: the `<-` clauses decide, the container does not",
    ),
    (
        ElixirCallTarget::Try,
        ElixirException::NoCognitiveLevel,
        "#416: the handlers nest, the `try` wrapper does not",
    ),
];

/// For every word the enum spells: a one-call body `f` holding
/// `<word> x do _ -> x end`. The rows — `Branch`, `Switch`, `Loop`,
/// text-gated on the word — are asked of that call with the real
/// context; the metrics are run on `f`'s own space and compared to the
/// same body under a plain `foo`. A word the rows own decides
/// (standard or modified cyclomatic above the baseline) and nests
/// (cognitive above it) unless `ELIXIR_WORD_POLICY` says which it
/// forgoes; a word the rows reject does neither. The enum-growth
/// guard: `elixir_call_target_word` is total over `ALL` by its own test.
///
/// Retired the hand-written form in `elixir_spelling_parity_tests.rs`
/// under #1700's protocol: dropping `While` from the `Loop` row gate
/// and, separately, from the cyclomatic arm each failed here beside it.
#[cfg(feature = "elixir")]
#[test]
fn elixir_words_the_rows_own_decide_and_nest_except_by_declared_policy() {
    let table = big_code_analysis_ast::roles::tables()
        .iter()
        .find(|t| t.lang == LANG::Elixir)
        .expect("Elixir has a table");
    let rows: Vec<_> = (table.rows)()
        .into_iter()
        .filter_map(|r| r.declared().copied())
        .filter(|r| matches!(r.role, "Branch" | "Switch" | "Loop"))
        .collect();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter().all(|r| r.text_gated),
        "the control rows read the word"
    );

    let score = |w: &str| {
        let source = format!("def f(x) do\n  {w} x do\n    _ -> x\n  end\nend\n");
        let root = crate::test_support::space_verbatim(
            LANG::Elixir,
            source.as_bytes(),
            MetricsOptions::default().with_only(&[Metric::Cyclomatic, Metric::Cognitive]),
        );
        let f = &crate::test_support::child_space(&root, "f").metrics;
        (
            f.cyclomatic.cyclomatic(),
            f.cyclomatic.cyclomatic_modified(),
            f.cognitive.cognitive(),
        )
    };
    let baseline = score("foo");
    let mut owned_words = Vec::new();
    for target in ElixirCallTarget::ALL {
        let w = elixir_call_target_word(target).unwrap_or("foo");
        let source = format!("def f(x) do\n  {w} x do\n    _ -> x\n  end\nend\n");
        let bytes = source.as_bytes();
        let mut row_owns = None;
        for_each_node_with_sibling_cache::<ElixirCode>(bytes, |node, ancestors| {
            if node.kind_id() == Elixir::Call as u16 && node.start_position().0 == 1 {
                let kind = node.kind_id();
                row_owns = Some(
                    rows.iter()
                        .any(|r| (r.owns_kind)(kind, node, bytes, ancestors)),
                );
            }
        });
        let row_owns = row_owns.expect("the fixture's control call");
        let (std, modified, cog) = score(w);
        let decides = std > baseline.0 || modified > baseline.1;
        let nests = cog > baseline.2;
        let exceptions: Vec<ElixirException> = ELIXIR_WORD_POLICY
            .iter()
            .filter(|(t, ..)| *t == target)
            .map(|(_, e, _)| *e)
            .collect();
        let want_decides = row_owns && !exceptions.contains(&ElixirException::NoContainerDecision);
        let want_nests = row_owns && !exceptions.contains(&ElixirException::NoCognitiveLevel);
        assert_eq!(
            (decides, nests),
            (want_decides, want_nests),
            "`{w}`: rows own it = {row_owns}, policy {exceptions:?}; scores {std} / {modified} / {cog} vs plain {baseline:?}",
        );
        if row_owns {
            owned_words.push(w);
        }
        // A policy entry on a word the rows reject could never be
        // exercised: an exception to a decision that is not made.
        assert!(
            exceptions.is_empty() || row_owns,
            "`{w}` has a policy entry but no row owns it"
        );
    }
    assert_eq!(
        owned_words,
        [
            "if", "unless", "for", "while", "case", "cond", "with", "try", "receive"
        ],
        "the words the three rows own, in enum order"
    );
    // The stale direction of the policy: every entry names a word the
    // rows own (the `exceptions.is_empty() || row_owns` assertion above
    // is per word; this is the same fact read off the owned list) and
    // every entry's exception changed an expectation — a policy on a
    // word that decides and nests regardless would be a lie the metrics
    // tell no one.
    for (target, exception, why) in ELIXIR_WORD_POLICY {
        let w = elixir_call_target_word(*target).unwrap_or("foo");
        assert!(
            owned_words.contains(&w),
            "policy entry {target:?} {exception:?} ({why}) names a word no row owns"
        );
        let (std, modified, cog) = score(w);
        let changed = match exception {
            ElixirException::NoContainerDecision => std == baseline.0 && modified == baseline.1,
            ElixirException::NoCognitiveLevel => cog == baseline.2,
        };
        assert!(
            changed,
            "policy entry {target:?} {exception:?} ({why}) changes nothing: the metrics do not carry it"
        );
    }
}
