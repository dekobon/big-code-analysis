// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use regex::bytes::Regex;

use crate::cfg_predicate::attribute_marks_test as rust_attribute_marks_test;
use crate::macros::csharp_invocation_expr_kinds;
use crate::*;

static AHO_CORASICK: OnceLock<AhoCorasick> = OnceLock::new();
static RE: OnceLock<Regex> = OnceLock::new();

// Shared ancestor-walk scaffold behind `check_if_func!` /
// `check_if_arrow_func!`. Both decide whether a JS `function_expression`
// / `arrow_function` is a *named function* (vs. an anonymous closure) by
// counting ancestors that bind the expression to a name (`$up` kinds:
// `var x = …`, `x = …`, `label:`, object `pair`) while stopping the walk
// at frames that prove the expression is used positionally (`$stop`
// kinds: a block, return, `new`, call/arguments). A positive count, or a
// per-call `$extra` adjacency check, marks it a named function.
//
// This must stay a `macro_rules!` (not a `fn`): the `$up` / `$stop`
// variant lists are matched against each JS-family language's own
// `kind_id` enum, brought into scope by a per-language `use $language::*`
// glob at the call site. A function could not name variants that only
// exist after that glob import.
//
// Kept token-identical to the two hand-written predicates it replaced:
// the bracketed variant lists expand straight into the `matches!`
// patterns and `$extra` into the trailing `|| …` disjunct.
macro_rules! js_ancestor_walk {
    (
        $parser:ident,
        $node:ident,
        [$($up:ident)|+],
        [$($stop:ident)|+],
        $extra:expr $(,)?
    ) => {
        $node.count_specific_ancestors::<$parser>(
            |node| matches!(node.kind_id().into(), $($up)|+),
            |node| matches!(node.kind_id().into(), $($stop)|+),
        ) > 0
            || $extra
    };
}

macro_rules! check_if_func {
    ($parser: ident, $node: ident) => {
        js_ancestor_walk!(
            $parser,
            $node,
            [VariableDeclarator | AssignmentExpression | LabeledStatement | Pair],
            [StatementBlock | ReturnStatement | NewExpression | Arguments],
            $node.is_child(Identifier as u16),
        )
    };
}

macro_rules! check_if_arrow_func {
    ($parser: ident, $node: ident) => {
        js_ancestor_walk!(
            $parser,
            $node,
            [VariableDeclarator | AssignmentExpression | LabeledStatement],
            [StatementBlock | ReturnStatement | NewExpression | CallExpression],
            $node.has_sibling(PropertyIdentifier as u16),
        )
    };
}

macro_rules! is_js_func {
    ($parser: ident, $node: ident) => {
        match $node.kind_id().into() {
            FunctionDeclaration | MethodDefinition => true,
            FunctionExpression => check_if_func!($parser, $node),
            ArrowFunction => check_if_arrow_func!($parser, $node),
            _ => false,
        }
    };
}

macro_rules! is_js_closure {
    ($parser: ident, $node: ident) => {
        match $node.kind_id().into() {
            GeneratorFunction | GeneratorFunctionDeclaration => true,
            FunctionExpression => !check_if_func!($parser, $node),
            ArrowFunction => !check_if_arrow_func!($parser, $node),
            _ => false,
        }
    };
}

macro_rules! is_js_func_and_closure_checker {
    ($parser: ident, $language: ident) => {
        #[inline]
        fn is_func(node: &Node) -> bool {
            use $language::*;
            is_js_func!($parser, node)
        }

        #[inline]
        fn is_closure(node: &Node) -> bool {
            use $language::*;
            is_js_closure!($parser, node)
        }
    };
}

// Generate an `is_string` impl for a JS-family `Checker` block. The
// MozJS / JavaScript / TypeScript grammars expose `String2` as the
// anonymous `"string"` keyword alias for `String`; TSX additionally
// exposes `String3` (the JSX-attribute string production). The
// alterator flattens these aliases; the generic `string` filter must
// agree (issue #283).
macro_rules! impl_js_family_is_string {
    ($lang:ident $(, $extra:ident)* $(,)?) => {
        fn is_string(node: &Node) -> bool {
            matches!(
                node.kind_id().into(),
                $lang::String | $lang::String2 | $lang::TemplateString
                    $(| $lang::$extra)*
            )
        }
    };
}

// Generate an `is_string` impl for languages whose `is_string`
// predicate is a flat `matches!` against one or more variant
// kinds. Reduces drift risk for new alias additions and gives a
// single table that answers "which kinds count as a string for
// `find string` / `count string`?" per language (issue #301).
//
// Languages whose `is_string` needs anything beyond a flat variant
// list (e.g. JS family's `String` + `String2` + `TemplateString`
// pattern) keep their own dedicated macros or impls.
macro_rules! impl_simple_is_string {
    ($lang:ident, $first:ident $(, $rest:ident)* $(,)?) => {
        fn is_string(node: &Node) -> bool {
            matches!(
                node.kind_id().into(),
                $lang::$first $(| $lang::$rest)*
            )
        }
    };
}

#[inline]
fn get_aho_corasick_match(code: &[u8]) -> bool {
    AHO_CORASICK
        .get_or_init(|| AhoCorasick::new(vec![b"<div rustbindgen"]).unwrap())
        .is_match(code)
}

#[doc(hidden)]
pub trait Checker {
    fn is_comment(_: &Node) -> bool;
    fn is_useful_comment(_: &Node, _: &[u8]) -> bool;
    fn is_func_space(_: &Node) -> bool;
    fn is_func(_: &Node) -> bool;
    fn is_closure(_: &Node) -> bool;
    fn is_call(_: &Node) -> bool;
    fn is_non_arg(_: &Node) -> bool;
    fn is_string(_: &Node) -> bool;
    fn is_else_if(_: &Node) -> bool;
    fn is_primitive(_id: u16) -> bool;

    fn is_error(node: &Node) -> bool {
        node.has_error()
    }

    /// Return `true` to elide this node and all its descendants from
    /// every metric. Used by language modules to filter
    /// test-only / generated / preprocessor-disabled subtrees.
    ///
    /// The default returns `false` for every node, preserving the
    /// pre-#182 behavior. Language overrides drive opt-in skips
    /// (currently: `RustCode` filters `#[cfg(test)]` items, gated
    /// by the runtime `MetricsOptions::exclude_tests` flag).
    #[inline]
    fn should_skip_subtree(_node: &Node, _code: &[u8]) -> bool {
        false
    }

    /// Source-aware variant of [`is_func_space`]. The default forwards
    /// to the byte-less predicate so languages whose function-space
    /// classification is encoded in distinct grammar productions (Java,
    /// Rust, Python, …) need no override. Languages whose function
    /// boundaries are macro-shaped — Elixir's `def` / `defp` /
    /// `defmacro` / `defmacrop` / `defmodule` — override this to
    /// disambiguate `Call` nodes by their target identifier text
    /// (#275).
    #[inline]
    fn is_func_space_with_code(node: &Node, _code: &[u8]) -> bool {
        Self::is_func_space(node)
    }

    /// Source-aware variant of [`is_func`]. Same rationale as
    /// [`is_func_space_with_code`] (#275).
    #[inline]
    fn is_func_with_code(node: &Node, _code: &[u8]) -> bool {
        Self::is_func(node)
    }

    /// Combined predicate the walker uses to decide whether to promote
    /// `node` to a new function-space frame. Default forwards to
    /// `is_func_with_code || is_func_space_with_code` so existing
    /// languages need no override — each kept the freedom to expose
    /// `is_func` and `is_func_space` as disjoint sets (Rust includes
    /// closures in `is_func_space`, Java keeps lambdas out, …) and
    /// this method preserves that flexibility while letting Elixir
    /// halve its per-`Call` source-text lookups: a single
    /// `elixir_call_keyword` call answers both halves at once
    /// (#310 follow-on perf).
    #[inline]
    fn promotes_to_func_space_with_code(node: &Node, code: &[u8]) -> bool {
        Self::is_func_with_code(node, code) || Self::is_func_space_with_code(node, code)
    }
}

impl Checker for PreprocCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Preproc::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(_: &Node) -> bool {
        false
    }

    fn is_func(_: &Node) -> bool {
        false
    }

    fn is_closure(_: &Node) -> bool {
        false
    }

    fn is_call(_: &Node) -> bool {
        false
    }

    fn is_non_arg(_: &Node) -> bool {
        false
    }

    impl_simple_is_string!(Preproc, StringLiteral, RawStringLiteral);

    fn is_else_if(_: &Node) -> bool {
        false
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for CcommentCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Ccomment::Comment
    }

    fn is_useful_comment(node: &Node, code: &[u8]) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    fn is_func_space(_: &Node) -> bool {
        false
    }

    fn is_func(_: &Node) -> bool {
        false
    }

    fn is_closure(_: &Node) -> bool {
        false
    }

    fn is_call(_: &Node) -> bool {
        false
    }

    fn is_non_arg(_: &Node) -> bool {
        false
    }

    impl_simple_is_string!(Ccomment, StringLiteral, RawStringLiteral);

    fn is_else_if(_: &Node) -> bool {
        false
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for CppCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Cpp::Comment
    }

    fn is_useful_comment(node: &Node, code: &[u8]) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    // Issue #285 contract: every `Cpp::FunctionDefinition*` alias must
    // be enumerated here AND in `is_func`, `get_func_space_name`, and
    // `get_space_kind` (see `src/getter.rs`). Aliased kind_ids
    // 489/491/494 are not emitted by the currently pinned
    // `tree-sitter-mozcpp` parse tables on any input we can construct,
    // so a missing variant won't fail a parse-and-assert test — it
    // will silently drop those nodes from FuncSpace creation the next
    // time a grammar bump starts emitting them (see lesson 2 in
    // `docs/development/lessons_learned.md`).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::TranslationUnit
                | Cpp::FunctionDefinition
                | Cpp::FunctionDefinition2
                | Cpp::FunctionDefinition3
                | Cpp::FunctionDefinition4
                | Cpp::StructSpecifier
                | Cpp::ClassSpecifier
                | Cpp::NamespaceDefinition
        )
    }

    // Issue #285 contract: keep this in sync with `is_func_space` and
    // the C++ getters — see comment above `is_func_space`.
    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::FunctionDefinition
                | Cpp::FunctionDefinition2
                | Cpp::FunctionDefinition3
                | Cpp::FunctionDefinition4
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Cpp::LambdaExpression
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Cpp::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::LPAREN | Cpp::LPAREN2 | Cpp::COMMA | Cpp::RPAREN
        )
    }

    impl_simple_is_string!(Cpp, StringLiteral, ConcatenatedString, RawStringLiteral);

    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Cpp::IfStatement {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Cpp::ElseClause;
        }
        false
    }

    #[inline]
    fn is_primitive(id: u16) -> bool {
        id == Cpp::PrimitiveType
    }
}

impl Checker for PythonCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Python::Comment
    }

    fn is_useful_comment(node: &Node, code: &[u8]) -> bool {
        // comment containing coding info are useful
        node.start_row() <= 1
            && RE
                .get_or_init(|| {
                    Regex::new(r"^[ \t\f]*#.*?coding[:=][ \t]*([-_.a-zA-Z0-9]+)").unwrap()
                })
                .is_match(&code[node.start_byte()..node.end_byte()])
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Python::Module | Python::FunctionDefinition | Python::ClassDefinition
        )
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Python::FunctionDefinition
    }

    fn is_closure(node: &Node) -> bool {
        // Route through the single lambda-alias chokepoint so closure
        // detection here and the cognitive lambda-scope walks accept the
        // exact same set: `Lambda` (196, the concrete production emitted
        // today) and `Lambda2` (197, the currently-unseen hidden alias).
        // `Lambda3` (73, the `lambda` keyword token) is intentionally
        // excluded. Centralizing means a future grammar bump that
        // promotes `Lambda2` cannot silently undercount closures in
        // nom/nargs or desync from cognitive (issues #419/#422; lesson 2
        // in lessons_learned.md). The drift-guard test below asserts
        // `Lambda2` stays unseen until then.
        crate::metrics::cognitive::python_is_lambda(node)
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Python::Call
    }

    fn is_non_arg(node: &Node) -> bool {
        // tree-sitter-python emits the PEP 570 positional-only marker `/`
        // as a `positional_separator` node and the PEP 3102 keyword-only
        // marker `*` as a `keyword_separator` node, both as direct children
        // of the `parameters` list. They are punctuation, not parameters, so
        // they must be excluded or they inflate nargs by one each (issue #414).
        matches!(
            node.kind_id().into(),
            Python::LPAREN
                | Python::COMMA
                | Python::RPAREN
                | Python::PositionalSeparator
                | Python::KeywordSeparator
        )
    }

    impl_simple_is_string!(Python, String, ConcatenatedString);

    // Python models `elif` as a dedicated `elif_clause` node, which is
    // handled directly by cognitive/cyclomatic dispatch as a branch
    // extension — `is_else_if` is intentionally never invoked for
    // `elif_clause` because it is not an `if_statement` and is not in
    // any of the structural kind sets that `count_specific_ancestors`
    // walks for nesting (issue #274).
    //
    // `else: if x: ...` chains also exist — semantically equivalent to
    // `else if` — but the grammar wraps the inner `if_statement` in a
    // `block` node, so the shape is `else_clause → block → if_statement`
    // rather than the direct `else_clause → if_statement` used by
    // C++/JS/TS/TSX/Rust. Match the chained shape by walking through
    // the `block` and requiring the inner `if` to be the block's sole
    // named child; sibling statements would mean a real nested-if, not
    // a chain (issue #276).
    //
    // `block` has two aliased kind_ids in tree-sitter-python
    // (`Block` = 135, `Block2` = 160 — both surface as `"block"`); we
    // accept either via `python_is_block`, the single normalization
    // point for the aliases (issue #419; lesson 2 in
    // `docs/development/lessons_learned.md`).
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Python::IfStatement
            && node.parent().is_some_and(|parent| {
                crate::metrics::npa::python_is_block(&parent)
                    && parent.children().filter(Node::is_named).count() == 1
                    && parent
                        .parent()
                        .is_some_and(|gp| gp.kind_id() == Python::ElseClause)
            })
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

/// Returns the `class_body` child of a Java `object_creation_expression`
/// when the node is an anonymous class (`new Runnable() { ... }`), or
/// `None` for a plain constructor call (`new Foo()`). Shared by
/// `JavaCode::is_func_space` and `JavaCode::get_space_kind` so both agree
/// on exactly which `object_creation_expression` nodes open a Class space
/// (#463); a lambda is a distinct `lambda_expression` node and never
/// reaches this path.
pub(crate) fn java_anonymous_class_body<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    node.first_child(|id| id == Java::ClassBody as u16)
}

impl Checker for JavaCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Java::LineComment || node.kind_id() == Java::BlockComment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    // `EnumDeclaration`, `RecordDeclaration`, and `AnnotationTypeDeclaration`
    // are class-like declarations that can contain fields and methods,
    // so they open a class space alongside `ClassDeclaration` /
    // `InterfaceDeclaration` (issue #280). Without them, `Npa`/`Npm`/`Wmc`
    // never see their bodies as class scopes and silently produce zero
    // counts. Annotation types map to `Interface` in `get_space_kind`
    // (their elements are abstract methods at the bytecode level);
    // enums and records map to `Class`.
    //
    // An `object_creation_expression` carrying a `class_body` child is
    // an anonymous class (`new Runnable() { ... }`); it opens its own
    // Class space so its members are attributed to it, not the
    // enclosing method (#463). A plain `new Foo()` has no `class_body`
    // child and must not open a space, so the arm is gated on the
    // body's presence. This mirrors PHP's `AnonymousClass` handling and
    // brings Java to parity with PHP/C# anonymous forms.
    fn is_func_space(node: &Node) -> bool {
        if node.kind_id() == Java::ObjectCreationExpression as u16 {
            return java_anonymous_class_body(node).is_some();
        }
        matches!(
            node.kind_id().into(),
            Java::Program
                | Java::ClassDeclaration
                | Java::InterfaceDeclaration
                | Java::EnumDeclaration
                | Java::RecordDeclaration
                | Java::AnnotationTypeDeclaration
        )
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Java::MethodDeclaration || node.kind_id() == Java::ConstructorDeclaration
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Java::LambdaExpression
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Java::MethodInvocation
    }

    fn is_non_arg(node: &Node) -> bool {
        // Java's explicit receiver parameter (`void m(S this, int a)`, JLS
        // 8.4.1) parses as a `receiver_parameter` child of
        // `formal_parameters`, distinct from a real `formal_parameter`. It
        // binds `this`, not a value, so it is not a formal parameter and
        // must be excluded — matching Rust's `SelfParameter` (#457), Go's
        // `receiver` field, and C++'s implicit `this` (#470).
        matches!(
            node.kind_id().into(),
            Java::LPAREN | Java::COMMA | Java::RPAREN | Java::ReceiverParameter
        )
    }

    impl_simple_is_string!(Java, StringLiteral, MultilineStringLiteral);

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // tree-sitter-java models `else if` as an `Else` keyword token followed
        // by a nested `if_statement` (no wrapping `else_clause` node).
        node.kind_id() == Java::IfStatement
            && node
                .previous_sibling()
                .is_some_and(|prev| prev.kind_id() == Java::Else)
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

/// Counts the `accessor_declaration` children (`get` / `set` / `init`) of a C#
/// `property_declaration` / `indexer_declaration` by walking its
/// `accessor_list`. Returns `0` for the expression-bodied form
/// (`this[int i] => _d[i];` / `int W => _w;`), which has no accessor list.
///
/// Shared by `csharp_member_has_accessors` (here) and the npm reference
/// `csharp_count_member`, which keeps its own `.max(1)` fallback so an
/// accessor-less expression-bodied member still counts as one method (#464).
pub(crate) fn csharp_accessor_count(node: &Node) -> usize {
    node.children()
        .filter(|c| c.kind_id() == Csharp::AccessorList as u16)
        .flat_map(|list| list.children())
        .filter(|c| c.kind_id() == Csharp::AccessorDeclaration as u16)
        .count()
}

/// Returns `true` when a C# `indexer_declaration` / `property_declaration`
/// carries bodied accessors — an `accessor_list` containing at least one
/// `accessor_declaration` (`get` / `set` / `init`). Returns `false` for the
/// expression-bodied form (`this[int i] => _d[i];` / `int W => _w;`), which
/// has no accessor list and defines a single implicit getter.
///
/// Shared by `CsharpCode::is_func` / `is_func_space` and
/// `CsharpCode::get_space_kind` so all three agree on when an indexer or
/// property opens its own Function space versus deferring to its accessor
/// children. This mirrors the npm reference (`csharp_count_member`): the
/// member counts as its accessor count, falling back to 1 for the
/// accessor-less expression-bodied form (#464 indexer, #472 property).
pub(crate) fn csharp_member_has_accessors(node: &Node) -> bool {
    csharp_accessor_count(node) > 0
}

impl Checker for CsharpCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Csharp::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    // A bodied indexer (`this[int i] { get; set; }`) or property
    // (`int X { get; set; }`) defers to its `accessor_declaration` children
    // for its function spaces — counting it here too double-counts (property,
    // #472) or triple-counts (indexer, #464) the member in nom/wmc. Only the
    // accessor-less expression-bodied form (`this[int i] => _d[i];` /
    // `int W => _w;`) opens a space directly, matching the npm `.max(1)`
    // fallback and the way the implicit getter is the sole callable.
    fn is_func_space(node: &Node) -> bool {
        if matches!(
            node.kind_id().into(),
            Csharp::IndexerDeclaration | Csharp::PropertyDeclaration
        ) {
            return !csharp_member_has_accessors(node);
        }
        matches!(
            node.kind_id().into(),
            Csharp::CompilationUnit
                | Csharp::ClassDeclaration
                | Csharp::StructDeclaration
                | Csharp::RecordDeclaration
                | Csharp::InterfaceDeclaration
                | Csharp::EnumDeclaration
                | Csharp::MethodDeclaration
                | Csharp::ConstructorDeclaration
                | Csharp::DestructorDeclaration
                | Csharp::LocalFunctionStatement
                | Csharp::LambdaExpression
                | Csharp::AnonymousMethodExpression
                | Csharp::AccessorDeclaration
                | Csharp::OperatorDeclaration
                | Csharp::ConversionOperatorDeclaration
        )
    }

    fn is_func(node: &Node) -> bool {
        if matches!(
            node.kind_id().into(),
            Csharp::IndexerDeclaration | Csharp::PropertyDeclaration
        ) {
            return !csharp_member_has_accessors(node);
        }
        matches!(
            node.kind_id().into(),
            Csharp::MethodDeclaration
                | Csharp::ConstructorDeclaration
                | Csharp::DestructorDeclaration
                | Csharp::LocalFunctionStatement
                | Csharp::AccessorDeclaration
                | Csharp::OperatorDeclaration
                | Csharp::ConversionOperatorDeclaration
        )
    }

    fn is_closure(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::LambdaExpression | Csharp::AnonymousMethodExpression
        )
    }

    fn is_call(node: &Node) -> bool {
        // The C# grammar emits three aliased `kind_id`s for
        // `invocation_expression`; matching only the unsuffixed variant
        // silently drops the rest (lesson #2 in lessons_learned.md).
        matches!(node.kind_id().into(), csharp_invocation_expr_kinds!())
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::LPAREN | Csharp::COMMA | Csharp::RPAREN
        )
    }

    impl_simple_is_string!(
        Csharp,
        StringLiteral,
        VerbatimStringLiteral,
        RawStringLiteral,
        InterpolatedStringExpression,
    );

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // tree-sitter-c-sharp models `else if` as an `Else` keyword token
        // followed by a nested `if_statement` (no wrapping `else_clause` node).
        node.kind_id() == Csharp::IfStatement
            && node
                .previous_sibling()
                .is_some_and(|prev| prev.kind_id() == Csharp::Else)
    }

    #[inline]
    fn is_primitive(id: u16) -> bool {
        // Without this, every `PredefinedType` keyword (`int`, `string`,
        // `bool`, `object`, …) collapses into a single Halstead operator
        // because they share one `kind_id`. Returning `true` here routes
        // them through the lexeme-keyed `primitive_operators` map so
        // distinct keywords count as distinct operators (issue #286).
        id == Csharp::PredefinedType as u16
    }
}

impl Checker for MozjsCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Mozjs::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozjs::Program
                | Mozjs::FunctionExpression
                | Mozjs::Class
                | Mozjs::GeneratorFunction
                | Mozjs::FunctionDeclaration
                | Mozjs::MethodDefinition
                | Mozjs::GeneratorFunctionDeclaration
                | Mozjs::ClassDeclaration
                | Mozjs::ArrowFunction
        )
    }

    is_js_func_and_closure_checker!(MozjsParser, Mozjs);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Mozjs::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozjs::LPAREN | Mozjs::COMMA | Mozjs::RPAREN
        )
    }

    impl_js_family_is_string!(Mozjs);

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Mozjs::IfStatement {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Mozjs::ElseClause;
        }
        false
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for JavascriptCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Javascript::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Javascript::Program
                | Javascript::FunctionExpression
                | Javascript::Class
                | Javascript::GeneratorFunction
                | Javascript::FunctionDeclaration
                | Javascript::MethodDefinition
                | Javascript::GeneratorFunctionDeclaration
                | Javascript::ClassDeclaration
                | Javascript::ArrowFunction
        )
    }

    is_js_func_and_closure_checker!(JavascriptParser, Javascript);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Javascript::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Javascript::LPAREN | Javascript::COMMA | Javascript::RPAREN
        )
    }

    impl_js_family_is_string!(Javascript);

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Javascript::IfStatement
            && node
                .parent()
                .is_some_and(|p| p.kind_id() == Javascript::ElseClause)
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for TypescriptCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Typescript::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Typescript::Program
                | Typescript::FunctionExpression
                | Typescript::Class
                | Typescript::GeneratorFunction
                | Typescript::FunctionDeclaration
                | Typescript::MethodDefinition
                | Typescript::GeneratorFunctionDeclaration
                | Typescript::ClassDeclaration
                | Typescript::AbstractClassDeclaration
                | Typescript::InterfaceDeclaration
                | Typescript::ArrowFunction
        )
    }

    is_js_func_and_closure_checker!(TypescriptParser, Typescript);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Typescript::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Typescript::LPAREN | Typescript::COMMA | Typescript::RPAREN
        )
    }

    impl_js_family_is_string!(Typescript);

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Typescript::IfStatement {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Typescript::ElseClause;
        }
        false
    }

    #[inline]
    fn is_primitive(id: u16) -> bool {
        id == Typescript::PredefinedType
    }
}

impl Checker for TsxCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Tsx::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Tsx::Program
                | Tsx::FunctionExpression
                | Tsx::Class
                | Tsx::GeneratorFunction
                | Tsx::FunctionDeclaration
                | Tsx::MethodDefinition
                | Tsx::GeneratorFunctionDeclaration
                | Tsx::ClassDeclaration
                | Tsx::AbstractClassDeclaration
                | Tsx::InterfaceDeclaration
                | Tsx::ArrowFunction
        )
    }

    is_js_func_and_closure_checker!(TsxParser, Tsx);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Tsx::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Tsx::LPAREN | Tsx::COMMA | Tsx::RPAREN
        )
    }

    impl_js_family_is_string!(Tsx, String3);

    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Tsx::IfStatement
            && node
                .parent()
                .is_some_and(|p| p.kind_id() == Tsx::ElseClause)
    }

    #[inline]
    fn is_primitive(id: u16) -> bool {
        id == Tsx::PredefinedType
    }
}

/// Strip the `#` / `#!` marker plus the `[...]` brackets from a
/// Rust `AttributeItem` / `InnerAttributeItem` token's raw text,
/// returning the inner body. Returns `None` if the input shape is
/// unexpected — callers skip silently rather than feed the matcher
/// the literal `#[...]` form.
fn rust_attribute_body<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.trim()
        .strip_prefix(marker)
        .and_then(|t| t.trim_start().strip_prefix('['))
        .and_then(|t| t.trim().strip_suffix(']'))
}

fn rust_item_is_test_only(node: &Node, code: &[u8]) -> bool {
    rust_outer_attr_marks_test(node, code) || rust_inner_attr_marks_test(node, code)
}

// The tree-sitter Rust grammar exposes outer attributes (`#[...]`) as
// `AttributeItem` siblings *before* the decorated item. Walk backward
// across consecutive attribute siblings; any match short-circuits. This
// scan runs for every item kind, including `mod_item`, so
// `#[cfg(test)] mod tests` (an outer attribute on the module) is caught
// here while `mod tests { #![cfg(test)] }` is caught by the inner scan.
fn rust_outer_attr_marks_test(node: &Node, code: &[u8]) -> bool {
    let mut sibling = node.previous_sibling();
    while let Some(s) = sibling {
        if s.kind_id() != Rust::AttributeItem {
            break;
        }
        if let Some(text) = s.utf8_text(code)
            && let Some(inner) = rust_attribute_body(text, "#")
            && rust_attribute_marks_test(inner)
        {
            return true;
        }
        sibling = s.previous_sibling();
    }
    false
}

// `mod_item` additionally accepts inner attributes (`#![cfg(test)]`).
// The grammar nests these inside the module's `declaration_list` body,
// not as direct `mod_item` children, so descend one level via the
// `body` field before scanning. Non-module items have no inner-attribute
// test form, so this returns `false` for them immediately.
fn rust_inner_attr_marks_test(node: &Node, code: &[u8]) -> bool {
    if node.kind_id() == Rust::ModItem
        && let Some(body) = node.child_by_field_name("body")
    {
        for child in body.children() {
            if child.kind_id() != Rust::InnerAttributeItem {
                continue;
            }
            if let Some(text) = child.utf8_text(code)
                && let Some(inner) = rust_attribute_body(text, "#!")
                && rust_attribute_marks_test(inner)
            {
                return true;
            }
        }
    }
    false
}

impl Checker for RustCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Rust::LineComment || node.kind_id() == Rust::BlockComment
    }

    fn is_useful_comment(node: &Node, code: &[u8]) -> bool {
        if let Some(parent) = node.parent()
            && parent.kind_id() == Rust::TokenTree
        {
            // A comment could be a macro token
            return true;
        }
        let code = &code[node.start_byte()..node.end_byte()];
        code.starts_with(b"/// cbindgen:")
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Rust::SourceFile
                | Rust::FunctionItem
                | Rust::ImplItem
                | Rust::TraitItem
                | Rust::ClosureExpression
        )
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Rust::FunctionItem
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Rust::ClosureExpression
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Rust::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        // A *typed* receiver (`self: Box<Self>`, `self: Rc<Self>`,
        // `self: Pin<&mut Self>` — arbitrary self types) does not parse as
        // `SelfParameter`; the grammar models it as an ordinary `parameter`
        // node whose binding is the `self` keyword (`Rust::Zelf`). It is
        // still a receiver, so it is excluded too, for parity with the
        // bare-receiver case and with Go/C++ (#457). A normal `parameter`
        // such as `x: i32` binds an `identifier`, never `self`, so this
        // child check is unambiguous.
        let is_typed_self_receiver = node.kind_id() == Rust::Parameter
            && node.children().any(|child| child.kind_id() == Rust::Zelf);

        // `SelfParameter` is Rust's bare method receiver (`self`, `&self`,
        // `&mut self`). Like Go's `receiver` field and C++'s implicit
        // `this`, it is not a formal parameter and must not be counted
        // (see #457).
        matches!(
            node.kind_id().into(),
            Rust::LPAREN
                | Rust::COMMA
                | Rust::RPAREN
                | Rust::PIPE
                | Rust::AttributeItem
                | Rust::SelfParameter
        ) || is_typed_self_receiver
    }

    impl_simple_is_string!(Rust, StringLiteral, RawStringLiteral);

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Rust::IfExpression {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Rust::ElseClause;
        }
        false
    }

    #[inline]
    fn is_primitive(id: u16) -> bool {
        matches!(
            id.into(),
            Rust::PrimitiveType
                | Rust::PrimitiveType2
                | Rust::PrimitiveType3
                | Rust::PrimitiveType4
                | Rust::PrimitiveType5
                | Rust::PrimitiveType6
                | Rust::PrimitiveType7
                | Rust::PrimitiveType8
                | Rust::PrimitiveType9
                | Rust::PrimitiveType10
                | Rust::PrimitiveType11
                | Rust::PrimitiveType12
                | Rust::PrimitiveType13
                | Rust::PrimitiveType14
                | Rust::PrimitiveType15
                | Rust::PrimitiveType16
                | Rust::PrimitiveType17
        )
    }

    /// Skip the subtree when `node` is a `mod`, `fn`, `impl`,
    /// `trait`, `const`, or `static` item marked test-only by an
    /// outer or inner attribute (`#[test]`, `#[cfg(test)]`,
    /// `#[tokio::test]`, `#![cfg(test)]`, …). The runtime guard
    /// in `spaces::metrics_with_options` only consults this hook
    /// when the caller opts in via `MetricsOptions::exclude_tests`,
    /// so the default `metrics()` entry point is unaffected.
    fn should_skip_subtree(node: &Node, code: &[u8]) -> bool {
        if !matches!(
            node.kind_id().into(),
            Rust::ModItem
                | Rust::FunctionItem
                | Rust::ImplItem
                | Rust::TraitItem
                | Rust::ConstItem
                | Rust::StaticItem
        ) {
            return false;
        }
        rust_item_is_test_only(node, code)
    }
}

impl Checker for GoCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Go::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Go::SourceFile | Go::FunctionDeclaration | Go::MethodDeclaration | Go::FuncLiteral
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Go::FunctionDeclaration | Go::MethodDeclaration
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Go::FuncLiteral
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Go::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(node.kind_id().into(), Go::LPAREN | Go::COMMA | Go::RPAREN)
    }

    impl_simple_is_string!(Go, InterpretedStringLiteral, RawStringLiteral);

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Go::IfStatement
            && node
                .parent()
                .is_some_and(|p| p.kind_id() == Go::IfStatement)
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for KotlinCode {
    fn is_comment(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LineComment | Kotlin::BlockComment
        )
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::SourceFile
                | Kotlin::ClassDeclaration
                | Kotlin::ObjectDeclaration
                | Kotlin::CompanionObject
                | Kotlin::ObjectLiteral
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::FunctionDeclaration | Kotlin::SecondaryConstructor
        )
    }

    fn is_closure(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LambdaLiteral | Kotlin::AnonymousFunction
        )
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Kotlin::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LPAREN | Kotlin::COMMA | Kotlin::RPAREN
        )
    }

    impl_simple_is_string!(Kotlin, StringLiteral, MultilineStringLiteral);

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // tree-sitter-kotlin models `else if` as an `else` keyword sibling
        // followed by an `if_expression`, not a wrapping clause node.
        node.kind_id() == Kotlin::IfExpression
            && node
                .previous_sibling()
                .is_some_and(|prev| prev.kind_id() == Kotlin::Else)
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for PerlCode {
    fn is_comment(node: &Node) -> bool {
        matches!(node.kind_id().into(), Perl::Comments | Perl::PodStatement)
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::SourceFile
                | Perl::FunctionDefinition
                | Perl::FunctionDefinitionWithoutSub
                | Perl::AnonymousFunction
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::FunctionDefinition | Perl::FunctionDefinitionWithoutSub
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Perl::AnonymousFunction
    }

    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::CallExpressionWithSpacedArgs
                | Perl::CallExpressionWithSub
                | Perl::CallExpressionWithArgsWithBrackets
                | Perl::CallExpressionWithVariable
                | Perl::CallExpressionWithBareword
                | Perl::CallExpressionRecursive
                | Perl::MethodInvocation
        )
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::LPAREN | Perl::COMMA | Perl::RPAREN | Perl::FatComma
        )
    }

    // `HeredocBodyStatement` wraps the heredoc body text (and any
    // `Interpolation` children) that appears as a top-level
    // statement after the heredoc-introducing `<<TAG`; it is the
    // visible literal node and is treated as a string here, the
    // same way Bash's `heredoc_body` is treated as a string.
    impl_simple_is_string!(
        Perl,
        StringSingleQuoted,
        StringDoubleQuoted,
        StringQQuoted,
        StringQqQuoted,
        BacktickQuoted,
        CommandQxQuoted,
        HeredocBodyStatement,
    );

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // tree-sitter-perl emits `elsif_clause` as a direct child of the
        // surrounding `if_statement` (not as a wrapper around a nested
        // `if`), so the clause node itself is the else-if.
        node.kind_id() == Perl::ElsifClause
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for LuaCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Lua::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Lua::Chunk
                | Lua::FunctionDeclaration
                | Lua::FunctionDeclaration2
                | Lua::FunctionDeclaration3
                | Lua::FunctionDefinition
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Lua::FunctionDeclaration | Lua::FunctionDeclaration2 | Lua::FunctionDeclaration3
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Lua::FunctionDefinition
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Lua::FunctionCall
    }

    fn is_non_arg(node: &Node) -> bool {
        // NOTE: `impl NArgs for LuaCode` overrides `compute` with a positive
        // filter on `Identifier | VarargExpression` and never calls `is_non_arg`.
        // This implementation satisfies the trait contract but is unused for NArgs.
        matches!(
            node.kind_id().into(),
            Lua::LPAREN | Lua::COMMA | Lua::RPAREN
        )
    }

    impl_simple_is_string!(Lua, String);

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // Lua uses a dedicated elseif_statement node rather than nesting a
        // second if_statement inside the outer one (as Go does).
        node.kind_id() == Lua::ElseifStatement
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for BashCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Bash::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Bash::Program | Bash::FunctionDefinition
        )
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Bash::FunctionDefinition
    }

    fn is_closure(_node: &Node) -> bool {
        false
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Bash::Command
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Bash::LPAREN | Bash::RPAREN | Bash::COMMA | Bash::SEMI
        )
    }

    // tree-sitter-bash 0.25.1 only emits the `heredoc_body`
    // parser-node symbol (`HeredocBody2`) in observed parse trees;
    // the duplicate `HeredocBody` entry plus the hidden
    // `_heredoc_body` (`HeredocBody3`) and `_simple_heredoc_body`
    // (`SimpleHeredocBody`) rules do not surface, so they are
    // intentionally omitted here.
    impl_simple_is_string!(
        Bash,
        String,
        RawString,
        AnsiCString,
        TranslatedString,
        HeredocBody2,
    );

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Bash::ElifClause
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for TclCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Tcl::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(node.kind_id().into(), Tcl::SourceFile | Tcl::Procedure)
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Tcl::Procedure
    }

    // Tcl closures (`apply`) are ordinary commands; the grammar has no distinct closure node.
    fn is_closure(_: &Node) -> bool {
        false
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Tcl::Command
    }

    // Tcl arguments are whitespace-separated; no punctuation to exclude.
    fn is_non_arg(_: &Node) -> bool {
        false
    }

    impl_simple_is_string!(Tcl, QuotedWord, BracedWord, BracedWordSimple);

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // Tcl grammar has a dedicated `elseif` named node, not a nested `if`.
        node.kind_id() == Tcl::Elseif
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

impl Checker for PhpCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Php::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::Program
                | Php::FunctionDefinition
                | Php::MethodDeclaration
                | Php::AnonymousFunction
                | Php::ArrowFunction
                | Php::ClassDeclaration
                | Php::InterfaceDeclaration
                | Php::TraitDeclaration
                | Php::EnumDeclaration
                | Php::AnonymousClass
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::FunctionDefinition | Php::MethodDeclaration
        )
    }

    fn is_closure(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::AnonymousFunction | Php::ArrowFunction
        )
    }

    // Intentionally narrower than ABC's `branches` set: ABC additionally
    // counts `ObjectCreationExpression` (`new Foo()`) as a branch, but
    // `is_call` drives the `--ops` CLI feature and should match the
    // user's mental model of "function/method call sites" (mirrors
    // Java's `is_call` = `MethodInvocation` while ABC counts
    // `MethodInvocation | New`).
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::FunctionCallExpression
                | Php::MemberCallExpression
                | Php::ScopedCallExpression
                | Php::NullsafeMemberCallExpression
        )
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::LPAREN | Php::LPAREN2 | Php::COMMA | Php::RPAREN | Php::RPAREN2 | Php::DOTDOTDOT
        )
    }

    // `String` is the named single-quoted literal; `String2` and
    // `String3` are aliased kind_ids that the language enum also
    // maps to `"string"` (`String2` is the `string` type keyword
    // and `String3` is the hidden `_string` supertype that covers
    // any string literal). Include all three so generic
    // string-filtering stays consistent with `get_op_type` and the
    // `Alterator` text-preservation arm (issue #288).
    impl_simple_is_string!(
        Php,
        String,
        String2,
        String3,
        EncapsedString,
        Heredoc,
        Nowdoc,
        ShellCommandExpression,
    );

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::ElseIfClause | Php::ElseIfClause2
        )
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

impl Checker for ElixirCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Elixir::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    // Elixir has no syntactic function-definition node: `def`/`defp` /
    // `defmacro`/`defmacrop` / `defmodule` are ordinary `Call` nodes
    // with the macro identifier in the `target` field. The byte-less
    // [`is_func_space`] and [`is_func`] cannot distinguish them from
    // any other `Call`, so they conservatively return zero (only the
    // file root and explicit anonymous functions surface as func
    // spaces). The text-aware [`is_func_space_with_code`] /
    // [`is_func_with_code`] overrides below promote the macro-shaped
    // declarations to first-class function / class spaces (#275). The
    // walker passes the source bytes through, so the metrics
    // attributed to a `def`'s body now correctly nest under a Function
    // space and `Wmc` / `Npm` / `Npa` see a `defmodule` Class.
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::Source | Elixir::AnonymousFunction
        )
    }

    fn is_func(_: &Node) -> bool {
        false
    }

    fn is_func_space_with_code(node: &Node, code: &[u8]) -> bool {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        if Self::is_func_space(node) {
            return true;
        }
        let Some(kw) = elixir_call_keyword(node, code) else {
            return false;
        };
        if elixir_is_class_macro(kw) {
            return true;
        }
        // A `def` / `defp` / `defmacro` / `defmacrop` nested inside a
        // `quote do … end` template does NOT declare a method of any
        // enclosing module — the syntax tree there is a code template
        // emitted later, on macro expansion (#310).
        elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)
    }

    fn is_func_with_code(node: &Node, code: &[u8]) -> bool {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_inside_quote_block, elixir_is_method_macro,
        };
        let Some(kw) = elixir_call_keyword(node, code) else {
            return false;
        };
        elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)
    }

    fn promotes_to_func_space_with_code(node: &Node, code: &[u8]) -> bool {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        // Cheap path: byte-less `is_func_space` matches `Source` and
        // `AnonymousFunction` without needing any text inspection.
        if Self::is_func_space(node) {
            return true;
        }
        // Otherwise one `elixir_call_keyword` lookup answers the
        // combined question instead of the two the default impl would
        // have made via `is_func_with_code || is_func_space_with_code`.
        let Some(kw) = elixir_call_keyword(node, code) else {
            return false;
        };
        if elixir_is_class_macro(kw) {
            return true;
        }
        elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Elixir::AnonymousFunction
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Elixir::Call
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::LPAREN | Elixir::LPAREN2 | Elixir::RPAREN | Elixir::COMMA
        )
    }

    impl_simple_is_string!(Elixir, String, Charlist, Sigil);

    // Elixir lacks an `else if` chain construct. Multi-way branching uses
    // `cond do ... end` (a `Call` whose `do_block` holds many
    // `stab_clause`s, each `+1` nesting in cognitive) or nested
    // `if/else`. No tail-recursive chain to collapse here.
    #[inline]
    fn is_else_if(_: &Node) -> bool {
        false
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

impl Checker for RubyCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Ruby::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Ruby::Program
                | Ruby::Method
                | Ruby::SingletonMethod
                | Ruby::Lambda
                | Ruby::Block
                | Ruby::DoBlock
                | Ruby::Class
                | Ruby::SingletonClass
                | Ruby::Module
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(node.kind_id().into(), Ruby::Method | Ruby::SingletonMethod)
    }

    fn is_closure(node: &Node) -> bool {
        match node.kind_id().into() {
            Ruby::Lambda => true,
            // A stabby lambda `->(z) { … }` parses as a `Lambda` node that
            // CONTAINS the `Block`/`DoBlock` for its body, so the `Lambda`
            // arm above already counts it. Counting the inner block again
            // would double-count one closure as two (#465). The keyword
            // forms `lambda { }` / `proc { }` parse as a `Call` carrying a
            // `Block`/`DoBlock` argument (parent is not a `Lambda`), so they
            // still count exactly once.
            Ruby::Block | Ruby::DoBlock => node
                .parent()
                .is_none_or(|parent| parent.kind_id() != Ruby::Lambda),
            _ => false,
        }
    }

    // tree-sitter-ruby 0.23.1 emits four aliased visible variants of the
    // `call` rule (`Call`, `Call2`, `Call3`, `Call4`); `Call5` ("_call")
    // is the hidden inner production and does not surface.
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Ruby::Call | Ruby::Call2 | Ruby::Call3 | Ruby::Call4
        )
    }

    fn is_non_arg(node: &Node) -> bool {
        // `PIPE` is included because block parameter lists are delimited
        // by `|` rather than parentheses (e.g. `[1,2,3].each { |x| … }`).
        matches!(
            node.kind_id().into(),
            Ruby::LPAREN
                | Ruby::LPAREN2
                | Ruby::RPAREN
                | Ruby::RPAREN2
                | Ruby::COMMA
                | Ruby::SEMI
                | Ruby::PIPE
        )
    }

    impl_simple_is_string!(
        Ruby,
        String,
        ChainedString,
        BareString,
        Subshell,
        Regex,
        HeredocBody,
        DelimitedSymbol,
        SimpleSymbol,
        StringArray,
        SymbolArray,
        Character,
    );

    // tree-sitter-ruby exposes `elsif` as its own named clause node, so the
    // dedicated-clause-node strategy applies here (same as Lua/Bash/PHP).
    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Ruby::Elsif
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

impl Checker for GroovyCode {
    fn is_comment(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::LineComment | Groovy::BlockComment
        )
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    // Mirrors `impl Checker for JavaCode` exactly: `EnumDeclaration`,
    // `RecordDeclaration`, and `AnnotationTypeDeclaration` open class
    // spaces so `Npa`/`Npm`/`Wmc` walk their bodies. Cross-language parity
    // with Java is the point — identical-shape sources must agree on
    // class-shaped metrics (issue #280).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::SourceFile
                | Groovy::ClassDeclaration
                | Groovy::TraitDeclaration
                | Groovy::InterfaceDeclaration
                | Groovy::EnumDeclaration
                | Groovy::RecordDeclaration
                | Groovy::AnnotationTypeDeclaration
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::MethodDeclaration | Groovy::ConstructorDeclaration
        )
    }

    fn is_closure(node: &Node) -> bool {
        matches!(node.kind_id().into(), Groovy::Closure)
    }

    // `command_chain` is the new grammar's distinct node for Groovy's
    // command-style juxtaposed calls (`foo bar baz`) which the prior
    // amaanq grammar mis-modelled as `juxt_function_call`; it is a
    // genuine method-call form and stays in `is_call`.
    //
    // Intentionally excludes `ObjectCreationExpression` (`new Foo()`):
    // `is_call` follows the Java-family convention (Java's `is_call` =
    // `MethodInvocation`, C#'s = `InvocationExpression`) of counting
    // method/function call sites only. Constructor invocations are an
    // ABC concern — Groovy's ABC `branches` counts `New` separately
    // (see `groovy_count_token_branch` in metrics/abc.rs).
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::MethodInvocation | Groovy::CommandChain
        )
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::LPAREN | Groovy::COMMA | Groovy::RPAREN
        )
    }

    impl_simple_is_string!(Groovy, StringLiteral);

    // The dekobon Groovy grammar models `if_statement` with the `else`
    // keyword token emitted inline followed by the inner `if_statement`
    // sibling — same shape as the prior amaanq grammar and Java.
    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Groovy::IfStatement
            && node
                .previous_sibling()
                .is_some_and(|prev| prev.kind_id() == Groovy::Else)
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use super::*;
    use crate::count::count;
    use crate::langs::{
        BashParser, JavascriptParser, MozjsParser, PhpParser, TsxParser, TypescriptParser,
    };
    use std::path::PathBuf;

    fn parse(source: &str) -> BashParser {
        BashParser::new(source.as_bytes().to_vec(), &PathBuf::from("test.sh"), None)
    }

    fn count_strings(source: &str) -> usize {
        count(&parse(source), &["string".to_string()]).0
    }

    // `count`'s filter parser accepts a numeric string as a `kind_id` match
    // (parser.rs `get_filters`), so `has_kind` reuses the same primitive.
    fn has_kind(source: &str, kind_id: u16) -> bool {
        count(&parse(source), &[kind_id.to_string()]).0 > 0
    }

    #[test]
    fn bash_is_string_excludes_word_tokens() {
        // `echo hello world` produces three Word nodes — none of them are
        // string literals. Regression for #44 (Word must not match
        // is_string).
        assert_eq!(count_strings("echo hello world\n"), 0);
        assert_eq!(
            count_strings("if [ -f file.txt ]; then cat file.txt; fi\n"),
            0
        );
    }

    #[test]
    fn bash_is_string_matches_quoted_literals() {
        // Regular double-quoted string -> `string` (Bash::String).
        assert_eq!(count_strings("echo \"double\"\n"), 1);
        // Single-quoted string -> `raw_string` (Bash::RawString).
        assert_eq!(count_strings("echo 'single'\n"), 1);
        // ANSI-C quoting -> `ansi_c_string` (Bash::AnsiCString).
        assert_eq!(count_strings("echo $'ansi-c'\n"), 1);
    }

    #[test]
    fn bash_is_string_matches_translated_string() {
        // tree-sitter-bash only emits a visible `translated_string` node
        // in assignment-style contexts; in command arguments the `$"..."`
        // tokenizes as `$` plus a regular `string`. Use an assignment so
        // the wrapper actually appears in the AST.
        let src = "x=$\"translated\"\n";
        assert!(
            has_kind(src, Bash::TranslatedString as u16),
            "expected a translated_string node in {src:?}"
        );
        // The wrapper plus its inner `string` child both match is_string,
        // so count is 2.
        assert_eq!(count_strings(src), 2);
    }

    #[test]
    fn bash_is_string_matches_heredoc_bodies() {
        // Plain heredoc body.
        assert_eq!(
            count_strings("cat <<EOF\nhello world\nEOF\n"),
            1,
            "heredoc body should be counted as a string literal"
        );
        // Quoted-tag heredoc disables expansions but is still a string.
        assert_eq!(
            count_strings("cat <<'EOF'\nliteral $not_expanded\nEOF\n"),
            1
        );
        // Heredoc with an embedded expansion still yields exactly one
        // body node (parallel to a JS template string with `${x}`).
        assert_eq!(count_strings("cat <<EOF\nhi $name\nEOF\n"), 1);
    }

    // ===== PHP `is_string` regression tests (issue #288) =====

    fn parse_php(source: &str) -> PhpParser {
        PhpParser::new(source.as_bytes().to_vec(), &PathBuf::from("test.php"), None)
    }

    fn count_php_strings(source: &str) -> usize {
        count(&parse_php(source), &["string".to_string()]).0
    }

    #[test]
    fn php_is_string_matches_single_quoted_literal() {
        // `Php::String` is the named single-quoted literal. Inert
        // single-quoted strings have always been matched; this anchors
        // the baseline before exercising the alias kinds.
        assert_eq!(count_php_strings("<?php $x = 'single';"), 1);
    }

    #[test]
    fn php_is_string_matches_encapsed_heredoc_nowdoc_shell() {
        // `EncapsedString` (double-quoted), `Heredoc`, `Nowdoc`, and
        // `ShellCommandExpression` (backticks) must all match
        // `is_string`. Pre-#288 the alterator/checker arms were almost
        // aligned for these named literals — this test locks in the
        // shape.
        assert_eq!(count_php_strings("<?php $x = \"double\";"), 1);
        assert_eq!(
            count_php_strings("<?php $x = <<<EOT\nbody\nEOT;\n"),
            1,
            "heredoc should match is_string"
        );
        assert_eq!(
            count_php_strings("<?php $x = <<<'EOT'\nbody\nEOT;\n"),
            1,
            "nowdoc should match is_string"
        );
        assert_eq!(
            count_php_strings("<?php $x = `ls`;"),
            1,
            "shell command (backtick) should match is_string"
        );
    }

    #[test]
    fn php_is_string_matches_string_alias_kinds() {
        // Regression for #288. Before the fix, only `Php::String`
        // (kind_id 368, the named single-quoted literal) matched
        // `is_string`. The `Php::String2` (anonymous `string` type
        // keyword, kind_id 25) and `Php::String3` (the hidden `_string`
        // supertype, kind_id 378) alias kinds — both of which the
        // language enum maps to `"string"` — were missed. A function
        // with a `: string` return type produces a `Php::String2`
        // anonymous-keyword node, so we exercise it here. The named
        // `Php::String` literal in the body matches too.
        let src = "<?php function f(): string { return 'x'; }";
        // Two string-matching nodes: the `string` return-type keyword
        // (Php::String2) and the `'x'` literal (Php::String). Pre-fix
        // only the literal matched (count would be 1).
        assert_eq!(count_php_strings(src), 2);
    }

    // ===== JS-family `is_string` regression tests (issue #283) =====

    // Walk the AST and return true iff any node has `kind_id == target`.
    // Used to confirm an alias kind actually surfaces in a real parse
    // before asserting it routes through `is_string`.
    fn ast_has_kind_id<P: ParserTrait>(parser: &P, target: u16) -> bool {
        let mut stack = vec![parser.get_root()];
        while let Some(node) = stack.pop() {
            if node.kind_id() == target {
                return true;
            }
            for i in (0..node.child_count()).rev() {
                if let Some(c) = node.child(i) {
                    stack.push(c);
                }
            }
        }
        false
    }

    // For each language, count nodes whose kind_id is exactly `target`
    // *and* simultaneously match `is_string`. A non-zero result proves
    // both that the alias appears in the parse and that the checker
    // accepts it. Pre-fix this would be zero for the alias kinds.
    fn count_string_matches_for_kind<P: ParserTrait, F: Fn(&Node) -> bool>(
        parser: &P,
        target: u16,
        is_string: F,
    ) -> usize {
        let mut stack = vec![parser.get_root()];
        let mut hits = 0;
        while let Some(node) = stack.pop() {
            if node.kind_id() == target && is_string(&node) {
                hits += 1;
            }
            for i in (0..node.child_count()).rev() {
                if let Some(c) = node.child(i) {
                    stack.push(c);
                }
            }
        }
        hits
    }

    #[test]
    fn javascript_is_string_matches_string2_alias() {
        // `Javascript::String2` (kind_id 221) aliases to `"string"`
        // (see `language_javascript.rs`). The alterator already
        // flattens it (#119); the generic `string` filter must agree
        // (#283). Use a source mix that exercises both the primary
        // `String` and the anonymous `String2` productions.
        let src = "const a = 'single';\nconst b = \"double\";\nimport \"m\";\n";
        let parser = JavascriptParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.js"), None);
        // First confirm String2 actually surfaces in this parse —
        // otherwise the assertion below would be vacuously true.
        assert!(
            ast_has_kind_id(&parser, Javascript::String2 as u16),
            "expected Javascript::String2 to appear in the parse",
        );
        // Then assert every String2 node matches is_string.
        assert!(
            count_string_matches_for_kind(
                &parser,
                Javascript::String2 as u16,
                JavascriptCode::is_string,
            ) > 0,
            "Javascript::String2 nodes must match is_string",
        );
    }

    #[test]
    fn mozjs_is_string_matches_string2_alias() {
        // Parallel coverage for the MozJS dialect; same `String2`
        // alias as upstream JavaScript (kind_id 220 here).
        let src = "const a = 'single';\nconst b = \"double\";\nimport \"m\";\n";
        let parser = MozjsParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.js"), None);
        assert!(
            ast_has_kind_id(&parser, Mozjs::String2 as u16),
            "expected Mozjs::String2 to appear in the parse",
        );
        assert!(
            count_string_matches_for_kind(&parser, Mozjs::String2 as u16, MozjsCode::is_string) > 0,
            "Mozjs::String2 nodes must match is_string",
        );
    }

    #[test]
    fn typescript_is_string_matches_string2_alias() {
        // TypeScript exposes both the primary `String` literal and
        // a `String2` alias (kind_id 135). The latter sits among the
        // type-keyword tokens in the enum, so a `: string` annotation
        // is a reliable producer.
        let src = "const a: string = 'x';\nfunction f(): string { return 'y'; }\n";
        let parser = TypescriptParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.ts"), None);
        assert!(
            ast_has_kind_id(&parser, Typescript::String2 as u16),
            "expected Typescript::String2 to appear in the parse",
        );
        assert!(
            count_string_matches_for_kind(
                &parser,
                Typescript::String2 as u16,
                TypescriptCode::is_string,
            ) > 0,
            "Typescript::String2 nodes must match is_string",
        );
    }

    #[test]
    fn tsx_is_string_matches_string2_and_string3_aliases() {
        // TSX uniquely carries two anonymous `"string"` aliases:
        // `String3` (kind_id 141, the type-annotation keyword) and
        // `String2` (kind_id 261). Both must appear in this fixture:
        // the `: string` annotation produces `String3`, and the
        // `'x'` / `"y"` / `"m"` / `"c"` literals produce `String2`.
        // Asserting presence of both *before* checking `is_string`
        // ensures a future grammar bump that stops emitting either
        // alias fails loudly here rather than silently dropping
        // coverage (which would invalidate the regression for #283).
        let src = "const a: string = 'x';\n\
                   const b = \"y\";\n\
                   import \"m\";\n\
                   const el = <div className=\"c\">{\"t\"}</div>;\n";
        let parser = TsxParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.tsx"), None);
        assert!(
            ast_has_kind_id(&parser, Tsx::String3 as u16),
            "expected Tsx::String3 (type-keyword `string`) in the parse",
        );
        assert!(
            ast_has_kind_id(&parser, Tsx::String2 as u16),
            "expected Tsx::String2 (string-literal alias) in the parse",
        );
        assert!(
            count_string_matches_for_kind(&parser, Tsx::String3 as u16, TsxCode::is_string) > 0,
            "Tsx::String3 nodes must match is_string",
        );
        assert!(
            count_string_matches_for_kind(&parser, Tsx::String2 as u16, TsxCode::is_string) > 0,
            "Tsx::String2 nodes must match is_string",
        );
    }

    // Walk the AST and return the first node whose `kind_id` equals
    // `target`. Used by the `is_else_if` tests below to fish a
    // specific node out of the parse tree without depending on the
    // `count` helper above.
    fn find_first_kind<P: ParserTrait>(parser: &P, target: u16) -> Option<Node<'_>> {
        let mut stack = vec![parser.get_root()];
        while let Some(node) = stack.pop() {
            if node.kind_id() == target {
                return Some(node);
            }
            for i in (0..node.child_count()).rev() {
                if let Some(c) = node.child(i) {
                    stack.push(c);
                }
            }
        }
        None
    }

    /// `#[cfg(test)] mod tests { … }` carries the test marker as an
    /// *outer* attribute sibling before the `mod_item`. The outer scan
    /// must run for `mod_item` nodes too, so this case is caught by
    /// `rust_outer_attr_marks_test` — not the inner scan. Pins the key
    /// invariant of the helper split: a `mod_item`'s outer attributes are
    /// never skipped in favour of only its inner attributes.
    #[test]
    fn rust_outer_attr_on_mod_is_test_only() {
        let src = "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        let parser = RustParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.rs"), None);
        let code = parser.get_code();
        let node = find_first_kind(&parser, Rust::ModItem as u16).expect("mod_item");

        assert!(rust_item_is_test_only(&node, code));
        // The marker lives on the outer scan; the inner scan sees no
        // `#![cfg(test)]` and must report false.
        assert!(rust_outer_attr_marks_test(&node, code));
        assert!(!rust_inner_attr_marks_test(&node, code));
    }

    /// `mod tests { #![cfg(test)] … }` carries the marker as an *inner*
    /// attribute nested in the module body. The outer sibling scan finds
    /// nothing; `rust_inner_attr_marks_test` descends via the `body`
    /// field and catches it.
    #[test]
    fn rust_inner_attr_in_mod_is_test_only() {
        let src = "mod tests {\n    #![cfg(test)]\n    fn t() {}\n}\n";
        let parser = RustParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.rs"), None);
        let code = parser.get_code();
        let node = find_first_kind(&parser, Rust::ModItem as u16).expect("mod_item");

        assert!(rust_item_is_test_only(&node, code));
        assert!(!rust_outer_attr_marks_test(&node, code));
        assert!(rust_inner_attr_marks_test(&node, code));
    }

    /// A plain, unattributed item is not test-only — neither scan matches.
    #[test]
    fn rust_plain_item_is_not_test_only() {
        let src = "fn foo() {}\n";
        let parser = RustParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.rs"), None);
        let code = parser.get_code();
        let node = find_first_kind(&parser, Rust::FunctionItem as u16).expect("function_item");

        assert!(!rust_item_is_test_only(&node, code));
        assert!(!rust_outer_attr_marks_test(&node, code));
        assert!(!rust_inner_attr_marks_test(&node, code));
    }

    #[test]
    fn groovy_is_else_if_recognises_else_followed_by_if() {
        // Direct assertion that `GroovyCode::is_else_if` returns true
        // for an `if_statement` whose previous sibling is the `else`
        // token. Defends the sibling-token strategy against accidental
        // regression to a `false` stub (lesson 10, #115 / #239).
        let src = "if (x) { } else if (y) { } else { }";
        let parser =
            GroovyParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.groovy"), None);
        let outer =
            find_first_kind(&parser, Groovy::IfStatement as u16).expect("outer if_statement");
        // Locate the inner if_statement (in the `alternative` slot of
        // the outer if, after the `else` token).
        let mut inner: Option<Node> = None;
        for i in 0..outer.child_count() {
            if let Some(c) = outer.child(i)
                && c.kind_id() == Groovy::IfStatement as u16
            {
                inner = Some(c);
                break;
            }
        }
        let inner = inner.expect("expected an inner if_statement");
        assert!(
            GroovyCode::is_else_if(&inner),
            "inner if_statement after `else` must be recognised as else-if"
        );
        assert!(
            !GroovyCode::is_else_if(&outer),
            "outer if_statement must not be recognised as else-if"
        );
    }

    #[test]
    fn groovy_is_else_if_false_for_standalone_if() {
        // A bare `if` (no `else` preceding it) must NOT register as
        // an else-if.
        let src = "if (x) { println(x) }";
        let parser =
            GroovyParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.groovy"), None);
        let node = find_first_kind(&parser, Groovy::IfStatement as u16).expect("if_statement");
        assert!(!GroovyCode::is_else_if(&node));
    }

    #[test]
    fn groovy_is_call_excludes_constructors() {
        // Regression for #430. `GroovyCode::is_call` previously matched
        // `ObjectCreationExpression`, so `new Foo()` was counted as a
        // call in Groovy but not in Java/C# (whose `is_call` is method
        // invocation only). The Java-family convention is that `is_call`
        // (the `--ops`/`call` filter) counts method/function call sites
        // only; constructors are an ABC concern. This test mixes all
        // three Groovy call-shaped forms and pins the count at 2:
        //   * `new Foo()`        -> object_creation_expression (NOT a call)
        //   * `a.bar()`          -> method_invocation           (a call)
        //   * `println "hi"`     -> command_chain               (a call)
        // Pre-fix this count was 3 (the constructor was miscounted).
        let src = "def m() {\n  def a = new Foo()\n  a.bar()\n  println \"hi\"\n}\n";
        let parser =
            GroovyParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.groovy"), None);
        assert_eq!(
            count(&parser, &["call".to_string()]).0,
            2,
            "is_call must count method_invocation + command_chain only, not the constructor"
        );

        // Direct predicate assertions: the constructor node must be
        // rejected while both genuine call forms are accepted.
        let ctor = find_first_kind(&parser, Groovy::ObjectCreationExpression as u16)
            .expect("object_creation_expression");
        assert!(
            !GroovyCode::is_call(&ctor),
            "object_creation_expression must not be a call"
        );
        let method =
            find_first_kind(&parser, Groovy::MethodInvocation as u16).expect("method_invocation");
        assert!(
            GroovyCode::is_call(&method),
            "method_invocation must be a call"
        );
        let chain = find_first_kind(&parser, Groovy::CommandChain as u16).expect("command_chain");
        assert!(GroovyCode::is_call(&chain), "command_chain must be a call");
    }

    fn parse_python(src: &str) -> PythonParser {
        PythonParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.py"), None)
    }

    // Walk the AST and return every node whose `kind_id` equals `target`,
    // in DFS pre-order. Used by the Python `is_else_if` tests below to
    // distinguish the outer if from the inner one in an `else: if` chain.
    fn find_all_kinds<P: ParserTrait>(parser: &P, target: u16) -> Vec<Node<'_>> {
        let mut out = Vec::new();
        let mut stack = vec![parser.get_root()];
        while let Some(node) = stack.pop() {
            if node.kind_id() == target {
                out.push(node);
            }
            for i in (0..node.child_count()).rev() {
                if let Some(c) = node.child(i) {
                    stack.push(c);
                }
            }
        }
        out
    }

    #[test]
    fn python_is_else_if_recognises_if_inside_else_clause() {
        // `else: if b:` chains parse as `else_clause → block → if_statement`
        // (the `block` wrapper is Python-specific). `is_else_if` must
        // walk through that wrapper. Regression for #276 (the stub
        // returned `false` unconditionally).
        let src = "if a:\n    pass\nelse:\n    if b:\n        pass\n";
        let parser = parse_python(src);
        let outer =
            find_first_kind(&parser, Python::IfStatement as u16).expect("outer if_statement");
        let inner =
            find_python_if_inside_else_block(&parser).expect("inner if_statement under else");
        assert!(
            PythonCode::is_else_if(&inner),
            "if_statement inside else_clause's block must be recognised as else-if"
        );
        assert!(
            !PythonCode::is_else_if(&outer),
            "outer if_statement must not be recognised as else-if"
        );
    }

    #[test]
    fn python_is_else_if_false_for_standalone_if() {
        // A bare `if` whose parent is the module / function body must
        // NOT register as an else-if.
        let src = "if a:\n    pass\n";
        let parser = parse_python(src);
        let node = find_first_kind(&parser, Python::IfStatement as u16).expect("if_statement");
        assert!(!PythonCode::is_else_if(&node));
    }

    #[test]
    fn python_is_else_if_false_for_outer_if_with_elif_alternative() {
        // `elif` parses as an `ElifClause`, not an `IfStatement`, so the
        // only `IfStatement` in `if … elif …` is the outer one. Its
        // parent is the module / function body, not an `else_clause`.
        // Pins that the presence of `elif` in the AST does not trip
        // `is_else_if` for the outer `if`.
        let src = "if a:\n    pass\nelif b:\n    pass\n";
        let parser = parse_python(src);
        let outer = find_first_kind(&parser, Python::IfStatement as u16).expect("if_statement");
        assert!(!PythonCode::is_else_if(&outer));
    }

    // Return the inner `if_statement` that sits directly inside an
    // `else_clause`'s `block` wrapper, or `None` if no such node exists.
    // Used by tests below instead of relying on `find_all_kinds`'s DFS
    // pre-order to land at `ifs[1]`.
    fn find_python_if_inside_else_block(parser: &PythonParser) -> Option<Node<'_>> {
        find_all_kinds(parser, Python::IfStatement as u16)
            .into_iter()
            .find(|n| {
                n.parent().is_some_and(|p| {
                    matches!(p.kind_id().into(), Python::Block | Python::Block2)
                        && p.parent()
                            .is_some_and(|gp| gp.kind_id() == Python::ElseClause)
                })
            })
    }

    #[test]
    fn python_is_else_if_false_when_else_body_has_siblings() {
        // `else: if b:` followed by another statement at the same indent
        // is a real nested-if, not a chain. The block has 2 named
        // children, so `is_else_if` must return false.
        let src = "if a:\n    pass\nelse:\n    if b:\n        pass\n    pass\n";
        let parser = parse_python(src);
        let inner =
            find_python_if_inside_else_block(&parser).expect("inner if_statement under else");
        assert!(
            !PythonCode::is_else_if(&inner),
            "inner if must NOT be recognised as else-if when its block has siblings"
        );
    }

    // Drift guard for #419: tree-sitter-python lists hidden aliases for
    // both `block` and `lambda` that are NOT emitted today —
    // `Block` (135, the hidden `_block` supertype; only `Block2` 160 is
    // emitted) and `Lambda2` (197, an unseen lambda alias; only
    // `Lambda` 196 and the `lambda` keyword token `Lambda3` 73 appear).
    // Several metric sites (`is_closure` — which now routes through
    // `cognitive::python_is_lambda`, the three cognitive lambda-scope
    // sites it feeds (#422), `is_else_if`, `python_is_block` in npa,
    // npm's class-body lookup, loc's no-op arm) already enumerate these
    // aliases defensively so a future grammar bump that promotes either
    // supertype to a concrete node cannot silently undercount.
    // This test pins their current absence across representative Python
    // (function, class, if/for bodies, lambda); if a bump ever emits one,
    // the guard flips red and forces a positive assertion to be added —
    // mirroring the `Php::String3` hidden-supertype guard above.
    #[test]
    fn python_hidden_block_and_lambda_aliases_stay_unseen() {
        let src = "def f(a, b):\n    if a:\n        return b\n    for x in b:\n        print(x)\n\nclass C:\n    def m(self):\n        pass\n\ng = lambda x: x + 1\n";
        let parser = parse_python(src);

        // Confirm the LIVE aliases actually surface, so this fixture is
        // a meaningful witness (a guard that never parses a block or a
        // lambda would pass vacuously).
        assert!(
            ast_has_kind_id(&parser, Python::Block2 as u16),
            "expected Python::Block2 (160, the emitted `block`) in the parse",
        );
        assert!(
            ast_has_kind_id(&parser, Python::Lambda as u16),
            "expected Python::Lambda (196, the emitted `lambda`) in the parse",
        );

        // The hidden supertypes must NOT surface. If either now appears,
        // add a positive assertion routing it through the relevant
        // predicate (see #419).
        assert!(
            !ast_has_kind_id(&parser, Python::Block as u16),
            "Python::Block (135) is the hidden `_block` supertype; if it now appears, route it through python_is_block and assert positively (#419)",
        );
        assert!(
            !ast_has_kind_id(&parser, Python::Lambda2 as u16),
            "Python::Lambda2 (197) is an unseen lambda alias; if it now appears, cognitive::python_is_lambda (reused by is_closure and the three cognitive lambda-scope sites) must detect it and a positive closure assertion is required (#419/#422)",
        );
    }

    // #422: the cognitive lambda-alias chokepoint and `is_closure` must
    // recognise the *same* live `lambda` node. Before #422, cognitive's
    // three lambda sites compared against `Lambda` (196) only while
    // `is_closure` accepted `Lambda | Lambda2`; both now route through
    // `cognitive::python_is_lambda`, so this pins that they agree on the
    // emitted `Lambda` and that the predicate is not vacuously true (it
    // rejects the enclosing `FunctionDefinition`). The unseen `Lambda2`
    // half of the set is covered by the drift guard above.
    #[test]
    fn python_is_lambda_matches_live_lambda_and_agrees_with_is_closure() {
        use crate::metrics::cognitive::python_is_lambda;

        let parser = parse_python("def f():\n    return lambda x: x and x\n");
        let lambda = find_first_kind(&parser, Python::Lambda as u16)
            .expect("the lambda expression must parse as Python::Lambda (196)");

        assert!(
            python_is_lambda(&lambda),
            "python_is_lambda must accept the emitted Lambda node",
        );
        assert!(
            PythonCode::is_closure(&lambda),
            "is_closure must agree with python_is_lambda on the same lambda node",
        );

        // Not vacuously true: a non-lambda node (the enclosing function)
        // must be rejected, so the predicate is discriminating.
        let func = find_first_kind(&parser, Python::FunctionDefinition as u16)
            .expect("the def must parse as Python::FunctionDefinition");
        assert!(
            !python_is_lambda(&func),
            "python_is_lambda must reject a non-lambda node",
        );
    }

    // Regression for #301: every language consolidated under
    // `impl_simple_is_string!` must still recognise its canonical
    // string literal via the `"string"` filter (which routes through
    // `Checker::is_string`). The positive test now drills down to
    // every individual variant of every multi-variant language so a
    // future macro invocation that drops a variant (e.g. forgetting
    // `Cpp::ConcatenatedString` after a grammar bump) fails loudly.
    //
    // JS-family languages (Mozjs/Javascript/Typescript/Tsx) keep
    // their dedicated `impl_js_family_is_string!` macro and have
    // their own alias-aware tests above; they are intentionally
    // not duplicated here.
    fn count_with_parser<P: ParserTrait>(parser: &P) -> usize {
        count(parser, &["string".to_string()]).0
    }

    // Assert that `target` kind_id appears in the parse and every
    // such node matches `is_string`. The two-step check makes test
    // failures unambiguous: a presence failure means the fixture no
    // longer produces the variant (likely grammar drift); a match
    // failure means a macro invocation dropped the variant.
    fn assert_variant_is_string<P: ParserTrait, F: Fn(&Node) -> bool>(
        parser: &P,
        target: u16,
        is_string: F,
        lang: &str,
        variant: &str,
    ) {
        assert!(
            ast_has_kind_id(parser, target),
            "{lang}::{variant} (kind_id {target}) did not appear in the parse — fixture broken",
        );
        assert!(
            count_string_matches_for_kind(parser, target, is_string) > 0,
            "{lang}::{variant} must route through is_string",
        );
    }

    // Collapses the 6-line-per-variant `assert_variant_is_string`
    // call into a single token per variant. `$lang` is the language
    // enum (e.g. `Cpp`); `$code` is the `Checker`-implementing type
    // (e.g. `CppCode`); the trailing list names the enum variants to
    // exercise. The macro feeds `stringify!` for both the language
    // and variant labels so test failures keep the same "Lang::Variant"
    // wording the helper already emits.
    macro_rules! assert_variants_is_string {
        ($parser:expr, $lang:ident, $code:ident, [$($variant:ident),+ $(,)?]) => {
            $(
                assert_variant_is_string(
                    $parser,
                    $lang::$variant as u16,
                    $code::is_string,
                    stringify!($lang),
                    stringify!($variant),
                );
            )+
        };
    }

    // Parse `$src` with `$parser_ty` and assert the generic `"string"`
    // filter yields zero matches. Used by the negative test, which
    // walks 17 languages with identical per-language shape (parse →
    // count → assert_eq! 0).
    macro_rules! assert_no_string_matches {
        ($parser_ty:ident, $path:expr, $src:expr, $lang:literal $(,)?) => {{
            let parser = $parser_ty::new($src.to_vec(), $path, None);
            assert_eq!(count_with_parser(&parser), 0, $lang);
        }};
    }

    #[test]
    fn simple_is_string_macro_recognises_each_language() {
        use crate::langs::{
            CcommentParser, CppParser, CsharpParser, ElixirParser, GoParser, GroovyParser,
            JavaParser, KotlinParser, LuaParser, PerlParser, PreprocParser, PythonParser,
            RubyParser, RustParser, TclParser,
        };

        let path = PathBuf::from("test");

        // ---- Preproc (2 variants): StringLiteral, RawStringLiteral ----
        let src = b"#include \"foo.h\"\nR\"(raw)\"\n".to_vec();
        let parser = PreprocParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Preproc,
            PreprocCode,
            [StringLiteral, RawStringLiteral]
        );

        // ---- Ccomment (2 variants): StringLiteral, RawStringLiteral ----
        // The Ccomment grammar is a stub that only emits Comment /
        // StringLiteral / RawStringLiteral; feed it both forms.
        let src = b"\"hello\"\nR\"(raw)\"\n".to_vec();
        let parser = CcommentParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Ccomment,
            CcommentCode,
            [StringLiteral, RawStringLiteral]
        );

        // ---- Cpp (3 variants): StringLiteral, ConcatenatedString, RawStringLiteral ----
        // C++ string concatenation (`"a" "b"`) produces a
        // `concatenated_string` node wrapping the literals.
        let src =
            b"const char* a = \"hi\";\nconst char* b = \"a\" \"b\";\nconst char* c = R\"(raw)\";\n"
                .to_vec();
        let parser = CppParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Cpp,
            CppCode,
            [StringLiteral, ConcatenatedString, RawStringLiteral]
        );

        // ---- Python (2 variants): String, ConcatenatedString ----
        // Python concatenates adjacent string literals into a
        // `concatenated_string` node.
        let src = b"a = \"hi\"\nb = \"a\" \"b\"\n".to_vec();
        let parser = PythonParser::new(src, &path, None);
        assert_variants_is_string!(&parser, Python, PythonCode, [String, ConcatenatedString]);

        // ---- Java (2 variants): StringLiteral, MultilineStringLiteral ----
        // `Java::MultilineStringLiteral` maps to `_multiline_string_literal`
        // (leading-underscore hidden supertype) and does NOT surface as a
        // concrete kind_id in observed parses. Triple-quoted text blocks
        // instead produce regular `StringLiteral` nodes. The variant is
        // intentionally listed in the macro so a future grammar revision
        // that promotes it can't bypass `is_string`; presence is asserted
        // below to flag drift.
        let src = b"class C { String a = \"hi\"; String b = \"\"\"\nmulti\n\"\"\"; }\n".to_vec();
        let parser = JavaParser::new(src, &path, None);
        assert_variants_is_string!(&parser, Java, JavaCode, [StringLiteral]);
        assert!(
            !ast_has_kind_id(&parser, Java::MultilineStringLiteral as u16),
            "Java::MultilineStringLiteral is documented as the hidden _multiline_string_literal supertype; if it now appears in parses, replace this with a positive variant assertion",
        );

        // ---- Csharp (4 variants): StringLiteral, VerbatimStringLiteral,
        // RawStringLiteral, InterpolatedStringExpression ----
        // Verbatim: `@"..."`; raw: triple-double-quote; interpolated: `$"..."`.
        let src = b"class C { string a = \"hi\"; string b = @\"verb\"; string c = \"\"\"raw\"\"\"; string d = $\"int{1}\"; }\n".to_vec();
        let parser = CsharpParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Csharp,
            CsharpCode,
            [
                StringLiteral,
                VerbatimStringLiteral,
                RawStringLiteral,
                InterpolatedStringExpression,
            ]
        );

        // ---- Rust (2 variants): StringLiteral, RawStringLiteral ----
        let src = b"fn main() { let a = \"hi\"; let b = r\"raw\"; }\n".to_vec();
        let parser = RustParser::new(src, &path, None);
        assert_variants_is_string!(&parser, Rust, RustCode, [StringLiteral, RawStringLiteral]);

        // ---- Go (2 variants): InterpretedStringLiteral, RawStringLiteral ----
        // Backtick-delimited string is the raw form.
        let src = b"package main\nfunc main() { _ = \"hi\"; _ = `raw` }\n".to_vec();
        let parser = GoParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Go,
            GoCode,
            [InterpretedStringLiteral, RawStringLiteral]
        );

        // ---- Kotlin (2 variants): StringLiteral, MultilineStringLiteral ----
        let src = b"fun main() { val a = \"hi\"; val b = \"\"\"multi\"\"\" }\n".to_vec();
        let parser = KotlinParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Kotlin,
            KotlinCode,
            [StringLiteral, MultilineStringLiteral]
        );

        // ---- Lua (1 variant): String ----
        let src = b"local a = \"hi\"\nlocal b = [[long]]\n".to_vec();
        let parser = LuaParser::new(src, &path, None);
        assert_variants_is_string!(&parser, Lua, LuaCode, [String]);

        // ---- Perl (7 variants): StringSingleQuoted, StringDoubleQuoted,
        // StringQQuoted, StringQqQuoted, BacktickQuoted, CommandQxQuoted,
        // HeredocBodyStatement ----
        let src = b"my $a = 'single';\nmy $b = \"double\";\nmy $c = q(qquoted);\nmy $d = qq(qqquoted);\nmy $e = `cmd`;\nmy $f = qx(qxcmd);\nmy $g = <<EOT;\nbody\nEOT\n".to_vec();
        let parser = PerlParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Perl,
            PerlCode,
            [
                StringSingleQuoted,
                StringDoubleQuoted,
                StringQQuoted,
                StringQqQuoted,
                BacktickQuoted,
                CommandQxQuoted,
                HeredocBodyStatement,
            ]
        );

        // ---- Bash (5 variants): String, RawString, AnsiCString,
        // TranslatedString, HeredocBody2 ----
        // TranslatedString surfaces as a wrapper node only in
        // assignment-style contexts (see `bash_is_string_matches_translated_string`).
        let src = b"a=\"d\"\nb='r'\nc=$'ansi'\nd=$\"t\"\ncat <<EOF\nbody\nEOF\n".to_vec();
        let parser = crate::langs::BashParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Bash,
            BashCode,
            [
                String,
                RawString,
                AnsiCString,
                TranslatedString,
                HeredocBody2
            ]
        );

        // ---- Tcl (3 variants): QuotedWord, BracedWord, BracedWordSimple ----
        // Tcl uses two `braced_word` rules: the named rule `braced_word`
        // (an argument-position braced expression that admits commands
        // and substitutions, e.g. `proc ... { body }`'s `body` arg) and
        // `braced_word_simple` (a plain inert braced word like
        // `{braced}` in `set v {braced}`). The fixture below mixes a
        // `proc ... {body}` (BracedWord), a simple `set` assignment
        // (BracedWordSimple), and a `set` to a quoted literal
        // (QuotedWord).
        let src = b"set a \"quoted\"\nset b {braced}\nproc p {x y} { return $x }\n".to_vec();
        let parser = TclParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Tcl,
            TclCode,
            [QuotedWord, BracedWordSimple, BracedWord]
        );

        // ---- Php (7 variants): String, String2, String3,
        // EncapsedString, Heredoc, Nowdoc, ShellCommandExpression ----
        // String2 is the `string` type-keyword (`: string` return
        // type, exercised here). String3 is the hidden `_string`
        // supertype (kind_id => "_string" — name starts with `_`),
        // which tree-sitter does NOT emit as a concrete node — see
        // its empirical absence asserted below.
        let src = b"<?php function f(): string { $a = 'single'; $b = \"double\"; $c = <<<EOT\nbody\nEOT;\n$d = <<<'EOT'\nnow\nEOT;\n$e = `ls`; return $a; }\n".to_vec();
        let parser = PhpParser::new(src, &path, None);
        assert_variants_is_string!(&parser, Php, PhpCode, [String, String2]);
        // `Php::String3` is the hidden `_string` supertype — never
        // surfaces as a concrete kind_id in observed parses; the
        // checker still lists it so future grammar revisions that
        // promote it cannot silently bypass `is_string`. Verified
        // unreachable empirically (assertion below proves the
        // fixture does not produce it; the variant is intentionally
        // unverifiable through a positive test until then).
        assert!(
            !ast_has_kind_id(&parser, Php::String3 as u16),
            "Php::String3 is documented as the hidden _string supertype; if it now appears in parses, add a positive variant assertion",
        );
        assert_variants_is_string!(
            &parser,
            Php,
            PhpCode,
            [EncapsedString, Heredoc, Nowdoc, ShellCommandExpression]
        );

        // ---- Elixir (3 variants): String, Charlist, Sigil ----
        // Charlists use single quotes; sigils use `~s(...)` etc.
        let src = b"a = \"hi\"\nb = 'charlist'\nc = ~s(sigil)\n".to_vec();
        let parser = ElixirParser::new(src, &path, None);
        assert_variants_is_string!(&parser, Elixir, ElixirCode, [String, Charlist, Sigil]);

        // ---- Ruby (11 variants): String, ChainedString, BareString,
        // Subshell, Regex, HeredocBody, DelimitedSymbol, SimpleSymbol,
        // StringArray, SymbolArray, Character ----
        // ChainedString: two adjacent string literals (`"a" "b"`).
        // BareString: an unquoted element inside `%w[...]` / inside
        // a string array context. `%w[bare1 bare2]` emits a
        // StringArray whose children are BareString nodes — both
        // surface in the parse. Use a script that produces every
        // form so the per-variant assertion can hit each.
        let src = b"a = \"hi\"\nb = \"x\" \"y\"\nc = `cmd`\nd = /re/\ne = <<EOT\nbody\nEOT\nf = :sym\ng = :\"dsym\"\nh = %w[bare1 bare2]\ni = %i[s1 s2]\nj = ?A\n".to_vec();
        let parser = RubyParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Ruby,
            RubyCode,
            [
                String,
                ChainedString,
                BareString,
                Subshell,
                Regex,
                HeredocBody,
                DelimitedSymbol,
                SimpleSymbol,
                StringArray,
                SymbolArray,
                Character,
            ]
        );

        // ---- Groovy (1 variant): StringLiteral ----
        // The dekobon Groovy grammar consolidates every string shape
        // (single / double / triple-quoted, slashy `/.../`, dollar-
        // slashy `$/.../$`, GString-interpolated) under one
        // `string_literal` rule, so a single variant suffices —
        // unlike Java, character literals are not a separate node.
        let src =
            b"def m() { def a = \"hi\"; def b = \"\"\"multi\"\"\"; def c = /pat/ }\n".to_vec();
        let parser = GroovyParser::new(src, &path, None);
        assert_variants_is_string!(&parser, Groovy, GroovyCode, [StringLiteral]);
    }

    #[test]
    fn simple_is_string_macro_rejects_non_string_nodes() {
        // Pure-identifier source must produce zero string matches.
        // Catches a regression where a macro invocation accidentally
        // included a too-broad variant (e.g. `Identifier`). Covers
        // every language consolidated under `impl_simple_is_string!`.
        use crate::langs::{
            BashParser, CcommentParser, CppParser, CsharpParser, ElixirParser, GoParser,
            GroovyParser, JavaParser, KotlinParser, LuaParser, PerlParser, PreprocParser,
            PythonParser, RubyParser, RustParser, TclParser,
        };

        let path = PathBuf::from("test");

        // Each fixture is the most minimal identifier-only input that
        // still parses for the target language. Per-language comments
        // below flag the few cases where the input choice is load-
        // bearing (Bash `s=$y` to avoid `string`-kind expansion nodes,
        // Tcl `set x $y` to keep the bareword as `Word`, etc.).
        assert_no_string_matches!(PreprocParser, &path, b"#define FOO 1\n", "Preproc");
        // Ccomment: input that lexes as a single line comment only.
        assert_no_string_matches!(CcommentParser, &path, b"// just a comment\n", "Ccomment");
        assert_no_string_matches!(CppParser, &path, b"int main() { return x; }\n", "Cpp");
        assert_no_string_matches!(PythonParser, &path, b"x = y\n", "Python");
        assert_no_string_matches!(JavaParser, &path, b"class C { int x = y; }\n", "Java");
        // Csharp: identifier-only field initializer.
        assert_no_string_matches!(CsharpParser, &path, b"class C { int x = y; }\n", "Csharp");
        assert_no_string_matches!(RustParser, &path, b"fn main() { let x = y; }\n", "Rust");
        assert_no_string_matches!(
            GoParser,
            &path,
            b"package main\nfunc main() { _ = x }\n",
            "Go"
        );
        assert_no_string_matches!(KotlinParser, &path, b"fun main() { val x = y }\n", "Kotlin");
        // Perl: identifier-only assignment with no quoted forms.
        assert_no_string_matches!(PerlParser, &path, b"my $x = $y;\n", "Perl");
        assert_no_string_matches!(LuaParser, &path, b"local x = y\n", "Lua");
        // Bash: assignment of one variable to another, no literals.
        // `s=$y` produces only Variable/SimpleExpansion nodes; no
        // string-kind node should appear.
        assert_no_string_matches!(BashParser, &path, b"s=$y\n", "Bash");
        // Tcl: `set` of an unquoted identifier word. The unquoted
        // bareword surfaces as `Word`, not any of the three string
        // kinds.
        assert_no_string_matches!(TclParser, &path, b"set x $y\n", "Tcl");
        // Php: identifier-only assignment.
        assert_no_string_matches!(PhpParser, &path, b"<?php $x = $y;\n", "Php");
        // Elixir: integer assignment, no string/charlist/sigil.
        assert_no_string_matches!(ElixirParser, &path, b"x = 1\n", "Elixir");
        assert_no_string_matches!(RubyParser, &path, b"x = y\n", "Ruby");
        // Groovy: identifier-only method body.
        assert_no_string_matches!(GroovyParser, &path, b"def m() { def x = y }\n", "Groovy");
    }

    #[test]
    fn mozjs_parses_using_declaration() {
        // Drift marker for the JS-base-grammar bump 0.23.1 -> 0.25.0
        // (#407): the `using` / `await using` explicit-resource-
        // management declaration is a 0.25.0 grammar feature. The
        // bundled mozjs parser was previously stale at 0.23.1 while
        // its marker claimed 0.25.0 (the #400 baseline lie), so this
        // node could not appear. Asserting it surfaces pins the regen
        // against reversion — it fails against the pre-#407 parser.
        let src = "function f() {\n  using r = acquire();\n  return r;\n}\n";
        let parser = MozjsParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.js"), None);
        assert!(
            ast_has_kind_id(&parser, Mozjs::UsingDeclaration as u16),
            "expected Mozjs::UsingDeclaration to appear in the parse",
        );
    }
}
