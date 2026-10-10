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

        if GENERIC_ANGLES.is_closer(node, ancestors) {
            return TokenRole::Unknown;
        }
        // Some guides that informed grammar choice for Halstead
        // keywords, operators, literals: https://docs.oracle.com/javase/specs/jls/se18/html/jls-3.html#jls-3.12
        // https://www.geeksforgeeks.org/software-engineering-halsteads-software-metrics/?msclkid=5e181114abef11ecbb03527e95a34828
        match node.kind_id().into() {
            // `class` declares a class (billed below, #1552) except as the
            // tail of a class literal (`C.class`), where it is an operand:
            // JLS §15.8.2 makes the literal a primary expression, not a
            // literal (§3.10), so its parts bill as they would anywhere
            // else — the type name an operand, the primitive or `void`
            // and the `.` / `[]` operators — and the `class` tail is the
            // operand they act on, as Kotlin's and PHP's `Foo::class` and
            // Groovy's `Foo.class` bill theirs (#1566).
            Class if ancestors.parent_has_kind(node, ClassLiteral as u16) => TokenRole::Operand,
            // `static` is a modifier (below, #1565) except in an import
            // or module directive (`import static`, `requires static`),
            // whose own keyword — `import`, `requires` — bills nothing
            // here, so neither does the word it qualifies.
            Static
                if ancestors.parent(node).is_some_and(|p| {
                    matches!(p.kind_id().into(), ImportDeclaration | RequiresModifier)
                }) =>
            {
                TokenRole::Unknown
            }
            // Operator: control flow
            | If | Else | Switch | Case | Try | Catch | Throw | Throws | Throws2 | For
            | While | Continue | Break | Do | Finally
            // Operator: keywords
            | New | Return | Default | Abstract | Assert | Instanceof | Extends | Final
            | Implements | Transient | Synchronized | VoidType
            // Operator: the rest of the modifiers, billed as C# and Kotlin
            // bill theirs (#1565). Each is its keyword leaf, one kind
            // apiece; the `modifiers` wrapper stays unlisted, so a
            // modifier bills once and an annotation inside it adds only
            // its own tokens. `default` above is one kind for a `switch`
            // label and an interface method's modifier. `sealed` and
            // `permits` (`Permits2`; `Permits` is the clause) are
            // contextual, and the grammar parses each used as a name
            // (`var sealed = 1`, `int permits`) as an `identifier`, so
            // they need no guard; `non-sealed` cannot be a name.
            | Public | Private | Protected | Static | Native | Strictfp | Volatile | Sealed
            | NonDASHsealed | Permits2
            // Operator: type-declaration keywords, billed as C# bills
            // its own (#1552). Only the leaves: no `*_declaration`
            // wrapper is listed, so a declaration bills its keyword once.
            | Class | Interface | Enum | Record | ATinterface
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
            | StringLiteral | CharacterLiteral | HexIntegerLiteral | OctalIntegerLiteral
            | BinaryIntegerLiteral | DecimalIntegerLiteral | HexFloatingPointLiteral
            | DecimalFloatingPointLiteral | This => {
                TokenRole::Operand
            },
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
        Self::get_op_type(node, ancestors)
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
            Java::VoidType => "void",
            // The kind is `boolean_type`; spelled as written, as Groovy's
            // text-keyed primitives are (#1568).
            Java::BooleanType => "boolean",
            _ => typ.into(),
        }
    }
}
