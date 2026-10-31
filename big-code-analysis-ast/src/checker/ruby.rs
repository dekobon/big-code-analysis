//! `Checker` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{node_bytes, ruby_directive_opener, ruby_magic_comment};

/// Whether `node` — a `Block` / `DoBlock` — is the body of a stabby
/// lambda (`->(z) { … }` / `->(z) do … end`), which parses as a `Lambda`
/// node that CONTAINS the block. The `Lambda` wrapper already counts as
/// the closure (#465) and opens the function space (#1257), so
/// classifying the body again would double-count one lambda as two
/// closures, or open a phantom nested Function space. The keyword forms
/// `lambda { }` / `proc { }` and iterator blocks (`[1].each { |x| x }`)
/// parse as a `Call` carrying the block argument — the parent is not a
/// `Lambda` — so they still classify exactly once.
///
/// One helper the `Closure` and `FuncOpener` rows of `roles/ruby.rs`
/// both gate on, so the closure count, the space tree, and the
/// cognitive nesting surcharge — each a reader of those rows — cannot
/// drift apart (`.claude/rules/grammar-dispatch.md` §6).
///
/// The parent comes off the caller's chain: all consumers run per node
/// from a walk, and `Node::parent` costs `O(depth)` because
/// `tree_sitter` resolves it by descending from the root (#1088).
#[must_use]
pub(crate) fn is_stabby_lambda_body<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    // role-bypass: allow(FuncOpener, Closure) — the gate the rows read:
    // whether the parent is the lambda, not whether a node opens a space
    ancestors
        .parent(node)
        .is_some_and(|parent| parent.kind_id() == Ruby::Lambda)
}

/// The statement a comment run directly above `node` documents: `node`
/// itself, or the statement a wrapper stands for. A statement modifier
/// (`X = 1 unless defined?(X)`, `def m; end if c`) documents its
/// statement, as YARD reads it. The grammar parks a doc comment that
/// precedes the first statement of a `class` / `module` body or of a
/// brace block outside the `body_statement` / `block_body` wrapper, so
/// the wrapper stands in for its first child; `BodyStatement2` is the
/// hidden `_body_statement`, kept as a defensive arm, and `dloc.rs` pins
/// that the grammar never emits it. A loop rather than recursion: a chain
/// of modifiers as long as the file must not exhaust the walker's stack
/// (#1597, as for Lua's dotted targets). `None` for an empty wrapper.
fn ruby_documented_core<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let mut node = *node;
    loop {
        node = match node.kind_id().into() {
            // role-bypass: allow(Branch) — the modifier's body is the
            // documented item; doc attachment, not membership
            Ruby::IfModifier | Ruby::UnlessModifier => node.child_by_field_name("body")?,
            Ruby::BodyStatement | Ruby::BodyStatement2 | Ruby::BlockBody => node.child(0)?,
            _ => return Some(node),
        };
    }
}

/// Whether `node`, unwrapped by [`ruby_documented_core`], is a statement
/// YARD documents from the comment run directly above it: a `def`,
/// `class` or `module`, a `class << Const`, a constant or class-variable
/// assignment, a method alias (`alias` or `alias_method`), an `attr` /
/// `attr_*` call or a visibility-wrapped `def`.
fn ruby_documented_statement(node: &Node, code: &[u8]) -> bool {
    // role-bypass: allow(FuncOpener, Container, Member) — doc attachment, not
    // membership: what a doc comment binds to is `DocTarget`'s own row (R09)
    match node.kind_id().into() {
        Ruby::Method | Ruby::SingletonMethod | Ruby::Class | Ruby::Module => true,
        // `Alias` is the `alias new old` statement (305), which YARD's alias
        // handler documents; `Alias2` (106) is the keyword token inside it,
        // never a statement, and is left out on purpose. A global-variable
        // alias (`alias $new $old`) names no method, and YARD takes none.
        Ruby::Alias => node
            .child_by_field_name("name")
            .is_none_or(|name| name.kind_id() != Ruby::GlobalVariable as u16),
        // YARD registers a class for `class << Const`, but nothing for
        // `class << self` or another object, so a comment above one of
        // those documents nothing.
        Ruby::SingletonClass => node.child_by_field_name("value").is_some_and(|value| {
            matches!(
                value.kind_id().into(),
                Ruby::Constant | Ruby::ScopeResolution | Ruby::ScopeResolution2
            )
        }),
        // YARD's constant handler documents `MAX = 10` and the scoped
        // `Foo::BAR = 1`, and its class-variable handler `@@count = 0`; an
        // assignment to a local or an instance variable documents nothing.
        // `Assignment2` and `ScopeResolution2` are the grammar's aliased
        // productions of the same rules (grammar-dispatch rule 1).
        Ruby::Assignment | Ruby::Assignment2 => {
            node.child_by_field_name("left").is_some_and(|left| {
                // role-bypass: allow(Member) — doc attachment, not membership
                matches!(
                    left.kind_id().into(),
                    Ruby::Constant
                        | Ruby::ScopeResolution
                        | Ruby::ScopeResolution2
                        | Ruby::ClassVariable
                )
            })
        }
        _ if RubyCode::is_call(node) => ruby_documented_call(node, code),
        _ => false,
    }
}

/// Whether the call `node` is one YARD documents from the comment run
/// above it: an attribute macro, `alias_method`, or a visibility-wrapped
/// `def` (`private def m`). YARD's visibility handlers take only a call
/// with no receiver, so `self.private def m` or `@cfg.private def m`
/// documents nothing; its attribute handler ignores the receiver. A bare
/// `private` on its own row is an identifier, not a call, and ends the
/// run; `private :m` re-scopes a method documented at its `def`, so a
/// comment above it is not documentation either.
fn ruby_documented_call(node: &Node, code: &[u8]) -> bool {
    let Some(method) = node.child_by_field_name("method") else {
        return false;
    };
    match node_bytes(&method, code) {
        // `alias_method :new, :old` is the call form of `alias`, which the
        // same YARD handler documents.
        b"attr" | b"attr_reader" | b"attr_writer" | b"attr_accessor" | b"alias_method" => true,
        b"private"
        | b"protected"
        | b"public"
        | b"module_function"
        | b"private_class_method"
        | b"public_class_method" => {
            node.child_by_field_name("receiver").is_none()
                && node
                    .child_by_field_name("arguments")
                    .and_then(|arguments| arguments.children().find(Node::is_named))
                    .is_some_and(|argument| {
                        // role-bypass: allow(FuncOpener, Container, Member) — doc attachment, not
                        // membership: what a doc comment binds to is `DocTarget`'s own row (R09)
                        matches!(
                            argument.kind_id().into(),
                            Ruby::Method | Ruby::SingletonMethod
                        )
                    })
        }
        _ => false,
    }
}

/// The first statement of a `then` wrapper (`Then`, 243 — `Then2`, 55, is
/// the keyword token, never a wrapper), or `None` for any other node.
fn ruby_then_first_statement<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    (node.kind_id() == Ruby::Then)
        .then(|| node.children().find(Node::is_named))
        .flatten()
}

/// Whether YARD reads a documented statement where it sits; `is_def` says
/// the statement is a bare `def`, the one YARD also reads in a method
/// body. YARD's processor runs a handler only on a statement some handler
/// claims, and descends only into the namespace bodies it opens — the
/// file, a class, a module, a singleton class — the branches of an
/// `if` / `unless` / `elsif` there, and a method body, where only the
/// handler for `def` runs: YARD documents a method nested in another, and
/// never a constant, alias, attribute, visibility call, singleton class or
/// conditional there. No handler claims a `case`, a `begin` or a loop, so
/// nothing in their bodies is documented. Unlike YARD, a statement in a
/// call's block (`included do`, an RSpec `describe` block) is read.
/// `scope` is the statement itself, or the `then` wrapper standing for
/// it; `ancestors` is `scope`'s chain.
fn ruby_yard_reads<'a>(is_def: bool, scope: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    // Read upward, the rule is: the nearest namespace admits the
    // statement; a method body met first rejects it if it needs a
    // namespace; a singleton class or conditional on the way makes it
    // need one; and a `case`, `begin` or loop rejects it outright, with
    // any namespace opened inside one. Folded down instead, so it is
    // memoised per ancestor (#1597), the state at each depth carries the
    // upward answer for both starting needs.
    const DEF_READS: u8 = 0b01;
    const NAMESPACED_READS: u8 = 0b10;
    const BOTH: u8 = DEF_READS | NAMESPACED_READS;
    let reads = ancestors.fold_down(
        scope,
        0,
        FoldLane::DocScope,
        BOTH,
        // Where YARD reads a doc comment, not whether a node opens a space:
        // each arm below that names a space opener is exempt for that.
        |above, ancestor, _, _| match ancestor.kind_id().into() {
            // role-bypass: allow(Container) — the file, where YARD reads
            Ruby::Program => BOTH,
            // Each opens a namespace only where one already reads it. For a
            // class or module — Ruby rejects one in a method body — that
            // closes it only under a `case`, `begin` or loop, whose bodies
            // YARD never parses; a singleton class or conditional in a
            // method body closes as before.
            // role-bypass: allow(Container, Branch, ElseIf) — the namespaces and the
            // conditionals, where YARD reads: doc attachment, not membership
            Ruby::Class
            | Ruby::Module
            | Ruby::SingletonClass
            | Ruby::If
            | Ruby::Unless
            | Ruby::Elsif => {
                if above & NAMESPACED_READS == 0 {
                    0
                } else {
                    BOTH
                }
            }
            // role-bypass: allow(FuncOpener) — a `def` body, where YARD reads
            Ruby::Method | Ruby::SingletonMethod => above & DEF_READS,
            // role-bypass: allow(Switch, Loop) — the bodies YARD never parses;
            // doc attachment, not membership
            Ruby::Case | Ruby::CaseMatch | Ruby::Begin | Ruby::While | Ruby::Until | Ruby::For => 0,
            _ => above,
        },
    );
    let need = if is_def { DEF_READS } else { NAMESPACED_READS };
    reads & need != 0
}

impl Checker for RubyCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Ruby::Comment
    }

    fn is_doc_target<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        // The grammar parks a comment that precedes the first statement of
        // an `if`, `elsif` or `unless` body ahead of the `then` wrapper, so
        // the wrapper stands for that statement. `then` decides no scope,
        // so its chain — the comment's — reads the same as the
        // statement's. The statement test goes first, so the scope fold
        // runs only for a candidate.
        let statement = ruby_then_first_statement(node).unwrap_or(*node);
        ruby_documented_core(&statement).is_some_and(|core| {
            ruby_documented_statement(&core, code)
                && ruby_yard_reads(
                    // role-bypass: allow(FuncOpener, Container, Member) — doc attachment, not
                    // membership: what a doc comment binds to is `DocTarget`'s own row (R09)
                    matches!(core.kind_id().into(), Ruby::Method | Ruby::SingletonMethod),
                    node,
                    ancestors,
                )
        })
    }

    fn doc_target_start_row(node: &Node) -> usize {
        ruby_then_first_statement(node).unwrap_or(*node).start_row()
    }

    fn is_doc_run_directive(node: &Node, code: &[u8]) -> bool {
        ruby_magic_comment(node_bytes(node, code))
    }

    // RDoc's `##` and YARD's `@!` directives document a method that has no
    // `def` to sit above (`define_method`, a DSL call), so the block counts
    // whatever follows it — but only where the tools read it: a namespace
    // body, never a method's.
    fn is_doc_run_opener<'a>(
        first: &Node<'a>,
        _next: Option<&Node<'a>>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        first.kind_id() == Ruby::Comment
            && ruby_directive_opener(node_bytes(first, code))
            && ruby_yard_reads(false, first, ancestors)
    }

    // The space tree is declared in `roles/ruby.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The block rows read the chain — a `block` is
    // a space only outside a stabby lambda (#465, #1257) — so this
    // byte-less spelling stays the kind table, which over-approximates
    // on a lambda's own body block exactly as it did before the rows:
    // it forwards with a *known-empty* chain, the node read as if it
    // were the root, so the parent gate sees no lambda. Not
    // `Ancestors::unknown()`, which climbs through `Node::parent` and
    // would answer the gated result here — a change in this spelling's
    // answer no output could see. The walk calls the `_with_code` one.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::ruby::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::ruby::func_opener::is_callable(node, &[], ancestors)
    }

    // Ruby is the one non-JS grammar whose closure test is not
    // answerable from the node's own kind: the `Closure` row gates a
    // `block` / `do_block` on not being a stabby lambda's own body.
    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::ruby::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/ruby.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(ruby, call, is_call, is_call_with_code);
    impl_row_predicates!(ruby, string_lit, is_string, is_string_with_code);

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

    // The `ElseIf` row of `roles/ruby.rs`: the dedicated `elsif` clause
    // itself is the else-if (#1697).
    impl_else_if_from_row!(ruby);
}
