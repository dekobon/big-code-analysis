//! The kind lists Tcl and iRules share (#1695; contract 5 of #1690).
//! Each is a fragment a dialect's row splices with `..name!()`, and
//! iRules adds its own kinds beside it. The grammars give these kinds
//! different ids, so a fragment spells variant *names* and
//! `role_table!` resolves each against the dialect enum the row names.
//!
//! A `binop_expr` names no operator field: its operator is one token
//! among its children, so each row's owner is gated on *having* a
//! payer child, and the row's projection — the first child whose kind
//! is a declared payer — is the lookup.

/// The comparison tokens both dialects spell inside a `binop_expr`:
/// the six symbols, the string comparators `eq` / `ne` and the list
/// membership tests `in` / `ni`. Each is a kind of its own that the
/// grammar emits only there.
macro_rules! tcl_family_comparison_payers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            EQEQ, BANGEQ, LT, GT, LTEQ, GTEQ, Eq, Ne, In, Ni,
        ] $($rest)*);
    };
}

/// `&&` / `||`, which both grammars emit only inside a `binop_expr`.
macro_rules! tcl_family_short_circuit_payers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)* AMPAMP, PIPEPIPE,] $($rest)*);
    };
}

pub(crate) use {tcl_family_comparison_payers, tcl_family_short_circuit_payers};
