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

use std::cell::Cell;

use crate::test_support::check_func_space_only;
use crate::*;

/// `[cyclomatic, cyclomatic_modified, abc conditions, cognitive]` sums of
/// the root space; both cyclomatic tiers carry a base of 1.
#[cfg(feature = "elixir")]
type Scores = [u64; 4];

#[cfg(feature = "elixir")]
fn measure(source: &str) -> Scores {
    let out = Cell::new([0; 4]);
    check_func_space_only::<ElixirParser, _>(
        &format!("{source}\n"),
        "foo.exs",
        &[Metric::Cyclomatic, Metric::Abc, Metric::Cognitive],
        |space| {
            let m = &space.metrics;
            out.set([
                m.cyclomatic.cyclomatic_sum(),
                m.cyclomatic.cyclomatic_modified_sum(),
                m.abc.conditions_sum(),
                m.cognitive.cognitive_sum(),
            ]);
        },
    );
    out.get()
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
/// scores what the two-clause `case` does.
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
