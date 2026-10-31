//! `Cyclomatic` implementation for C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/cpp.rs`, read at the projection
// the metric counts (#1694, #1695, #1697).
//
// `&&` / `||` from `ShortCircuit` at the token, each with an ISO
// alternative spelling (`and`, `or`, [lex.digraph]) the grammar gives a
// kind of its own; without those `b and c` scored no decision beside
// `b && c`'s one (#1522). The grammar also spells the four tokens where
// nothing branches — reference declarators (`int&& x`), the
// ref-qualifier (`void f() &&`), overload names (`operator&&`), and a
// requires-clause's constraint conjunction — and the row's payer gate
// (`cpp_family_operator_is_applied`) admits only a `binary_expression`
// and a fold's operator (#1525).
//
// The `conditional_expression` from `Ternary` at its wrapper. The `if` /
// `for` / `while` / `catch` *keyword tokens* from `Branch` / `Loop` /
// `Catch` (#284): the `for` keyword fires once in a classic `for` and
// once in a `for_range_loop`, the `while` once in a `while` and once in
// a `do … while`, so the statement nodes would double-count. The `case`
// label from `CaseArm` at the token, its row excluding `default`; the
// `switch_statement` from `Switch` at the owner, the modified tier's
// container.
//
// A requires clause or requires-expression scores no decision, however
// it is written: the parenthesised `requires (A<T> && B<T>)` parses as
// an ordinary `binary_expression`, and a requires-expression's
// requirements are never evaluated (#1533), so both are `constraints`.
// Any other compile-time expression the grammar parses as
// `binary_expression` (`static_assert`, `noexcept(…)`, a template
// argument, a concept body) still counts, as `if constexpr` does and as
// cognitive scores it.
impl_cyclomatic_rows!(
    CppCode,
    Cpp,
    roles = crate::roles::cpp,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        catch::pays_at_leaf_kind,
    ],
    standard = [case_arm::pays_at_leaf_kind],
    modified = [switch::owns_kind],
    extra = [],
    constraints = [RequiresClause, RequiresExpression],
);
