//! Mozcpp's role rows. The dialect owns no file extension and shares
//! Cpp's grammar shape with different kind ids, so its rows splice the
//! same `cpp_family_*` fragments Cpp's do (`roles/c_family.rs`,
//! contract 5 of #1690): the `Comparison` and `ShortCircuit` rows are
//! declared through them (#1694), the Halstead keyword rows are
//! declared (#1693) and the rest are pending or absent as Cpp's are.
//! Nothing routes a file to this dialect, so no corpus snapshot reaches
//! these rows; `tests/parity/cpp_mozcpp_parity.rs` holds the two
//! dialects together.

use crate::Mozcpp;
use crate::role_table;
use crate::roles::c_family::{
    c_family_decl_keywords, c_family_modifiers, c_family_primitives, cpp_family_comparison_owners,
    cpp_family_comparison_payers, cpp_family_gates, cpp_family_short_circuit_owners,
    cpp_family_short_circuit_payers,
};

cpp_family_gates!(Mozcpp);

role_table! {
    lang: Mozcpp;
    Call { pending R08 }
    StringLit { pending R08 }
    // The C++ binary operators are one `binary_expression` kind (the
    // aliased `BinaryExpression2` at the pinned grammar; the unsuffixed
    // id is the never-emitted pre-alias) plus the fold, told apart by
    // the `operator` field, so each row's owners are gated on that
    // field holding one of its payers and each payer on applying its
    // operator (`roles/c_family.rs` has both lists and their
    // derivations — #1448, #1522, #1525, #1533). A `requires` clause or
    // expression is compile-time and its operators decide nothing; the
    // rows do not know that, because a parenthesised constraint parses
    // as an ordinary `binary_expression` and only the root crate's
    // `ConstraintReach` can see the clause above it. The consumers read
    // the rows *under* that reach.
    Comparison {
        owns [..cpp_family_comparison_owners!()]
        pays [..cpp_family_comparison_payers!()]
        project "operator"
    }
    ShortCircuit {
        owns [..cpp_family_short_circuit_owners!()]
        pays [..cpp_family_short_circuit_payers!()]
        project "operator"
    }
    EagerLogical { absent "C++'s `&` / `|` / `^` are bitwise; `and` / `or` are the lazy spellings" }
    // The `conditional_expression` owns the ternary and its `?` pays,
    // ungated: the grammar spells a bare `?` in no other production. The
    // GNU elision `a ?: b` is the same node with its `consequence` field
    // empty, which the ABC slot walker reads by field (#1102).
    Ternary {
        owns [ConditionalExpression]
        pays [QMARK]
        project "operator"
    }
    NullSafe { absent "C++ has no optional navigation or null coalescing" }
    CaseArm { pending R05 }
    Guard { absent "C++ has no guard clause on a case or a function head" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // Cpp's three keyword rows, verbatim. The dialect shares Cpp's
    // grammar shape and owns no file extension, so nothing routes to it
    // and no corpus snapshot reaches these rows;
    // `tests/parity/cpp_mozcpp_parity.rs` holds the two vocabularies
    // together. The derivations are on the Cpp rows.
    Modifier {
        owns [
            ..c_family_modifiers!(),
            Virtual, Mutable, Constinit, Consteval, Explicit, Friend, Public, Private,
            Protected, Override, Final, Noexcept2,
        ]
    }
    DeclKeyword { owns [..c_family_decl_keywords!(), Class, Namespace, Template, Typename, Using] }
    Primitive { owns [..c_family_primitives!()] }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "C++ documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
