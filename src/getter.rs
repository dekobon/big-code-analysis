// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use crate::metrics::halstead::HalsteadType;

use crate::spaces::SpaceKind;
use crate::traits::Search;

use crate::*;

macro_rules! get_operator {
    ($language:ident) => {
        #[inline]
        fn get_operator_id_as_str(id: u16) -> &'static str {
            let typ = id.into();
            match typ {
                $language::LPAREN => "()",
                $language::LBRACK => "[]",
                $language::LBRACE => "{}",
                _ => typ.into(),
            }
        }
    };
}

// Emit a `Getter::get_op_type` body for a JS-family language. The four
// JS-family grammars (JavaScript, MozJS, TypeScript, TSX) share most of
// their Halstead operator/operand kind classifications; per-language
// deltas are passed as bracketed extras so all four impls stay in
// lockstep when a kind is added or removed (issue #299).
//
// `$op_extras` per language:
//   * JavaScript / MozJS: `OptionalChain` — the bare `?.` token (these
//     grammars expose no `optional_chain` wrapper).
//   * TypeScript / TSX:   `QMARKDOT`, `PredefinedType` — `QMARKDOT` is
//     the bare `?.` token under the `optional_chain` wrapper (issue
//     #281); `PredefinedType` is the TS type keyword set (`string`,
//     `number`, `boolean`, …).
//
// `$operand_extras` per language:
//   * JavaScript / MozJS: `Identifier2`, `String2` — anonymous keyword
//     aliases the JS grammar exposes for `Identifier` and `String`.
//   * TypeScript: `String2`, `NestedIdentifier`, `MemberExpression4`.
//     `String2` is the TS-only anonymous `"string"` alias the grammar
//     emits for the `string` type-annotation keyword (kind_id 135, in
//     the type-keyword range alongside `Boolean` / `Symbol`); it must
//     be in `operand_extras` to agree with `Checker::is_string` which
//     also matches it (issue #313, parallel to #283).
//     `NestedIdentifier` and `MemberExpression4` are other TS-only
//     productions.
//   * TSX: union of the above plus `String3`. TSX uniquely exposes
//     *two* anonymous `"string"` aliases: `String2` (kind_id 261, the
//     string-literal alias) and `String3` (kind_id 141, the
//     type-annotation keyword — the role TS's `String2` plays).
//     `Checker::is_string` matches both, so both must be operands
//     (#313).
//
// The `TemplateString` interpolation guard is shared verbatim (issue
// #192): a bare `` `...` `` mirrors a `"..."` operand, but an
// interpolated template must yield `Unknown` because its inner
// `TemplateSubstitution` expressions are walked separately.
macro_rules! impl_js_family_get_op_type {
    (
        $lang:ident,
        op_extras: [$($op_extra:ident),* $(,)?],
        operand_extras: [$($operand_extra:ident),* $(,)?] $(,)?
    ) => {
        fn get_op_type(node: &Node) -> HalsteadType {
            use $lang::*;

            match node.kind_id().into() {
                Export | Import | Import2 | Extends | DOT | From | LPAREN | COMMA | As | STAR
                | GTGT | GTGTGT | COLON | Return | Delete | Throw | Break | Continue | If
                | Else | Switch | Case | Default | Async | Do | For | In | Of | While | Try
                | Catch | Finally | With | EQ | AT | AMPAMP | PIPEPIPE | PLUS | DASH | DASHDASH
                | PLUSPLUS | SLASH | PERCENT | STARSTAR | PIPE | AMP | LTLT | TILDE | LT | LTEQ
                | EQEQ | BANGEQ | GTEQ | GT | PLUSEQ | BANG | BANGEQEQ | EQEQEQ | DASHEQ
                | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ | GTGTEQ | GTGTGTEQ | LTLTEQ | AMPEQ
                | CARET | CARETEQ | PIPEEQ | Yield | LBRACK | LBRACE | Await | QMARK
                | QMARKQMARK | EQGT | DOTDOTDOT | New | Let | Var | Const | Function
                | FunctionExpression | SEMI | Typeof | Instanceof | Void
                $(| $op_extra)* => HalsteadType::Operator,
                Identifier | MemberExpression | MemberExpression2 | MemberExpression3
                | PropertyIdentifier | String | Number | True | False | Null | This | Super
                | Undefined | Set | Get
                $(| $operand_extra)* => HalsteadType::Operand,
                // A `` `...` `` is a string literal; without interpolation it
                // mirrors `"..."` and contributes one operand. When it has a
                // `TemplateSubstitution` child the inner expression is already
                // walked and classified, so counting the wrapper too would
                // double-count its contribution to `N2` (issue #192, same
                // pattern as #183 C# / #191 Kotlin / #199 Perl).
                TemplateString => {
                    Self::string_operand_type(node, &[TemplateSubstitution as u16])
                }
                _ => HalsteadType::Unknown,
            }
        }
    };
}

#[doc(hidden)]
pub trait Getter {
    fn get_func_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        Self::get_func_space_name(node, code)
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        // we're in a function or in a class
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            Some("<anonymous>")
        }
    }

    fn get_space_kind(_node: &Node) -> SpaceKind {
        SpaceKind::Unknown
    }

    /// Source-aware variant of [`get_space_kind`]. The default
    /// forwards to the byte-less classifier; languages whose space
    /// kinds are encoded in macro identifier text (Elixir's
    /// `defmodule` / `def` / `defp` / `defmacro` / `defmacrop` Calls)
    /// override this so the walker can attribute the correct
    /// `SpaceKind` to each promoted func space (#275).
    #[inline]
    fn get_space_kind_with_code(node: &Node, _code: &[u8]) -> SpaceKind {
        Self::get_space_kind(node)
    }

    fn get_op_type(_node: &Node) -> HalsteadType {
        HalsteadType::Unknown
    }

    /// Classifies a string-literal `node` as a single Halstead
    /// operand, *unless* it wraps an interpolation child drawn from
    /// `interp_kinds` — in which case the wrapper yields
    /// [`HalsteadType::Unknown`] because the inner expressions are
    /// walked and counted separately. Counting the wrapper too would
    /// double-count their contribution to `N2`.
    ///
    /// This declares the per-language interpolation skip once (issue
    /// #420), replacing nine independently-added regression fixes
    /// (#183 / #184 / #191 / #192 / #199 / #277, …). Each language
    /// supplies only its own grammar's interpolation child-kind ids;
    /// the per-call rationale lives at each call site.
    fn string_operand_type(node: &Node, interp_kinds: &[u16]) -> HalsteadType {
        if node.wraps_any(interp_kinds) {
            HalsteadType::Unknown
        } else {
            HalsteadType::Operand
        }
    }

    fn get_operator_id_as_str(_id: u16) -> &'static str {
        ""
    }
}

impl Getter for PythonCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Python::FunctionDefinition => SpaceKind::Function,
            Python::ClassDefinition => SpaceKind::Class,
            Python::Module => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Python::*;

        match node.kind_id().into() {
            // The `not` / `in` / `is` leaf tokens are operators on their own
            // (`not x`, `a in b`, `a is b`, `for x in y`), but the grammar
            // also nests them inside the compound `not in` (Notin) and
            // `is not` (Isnot) nodes. When the leaf's parent is one of those
            // compounds, the compound itself is classified as the single
            // operator below, so the leaf must yield Unknown — otherwise
            // `a not in b` would count `not` + `in` as two operators (#413).
            Not | In | Is => match node.parent().map(|p| p.kind_id().into()) {
                Some(Notin | Isnot) => HalsteadType::Unknown,
                _ => HalsteadType::Operator,
            },
            Import | DOT | From | COMMA | As | STAR | GTGT | Assert | COLONEQ | Return | Def
            | Del | Raise | Pass | Break | Continue | If | Elif | Else | Async | For
            | While | Try | Except | Finally | With | DASHGT | EQ | Global | Nonlocal | Exec
            | AT | And | Or | PLUS | DASH | SLASH | PERCENT | SLASHSLASH | STARSTAR | PIPE
            | AMP | CARET | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | LTGT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | ATEQ | SLASHSLASHEQ | PERCENTEQ | STARSTAREQ
            | GTGTEQ | LTLTEQ | AMPEQ | CARETEQ | PIPEEQ | Yield | Print
            // `not in` / `is not` compounds count as one operator each; the
            // inner Not/In/Is leaves are suppressed by the parent-guard arm
            // above (#413).
            | Notin | Isnot
            // `match` / `case` keyword tokens (Match=26, Case=27), mirroring
            // the cyclomatic metric which already counts each `case` clause
            // and the Rust Halstead which counts `match` (#413).
            | Match | Case
            // `nonlocal` keyword token, for parity with `global` which was
            // already classified (#413).
            // `await`: count only the await-expression node (Await=237);
            // Await2 (keyword token 95) is the nested keyword and was being
            // double-counted, mirroring how `yield` counts only the Yield
            // node and not its keyword leaf (#413).
            | Await
            // `lambda`: count only the keyword token (Lambda3=73), not the
            // Lambda/Lambda2 expression nodes that wrap it, to avoid the same
            // node+keyword double count fixed for await (#413).
            | Lambda3 => {
                HalsteadType::Operator
            }
            Identifier | Integer | Float | True | False | None => HalsteadType::Operand,
            String => {
                // Docstring / module-level string statement: an `ExpressionStatement`
                // whose only child is the string. Skip those.
                let Some(parent) = node.parent() else {
                    return HalsteadType::Unknown;
                };
                if parent.kind_id() == ExpressionStatement && parent.child_count() == 1 {
                    return HalsteadType::Unknown;
                }
                // Regression #191: an f-string wraps `Interpolation` children
                // whose inner expressions are walked and counted separately.
                // Skip the wrapping literal to avoid double-counting (same
                // pattern as #180 for Bash/Elixir and #184 for PHP).
                Self::string_operand_type(node, &[Interpolation as u16])
            }
            _ => HalsteadType::Unknown,
        }
    }

    fn get_operator_id_as_str(id: u16) -> &'static str {
        Into::<Python>::into(id).into()
    }
}

impl Getter for MozjsCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Mozjs::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction => SpaceKind::Function,
            Class | ClassDeclaration => SpaceKind::Class,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = node.parent() {
                match parent.kind_id().into() {
                    Mozjs::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    Mozjs::VariableDeclarator => {
                        if let Some(name) = parent.child_by_field_name("name") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    _ => {}
                }
            }
            Some("<anonymous>")
        }
    }

    impl_js_family_get_op_type!(
        Mozjs,
        op_extras: [OptionalChain],
        operand_extras: [Identifier2, String2],
    );

    get_operator!(Mozjs);
}

impl Getter for JavascriptCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Javascript::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction => SpaceKind::Function,
            Class | ClassDeclaration => SpaceKind::Class,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = node.parent() {
                match parent.kind_id().into() {
                    Javascript::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    Javascript::VariableDeclarator => {
                        if let Some(name) = parent.child_by_field_name("name") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    _ => {}
                }
            }
            Some("<anonymous>")
        }
    }

    impl_js_family_get_op_type!(
        Javascript,
        op_extras: [OptionalChain],
        operand_extras: [Identifier2, String2],
    );

    get_operator!(Javascript);
}

impl Getter for TypescriptCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Typescript::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction => SpaceKind::Function,
            Class | ClassDeclaration | AbstractClassDeclaration => SpaceKind::Class,
            InterfaceDeclaration => SpaceKind::Interface,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = node.parent() {
                match parent.kind_id().into() {
                    Typescript::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    Typescript::VariableDeclarator => {
                        if let Some(name) = parent.child_by_field_name("name") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    _ => {}
                }
            }
            Some("<anonymous>")
        }
    }

    // TS exposes `String2` as the anonymous `"string"` alias for the
    // type-annotation keyword (kind_id 135, in the type-keyword block
    // of the enum). `Checker::is_string` already matches it (#283);
    // including it here closes the Checker/Getter agreement gap
    // (#313). `NestedIdentifier` and `MemberExpression4` are TS-only
    // member-expression productions.
    impl_js_family_get_op_type!(
        Typescript,
        op_extras: [QMARKDOT, PredefinedType],
        operand_extras: [String2, NestedIdentifier, MemberExpression4],
    );

    get_operator!(Typescript);
}

impl Getter for TsxCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Tsx::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction => SpaceKind::Function,
            Class | ClassDeclaration | AbstractClassDeclaration => SpaceKind::Class,
            InterfaceDeclaration => SpaceKind::Interface,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = node.parent() {
                match parent.kind_id().into() {
                    Tsx::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    Tsx::VariableDeclarator => {
                        if let Some(name) = parent.child_by_field_name("name") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    _ => {}
                }
            }
            Some("<anonymous>")
        }
    }

    // TSX exposes two anonymous `"string"` aliases: `String2` (the
    // string-literal alias, kind_id 261) and `String3` (the
    // type-annotation keyword, kind_id 141 — the role TS's `String2`
    // plays). `Checker::is_string` matches both (#283); including
    // `String3` here closes the audit gap surfaced by #313 (the same
    // class of inconsistency that #313 fixed for TS::String2).
    impl_js_family_get_op_type!(
        Tsx,
        op_extras: [QMARKDOT, PredefinedType],
        operand_extras: [Identifier2, String2, String3, NestedIdentifier, MemberExpression4],
    );

    get_operator!(Tsx);
}

impl Getter for RustCode {
    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        // we're in a function or in a class or an impl
        // for an impl: we've  'impl ... type {...'
        if let Some(name) = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("type"))
        {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            Some("<anonymous>")
        }
    }

    fn get_space_kind(node: &Node) -> SpaceKind {
        use Rust::*;

        match node.kind_id().into() {
            FunctionItem | ClosureExpression => SpaceKind::Function,
            TraitItem => SpaceKind::Trait,
            ImplItem => SpaceKind::Impl,
            SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Rust::*;

        match node.kind_id().into() {
            // `||` is treated as an operator only if it's part of a binary expression.
            // This prevents misclassification inside macros where closures without arguments (e.g., `let closure = || { /* ... */ };`)
            // are not recognized as `ClosureExpression` and their `||` node is identified as `PIPEPIPE` instead of `ClosureParameters`.
            //
            // Similarly, exclude `/` when it corresponds to the third slash in `///` (`OuterDocCommentMarker`)
            PIPEPIPE | SLASH => match node.parent() {
                Some(parent) if matches!(parent.kind_id().into(), BinaryExpression) => {
                    HalsteadType::Operator
                }
                _ => HalsteadType::Unknown,
            },
            // Ensure `!` is counted as an operator unless it belongs to an `InnerDocCommentMarker` `//!`
            BANG => match node.parent() {
                Some(parent) if !matches!(parent.kind_id().into(), InnerDocCommentMarker) => {
                    HalsteadType::Operator
                }
                _ => HalsteadType::Unknown,
            },
            // COLONCOLON (`::`) is the path-segment separator. C++, Java,
            // C#, and Kotlin all classify it as an operator; omitting it
            // here (issue #394) silently dropped every path expression
            // (`std::collections::HashMap`, `Vec::new`, `T::method`) into
            // HalsteadType::Unknown, deflating n1/N1 for path-heavy code.
            //
            // The 14 declaration/visibility keywords (Const, Static, Enum,
            // Struct, Trait, Impl, Use, Mod, Pub, Type, Union, Where,
            // Extern, Dyn) were inconsistently absent — the impl already
            // accepted 17 other keywords (As, Async, Await, Break, …, Fn).
            // Including them brings declaration-heavy code in line with
            // statement-heavy code.
            LPAREN | LBRACE | LBRACK | As | EQGT | PLUS | STAR | Async | Await | Break
            | Continue | Else | For | If | In | Let | Loop | Match | Return | Unsafe | While
            | EQ | COMMA | DASHGT | QMARK | LT | GT | AMP | MutableSpecifier | DOTDOT
            | DOTDOTEQ | DASH | AMPAMP | PIPE | CARET | EQEQ | BANGEQ | LTEQ | GTEQ | LTLT
            | GTGT | PERCENT | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ | AMPEQ | PIPEEQ
            | CARETEQ | LTLTEQ | GTGTEQ | Move | DOT | PrimitiveType | PrimitiveType2
            | PrimitiveType3 | PrimitiveType4 | PrimitiveType5 | PrimitiveType6
            | PrimitiveType7 | PrimitiveType8 | PrimitiveType9 | PrimitiveType10
            | PrimitiveType11 | PrimitiveType12 | PrimitiveType13 | PrimitiveType14
            | PrimitiveType15 | PrimitiveType16 | PrimitiveType17 | Fn | SEMI | COLONCOLON
            | Const | Static | Enum | Struct | Trait | Impl | Use | Mod | Pub | Type | Union
            | Where | Extern | Dyn => HalsteadType::Operator,
            // FieldIdentifier (e.g. `p.x`) and TypeIdentifier (e.g. `Vec`,
            // `HashMap`) are operand-class names — C++ and Go classify them
            // the same way (see arms ~588 and ~862 below). Omitting them
            // here silently dropped both into HalsteadType::Unknown,
            // deflating n2/N2 and the derived vocabulary/volume/effort
            // estimates (issue #390).
            Identifier | TypeIdentifier | FieldIdentifier | StringLiteral | RawStringLiteral
            | IntegerLiteral | FloatLiteral | BooleanLiteral | Zelf | CharLiteral | UNDERSCORE => {
                HalsteadType::Operand
            }
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Rust);
}

impl Getter for CppCode {
    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        // Issue #285 contract: every `Cpp::FunctionDefinition*` alias
        // must be enumerated here AND in `get_space_kind` below AND
        // in `is_func` / `is_func_space` (see `src/checker.rs`).
        // The aliased kind_ids 489/491/494 are not emitted by the
        // currently pinned `tree-sitter-mozcpp` parse tables, so a
        // dropped variant would silently fall through to the
        // `_ => name-field` arm and yield the wrong name (or `None`).
        match node.kind_id().into() {
            Cpp::FunctionDefinition
            | Cpp::FunctionDefinition2
            | Cpp::FunctionDefinition3
            | Cpp::FunctionDefinition4 => {
                if let Some(op_cast) = node.first_child(|id| Cpp::OperatorCast == id) {
                    let code = &code[op_cast.start_byte()..op_cast.end_byte()];
                    return std::str::from_utf8(code).ok();
                }
                // we're in a function_definition so need to get the declarator
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    let declarator_node = declarator;
                    if let Some(fd) = declarator_node.first_occurrence(|id| {
                        Cpp::FunctionDeclarator == id
                            || Cpp::FunctionDeclarator2 == id
                            || Cpp::FunctionDeclarator3 == id
                    }) && let Some(first) = fd.child(0)
                    {
                        match first.kind_id().into() {
                            Cpp::TypeIdentifier
                            | Cpp::Identifier
                            | Cpp::FieldIdentifier
                            | Cpp::DestructorName
                            | Cpp::OperatorName
                            | Cpp::QualifiedIdentifier
                            | Cpp::QualifiedIdentifier2
                            | Cpp::QualifiedIdentifier3
                            | Cpp::QualifiedIdentifier4
                            | Cpp::TemplateFunction
                            | Cpp::TemplateMethod => {
                                let code = &code[first.start_byte()..first.end_byte()];
                                return std::str::from_utf8(code).ok();
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {
                if let Some(name) = node.child_by_field_name("name") {
                    let code = &code[name.start_byte()..name.end_byte()];
                    return std::str::from_utf8(code).ok();
                }
            }
        }
        None
    }

    fn get_space_kind(node: &Node) -> SpaceKind {
        use Cpp::*;

        // Issue #285 contract: keep every `FunctionDefinition*` alias
        // listed here — see the comment above `get_func_space_name`.
        match node.kind_id().into() {
            FunctionDefinition | FunctionDefinition2 | FunctionDefinition3
            | FunctionDefinition4 => SpaceKind::Function,
            StructSpecifier => SpaceKind::Struct,
            ClassSpecifier => SpaceKind::Class,
            NamespaceDefinition => SpaceKind::Namespace,
            TranslationUnit => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Cpp::*;

        match node.kind_id().into() {
            DOT | DOTSTAR | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI | Return
            | Break | Continue | If | Else | Switch | Case | Default | For | While | Goto | Do
            | Delete | New | Try | Try2 | Catch | Throw | EQ | AMPAMP | PIPEPIPE | DASH
            | DASHDASH | DASHGT | DASHGTSTAR | PLUS | PLUSPLUS | SLASH | PERCENT | PIPE | AMP
            | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | GT2 | LTEQGT | PLUSEQ
            | DASHEQ | BANG | STAREQ | SLASHEQ | PERCENTEQ | GTGTEQ | LTLTEQ | AMPEQ | CARET
            | CARETEQ | PIPEEQ | LBRACK | LBRACE | QMARK | COLONCOLON | PrimitiveType
            | TypeSpecifier | Sizeof => HalsteadType::Operator,
            Identifier | TypeIdentifier | FieldIdentifier | RawStringLiteral | StringLiteral
            | NumberLiteral | True | False | Null | DOTDOTDOT => HalsteadType::Operand,
            NamespaceIdentifier => match node.parent() {
                Some(parent) if matches!(parent.kind_id().into(), NamespaceDefinition) => {
                    HalsteadType::Operand
                }
                _ => HalsteadType::Unknown,
            },
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Cpp);
}

impl Getter for PreprocCode {}
impl Getter for CcommentCode {}

impl Getter for JavaCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Java::*;

        // `EnumDeclaration` and `RecordDeclaration` are class-like
        // (they extend `Object`, hold fields, and can declare methods)
        // so they share `SpaceKind::Class`. `AnnotationTypeDeclaration`
        // implicitly extends `java.lang.annotation.Annotation` (an
        // interface) and its elements are abstract methods at the
        // bytecode level, so it maps to `SpaceKind::Interface`.
        match node.kind_id().into() {
            ClassDeclaration | EnumDeclaration | RecordDeclaration => SpaceKind::Class,
            MethodDeclaration | ConstructorDeclaration | LambdaExpression => SpaceKind::Function,
            InterfaceDeclaration | AnnotationTypeDeclaration => SpaceKind::Interface,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Java::*;
        // Some guides that informed grammar choice for Halstead
        // keywords, operators, literals: https://docs.oracle.com/javase/specs/jls/se18/html/jls-3.html#jls-3.12
        // https://www.geeksforgeeks.org/software-engineering-halsteads-software-metrics/?msclkid=5e181114abef11ecbb03527e95a34828
        match node.kind_id().into() {
            // Operator: control flow
            | If | Else | Switch | Case | Try | Catch | Throw | Throws | Throws2 | For | While | Continue | Break | Do | Finally
            // Operator: keywords
            | New | Return | Default | Abstract | Assert | Instanceof | Extends | Final | Implements | Transient | Synchronized | Super | This | VoidType
            // Operator: brackets and comma and terminators (separators)
            | SEMI | COMMA | COLONCOLON | DOT | DASHGT | LBRACE | LBRACK | LPAREN
            // Operator: operators
            | EQ | LT | GT | BANG | TILDE | QMARK | COLON
            | EQEQ | LTEQ | GTEQ | BANGEQ | AMPAMP | PIPEPIPE | PLUSPLUS | DASHDASH
            | PLUS | DASH | STAR | SLASH | AMP | PIPE | CARET | PERCENT| LTLT | GTGT | GTGTGT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | AMPEQ | PIPEEQ | CARETEQ | PERCENTEQ | LTLTEQ | GTGTEQ | GTGTGTEQ
            // primitive types
            | Byte | Short | Int | Long | Char | Float | Double | BooleanType
            => {
                HalsteadType::Operator
            },
            // Operands: variables, constants, literals
            Identifier | NullLiteral | ClassLiteral | True | False | StringLiteral | CharacterLiteral | HexIntegerLiteral | OctalIntegerLiteral | BinaryIntegerLiteral | DecimalIntegerLiteral | HexFloatingPointLiteral | DecimalFloatingPointLiteral  => {
                HalsteadType::Operand
            },
            _ => {
                HalsteadType::Unknown
            },
        }
    }

    fn get_operator_id_as_str(id: u16) -> &'static str {
        let typ = id.into();
        match typ {
            Java::LPAREN => "()",
            Java::LBRACK => "[]",
            Java::LBRACE => "{}",
            Java::VoidType => "void",
            _ => typ.into(),
        }
    }
}

impl Getter for CsharpCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Csharp::*;

        // `EnumDeclaration` maps to `SpaceKind::Class` for cross-language
        // parity with Java/PHP/Groovy (issue #429): a C# enum opens a
        // FuncSpace via `is_func_space`, so it must classify here too or
        // it falls through to `_ => SpaceKind::Unknown`.
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
            | ConversionOperatorDeclaration
            | IndexerDeclaration => SpaceKind::Function,
            CompilationUnit => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
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
            // Operands: identifiers and literals.
            Identifier | GenericName | QualifiedName | AliasQualifiedName
            | IntegerLiteral | RealLiteral | BooleanLiteral | NullLiteral | True | False
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

impl Getter for KotlinCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Kotlin::*;

        match node.kind_id().into() {
            // The Kotlin grammar models classes and interfaces under a single
            // `class_declaration` node with the discriminating keyword
            // (`class` vs `interface`) appearing as a direct child token.
            // Distinguishing them at the space-kind level lets the OOP
            // metrics (`wmc`, `npa`, `npm`) attribute counts to the right
            // bucket without re-inspecting the AST.
            ClassDeclaration => {
                if node.first_child(|id| id == Interface).is_some() {
                    SpaceKind::Interface
                } else {
                    SpaceKind::Class
                }
            }
            // `object MyObject { ... }` is a singleton; treat it as a class
            // for OOP metric purposes (it has properties, methods, init
            // blocks, exactly like a class).
            ObjectDeclaration => SpaceKind::Class,
            FunctionDeclaration | SecondaryConstructor | LambdaLiteral | AnonymousFunction => {
                SpaceKind::Function
            }
            SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Kotlin::*;

        match node.kind_id().into() {
            // Operator: control flow keywords
            If | Else | When | For | While | Do | Try | Catch | Finally | Throw | Return
            | ReturnAT
            // Operator: other keywords
            | Class | Fun | Object | Val | Var | In | Is | As | AsQMARK | BANGis | BANGin
            | This | Super | Constructor
            // Operator: brackets, separators, terminators
            | SEMI | COMMA | COLONCOLON | DOT | LBRACE | LBRACK | LPAREN
            // Operator: assignment and arithmetic
            | EQ | PLUS | DASH | STAR | SLASH | PERCENT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | PLUSPLUS | DASHDASH
            // Operator: comparison and equality
            | LT | GT | LTEQ | GTEQ | EQEQ | EQEQEQ | BANGEQ | BANGEQEQ
            // Operator: logical and misc
            | AMPAMP | PIPEPIPE | BANG | BANGBANG
            | QMARK | QMARKCOLON | QMARKDOT
            | DOTDOT | DOTDOTLT | DASHGT | COLON => HalsteadType::Operator,
            // Operands: identifiers and literals
            Identifier | NumberLiteral | FloatLiteral | CharacterLiteral | Label => {
                HalsteadType::Operand
            }
            // Regression #191: a Kotlin string template (`"Hi $name"` or
            // `"${expr}"`) wraps `Interpolation` children whose inner
            // expressions are walked and counted separately. Skip the
            // wrapping literal to avoid double-counting (same pattern as
            // #180 for Bash/Elixir and #184 for PHP). Both single-line and
            // multi-line (triple-quoted) string literals support
            // interpolation in Kotlin.
            StringLiteral | MultilineStringLiteral => {
                Self::string_operand_type(node, &[Interpolation as u16])
            }
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Kotlin);
}

impl Getter for GoCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        // Aliased because `Go::Go` (the `go` keyword variant) collides with
        // the bare enum name in pattern position under `use Go::*;`.
        use Go as G;

        match node.kind_id().into() {
            G::FunctionDeclaration | G::MethodDeclaration | G::FuncLiteral => SpaceKind::Function,
            G::SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Go as G;

        match node.kind_id().into() {
            // Control flow and declaration keywords
            G::If | G::Else | G::Switch | G::Case | G::Default | G::For | G::Range
            | G::Continue | G::Break | G::Fallthrough | G::Goto | G::Return | G::Select
            | G::Defer | G::Go | G::Func | G::Type | G::Struct | G::Interface | G::Map
            | G::Chan | G::Const | G::Var | G::Package | G::Import
            // Punctuation acting as operators
            | G::SEMI | G::COMMA | G::COLON | G::LBRACE | G::LBRACK | G::LPAREN
            | G::DOT | G::DOTDOTDOT
            // Operators
            | G::EQ | G::COLONEQ | G::PLUS | G::DASH | G::STAR | G::SLASH | G::PERCENT
            | G::AMP | G::PIPE | G::CARET | G::TILDE | G::BANG | G::LT | G::GT
            | G::LTEQ | G::GTEQ | G::EQEQ | G::BANGEQ | G::AMPAMP | G::PIPEPIPE
            | G::LTLT | G::GTGT | G::AMPCARET | G::LTDASH | G::PLUSPLUS | G::DASHDASH
            | G::PLUSEQ | G::DASHEQ | G::STAREQ | G::SLASHEQ | G::PERCENTEQ | G::AMPEQ
            | G::PIPEEQ | G::CARETEQ | G::LTLTEQ | G::GTGTEQ | G::AMPCARETEQ
                => HalsteadType::Operator,
            // Operands: identifiers and literals
            G::Identifier | G::Identifier2 | G::Identifier3 | G::BlankIdentifier
            | G::FieldIdentifier | G::PackageIdentifier | G::TypeIdentifier
            | G::LabelName | G::IntLiteral | G::FloatLiteral | G::ImaginaryLiteral
            | G::RuneLiteral | G::InterpretedStringLiteral | G::RawStringLiteral | G::Nil
            | G::True | G::False | G::Iota
                => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Go);
}

impl Getter for PerlCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Perl::FunctionDefinition
            | Perl::FunctionDefinitionWithoutSub
            | Perl::AnonymousFunction => SpaceKind::Function,
            Perl::SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Perl as P;

        match node.kind_id().into() {
            // Control-flow and declaration keywords. `Perl::Sub` is the
            // `sub` keyword (token id 16); `Perl::SUB` is the `__SUB__`
            // literal (token id 7) — that one is an operand, not an
            // operator. Same split for `Package` (keyword) vs `PACKAGE`
            // (`__PACKAGE__` literal).
            P::If | P::Unless | P::Else | P::Elsif | P::While | P::Until | P::For
            | P::Foreach | P::When | P::Continue | P::Next | P::Last | P::Redo | P::Goto
            | P::Return | P::Sub | P::Package | P::My | P::Our
            | P::Local | P::State | P::Use | P::No | P::Require | P::Bless | P::And | P::Or
            | P::Xor | P::Not | P::Eq | P::Ne | P::Lt | P::Gt | P::Le | P::Ge | P::Cmp
            // Punctuation acting as operators
            | P::SEMI | P::COMMA | P::COLON | P::COLONCOLON | P::LBRACE | P::LBRACK
            | P::LPAREN | P::DOT | P::DOTDOT | P::DOTDOTDOT | P::FatComma | P::DASHGT
            | P::QMARK | P::BSLASH | P::DOLLAR | P::DOLLARHASH | P::AT | P::PERCENT | P::HASH
            // Arithmetic / comparison / logical / bitwise / assignment operators
            | P::EQ | P::PLUS | P::DASH | P::STAR | P::SLASH | P::STARSTAR | P::BANG
            | P::TILDE | P::EQTILDE | P::BANGTILDE | P::EQEQ | P::BANGEQ | P::LT | P::GT
            | P::LTEQ | P::GTEQ | P::AMPAMP | P::PIPEPIPE | P::SLASHSLASH | P::PIPE
            | P::CARET | P::LTLT | P::GTGT | P::TILDETILDE | P::PLUSPLUS | P::DASHDASH
            | P::PLUSEQ | P::DASHEQ | P::STAREQ | P::SLASHEQ | P::PERCENTEQ | P::STARSTAREQ
            | P::AMPEQ | P::PIPEEQ | P::CARETEQ | P::LTLTEQ | P::GTGTEQ | P::AMPAMPEQ
            | P::PIPEPIPEEQ | P::SLASHSLASHEQ | P::DOTEQ | P::XEQ
            | P::LTEQGT | P::AMPDOTEQ | P::PIPEDOTEQ | P::CARETDOTEQ | P::Isa
                => HalsteadType::Operator,
            // Operands: identifiers and literals. Non-interpolating
            // string literals (`'…'`, `q{…}`) are leaf operands; the
            // interpolating kinds (`"…"`, `qq{…}`, `` `…` ``, `qx{…}`)
            // are handled separately below so their inner
            // scalar/array/hash variables are not double-counted.
            P::Identifier | P::ScalarVariable | P::ArrayVariable | P::HashVariable
            | P::PackageVariable | P::SpecialScalarVariable | P::PackageName | P::ModuleName
            | P::BarewordImport | P::Typeglob | P::FileHandle
            | P::Integer | P::FloatingPoint | P::ScientificNotation | P::Hexadecimal | P::Octal
            | P::True | P::False | P::SpecialLiteral
            | P::StringSingleQuoted | P::StringQQuoted
            | P::FILE | P::LINE | P::SUB | P::PACKAGE
                => HalsteadType::Operand,
            // Perl's interpolating string-like literals count as one
            // operand when inert. When they carry an `Interpolation`
            // child the inner scalar / array / hash variables are
            // already walked and classified as operands; counting the
            // wrapping literal too would double-count the inner
            // variables' contribution to `N2` (issue #199, same
            // pattern as #180 for Elixir/Bash, #183 for C#, #184 for
            // PHP, #191 for Kotlin). `HeredocBodyStatement` is the
            // visible body of `<<TAG ... TAG` heredocs (issue #287);
            // it is interpolation-capable, so it joins the same
            // dispatch — inert heredocs are one operand, interpolating
            // heredocs let the inner variables carry the count.
            P::StringDoubleQuoted | P::StringQqQuoted | P::BacktickQuoted
            | P::CommandQxQuoted | P::HeredocBodyStatement => {
                Self::string_operand_type(node, &[P::Interpolation as u16])
            }
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Perl);
}

impl Getter for LuaCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Lua::FunctionDeclaration
            | Lua::FunctionDeclaration2
            | Lua::FunctionDeclaration3
            | Lua::FunctionDefinition => SpaceKind::Function,
            Lua::Chunk => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        match node.kind_id().into() {
            // Control-flow and declaration keywords
            Lua::If
            | Lua::Then
            | Lua::Else
            | Lua::Elseif
            | Lua::End2
            | Lua::For
            | Lua::In
            | Lua::While
            | Lua::Do
            | Lua::Repeat
            | Lua::Until
            | Lua::Return
            | Lua::Goto
            | Lua::Local
            | Lua::Function
            // Logical operators (keywords in Lua)
            | Lua::And
            | Lua::Or
            | Lua::Not
            // Structural punctuation
            | Lua::SEMI
            | Lua::COMMA
            | Lua::COLON
            | Lua::COLONCOLON
            | Lua::LBRACE
            | Lua::RBRACE
            | Lua::LBRACK
            | Lua::RBRACK
            | Lua::LPAREN
            | Lua::RPAREN
            | Lua::DOT
            | Lua::DOTDOT
            // Arithmetic / concat / length
            | Lua::PLUS
            | Lua::DASH
            | Lua::STAR
            | Lua::SLASH
            | Lua::SLASHSLASH
            | Lua::PERCENT
            | Lua::CARET
            | Lua::HASH
            // Bitwise (Lua 5.3+)
            | Lua::AMP
            | Lua::PIPE
            | Lua::TILDE
            | Lua::LTLT
            | Lua::GTGT
            // Comparison
            | Lua::EQEQ
            | Lua::TILDEEQ
            | Lua::LT
            | Lua::GT
            | Lua::LTEQ
            | Lua::GTEQ
            // Assignment
            | Lua::EQ
            // `break` is a named leaf node (no anonymous keyword child), so it must be
            // matched directly here — unlike `return`/`goto` which are anonymous tokens.
            | Lua::BreakStatement => HalsteadType::Operator,

            // Operands: identifiers and literals
            Lua::Identifier | Lua::Number | Lua::String | Lua::True | Lua::False | Lua::Nil
            | Lua::VarargExpression => HalsteadType::Operand,

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Lua);
}

/// Returns whether a Bash string node carries any expansion child that
/// would itself be classified as an operand by [`BashCode::get_op_type`]
/// (`$var`, `${name[…]}`, `$(cmd)`, `$((expr))`).
#[inline]
fn bash_string_has_expansion(node: &Node) -> bool {
    node.children().any(|c| {
        matches!(
            c.kind_id().into(),
            Bash::SimpleExpansion
                | Bash::Expansion
                | Bash::CommandSubstitution
                | Bash::ArithmeticExpansion
        )
    })
}

impl Getter for BashCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Bash::FunctionDefinition => SpaceKind::Function,
            Bash::Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        match node.kind_id().into() {
            // Control flow and declaration keywords
            Bash::If | Bash::Then | Bash::Fi | Bash::Elif | Bash::Else
            | Bash::For | Bash::In | Bash::While | Bash::Until | Bash::Do | Bash::Done
            | Bash::Case | Bash::Esac
            | Bash::Function | Bash::Local | Bash::Declare | Bash::Typeset
            | Bash::Export | Bash::Readonly | Bash::Unset | Bash::Unsetenv
            // Punctuation acting as operators
            | Bash::LPAREN | Bash::RPAREN | Bash::LBRACE | Bash::RBRACE
            | Bash::LBRACK | Bash::RBRACK | Bash::LBRACKLBRACK | Bash::RBRACKRBRACK
            | Bash::SEMI | Bash::SEMISEMI | Bash::SEMIAMP | Bash::SEMISEMIAMP
            | Bash::COMMA | Bash::COLON
            // Assignment, arithmetic, and comparison operators
            | Bash::EQ | Bash::PLUSEQ | Bash::DASHEQ | Bash::STAREQ | Bash::SLASHEQ
            | Bash::PERCENTEQ | Bash::STARSTAREQ | Bash::LTLTEQ | Bash::GTGTEQ
            | Bash::AMPEQ | Bash::CARETEQ | Bash::PIPEEQ
            | Bash::PLUS | Bash::DASH | Bash::STAR | Bash::SLASH | Bash::PERCENT | Bash::STARSTAR
            | Bash::PLUSPLUS | Bash::DASHDASH
            | Bash::EQEQ | Bash::BANGEQ | Bash::LT | Bash::GT | Bash::LTEQ | Bash::GTEQ
            | Bash::EQTILDE
            // Logical and bitwise operators
            | Bash::AMPAMP | Bash::PIPEPIPE | Bash::PIPE | Bash::PIPEAMP
            | Bash::AMP | Bash::CARET | Bash::TILDE | Bash::BANG
            | Bash::LTLT | Bash::GTGT
            // Test operators (prefix)
            | Bash::DASHa | Bash::DASHo
            // Redirection operators
            | Bash::AMPGT | Bash::GTAMP | Bash::LTAMP | Bash::GTPIPE
            | Bash::LTAMPDASH | Bash::GTAMPDASH | Bash::LTLTDASH | Bash::AMPGTGT
            | Bash::LTLTLT
            // Ternary operator
            | Bash::QMARK | Bash::QMARK2
                => HalsteadType::Operator,

            // Quoted strings count as one operand when they are inert.
            // When they contain any `$var`/`${...}`/`$(...)`/`$((...))`
            // expansion child, those expansions are already walked and
            // classified as operands; counting the wrapping literal too
            // would double-count the inner identifiers (issue #180).
            // `RawString` is single-quoted and never interpolates, but
            // the check is uniform across the four string kinds for
            // clarity.
            Bash::String | Bash::RawString | Bash::AnsiCString | Bash::TranslatedString => {
                if bash_string_has_expansion(node) {
                    HalsteadType::Unknown
                } else {
                    HalsteadType::Operand
                }
            }

            // Operands: identifiers, literals, variables. `variable_name`
            // and `special_variable_name` each surface under multiple
            // aliased kind_ids (tree-sitter generates one per parse-table
            // context); every alias must be matched or assignment LHS
            // identifiers like `name` in `name=value` are silently
            // unclassified — see lesson 2.
            Bash::Word | Bash::Word2 | Bash::Word3 | Bash::Word4
            | Bash::Number | Bash::Number2 | Bash::NumberToken1 | Bash::NumberToken2
            | Bash::SimpleExpansion
            | Bash::VariableName | Bash::VariableName2 | Bash::VariableName3
            | Bash::SpecialVariableName | Bash::SpecialVariableName2
            | Bash::CommandName | Bash::Concat
                => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Bash);
}

impl Getter for TclCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Tcl::Procedure => SpaceKind::Function,
            Tcl::SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        match node.kind_id().into() {
            // Anonymous keyword tokens (control-flow and declaration keywords).
            Tcl::Proc
            | Tcl::If2
            | Tcl::Elseif2
            | Tcl::Else2
            | Tcl::While2
            | Tcl::Foreach2
            | Tcl::Set2
            | Tcl::Global2
            | Tcl::Namespace2
            | Tcl::Try2
            | Tcl::Catch2
            | Tcl::Finally2
            | Tcl::Regexp2
            | Tcl::Expr2
            // String comparison operators.
            | Tcl::Eq
            | Tcl::Ne
            | Tcl::In
            | Tcl::Ni
            // Structural punctuation.
            | Tcl::LBRACE
            | Tcl::RBRACE
            | Tcl::LBRACK
            | Tcl::RBRACK
            | Tcl::LPAREN
            | Tcl::LPAREN2
            | Tcl::RPAREN
            | Tcl::SEMI
            | Tcl::COLON
            | Tcl::COLONCOLON
            | Tcl::COLONCOLON2
            // Arithmetic / exponent operators.
            | Tcl::PLUS
            | Tcl::DASH
            | Tcl::STAR
            | Tcl::SLASH
            | Tcl::PERCENT
            | Tcl::STARSTAR
            // Bitwise operators.
            | Tcl::AMP
            | Tcl::PIPE
            | Tcl::CARET
            | Tcl::TILDE
            | Tcl::LTLT
            | Tcl::GTGT
            // Comparison operators.
            | Tcl::EQEQ
            | Tcl::BANGEQ
            | Tcl::LT
            | Tcl::GT
            | Tcl::LTEQ
            | Tcl::GTEQ
            // Logical operators.
            | Tcl::BANG
            | Tcl::AMPAMP
            | Tcl::PIPEPIPE
            // Ternary conditional operator.
            | Tcl::QMARK => HalsteadType::Operator,

            // Operands: identifiers and literals.
            // Id2 (anonymous "id" token, kind_id=85) is intentionally excluded: it only
            // appears as a leaf child of VariableSubstitution ($varname syntax), which is
            // already counted as an operand. Including Id2 would double-count each bare
            // variable reference.
            Tcl::Id
            | Tcl::SimpleWord
            | Tcl::Number
            | Tcl::BracedWord
            | Tcl::BracedWordSimple
            | Tcl::VariableSubstitution => HalsteadType::Operand,

            // Double-quoted strings count as a single operand when inert
            // (`"hello world"`). When they carry a `$var` or `[cmd]`
            // interpolation child, the inner `variable_substitution` /
            // `command_substitution` nodes are walked separately and
            // contribute their own operands; counting the wrapping
            // `QuotedWord` too would double-count `N2` (issue #277, same
            // pattern as #180/#183/#184 for Bash/C#/PHP).
            Tcl::QuotedWord => Self::string_operand_type(
                node,
                &[
                    Tcl::VariableSubstitution as u16,
                    Tcl::CommandSubstitution as u16,
                ],
            ),

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Tcl);
}

impl Getter for PhpCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            // PHP traits are class-like mixins whose method
            // implementations roll up into the consuming class's WMC; we
            // map them to `SpaceKind::Class` so the per-class metrics
            // (NPA, NPM, WMC) treat them uniformly. The output may label
            // them "class" — that is intentional for metric coherence.
            // LOAD-BEARING: `Wmc::compute` for PhpCode does not match
            // `SpaceKind::Trait`. If you remap `TraitDeclaration` here,
            // also update `src/metrics/wmc.rs`.
            Php::ClassDeclaration
            | Php::AnonymousClass
            | Php::EnumDeclaration
            | Php::TraitDeclaration => SpaceKind::Class,
            Php::InterfaceDeclaration => SpaceKind::Interface,
            Php::FunctionDefinition
            | Php::MethodDeclaration
            | Php::AnonymousFunction
            | Php::ArrowFunction => SpaceKind::Function,
            Php::Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Php::*;
        match node.kind_id().into() {
            // Operator: control-flow keywords
            If | Else | Elseif | Endif
            | Switch | Case | Default | Endswitch
            | For | Endfor | Foreach | Endforeach
            | While | Endwhile | Do
            | Break | Continue
            | Return | Throw | Try | Catch | Finally
            | Match | Yield | Yieldfrom | Goto
            | Echo | Exit | Print
            | Include | IncludeOnce | Require | RequireOnce

            // Operator: declaration keywords
            | Function | Class | Interface | Trait | Enum | Namespace
            | Use | Const | Global | Static | VarModifier
            | Public | Protected | Private
            | Final | Abstract | Readonly
            | New | Clone | Instanceof | As | Insteadof | Extends | Implements
            | Fn | Declare | Enddeclare | Unset | List
            | Zelf | Parent

            // Operator: structural punctuation
            | LBRACE | RBRACE | LPAREN | LPAREN2 | RPAREN | RPAREN2
            | LBRACK | RBRACK
            | COMMA | SEMI | COLON | COLONCOLON
            | DASHGT | QMARKDASHGT | EQGT | BSLASH | DOTDOTDOT | QMARK | AT
            | HASHLBRACK

            // Operator: arithmetic
            | PLUS | DASH | STAR | SLASH | PERCENT | STARSTAR
            | PLUSPLUS | DASHDASH

            // Operator: comparison
            | EQEQ | EQEQEQ | BANGEQ | BANGEQEQ | LTGT
            | LT | GT | LTEQ | GTEQ | LTEQGT

            // Operator: logical
            | AMPAMP | PIPEPIPE | BANG
            | And | Or | Xor | QMARKQMARK

            // Operator: bitwise
            | AMP | PIPE | CARET | TILDE | LTLT | GTGT

            // Operator: assignment
            | EQ
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ
            | DOTEQ | QMARKQMARKEQ
            | AMPEQ | PIPEEQ | CARETEQ | LTLTEQ | GTGTEQ

            // Operator: string concat
            | DOT
                => HalsteadType::Operator,

            // Operands: identifiers and literals.
            // `String`/`String2`/`String3` (single-quoted) and
            // `Nowdoc` never interpolate and are always counted as
            // one operand each.
            Name | Name2 | VariableName | DynamicVariableName
            | Integer | Float | Float2
            | String | String2 | String3
            | Nowdoc
            | Boolean | Null | Null2
            | NamedType | OptionalType | UnionType | IntersectionType
            | DisjunctiveNormalFormType | BottomType
            | PrimitiveType | CastType
            | QualifiedName | RelativeName | NamespaceName
            | Int | Bool | Array | Object
                => HalsteadType::Operand,

            // `EncapsedString` (double-quoted), `Heredoc`, and
            // `ShellCommandExpression` (backticks) count as one
            // operand when inert. When they carry a `$var`,
            // `${name}`, or `{$expr}` interpolation child, those
            // inner expressions are already walked and classified as
            // operands in their own right; counting the wrapping
            // literal too would double-count their contribution to
            // `N2` (issue #184, same pattern as #180 for Elixir/Bash
            // and #183 for C#). `ShellCommandExpression` was previously
            // omitted entirely (issue #288), so backtick literals
            // contributed no Halstead operand at all even when inert.
            EncapsedString | Heredoc | ShellCommandExpression => {
                // PHP's interpolation children appear directly on the
                // wrapping literal, except `Heredoc`, which holds them
                // one level down under a single `heredoc_body` child —
                // so the descend below mirrors the original
                // `php_string_has_interpolation` two-level walk.
                const PHP_INTERP_KINDS: &[u16] = &[
                    // `"$name"` → direct `variable_name` child.
                    VariableName as u16,
                    // `"${name}"` → direct `dynamic_variable_name` child.
                    DynamicVariableName as u16,
                    // `"$arr[0]"` → direct `subscript_expression` child.
                    // The grammar gives this kind three numeric aliases.
                    SubscriptExpression as u16,
                    SubscriptExpression2 as u16,
                    SubscriptExpression3 as u16,
                    // `"$obj->prop"` → direct `member_access_expression`
                    // child. PHP's bare-interpolation syntax does not
                    // support `?->` (nullsafe) or `::` (scope), so only
                    // member-access aliases need handling here; nullsafe /
                    // scope forms always go through the `{ … }` wrapper.
                    MemberAccessExpression as u16,
                    MemberAccessExpression2 as u16,
                    MemberAccessExpression3 as u16,
                    // `"{$expr}"` → anonymous `{` (LBRACE) opens the
                    // complex-interpolation wrapper whose body is an
                    // arbitrary expression; the brace appears as a direct
                    // child.
                    LBRACE as u16,
                ];
                // Single pass over the direct children: an interpolation
                // child on the literal itself (EncapsedString /
                // ShellCommandExpression) OR, for Heredoc, one nested under
                // its `heredoc_body` child. Folding both checks into one walk
                // avoids re-scanning the children a second time through
                // `string_operand_type` for an inert heredoc.
                let has_interp = node.children().any(|c| {
                    let kind = c.kind_id();
                    PHP_INTERP_KINDS.contains(&kind)
                        || (kind == HeredocBody as u16 && c.wraps_any(PHP_INTERP_KINDS))
                });
                if has_interp {
                    HalsteadType::Unknown
                } else {
                    HalsteadType::Operand
                }
            }

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Php);
}

// Extracts the human-readable head name from the first non-`target`
// child of an Elixir `def` / `defp` / `defmacro` / `defmacrop` /
// `defmodule` Call. Handles three shapes:
//   - `Arguments` wrapper: descend one level and recurse.
//   - `Identifier` / `Alias` leaf: return its source text.
//   - inner `Call` (e.g. `def foo(x, y)`): return the target identifier
//     text of that inner Call.
// Returns `None` when the child does not match any of these shapes,
// allowing the caller to keep scanning siblings (notably the
// `do_block`, which is unconditionally present and never carries the
// name).
fn elixir_extract_head_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
    use Elixir as E;

    let text = |n: &Node| std::str::from_utf8(&code[n.start_byte()..n.end_byte()]).ok();
    match node.kind_id().into() {
        E::Identifier | E::Alias => text(node),
        E::Call => text(&node.child_by_field_name("target")?),
        E::Arguments
        | E::Arguments2
        | E::Arguments3
        | E::Arguments4
        | E::Arguments5
        | E::CallArgumentsWithTrailingSeparator => node
            .children()
            .find_map(|child| elixir_extract_head_name(&child, code)),
        _ => None,
    }
}

impl Getter for ElixirCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Elixir as E;

        match node.kind_id().into() {
            E::AnonymousFunction => SpaceKind::Function,
            E::Source => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    // Source-aware classifier (#275). Elixir's `defmodule` /
    // `def` / `defp` / `defmacro` / `defmacrop` are not distinct
    // grammar productions — they all parse as `Call` nodes whose
    // `target` Identifier text spells the keyword. The walker promotes
    // these Calls to func spaces via `Checker::is_func_space_with_code`;
    // this method labels the promoted space with the right `SpaceKind`
    // so `Wmc` / `Npm` / `Npa` see a Class for `defmodule` and a
    // Function for the method-defining macros.
    fn get_space_kind_with_code(node: &Node, code: &[u8]) -> SpaceKind {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        let kind = Self::get_space_kind(node);
        if kind != SpaceKind::Unknown {
            return kind;
        }
        match elixir_call_keyword(node, code) {
            Some(kw) if elixir_is_class_macro(kw) => SpaceKind::Class,
            // Method-defining macros nested inside a `quote do … end`
            // template are not real method declarations (#310).
            Some(kw) if elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code) => {
                SpaceKind::Function
            }
            _ => SpaceKind::Unknown,
        }
    }

    // Source-aware name extraction for the macro-shaped declarations.
    // `def foo(x, y) do … end` parses (with the tree-sitter-elixir
    // grammar shipped here) as
    //   `Call { target: Identifier "def", Arguments { Call { target:
    //     Identifier "foo", Arguments { … } } }, DoBlock { … } }`
    // i.e. the head Call is wrapped in an `Arguments` node, not a
    // direct child. `defmodule Foo.Bar do … end` parses similarly with
    // the `Alias` inside `Arguments`. We descend through one
    // `Arguments` layer when present, then either:
    //   - return the `Identifier` / `Alias` text directly
    //     (`defmodule Foo`, `def foo` for an arity-zero head with no
    //     parentheses), or
    //   - return the inner head Call's `target` text
    //     (`def foo(x, y)`).
    // Falls back to the trait default behaviour (`<anonymous>` for
    // nodes without a `name` field) when the Call is not one we
    // recognise.
    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        use Elixir as E;

        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        // The Class kind always names its head; for method macros we
        // additionally require the Call NOT to be inside a `quote`
        // template, matching the func-space promotion rule (#310).
        if node.kind_id() == E::Call as u16
            && let Some(kw) = elixir_call_keyword(node, code)
            && (elixir_is_class_macro(kw)
                || (elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)))
        {
            let target_id = node.child_by_field_name("target").map(|t| t.id());
            if let Some(name) = node
                .children()
                .filter(|child| Some(child.id()) != target_id)
                .find_map(|child| elixir_extract_head_name(&child, code))
            {
                return Some(name);
            }
        }

        if let Some(name) = node.child_by_field_name("name") {
            return std::str::from_utf8(&code[name.start_byte()..name.end_byte()]).ok();
        }
        Some("<anonymous>")
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Elixir as E;

        match node.kind_id().into() {
            // Reserved-word keywords that have dedicated token kinds in
            // the grammar — block delimiters, exception clauses, the
            // `fn` keyword, and word-form logical / membership operators.
            // (Macro-shaped keywords like `def`/`defp`/`if`/`case`/`cond`
            // are NOT here: they surface as `Identifier` tokens in a
            // `Call`'s `target` field and are counted as operands below.)
            E::Do | E::End | E::End2 | E::Else | E::After | E::Catch | E::Rescue | E::Fn
            | E::When | E::Not | E::Or | E::And | E::In | E::Notin
            // Structural punctuation acting as operators
            | E::LPAREN | E::LPAREN2 | E::RPAREN | E::LBRACE | E::RBRACE
            | E::LBRACK | E::LBRACK2 | E::RBRACK | E::LTLT | E::GTGT
            | E::COMMA | E::SEMI | E::COLON | E::COLONCOLON | E::DOT
            | E::DOTDOT | E::DOTDOTDOT | E::PERCENT | E::HASHLBRACE | E::AT
            // Arithmetic / unary
            | E::PLUS | E::DASH | E::STAR | E::STARSTAR | E::SLASH
            // Comparison
            | E::EQEQ | E::EQEQEQ | E::BANGEQ | E::BANGEQEQ
            | E::LT | E::GT | E::LTEQ | E::GTEQ
            // Logical
            | E::AMPAMP | E::PIPEPIPE | E::BANG
            // Bitwise / Erlang-band
            | E::AMP | E::PIPE | E::CARET | E::TILDE
            | E::AMPAMPAMP | E::PIPEPIPEPIPE | E::CARETCARETCARET | E::TILDETILDETILDE
            | E::LTLTLT | E::GTGTGT
            // Assignment / match
            | E::EQ
            // Concat / list operations
            | E::PLUSPLUS | E::DASHDASH | E::LTGT
            | E::PLUSPLUSPLUS | E::DASHDASHDASH
            // Pipe / capture / generator / stab arrow
            | E::PIPEGT | E::LTPIPEGT | E::DASHGT | E::LTDASH
            // Map pair / default arg / regex match / range step
            | E::EQGT | E::BSLASHBSLASH | E::EQTILDE | E::SLASHSLASH
            // Custom / less common Elixir operators
            | E::LTTILDE | E::TILDEGT | E::LTTILDEGT | E::LTLTTILDE | E::TILDEGTGT
                => HalsteadType::Operator,

            // String literals contribute exactly one operand each when
            // they are inert. When they carry an `interpolation` child,
            // the interpolated expressions are already walked and counted
            // as operands in their own right; counting the wrapping
            // literal as well would double-count the inner identifiers'
            // contribution (issue #180). The interpolation markers
            // `#{` / `}` are classified as operators via `HASHLBRACE` /
            // `RBRACE`, so an interpolated literal still adds operator
            // weight without inflating `N2`.
            E::String | E::Charlist | E::Sigil => {
                Self::string_operand_type(node, &[E::Interpolation as u16])
            }

            // Operands: identifiers and literals. Sigil names/modifiers
            // (`~r`, the trailing `i`/`u` flags) stay as operands even
            // for interpolated sigils — they are distinct tokens with
            // their own text.
            E::Identifier | E::Alias | E::OperatorIdentifier
            | E::SigilName | E::SigilName2 | E::SigilModifiers
            | E::Keyword | E::Keyword2 | E::QuotedKeyword
            | E::Integer | E::Float | E::Char
            | E::Atom | E::Atom2 | E::QuotedAtom
            | E::Boolean | E::True | E::False
            | E::Nil | E::Nil2
                => HalsteadType::Operand,

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Elixir);
}

impl Getter for RubyCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Ruby as R;

        match node.kind_id().into() {
            R::Class | R::SingletonClass => SpaceKind::Class,
            R::Module => SpaceKind::Namespace,
            R::Method | R::SingletonMethod | R::Lambda | R::Block | R::DoBlock => {
                SpaceKind::Function
            }
            R::Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Ruby as R;

        match node.kind_id().into() {
            // Control-flow keyword tokens. tree-sitter-ruby gives each
            // keyword its own anonymous numbered variant (e.g. `If2` is
            // the `if` keyword token; `If` is the named statement node).
            R::If2 | R::Unless2 | R::While2 | R::Until2 | R::For2 | R::In2 | R::Do2
            | R::Case2 | R::When2 | R::Elsif2 | R::Else2 | R::Then2
            | R::Begin2 | R::Ensure2 | R::Rescue2
            | R::Return3 | R::Yield3 | R::Break3 | R::Next3 | R::Redo2 | R::Retry2
            // Declaration keywords. `End`/`End2` are the two aliased
            // visible kinds for the `end` block closer (kind_ids 0 and
            // 13) that every `def`/`class`/`module`/`begin`/`if`/loop
            // construct emits; `BEGIN`/`END` are the special `BEGIN { }`
            // / `END { }` block-form keywords (kinds 4 / 7) and are
            // distinct from the lowercase `end` closer.
            | R::Def | R::End | R::End2 | R::Class2 | R::Module2
            | R::BEGIN | R::END
            | R::Undef2 | R::Alias2
            // Logical / definedness keywords
            | R::And | R::Or | R::Not | R::DefinedQMARK
            // Structural punctuation acting as operators
            | R::LPAREN | R::LPAREN2 | R::RPAREN | R::RPAREN2
            | R::LBRACE | R::RBRACE | R::LBRACK | R::LBRACK2 | R::LBRACK3 | R::RBRACK
            | R::COMMA | R::SEMI | R::DOT | R::COLONCOLON | R::COLONCOLON2 | R::AMPDOT
            | R::COLON | R::COLON2 | R::HASHLBRACE | R::DASHGT
            // Method-name operator markers (`def +@`, `def -@`, `def ~@`)
            // and indexer methods.
            | R::PLUSAT | R::DASHAT | R::TILDEAT
            | R::LBRACKRBRACK | R::LBRACKRBRACKEQ
            // Arithmetic
            | R::PLUS | R::DASH | R::DASH2 | R::DASH3 | R::DASH4 | R::STAR | R::STAR2 | R::STAR3
            | R::SLASH | R::SLASH2 | R::PERCENT
            | R::STARSTAR | R::STARSTAR2 | R::STARSTAR3
            // Comparison
            | R::EQEQ | R::BANGEQ | R::EQEQEQ
            | R::LT | R::GT | R::LTEQ | R::GTEQ | R::LTEQGT
            | R::EQTILDE | R::BANGTILDE
            // Logical / unary
            | R::AMPAMP | R::PIPEPIPE | R::BANG | R::TILDE
            // Bitwise / shift
            | R::AMP | R::AMP2 | R::PIPE | R::CARET | R::LTLT | R::LTLT2 | R::GTGT
            // Assignment
            | R::EQ | R::EQ2
            | R::PLUSEQ | R::DASHEQ | R::STAREQ | R::SLASHEQ | R::PERCENTEQ
            | R::STARSTAREQ | R::AMPEQ | R::AMPAMPEQ | R::PIPEEQ | R::PIPEPIPEEQ
            | R::CARETEQ | R::LTLTEQ | R::GTGTEQ
            // Hash arrow, ternary, range
            | R::EQGT | R::QMARK | R::DOTDOT | R::DOTDOTDOT
            // Subshell backtick used as method-name marker (def `...)
            | R::BQUOTE
                => HalsteadType::Operator,

            // String-like literals contribute one operand each when inert.
            // If the literal carries an `Interpolation` child the inner
            // expressions are already walked and counted as operands; the
            // wrapping literal would otherwise double-count them
            // (same pattern as C# #183 / Elixir #180).
            R::String | R::ChainedString | R::BareString | R::Subshell
            | R::Regex | R::HeredocBody | R::StringArray | R::SymbolArray
            | R::DelimitedSymbol => {
                Self::string_operand_type(node, &[R::Interpolation as u16])
            }

            // Operands: identifiers and literals.
            R::Identifier | R::IdentifierSuffix | R::IdentifierSuffixToken1
            | R::Constant | R::ConstantSuffix | R::ConstantSuffixToken1
            | R::InstanceVariable | R::ClassVariable | R::GlobalVariable
            | R::Integer | R::Float | R::Complex | R::Rational
            | R::Character | R::SimpleSymbol | R::BareSymbol | R::HashKeySymbol
            // `Nil2` is the leaf `nil` keyword token; `Nil` (named) wraps
            // it. Counting both would double-count every `nil` literal —
            // only the wrapping named node contributes one operand.
            | R::True | R::False | R::Nil
            | R::Zelf | R::Super
            | R::Line | R::File | R::Encoding
                => HalsteadType::Operand,

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Ruby);
}

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

    fn get_op_type(node: &Node) -> HalsteadType {
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
            | False | StringLiteral | NumberLiteral => HalsteadType::Operand,

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
