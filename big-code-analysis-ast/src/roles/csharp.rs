//! Csharp's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::checker::csharp_is_aliased_modifier;
use crate::getter::node_text;
use crate::role_table;
use crate::{Ancestors, Csharp, Node};

/// Whether a childless `modifier` node — the grammar's alias for a
/// bare keyword in a parameter (`ref` / `out` / `in` / `scoped` /
/// `readonly`, #1418) or on a lambda (`static` / `async`, #1482) —
/// spells one of those keywords. The aliased `this` of an extension
/// receiver (`M(this Foo f)`) is not one: it is the operand `this` is
/// everywhere, and `CsharpCode::get_op_type_with_code` bills it so
/// *before* reading this row, which is why adding `"this"` to the list
/// below changes no output — measured under #1693; the ordering, not
/// the list, is what keeps it an operand. A `modifier` *with* a child
/// wraps a keyword leaf the row already owns and is nothing itself. A
/// grammar bump that aliases another spelling lands here unclaimed
/// rather than billed as whichever word happened to be last.
fn aliases_a_modifier_keyword<'a>(
    node: &Node<'a>,
    code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    csharp_is_aliased_modifier(node)
        && matches!(
            node_text(code, node),
            Some("ref" | "out" | "in" | "scoped" | "readonly" | "static" | "async")
        )
}

/// Whether a comparison token applies its operator: its parent is a
/// `binary_expression` (either alias). A `grammar.json` sweep of
/// tree-sitter-c-sharp 0.23.5 finds each token in these productions,
/// and only `binary_expression` (`a < b`) qualifies:
///
/// - `<` / `>` — `binary_expression`, `relational_pattern`,
///   `operator_declaration`, `type_argument_list`,
///   `type_parameter_list`, `function_pointer_type`
/// - `<=` / `>=` — `binary_expression`, `relational_pattern`,
///   `operator_declaration`
/// - `==` / `!=` — `binary_expression`, `preproc_binary_expression`
///   (`#if A == B`, parsed as `BinaryExpression` at this pin),
///   `operator_declaration`
///
/// Three reasons to exclude, one per family. The generic brackets and
/// `delegate*<int, int>` are type syntax, no more a decision than the
/// `?` of a nullable type (#1275). An `operator_declaration` names the
/// operator being *defined* rather than applying it; #1297 found this
/// for `<` / `>`, and the other four still scored 1 against their 0 on
/// a class overloading all six, every member of which has
/// `cyclomatic()` 1 (#1420). A `relational_pattern`'s operator is not
/// its own decision: the enclosing `switch_expression_arm` or `if`
/// slot already scores one, so counting it charged `x switch { > 5 =>
/// … }` twice what the constant arm scores (#1383). Allowlist
/// polarity, so a grammar bump that grows a seventh production fails
/// closed, and a token error recovery reparents under `{ERROR}` stops
/// counting — valid input is unaffected and no C# corpus file parses
/// with an `{ERROR}` node, measured.
fn under_binary_expression<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent(node).is_some_and(|p| {
        p.kind_id() == Csharp::BinaryExpression as u16
            || p.kind_id() == Csharp::BinaryExpression2 as u16
    })
}

/// Whether a `binary_expression`'s `operator` field is one of the
/// `Comparison` payers.
fn comparison_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_some_and(|op| comparison::PAYERS.contains(&op.kind_id()))
}

/// Whether a `binary_expression`'s `operator` field is `&&` or `||`.
fn short_circuit_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    short_circuit::project(node).is_some_and(|op| short_circuit::PAYERS.contains(&op.kind_id()))
}

role_table! {
    lang: Csharp;
    Call { pending R08 }
    StringLit { pending R08 }
    // A `binary_expression` (both aliases: `BinaryExpression2` is the
    // second id carrying the kind string, listed per grammar-dispatch
    // §1 and unreachable at this pin, where `#if A == B` parses as the
    // first) owns a comparison through its `operator` field, and every
    // comparison token is gated to that parent. The type tests
    // `is_expression` (`o is T`) and `is_pattern_expression` (`o is T
    // t`, `o is not null`) are disjoint alternatives that each wrap one
    // `is` keyword, which pays for them; the grammar spells `is`
    // nowhere else. `??` is `NullSafe`'s and the `and` / `or` / `not`
    // pattern combinators are no comparison.
    Comparison {
        owns [
            BinaryExpression(gated_by comparison_operator),
            BinaryExpression2(gated_by comparison_operator),
            IsExpression, IsPatternExpression,
        ]
        pays [
            EQEQ gated_by under_binary_expression, BANGEQ gated_by under_binary_expression,
            LTEQ gated_by under_binary_expression, GTEQ gated_by under_binary_expression,
            LT gated_by under_binary_expression, GT gated_by under_binary_expression,
            Is,
        ]
        project "operator"
    }
    ShortCircuit {
        owns [
            BinaryExpression(gated_by short_circuit_operator),
            BinaryExpression2(gated_by short_circuit_operator),
        ]
        pays [AMPAMP, PIPEPIPE]
        project "operator"
    }
    EagerLogical { pending R05 }
    Ternary { pending R05 }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { pending R05 }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // Every modifier keyword leaf, plus the childless `modifier` alias
    // the grammar emits for a bare keyword in a parameter or on a
    // lambda, under its text gate. `ref` / `out` / `in` / `scoped` are
    // listed here as the aliased spelling only: as their own kinds they
    // are the expression keywords of `ref x` / `out var y` / `in` / a
    // `scoped` local, and bill with `await` and `is`. `new` is the
    // hiding modifier and the allocation keyword in one kind, billed
    // with the expression keywords for the same reason. The `modifier`
    // rule with a child wraps a leaf this row owns and is nothing
    // itself (grammar-dispatch section 5).
    Modifier {
        owns [
            Public, Private, Protected, Internal, Static, Abstract, Virtual, Override, Sealed,
            Partial, Readonly, Const, Extern, Unsafe, Volatile, Async, Required, File, Fixed,
            Implicit, Explicit, Modifier(text_gated_by aliases_a_modifier_keyword),
        ]
    }
    // The declaration and namespace keywords, each its leaf under an
    // unclassified `*_declaration` (#1552). `using` bills, and so does
    // the `static` of `using static` through the row above: C# bills
    // its directive keywords where Java and Groovy bill none of theirs
    // (#1600).
    DeclKeyword { owns [Class, Struct, Interface, Enum, Record, Delegate, Namespace, Using] }
    // `predefined_type` is one kind for every spelling (`int`, `string`,
    // `bool`, …), so `Checker::is_primitive` keys it by text (#286).
    // `var` is excluded: it names no type, and the `implicit_type`
    // wrapping it bills nothing.
    Primitive {
        owns [PredefinedType]
        excludes [Var "`var` names no type; its `implicit_type` wrapper bills nothing"]
    }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "C# documents with `///` comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
