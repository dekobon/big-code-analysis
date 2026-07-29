//! `Getter` implementation for Groovy.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for GroovyCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Groovy::{
            AnnotationTypeDeclaration, ClassDeclaration, Closure, ConstructorDeclaration,
            EnumDeclaration, InterfaceDeclaration, MethodDeclaration, RecordDeclaration,
            SourceFile, TraitDeclaration,
        };

        // Mirrors `impl Getter for JavaCode::get_space_kind` for class/
        // method shapes (issue #280, lesson 11). `Closure` tags as
        // `Function` because Groovy closures are first-class callable
        // bodies, the same way Java's `LambdaExpression` is tagged.
        // The new dekobon grammar models `TraitDeclaration` as a
        // distinct node (the prior amaanq grammar mis-parsed `trait`
        // as `juxt_function_call` + `closure` — see #247); it gets
        // `Interface` because Groovy traits are interfaces with default
        // method bodies.
        // Groovy anonymous classes (`new Runnable() { ... }`) get no
        // Class space here, unlike Java (#463): the pinned dekobon
        // grammar does not attach the body to the
        // `object_creation_expression`. It parses `new Runnable()` as a
        // bare `object_creation_expression` and the following `{ ... }`
        // as a separate `closure`, so the members already land in that
        // closure's `Function` space rather than being mis-attributed to
        // the enclosing method. This is an upstream-grammar limitation,
        // not a wrapper bug; adding an `ObjectCreationExpression` +
        // `class_body` arm here (as Java does) would be permanently
        // inert under this grammar. Revisit if/when the grammar is
        // bumped to model anonymous-class bodies as `class_body`.
        match node.kind_id().into() {
            ClassDeclaration | EnumDeclaration | RecordDeclaration => SpaceKind::Class,
            InterfaceDeclaration | TraitDeclaration | AnnotationTypeDeclaration => {
                SpaceKind::Interface
            }
            MethodDeclaration | ConstructorDeclaration | Closure => SpaceKind::Function,
            SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Groovy::*;
        // Mirrors `JavaCode`'s minimal classification — modifiers
        // (`Public`, `Static`, …), declaration keywords (`Class`,
        // `Interface`, …), and module keywords (`Package`, `Import`,
        // …) are excluded because they live inside `Modifiers` /
        // `*Declaration` wrappers and would over-count if treated as
        // separate operators. The dekobon Groovy grammar (#246, #247)
        // emits a distinct named node for every Groovy-specific
        // operator (Elvis `?:`, safe-nav `?.`, identity `===`/`!==`,
        // regex `=~`/`==~`, spaceship `<=>`, exclusive ranges
        // `..<` / `<..` / `<..<`, `as` coercion, etc.); their leaf
        // tokens are listed here as operators so Halstead counts the
        // tokens directly rather than the wrapping expression node.
        // `NumberLiteral` is the new grammar's consolidated numeric
        // literal — the prior grammar split numbers by radix
        // (Hex/Octal/Binary/Decimal Integer/Float).
        match node.kind_id().into() {
            // Control-flow + keyword operators (mirrors Java's set,
            // minus tokens that no longer exist in the dekobon grammar
            // — `This`, `VoidType`, `Throws2`).
            If | Else | Switch | Case | Try | Catch | Throw | Throws | For | While | Continue
            | Break | Do | Finally | New | Return | Default | Abstract | Assert | Instanceof
            | Extends | Final | Implements | Transient | Synchronized | Super | Def | In | As
            // Separators / brackets.
            | SEMI | COMMA | COLONCOLON | DOT | DASHGT | LBRACE | LBRACK | LPAREN
            // Java-compatible operators (arithmetic, bitwise, comparison, assignment).
            | EQ | LT | GT | BANG | TILDE | QMARK | COLON | EQEQ | LTEQ | GTEQ | BANGEQ
            | AMPAMP | PIPEPIPE | PLUSPLUS | DASHDASH | PLUS | DASH | STAR | SLASH | AMP
            | PIPE | CARET | PERCENT | LTLT | GTGT | GTGTGT | PLUSEQ | DASHEQ | STAREQ
            | SLASHEQ | AMPEQ | PIPEEQ | CARETEQ | PERCENTEQ | LTLTEQ | GTGTEQ | GTGTGTEQ
            | STARSTAR | STARSTAREQ
            // Groovy-specific operator tokens added by the dekobon
            // grammar (closes #247): ranges `..` / `..<` / `<..` /
            // `<..<`, Elvis `?:` and Elvis-assign `?=`, safe-nav `?.`,
            // safe-chain `??.`, spread-dot `*.`, method-pointer `.&`,
            // direct-field `.@`, safe-index `?[`, identity `===` /
            // `!==`, spaceship `<=>`, regex `=~` / `==~`, logical
            // implication `==>`, and spread-map `*:`.
            | DOTDOT | DOTDOTLT | LTDOTDOT | LTDOTDOTLT | QMARKCOLON | QMARKEQ | QMARKDOT
            | QMARKQMARKDOT | STARDOT | DOTAMP | DOTAT | QMARKLBRACK | EQEQEQ | BANGEQEQ
            | LTEQGT | EQTILDE | EQEQTILDE | EQEQGT | STARCOLON => HalsteadType::Operator,

            Identifier | TypeIdentifier | QualifiedName | QualifiedType | NullLiteral | True
            | False | NumberLiteral => HalsteadType::Operand,

            // A Groovy GString interpolates inner expressions whose
            // operands are walked and counted separately, so the
            // wrapping literal must yield Unknown to avoid double-
            // counting (issue #454, same mechanism as Kotlin #191 /
            // PHP #184 / Elixir #180, generalized in #420). The dekobon
            // grammar emits two interpolation child kinds: the braced
            // long form `${expr}` (`gstring_brace_interpolation`) and
            // the short `$name` / `$obj.field` form
            // (`gstring_dollar_interpolation`). A plain non-interpolated
            // literal has neither child and stays a single operand.
            StringLiteral => Self::string_operand_type(
                node,
                &[
                    GstringBraceInterpolation as u16,
                    GstringDollarInterpolation as u16,
                ],
            ),

            _ => HalsteadType::Unknown,
        }
    }

    fn get_operator_id_as_str(id: u16) -> &'static str {
        let typ = id.into();
        match typ {
            Groovy::LPAREN => "()",
            Groovy::LBRACK => "[]",
            Groovy::LBRACE => "{}",
            _ => typ.into(),
        }
    }
}
