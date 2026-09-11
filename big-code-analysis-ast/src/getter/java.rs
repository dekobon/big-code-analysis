//! `Getter` implementation for Java.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

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
        if node.kind_id() == Java::StaticInitializer as u16 {
            return Some("<static-init>");
        }
        crate::getter::default_func_space_name(node, code, ancestors)
    }

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
            // `CompactConstructorDeclaration` is a record's compact
            // constructor (`R { … }`); it is a constructor with an
            // implicit parameter list, so it shares the canonical
            // constructor's space kind (#1160). It carries a required
            // `name` field holding the record's simple name, so the
            // default `get_func_space_name` already names the space `R` —
            // identical to what the canonical spelling would produce.
            // `StaticInitializer` is `static { … }` (#1184).
            MethodDeclaration
            | ConstructorDeclaration
            | CompactConstructorDeclaration
            | LambdaExpression
            | StaticInitializer => SpaceKind::Function,
            InterfaceDeclaration | AnnotationTypeDeclaration => SpaceKind::Interface,
            // An anonymous class (`new Runnable() { ... }`) is an
            // `object_creation_expression` carrying a `class_body` child;
            // it opens a Class space, matching PHP's `AnonymousClass` and
            // C#'s anonymous forms (#463). A plain `new Foo()` has no
            // `class_body`, so it falls through to `Unknown` (no space).
            // The shared helper keeps this gate identical to the one in
            // `JavaCode::is_func_space`.
            ObjectCreationExpression
                if crate::checker::java_anonymous_class_body(node).is_some() =>
            {
                SpaceKind::Class
            }
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Java::*;
        // Some guides that informed grammar choice for Halstead
        // keywords, operators, literals: https://docs.oracle.com/javase/specs/jls/se18/html/jls-3.html#jls-3.12
        // https://www.geeksforgeeks.org/software-engineering-halsteads-software-metrics/?msclkid=5e181114abef11ecbb03527e95a34828
        match node.kind_id().into() {
            // Operator: control flow
            | If | Else | Switch | Case | Try | Catch | Throw | Throws | Throws2 | For
            | While | Continue | Break | Do | Finally
            // Operator: keywords
            | New | Return | Default | Abstract | Assert | Instanceof | Extends | Final
            | Implements | Transient | Synchronized | VoidType
            // Operator: brackets and comma and terminators (separators)
            | SEMI | COMMA | COLONCOLON | DOT | DASHGT | LBRACE | LBRACK | LPAREN
            // Operator: operators
            | EQ | LT | GT | BANG | TILDE | QMARK | COLON
            | EQEQ | LTEQ | GTEQ | BANGEQ | AMPAMP | PIPEPIPE | PLUSPLUS | DASHDASH
            | PLUS | DASH | STAR | SLASH | AMP | PIPE | CARET | PERCENT | LTLT | GTGT | GTGTGT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | AMPEQ | PIPEEQ | CARETEQ | PERCENTEQ
            | LTLTEQ | GTGTEQ | GTGTGTEQ
            // primitive types
            | Byte | Short | Int | Long | Char | Float | Double | BooleanType
            => {
                TokenRole::Operator
            },
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
            Identifier | NullLiteral | ClassLiteral | True | False | StringLiteral
            | CharacterLiteral | HexIntegerLiteral | OctalIntegerLiteral
            | BinaryIntegerLiteral | DecimalIntegerLiteral | HexFloatingPointLiteral
            | DecimalFloatingPointLiteral | This => {
                TokenRole::Operand
            },
            _ => {
                TokenRole::Unknown
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
