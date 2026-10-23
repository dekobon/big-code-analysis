//! Bash's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Bash;
use crate::role_table;

role_table! {
    lang: Bash;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { absent "Bash's `&&` / `||` are lazy; `-a` / `-o` inside `[ ]` are test \
         operators the grammar does not split out" }
    Ternary { pending R05 }
    NullSafe { absent "Bash has no optional navigation or null coalescing" }
    CaseArm { pending R05 }
    Guard { pending R05 }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { absent "Bash has no exception handling; `trap` is a command" }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Bash declares with builtins (`local`, `readonly`), which are commands, not \
         modifier keywords" }
    // The declaration builtins the grammar gives a kind of their own:
    // `function`, and `local` / `declare` / `typeset` / `export` /
    // `readonly`, each the leaf of a `declaration_command` or a
    // `function_definition`, neither listed. `unset` / `unsetenv` are
    // the inverse and bill with the commands.
    DeclKeyword { owns [Function, Local, Declare, Typeset, Export, Readonly] }
    Primitive { absent "Bash has no type names" }
    FuncOpener { pending R06 }
    Closure { absent "Bash has no anonymous function" }
    Container { pending R06 }
    Member { absent "Bash has no container to hold a member" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Bash documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
