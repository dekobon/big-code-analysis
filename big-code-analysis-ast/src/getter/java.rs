//! `Getter` implementation for Java.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

const GENERIC_ANGLES: GenericAngleKinds = GenericAngleKinds {
    lists: &[Java::TypeArguments as u16, Java::TypeParameters as u16],
    openers: &[Java::LT as u16],
    closers: &[Java::GT as u16],
    is_misparse: never_misparsed,
};

impl Getter for JavaCode {
    /// Names the space, synthesising one for constructs that carry no
    /// name token (#1184).
    ///
    /// `get_func_space_name` returns `Option<&'a str>` borrowed from
    /// `code`, so the only name available for a nameless construct is a
    /// `&'static str` — a per-property spelling like `<get-foo>` would
    /// need a signature change. Angle brackets follow the existing
    /// `<anonymous>` convention and cannot collide with a real
    /// identifier in any of these grammars.
    ///
    /// Sibling collisions are accepted, exactly as multiple
    /// `<anonymous>` siblings already are: two properties each with a
    /// getter, or two `static { }` blocks in one class, produce two
    /// spaces with the same name. Nothing enforces name uniqueness
    /// among siblings, and inventing an index would make the name
    /// unstable under an unrelated edit.
    ///
    /// `<static-init>` rather than the JVM's `<clinit>`: the same
    /// construct exists in JavaScript, where `<clinit>` would mean
    /// nothing, and one spelling across languages is worth more here
    /// than JVM precision.
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // role-bypass: allow(FuncOpener) — naming, not membership: which opener gets the synthetic
        // name
        if node.kind_id() == Java::StaticInitializer as u16 {
            return Some("<static-init>");
        }
        crate::getter::default_func_space_name(node, code, ancestors)
    }

    // The mapping is the `opens` annotations of `roles/java.rs` (#1696):
    // `EnumDeclaration` and `RecordDeclaration` are class-like and share
    // `Class`; `AnnotationTypeDeclaration` maps to `Interface` (#280); an
    // anonymous class opens `Class` only with a body (#463); a record's
    // compact constructor shares the constructor's kind (#1160); `static
    // { … }` is a `Function` (#1184). A lambda opens nothing — the walk
    // never promoted one, and before #1696 this answered `Function` for
    // a node it was never asked about. Java's rows read no bytes and no
    // chain, so the byte-less spelling forwards with none.
    fn get_space_kind(node: &Node) -> SpaceKind {
        Self::get_space_kind_with_code(node, &[], Ancestors::unknown())
    }

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::java::opens_space(node, code, ancestors)
    }

    keyword_roles!(java);

    paying_roles!(java);

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Java::*;

        if GENERIC_ANGLES.is_closer(node, ancestors) {
            return TokenRole::Unknown;
        }
        // Some guides that informed grammar choice for Halstead
        // keywords, operators, literals: https://docs.oracle.com/javase/specs/jls/se18/html/jls-3.html#jls-3.12
        // https://www.geeksforgeeks.org/software-engineering-halsteads-software-metrics/?msclkid=5e181114abef11ecbb03527e95a34828
        // The modifiers, the declaration keywords and the primitive
        // types are not here: `roles/java.rs` declares them, gates
        // included, and `get_keyword_role` bills them (#1693). Nor are
        // the comparison operators, `instanceof`, `&&` and `||`: the
        // `Comparison` and `ShortCircuit` rows declare them and
        // `get_paying_role` bills them (#1694).
        match node.kind_id().into() {
            // Operator: control flow
            // `case` is the `CaseArm` row's and `get_paying_role` bills it
            // (#1695); `default` is the fallback label's and stays.
            If | Else | Switch | Try | Catch | Throw | Throws | Throws2 | For
            | While | Continue | Break | Do | Finally
            // Operator: keywords. `default` is one kind for a `switch`
            // label and an interface method's modifier.
            | New | Return | Default | Assert | Extends | Implements
            // Operator: brackets and comma and terminators (separators)
            | SEMI | COMMA | COLONCOLON | DOT | DASHGT | LBRACE | LBRACK | LPAREN
            // Operator: operators. `?` stays for the spelling the
            // `Ternary` row's `under TernaryExpression` gate leaves out — a
            // wildcard bound (`List<? extends T>`) — which billed before
            // the row existed; the ternary's `?` is the row's.
            // role-bypass: allow(Ternary) — the operator classification of the spelling the row's
            // `under TernaryExpression` gate leaves out: a wildcard bound
            | EQ | BANG | TILDE | QMARK | COLON | PLUSPLUS | DASHDASH
            | PLUS | DASH | STAR | SLASH | AMP | PIPE | CARET | PERCENT | LTLT | GTGT | GTGTGT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | AMPEQ | PIPEEQ | CARETEQ | PERCENTEQ
            | LTLTEQ | GTGTEQ | GTGTGTEQ
            => {
                TokenRole::Operator
            },
            // The `Comparison` row pays `<` / `>` only under a
            // `binary_expression` and `instanceof` only under its
            // expression, the gates ABC needs (#1274, #1461). Halstead
            // bills every other spelling too: the opener of a generic
            // list (`List<String>`, as the `<>` pair — #1559; its closer
            // returned `Unknown` above) and a token error recovery
            // reparents under `ERROR` (`return a < ;`, `x instanceof` at
            // end of input), which billed before the row existed and
            // must not stop. So this arm is unconditional, and for these
            // three kinds the row is not what makes Halstead bill them —
            // the perturbation evidence for the Halstead path is the
            // other six payers.
            // role-bypass: allow(Comparison) — the operator classification of the spellings the row's
            // `under BinaryExpression` / `under InstanceofExpression` gates leave out: a generic
            // opener and a recovered token
            LT | GT | Instanceof => TokenRole::Operator,
            // `super` is the receiver of `super.f()` / `super(x)` /
            // `super::f` everywhere except a wildcard type bound, where
            // `? super String` denotes no value and is the mirror image
            // of `? extends String` — whose `extends` this same match
            // bills as an operator. Both keywords are direct children of
            // the same `wildcard` node, so the parent alone separates
            // the bound from the reference (#1380).
            Super => match ancestors.parent(node).map(|p| p.kind_id().into()) {
                Some(Wildcard) => TokenRole::Operator,
                _ => TokenRole::Operand,
            },
            // `class` as the tail of a class literal (`C.class`) is the
            // operand the `.` acts on (#1566); the `DeclKeyword` row's
            // gate leaves it to this arm, and carries the derivation.
            // role-bypass: allow(DeclKeyword) — the operand classification of the gated-out spelling
            Class if ancestors.parent_has_kind(node, ClassLiteral as u16) => TokenRole::Operand,
            // Operands: variables, constants, literals. `This` joined
            // them in #1380: a self-reference names the receiver a `.`
            // or `::` acts on, so billing it as an operator made
            // `this.x` a binary operator with one operand while `p.x`
            // has one operator and two. The explicit receiver parameter
            // (`void m(J J.this)`) reaches this arm too, and is an
            // operand for the same reason a parameter name is — which is
            // the opposite call from C#'s indexer declarator in
            // `csharp.rs`, deliberately: a parameter name is an operand
            // everywhere here, while a *member* named by a keyword sits
            // with `operator +`.
            //
            // `TypeIdentifier` joined in #1560, as C#, Rust, the C family,
            // Go, Groovy and TypeScript bill type names: a field or
            // parameter type, an `implements` / `extends` entry, a type
            // parameter, a generic argument, each segment of
            // `java.util.List` (the `.` bills as itself), and since #1566
            // the type of a class literal (`Foo.class`). It is the one
            // type-name kind and no listed node wraps it, so each name
            // bills once.
            Identifier | TypeIdentifier | NullLiteral | True | False
            | CharacterLiteral | HexIntegerLiteral | OctalIntegerLiteral
            | BinaryIntegerLiteral | DecimalIntegerLiteral | HexFloatingPointLiteral
            | DecimalFloatingPointLiteral | This => {
                TokenRole::Operand
            },
            // A Java 21 string template (`STR."Hi \{name}!"`) is a
            // `string_literal` holding `string_interpolation` children,
            // whose expressions are walked and billed on their own; the
            // wrapper then yields `Unknown` so `name` is not billed twice
            // (#420's skip, which Java lacked until #1703). The
            // interpolation set is the `StringLit` row's `wraps` list,
            // spelled once in `roles/java.rs`. A plain literal has no
            // such child and stays one operand.
            StringLiteral => {
                Self::string_operand_type(node, crate::roles::java::string_lit::WRAPS)
            }
            _ => {
                TokenRole::Unknown
            },
        }
    }

    // `var x = …` parses `var` as a `type_identifier` (it is a reserved
    // type name, not a keyword token), so the #1560 operand arm would
    // bill an operand spelled `var` that names no type. It stays
    // unbilled, as C#'s `var` (an `implicit_type` wrapping its own
    // keyword) is; a variable *named* `var` is an `identifier` and is
    // unaffected. Only the bytes tell the two type names apart, so the
    // byte-less `get_op_type` above still answers `Operand` for it; the
    // walk calls this spelling (grammar-dispatch section 7).
    fn get_op_type_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> TokenRole {
        if node.kind_id() == Java::TypeIdentifier && node_text(code, node) == Some("var") {
            return TokenRole::Unknown;
        }
        keyword_or_op_type::<Self>(node, code, ancestors)
    }

    fn get_operator_spelling<'a>(
        node: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
    ) -> Option<&'static str> {
        GENERIC_ANGLES.opener_spelling(node, ancestors)
    }

    fn get_operator_id_as_str(id: u16) -> &'static str {
        let typ = id.into();
        match typ {
            Java::LPAREN => "()",
            Java::LBRACK => "[]",
            Java::LBRACE => "{}",
            // role-bypass: allow(Primitive) — naming: how the kind renders, not whether it is one
            Java::VoidType => "void",
            // The kind is `boolean_type`; spelled as written, as Groovy's
            // text-keyed primitives are (#1568).
            // role-bypass: allow(Primitive) — naming, as above
            Java::BooleanType => "boolean",
            _ => typ.into(),
        }
    }
}
