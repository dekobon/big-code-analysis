//! `Getter` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

// A function pointer type's `<>` (`delegate*<int, void>`) encloses
// its parameter and return types (#1567).
const GENERIC_ANGLES: GenericAngleKinds = GenericAngleKinds {
    lists: &[
        Csharp::TypeArgumentList as u16,
        Csharp::TypeParameterList as u16,
        Csharp::FunctionPointerType as u16,
    ],
    openers: &[Csharp::LT as u16],
    closers: &[Csharp::GT as u16],
    is_misparse: csharp_comparison_misparse,
};

// Whether `list` holds the two comparisons of a call argument
// (`F(a < b, c > d)`) that tree-sitter-c-sharp reads as a declaration
// typed `a<b, c>` and named `d`. The C# specification keeps the type
// arguments only where the token after `>` allows it (§6.2.5), and an
// identifier allows it only after `is`, `case`, `out` or in a tuple —
// so a declaration typed by a generic name is two comparisons when it is
// a call or indexer argument without `out`. `F(out List<int> x)`,
// `(List<int> p, int q) = …` and `o is List<int> x` keep their pair. A
// member access before the `<` (`F(x.a < b, c > d)`) wraps the generic
// name in a `qualified_name`, whose final name alone can be the misparse.
// Generic qualifiers keep their type brackets. The `>=` spelling
// (`F(a < b, c >= d)`) parses as an assignment instead and is not undone.
fn csharp_comparison_misparse<'a>(list: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    let mut up = ancestors.iter(list).map(|(node, _)| node);
    let Some(name) = up
        .next()
        .filter(|name| name.kind_id() == Csharp::GenericName as u16)
    else {
        return false;
    };
    let declaration = match up.next() {
        Some(qualified) if qualified.kind_id() == Csharp::QualifiedName as u16 => {
            if qualified
                .child_by_field_name("name")
                .is_none_or(|last| last.id() != name.id())
            {
                return false;
            }
            up.next()
        }
        parent => parent,
    };
    let (Some(declaration), Some(argument), Some(arguments)) = (declaration, up.next(), up.next())
    else {
        return false;
    };
    declaration.kind_id() == Csharp::DeclarationExpression as u16
        && argument.kind_id() == Csharp::Argument as u16
        && !argument.is_child(Csharp::Out as u16)
        && matches!(
            arguments.kind_id().into(),
            Csharp::ArgumentList | Csharp::BracketedArgumentList
        )
}

// Whether `node` is the `,` opening an interpolation's alignment
// (`$"{x,8}"`) or the `:` opening its format string (`$"{x:F2}"`).
// Both delimit the field, as the `{` / `}` braces do, which #1314 left
// unbilled in every interpolating language; Python's format-spec `:`
// went the same way in #1486 (#1573). The guard reads the parent, so a
// ternary `:`, a named-argument `:` or an argument-list `,` written
// inside the hole (`$"{f(a, b)}"`) keeps billing.
//
// The alignment *value* is a `constant_expression` and walks as one:
// `{x,8}` bills `8` and `{x,-W}` bills `-` and `W`. That is where C#
// and Python differ: a Python literal width (`f"{x:>10}"`) is
// format-spec text and bills nothing, while C#'s `8` is an
// `integer_literal` token. An expression width bills in both
// (`f"{x:>{w}}"` bills `w`). The format string after `:` (`F2`) is
// string content and has no node to bill.
//
// On invalid input with no expression before the delimiter
// (`$"{x,}"`, `$"{:F2}"`) error recovery leaves the `,` / `:` outside
// any clause node, so it bills. Valid C# always has both, so this is
// documented rather than tested (grammar-dispatch §6).
fn is_interpolation_clause_delimiter<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    let clause = match node.kind_id().into() {
        Csharp::COLON => Csharp::InterpolationFormatClause,
        Csharp::COMMA => Csharp::InterpolationAlignmentClause,
        _ => return false,
    };
    ancestors.parent_has_kind(node, clause as u16)
}

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

    keyword_roles!(csharp);

    paying_roles!(csharp);

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Csharp::*;

        if GENERIC_ANGLES.is_closer(node, ancestors)
            || is_interpolation_clause_delimiter(node, ancestors)
        {
            return TokenRole::Unknown;
        }

        match node.kind_id().into() {
            // Control-flow keywords
            // `case` stays for the spelling the `CaseArm` row's gate
            // leaves out — `goto case 2;` (#1450) — which billed before
            // the row existed; a switch arm's is the row's. `default` is
            // the fallback label's and stays; `when`, the `Guard` row's
            // owners' keyword, is excluded there and bills here. `is`,
            // `<` … and `?` likewise stay for the spellings the
            // `Comparison` and `Ternary` rows' gates leave out.
            // role-bypass: allow(Comparison, Ternary, CaseArm) — the operator classification of
            // the spellings the rows' `under_type_test` / `under_binary_expression` /
            // `under_ternary` / `is_switch_arm_case` gates leave out
            If | Else | Switch | Case | Default | Try | Catch | Finally | Throw
            | Return | Yield | Break | Continue | Goto | For | Foreach | While | Do
            // Expression-keyword operators. `new` is the allocation
            // keyword and the hiding modifier in one kind. The
            // modifiers, the declaration keywords and `predefined_type`
            // are not here: `roles/csharp.rs` declares them and
            // `get_keyword_role` bills them (#1693).
            // `is` for the spelling the `Comparison` row's `under_type_test`
            // gate leaves out — a keyword error recovery strands — which
            // billed before the row existed.
            | Is
            | New | Await | As | Typeof | Sizeof | Checked | Unchecked | Ref | Out | In
            | Params | Lock | Stackalloc | Where | With | When | Operator
            | Scoped | Not | And | Or
            // Property/event accessor keywords
            | Get | Set | Init | Add | Remove
            // Structural punctuation. `?` stays for the spellings the
            // `Ternary` row's `under_ternary` gate leaves out — a
            // conditional access (`a?.b`, the `NullSafe` row's), a
            // nullable type, a constraint's `class?` — which billed before
            // the row existed.
            | LBRACE | LBRACK | LPAREN | COMMA | SEMI | COLON | COLONCOLON | DOT
            | DOTDOT | EQGT | DASHGT | QMARK
            // Arithmetic / logical / bitwise / assignment operators. The
            // comparison tokens, `is`, `&&` and `||` are the `Comparison`
            // and `ShortCircuit` rows' and `get_paying_role` bills them
            // (#1695); the six comparison tokens stay for the spellings
            // the row's `under_binary_expression` gate leaves out — a
            // generic bracket (the `<>` pair; its closer returned
            // `Unknown` above), an operator declaration's name, a
            // relational pattern, a recovered token — which billed
            // before the row existed.
            | EQ | EQEQ | BANGEQ | LT | GT | LTEQ | GTEQ
            | PLUS | DASH | STAR | SLASH | PERCENT
            | AMP | PIPE | CARET | TILDE | BANG
            | QMARKQMARK
            | LTLT | GTGT | GTGTGT
            | PLUSPLUS | DASHDASH
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | AMPEQ | PIPEEQ | CARETEQ | LTLTEQ | GTGTEQ | GTGTGTEQ | QMARKQMARKEQ
                => TokenRole::Operator,
            // `boolean_literal: choice('true', 'false')` wraps the
            // keyword leaf, so a literal reaches the walker twice;
            // listing both kinds inflated `N2` by one per literal while
            // the text-keyed `n2` hid it (#1253). The leaf cannot simply
            // be dropped: the overloadable-operator list emits a bare
            // `true` / `false` under `operator_declaration` — the
            // grammar's only unwrapped position. There the keyword
            // *names* an operator, exactly as the `+` of `operator +`
            // does, so it is billed as one (#1296); C# merely spells two
            // of its overloadable operators with keywords instead of
            // punctuation. The fallback keeps any other unwrapped
            // position an operand, the role the keyword has as a value.
            True | False => match ancestors.parent(node).map(|p| p.kind_id().into()) {
                Some(BooleanLiteral) => TokenRole::Unknown,
                Some(OperatorDeclaration) => TokenRole::Operator,
                _ => TokenRole::Operand,
            },
            // `this` is a self-reference everywhere (`this.x`,
            // `this[i]`, `: this(1)`, `f(this)`) except directly under
            // an `indexer_declaration`, where the keyword *names* the
            // member being declared rather than denoting a value —
            // `public int this[int i] { … }`. That position keeps its
            // operator classification alongside the `operator` keyword
            // of an overload declaration, which this match already
            // bills as an operator (#1380). The extension-method
            // receiver (`static void M(this Foo f)`) is a childless
            // `modifier` node, kind 249, not this kind at all; it is
            // classified — as an operand, so the two positions agree —
            // by `get_op_type_with_code` below (#1418).
            //
            // Java's receiver parameter (`void m(J J.this)`) is the
            // other declarator use of the keyword in this workspace and
            // `java.rs` calls it an operand — a parameter *name* is an
            // operand in every language here, where a member named by a
            // keyword belongs with `operator +`.
            This => match ancestors.parent(node).map(|p| p.kind_id().into()) {
                Some(IndexerDeclaration) => TokenRole::Operator,
                _ => TokenRole::Operand,
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
            //
            // `Base` joined this list in #1380 for the same structural
            // reason `This` did: it is the receiver of `base.M()` /
            // `base[i]` / `: base(x)`. The `base_list` container that
            // spells inheritance (`class D : B`) holds the base type's
            // identifier and no `base` keyword, so nothing bills this
            // text twice.
            //
            // `ImplicitParameter` is the parameter of `x => x`: a
            // childless leaf where `(x) => x` puts an `identifier` inside
            // a `parameter_list`, so the bare spelling dropped its name
            // from both Halstead halves (#1544). Nothing containing it
            // is classified.
            Identifier | Base | ImplicitParameter
            | IntegerLiteral | RealLiteral | BooleanLiteral | NullLiteral
            | CharacterLiteral | StringLiteral | VerbatimStringLiteral | RawStringLiteral
                => TokenRole::Operand,
            // `$"..."` counts as one operand when inert. When it carries
            // any `Interpolation` child the inner expressions are
            // already walked and classified as operands; counting the
            // wrapping literal too would double-count the inner
            // identifiers' contribution to `N2` (issue #183, same
            // pattern as #180 for Elixir/Bash).
            InterpolatedStringExpression => {
                Self::string_operand_type(node, &[Interpolation as u16])
            }
            _ => TokenRole::Unknown,
        }
    }

    // The parameter-position spelling of six keywords (#1418). The
    // grammar's `_parameter_type_with_modifiers` rule aliases the bare
    // tokens `this scoped ref out in readonly` to `$.modifier`, so in
    // that one position each is a **childless** `modifier` node rather
    // than the kind its own arm above lists; `_lambda_expression_init`
    // and `anonymous_method_expression` alias a bare `static` / `async`
    // the same way (#1482). Nothing billed them, so an extension
    // receiver, a `ref` parameter and an `out` parameter each
    // contributed to neither Halstead half.
    //
    // The role follows the text, never the position: each keyword keeps
    // the role its own kind carries elsewhere, so `M(ref Foo f)` and
    // `f(ref x)` agree, and `this` stays the operand #1380 made it. The
    // keyword spellings are the `Modifier` row's text-gated entry in
    // `roles/csharp.rs` (#1693), which carries the child-presence
    // argument; this override keeps only the operand half, which no
    // row expresses. That decision's `IndexerDeclaration` carve-out
    // cannot arise here — an indexer declarator is not a parameter.
    //
    // Reading the bytes needs this spelling, which is the one the walk
    // calls (`compute_halstead` -> `get_op_type_with_code`), so the
    // classification reaches every count (grammar-dispatch section 7).
    //
    // `CsharpCode::is_primitive` routes the operator half through the
    // lexeme-keyed map, because these seven keywords have two kind
    // spellings for one operator; its comment carries that argument.
    fn get_op_type_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> TokenRole {
        // The aliased `this` is the operand `this` is everywhere; the
        // keyword spellings are the `Modifier` row's text-gated
        // `Modifier` entry, which `get_keyword_role` bills first, so a
        // spelling the row does not name falls through here to
        // `Unknown` — `get_op_type` lists no `Modifier` arm.
        if csharp_is_aliased_modifier(node) && node_text(code, node) == Some("this") {
            return TokenRole::Operand;
        }
        keyword_or_op_type::<Self>(node, code, ancestors)
    }

    fn get_operator_spelling<'a>(
        node: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
    ) -> Option<&'static str> {
        GENERIC_ANGLES.opener_spelling(node, ancestors)
    }

    get_operator!(Csharp);
}
