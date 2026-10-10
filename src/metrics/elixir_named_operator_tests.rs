//! A *named* Elixir operator scores like any other name (#1534).
//!
//! `&==/2`, `&and/2` and `Kernel.||(a, b)` name an operator: the grammar
//! wraps the token in an `operator_identifier`. Halstead billed both the
//! wrapper (an operand) and the token inside it (an operator), and
//! cyclomatic counted a captured `&&` / `||` / `and` / `or` as a decision.
//! Every row pairs the fixture with a twin naming an ordinary function,
//! which is the oracle; the twin's numbers are asserted outright too, so
//! a row cannot pass by both drifting. A remote call (`Kernel.||(a, b)`)
//! still applies the operator, so it keeps the decision; it is in the
//! applied table below.

use std::cell::Cell;

use crate::test_support::check_func_space_only;
use crate::*;

/// `[n1, N1, n2, N2]`.
#[cfg(feature = "elixir")]
type Halstead = [u64; 4];

/// `[cyclomatic, cyclomatic_modified]` sums of the root space, which
/// carries a base of 1 each.
#[cfg(feature = "elixir")]
type Cyclomatic = [u64; 2];

#[cfg(feature = "elixir")]
fn measure(source: &str) -> (Halstead, Cyclomatic) {
    let out = Cell::new(([0; 4], [0; 2]));
    check_func_space_only::<ElixirParser, _>(
        &format!("{source}\n"),
        "foo.exs",
        &[Metric::Halstead, Metric::Cyclomatic],
        |space| {
            let m = &space.metrics;
            out.set((
                [
                    m.halstead.unique_operators(),
                    m.halstead.total_operators(),
                    m.halstead.unique_operands(),
                    m.halstead.total_operands(),
                ],
                [
                    m.cyclomatic.cyclomatic_sum(),
                    m.cyclomatic.cyclomatic_modified_sum(),
                ],
            ));
        },
    );
    out.get()
}

/// Whether `source` parses with an `operator_identifier`, so a row
/// whose fixture drifted to an ordinary name cannot pass by comparing
/// the twin with itself.
#[cfg(feature = "elixir")]
fn names_an_operator(source: &str) -> bool {
    let parser = ElixirParser::new(
        source.as_bytes().to_vec(),
        std::path::Path::new("foo.exs"),
        None,
    );
    crate::test_support::ast_has_kind_id(&parser, Elixir::OperatorIdentifier as u16)
}

/// Each named operator bills one operand — its `operator_identifier` —
/// and no operator, exactly as `foo` does in `&foo/2`, and decides
/// nothing. Before #1534 each row billed one operator more, and the
/// `&&` / `||` / `and` / `or` rows scored a decision in both tiers.
#[cfg(feature = "elixir")]
#[test]
fn elixir_named_operator_scores_like_a_named_function() {
    // `(twin, named, halstead)`; every row decides nothing.
    let rows: [(&str, &str, Halstead); 15] = [
        ("a = &foo/2", "a = &==/2", [3, 3, 3, 3]),
        ("a = &foo/2", "a = &in/2", [3, 3, 3, 3]),
        ("a = &foo/2", "a = &not in/2", [3, 3, 3, 3]),
        ("a = &foo/2", "a = &and/2", [3, 3, 3, 3]),
        ("a = &foo/2", "a = &or/2", [3, 3, 3, 3]),
        ("a = &foo/2", "a = &||/2", [3, 3, 3, 3]),
        ("a = &foo/1", "a = &not/1", [3, 3, 3, 3]),
        ("a = &foo/1", "a = &!/1", [3, 3, 3, 3]),
        ("a = &foo/2", "a = &+/2", [3, 3, 3, 3]),
        ("a = Kernel.foo(x, y)", "a = Kernel.==(x, y)", [4, 4, 5, 5]),
        // A capture through the module names the operator too.
        ("a = &Kernel.foo/2", "a = &Kernel.||/2", [4, 4, 4, 4]),
        // `..` is a childless `operator_identifier`: the wrapper is the
        // only node that can bill it, which is why it is the keeper.
        ("a = foo", "a = ..", [1, 1, 2, 2]),
        ("a = &foo/0", "a = &../0", [3, 3, 3, 3]),
        // The lexer reads `&&&` as one token, so `&&&/2` holds no
        // capture `&`: it is the name `&&&` divided by 2, like `foo/2`.
        // `&&/1` lexes the same way, as the name `&&`.
        ("a = foo/2", "a = &&&/2", [2, 2, 3, 3]),
        ("a = foo/1", "a = &&/1", [2, 2, 3, 3]),
    ];
    for (twin, named, halstead) in rows {
        assert!(
            names_an_operator(named),
            "`{named}` lost its operator_identifier"
        );
        let want = (halstead, [1, 1]);
        assert_eq!(measure(twin), want, "`{twin}` moved; re-derive the row");
        assert_eq!(measure(named), want, "`{named}` must score like `{twin}`");
    }
}

/// The operand is keyed by the operator's text, so two named operators
/// stay two names: `a`, `==`, `2`, `b`, `<`, with `2` twice.
#[cfg(feature = "elixir")]
#[test]
fn elixir_named_operators_are_distinct_operands() {
    let (halstead, _) = measure("a = &==/2\nb = &</2");
    assert_eq!(halstead[2..], [5, 6]);
}

/// An applied operator keeps its operator and its operands, and a
/// short-circuit one keeps its decision in both tiers. A remote call
/// naming a short-circuit operator, directly or through a pipe, expands
/// the `Kernel` macro and so decides as `x || y` does, while billing its
/// name as the operand a remote call's function name is. The same call on
/// a user module (`Foo.||`) is an ordinary function call and decides
/// nothing.
#[cfg(feature = "elixir")]
#[test]
fn elixir_applied_operator_keeps_its_operator() {
    // `(source, halstead, cyclomatic)`.
    let rows: [(&str, Halstead, Cyclomatic); 11] = [
        ("a = x == y", [2, 2, 3, 3], [1, 1]),
        ("a = x in y", [2, 2, 3, 3], [1, 1]),
        ("a = not x", [2, 2, 2, 2], [1, 1]),
        ("a = x and y", [2, 2, 3, 3], [2, 2]),
        ("a = x || y", [2, 2, 3, 3], [2, 2]),
        ("a = Kernel.||(x, y)", [4, 4, 5, 5], [2, 2]),
        ("a = Kernel.&&(x, y)", [4, 4, 5, 5], [2, 2]),
        ("a = x |> Kernel.||(y)", [4, 4, 5, 5], [2, 2]),
        ("a = Elixir.Kernel.||(x, y)", [4, 4, 5, 5], [2, 2]),
        ("a = Foo.||(x, y)", [4, 4, 5, 5], [1, 1]),
        ("a = x |> Foo.||(y)", [4, 4, 5, 5], [1, 1]),
    ];
    for (source, halstead, cyclomatic) in rows {
        assert_eq!(measure(source), (halstead, cyclomatic), "`{source}`");
    }
}
