//! Ruby: what YARD reads and where. The call forms YARD's handlers
//! document from the comment run above them, the scope fold that says
//! where a handler runs, and the stabby-lambda body the `Lambda` wrapper
//! already counts — read by the `Checker` impl and the role rows alike.
//!
//! The `role-bypass: allow(…)` markers below moved here with their fns
//! from `checker/ruby.rs`; `utils/check-role-bypass.py` reads
//! `checker/`, `getter/` and the metrics, not `lang_helpers/`, so here
//! they are prose — each still names the role that claims the kind, as
//! a reader of the comparison expects, but nothing checks it (#1701).

use crate::Ruby;
use crate::lang_helpers::doc_comment::node_bytes;
use crate::node::{Ancestors, FoldLane, Node};

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

/// Whether the call `node` is one YARD documents from the comment run
/// above it: an attribute macro, `alias_method`, or a visibility-wrapped
/// `def` (`private def m`). YARD's visibility handlers take only a call
/// with no receiver, so `self.private def m` or `@cfg.private def m`
/// documents nothing; its attribute handler ignores the receiver. A bare
/// `private` on its own row is an identifier, not a call, and ends the
/// run; `private :m` re-scopes a method documented at its `def`, so a
/// comment above it is not documentation either.
pub(crate) fn ruby_documented_call(node: &Node, code: &[u8]) -> bool {
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
                        // role-bypass: allow(FuncOpener, DocTarget) — whether a visibility
                        // call's argument is a `def` (not a class or an alias, which
                        // `private` does not take): the call's shape, read by the
                        // `DocTarget` row's gate, not membership
                        matches!(
                            argument.kind_id().into(),
                            Ruby::Method | Ruby::SingletonMethod
                        )
                    })
        }
        _ => false,
    }
}

/// The scope a documented statement needs to sit in for YARD to read
/// it: a `def` is read in a method body too; everything else only in a
/// namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YardScope {
    /// A namespace or a method body — what a bare `def` (`method`,
    /// `singleton_method`) needs.
    DefBody,
    /// A namespace only — what a class, module, constant, alias,
    /// attribute, visibility call, singleton class or directive block
    /// needs.
    Namespace,
}

impl YardScope {
    /// What the documented `core` statement needs: a `def` is read in a
    /// `def` body where a namespace is not — the scope question, context
    /// over the `DocTarget` row that owns both.
    #[must_use]
    pub(crate) fn of(core: &Node) -> Self {
        // role-bypass: allow(DocTarget, FuncOpener) — which scope the core
        // needs, not whether it is a target or opens a space
        if matches!(core.kind_id().into(), Ruby::Method | Ruby::SingletonMethod) {
            Self::DefBody
        } else {
            Self::Namespace
        }
    }
}

/// Whether YARD reads a documented statement where it sits, given what
/// the statement `needs` of that scope. YARD's processor runs a handler
/// only on a statement some handler claims, and descends only into the
/// namespace bodies it opens — the
/// file, a class, a module, a singleton class — the branches of an
/// `if` / `unless` / `elsif` there, and a method body, where only the
/// handler for `def` runs: YARD documents a method nested in another, and
/// never a constant, alias, attribute, visibility call, singleton class or
/// conditional there. No handler claims a `case`, a `begin` or a loop, so
/// nothing in their bodies is documented. Unlike YARD, a statement in a
/// call's block (`included do`, an RSpec `describe` block) is read.
/// `scope` is the statement itself, or the `then` wrapper standing for
/// it; `ancestors` is `scope`'s chain.
pub(crate) fn ruby_yard_reads<'a>(
    needs: YardScope,
    scope: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
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
            // role-bypass: allow(Container, Branch, ElseIf, DocTarget) — the namespaces
            // and the conditionals, where YARD reads: the scope a target is read in,
            // context over the `DocTarget` row, not membership
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
            // role-bypass: allow(FuncOpener, DocTarget) — a `def` body, where YARD
            // reads a `def` and nothing else: context over the row, not membership
            Ruby::Method | Ruby::SingletonMethod => above & DEF_READS,
            // role-bypass: allow(Switch, Loop) — the bodies YARD never parses;
            // doc attachment, not membership
            Ruby::Case | Ruby::CaseMatch | Ruby::Begin | Ruby::While | Ruby::Until | Ruby::For => 0,
            _ => above,
        },
    );
    let need = match needs {
        YardScope::DefBody => DEF_READS,
        YardScope::Namespace => NAMESPACED_READS,
    };
    reads & need != 0
}
