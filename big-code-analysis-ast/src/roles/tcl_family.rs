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

/// The string literal of both dialects (#1698): a `quoted_word`, which
/// `wraps` the `$var` and `[cmd]` substitutions whose inner nodes are
/// walked and billed on their own (#277), and the two braced spellings.
/// `braced_word` is the literal of `lappend x {a b}` — the grammar
/// reserves `braced_word_simple` for the handful of commands it models
/// (`set`, `foreach`, `regexp`, …) and spells every other command's
/// braced argument `braced_word`. The same kind is also every `proc` /
/// `if` body and every iRules `when` handler, which only the enclosing
/// command separates from the literal (#1381): the row gates it on
/// `is_braced_literal`, which each dialect emits over its own
/// `BracedWordKinds` through `Getter::is_braced_script_word` — the one
/// reader Halstead, `find string` and the dump flattening share (#1382).
macro_rules! tcl_family_strings {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            QuotedWord,
            BracedWord(gated_by is_braced_literal),
            BracedWordSimple,
        ] $($rest)*);
    };
}

/// The gate `tcl_family_strings!` reads, per dialect, over the
/// dialect's braced-word kinds table.
macro_rules! tcl_family_string_gates {
    ($lang:ident, $code:ident, $kinds:path) => {
        /// Whether a `braced_word` is a literal rather than a script
        /// body (#1381): not a `proc` / `if` / `when` body, by the
        /// enclosing command and the slot it fills.
        fn is_braced_literal<'a>(
            node: &$crate::Node<'a>,
            code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            !<$crate::$code as $crate::Getter>::is_braced_script_word(
                node, code, ancestors, &$kinds,
            )
        }
    };
}

pub(crate) use {
    tcl_family_comparison_payers, tcl_family_short_circuit_payers, tcl_family_string_gates,
    tcl_family_strings,
};
