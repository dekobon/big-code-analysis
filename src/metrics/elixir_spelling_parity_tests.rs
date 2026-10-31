//! Two spellings of one Elixir program score alike (#1574).
//!
//! Elixir writes a construct's sections three ways: a `do … end` block, a
//! trailing keyword list (`if c, do: 1, else: 2`), and the same list in
//! brackets (`if(c, [do: 1, else: 2])`), which the compiler reads as the
//! identical last argument. And it applies a short-circuit operator either
//! as an operator (`a || b`) or as a `Kernel` remote call
//! (`Kernel.||(a, b)`), which expands the same macro.
//!
//! Every group pairs the spellings with a twin whose score predates the
//! fix — the block form, or the operator form — and that is the oracle:
//! the twin's numbers are asserted outright, so a group cannot pass by
//! every spelling drifting together. The one twin that moved is the
//! `with … else` block form's cognitive score, decided in
//! [`elixir_with_else_scores_like_case`] against `case`.

use crate::test_support::metrics_verbatim;
use crate::*;

/// `[cyclomatic, cyclomatic_modified, abc conditions, cognitive]` sums of
/// the root space; both cyclomatic tiers carry a base of 1.
#[cfg(feature = "elixir")]
type Scores = [u64; 4];

#[cfg(feature = "elixir")]
fn measure(source: &str) -> Scores {
    let m = metrics_verbatim(
        LANG::Elixir,
        format!("{source}\n").as_bytes(),
        MetricsOptions::default().with_only(&[Metric::Cyclomatic, Metric::Abc, Metric::Cognitive]),
    );
    [
        m.cyclomatic.cyclomatic_sum(),
        m.cyclomatic.cyclomatic_modified_sum(),
        m.abc.conditions_sum(),
        m.cognitive.cognitive_sum(),
    ]
}

/// Asserts `twin` scores `want`, then that every spelling scores the same.
#[cfg(feature = "elixir")]
fn assert_group(twin: &str, want: Scores, spellings: &[&str]) {
    assert_eq!(
        measure(twin),
        want,
        "twin `{twin}` moved; re-derive the group"
    );
    for spelling in spellings {
        assert_eq!(
            measure(spelling),
            want,
            "`{spelling}` must score like `{twin}`"
        );
    }
}

/// A `Kernel` remote call naming `&&` / `||` is the operator: one
/// decision, one ABC slot per operand, one cognitive sequence. Before
/// #1574 only cyclomatic counted it; ABC and cognitive scored 0, and a
/// nested call read as a plain operand, so `a || Kernel.||(b, c)` paid a
/// slot for the inner call that `a || (b || c)` does not.
#[cfg(feature = "elixir")]
#[test]
fn elixir_kernel_remote_call_scores_like_the_operator() {
    assert_group(
        "a || b",
        [2, 2, 2, 1],
        &[
            "Kernel.||(a, b)",
            "Elixir.Kernel.||(a, b)",
            // The pipe passes the piped value as the first operand.
            "a |> Kernel.||(b)",
        ],
    );
    assert_group("a && b", [2, 2, 2, 1], &["Kernel.&&(a, b)"]);
    // A nested chain pays its own operands and no slot as an operand.
    assert_group(
        "a || (b || c)",
        [3, 3, 3, 1],
        &[
            "Kernel.||(a, Kernel.||(b, c))",
            "a || Kernel.||(b, c)",
            "Kernel.||(a, b || c)",
        ],
    );
    // A change of operator opens a second cognitive sequence.
    assert_group(
        "a && (b || c)",
        [3, 3, 3, 2],
        &["Kernel.&&(a, Kernel.||(b, c))"],
    );
    // Piping the call's *result* passes no operand to it.
    assert_group("(a || b) |> f()", [2, 2, 2, 1], &["Kernel.||(a, b) |> f()"]);
    // As an `if` predicate the call scores itself, as the chain does.
    assert_group(
        "if a || b, do: 1",
        [3, 3, 2, 2],
        &["if Kernel.||(a, b), do: 1"],
    );
}

/// `Kernel.and` / `Kernel.or` expand the same short-circuit macros as
/// `a and b` / `a or b`, but the grammar names them with a plain
/// `identifier` rather than an operator token, so before #1582 they
/// scored nothing in any metric. Each word form keys the cognitive
/// sequence its symbol does, so a mixed run of `or` and `||` is one
/// sequence and a change of operation opens a second.
#[cfg(feature = "elixir")]
#[test]
fn elixir_kernel_word_operator_scores_like_the_operator() {
    assert_group(
        "a and b",
        [2, 2, 2, 1],
        &[
            "Kernel.and(a, b)",
            "Elixir.Kernel.and(a, b)",
            "a |> Kernel.and(b)",
        ],
    );
    assert_group("a or b", [2, 2, 2, 1], &["Kernel.or(a, b)"]);
    assert_group(
        "a or (b or c)",
        [3, 3, 3, 1],
        &["Kernel.or(a, Kernel.or(b, c))", "a or Kernel.||(b, c)"],
    );
    assert_group(
        "a and (b or c)",
        [3, 3, 3, 2],
        &["Kernel.and(a, Kernel.or(b, c))"],
    );
    assert_group(
        "if a and b, do: 1",
        [3, 3, 2, 2],
        &["if Kernel.and(a, b), do: 1"],
    );
}

/// A negation decides nothing in any spelling, applied or called on
/// `Kernel`; a user module's `and/2` is an ordinary function; and a
/// capture or a bare reference only names `Kernel`'s.
#[cfg(feature = "elixir")]
#[test]
fn elixir_kernel_negation_and_foreign_word_operator_decide_nothing() {
    assert_group(
        "not x",
        [1, 1, 0, 0],
        &["!x", "Kernel.not(x)", "Kernel.!(x)"],
    );
    assert_group(
        "Foo.foo(a, b)",
        [1, 1, 0, 0],
        &[
            "Foo.and(a, b)",
            "Foo.or(a, b)",
            "&Kernel.and/2",
            "Kernel.and",
        ],
    );
}

/// A user module's `||/2` is an ordinary function, and a capture only
/// names `Kernel`'s: neither decides anything in any metric.
#[cfg(feature = "elixir")]
#[test]
fn elixir_non_kernel_remote_call_decides_nothing() {
    assert_group(
        "Foo.foo(a, b)",
        [1, 1, 0, 0],
        &["Foo.||(a, b)", "a |> Foo.||(b)", "&Kernel.||/2"],
    );
    // In a slot, neither a user module's `||` nor a `Kernel` function
    // that is no operator scores itself: each pays the slot as a call.
    assert_group(
        "if foo(a), do: 1",
        [2, 2, 1, 1],
        &["if Kernel.foo(a), do: 1", "if Foo.||(a, b), do: 1"],
    );
}

/// `if` / `unless` with an `else`, in every spelling of the keyword list,
/// including a trailing comma and quoted keys (`"do":` is the atom
/// `:do`). Before #1574 the keyword forms lost cognitive's `else` +1,
/// and the bracketed and quoted forms lost ABC's `else` condition too.
#[cfg(feature = "elixir")]
#[test]
fn elixir_if_else_scores_alike_in_every_spelling() {
    assert_group(
        "if c do 1 else 2 end",
        [2, 2, 2, 2],
        &[
            "if(c, do: 1, else: 2)",
            "if(c, [do: 1, else: 2])",
            "if(c, [do: 1, else: 2,])",
            "if(c, \"do\": 1, \"else\": 2)",
            "if(c, [\"do\": 1, \"else\": 2])",
        ],
    );
    assert_group(
        "unless c do 1 else 2 end",
        [2, 2, 2, 2],
        &["unless(c, do: 1, else: 2)", "unless(c, [do: 1, else: 2])"],
    );
    // Each `else` belongs to its own `if`, however the two nest.
    assert_group(
        "if a do (if b do 1 else 2 end) else 3 end",
        [3, 3, 4, 5],
        &[
            "if(a, do: if(b, do: 1, else: 2), else: 3)",
            "if(a, [do: if(b, [do: 1, else: 2]), else: 3])",
        ],
    );
}

/// Only the *last* argument, and only a list holding nothing but pairs,
/// is a keyword list: `[do: 1, else: 2]` in first position, `[1, do: …]`
/// and `[]` are ordinary list arguments, so the `if` has no `else`. And
/// `else:` is an ordinary option name to any other function.
#[cfg(feature = "elixir")]
#[test]
fn elixir_list_that_is_not_a_keyword_list_has_no_sections() {
    assert_group("[1]", [1, 1, 0, 0], &["[else: 1]", "[do: 1, else: 2]"]);
    assert_group(
        "if(c, x)",
        [2, 2, 1, 1],
        &[
            "if([do: 1, else: 2], c)",
            "if(c, [1, do: 1, else: 2])",
            "if(c, [])",
        ],
    );
    assert_group(
        "f(x, y)",
        [1, 1, 0, 0],
        &["f(x, else: 1)", "f(x, [else: 1])"],
    );
    assert_group(
        "Foo.bar(x, y)",
        [1, 1, 0, 0],
        &["Foo.bar(x, else: 1)", "Foo.bar(x, [else: 1])"],
    );
    // An interpolated key has no static name. Only an ordinary call
    // takes one validly, and no ordinary call has sections, so this row
    // exercises the reading rather than discriminating it.
    assert_group("f(x, y)", [1, 1, 0, 0], &["f(x, \"else#{y}\": 1)"]);
}

/// Clause constructs: the bracketed `cond` lost its free `true ->`
/// default, because the `cond` lookup climbed a fixed number of parents.
#[cfg(feature = "elixir")]
#[test]
fn elixir_clause_constructs_score_alike_in_every_spelling() {
    assert_group(
        "case x do 1 -> :a; 2 -> :b end",
        [3, 2, 2, 1],
        &[
            "case(x, do: (1 -> :a; 2 -> :b))",
            "case(x, [do: (1 -> :a; 2 -> :b)])",
        ],
    );
    assert_group(
        "cond do x > 1 -> :a; true -> :b end",
        [2, 2, 1, 1],
        &[
            "cond(do: (x > 1 -> :a; true -> :b))",
            "cond([do: (x > 1 -> :a; true -> :b)])",
        ],
    );
}

/// `try`'s `rescue:` / `catch:` pay +nesting and its `else:` +1 in the
/// keyword forms, as their blocks do; before #1574 the keyword `try`
/// scored cognitive 0. A `def` body is an implicit `try` and takes the
/// same sections.
#[cfg(feature = "elixir")]
#[test]
fn elixir_try_sections_score_alike_in_every_spelling() {
    assert_group(
        "try do f() rescue e in RuntimeError -> e catch :x -> 1 end",
        [3, 2, 2, 2],
        &[
            "try(do: f(), rescue: (e in RuntimeError -> e), catch: (:x -> 1))",
            "try([do: f(), rescue: (e in RuntimeError -> e), catch: (:x -> 1)])",
        ],
    );
    assert_group(
        "try do f() rescue _ -> 1 else x -> x end",
        [2, 2, 1, 2],
        &[
            "try(do: f(), rescue: (_ -> 1), else: (x -> x))",
            "try([do: f(), rescue: (_ -> 1), else: (x -> x)])",
        ],
    );
    // A handler raises nesting, not just +1: the `if` inside it costs 2.
    assert_group(
        "try do f() rescue _ -> (if a do 1 end) end",
        [2, 3, 1, 3],
        &[
            "try(do: f(), rescue: (_ -> if(a, do: 1)))",
            "try([do: f(), rescue: (_ -> if(a, do: 1))])",
        ],
    );
    assert_group(
        "try do f() catch _ -> (if a do 1 end) end",
        [2, 3, 1, 3],
        &["try(do: f(), catch: (_ -> if(a, do: 1)))"],
    );
    assert_group(
        "def f() do\n  g()\nrescue\n  e -> e\nend",
        [3, 2, 1, 1],
        &["def f(), do: g(), rescue: (e -> e)"],
    );
}

/// A `with`'s `else` holds the clauses that dispatch over the value a
/// `<-` failed to match: the arms of a `case` in all but spelling. A
/// `case`'s arms add no cognitive cost past the +nesting its container
/// pays, and Ruby draws the same line for the `else` of `case/when`
/// (#451), so the block form's `else` token adds nothing either; the
/// keyword form already scored that (#1574). An `if`'s `else` is a
/// conditional's alternative and keeps its +1 (see the nested row).
///
/// The bracketed form also lost its modified tier: the `else` lookup
/// read only a bare keyword list, so the dispatching `else` looked absent.
#[cfg(feature = "elixir")]
#[test]
fn elixir_with_else_scores_like_case() {
    // The oracle: the same dispatch as one `case`, cognitive 1.
    assert_eq!(
        measure("case g(x) do {:ok, a} -> a; {:error, e} -> e; :z -> 0 end")[3],
        1
    );
    assert_group(
        "with({:ok, a} <- g(x), do: a, else: ({:error, e} -> e; :z -> 0))",
        [4, 3, 3, 1],
        &[
            "with {:ok, a} <- g(x) do a else {:error, e} -> e; :z -> 0 end",
            "with({:ok, a} <- g(x), [do: a, else: ({:error, e} -> e; :z -> 0)])",
        ],
    );
    // A non-dispatching `else _ -> 0` is free in the modified tier too.
    assert_group(
        "with({:ok, a} <- g(x), do: a, else: (_ -> 0))",
        [2, 2, 1, 1],
        &[
            "with {:ok, a} <- g(x) do a else _ -> 0 end",
            "with({:ok, a} <- g(x), [do: a, else: (_ -> 0)])",
        ],
    );
    // The `if` nested in the body keeps its own `else` +1 at nesting 1.
    assert_group(
        "with({:ok, a} <- g(x), do: if(a, do: 1, else: 2), else: (_ -> 0))",
        [3, 3, 3, 4],
        &["with {:ok, a} <- g(x) do (if a do 1 else 2 end) else _ -> 0 end"],
    );
}

/// A `for … reduce:`'s first accumulator clause is its head, free in
/// every spelling (#1535). The bracketed form missed the `for` behind
/// the list and scored the head as a decision.
#[cfg(feature = "elixir")]
#[test]
fn elixir_for_reduce_head_is_free_in_every_spelling() {
    assert_group(
        "for x <- xs, reduce: 0 do acc -> acc + x end",
        [2, 2, 0, 1],
        &[
            "for(x <- xs, reduce: 0, do: (acc -> acc + x))",
            "for(x <- xs, [reduce: 0, do: (acc -> acc + x)])",
        ],
    );
    assert_group(
        "for x <- xs, reduce: 0 do 0 -> x; acc -> acc + x end",
        [3, 2, 1, 1],
        &[
            "for(x <- xs, reduce: 0, do: (0 -> x; acc -> acc + x))",
            "for(x <- xs, [reduce: 0, do: (0 -> x; acc -> acc + x)])",
        ],
    );
}

/// A `receive` is a `case` over the mailbox and scores as one in every
/// spelling (#1581): before, modified cyclomatic and cognitive did not
/// know the construct, so it sat at the base of both, and its `after`
/// timeout counted as one more clause. The timeout is the path taken
/// when no clause matched in time — `receive`'s default arm, which
/// `case`'s `_ ->` twin shows is free — so with it the `receive` still
/// scores what the two-clause `case` does. A `receive` has one free arm,
/// though, so beside a bare `_ ->` the timeout counts.
#[cfg(feature = "elixir")]
#[test]
fn elixir_receive_scores_like_case() {
    assert_group(
        "case x do {:a, y} -> y; :b -> 0 end",
        [3, 2, 2, 1],
        &[
            "receive do {:a, y} -> y; :b -> 0 end",
            "receive do {:a, y} -> y; :b -> 0 after 10 -> :t end",
            "receive(do: ({:a, y} -> y; :b -> 0), after: (10 -> :t))",
            "receive([do: ({:a, y} -> y; :b -> 0), after: (10 -> :t)])",
        ],
    );
    // The container raises nesting for what its clauses hold.
    assert_group(
        "case x do {:a, y} -> (if y do 1 end) end",
        [3, 3, 2, 3],
        &[
            "receive do {:a, y} -> (if y do 1 end) after 10 -> :t end",
            "receive(do: ({:a, y} -> if(y, do: 1)), after: (10 -> :t))",
        ],
    );
    // A `receive` gets one free arm. With a bare `_ ->` in its `do`
    // section the timeout is a path of its own: a matched `{:a, y}`, any
    // other message, and an empty mailbox timing out make three paths,
    // so cyclomatic is 3 (two decisions: `{:a, y}` and `after`, the `_`
    // free), modified 2 (base + the container), C 2 (one per decision),
    // cognitive 1 (+nesting on the container, arms free) — exactly the
    // three-clause `case` beside it, whose `_ ->` is its free arm.
    assert_group(
        "case x do {:a, y} -> y; :b -> 0; _ -> 1 end",
        [3, 2, 2, 1],
        &[
            "receive do {:a, y} -> y; _ -> 0 after 10 -> :t end",
            "receive(do: ({:a, y} -> y; _ -> 0), after: (10 -> :t))",
            "receive([do: ({:a, y} -> y; _ -> 0), after: (10 -> :t)])",
        ],
    );
    // A lone catch-all is free in standard and still pays the container.
    assert_group(
        "case x do _ -> 0 end",
        [1, 2, 0, 1],
        &["receive do _ -> 0 end", "receive(do: (_ -> 0))"],
    );
    // With no message clause it is a timed wait, the sleep it spells.
    assert_group(
        "Process.sleep(10)",
        [1, 1, 0, 0],
        &[
            "receive do after 10 -> :t end",
            "receive(after: (10 -> :t))",
            "receive([after: (10 -> :t)])",
        ],
    );
}

/// The Elixir cyclomatic and cognitive bodies dispatch once on the
/// cached word rather than reading the `Branch` / `Switch` / `Loop` rows
/// (one text read in place of five — plan §4.2), so the rows and the
/// policy tables are two spellings of one set, and this holds them
/// together word by word. For every `ElixirCallTarget`, a `call` spelling
/// it with a one-arm `do` body is measured against the same body under a
/// plain `foo`: the *container's own* contribution — what the `call`
/// adds beyond its arm — is non-zero in some tier, and cognitive opens a
/// level, exactly when the `Branch`, `Switch` or `Loop` row owns the
/// word — minus the two per-word policies the consumers carry, each
/// named here so a third cannot slip in unlisted: `with` pays no
/// container decision (its decisions are its `<-` clauses, its `else`
/// pays modified only when it dispatches, #1535) but opens a level; `try`
/// pays modified but opens no level (its `rescue` / `catch` arms do,
/// #416). The one arm is a bare `_ ->`, which every construct's default
/// exclusion leaves free (#1272) — so under a plain `foo` the body
/// scores the base `(1, 1, 0)` and whatever a construct adds is the
/// container's own. A word added to a row gate and not to a consumer's
/// arm, or the reverse, fails here; so does a variant added to
/// `ElixirCallTarget` without a spelling here (#1697). `receive` is owned and dispatches here; its
/// `after`-only timed-wait exception is pinned separately (#1581).
#[cfg(feature = "elixir")]
#[test]
fn elixir_consumer_word_policy_agrees_with_the_control_flow_rows() {
    use crate::lang_helpers::elixir::ElixirCallTarget;
    use crate::roles::elixir::{branch, r#loop, switch};
    use crate::test_support::for_each_node_with_chain;

    let word = |t: ElixirCallTarget| -> &'static str {
        use ElixirCallTarget as T;
        match t {
            T::If => "if",
            T::Unless => "unless",
            T::For => "for",
            T::While => "while",
            T::Case => "case",
            T::Cond => "cond",
            T::With => "with",
            T::Try => "try",
            T::Receive => "receive",
            T::Throw => "throw",
            T::Raise => "raise",
            T::Reraise => "reraise",
            T::Exit => "exit",
            T::Def => "def",
            T::Defp => "defp",
            T::Defmacro => "defmacro",
            T::Defmacrop => "defmacrop",
            T::Defmodule => "defmodule",
            T::Defprotocol => "defprotocol",
            T::Defimpl => "defimpl",
            T::Defstruct => "defstruct",
            T::Defguard => "defguard",
            T::Defguardp => "defguardp",
            T::Quote => "quote",
            T::Alias => "alias",
            T::Import => "import",
            T::Require => "require",
            T::Use => "use",
            T::Other => "foo",
            // `#[non_exhaustive]` across the crate boundary wants the
            // arm; a new variant must be spelled here, not skipped.
            other => panic!("`ElixirCallTarget::{other:?}` has no fixture spelling in this test"),
        }
    };
    // The scores of the enclosing `f`'s *own* space (`value`, not the
    // subtree `sum`): a `def`-family word opens a function space of its
    // own, whose base cyclomatic 1 would read as a decision in a sum.
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
    let mut rows_owning = 0;
    for target in ElixirCallTarget::ALL {
        let w = word(target);
        let source = format!("def f(x) do\n  {w} x do\n    _ -> x\n  end\nend\n");
        let bytes = source.as_bytes();
        let mut owned = None;
        for_each_node_with_chain::<ElixirCode>(bytes, |node, chain| {
            if node.kind_id() == Elixir::Call as u16 && node.start_position().0 == 1 {
                let a = Ancestors::known(chain);
                let kind = node.kind_id();
                owned = Some(
                    branch::owns_kind(kind, node, bytes, a)
                        || switch::owns_kind(kind, node, bytes, a)
                        || r#loop::owns_kind(kind, node, bytes, a),
                );
            }
        });
        let row_owns = owned.expect("the fixture's control call");
        let (std, modified, cog) = score(w);
        let decides = std > baseline.0 || modified > baseline.1;
        let nests = cog > baseline.2;
        rows_owning += usize::from(row_owns);
        let container_decides = row_owns && target != ElixirCallTarget::With;
        let opens_level = row_owns && target != ElixirCallTarget::Try;
        assert_eq!(
            (decides, nests),
            (container_decides, opens_level),
            "`{w}`: rows own it = {row_owns}; scores {std} / {modified} / {cog} vs plain {baseline:?}",
        );
    }
    assert_eq!(
        rows_owning, 9,
        "if unless for while case cond with try receive"
    );
}
