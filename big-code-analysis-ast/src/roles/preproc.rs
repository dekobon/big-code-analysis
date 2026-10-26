//! Preproc's role rows. The C-family preprocessor helper grammar
//! recognises comments and string literals (`checker/preproc.rs`), so
//! `StringLit` is declared and the two comment rows are pending the
//! packages that declare them; every other role names a construct the
//! grammar cannot emit.

use crate::{Preproc, role_table};

role_table! {
    lang: Preproc;
    Call { absent "the preprocessor pass parses only directives, comments and the string literals \
         they carry; it emits no such construct" }
    // The literals the pass must not strip or read into: `find string`
    // and the byte-less `is_string` both read this kind table (#1698).
    StringLit { owns [StringLiteral, RawStringLiteral] }
    Comparison { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    ShortCircuit { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    EagerLogical { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Ternary { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    NullSafe { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    CaseArm { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Guard { absent "the preprocessor pass parses only directives, comments and the string literals \
         they carry; it emits no such construct" }
    Branch { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Switch { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Loop { absent "the preprocessor pass parses only directives, comments and the string literals \
         they carry; it emits no such construct" }
    Catch { absent "the preprocessor pass parses only directives, comments and the string literals \
         they carry; it emits no such construct" }
    ElseIf { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Exit { absent "the preprocessor pass parses only directives, comments and the string literals \
         they carry; it emits no such construct" }
    Modifier { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    DeclKeyword { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Primitive { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    FuncOpener { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Closure { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Container { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Member { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    DocTarget { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    DocRunOpener { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
    DocRunDirective { absent "the preprocessor pass parses only directives, comments and the \
         string literals they carry; it emits no such construct" }
    HoldsDocInBody { absent "the preprocessor pass parses only directives, comments and the string \
         literals they carry; it emits no such construct" }
}
