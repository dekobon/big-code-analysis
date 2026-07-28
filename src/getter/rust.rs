//! `Getter` implementation for Rust.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for RustCode {
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        _ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // we're in a function or in a class or an impl
        // for an impl: we've  'impl ... type {...'
        if let Some(name) = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("type"))
        {
            node_text(code, &name)
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
