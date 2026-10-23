//! Ccomment's role rows. The C-family comment-stripping helper grammar
//! recognises comments and string literals (`checker/ccomment.rs`), so
//! those three rows are pending the packages that declare them; every
//! other role names a construct the grammar cannot emit.

use crate::role_table;

role_table! {
    lang: Ccomment;
    Call { absent "the comment-stripping pass parses only comments and the string literals it must \
         not strip; it emits no such construct" }
    StringLit { pending R08 }
    Comparison { absent "the comment-stripping pass parses only comments and the string literals \
         it must not strip; it emits no such construct" }
    ShortCircuit { absent "the comment-stripping pass parses only comments and the string literals \
         it must not strip; it emits no such construct" }
    EagerLogical { absent "the comment-stripping pass parses only comments and the string literals \
         it must not strip; it emits no such construct" }
    Ternary { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    NullSafe { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    CaseArm { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    Guard { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    Branch { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    Switch { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    Loop { absent "the comment-stripping pass parses only comments and the string literals it must \
         not strip; it emits no such construct" }
    Catch { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    ElseIf { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    Exit { absent "the comment-stripping pass parses only comments and the string literals it must \
         not strip; it emits no such construct" }
    Modifier { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    DeclKeyword { absent "the comment-stripping pass parses only comments and the string literals \
         it must not strip; it emits no such construct" }
    Primitive { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    FuncOpener { absent "the comment-stripping pass parses only comments and the string literals \
         it must not strip; it emits no such construct" }
    Closure { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    Container { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    Member { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    DocTarget { absent "the comment-stripping pass parses only comments and the string literals it \
         must not strip; it emits no such construct" }
    DocRunOpener { absent "the comment-stripping pass parses only comments and the string literals \
         it must not strip; it emits no such construct" }
    DocRunDirective { absent "the comment-stripping pass parses only comments and the string \
         literals it must not strip; it emits no such construct" }
    HoldsDocInBody { absent "the comment-stripping pass parses only comments and the string \
         literals it must not strip; it emits no such construct" }
}
