//! Cpp's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — are declared; the rest are pending
//! the package that declares each (#1692).

use crate::Cpp;
use crate::role_table;
use crate::roles::c_family::{
    c_family_calls, c_family_decl_keywords, c_family_modifiers, c_family_primitives,
    c_family_strings, cpp_family_binary_owners, cpp_family_comparison_payers,
    cpp_family_containers, cpp_family_exits, cpp_family_func_openers, cpp_family_gates,
    cpp_family_loops, cpp_family_short_circuit_payers,
};
use crate::roles::control_flow::else_if_parent_clause_gate;

cpp_family_gates!(Cpp);
// The chained `if` sits directly inside an `else_clause`; a nested one
// under `else { … }` sits inside the compound statement (#1697).
else_if_parent_clause_gate!(Cpp, ElseClause);

role_table! {
    lang: Cpp;
    Call { owns [..c_family_calls!()] }
    // `R"(…)"` joins the family's two: it is one string-like token to
    // `find` and the dump (#398, the peer of #391 for Rust).
    StringLit { owns [..c_family_strings!(), RawStringLiteral] }
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
        owns [..cpp_family_binary_owners!()]
        pays [..cpp_family_comparison_payers!()]
        project "operator"
    }
    ShortCircuit {
        owns [..cpp_family_binary_owners!()]
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
    // A `case_statement` is the grammar's node for a `case X:` label
    // and a `default:` label alike; the arm owns a decision when it
    // holds the `case` keyword, which pays — one per label, so a
    // fall-through run `case 2: case 3:` is two. The `default` keyword
    // is the fallback and pays nothing (lesson 11, #469); Halstead
    // bills it as the keyword it is through the byte-less arm.
    CaseArm {
        owns [CaseStatement(holds_payer)]
        pays [Case]
        excludes [Default "the fallback label, no decision (lesson 11, #469)"]
    }
    Guard { absent "C++ has no guard clause on a case or a function head" }
    // The C family's control flow (#1697): the one-kind rows are spelled
    // inline, the loop and exit lists are the two C++ grammars' own
    // fragments (`cpp_family_loops!`, `cpp_family_exits!` in
    // `roles/c_family.rs`), so Mozcpp is this table under other ids. The `ElseIf`
    // gate is emitted beside this table. `Catch` pays at the `catch`
    // keyword, which is where this dialect differs from ObjC.
    Branch { owns [IfStatement] pays [If] }
    Switch { owns [SwitchStatement] }
    Loop { owns [..cpp_family_loops!()] pays [For, While] }
    Catch { owns [CatchClause] pays [Catch] }
    ElseIf { owns [IfStatement(gated_by continues_else_clause)] pays [] }
    Exit { owns [..cpp_family_exits!()] }
    // The shared C-family lists (`roles/c_family.rs`) plus C++'s own
    // modifiers: `virtual`, `mutable`, `constinit`, `consteval`,
    // `explicit`, `friend`, the access specifiers, the `override` /
    // `final` virt-specifiers and the `noexcept` of an exception
    // specification. Each is the keyword leaf of an unclassified wrapper
    // (`virtual_specifier`, `access_specifier`,
    // `explicit_function_specifier`, the `noexcept` node that `Noexcept`
    // names and `Noexcept2` is the leaf of) or sits bare in its
    // declaration (`virtual`, `friend`), so each bills once. `override`
    // and `final` are contextual: used as a name (`int override = 1;`)
    // they parse as an `identifier`, so they need no gate. `auto` is
    // not here: in C++ it is a placeholder type, not a storage class,
    // and the grammar emits it only under `placeholder_type_specifier`.
    Modifier {
        owns [
            ..c_family_modifiers!(),
            Virtual, Mutable, Constinit, Consteval, Explicit, Friend, Public, Private,
            Protected, Override, Final, Noexcept2,
        ]
    }
    // The shared list plus `class`, `namespace`, `template`, `typename`
    // and `using` (#1552, #1557), each the leaf of an unclassified
    // wrapper (`class_specifier`, `namespace_definition`,
    // `template_declaration`, `type_parameter_declaration`,
    // `alias_declaration` / `using_declaration`). `enum class` holds two
    // of these leaves and bills both. A template-parameter `class` is
    // the synonym of `typename`, so the two spellings of
    // `template <class T>` score alike.
    DeclKeyword { owns [..c_family_decl_keywords!(), Class, Namespace, Template, Typename, Using] }
    Primitive { owns [..c_family_primitives!()] }
    // The shared C++ openers and containers (`roles/c_family.rs`); the
    // lambda is a closure that opens no space.
    FuncOpener { owns [..cpp_family_func_openers!()] }
    Closure { owns [LambdaExpression] }
    Container { owns [..cpp_family_containers!()] }
    // A `field_declaration` in a class body — a data member, or a
    // bodiless member function the method consumer tells apart by its
    // declarator (#1300). Which it is, and the access section it sits
    // in, is the consumers' reading.
    Member { owns [FieldDeclaration] }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "C++ documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
