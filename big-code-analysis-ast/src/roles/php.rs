//! Php's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Php;
use crate::role_table;

role_table! {
    lang: Php;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { pending R05 }
    Ternary { pending R05 }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { absent "PHP has no guard clause" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // Each modifier is its keyword leaf, one kind apiece, under an
    // unlisted `*_modifier` wrapper or bare in its declaration. `var`
    // is the `var_modifier` node, the grammar's one spelling of the
    // PHP 4 property keyword. `static` and `const` are one kind each
    // for the modifier and the statement (`static $x`, `const K = 1`),
    // and `global` for the statement alone; all bill once per
    // spelling.
    Modifier {
        owns [Public, Protected, Private, Static, Final, Abstract, Readonly, VarModifier, Const, Global]
    }
    // The declaration keywords, each its leaf under an unlisted
    // `*_declaration`; `fn` opens an arrow function and `declare` a
    // `declare(strict_types=1)` block.
    DeclKeyword { owns [Function, Fn, Class, Interface, Trait, Enum, Namespace, Use, Declare] }
    // PHP bills a primitive as an *operand* — the type name is what a
    // declaration acts on, keyed by its text — and `getter/php.rs`
    // says so where it reads this row. `primitive_type` is the node
    // that carries it: the wrapper is childless for `callable`,
    // `iterable`, `mixed`, `void`, `false` and `true` (the grammar
    // emits no token node for those spellings), so the keyword leaves
    // it does hold (`int`, `bool`, `array`, `object`, `string`,
    // `float`, `null`) are the duplicate, suppressed by the getter
    // under this wrapper (#1293). `bottom_type` (`never`) and
    // `cast_type` (`(int)`) are leaves of their own and bill with it;
    // a cast never emits a keyword node (#1484).
    Primitive { owns [PrimitiveType, BottomType, CastType] }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "PHP documents with PHPDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
