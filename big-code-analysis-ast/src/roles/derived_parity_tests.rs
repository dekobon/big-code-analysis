//! Cross-consumer parity derived from the role tables (#1700): for
//! every production language, every declared row, every *candidate*
//! node of a corpus — a node whose kind the row names as an owner, a
//! payer or a leaf, gates aside — ask the row with the walk's real
//! bytes and chain and compare against each `Checker` spelling that
//! reads it, and against the DSL's own invariants. The inputs come
//! from the rows, so a kind no fixture spelled is still asked; the
//! sibling sweep of #1458 is a test run rather than an issue-body
//! table.
//!
//! A comparison discriminates only when its two sides are computed by
//! different code, so every assertion below says which class it is:
//!
//! - **S**, a macro self-test: two emitted items of one expansion
//!   (`owns ⇒ lists_kind`). Cheap, kept, and not consumer parity.
//! - **F**, a context-dropping forward: a `Checker` spelling that
//!   hands the row no bytes or an empty chain (`is_closure`,
//!   `is_else_if`, `is_doc_comment`, the byte-less `is_func_space`).
//!   The row with the real context is the reference; where a language
//!   is known to disagree, the disagreement is a named entry pinned
//!   in both directions, never a tolerance.
//! - **H**, a hand-written consumer: a `Checker` method spelled
//!   independently of the row (Elixir's `promotes_to_func_space`,
//!   Python's `is_else_if = Branch ∧ ElseIf`).
//! - **R**, a row–row relation: two declarations, with every exception
//!   named and a positive count so the relation cannot hold vacuously.
//!   A relation whose right side contains its left by construction is
//!   not one; `DocComment ⊆ Comment ∪ DocComment` is a Checker fact
//!   and sits with the forwards.
//!
//! The metric-arm class — a root-crate consumer re-encoding a row's
//! membership (`rows_own!`, Elixir's `ElixirCallTarget` dispatch) —
//! lives in `src/metrics/derived_parity_tests.rs`, since this crate
//! computes no metric.
//!
//! # What this cannot do
//!
//! It cannot detect an *omitted* row: inputs and expected verdicts come
//! from the same declaration. It cannot ask about a kind the corpus
//! never emits; `make role-coverage-claimed` names those. And a forward
//! whose context the row happens not to read answers the same under
//! any context, so a wrong forward passes here until the day a gate
//! reads what it drops (lesson 103) — which is the day this fails, and
//! is why every forward is compared on every candidate rather than on
//! the fixtures someone thought of. Hand-written fixtures with
//! independently specified expected values, including negative
//! contexts, stay for all three (`roles/*_tests.rs`, `doc_tests/`).
//!
//! # The corpus
//!
//! `control_flow/<lang>/*` in the output submodule — the per-language
//! fixtures #1744 preserved — plus the R09 documentation fixtures read
//! in place through `doc_tests::fixtures()`, plus `BYTE_GATE_FIXTURES`,
//! the shapes a text gate's answer turns on that neither spells
//! (#1863). Mozcpp and Mozjs own no fixture of their own and read the
//! C++ and JavaScript files under their own grammars; Ccomment and
//! Preproc own no extension and have only inline fixtures. Every
//! language must yield at least one candidate, so a language the
//! routing forgets fails rather than passing empty.
//!
//! The CI `features` and `chain-audit` legs run the lib tests with no
//! submodule checked out. The structural assertions — the macro
//! invariants, forward agreement on every node, the declared-list
//! relations, the exact gated-out shapes — hold on the documentation
//! fixtures alone and run there. An obligation that needs the *whole*
//! corpus — a positive per owner kind or per relation, the stale
//! direction of an exception table, Elixir's per-word coverage — is a
//! `corpus_`-prefixed test those legs filter out by name, as they do
//! the corpus binaries, so the skip is visible in the test count rather
//! than a pass; `test` and `coverage` run them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::roles::{DeclaredRow, LanguageTable, RowView};
use crate::test_support::{
    control_flow_corpus_present, control_flow_sources, for_each_node_with_sibling_cache,
    for_each_node_with_sibling_cache_tolerating_errors,
};
use crate::{Ancestors, Checker, LANG, Node, with_code_tag};

// ---------------------------------------------------------------------------
// Sources: what each language's table is asked on
// ---------------------------------------------------------------------------

/// Printed once per run when the control-flow corpus is absent.
static CONTROL_FLOW_ABSENT: std::sync::Once = std::sync::Once::new();

// The exception tables (`NO_POSITIVE_IN_CORPUS`, `KNOWN_DISAGREEMENTS`,
// `RELATION_NO_POSITIVE_IN_CORPUS`) and the per-language positive
// counts describe the *full* corpus — control-flow fixtures and
// documentation fixtures together — so without the submodule
// (`test_support::control_flow_corpus_present`) their stale-direction
// pins and the non-vacuity counts cannot be met honestly: the
// `features` and `chain-audit` CI legs check out no submodule. The
// structural assertions run on the documentation fixtures regardless;
// the corpus-shaped ones are `corpus_` tests that assert the submodule
// is present, and those legs filter the prefix out by name.

/// One source a language is asked on.
struct Source {
    name: String,
    bytes: Vec<u8>,
}

/// Shapes a text gate's answer turns on that no other source spells,
/// kept in the tree so the byte backstop
/// (`unmarked_owners_answer_alike_without_the_bytes`) can see them on
/// the legs that check out no submodule: a gate re-declared under a
/// plain `gated_by` must *move* on some candidate to be caught, and
/// each of these is the one candidate that moves it (#1863).
///
/// - A record whose body writes a same-arity constructor of another
///   type: the header gate compares type spellings, and with no bytes
///   every same-arity constructor spells the empty type and reads as
///   canonical. `B` writes the canonical one, the control.
/// - A `[ … ]` test joined by `-a` / `-o`: one `test_operator` kind
///   spells the connectives and the comparisons, told apart by text.
///
/// Keyed by the table's name, as the exception tables below are: a
/// language is asked its fixture only when its table is enabled. Not
/// by `LANG`, as the documentation fixtures are: a `LANG` literal here
/// makes `utils/check-test-lang-gates.py` derive a gate on these three
/// languages for every test that loads a source, all of which run
/// under any one language.
const BYTE_GATE_FIXTURES: &[(&str, &str, &[u8])] = &[
    (
        "Java",
        "gate/java-records",
        b"record A(int a) { A(long a) { this((int) a); } }\nrecord B(int b) { B(int b) { this.b = b; } }\n",
    ),
    (
        "Groovy",
        "gate/groovy-records",
        b"record A(int a) { A(long a) { this(1) } }\nrecord B(int b) { B(int b) { this.b = b } }\n",
    ),
    (
        "Bash",
        "gate/bash-tests",
        b"if [ \"$a\" -eq 1 -a \"$b\" -eq 2 ]; then :; fi\n[ -n \"$a\" -o -n \"$b\" ]\n",
    ),
];

/// Every source `lang` is asked on: its control-flow files (routed by
/// `test_support::control_flow_sources`), its documentation fixtures,
/// read in place, and its [`BYTE_GATE_FIXTURES`]. Without the output
/// submodule — the CI legs that check out no submodules — the in-tree
/// fixtures alone, which every language has, and the census says so
/// once.
fn sources_for(lang: LANG) -> Vec<Source> {
    let mut out: Vec<Source> = if let Some(files) = control_flow_sources(lang) {
        files
            .into_iter()
            .map(|(name, bytes)| Source { name, bytes })
            .collect()
    } else {
        CONTROL_FLOW_ABSENT.call_once(|| {
            println!(
                "control_flow/ not checked out: the census reads the documentation fixtures \
                 only; run `make worktree-setup` for the control-flow rows"
            );
        });
        Vec::new()
    };
    out.extend(
        super::doc_tests::fixtures()
            .into_iter()
            .filter(|f| f.lang == lang)
            .map(|f| Source {
                name: f.name.to_owned(),
                bytes: f.source.into_owned(),
            }),
    );
    let table = crate::roles::tables()
        .iter()
        .find(|t| t.lang == lang)
        .map(|t| t.name);
    out.extend(
        BYTE_GATE_FIXTURES
            .iter()
            .filter(|(name, _, _)| Some(*name) == table)
            .map(|(_, fixture, bytes)| Source {
                name: (*fixture).to_owned(),
                bytes: bytes.to_vec(),
            }),
    );
    out
}

/// Each fixture names a table and still moves a gate that table marks
/// `text_gated_by`: some node of a marked owner kind answers `owns`
/// differently under the fixture's bytes and under none. That is the
/// fixture's whole use — it is the candidate that would move the gate
/// were it re-declared `gated_by` — and nothing else would notice it
/// gone. The backstop skips a marked owner, so a fixture keyed by a
/// name no table carries, or one a grammar bump or a gate edit left
/// answering alike both ways, leaves the backstop passing with that
/// gate unguarded.
#[test]
fn byte_gate_fixtures_each_move_a_marked_gate() {
    let tables = crate::roles::tables();
    for (name, fixture, code) in BYTE_GATE_FIXTURES {
        let table = tables
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("`{fixture}` names `{name}`, which is no table"));
        if !table.lang.is_enabled() {
            continue;
        }
        let rows: Vec<DeclaredRow> = (table.rows)()
            .iter()
            .filter_map(RowView::declared)
            .copied()
            .collect();
        let mut moved = false;
        with_code_tag!(table.lang, |C| {
            for_each_node_with_sibling_cache::<C>(code, |node, ancestors| {
                let ancestors = ancestors.without_folds();
                let kind = node.kind_id();
                moved |= rows.iter().any(|row| {
                    row.text_gated_owners.contains(&kind)
                        && (row.owns_kind)(kind, node, code, ancestors)
                            != (row.owns_kind)(kind, node, &[], ancestors)
                });
            });
        });
        assert!(
            moved,
            "`{fixture}` moves no `text_gated_by` owner of {name}: the gate it was written \
             for answers alike with and without its bytes"
        );
    }
}

// ---------------------------------------------------------------------------
// The census
// ---------------------------------------------------------------------------

/// What one row answered on one candidate node: one flag per emitted
/// predicate, read together so the S-layer relations between them can
/// be asserted per node.
#[derive(Clone, Copy, Debug)]
#[allow(clippy::struct_excessive_bools)]
struct Verdict {
    owns: bool,
    pays_at_leaf: bool,
    pays_at_token: bool,
    pays_at_wrapper: bool,
    is_callable: bool,
    projects: bool,
}

/// One candidate occurrence: a node whose kind some row of its
/// language declares, with every row's verdict that declared it. The
/// word is `elixir_call_target` for an Elixir `Call`, so an Elixir
/// control row's occurrences are told apart by what they spell.
struct Occurrence {
    kind: u16,
    zero_width: bool,
    word: Option<&'static str>,
    verdicts: Vec<(usize, Verdict)>,
}

/// How often one kind was a candidate of a row, and how often the row
/// owned it — two counts of one thing that read alike, so named.
#[derive(Default, Clone, Copy)]
struct KindCount {
    seen: usize,
    owned: usize,
}

/// Per-row tallies over every candidate: how many owned, how many a
/// gated owner rejected, and per kind the [`KindCount`].
#[derive(Default)]
struct RowTally {
    owned: usize,
    rejected_owners: usize,
    kinds_seen: BTreeMap<u16, KindCount>,
}

/// The language's rows with every declared row's kind index, built
/// once per language so a node asks only the rows that declare its
/// kind: `O(declaring rows)` per node, not `O(rows)`.
struct Census {
    lang: LANG,
    /// The `LANG` variant's spelling, as `tables()` and the exception
    /// tables name it.
    name: &'static str,
    /// `doc_tests/parity.rs`'s named targets beyond the space tree,
    /// read once per language rather than per node.
    targets_beyond_space_tree: Vec<u16>,
    opens_space_fn: for<'a> fn(&Node<'a>, &[u8], Ancestors<'a, '_>) -> crate::SpaceKind,
    kind_name_fn: fn(u16) -> &'static str,
    rows: Vec<RowView>,
    /// The declared row at each index of `rows`, `None` for an absent
    /// or pending one — so a row index resolves in `O(1)`.
    declared: Vec<Option<DeclaredRow>>,
    by_kind: BTreeMap<u16, Vec<usize>>,
    tallies: Vec<RowTally>,
    occurrences: Vec<Occurrence>,
    nodes: usize,
}

impl Census {
    fn new(table: &LanguageTable) -> Self {
        let rows = (table.rows)();
        let declared: Vec<Option<DeclaredRow>> =
            rows.iter().map(|r| r.declared().copied()).collect();
        let mut by_kind: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
        for (i, row) in declared.iter().enumerate() {
            let Some(row) = row else { continue };
            for kind in row.candidate_kinds() {
                by_kind.entry(kind).or_default().push(i);
            }
        }
        let tallies = rows.iter().map(|_| RowTally::default()).collect();
        Self {
            lang: table.lang,
            name: table.name,
            targets_beyond_space_tree: super::doc_tests::parity::targets_beyond_the_space_tree(
                table.name,
            ),
            opens_space_fn: table.opens_space,
            kind_name_fn: table.kind_name,
            rows,
            declared,
            by_kind,
            tallies,
            occurrences: Vec::new(),
            nodes: 0,
        }
    }

    /// Every declared row with its index into `rows`.
    fn declared_rows(&self) -> impl Iterator<Item = (usize, &DeclaredRow)> {
        self.declared
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.as_ref().map(|r| (i, r)))
    }

    fn declared_row(&self, index: usize) -> &DeclaredRow {
        self.declared[index]
            .as_ref()
            .expect("index came from the declared list")
    }

    /// The declared row for `role`, `None` where the table declares it
    /// absent or pending. A role no table has is a typo, and one that
    /// returned `None` would make every relation over it hold
    /// vacuously, so it panics instead.
    fn row_named(&self, role: &str) -> Option<&DeclaredRow> {
        assert!(
            crate::roles::ROLES.contains(&role),
            "`{role}` is not a role; the relation over it would hold vacuously"
        );
        self.declared_rows()
            .map(|(_, r)| r)
            .find(|r| r.role == role)
    }

    /// The grammar's spelling of `kind`, from the generated enum: a
    /// declared kind is named whether or not the corpus spelled it, so
    /// a declared-list check cannot skip an unobserved kind.
    fn kind_name(&self, kind: u16) -> &'static str {
        (self.kind_name_fn)(kind)
    }

    fn record<'a>(&mut self, node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) {
        self.nodes += 1;
        let kind = node.kind_id();
        let Some(indices) = self.by_kind.get(&kind) else {
            return;
        };
        // Elixir's `Call` is the one candidate kind the rows tell apart
        // by text rather than by id; `Other` — every user call — is
        // recorded as no word.
        let word = (self.lang == LANG::Elixir && kind == crate::Elixir::Call as u16)
            .then(|| {
                crate::lang_helpers::elixir::elixir_call_target_word(
                    crate::lang_helpers::elixir::elixir_call_target(node, code, ancestors),
                )
            })
            .flatten();
        let zero_width = node.start_byte() == node.end_byte();
        let mut verdicts = Vec::with_capacity(indices.len());
        for &i in indices {
            // Copied, not borrowed: the tally below is a mutable borrow of
            // `self` beside this one.
            let row = *self.declared_row(i);
            let v = Verdict {
                owns: (row.owns_kind)(kind, node, code, ancestors),
                pays_at_leaf: (row.pays_at_leaf_kind)(kind, node, code, ancestors),
                pays_at_token: (row.pays_at_token_kind)(kind, node, code, ancestors),
                pays_at_wrapper: (row.pays_at_wrapper)(node, code, ancestors),
                is_callable: (row.is_callable_kind)(kind, node, code, ancestors),
                projects: (row.project)(node).is_some(),
            };
            let tally = &mut self.tallies[i];
            let count = tally.kinds_seen.entry(kind).or_default();
            count.seen += 1;
            if v.owns {
                tally.owned += 1;
                count.owned += 1;
            } else if row.owners.contains(&kind) {
                tally.rejected_owners += 1;
            }
            verdicts.push((i, v));
        }
        self.occurrences.push(Occurrence {
            kind,
            zero_width,
            word,
            verdicts,
        });
    }
}

/// Walks every source of `lang` through the table, recording every
/// candidate. The walk's chain carries the fold and sibling caches the
/// metric walk lends, so a gate that folds is priced as production
/// prices it.
fn census_of(table: &LanguageTable) -> Census {
    let mut census = Census::new(table);
    let lang = table.lang;
    let sources = sources_for(lang);
    assert!(
        !sources.is_empty(),
        "{lang:?}: no source routed; the table is asked nothing"
    );
    for source in &sources {
        let code = source.bytes.as_slice();
        with_code_tag!(lang, |C| {
            for_each_node_with_sibling_cache::<C>(code, |node, ancestors| {
                census.record(node, code, ancestors);
            });
        });
    }
    assert!(
        !census.occurrences.is_empty(),
        "{lang:?}: {} sources, {} nodes, no candidate of any row",
        sources.len(),
        census.nodes
    );
    census
}

/// Every enabled language's census, built once per test run.
fn every_census() -> Vec<Census> {
    let out: Vec<Census> = crate::roles::tables()
        .iter()
        .filter(|table| table.lang.is_enabled())
        .map(census_of)
        .collect();
    assert!(
        !out.is_empty(),
        "at least one language feature must be enabled for this test to mean anything"
    );
    out
}

// ---------------------------------------------------------------------------
// S — macro invariants, every row, every candidate
// ---------------------------------------------------------------------------

/// What `role_table!` means, asked of every candidate of every row of
/// every language on the corpus rather than of three synthetic Java
/// tables. Three of these relate an emitted `const` to an emitted
/// predicate and can fail on a macro defect: an owner that owns is
/// listed (`owns ⇒ lists_kind`), a leaf that pays is a declared leaf
/// kind (`pays_at_leaf ⇒ LEAF_KINDS`), a `holds_payer` owner that owns
/// projects (a `@clause_payer` without its `PAYERS.contains`). The
/// other three restate an emitted body — `pays_at_wrapper == owns ∧
/// ¬PAYS_NEVER`, `is_callable ⇒ owns ∧ CALLABLE`, `pays_at_token ⇒
/// PAYERS ∧ pays_at_leaf` — and pin that body's shape, nothing more.
/// A self-test of the macro either way, and labelled as one.
#[test]
fn macro_invariants_hold_on_every_candidate() {
    let mut asked = 0;
    let mut languages = 0;
    let mut zero_width = 0;
    for census in every_census() {
        languages += 1;
        for occ in &census.occurrences {
            // A zero-width node — error recovery's invented token, or a
            // valid empty body — is a candidate by kind and is recorded,
            // never asserted on (lesson 95).
            if occ.zero_width {
                zero_width += 1;
                continue;
            }
            for (i, v) in &occ.verdicts {
                let row = census.declared_row(*i);
                let at = format!(
                    "{} {} on `{}` (kind {})",
                    census.name,
                    row.role,
                    census.kind_name(occ.kind),
                    occ.kind
                );
                asked += 1;
                assert!(
                    !v.owns || (row.lists_kind)(occ.kind),
                    "{at}: owns but not listed"
                );
                assert!(
                    !v.pays_at_token || row.payers.contains(&occ.kind),
                    "{at}: pays at the token but is no declared payer"
                );
                assert!(
                    !v.pays_at_leaf || row.leaf_kinds.contains(&occ.kind),
                    "{at}: pays at the leaf but is no declared leaf kind"
                );
                assert!(
                    !v.is_callable || (v.owns && row.callable.contains(&occ.kind)),
                    "{at}: callable but not an owning CALLABLE kind"
                );
                assert_eq!(
                    v.pays_at_wrapper,
                    v.owns && !row.pays_never,
                    "{at}: pays_at_wrapper is owns ∧ ¬PAYS_NEVER"
                );
                assert!(
                    !v.pays_at_token || v.pays_at_leaf,
                    "{at}: pays at the token but not at the leaf"
                );
                // A `holds_payer` owner owns by its projection landing on
                // a payer, so one that owns projects.
                assert!(
                    !v.owns || !row.holds_payer.contains(&occ.kind) || v.projects,
                    "{at}: a holds_payer owner that owns but projects to nothing"
                );
            }
        }
    }
    // Not asserted zero: a valid tree can carry one (Bash's empty
    // heredoc body, lesson 95). Reported so a fixture that gains one is
    // visible, and skipped above so nothing is asked of it.
    if zero_width > 0 {
        println!("zero-width candidates skipped: {zero_width}");
    }
    // Ccomment and Preproc declare three kinds each over a one-line
    // fixture; every other language asks dozens per file. The bound is
    // per language so a single-grammar build is held to its share.
    assert!(
        asked >= 2 * languages,
        "{asked} verdicts over {languages} language(s); the corpus is thinner than it was"
    );
}

/// Every `(language, role, owner kind)` the corpus spells but whose
/// gate or projection never admits, each with why: a `holds_payer`
/// owner whose shared kind (`binary_expression`) never holds the row's
/// payer in these fixtures, or a gated owner whose every corpus
/// spelling is the gate's negative. Pinned in both directions — an
/// entry that gains a positive fails as stale — so the assertion below
/// is the one the issue's projection perturbation needs: make a row's
/// `project` return `None` and its `holds_payer` owners stop owning on
/// every occurrence, which drops a kind that is *not* listed here to
/// zero and fails. Per owner kind, not per row: a row with two owners
/// can lose every positive of one while the other keeps it green.
const NO_POSITIVE_IN_CORPUS: &[(&str, &str, &str, &str)] = &[
    (
        "C",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Csharp",
        "Modifier",
        "modifier",
        "the childless `modifier` alias owns only `ref` / `out` / `in` / `scoped` / `readonly` / `static` / `async` in a parameter, none spelled; `file` and `required` are kinds of their own",
    ),
    (
        "Csharp",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Elixir",
        "Member",
        "call",
        "`defstruct` is not spelled; the row is text-gated on it",
    ),
    (
        "Go",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Groovy",
        "Container",
        "object_creation_expression",
        "the one `new` has no class body; the gate admits only an anonymous class",
    ),
    (
        "Groovy",
        "DeclKeyword",
        "type_identifier",
        "`var` as a field's type is not spelled; the row is text-gated on the identifier",
    ),
    (
        "Groovy",
        "Primitive",
        "identifier",
        "no primitive type is spelled as a bare `identifier`; the row is text-gated on it",
    ),
    (
        "Groovy",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Irules",
        "ShortCircuit",
        "binop_expr",
        "a `binop_expr` owns a short circuit only when it holds `&&` / `||` / `and` / `or`; none in the fixtures",
    ),
    (
        "Irules",
        "StringLit",
        "braced_word",
        "every braced word is a script body or a value slot the gate rejects",
    ),
    (
        "Java",
        "Container",
        "object_creation_expression",
        "the one `new` has no class body; the gate admits only an anonymous class",
    ),
    (
        "Javascript",
        "Closure",
        "arrow_function",
        "the one arrow is bound (`const arrow = …`), the gate's negative",
    ),
    (
        "Javascript",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Kotlin",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Lua",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `and` / `or`; none in the fixtures",
    ),
    (
        "Mozjs",
        "Closure",
        "arrow_function",
        "the one arrow is bound (`const arrow = …`), the gate's negative",
    ),
    (
        "Mozjs",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Objc",
        "Modifier",
        "identifier",
        "no property-attribute word is spelled; the row is text-gated on the identifier",
    ),
    (
        "Objc",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Perl",
        "EagerLogical",
        "binary_expression",
        "a `binary_expression` owns an eager logical only when it holds `xor`; none in the fixtures",
    ),
    (
        "Perl",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||` / `and` / `or`; none in the fixtures",
    ),
    (
        "Perl",
        "ShortCircuit",
        "unary_expression",
        "tree-sitter-perl spells a low-precedence `and` as a two-operand `unary_expression` (#1539); none in the fixtures",
    ),
    (
        "Php",
        "EagerLogical",
        "binary_expression",
        "a `binary_expression` owns an eager logical only when it holds `xor`; none in the fixtures",
    ),
    (
        "Php",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Ruby",
        "ShortCircuit",
        "binary",
        "a `binary` owns a short circuit only when it holds `&&` / `||` / `and` / `or`; none in the fixtures",
    ),
    (
        "Rust",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Tcl",
        "ShortCircuit",
        "binop_expr",
        "a `binop_expr` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Tsx",
        "Closure",
        "arrow_function",
        "the two arrows are bound, the gate's negative",
    ),
    (
        "Tsx",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
    (
        "Typescript",
        "Closure",
        "arrow_function",
        "the one arrow is bound (`const arrow = …`), the gate's negative",
    ),
    (
        "Typescript",
        "ShortCircuit",
        "binary_expression",
        "a `binary_expression` owns a short circuit only when it holds `&&` / `||`; none in the fixtures",
    ),
];

/// The owner-kind positives and the `NO_POSITIVE_IN_CORPUS` pins,
/// computed once: what the corpus contradicts (an entry with a
/// positive — wrong on any subset of the corpus), what it leaves
/// unowned, what it leaves unmet, and the printed report.
struct OwnerPositives {
    contradicted: Vec<String>,
    unowned: Vec<String>,
    unmet: Vec<String>,
    report: String,
}

fn owner_positives(censuses: &[Census]) -> OwnerPositives {
    let mut unowned = Vec::new();
    let mut contradicted = Vec::new();
    let mut unmet = Vec::new();
    let mut used = std::collections::BTreeSet::new();
    let mut report = String::new();
    for census in censuses {
        for (i, row) in census.declared_rows() {
            let tally = &census.tallies[i];
            for &kind in row.owners {
                let Some(&KindCount { seen, owned }) = tally.kinds_seen.get(&kind) else {
                    continue;
                };
                let name = census.kind_name(kind);
                let named = NO_POSITIVE_IN_CORPUS
                    .iter()
                    .any(|(l, role, k, _)| *l == census.name && *role == row.role && *k == name);
                if named {
                    used.insert((census.name, row.role, name));
                }
                match (owned > 0, named) {
                    (false, false) => unowned.push(format!(
                        "{} {} `{name}` ({kind}): {seen} candidate(s), none owned",
                        census.name, row.role
                    )),
                    (true, true) => contradicted.push(format!(
                        "{} {} `{name}`: named as having no positive, but owns {owned} of {seen}",
                        census.name, row.role
                    )),
                    _ => {}
                }
            }
        }
        // The no-occurrence rows, as this walk sees them: every
        // `(role, kind)` the language declares that no source spelled.
        for (i, row) in census.declared_rows() {
            for kind in row.candidate_kinds() {
                if !census.tallies[i].kinds_seen.contains_key(&kind) {
                    let _ = writeln!(
                        report,
                        "no occurrence: {} {} `{}` ({kind})",
                        census.name,
                        row.role,
                        census.kind_name(kind)
                    );
                }
            }
        }
        // A gated owner the corpus never showed a rejected occurrence
        // of: the gate a fixture could delete green (audit-tests F1–F5
        // on R09). Only rows with a gate to delete — text-gated, or
        // several owners of which one may be gated.
        for (i, row) in census.declared_rows() {
            let tally = &census.tallies[i];
            if tally.owned > 0
                && tally.rejected_owners == 0
                && (row.text_gated || row.owners.len() > 1)
            {
                let _ = writeln!(
                    report,
                    "no rejected owner: {} {} ({} owned)",
                    census.name, row.role, tally.owned
                );
            }
        }
    }
    for (l, role, k, why) in NO_POSITIVE_IN_CORPUS {
        let enabled = censuses.iter().any(|c| c.name == *l);
        if enabled && !used.contains(&(*l, *role, *k)) {
            unmet.push(format!(
                "{l} {role} `{k}` ({why}): names no candidate the corpus spells"
            ));
        }
    }
    OwnerPositives {
        contradicted,
        unowned,
        unmet,
        report,
    }
}

/// No `NO_POSITIVE_IN_CORPUS` entry is contradicted by a positive the
/// fixtures spell — true on any subset of the corpus, so asked on
/// every CI leg. The no-occurrence and gate-without-a-negative reports
/// are printed here, never failed (`--nocapture`).
#[test]
fn no_named_no_positive_entry_is_contradicted() {
    let positives = owner_positives(&every_census());
    print!("{}", positives.report);
    assert!(
        positives.contradicted.is_empty(),
        "NO_POSITIVE_IN_CORPUS entries the corpus contradicts:\n{}",
        positives.contradicted.join("\n")
    );
}

/// Every `(language, role, owner kind)` with candidates in the corpus
/// admits at least one, or is named in [`NO_POSITIVE_IN_CORPUS`] with
/// the reason; and every name there still has candidates. Both need
/// the *whole* corpus — a missing positive on a subset proves nothing
/// — so this is a `corpus_` test: the CI legs that check out no
/// submodule filter the prefix out, as they do the corpus binaries,
/// and the skip is visible in their test count rather than a pass.
#[test]
fn corpus_every_owner_kind_the_corpus_spells_owns_somewhere_or_is_named() {
    assert!(
        control_flow_corpus_present(),
        "control_flow/ not checked out; run `make worktree-setup` — or filter `corpus_` tests"
    );
    let positives = owner_positives(&every_census());
    assert!(
        positives.unowned.is_empty(),
        "owner kinds the corpus spells that never own — a gate or projection that rejects \
         every spelling, or a construct the fixtures lack (name it in NO_POSITIVE_IN_CORPUS):\n{}",
        positives.unowned.join("\n")
    );
    assert!(
        positives.unmet.is_empty(),
        "stale NO_POSITIVE_IN_CORPUS entries:\n{}",
        positives.unmet.join("\n")
    );
}

/// The one language whose control rows are told apart by text: which
/// words the corpus spelled as an owned `Call`, per row, so Elixir's
/// coverage is per word and not per kind — `Call` observed says nothing
/// about `while` beside `for`. Printed per word; asserted that the
/// three control rows own at least six distinct words between them.
// test-lang-gates: hand-written(elixir) — the census is built through
//     `with_code_tag!` on every enabled language and this reads the
//     Elixir one, which exists only with its grammar; nothing in the
//     body names the parser
#[cfg(feature = "elixir")]
#[test]
fn corpus_elixir_control_rows_are_covered_per_word() {
    assert!(
        control_flow_corpus_present(),
        "control_flow/ not checked out; run `make worktree-setup` — or filter `corpus_` tests"
    );
    let census = every_census()
        .into_iter()
        .find(|c| c.lang == LANG::Elixir)
        .expect("Elixir is enabled");
    let mut words: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for occ in &census.occurrences {
        let Some(word) = occ.word else {
            continue;
        };
        for (i, v) in &occ.verdicts {
            if v.owns {
                *words
                    .entry((census.declared_row(*i).role, word))
                    .or_default() += 1;
            }
        }
    }
    for ((role, word), n) in &words {
        println!("elixir word: {role} `{word}` owned {n} time(s)");
    }
    let control: BTreeSet<&str> = words
        .keys()
        .filter(|(role, _)| matches!(*role, "Branch" | "Switch" | "Loop"))
        .map(|(_, w)| *w)
        .collect();
    assert!(
        control.len() >= 6,
        "Elixir's control rows own {} distinct words in the corpus: {control:?}",
        control.len()
    );
}

// ---------------------------------------------------------------------------
// F — the Checker spellings against the rows, with the real context
// ---------------------------------------------------------------------------

/// One `Checker` spelling compared against the row it reads, on every
/// candidate of the corpus. `forward` is the consumer as its callers
/// call it — with whatever context its signature drops — and `row` the
/// same question asked of the rows with the walk's bytes and chain.
/// They differ by construction only where the dropped context would
/// have changed the row's answer, which is exactly what a forward
/// cannot see and this can.
struct Forward {
    name: &'static str,
    answered: bool,
    row: bool,
}

/// The rows the forwards read, resolved once per language: what
/// `forwards_on` depends on, by name rather than behind a string
/// looked up per node.
struct ForwardRows {
    branch: Option<DeclaredRow>,
    else_if: Option<DeclaredRow>,
    closure: Option<DeclaredRow>,
    doc_body_residual: Option<DeclaredRow>,
    doc_comment: Option<DeclaredRow>,
    doc_run_directive: Option<DeclaredRow>,
    comment: Option<DeclaredRow>,
    call: Option<DeclaredRow>,
    string_lit: Option<DeclaredRow>,
    func_opener: Option<DeclaredRow>,
    docstring: Option<DeclaredRow>,
}

impl ForwardRows {
    fn of(census: &Census) -> Self {
        let row = |role| census.row_named(role).copied();
        Self {
            branch: row("Branch"),
            else_if: row("ElseIf"),
            closure: row("Closure"),
            doc_body_residual: row("DocBodyResidual"),
            doc_comment: row("DocComment"),
            doc_run_directive: row("DocRunDirective"),
            comment: row("Comment"),
            call: row("Call"),
            string_lit: row("StringLit"),
            func_opener: row("FuncOpener"),
            docstring: row("Docstring"),
        }
    }
}

/// What the forwards of one language answered on one node, handed to
/// `sink` one at a time. One table of pushes, sectioned by what each
/// forward drops; splitting it would scatter the ordering the sections
/// explain.
#[allow(clippy::too_many_lines)]
fn forwards_on<'a, C: Checker + crate::Getter>(
    census: &Census,
    rows: &ForwardRows,
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    sink: &mut dyn FnMut(Forward),
) {
    use crate::SpaceKind;
    let kind = node.kind_id();
    let owns = |row: &Option<DeclaredRow>| {
        row.as_ref()
            .is_some_and(|r| (r.owns_kind)(kind, node, code, ancestors))
    };
    let lists = |row: &Option<DeclaredRow>| row.as_ref().is_some_and(|r| (r.lists_kind)(kind));
    let mut push = |name, answered, row| {
        sink(Forward {
            name,
            answered,
            row,
        });
    };

    // Chain-only forwards: `&[]` bytes, the real chain.
    push("is_else_if", C::is_else_if(node, ancestors), {
        // Python's Checker is `Branch ∧ ElseIf`, so the dedicated
        // `elif_clause` the row also owns stays false; everywhere else
        // the Checker is the row.
        if census.lang == LANG::Python {
            owns(&rows.branch) && owns(&rows.else_if)
        } else {
            owns(&rows.else_if)
        }
    });
    push(
        "is_closure",
        C::is_closure(node, ancestors),
        owns(&rows.closure),
    );
    // The body-holding residual: every hand-written adapter composes
    // `is_func_with_code ∨ is_closure ∨ doc_body_residual::owns` with a
    // body-child test on `below` that no row spells, so the forward is compared
    // against the composition *without* the child test on the one
    // side it can be — a node the forward admits for some child is one
    // the composition admits. Every term reads the real bytes and the
    // real chain, as the adapters do since #1863.
    let holds_for_any_child = node
        .children()
        .any(|below| C::holds_doc_comment_in_body(node, code, ancestors, &below));
    let composition = C::is_func_with_code(node, code, ancestors)
        || C::is_closure(node, ancestors)
        || owns(&rows.doc_body_residual);
    push(
        "holds_doc_comment_in_body ⇒ is_func_with_code ∨ is_closure ∨ row",
        holds_for_any_child,
        holds_for_any_child && composition,
    );
    // And the direction the body-child test cannot defeat: a row owner
    // the adapter admits for *no* child is a row the adapter stopped
    // reading, since every such kind has a body child by the row's
    // definition (`DocBodyResidual` is the residual of bodies).
    let row_owner = owns(&rows.doc_body_residual);
    push(
        "DocBodyResidual row ⇒ holds_doc_comment_in_body for some child",
        row_owner,
        row_owner && holds_for_any_child,
    );

    // Byte-only forwards: the real bytes, a known-empty chain.
    let is_doc_comment = C::is_doc_comment(node, code);
    push("is_doc_comment", is_doc_comment, owns(&rows.doc_comment));
    push(
        "is_doc_run_directive",
        C::is_doc_run_directive(node, code),
        owns(&rows.doc_run_directive),
    );

    // Kind-only forwards: the two rows' kind tables as one.
    let is_comment = C::is_comment(node);
    push(
        "is_comment",
        is_comment,
        lists(&rows.comment) || lists(&rows.doc_comment),
    );
    // The two Checker-level facts the retired `doc_tests/parity.rs`
    // pin asked: a doc comment is a comment, and a docstring is
    // neither. Spelled as `forward ⇒ reference`, so a doc comment the
    // comment table lost fails here on its own node.
    push(
        "is_doc_comment ⇒ is_comment",
        is_doc_comment,
        is_doc_comment && is_comment,
    );
    let docstring = owns(&rows.docstring);
    push(
        "docstring reaches neither is_comment nor is_doc_comment",
        docstring,
        docstring && !is_comment && !is_doc_comment,
    );
    // The kind-only spellings against the *gated* row: they are the
    // kind table (`impl_row_predicates!`), so they disagree exactly on
    // a gate's negatives, and each such negative is a named entry.
    push("is_call", C::is_call(node), owns(&rows.call));
    push("is_string", C::is_string(node), owns(&rows.string_lit));
    // Not `is_primitive`: it answers "is this operator keyed by its
    // text in the Halstead vocabulary", which is the `Primitive` row's
    // kind table where a kind covers several spellings (TypeScript's
    // `predefined_type`) and the trait's `false` where each spelling
    // is a kind of its own (Java) — a consumer of the row's *shape*,
    // not of its membership, so no row answer is its reference.

    // The `_with_code` spellings against the rows, and the byte-less
    // spellings against the `_with_code` ones. The four `_with_code`
    // rows are one-expansion forwards in every language today
    // (`impl_row_predicates!`, `opens_space`): they drop no context
    // and guard only a hand-written override that stops reading its
    // row. The byte-less pair and `promotes_to_func_space_with_code`
    // are where a language overrides (Elixir, Ruby's gate).
    let with_code = C::is_func_space_with_code(node, code, ancestors);
    let opens = C::get_space_kind_with_code(node, code, ancestors);
    push(
        "is_call_with_code",
        C::is_call_with_code(node, code, ancestors),
        owns(&rows.call),
    );
    push(
        "is_string_with_code",
        C::is_string_with_code(node, code, ancestors),
        owns(&rows.string_lit),
    );
    push(
        "is_func_space_with_code",
        with_code,
        opens != SpaceKind::Unknown,
    );
    push(
        "get_space_kind_with_code",
        opens == (census.opens_space_fn)(node, code, ancestors),
        true,
    );
    push("is_func_space", C::is_func_space(node), with_code);
    push("get_space_kind", C::get_space_kind(node) == opens, true);
    push(
        "promotes_to_func_space_with_code",
        C::promotes_to_func_space_with_code(node, code, ancestors),
        C::is_func_with_code(node, code, ancestors) || with_code,
    );
    push(
        "is_func_with_code",
        C::is_func_with_code(node, code, ancestors),
        rows.func_opener
            .as_ref()
            .is_some_and(|r| (r.is_callable_kind)(kind, node, code, ancestors)),
    );
}

/// Where a forward is *known* to disagree with the row, by language,
/// forward and kind: each a context the forward drops and the row
/// reads. Pinned in both directions — an entry that stops disagreeing
/// fails as stale — so a tolerance cannot widen silently. A forward
/// with a build-time `!TEXT_GATED` / `!CHAIN_GATED` pin has no entry
/// here; its row takes neither of what it drops (#1793), and
/// `no_chain_rows_answer_alike_under_the_empty_chain` is the
/// behavioural half of the chain pin.
const KNOWN_DISAGREEMENTS: &[(&str, &str, &str, &str)] = &[
    // The byte-less `is_func_space` / `get_space_kind` forward with
    // `&[], Ancestors::known(&[])`, which is exact only where no
    // opener's gate reads the chain or the bytes. Ruby's `block` /
    // `do_block` under a stabby lambda is the chain case
    // (`ruby_byte_less_spellings_stay_the_kind_table`), which no
    // fixture here spells, and the one forward with no build-time
    // pin; Elixir is the bytes case, and overrides rather than forwards.
    (
        "Elixir",
        "is_func_space",
        "call",
        "`def` / `defmodule` are `Call`s told apart by their target text; the byte-less spelling lists only `source` and `anonymous_function` (#275, #1130)",
    ),
    (
        "Elixir",
        "get_space_kind",
        "call",
        "same: no bytes, no `defmodule` arm",
    ),
    // The C# primary constructor is a `parameter_list` under a type
    // declaration (#1828): a parent gate the byte-less pair's known-empty
    // chain cannot see, so both answer the pre-#1828 `false` / `Unknown`
    // (`csharp_byte_less_spellings_omit_the_primary_constructor`).
    (
        "Csharp",
        "is_func_space",
        "parameter_list",
        "the `FuncOpener` row gates the list on a type-declaration parent; the known-empty chain has none",
    ),
    (
        "Csharp",
        "get_space_kind",
        "parameter_list",
        "same: the byte-less spelling forwards with no chain",
    ),
    // A Java record's header is a `formal_parameters` under a
    // `record_declaration` whose body writes no canonical constructor
    // (#1842), and a Groovy record's a `record_components`: a gate on
    // the parent and its body — and, to tell the canonical constructor
    // by its types, the bytes — that the known-empty chain declines
    // before reading either, so both byte-less spellings answer the
    // pre-#1842 `false` / `Unknown`
    // (`java_byte_less_spellings_omit_the_record_header`,
    // `groovy_byte_less_spellings_omit_the_record_header`).
    (
        "Java",
        "is_func_space",
        "formal_parameters",
        "the `FuncOpener` row gates the list on a record parent with no canonical constructor; the known-empty chain has none",
    ),
    (
        "Java",
        "get_space_kind",
        "formal_parameters",
        "same: the byte-less spelling forwards with no chain",
    ),
    (
        "Groovy",
        "is_func_space",
        "record_components",
        "the `FuncOpener` row gates the list on a record parent with no canonical constructor; the known-empty chain has none",
    ),
    (
        "Groovy",
        "get_space_kind",
        "record_components",
        "same: the byte-less spelling forwards with no chain",
    ),
    // The kind-only `is_string` / `is_call` against the gated row: the
    // Tcl family's braced word is a string only where it is not a
    // script body or a value slot, which the kind table cannot see
    // (`tcl_script_body_is_a_string_only_to_the_byteless_spelling`).
    (
        "Tcl",
        "is_string",
        "braced_word",
        "the byte-less spelling is the kind table; the row's gate reads the chain",
    ),
    ("Irules", "is_string", "braced_word", "as Tcl"),
    (
        "Perl",
        "is_call",
        "call_expression_with_bareword",
        "a bareword before `=>` or as a hash key is no call; the gate reads the chain (`perl_bareword_is_a_call_only_to_the_byteless_spelling`)",
    ),
];

/// What one source's forwards disagreed with their rows on: every
/// `(forward, kind)` named in `KNOWN_DISAGREEMENTS` that occurred, and
/// every disagreement it does not name, rendered with the source's
/// name. The one walk both forward tests make, so the fixture test
/// under the gate and the corpus sweep behind `make role-parity-corpus`
/// classify a disagreement the same way. Returns whether the source
/// parsed with error recovery.
fn forward_disagreements<C: Checker + crate::Getter + crate::LanguageInfo>(
    census: &Census,
    rows: &ForwardRows,
    source_name: &str,
    code: &[u8],
    asked: &mut usize,
    seen: &mut BTreeSet<(&'static str, &'static str, String)>,
    unnamed: &mut Vec<String>,
) -> bool {
    let name = census.name;
    let (_, recovered) =
        for_each_node_with_sibling_cache_tolerating_errors::<C>(code, |node, ancestors| {
            if node.start_byte() == node.end_byte() {
                return;
            }
            forwards_on::<C>(census, rows, node, code, ancestors, &mut |f| {
                *asked += 1;
                if f.answered == f.row {
                    return;
                }
                let kind = node.kind().to_owned();
                let named = KNOWN_DISAGREEMENTS
                    .iter()
                    .any(|(l, fwd, k, _)| *l == name && *fwd == f.name && *k == kind);
                if named {
                    seen.insert((name, f.name, kind));
                } else {
                    unnamed.push(format!(
                        "{name} {} on `{kind}` in {source_name}: forward {} row {}",
                        f.name, f.answered, f.row
                    ));
                }
            });
        });
    recovered
}

/// Every forward agrees with its row on every node, except where
/// `KNOWN_DISAGREEMENTS` names the `(language, forward, kind)`; and
/// every name there is still a disagreement somewhere in the corpus.
/// A second walk rather than a read of the census, on purpose: the
/// census records candidates, and a forward that admits a kind no row
/// lists is exactly the disagreement hunted here — that node is not a
/// candidate.
#[test]
fn every_checker_forward_agrees_with_its_row_or_is_named() {
    let mut unnamed: Vec<String> = Vec::new();
    let mut seen: BTreeSet<(&str, &str, String)> = BTreeSet::new();
    let mut asked = 0;
    let censuses = every_census();
    for census in &censuses {
        let rows = ForwardRows::of(census);
        for source in sources_for(census.lang) {
            with_code_tag!(census.lang, |C| {
                let recovered = forward_disagreements::<C>(
                    census,
                    &rows,
                    &source.name,
                    &source.bytes,
                    &mut asked,
                    &mut seen,
                    &mut unnamed,
                );
                assert!(!recovered, "{}: a fixture parses clean", source.name);
            });
        }
    }
    assert!(asked > 0);
    let stale: Vec<String> = KNOWN_DISAGREEMENTS
        .iter()
        .filter(|(l, _, _, _)| censuses.iter().any(|c| c.name == *l))
        .filter(|(l, f, k, _)| !seen.contains(&(*l, *f, (*k).to_owned())))
        .map(|(l, f, k, _)| format!("{l} {f} on `{k}`"))
        .collect();
    assert!(
        unnamed.is_empty(),
        "forwards that disagree with their row on the real context, unnamed:\n{}",
        unnamed.join("\n")
    );
    if control_flow_corpus_present() {
        assert!(
            stale.is_empty(),
            "KNOWN_DISAGREEMENTS entries the corpus no longer shows:\n{}",
            stale.join("\n")
        );
    }
}

/// The stale direction of `KNOWN_DISAGREEMENTS` on its own: every
/// entry for an enabled language is met somewhere in the full corpus.
/// Asserted inside the forward test too when the corpus is present;
/// here as a `corpus_` test so a leg without the submodule shows the
/// obligation as filtered rather than passed.
#[test]
fn corpus_every_known_disagreement_is_still_met() {
    assert!(
        control_flow_corpus_present(),
        "control_flow/ not checked out; run `make worktree-setup` — or filter `corpus_` tests"
    );
    let mut asked = 0;
    let mut seen: BTreeSet<(&str, &str, String)> = BTreeSet::new();
    let mut unnamed: Vec<String> = Vec::new();
    let censuses = every_census();
    for census in &censuses {
        let rows = ForwardRows::of(census);
        for source in sources_for(census.lang) {
            with_code_tag!(census.lang, |C| {
                forward_disagreements::<C>(
                    census,
                    &rows,
                    &source.name,
                    &source.bytes,
                    &mut asked,
                    &mut seen,
                    &mut unnamed,
                );
            });
        }
    }
    let stale: Vec<String> = KNOWN_DISAGREEMENTS
        .iter()
        .filter(|(l, _, _, _)| censuses.iter().any(|c| c.name == *l))
        .filter(|(l, f, k, _)| !seen.contains(&(*l, *f, (*k).to_owned())))
        .map(|(l, f, k, _)| format!("{l} {f} on `{k}`"))
        .collect();
    assert!(
        stale.is_empty(),
        "KNOWN_DISAGREEMENTS entries the corpus no longer shows:\n{}",
        stale.join("\n")
    );
}

/// What the chain backstop found across every probed table: the
/// per-gate findings, deduplicated by `(role, kind)` since one climbing
/// gate answers on every candidate of its kind.
#[derive(Default)]
struct ChainProbe {
    asked: usize,
    disagreements: BTreeSet<String>,
    climbs: BTreeSet<String>,
}

impl ChainProbe {
    /// Asks every unchained row that owns `node`'s kind under the real
    /// chain and the empty one, then the `opens_space` fold where no
    /// chain-gated opener owns the kind.
    fn probe<'a>(
        &mut self,
        table: &LanguageTable,
        rows: &[RowView],
        unchained: &[DeclaredRow],
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) {
        let empty = Ancestors::known(&[]);
        let kind = node.kind_id();
        let mut owner_of_unchained = false;
        let climbed = |p: usize, s: usize| {
            (
                crate::node::node_resolved_parent_lookups::observed() - p,
                crate::node::node_resolved_sibling_lookups::observed() - s,
            )
        };
        for row in unchained {
            if !row.owners.contains(&kind) {
                continue;
            }
            owner_of_unchained = true;
            self.asked += 1;
            let at = format!("{} {} on `{}`", table.name, row.role, node.kind());
            let (p, s) = climbed(0, 0);
            let real = (row.owns_kind)(kind, node, code, ancestors);
            let rootless = (row.owns_kind)(kind, node, code, empty);
            let (parents, siblings) = climbed(p, s);
            if (parents, siblings) != (0, 0) {
                self.climbs.insert(format!(
                    "{at}: {parents} parent and {siblings} sibling climb(s)"
                ));
            }
            if real != rootless {
                self.disagreements.insert(format!(
                    "{at}: owns {real} with the chain, {rootless} without"
                ));
            }
        }
        // The fold reads every row's `opens` / `opens_by`, so it is
        // compared only where every space-opening row of the node's
        // kind is unchained: Ruby's `block` is a chain-gated opener and
        // disagrees by design
        // (`ruby_byte_less_spellings_stay_the_kind_table`).
        let chained_opener = rows.iter().filter_map(RowView::declared).any(|r| {
            r.chain_gated
                && matches!(r.role, "FuncOpener" | "Container")
                && r.owners.contains(&kind)
        });
        if owner_of_unchained && !chained_opener {
            let (p, s) = climbed(0, 0);
            let real = (table.opens_space)(node, code, ancestors);
            let rootless = (table.opens_space)(node, code, empty);
            let (parents, siblings) = climbed(p, s);
            if (parents, siblings) != (0, 0) {
                self.climbs.insert(format!(
                    "{} opens_space on `{}`: {parents} parent and {siblings} sibling climb(s)",
                    table.name,
                    node.kind()
                ));
            }
            if real != rootless {
                self.disagreements.insert(format!(
                    "{} opens_space on `{}`: {real:?} with the chain, {rootless:?} without",
                    table.name,
                    node.kind()
                ));
            }
        }
    }
}

/// The behavioural backstop for `no_chain` (#1793): on every row whose
/// `CHAIN_GATED` is false, every owner candidate of the corpus answers
/// `owns_kind` the same under the walk's real chain and under
/// `Ancestors::known(&[])` — the chain that says the node is the root
/// — and so does the table's `opens_space` fold, which `owns_kind`
/// never reaches for an `opens_by` owner. `CHAIN_GATED = false` proves
/// at build time that no such gate *takes* a chain; it cannot see one
/// that climbs with `Node::parent` from its two arguments. The
/// comparison sees a climb whose answer differs between the two
/// chains; a climb whose answer agrees on every candidate (lesson 114)
/// — `parent().is_some()`, say, true under both — is what the two
/// climb counters see: `node_resolved_parent_lookups` and
/// `node_resolved_sibling_lookups` must not move while the unchained
/// rows are asked, so any `Node::parent` / `Node::previous_sibling`
/// a marked gate reaches is reported whatever it answers. What neither
/// half sees is a climb through some other tree walk the counters do
/// not cover, which stays a review item on a marked gate. Never
/// `Ancestors::unknown()`, which rebuilds the parent and answers
/// exactly through the climb it exists to price (lesson 114).
///
/// Verified to discriminate (recorded on #1793): a `no_chain` gate
/// reading `node.parent().is_some()` — true under every chain — fails
/// the counter half. The comparison half can only see a gate whose
/// answer *depends on which chain it was handed*; a climb through
/// `Node::parent` reads the same tree under either chain and so never
/// moves it, which is why the counters bracket both `owns_kind` and
/// the `opens_space` fold (an `opens_by … no_chain` fn is reached only
/// by the latter).
#[test]
fn no_chain_rows_answer_alike_under_the_empty_chain() {
    let mut probe = ChainProbe::default();
    let mut rows_asked = 0;
    for table in crate::roles::tables() {
        if !table.lang.is_enabled() {
            continue;
        }
        let rows = (table.rows)();
        let unchained: Vec<DeclaredRow> = rows
            .iter()
            .filter_map(RowView::declared)
            .filter(|r| !r.chain_gated)
            .copied()
            .collect();
        rows_asked += unchained.len();
        for source in sources_for(table.lang) {
            let code = source.bytes.as_slice();
            with_code_tag!(table.lang, |C| {
                for_each_node_with_sibling_cache::<C>(code, |node, ancestors| {
                    if node.start_byte() != node.end_byte() {
                        probe.probe(table, &rows, &unchained, node, code, ancestors);
                    }
                });
            });
        }
    }
    assert!(
        rows_asked > 0,
        "no `!CHAIN_GATED` row in any enabled language"
    );
    assert!(
        probe.asked > 0,
        "no owner candidate of a `!CHAIN_GATED` row in the corpus"
    );
    assert!(
        probe.disagreements.is_empty(),
        "a `!CHAIN_GATED` row answers differently without its chain — a `no_chain` gate climbs:\n{}",
        probe
            .disagreements
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        probe.climbs.is_empty(),
        "a `!CHAIN_GATED` row climbed with `Node::parent` / `Node::previous_sibling` while asked:\n{}",
        probe.climbs.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// What the byte backstop found across every probed table: the
/// per-gate findings, deduplicated by `(role, kind)` since one
/// byte-reading gate answers on every candidate of its kind.
#[derive(Default)]
struct ByteProbe {
    asked: usize,
    disagreements: BTreeSet<String>,
}

impl ByteProbe {
    /// Asks every row that owns `node`'s kind through an owner it does
    /// not mark `text_gated_by`, with the bytes and with none, then the
    /// `opens_space` fold where no marked opener owns the kind.
    fn probe<'a>(
        &mut self,
        table: &LanguageTable,
        rows: &[DeclaredRow],
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) {
        // One chain for both asks, and no `FoldCache` on it: a memo is
        // keyed by node, never by buffer, so through the walk's cache
        // the byte-less ask would be served what the real bytes stored
        // a moment before, and a memoised byte reader would agree with
        // itself. The `SiblingCache` stays: a gate that reads a sibling
        // under a wide parent needs it, as on the walk.
        let ancestors = ancestors.without_folds();
        let kind = node.kind_id();
        let mut unmarked_owner = false;
        for row in rows {
            if !row.owners.contains(&kind) || row.text_gated_owners.contains(&kind) {
                continue;
            }
            unmarked_owner = true;
            self.asked += 1;
            let real = (row.owns_kind)(kind, node, code, ancestors);
            let byteless = (row.owns_kind)(kind, node, &[], ancestors);
            if real != byteless {
                self.disagreements.insert(format!(
                    "{} {} on `{}`: owns {real} with the bytes, {byteless} without",
                    table.name,
                    row.role,
                    node.kind()
                ));
            }
        }
        // The fold reads every row's `opens` / `opens_by`, so it is
        // compared only where no space-opening row marks the node's
        // kind: a marked opener reads the bytes by declaration.
        let marked_opener = rows.iter().any(|r| {
            matches!(r.role, "FuncOpener" | "Container") && r.text_gated_owners.contains(&kind)
        });
        if unmarked_owner && !marked_opener {
            let real = (table.opens_space)(node, code, ancestors);
            let byteless = (table.opens_space)(node, &[], ancestors);
            if real != byteless {
                self.disagreements.insert(format!(
                    "{} opens_space on `{}`: {real:?} with the bytes, {byteless:?} without",
                    table.name,
                    node.kind()
                ));
            }
        }
    }
}

/// The behavioural backstop for `TEXT_GATED` (#1863): every owner a row
/// does not mark `text_gated_by` answers `owns_kind` the same with the
/// walk's bytes and with `&[]`, on every candidate of the corpus, and
/// so does the table's `opens_space` fold, which `owns_kind` never
/// reaches for an `opens_by` owner. The flag is the author's word — the
/// keyword is the declaration, where `CHAIN_GATED` is held to the fn's
/// signature — so a gate that reads bytes under a plain `gated_by`
/// builds, passes every `!TEXT_GATED` assertion a byte-less forward
/// makes, and answers that forward from an empty slice.
/// `no_chain_rows_answer_alike_under_the_empty_chain` is the same
/// backstop on the chain axis.
///
/// Per owner, not per row (`TEXT_GATED_OWNERS`): a row one owner marks
/// is still asked through its others. What it sees is an owner whose
/// answer *moves* without the bytes on some candidate the corpus
/// spells; a byte reader no fixture can move — a gate comparing two
/// spellings that happen to agree when both are empty — is not seen,
/// nor an owner kind with no occurrence (`make role-coverage-claimed`).
/// It asks ownership and the space fold, which is what `TEXT_GATED`
/// claims: a `callable_when`, a `pays_itself_when` and a payer's gate
/// are outside the flag and outside this.
///
/// Verified to discriminate by re-keywording three gates to a plain
/// `gated_by` (recorded on #1863): Java's `opens_record_header`, whose
/// answer moves on the same-arity alternative constructor of
/// `gate/java-records`; Bash's `comparison_operator`, on the `-a` of
/// `gate/bash-tests`; and Elixir's `control_branch`, whose target read
/// the walk memoises per visited node.
#[test]
fn unmarked_owners_answer_alike_without_the_bytes() {
    let mut probe = ByteProbe::default();
    for table in crate::roles::tables() {
        if !table.lang.is_enabled() {
            continue;
        }
        let rows: Vec<DeclaredRow> = (table.rows)()
            .iter()
            .filter_map(RowView::declared)
            .copied()
            .collect();
        for source in sources_for(table.lang) {
            let code = source.bytes.as_slice();
            with_code_tag!(table.lang, |C| {
                for_each_node_with_sibling_cache::<C>(code, |node, ancestors| {
                    if node.start_byte() != node.end_byte() {
                        probe.probe(table, &rows, node, code, ancestors);
                    }
                });
            });
        }
    }
    assert!(
        probe.asked > 0,
        "no candidate of an owner without a text gate in the corpus"
    );
    assert!(
        probe.disagreements.is_empty(),
        "an owner not marked `text_gated_by` answers differently without the bytes — \
         its gate reads them under the wrong keyword:\n{}",
        probe
            .disagreements
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The `Closure` rows read no bytes in any language, which is what
/// lets every `Checker::is_closure` forward `&[]` — the pin the 19
/// forwards lack at build time, held here for every table at once.
#[test]
fn no_closure_row_reads_bytes() {
    for table in crate::roles::tables() {
        for row in (table.rows)() {
            if row.role() == "Closure"
                && let Some(r) = row.declared()
            {
                assert!(!r.text_gated, "{}: `is_closure` forwards `&[]`", table.lang);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R — relations between rows, declared once and asked of every table
// ---------------------------------------------------------------------------

/// One relation between rows of a table, as a predicate over a node's
/// verdicts: `left` holds only where `right` does, on the declared
/// lists and on every candidate of the corpus. A relation that needs a
/// per-kind exception names it in its own right-hand side, as the
/// `DocTarget` one reads `doc_tests/parity.rs`'s table; none carries
/// an exception list of its own.
struct Relation {
    name: &'static str,
    /// The row whose owners the relation constrains.
    left: &'static str,
    /// Whether `kind` of `lang` satisfies the right-hand side on the
    /// declared lists (gates aside).
    declared_right: fn(&Census, u16) -> bool,
    /// Whether a candidate node satisfies the right-hand side with the
    /// real context.
    gated_right: for<'a> fn(&Census, &Node<'a>, &[u8], Ancestors<'a, '_>) -> bool,
}

fn owns_any<'a>(
    census: &Census,
    roles: &[&str],
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let kind = node.kind_id();
    roles.iter().any(|role| {
        census
            .row_named(role)
            .is_some_and(|r| (r.owns_kind)(kind, node, code, ancestors))
    })
}

fn lists_any(census: &Census, roles: &[&str], kind: u16) -> bool {
    roles
        .iter()
        .any(|role| census.row_named(role).is_some_and(|r| (r.lists_kind)(kind)))
}

/// `ElseIf ⊆ Branch`, except for the *dedicated* clause kinds — an
/// `elif_clause`, `elsif`, `elseif_statement` — which no `Branch` row
/// owns because the chained `if` they stand for has no nested `if` to
/// inspect. Derived, not inferred from `pays_itself`: Python's
/// `elif_clause` pays through a separate `elif` keyword and is still
/// dedicated. Every such kind is named per language.
const ELSE_IF_DEDICATED: &[(&str, &str, &str)] = &[
    (
        "Bash",
        "elif_clause",
        "the `elif` clause is the chain link; no nested `if`",
    ),
    ("Irules", "elseif", "the `elseif` word is the chain link"),
    (
        "Lua",
        "elseif_statement",
        "the `elseif` statement is the chain link",
    ),
    (
        "Perl",
        "elsif_clause",
        "the `elsif` clause is the chain link",
    ),
    (
        "Php",
        "else_if_clause",
        "the one-word `elseif` clause, in its brace and `:` … `endif` aliases; the two-word `else if` is an `if_statement` the `Branch` row also owns",
    ),
    (
        "Python",
        "elif_clause",
        "the `elif` clause is the chain link, paid at its keyword",
    ),
    ("Ruby", "elsif", "the `elsif` clause is the chain link"),
    ("Tcl", "elseif", "the `elseif` word is the chain link"),
];

/// `DocTarget ⊆ FuncOpener ∪ Container ∪ Member`: the targets a doc
/// comment attaches to that open no space and are no member. Each is
/// named with why in `doc_tests/parity.rs`'s table, which this reads
/// through that test's own list so the two cannot drift; here only
/// the gated-node half is asked, on every candidate rather than on the
/// eight fixtures that test chose.
fn doc_target_outside_space_tree_is_named(census: &Census, kind: u16) -> bool {
    lists_any(census, &["FuncOpener", "Container", "Member"], kind)
        || census.targets_beyond_space_tree.contains(&kind)
}

const RELATIONS: &[Relation] = &[
    Relation {
        name: "ElseIf ⊆ Branch ∪ dedicated clause",
        left: "ElseIf",
        declared_right: |census, kind| {
            lists_any(census, &["Branch"], kind)
                || ELSE_IF_DEDICATED
                    .iter()
                    .any(|(l, k, _)| *l == census.name && census.kind_name(kind) == *k)
        },
        gated_right: |census, node, code, ancestors| {
            owns_any(census, &["Branch"], node, code, ancestors)
                || ELSE_IF_DEDICATED
                    .iter()
                    .any(|(l, k, _)| *l == census.name && node.kind() == *k)
        },
    },
    Relation {
        name: "Closure ∩ Container = ∅",
        left: "Closure",
        declared_right: |census, kind| !lists_any(census, &["Container"], kind),
        gated_right: |census, node, code, ancestors| {
            !owns_any(census, &["Container"], node, code, ancestors)
        },
    },
    // `DocComment ⇒ Comment ∪ DocComment` and `Docstring` outside both
    // are not relations between rows: a `DocComment` owner is in the
    // union of the two rows' lists by definition. What can drift is the
    // *Checker* — `is_doc_comment ⇒ is_comment`, `Docstring` reaching
    // neither — and that is a forward, asked in `forwards_on` with the
    // tag in hand (`is_doc_comment ⇒ is_comment`, `docstring`).
    Relation {
        name: "DocTarget ⊆ FuncOpener ∪ Container ∪ Member ∪ named",
        left: "DocTarget",
        declared_right: doc_target_outside_space_tree_is_named,
        gated_right: |census, node, code, ancestors| {
            owns_any(
                census,
                &["FuncOpener", "Container", "Member"],
                node,
                code,
                ancestors,
            ) || census.targets_beyond_space_tree.contains(&node.kind_id())
                || super::doc_tests::parity::gated_out_of_the_space_tree(census.name)
                    .iter()
                    .any(|(k, _)| *k == node.kind())
        },
    },
];

/// The exact gated-out shapes, in both directions: every `DocTarget`
/// owner the space tree lists by kind and rejects by gate on the
/// corpus is one `(kind, first line)` the provider names, and every
/// name is met — so an expression-bodied property the gate newly
/// rejects fails here by its text, and a bodied one the gate newly
/// admits fails as a stale name. The identity the retired
/// `doc_targets_the_space_tree_rejects_by_gate_are_each_named` pinned.
#[test]
fn doc_targets_the_space_tree_rejects_by_gate_are_exactly_the_named_shapes() {
    let censuses = every_census();
    let mut failures = Vec::new();
    for census in &censuses {
        let Some(target) = census.row_named("DocTarget") else {
            continue;
        };
        let listed: BTreeSet<u16> = ["FuncOpener", "Container", "Member"]
            .iter()
            .filter_map(|r| census.row_named(r))
            .flat_map(|r| r.owners.iter().copied())
            .collect();
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        for source in sources_for(census.lang) {
            let code = source.bytes.as_slice();
            with_code_tag!(census.lang, |C| {
                for_each_node_with_sibling_cache::<C>(code, |node, ancestors| {
                    let kind = node.kind_id();
                    if !(target.owns_kind)(kind, node, code, ancestors) || !listed.contains(&kind) {
                        return;
                    }
                    if owns_any(
                        census,
                        &["FuncOpener", "Container", "Member"],
                        node,
                        code,
                        ancestors,
                    ) {
                        return;
                    }
                    let text = node
                        .utf8_text(code)
                        .unwrap_or("")
                        .lines()
                        .next()
                        .unwrap_or("");
                    seen.insert((node.kind().to_owned(), text.to_owned()));
                });
            });
        }
        let want: BTreeSet<(String, String)> =
            super::doc_tests::parity::gated_out_of_the_space_tree(census.name)
                .iter()
                .map(|(k, t)| ((*k).to_owned(), (*t).to_owned()))
                .collect();
        if seen != want {
            failures.push(format!(
                "{}: targets the space tree lists but rejects by gate\n  seen: {seen:?}\n  named: {want:?}",
                census.name
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every relation holds on every table's declared lists and on every
/// candidate of the corpus; and for every language whose left row is
/// declared with owners, the corpus spells at least one positive — per
/// language, so a relation cannot hold vacuously on one table behind
/// another's positives. A language whose left row is `absent` is
/// skipped with its reason printed; one the corpus never spells is a
/// named gap in `RELATION_NO_POSITIVE_IN_CORPUS`, pinned both ways —
/// an entry naming a row no relation has, or a language whose row is
/// absent, is stale too.
#[test]
fn every_row_relation_holds_on_the_lists_and_the_corpus() {
    let censuses = every_census();
    let mut gaps_used: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut vacuous = Vec::new();
    for relation in RELATIONS {
        let mut failures = Vec::new();
        for census in &censuses {
            let Some(left) = census.row_named(relation.left) else {
                if let Some(RowView::Absent { why, .. }) =
                    census.rows.iter().find(|r| r.role() == relation.left)
                {
                    println!(
                        "[{}] {}: {} absent — {why}",
                        relation.name, census.name, relation.left
                    );
                }
                continue;
            };
            let mut positives = 0;
            for &kind in left.owners {
                if !(relation.declared_right)(census, kind) {
                    failures.push(format!(
                        "{} [{}]: declared kind `{}` ({kind}) breaks the relation",
                        census.name,
                        relation.name,
                        census.kind_name(kind)
                    ));
                }
            }
            let sources = sources_for(census.lang);
            for source in &sources {
                let code = source.bytes.as_slice();
                with_code_tag!(census.lang, |C| {
                    for_each_node_with_sibling_cache::<C>(code, |node, ancestors| {
                        if !(left.owns_kind)(node.kind_id(), node, code, ancestors) {
                            return;
                        }
                        positives += 1;
                        if !(relation.gated_right)(census, node, code, ancestors) {
                            failures.push(format!(
                                "{} [{}]: `{}` in {} breaks the relation",
                                census.name,
                                relation.name,
                                node.kind(),
                                source.name
                            ));
                        }
                    });
                });
            }
            let gap_named = RELATION_NO_POSITIVE_IN_CORPUS
                .iter()
                .any(|(l, r, _)| *l == census.name && *r == relation.left);
            if gap_named {
                gaps_used.insert((census.name, relation.left));
            }
            match (positives > 0, gap_named) {
                (false, false) if !left.owners.is_empty() => vacuous.push(format!(
                    "{} [{}]: {} declares owners and the corpus spells none that own; vacuous",
                    census.name, relation.name, relation.left
                )),
                (true, true) => failures.push(format!(
                    "{} [{}]: named as having no positive, but has {positives}",
                    census.name, relation.name
                )),
                _ => {}
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
    // The stale direction of the gap list: an entry for an enabled
    // language was consulted by some relation — so it names a left row
    // some relation has, declared with owners on that table.
    let stale: Vec<String> = RELATION_NO_POSITIVE_IN_CORPUS
        .iter()
        .filter(|(l, _, _)| censuses.iter().any(|c| c.name == *l))
        .filter(|(l, r, _)| !gaps_used.contains(&(*l, *r)))
        .map(|(l, r, why)| {
            format!("{l} {r} ({why}): no relation has that left row declared with owners here")
        })
        .collect();
    assert!(
        stale.is_empty(),
        "stale RELATION_NO_POSITIVE_IN_CORPUS entries:\n{}",
        stale.join("\n")
    );
    if control_flow_corpus_present() {
        assert!(vacuous.is_empty(), "{}", vacuous.join("\n"));
    }
}

/// The per-language non-vacuity of every relation — a declared left
/// row the full corpus spells at least one positive of, or a named gap
/// — as a `corpus_` test, so a leg without the submodule shows the
/// obligation as filtered rather than passed. The relation test above
/// asserts it too when the corpus is present.
#[test]
fn corpus_every_relation_has_a_positive_per_language_or_a_named_gap() {
    assert!(
        control_flow_corpus_present(),
        "control_flow/ not checked out; run `make worktree-setup` — or filter `corpus_` tests"
    );
    every_row_relation_holds_on_the_lists_and_the_corpus();
}

/// `(language, left row)` pairs whose row declares owners the corpus
/// never spells as a positive, so the relation on that table is asked
/// of nothing: a gap the fixtures leave, named rather than passed over,
/// and pinned both ways.
const RELATION_NO_POSITIVE_IN_CORPUS: &[(&str, &str, &str)] = &[
    (
        "Csharp",
        "Closure",
        "no lambda or anonymous method in the fixtures",
    ),
    (
        "Javascript",
        "Closure",
        "the one arrow is bound, the gate's negative",
    ),
    (
        "Mozjs",
        "Closure",
        "the one arrow is bound, the gate's negative",
    ),
    ("Objc", "Closure", "no block literal in the fixtures"),
    ("Perl", "Closure", "no anonymous `sub` in the fixtures"),
    ("Python", "Closure", "no `lambda` in the fixtures"),
];

/// `doc_tests/parity.rs`'s table of targets beyond the space tree is
/// exact in the stale direction too: every named kind is still a
/// `DocTarget` owner and still outside the space tree. The relation
/// above subtracts the names; this is what keeps a name from outliving
/// the row entry it explains.
#[test]
fn every_named_target_beyond_the_space_tree_is_still_one() {
    let mut failures = Vec::new();
    for table in super::doc_tests::parity::doc_target_tables() {
        let owned: std::collections::BTreeSet<u16> =
            table.space.iter().copied().flatten().copied().collect();
        for (kind, why) in &table.beyond {
            if !table.doc_target.contains(kind) {
                failures.push(format!(
                    "{}: named kind {kind} ({why}) is no longer a target",
                    table.lang
                ));
            }
            if owned.contains(kind) {
                failures.push(format!(
                    "{}: named kind {kind} ({why}) is in the space tree; the name is stale",
                    table.lang
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `ELSE_IF_DEDICATED` is exact: every entry is an `ElseIf` owner the
/// `Branch` row does not list, and every such owner of an enabled
/// language is an entry.
#[test]
fn the_dedicated_else_if_clauses_are_exactly_the_owners_branch_does_not_list() {
    let censuses = every_census();
    let mut dedicated_failures = Vec::new();
    for census in &censuses {
        let (Some(else_if), branch) = (census.row_named("ElseIf"), census.row_named("Branch"))
        else {
            continue;
        };
        for &kind in else_if.owners {
            let in_branch = branch.is_some_and(|b| b.owners.contains(&kind));
            let name = census.kind_name(kind);
            let named = ELSE_IF_DEDICATED
                .iter()
                .any(|(l, k, _)| *l == census.name && *k == name);
            if in_branch == named {
                dedicated_failures.push(format!(
                    "{}: ElseIf owner `{name}` ({kind}) is {} and {}",
                    census.name,
                    if in_branch {
                        "a Branch owner"
                    } else {
                        "no Branch owner"
                    },
                    if named {
                        "named dedicated"
                    } else {
                        "not named dedicated"
                    }
                ));
            }
        }
    }
    // The stale direction: every entry for an enabled language names a
    // kind its `ElseIf` row still owns and its `Branch` row still does
    // not — an entry outliving its owner, or spelling a kind the
    // grammar lacks, fails here rather than being skipped above.
    for (lang, kind_name, why) in ELSE_IF_DEDICATED {
        let Some(census) = censuses.iter().find(|c| c.name == *lang) else {
            continue;
        };
        let owned_by = |role: &str| {
            census
                .row_named(role)
                .is_some_and(|r| r.owners.iter().any(|&k| census.kind_name(k) == *kind_name))
        };
        if !owned_by("ElseIf") || owned_by("Branch") {
            dedicated_failures.push(format!(
                "{lang}: ELSE_IF_DEDICATED names `{kind_name}` ({why}), which ElseIf {} and Branch {}",
                if owned_by("ElseIf") { "owns" } else { "does not own" },
                if owned_by("Branch") { "owns" } else { "does not own" }
            ));
        }
    }
    assert!(
        dedicated_failures.is_empty(),
        "{}",
        dedicated_failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// The integration corpora — opt-in, `make role-parity-corpus`
// ---------------------------------------------------------------------------

/// The same forward comparison over every tracked file of the three
/// repository corpora (`DeepSpeech`, `pdf.js`, `serde`) a language's
/// extensions route to, with the tolerant walk: a file that recovers into an
/// `ERROR` is walked and labelled, and its disagreements are reported
/// under that label rather than failed — error recovery is parity on a
/// tree the grammar did not mean, not a language contract
/// (grammar-dispatch §6). Ignored by default: a thousand C++ files are
/// not a unit test's budget; `make role-parity-corpus` runs it, and
/// `make role-corpus-refresh` runs it with the observation.
#[test]
#[ignore = "walks the integration corpora; run through `make role-parity-corpus`"]
fn every_checker_forward_agrees_with_its_row_across_the_integration_corpora() {
    let repos = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the ast crate sits under the workspace root")
        .join("tests")
        .join("repositories");
    let censuses: Vec<(Census, ForwardRows)> = every_census()
        .into_iter()
        .map(|c| {
            let rows = ForwardRows::of(&c);
            (c, rows)
        })
        .collect();
    let mut files = 0;
    let mut present: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_corpus: BTreeMap<&str, usize> = BTreeMap::new();
    let mut recovery_files = 0;
    let mut asked = 0;
    let mut seen: BTreeSet<(&str, &str, String)> = BTreeSet::new();
    let mut unnamed: BTreeSet<String> = BTreeSet::new();
    let mut recovery_only: BTreeSet<String> = BTreeSet::new();
    for corpus in INTEGRATION_CORPORA {
        let root = repos.join(corpus);
        assert!(
            root.is_dir(),
            "{} missing; run `make worktree-setup`",
            root.display()
        );
        let mut paths: Vec<std::path::PathBuf> = files_under(&root);
        paths.sort();
        present.insert(corpus, paths.len());
        for path in paths {
            let Some(lang) = crate::tools::get_language_for_file(&path) else {
                continue;
            };
            let Some((census, rows)) = censuses.iter().find(|(c, _)| c.lang == lang) else {
                continue;
            };
            // `Ok(None)` is the reader's deliberate skip — a binary, empty,
            // too-short or UTF-16 file (`SkipReason`); an error is a corpus
            // the sweep cannot claim to have walked.
            let code = match crate::tools::read_file_with_eol(&path) {
                Ok(Some(code)) => code,
                Ok(None) => continue,
                Err(e) => panic!("{}: unreadable corpus file: {e}", path.display()),
            };
            files += 1;
            *per_corpus.entry(corpus).or_insert(0) += 1;
            let name = format!(
                "{corpus}/{}",
                path.strip_prefix(&root).expect("under root").display()
            );
            let mut pending = Vec::new();
            let recovered = with_code_tag!(lang, |C| {
                forward_disagreements::<C>(
                    census,
                    rows,
                    &name,
                    &code,
                    &mut asked,
                    &mut seen,
                    &mut pending,
                )
            });
            if recovered {
                recovery_only.extend(pending);
            } else {
                unnamed.extend(pending);
            }
            recovery_files += usize::from(recovered);
        }
    }
    println!("integration corpora: {files} files walked, {recovery_files} with error recovery");
    for line in &recovery_only {
        println!("recovery-only disagreement: {line}");
    }
    assert!(files > 0, "no corpus file routed to an enabled language");
    assert!(asked > 0);
    println!("named disagreements met in the corpora: {}", seen.len());
    assert_every_corpus_present(&present, &per_corpus);
    assert!(
        unnamed.is_empty(),
        "forwards that disagree with their row on a clean-parsing corpus file, unnamed:\n{}",
        unnamed.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// The three repository corpora the sweep walks, as checked out under
/// `tests/repositories/`. The fourth corpus `check-role-coverage.py`
/// samples — the output submodule's `control_flow/` — is the default
/// tests' corpus (`control_flow_sources`), so it is not walked twice;
/// a corpus added there is added here or to the routing, not both.
const INTEGRATION_CORPORA: [&str; 3] = ["DeepSpeech", "pdf.js", "serde"];

/// Each corpus holds files, or the sweep walked a partial checkout:
/// the interrupted-checkout state #1171 describes leaves the directory
/// present and empty, and that must not read as a walked corpus. What
/// each corpus *routed* is a different question — a single-language
/// build routes nothing from a corpus in another language, which is
/// honest — so it is reported, not asserted.
fn assert_every_corpus_present(present: &BTreeMap<&str, usize>, routed: &BTreeMap<&str, usize>) {
    for corpus in INTEGRATION_CORPORA {
        let files = present.get(corpus).copied().unwrap_or(0);
        assert!(
            files > 0,
            "{corpus}: no file under it; run `make worktree-setup` (an interrupted checkout, #1171)"
        );
        println!(
            "{corpus}: {files} files, {} routed to an enabled language",
            routed.get(corpus).copied().unwrap_or(0)
        );
    }
}

/// Every regular file `root`'s git index tracks: the corpus its
/// recorded submodule commit describes, however deep the checkout
/// (#1836). Read from the index rather than the directory, so a nested
/// submodule is one `160000` gitlink and never its contents, an
/// untracked file is invisible, and a symlink is one `120000` entry
/// with nothing listed under it — dropped, as `check-role-coverage.py`
/// drops it, so the two list a corpus's files the same way. A `root`
/// git does not resolve to its own repository (an uninitialised
/// submodule, where git answers for the parent checkout), a failed
/// `git`, or a tracked file missing from disk is a failure, not a
/// smaller walk: a sweep that walked less than it claims must say so.
fn files_under(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let toplevel = git_output(root, &["rev-parse", "--show-toplevel"]);
    let toplevel = path_from_git(toplevel.strip_suffix(b"\n").unwrap_or(&toplevel));
    let canonical = |p: &std::path::Path| {
        std::fs::canonicalize(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    };
    assert_eq!(
        canonical(&toplevel),
        canonical(root),
        "{}: not the root of its own git repository; run `make worktree-setup`",
        root.display()
    );
    let listing = git_output(root, &["ls-files", "-s", "-z"]);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for record in listing.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        // `<mode> <object> <stage>\t<path>`; a conflicted path is listed
        // once per merge stage.
        let tab = record
            .iter()
            .position(|&b| b == b'\t')
            .unwrap_or_else(|| panic!("{}: malformed `git ls-files` record", root.display()));
        let (meta, name) = (&record[..tab], &record[tab + 1..]);
        let mode = meta
            .split(|&b| b == b' ')
            .next()
            .expect("`split` yields at least one field");
        if !SAMPLED_MODES.contains(&mode) || !seen.insert(name) {
            continue;
        }
        let path = root.join(path_from_git(name));
        // `is_file` answers false for a file it cannot reach, and would
        // call an unreadable checkout a missing one.
        let on_disk = match std::fs::metadata(&path) {
            Ok(meta) => meta.is_file(),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                false
            }
            Err(e) => panic!("{}: tracked but unreadable: {e}", path.display()),
        };
        assert!(
            on_disk,
            "{}: tracked but not on disk; {MISSING_FILE_REMEDY}",
            path.display()
        );
        out.push(path);
    }
    out
}

/// The index modes `files_under` lists, as `check-role-coverage.py`'s
/// `SAMPLED_MODES`: a regular and an executable file. A `120000`
/// symlink would list a file twice under two names, and a `160000`
/// gitlink is a nested submodule, whose files belong to a commit the
/// corpus's own does not record.
const SAMPLED_MODES: [&[u8]; 2] = [b"100644", b"100755"];

/// What repairs a tracked file missing from disk. `make worktree-setup`
/// restores an interrupted checkout (#1171) — and, forcing it, a file
/// deleted on purpose, which only `git rm` keeps deleted — but `git diff
/// HEAD` hides a sparse or skip-worktree entry, so it finds those corpora
/// complete and leaves them alone.
const MISSING_FILE_REMEDY: &str = "run `make worktree-setup` (a sparse or skip-worktree corpus \
     needs `git sparse-checkout disable` or `git update-index --no-skip-worktree`, and a file \
     deleted on purpose `git rm`)";

/// The variables through which git finds a repository, or its object
/// store, other than the one `-C` names, as `check-role-coverage.py`'s
/// `REPOSITORY_ENV`. Git exports `GIT_DIR` and `GIT_INDEX_FILE` to
/// hooks, and `GIT_DIR` to `git rebase --exec` in a linked worktree;
/// inherited, they outrank `-C`, so `--show-toplevel` names the corpus
/// while `ls-files` lists the parent checkout's index.
/// `GIT_OBJECT_DIRECTORY`, which a `receive-pack` quarantine hook
/// exports, fails discovery outright once the quarantine it names is
/// gone, and an alternate is consulted before the corpus's own store.
const REPOSITORY_ENV: [&str; 6] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

/// `git -C dir <args>`'s standard output, or a panic naming the corpus.
fn git_output(dir: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let mut cmd = std::process::Command::new("git");
    for var in REPOSITORY_ENV {
        cmd.env_remove(var);
    }
    let out = cmd
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{}: cannot run git: {e}", dir.display()));
    assert!(
        out.status.success(),
        "{}: `git {}` failed: {}",
        dir.display(),
        args.join(" "),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    out.stdout
}

/// A path as git prints it under `-z`: unquoted bytes, which are the
/// OS path on unix and UTF-8 everywhere else.
#[cfg(unix)]
fn path_from_git(bytes: &[u8]) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes).into()
}

#[cfg(not(unix))]
fn path_from_git(bytes: &[u8]) -> std::path::PathBuf {
    std::str::from_utf8(bytes)
        .unwrap_or_else(|e| panic!("git printed a non-UTF-8 path: {e}"))
        .into()
}

/// `git` for a scratch fixture repository. Every inherited `GIT_*`
/// variable goes first, as `utils/check-deny-manifests-test.py`'s
/// `_git_env` drops them: an exported `GIT_DIR` or `GIT_INDEX_FILE` (a
/// hook, `git rebase --exec` in a linked worktree) makes this `init`,
/// `add` and `commit` write into the checkout running the test, and
/// `GIT_CONFIG_PARAMETERS` outranks the configuration pinned here. The
/// host's configuration is then kept away as `tests/common/vcs_fixture.rs`
/// keeps it (#1771): no global or system file, no per-user ignore or
/// attributes file, and a throwaway identity for the commits a fixture
/// makes.
fn fixture_git(dir: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let mut cmd = std::process::Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.as_encoded_bytes().starts_with(b"GIT_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@x"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "core.excludesFile")
        .env("GIT_CONFIG_VALUE_0", "/dev/null")
        .env("GIT_CONFIG_KEY_1", "core.attributesFile")
        .env("GIT_CONFIG_VALUE_1", "/dev/null")
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// `files_under` and `check-role-coverage.py`'s `corpus_listing` list a
/// corpus the same way, and each side spells the contract in its own
/// constants. The Python module's are asked for as it holds them —
/// imported and printed, never read back out of its source — and
/// compared with the three here, as `python_reader_agrees_with_the_compiled_tables`
/// compares the role-table reader with the compiled tables.
#[test]
fn the_two_enumerators_share_their_contract() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the ast crate sits under the workspace root");
    let script = r#"
import importlib.util, json, sys
spec = importlib.util.spec_from_file_location("cov", sys.argv[1])
cov = importlib.util.module_from_spec(spec)
sys.modules["cov"] = cov
spec.loader.exec_module(cov)
print(json.dumps({
    "modes": [m.decode() for m in cov.SAMPLED_MODES],
    "env": list(cov.REPOSITORY_ENV),
    "remedy": cov.MISSING_FILE_REMEDY,
}))
"#;
    let out = std::process::Command::new("python3")
        .arg("-B")
        .arg("-c")
        .arg(script)
        .arg(root.join("utils/check-role-coverage.py"))
        .output()
        .expect("python3 is on PATH: the role gates need it, so this test does too");
    assert!(
        out.status.success(),
        "check-role-coverage.py did not import:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let theirs: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("the probe prints one JSON object");
    let ours = serde_json::json!({
        "modes": SAMPLED_MODES.iter().map(|m| std::str::from_utf8(m).expect("ASCII mode")).collect::<Vec<_>>(),
        "env": REPOSITORY_ENV,
        "remedy": MISSING_FILE_REMEDY,
    });
    pretty_assertions::assert_eq!(theirs, ours);
}

/// `files_under` lists exactly the tracked regular and executable
/// files. On a scratch repository holding every entry the index can
/// carry that is not one — a symlink to a file and one to a directory
/// (serde links one crate's `src` into another's), a nested repository
/// as a registered, active submodule with its own file on disk, as a
/// recursive checkout leaves it (#1836) — and an untracked file, so the
/// sweep's walker is exercised under the gate the sweep itself is not.
/// The submodule is added rather than written as a bare gitlink because
/// `ls-files --recurse-submodules` descends only into an active one.
#[cfg(unix)]
#[test]
fn files_under_lists_the_tracked_regular_files_and_nothing_else() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("scratch dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("a/b")).expect("mkdir");
    std::fs::write(root.join("a/x.rs"), "fn x() {}\n").expect("write");
    std::fs::write(root.join("a/b/y.rs"), "fn y() {}\n").expect("write");
    std::fs::write(root.join("a/run.rs"), "fn run() {}\n").expect("write");
    std::fs::set_permissions(
        root.join("a/run.rs"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("chmod");
    std::os::unix::fs::symlink(root.join("a/b"), root.join("link_to_b")).expect("symlink");
    std::os::unix::fs::symlink(root.join("a/x.rs"), root.join("link_to_x.rs")).expect("symlink");
    fixture_git(root, &["init", "-q"]);
    fixture_git(root, &["add", "--", "a", "link_to_b", "link_to_x.rs"]);
    let source = tempfile::tempdir().expect("scratch dir");
    std::fs::write(source.path().join("n.rs"), "fn n() {}\n").expect("write");
    fixture_git(source.path(), &["init", "-q"]);
    fixture_git(source.path(), &["add", "n.rs"]);
    fixture_git(source.path(), &["commit", "-q", "-m", "nested"]);
    let url = source.path().to_str().expect("UTF-8 scratch path");
    fixture_git(
        root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            url,
            "nested",
        ],
    );
    std::fs::write(root.join("stray.rs"), "fn stray() {}\n").expect("write");
    let index = String::from_utf8(fixture_git(root, &["ls-files", "-s"])).expect("UTF-8");
    let modes: BTreeSet<&str> = index.lines().filter_map(|l| l.split(' ').next()).collect();
    assert_eq!(
        modes,
        BTreeSet::from(["100644", "100755", "120000", "160000"]),
        "the fixture spells every mode the index can hold"
    );
    let mut found: Vec<String> = files_under(root)
        .into_iter()
        .map(|p| {
            p.strip_prefix(root)
                .expect("under root")
                .to_str()
                .expect("UTF-8")
                .to_owned()
        })
        .collect();
    found.sort();
    assert_eq!(found, [".gitmodules", "a/b/y.rs", "a/run.rs", "a/x.rs"]);
}

/// A hook, and `git rebase --exec` in a linked worktree, export
/// `GIT_DIR` and `GIT_INDEX_FILE`, and both outrank `git -C`. The listing
/// test above is re-run in a child with that pair naming another
/// repository: the child must pass, and that repository must come
/// through untouched. Verified by revert: without the scrub in
/// `fixture_git` the child commits into it, and without the one in
/// `git_output` the toplevel check passes and `files_under` lists its
/// index.
#[cfg(unix)]
#[test]
fn files_under_and_its_fixture_ignore_an_inherited_repository() {
    let victim = tempfile::tempdir().expect("scratch dir");
    std::fs::write(victim.path().join("keep.rs"), "fn keep() {}\n").expect("write");
    fixture_git(victim.path(), &["init", "-q"]);
    fixture_git(victim.path(), &["add", "keep.rs"]);
    fixture_git(victim.path(), &["commit", "-q", "-m", "victim"]);
    let read =
        |args: &[&str]| String::from_utf8_lossy(&fixture_git(victim.path(), args)).into_owned();
    let state = || (read(&["rev-parse", "HEAD"]), read(&["ls-files", "-s"]));
    let before = state();
    let git_dir = victim.path().join(".git");
    rerun_listing_test_with(&[
        ("GIT_DIR", git_dir.clone()),
        ("GIT_INDEX_FILE", git_dir.join("index")),
    ]);
    assert_eq!(state(), before, "the inherited repository was written to");
}

/// `GIT_OBJECT_DIRECTORY`, which a `receive-pack` quarantine hook
/// exports, replaces the object store git reads; once the quarantine it
/// names is gone, discovery itself fails with `not a git repository`.
/// The listing test is re-run in a child with it naming a directory
/// that does not exist. Verified by revert: without it in
/// `REPOSITORY_ENV` the child's toplevel check fails.
#[cfg(unix)]
#[test]
fn files_under_ignores_an_inherited_object_directory() {
    let gone = tempfile::tempdir().expect("scratch dir");
    let objects = gone.path().join("quarantine");
    rerun_listing_test_with(&[("GIT_OBJECT_DIRECTORY", objects)]);
}

/// Runs `files_under_lists_the_tracked_regular_files_and_nothing_else`
/// in a child test binary with `vars` exported, and asserts it passed.
/// `files_under` reads the environment through `Command`, and a test
/// cannot set its own process's environment without `unsafe`, so the
/// variables are set on a child instead.
#[cfg(unix)]
fn rerun_listing_test_with(vars: &[(&str, std::path::PathBuf)]) {
    // `module_path!()` carries the crate name; libtest's names do not.
    let child = format!(
        "{}::files_under_lists_the_tracked_regular_files_and_nothing_else",
        module_path!()
            .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
            .expect("the test module is nested under the crate root")
    );
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", &child, "--format", "pretty"])
        .envs(vars.iter().map(|(k, v)| (k, v)))
        .output()
        .expect("spawn the child");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "the listing test must pass with {vars:?} inherited; stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// An uninitialised submodule is a directory of the parent checkout,
/// where git answers for the parent without an error. The file is in
/// the *parent's* index, so without the toplevel check the walker
/// would list it rather than find nothing.
#[test]
#[should_panic(expected = "not the root of its own git repository")]
fn files_under_refuses_a_directory_that_is_not_its_own_repository() {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::create_dir(dir.path().join("corpus")).expect("mkdir");
    std::fs::write(dir.path().join("corpus/x.rs"), "fn x() {}\n").expect("write");
    fixture_git(dir.path(), &["init", "-q"]);
    fixture_git(dir.path(), &["add", "corpus/x.rs"]);
    files_under(&dir.path().join("corpus"));
}

/// A tracked file under a directory the user cannot search is reported
/// as unreadable, not as missing: the remedy for a missing file does
/// nothing for a permission problem.
#[cfg(unix)]
#[test]
fn files_under_names_an_unreadable_tracked_file_as_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("scratch dir");
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).expect("mkdir");
    std::fs::write(locked.join("x.rs"), "fn x() {}\n").expect("write");
    fixture_git(dir.path(), &["init", "-q"]);
    fixture_git(dir.path(), &["add", "locked/x.rs"]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    // A privileged user searches through mode 000, and then there is
    // nothing to refuse.
    let refused = std::fs::metadata(locked.join("x.rs"))
        .is_err()
        .then(|| std::panic::catch_unwind(|| files_under(dir.path())));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod back");
    let Some(refused) = refused else {
        return;
    };
    let message = refused
        .expect_err("an unreadable tracked file is refused")
        .downcast::<String>()
        .expect("a formatted panic message");
    assert!(message.contains("tracked but unreadable"), "{message}");
}

/// `a/x.rs` stats as `NotADirectory`, not `NotFound`, once `a` is a
/// regular file; nothing is there to read, so it is reported as missing
/// rather than unreadable.
#[test]
#[should_panic(expected = "tracked but not on disk")]
fn files_under_refuses_a_tracked_file_whose_directory_is_now_a_file() {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::create_dir(dir.path().join("a")).expect("mkdir");
    std::fs::write(dir.path().join("a/x.rs"), "fn x() {}\n").expect("write");
    fixture_git(dir.path(), &["init", "-q"]);
    fixture_git(dir.path(), &["add", "a/x.rs"]);
    std::fs::remove_dir_all(dir.path().join("a")).expect("rm");
    std::fs::write(dir.path().join("a"), "").expect("write");
    files_under(dir.path());
}

/// A sparse, skip-worktree or interrupted checkout (#1171) lists a file
/// the disk does not hold; the sweep must not walk around it.
#[test]
#[should_panic(expected = "tracked but not on disk")]
fn files_under_refuses_a_tracked_file_missing_from_disk() {
    let dir = tempfile::tempdir().expect("scratch dir");
    std::fs::write(dir.path().join("x.rs"), "fn x() {}\n").expect("write");
    std::fs::write(dir.path().join("gone.rs"), "fn gone() {}\n").expect("write");
    fixture_git(dir.path(), &["init", "-q"]);
    fixture_git(dir.path(), &["add", "x.rs", "gone.rs"]);
    std::fs::remove_file(dir.path().join("gone.rs")).expect("rm");
    files_under(dir.path());
}

/// A corpus whose directory holds no file is the interrupted-checkout
/// state, and `assert_every_corpus_present` names it; one that holds
/// files and routed none is reported, not failed.
#[test]
fn an_empty_corpus_directory_is_an_interrupted_checkout() {
    let present: BTreeMap<&str, usize> = INTEGRATION_CORPORA.iter().map(|c| (*c, 3)).collect();
    let routed: BTreeMap<&str, usize> = [("serde", 0)].into_iter().collect();
    assert_every_corpus_present(&present, &routed);
    let mut empty = present;
    empty.insert("pdf.js", 0);
    let failed = std::panic::catch_unwind(|| assert_every_corpus_present(&empty, &routed));
    let message = failed
        .expect_err("an empty corpus fails")
        .downcast::<String>()
        .expect("a formatted panic message");
    assert!(message.contains("pdf.js: no file under it"), "{message}");
}
