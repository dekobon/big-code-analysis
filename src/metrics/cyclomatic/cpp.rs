//! `Cyclomatic` implementation for C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// C++ has only `&&` and `||` short-circuit operators, each with an ISO
// alternative spelling (`and`, `or`, [lex.digraph]) that the grammar
// gives a kind of its own; without those `b and c` scored no decision
// beside `b && c`'s one (#1522). Each alternative occurs only in
// productions its symbol also occurs in, so it shares the symbol's arm.
// Grammar-specific loop kinds (`DoStatement`, `ForRangeLoop`) are NOT
// listed here because the `While` / `For` keyword-token arms above
// already fire inside them; adding the statement nodes would
// double-count (issue #284).
//
// The grammar also spells `&&` / `||` / `and` / `or` where nothing
// branches: reference declarators (`int&& x`, `T&& f()`, `auto&& y`),
// the ref-qualifier (`void f() &&`), overload names (`operator&&`,
// `operator and`), and a requires-clause's constraint conjunction and
// disjunction (`requires A<T> && B<T>`), which are compile-time
// constraint checks, not runtime branches; each scored a decision
// (#1525). `cpp_operator_is_applied` admits only a `binary_expression`
// and a fold's operator.
//
// A requires clause or requires-expression scores no decision, however
// it is written: the parenthesised `requires (A<T> && B<T>)` parses as
// an ordinary `binary_expression`, and a requires-expression's
// requirements are never evaluated (#1533). Any other compile-time
// expression the grammar parses as `binary_expression` (`static_assert`,
// `noexcept(…)`, a template argument, a concept body) still counts, as
// `if constexpr` does and as cognitive scores it.
impl_cyclomatic_c_family!(
    CppCode,
    Cpp,
    ConditionalExpression,
    [AMPAMP, PIPEPIPE, And, Or],
    applied_if = cpp_operator_is_applied,
    constraints = [RequiresClause, RequiresExpression],
);
