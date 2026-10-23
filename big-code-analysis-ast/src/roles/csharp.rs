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

role_table! {
    lang: Csharp;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
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
