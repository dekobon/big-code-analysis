//! `Getter` implementation for Groovy.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for GroovyCode {
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
        if node.kind_id() == Groovy::StaticInitializer as u16 {
            return Some("<static-init>");
        }
        crate::getter::default_func_space_name(node, code, ancestors)
    }

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
            // `Groovy::` qualified: this module imports an explicit list
            // rather than glob-importing, so a bare name here would parse
            // as a binding and match everything.
            MethodDeclaration | ConstructorDeclaration | Closure | Groovy::StaticInitializer => {
                SpaceKind::Function
            }
            SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
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
            // Slashy-string delimiter punctuation. A slashy string
            // (`/xyz/`) is a `StringLiteral` whose closing delimiter is
            // a `SLASH` — the kind id real division uses — so
            // `def b = /xyz/` reported a `/` operator with no division
            // in the source (#1314, the Groovy sibling of Elixir #1256
            // and Ruby/Perl #1312). Only the *closer* is a child: the
            // grammar folds the opening `/` into the literal's own
            // span, so this fabricated one `/` per literal rather than
            // Ruby's two. The `StringLiteral` is already the operand
            // (below), so the delimiter is suppressed exactly when its
            // parent is that node — the compound-leaf guard of
            // grammar-dispatch section 5.
            //
            // Parent, not ancestor, and here that distinction is
            // *observable*: a GString interpolation inside a slashy
            // string (`/x${a / b}y/`) puts a real division under a
            // `StringLiteral` ancestor, and an ancestor-scanning guard
            // would swallow it. Pinned by
            // `groovy_slashy_guard_is_parent_scoped_not_ancestor_scoped`.
            //
            // The other string forms need no arm: `'plain'` is a
            // childless leaf, `"dq"` closes with `"` (134), and
            // dollar-slashy `$/…/$` closes with `/$` (144) — none of
            // which this match classifies. `~/ab.c/` is a
            // `UnaryExpression` wrapping the same `StringLiteral`, so
            // its `~` still counts.
            SLASH
                if ancestors.parent_has_kind(node, StringLiteral as u16) =>
            {
                HalsteadType::Unknown
            }
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

            // `QualifiedName` (a `package` / `import` path) and
            // `QualifiedType` (`java.util.Map` in type position) were
            // listed here until #1263, and `QualifiedName` really did
            // double-count: `package com.example` billed `com`,
            // `example` AND `com.example`. `QualifiedType` never fired,
            // because the runtime emits the *alias* `QualifiedType2`
            // (kind_id 228) that this arm did not name — a latent
            // lesson-2 miss whose only effect was to make the wrong
            // classification unobservable for that half. Both are gone
            // rather than completed: a qualified path is its identifier
            // leaves plus the `.` operator, as in every other language
            // here, and as PHP settled the same call in #1293.
            //
            // Completing the alias list instead is the double count,
            // not the fix. Both halves are pinned against that by
            // `groovy_qualified_name_counts_leaves_not_the_composite_1263`
            // and `groovy_qualified_type_counts_leaves_not_the_composite_1352`.
            // Neither wrapper is ever a name's sole carrier
            // (grammar-dispatch section 6): `import java` still nests an
            // `identifier`, and a single-segment type is a bare
            // `type_identifier` the grammar does not wrap at all.
            Identifier | TypeIdentifier | NullLiteral | True | False | NumberLiteral => {
                HalsteadType::Operand
            }

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
