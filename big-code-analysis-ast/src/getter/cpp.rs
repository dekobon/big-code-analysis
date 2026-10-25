//! `Getter` implementation for C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::c_declarator::declarator_name;

// `GT2` is the template closer's own symbol, which the parser folds
// onto `GT` (`cpp_template_closer_alias_never_reaches_kind_id`); it is
// listed defensively.
const GENERIC_ANGLES: GenericAngleKinds = GenericAngleKinds {
    lists: &[
        Cpp::TemplateArgumentList as u16,
        Cpp::TemplateParameterList as u16,
    ],
    openers: &[Cpp::LT as u16],
    closers: &[Cpp::GT as u16, Cpp::GT2 as u16],
    is_misparse: never_misparsed,
};

impl Getter for CppCode {
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        _ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // Issue #285 contract: every `Cpp::FunctionDefinition*` alias
        // must be enumerated here AND in `get_space_kind` below AND
        // in `is_func` / `is_func_space` (see `src/checker.rs`).
        // The aliased kind_ids 489/491/494 are not emitted by the
        // currently pinned `tree-sitter-cpp` parse tables, so a
        // dropped variant would silently fall through to the
        // `_ => name-field` arm and yield the wrong name (or `None`).
        // role-bypass: allow(FuncOpener) — a naming rule, not whether the
        // definition opens a space
        match node.kind_id().into() {
            Cpp::FunctionDefinition
            | Cpp::FunctionDefinition2
            | Cpp::FunctionDefinition3
            | Cpp::FunctionDefinition4 => {
                if let Some(op_cast) = node.first_child(|id| Cpp::OperatorCast == id) {
                    return node_text(code, &op_cast);
                }
                // The name is not a child of the function node — see
                // `crate::c_declarator` (#1208).
                if let Some(name) = declarator_name::<Self>(node)
                    && matches!(
                        name.kind_id().into(),
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
                            | Cpp::TemplateMethod
                    )
                {
                    return node_text(code, &name);
                }
            }
            _ => {
                if let Some(name) = node.child_by_field_name("name") {
                    return node_text(code, &name);
                }
            }
        }
        None
    }

    // The mapping is the `opens` annotations of `roles/cpp.rs`
    // (#1696). The rows read no bytes and no chain, so the byte-less
    // spelling forwards with none.
    fn get_space_kind(node: &Node) -> SpaceKind {
        Self::get_space_kind_with_code(node, &[], Ancestors::unknown())
    }

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::cpp::opens_space(node, code, ancestors)
    }

    keyword_roles!(cpp);

    paying_roles!(cpp);

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Cpp::*;

        if GENERIC_ANGLES.is_closer(node, ancestors) {
            return TokenRole::Unknown;
        }

        // `LPAREN2` here (and the `LBRACK2`/`LBRACK3` aliases in the
        // Elixir/Ruby impls) is a defensive arm, not an active one: every
        // grammar's `public_symbol_map` collapses the second-alias opener
        // to its base before `Node::kind_id()` (`ts_node_symbol`) returns,
        // so `kind_id()` never yields the alias id and the arm cannot fire
        // for real source. It guards against a future grammar bump that
        // drops that collapse — at which point the alias would also need
        // folding to its pair glyph in `get_operator_id_as_str`. The
        // invariant is pinned by `second_alias_opener_collapses_to_base_kind_id`
        // in `metrics/halstead.rs` (issue #768).
        // The modifiers, the declaration keywords and the primitive
        // types are not here: `roles/cpp.rs` declares them and
        // `get_keyword_role` bills them (#1693). The comparison and
        // short-circuit tokens are read from the `Comparison` and
        // `ShortCircuit` rows by `get_paying_role` where they apply
        // their operator; the arm below keeps the spellings the rows'
        // `applied` gate leaves out (#1694).
        match node.kind_id().into() {
            // Raw-string delimiter punctuation. A `raw_string_literal`
            // carries its `R"(` opener as a bare `LPAREN` child — the
            // kind id a call or a grouping uses — so `R"(raw)"`
            // reported a `()` operator with no call in the source
            // (#1314, the C++ sibling of Elixir #1256 and Ruby/Perl
            // #1312). The literal is already an operand (below), so the
            // delimiter is suppressed exactly when its parent is that
            // node — the compound-leaf guard of grammar-dispatch
            // section 5. Verified across `R"(x)"`, the custom-delimiter
            // `R"tag(x)tag"` (which adds a `raw_string_delimiter` but
            // keeps the same `(`), and the `LR` / `u8R` prefixed forms;
            // the closing `)` needs no arm because #695 dropped every
            // closer from the operator set.
            //
            // Parent, not ancestor. That distinction is unobservable
            // here — `raw_string_content` is a leaf, so no `LPAREN` is
            // ever a deeper descendant of a raw string — but parent
            // scoping is correct by construction and keeps this arm the
            // same shape as its siblings. `mozcpp` carries the twin.
            LPAREN
                if ancestors.parent_has_kind(node, RawStringLiteral as u16) =>
            {
                TokenRole::Unknown
            }
            // The comparison and short-circuit tokens (`<` … `<=>`,
            // `not_eq`, `&&`, `||`, `and`, `or`) sit in this arm for the
            // spellings that apply no operator — an overload name
            // (`operator<`, `operator and`), a reference declarator or
            // ref-qualifier (`int&& x`, `void f() &&`), a requires-
            // clause's constraint conjunction (`requires A<T> && B<T>`),
            // a template delimiter's `<` (the `<>` pair; its closer
            // returned `Unknown` above) and a token error recovery
            // reparents — which bill as the operators they spell, as
            // they did before the rows existed. The rows pay the applied
            // spellings and this arm the rest; for these kinds the
            // Halstead vocabulary is the union, so dropping one from a
            // row moves nothing here. The ternary `?` and the `case` label
            // are not here: the `Ternary` and `CaseArm` rows pay them
            // ungated.
            // role-bypass: allow(Comparison, ShortCircuit) — the operator classification of the
            // spellings the rows' `applied` gate leaves out
            LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | GT2 | LTEQGT | NotEq | AMPAMP | PIPEPIPE
            | And | Or
            | DOT | DOTSTAR | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI | Return
            | Break | Continue | If | Else | Switch | Default | For | While | Goto | Do
            | Delete | New | Try | Try2 | Catch | Throw | EQ | DASH
            | DASHDASH | DASHGT | DASHGTSTAR | PLUS | PLUSPLUS | SLASH | PERCENT | PIPE | AMP
            | LTLT | TILDE | PLUSEQ
            | DASHEQ | BANG | STAREQ | SLASHEQ | PERCENTEQ | GTGTEQ | LTLTEQ | AMPEQ | CARET
            | CARETEQ | PIPEEQ | LBRACK | LBRACE | COLONCOLON | Sizeof
            // The ISO alternative tokens ([lex.digraph]): `not`, `compl`,
            // `and`, `or`, `bitand`, `bitor`, `xor`, `not_eq`, `and_eq`,
            // `or_eq`, `xor_eq` spell `!`, `~`, `&&`, `||`, `&`, `|`, `^`,
            // `!=`, `&=`, `|=`, `^=`. The grammar gives each a kind of its
            // own, so without these they fell into `Unknown` and `b and c`
            // billed no operator at all (#1522). Each kind occurs only in
            // productions its symbol also occurs in, so it needs no gate
            // the symbol lacks. Keyed by `kind_id` like every operator, so
            // `and` and `&&` are two distinct operators in n1: the `ops`
            // vocabulary names operators by kind, and folding the two in
            // the metric alone would make the two stores disagree. `and`,
            // `or` and `not_eq` sit in the arm above with their symbols.
            | Not | Compl | Bitand | Bitor | Xor | AndEq | OrEq | XorEq
            // An overloaded operator's name (#1296). `operator +` already
            // billed its `+` through the punctuation kind, but the grammar
            // spells four overloadable names as whole tokens of their own
            // — `[]`, `()`, the `""` of a user-defined literal and
            // `co_await` — and none was in either arm, so
            // `operator[]` and `operator()` billed nothing for the name
            // that tells them apart. `Operator` is the `operator` keyword
            // itself, which C# bills the same way. `CoAwait` is also the
            // keyword of `co_await expr`, an operator there too. Each
            // token occurs only under `operator_name`, `operator_cast` or
            // `co_await_expression`, none of which is classified, so
            // nothing bills one twice (grammar-dispatch section 5).
            //
            // `LPARENRPAREN` / `LBRACKRBRACK` are kinds of their own, not
            // the `LPAREN` / `LBRACK` of a call or subscript, so declaring
            // `operator[]` and applying `s[1]` are two `n1` entries, which
            // `bca ops` renders `operator[]` and `[]` (see
            // `get_operator_id_as_str`).
            | LPARENRPAREN | LBRACKRBRACK | DQUOTEDQUOTE | CoAwait | Operator
            // `co_return` and `co_yield` are the coroutine spellings of
            // `return` and of handing a value back, so they bill as
            // `return` does (#1547). Each is the keyword leaf of a
            // `co_return_statement` / `co_yield_statement`, which no arm
            // classifies, so nothing bills one twice.
            | CoReturn | CoYield
                => TokenRole::Operator,
            // `CharLiteral` — the full derivation lives on the same arm
            // in `src/getter/c.rs` (#1316): the wrapper is the only
            // classified node in a character literal, so it bills one
            // operand per literal, keyed by text, and `Checker::is_string`
            // deliberately stays without a `CharLiteral` arm.
            //
            // `This` was in *neither* arm until #1361, so a C++ `this`
            // contributed nothing at all: not an operator and not an
            // operand. Same shape as #1316, opposite of the #1351-#1355
            // wrapper/leaf over-counts — the fix adds an arm rather than
            // deleting one.
            //
            // Operand, not operator, on three grounds. **Structure**:
            // `field_expression` is `<receiver> -> <field>`, and `p->x`
            // already bills operands `p` / `x` around the `->` operator.
            // Calling `this` an operator would make `this->x` a binary
            // operator with one operand, and would score the identical
            // AST differently for `this->x` than for `p->x`.
            // **Role**: `this` is a pointer *rvalue* and stands exactly
            // where a variable stands — `return this;`, `*this`,
            // `f(this)`, which would otherwise pass an operator as a
            // call argument — while `->` / `.` / `*` are the operators
            // already counted acting on it. **Precedent**: eleven of the
            // fourteen languages here bill their self-reference as an
            // operand (Rust `Zelf`, Ruby `Zelf`, PHP's `$this`
            // `variable_name`, the four JS-family `This` arms, and
            // Python / ObjC / Groovy / Lua, whose grammars spell it a
            // plain `identifier`), and the JS-family arm in
            // `src/getter.rs` is the only *reasoned* one among them —
            // its `MetaProperty` note calls `this` "one atomic
            // operand". Java / C# / Kotlin disagree, but each
            // swept `This` in as one entry in a run of keywords — under
            // `// Operator: keywords`, `// Operator: other keywords` and
            // `// Expression-keyword operators` respectively — grouping
            // it by lexical class rather than by Halstead role; that
            // three-way split is #1380, deliberately not settled here.
            // Keeping C++ an operand also keeps it agreeing with ObjC,
            // whose `self` is an operand and whose `.mm` files route to
            // this very impl.
            //
            // Safe against a grammar-dispatch section 5 double count:
            // `this` is a childless leaf in every position the grammar
            // admits it — `this->x`, `(*this)`, `return this`, `f(this)`,
            // the `[this]` and `[=, this]` lambda captures, a
            // `decltype(this->x)` trailing return type, and a
            // constructor body after a member-initialiser list — and no
            // node that *contains* it (`field_expression`,
            // `pointer_expression`, `lambda_capture_specifier`,
            // `argument_list`) is classified, so it can double-count in
            // neither direction. Neither generated enum aliases the rule
            // (`This = 215` is the sole variant spelling `"this"` in
            // `language_cpp.rs` and `language_mozcpp.rs`), so there is no
            // `This2` for a single-variant arm to miss;
            // `cpp_this_is_a_childless_unaliased_leaf` pins both facts.
            //
            // C++23's explicit object parameter (`int g(this S&& self)`)
            // does *not* reach this arm: the pinned grammar cannot parse
            // it and emits `type_identifier "this"` plus an `ERROR`. That
            // spelling is already an operand via `TypeIdentifier`, so the
            // construct is unaffected either way.
            Identifier | TypeIdentifier | FieldIdentifier | RawStringLiteral | StringLiteral
            | CharLiteral | NumberLiteral | True | False | Null | This | DOTDOTDOT => {
                TokenRole::Operand
            }
            // A namespace identifier is an operand only where it
            // *names* a namespace; the same kind also spells the
            // qualifier in `ns::thing`, which the final arm leaves
            // `Unknown` (#1096).
            NamespaceIdentifier if ancestors.parent_has_kind(node, NamespaceDefinition as u16) => {
                TokenRole::Operand
            }
            _ => TokenRole::Unknown,
        }
    }

    fn get_operator_spelling<'a>(
        node: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
    ) -> Option<&'static str> {
        GENERIC_ANGLES.opener_spelling(node, ancestors)
    }

    // `get_operator!`'s three bracket arms, plus the two overloaded
    // operator names (#1296). The grammar names `LPARENRPAREN` /
    // `LBRACKRBRACK` `()` / `[]`, the same text a call's and a
    // subscript's opener render as, so `bca ops` would list one spelling
    // for two `n1` entries. Rendering each as the operator it names keeps
    // every listed operator distinct (lesson 4).
    fn get_operator_id_as_str(id: u16) -> &'static str {
        let typ = id.into();
        match typ {
            Cpp::LPAREN => "()",
            Cpp::LBRACK => "[]",
            Cpp::LBRACE => "{}",
            Cpp::LPARENRPAREN => "operator()",
            Cpp::LBRACKRBRACK => "operator[]",
            _ => typ.into(),
        }
    }
}
