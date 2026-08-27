//! `Getter` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for CsharpCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Csharp::*;

        // `EnumDeclaration` maps to `SpaceKind::Class` for cross-language
        // parity with Java/PHP/Groovy (issue #429): a C# enum opens a
        // FuncSpace via `is_func_space`, so it must classify here too or
        // it falls through to `_ => SpaceKind::Unknown`.
        // A bodied indexer (#464) or property (#472) defers to its
        // `accessor_declaration` children; only the accessor-less
        // expression-bodied form opens a Function space directly. Keep this
        // gate in lockstep with `CsharpCode::is_func` / `is_func_space` so the
        // walker and the space-kind classifier agree on which nodes promote.
        if matches!(
            node.kind_id().into(),
            IndexerDeclaration | PropertyDeclaration
        ) {
            return if crate::checker::csharp_member_has_accessors(node) {
                SpaceKind::Unknown
            } else {
                SpaceKind::Function
            };
        }
        match node.kind_id().into() {
            ClassDeclaration | StructDeclaration | RecordDeclaration | EnumDeclaration => {
                SpaceKind::Class
            }
            InterfaceDeclaration => SpaceKind::Interface,
            MethodDeclaration
            | ConstructorDeclaration
            | DestructorDeclaration
            | LocalFunctionStatement
            | LambdaExpression
            | AnonymousMethodExpression
            | AccessorDeclaration
            | OperatorDeclaration
            | ConversionOperatorDeclaration => SpaceKind::Function,
            CompilationUnit => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Csharp::*;

        match node.kind_id().into() {
            // Control-flow keywords
            If | Else | Switch | Case | Default | Try | Catch | Finally | Throw
            | Return | Yield | Break | Continue | Goto | For | Foreach | While | Do
            // Declaration / namespace keywords
            | Class | Struct | Interface | Enum | Record | Delegate | Namespace | Using
            // Modifiers
            | Public | Private | Protected | Internal | Static | Abstract | Virtual
            | Override | Sealed | Partial | Readonly | Const | Extern | Unsafe
            | Volatile | Async | Required | File | New | Fixed | Implicit | Explicit
            // Expression-keyword operators
            | Await | Is | As | Typeof | Sizeof | Checked | Unchecked | Ref | Out | In
            | Params | This | Base | Lock | Stackalloc | Where | With | When | Operator
            | Scoped | Not | And | Or
            // Property/event accessor keywords
            | Get | Set | Init | Add | Remove
            // Structural punctuation
            | LBRACE | LBRACK | LPAREN | COMMA | SEMI | COLON | COLONCOLON | DOT
            | DOTDOT | EQGT | DASHGT | QMARK
            // Arithmetic / comparison / logical / bitwise / assignment operators
            | EQ | EQEQ | BANGEQ | LT | GT | LTEQ | GTEQ
            | PLUS | DASH | STAR | SLASH | PERCENT
            | AMP | PIPE | CARET | TILDE | BANG
            | AMPAMP | PIPEPIPE | QMARKQMARK
            | LTLT | GTGT | GTGTGT
            | PLUSPLUS | DASHDASH
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | AMPEQ | PIPEEQ | CARETEQ | LTLTEQ | GTGTEQ | GTGTGTEQ | QMARKQMARKEQ
            // Predefined / primitive types
            | PredefinedType
                => HalsteadType::Operator,
            // `boolean_literal: choice('true', 'false')` wraps the
            // keyword leaf, so a literal reaches the walker twice;
            // listing both kinds inflated `N2` by one per literal while
            // the text-keyed `n2` hid it (#1253). The leaf cannot simply
            // be dropped: the overloadable-operator list emits a bare
            // `true` / `false` under `operator_declaration` — the
            // grammar's only unwrapped position — and suppressing that
            // would leave two such declarations with identical Halstead
            // vocabularies whenever their bodies match. That position
            // keeps its pre-#1253 `Operand` classification here; whether
            // an overloaded operator's *name* is better counted as an
            // operator, as `operator +` already is, is #1296.
            True | False => match ancestors.parent(node).map(|p| p.kind_id().into()) {
                Some(BooleanLiteral) => HalsteadType::Unknown,
                _ => HalsteadType::Operand,
            },
            // Operands: identifiers and literals. `NullLiteral` is a
            // childless leaf, so it needs no such guard.
            //
            // `QualifiedName` (`System.Text`), `GenericName`
            // (`List<int>`) and `AliasQualifiedName` (`global::Foo`)
            // were listed here until #1263. Each is a container whose
            // every part the walker already reaches and classifies —
            // the identifier leaves as operands, `.` / `::` / `<` / `>`
            // and any `predefined_type` argument as operators — so
            // listing the container too billed one occurrence twice
            // (grammar-dispatch section 5). Probed with `bca ops`:
            // dropping them leaves `System`+`Text`+`.`,
            // `List`+`<`+`int`+`>`, and `global`+`::`+`Foo` intact.
            Identifier
            | IntegerLiteral | RealLiteral | BooleanLiteral | NullLiteral
            | CharacterLiteral | StringLiteral | VerbatimStringLiteral | RawStringLiteral
                => HalsteadType::Operand,
            // `$"..."` counts as one operand when inert. When it carries
            // any `Interpolation` child the inner expressions are
            // already walked and classified as operands; counting the
            // wrapping literal too would double-count the inner
            // identifiers' contribution to `N2` (issue #183, same
            // pattern as #180 for Elixir/Bash).
            InterpolatedStringExpression => {
                Self::string_operand_type(node, &[Interpolation as u16])
            }
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Csharp);
}
