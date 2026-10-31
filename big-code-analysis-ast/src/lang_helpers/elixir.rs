//! Elixir: keyword `Call` identity (`def`, `defmodule`, `quote`, …),
//! and applied versus named operators.

use crate::Elixir;
use crate::node::{Ancestors, FoldLane, Node, WordSlot};

/// Reads the text of the `target` field of an Elixir `Call` node.
///
/// Most of Elixir's control-flow constructs (`if`, `unless`, `for`,
/// `while`, `case`, `cond`, `with`, `try`) and method-defining macros
/// (`def`, `defp`, `defmacro`, …) parse as `Call` nodes whose `target`
/// is an `Identifier` whose source text spells the keyword. The
/// `Cyclomatic` and `Exit` impls already follow this pattern; this
/// helper centralises the byte-text lookup so `Cognitive` and `Abc`
/// can share it.
///
/// Returns `None` for Calls whose target is not a simple identifier
/// (e.g. `Module.func(…)` parses as `RemoteCallWithParentheses` with
/// the dotted name as target) or when the bytes are not valid UTF-8.
///
/// This is the uncached read. A consumer asking about the node the walk
/// is visiting reads [`elixir_call_target`], which answers from the
/// walk's word slot after the first metric asks. An ask about an
/// *enclosing* call from a descendant evicts the visited node's entry,
/// so it is a judgement per site: a scan that asks about many other
/// nodes while the visited one has metrics still to run stays here
/// (`npm/elixir.rs`'s statement loop); a single ask from a node no
/// later metric reads the slot for (cognitive's `else` token and
/// section pair) may use the cached read, and hits when the enclosing
/// call was the last node visited (#1697).
#[inline]
#[must_use]
pub fn elixir_call_keyword<'a>(node: &'a Node<'a>, code: &'a [u8]) -> Option<&'a str> {
    if node.kind_id() != Elixir::Call as u16 {
        return None;
    }
    call_keyword_reads::record();
    let target = node.child_by_field_name("target")?;
    if target.kind_id() != Elixir::Identifier as u16 {
        return None;
    }
    target.utf8_text(code)
}

// How many `call` targets were read for their keyword: the walk's
// promotion must read one per `call`, not one per row that gates on it
// (#310, #1696), and the metric walk one per `call` across every
// consumer (#1697). Only the count tells a second read apart. Readable
// from the root crate's tests through `test-support`, since the walk
// that lends the word slot lives there.
crate::observation::counter!(call_keyword_reads, cfg(any(test, feature = "test-support")));

/// What a `call`'s `target` identifier spells, for every word some
/// consumer tells apart — the raw identity, not a policy reading (#1697).
/// The control-flow rows (`Branch`, `Switch`, `Loop`, `Exit`) and
/// `Member` gate on it directly, `FuncOpener` / `Container` through the
/// crate's `elixir_declaration` projection, and cyclomatic, cognitive,
/// ABC, NPM and NPA branch on it; one `call` is asked by all of them in sequence, and
/// [`elixir_call_target`] answers from the walk's word slot after the
/// first. `Other` is every other target — a user function, a remote
/// call, a computed callee, invalid UTF-8 — cached like any answer.
///
/// `#[repr(u8)]` because the slot stores the discriminant; the
/// conversion back is total, reading anything past the table as
/// `Other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[non_exhaustive]
pub enum ElixirCallTarget {
    /// Any target that is none of the words below.
    Other = 0,
    /// `if`.
    If,
    /// `unless`.
    Unless,
    /// `for`.
    For,
    /// `while`.
    While,
    /// `case`.
    Case,
    /// `cond`.
    Cond,
    /// `with`.
    With,
    /// `try`.
    Try,
    /// `receive`.
    Receive,
    /// `throw`.
    Throw,
    /// `raise`.
    Raise,
    /// `reraise`.
    Reraise,
    /// `exit`.
    Exit,
    /// `def`.
    Def,
    /// `defp`.
    Defp,
    /// `defmacro`.
    Defmacro,
    /// `defmacrop`.
    Defmacrop,
    /// `defmodule`.
    Defmodule,
    /// `defprotocol`.
    Defprotocol,
    /// `defimpl`.
    Defimpl,
    /// `defstruct`.
    Defstruct,
    /// `defguard`.
    Defguard,
    /// `defguardp`.
    Defguardp,
    /// `quote`.
    Quote,
    /// `alias`.
    Alias,
    /// `import`.
    Import,
    /// `require`.
    Require,
    /// `use`.
    Use,
}

impl ElixirCallTarget {
    /// Every variant, in discriminant order; what a consumer that keys
    /// its policy on the word iterates to prove the policy agrees with
    /// the rows for all of them.
    pub const ALL: [Self; 29] = [
        Self::Other,
        Self::If,
        Self::Unless,
        Self::For,
        Self::While,
        Self::Case,
        Self::Cond,
        Self::With,
        Self::Try,
        Self::Receive,
        Self::Throw,
        Self::Raise,
        Self::Reraise,
        Self::Exit,
        Self::Def,
        Self::Defp,
        Self::Defmacro,
        Self::Defmacrop,
        Self::Defmodule,
        Self::Defprotocol,
        Self::Defimpl,
        Self::Defstruct,
        Self::Defguard,
        Self::Defguardp,
        Self::Quote,
        Self::Alias,
        Self::Import,
        Self::Require,
        Self::Use,
    ];

    /// The word `text` spells, `Other` for anything else.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        // bca: suppress(cyclomatic) — one arm per word, a lookup table
        match text {
            "if" => Self::If,
            "unless" => Self::Unless,
            "for" => Self::For,
            "while" => Self::While,
            "case" => Self::Case,
            "cond" => Self::Cond,
            "with" => Self::With,
            "try" => Self::Try,
            "receive" => Self::Receive,
            "throw" => Self::Throw,
            "raise" => Self::Raise,
            "reraise" => Self::Reraise,
            "exit" => Self::Exit,
            "def" => Self::Def,
            "defp" => Self::Defp,
            "defmacro" => Self::Defmacro,
            "defmacrop" => Self::Defmacrop,
            "defmodule" => Self::Defmodule,
            "defprotocol" => Self::Defprotocol,
            "defimpl" => Self::Defimpl,
            "defstruct" => Self::Defstruct,
            "defguard" => Self::Defguard,
            "defguardp" => Self::Defguardp,
            "quote" => Self::Quote,
            "alias" => Self::Alias,
            "import" => Self::Import,
            "require" => Self::Require,
            "use" => Self::Use,
            _ => Self::Other,
        }
    }

    /// The method-defining macros: `def` / `defp` / `defmacro` /
    /// `defmacrop`.
    #[inline]
    #[must_use]
    pub fn is_method_macro(self) -> bool {
        matches!(
            self,
            Self::Def | Self::Defp | Self::Defmacro | Self::Defmacrop
        )
    }

    /// The abrupt exits: `throw` / `raise` / `reraise` / `exit`.
    #[inline]
    #[must_use]
    pub fn is_exit(self) -> bool {
        matches!(self, Self::Throw | Self::Raise | Self::Reraise | Self::Exit)
    }

    fn from_slot(value: u8) -> Self {
        Self::ALL
            .get(usize::from(value))
            .copied()
            .unwrap_or(Self::Other)
    }
}

/// [`ElixirCallTarget`] of `node`: `Other` for anything but a `call`
/// whose `target` is an `identifier` spelling one of the words. Read
/// once per `call` per walk when the walk lent a cache — the slot holds
/// the last node asked (#1697); every row gate and metric consumer that
/// wants the word goes through here, so the `call_keyword_reads`
/// counter counts misses.
#[inline]
#[must_use]
pub fn elixir_call_target<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> ElixirCallTarget {
    if node.kind_id() != Elixir::Call as u16 {
        return ElixirCallTarget::Other;
    }
    ElixirCallTarget::from_slot(ancestors.classified(WordSlot::ElixirCallTarget, node, || {
        elixir_call_keyword(node, code).map_or(ElixirCallTarget::Other, ElixirCallTarget::from_text)
            as u8
    }))
}

/// What a `call` declares, read from its target text once: a module or
/// a method (outside a `quote` template — #310). The `FuncOpener` and
/// `Container` rows and the walk's combined promotion lookup all read
/// this one answer, so a `call` pays one keyword scan however many of
/// them ask (#310 follow-on perf, #1696).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElixirDeclaration {
    /// `defmodule`.
    Module,
    /// `def` / `defp` / `defmacro` / `defmacrop`, outside any `quote`.
    Method,
}

/// [`ElixirDeclaration`] of `node`; `None` for anything but a `call`
/// whose target spells a declaring macro, a method macro inside a
/// `quote` template included.
#[must_use]
pub(crate) fn elixir_declaration<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<ElixirDeclaration> {
    match elixir_call_target(node, code, ancestors) {
        ElixirCallTarget::Defmodule => Some(ElixirDeclaration::Module),
        target
            if target.is_method_macro() && !elixir_is_inside_quote_block(node, code, ancestors) =>
        {
            Some(ElixirDeclaration::Method)
        }
        _ => None,
    }
}

/// Returns true when `node` is lexically nested inside the `do_block` of a
/// `quote do … end` Call (Elixir's metaprogramming template). A `def` /
/// `defp` / `defmacro` / `defmacrop` inside `quote` does not define a
/// method of any enclosing module — the syntax tree is a code template
/// emitted later, when the surrounding macro is invoked. Treating those
/// quoted Calls as methods inflates `Wmc` and disagrees with `Npm`'s
/// direct-children classification (#310).
///
/// Under a walk that lent a [`FoldCache`](crate::FoldCache), the answer
/// is folded down the chain once per ancestor — "is this, or anything
/// above it, a `quote`" — so each query is amortised `O(1)`. The climb
/// it replaces stopped at the first `quote` ancestor and was `O(1)` for
/// a `def` directly inside one, but a `def` under no `quote` at all
/// climbed to the root: one such `def` per nesting level measured
/// quadratic (0.12 s / 0.47 s / 1.97 s at depths 1000 / 2000 / 4000,
/// exponent 2.0) through `is_func_space_with_code` alone, which the
/// `nom/nested-quote` probe could not see because every `def` there sits
/// one step under a `quote` (lesson 82). Without a lent cache the climb
/// runs as before, each step `O(1)` on a known chain and `O(depth)` with
/// `Ancestors::unknown` (#1084).
#[inline]
#[must_use]
pub(crate) fn elixir_is_inside_quote_block<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    if ancestors.memoises_folds() {
        return ancestors.fold_down(
            node,
            0,
            FoldLane::InsideQuote,
            0,
            |inside, ancestor, _, _| {
                u8::from(inside == 1 || elixir_call_keyword(ancestor, code) == Some("quote"))
            },
        ) == 1;
    }
    ancestors
        .iter(node)
        .any(|(n, _)| elixir_call_keyword(&n, code) == Some("quote"))
}

/// Iterates the direct-child `Call` nodes inside the `do_block` of an
/// Elixir Call (typically a `defmodule`). Used by `Npm` / `Npa` to scan
/// a module body for method-defining macros / `defstruct` without
/// descending into nested modules. Yields no items when the Call has
/// no `do_block`.
#[inline]
pub fn elixir_do_block_call_children<'a>(
    node: &'a Node<'a>,
) -> impl Iterator<Item = Node<'a>> + 'a {
    node.children()
        .filter(|child| child.kind_id() == Elixir::DoBlock as u16)
        .flat_map(|do_block| do_block.children())
        .filter(|stmt| stmt.kind_id() == Elixir::Call as u16)
}

/// The `binary_operator` that applies `op`, an operator token, or `None`
/// when the token is not applied there.
///
/// An operator token also *names* the operator: `&==/2`, `&and/2` and
/// `Kernel.||(a, b)` hold it inside an `operator_identifier`, and the
/// single-token `atom` / `keyword` spell it too. None of those is a
/// `binary_operator` with operands to compare or chain, and under error
/// recovery the parent can be an `ERROR` with no operands, so every
/// decision metric keys on this parent rather than on the token (#1531,
/// #1534). A remote call that names the operator still *applies* it,
/// though; see [`elixir_applying_remote_call`].
///
/// Matched by rule name rather than by `kind_id`: the grammar aliases
/// `binary_operator` to three ids (`Elixir::BinaryOperator` through
/// `BinaryOperator3`), and a name comparison stays correct when a bump
/// adds a fourth (grammar-dispatch §1).
#[must_use]
pub fn elixir_applying_operator<'a>(
    op: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    ancestors
        .parent(op)
        .filter(|parent| parent.kind() == "binary_operator")
}

/// The name of the short-circuit operator a remote call on `Kernel` with
/// arguments applies, or `None` for any other node: the `||` of
/// `Kernel.||(a, b)`, or of `x |> Kernel.||(y)`, which the pipe rewrites
/// to it. Such a call expands the `Kernel` macro exactly as `a || b`
/// does, so it short-circuits and decides as the operator does, where a
/// capture (`&Kernel.||/2`, a call with no arguments) only names it. It
/// has no `binary_operator`, so its operands are the call's arguments,
/// and under a pipe the piped value, rather than `left` / `right` fields.
///
/// The receiver must spell `Kernel` (or `Elixir.Kernel`): a user module
/// may define `||/2` as an ordinary function, and `Foo.||(a, b)` neither
/// short-circuits nor decides.
///
/// The grammar lexes a symbolic name as a token inside the call's
/// `operator_identifier`, which is what is returned. `Kernel.and(a, b)`
/// and `Kernel.or(a, b)` spell their names as a plain `identifier`, with
/// no `And` / `Or` token to find, so that identifier is returned instead
/// (#1582). Any other name — `Kernel.+`, `Kernel.!`, `Kernel.not`,
/// `Kernel.in` — is a function or decides nothing, and is not found; nor
/// is the childless `..`. [`elixir_short_circuit_name`] says which
/// operation the name applies.
///
/// Every identifier a walk meets asks this of its call, so the receiver,
/// which turns nearly all of them away without a cursor, is read before
/// the call's children are scanned for its `arguments`.
///
/// Matched by rule name: `dot` and `arguments` are aliased to several
/// ids each (grammar-dispatch §1).
#[must_use]
pub fn elixir_kernel_short_circuit_name<'a>(call: &Node<'a>, code: &[u8]) -> Option<Node<'a>> {
    kernel_short_circuit_parts(call, code).map(|(name, _)| name)
}

fn kernel_short_circuit_parts<'a>(call: &Node<'a>, code: &[u8]) -> Option<(Node<'a>, Node<'a>)> {
    if call.kind() != "call" {
        return None;
    }
    let dot = call.child_by_field_name("target").filter(|target| {
        target.kind() == "dot"
            && target.child_by_field_name("left").is_some_and(|receiver| {
                matches!(
                    code.get(receiver.start_byte()..receiver.end_byte()),
                    Some(b"Kernel" | b"Elixir.Kernel")
                )
            })
    })?;
    let name = dot.child_by_field_name("right")?;
    let name = if name.kind() == "operator_identifier" {
        name.child(0)?
    } else {
        name
    };
    elixir_short_circuit_name(&name, code)
        .and_then(|_| call.children().find(|child| child.kind() == "arguments"))
        .map(|arguments| (name, arguments))
}

/// The short-circuit operation an operator token kind applies, keyed by
/// its symbol: `&&` for `&&` and `and`, `||` for `||` and `or`. A word
/// form demands a boolean left operand, but it is the same conjunction or
/// disjunction to read, so one sequence key serves both spellings.
/// `None` for any other kind.
#[must_use]
pub fn elixir_short_circuit_kind(kind_id: u16) -> Option<Elixir> {
    match kind_id.into() {
        Elixir::AMPAMP | Elixir::And => Some(Elixir::AMPAMP),
        Elixir::PIPEPIPE | Elixir::Or => Some(Elixir::PIPEPIPE),
        _ => None,
    }
}

/// [`elixir_short_circuit_kind`] for an operator's name as
/// [`elixir_kernel_short_circuit_name`] returns it: a token, or the
/// `identifier` spelling `Kernel.and` / `Kernel.or` (#1582), whose
/// bytes decide. Ask it only of such a name — an identifier is an `and`
/// operator nowhere else.
#[must_use]
pub fn elixir_short_circuit_name(name: &Node, code: &[u8]) -> Option<Elixir> {
    if name.kind_id() != Elixir::Identifier as u16 {
        return elixir_short_circuit_kind(name.kind_id());
    }
    match code.get(name.start_byte()..name.end_byte())? {
        b"and" => Some(Elixir::AMPAMP),
        b"or" => Some(Elixir::PIPEPIPE),
        _ => None,
    }
}

/// A recognized short-circuit `Kernel` call and the context its operands need.
pub struct ElixirRemoteCall<'tree, 'chain> {
    /// The call that applies the operator.
    pub call: Node<'tree>,
    /// The argument list found while recognizing the call.
    pub arguments: Node<'tree>,
    /// The call's own ancestry.
    pub ancestors: Ancestors<'tree, 'chain>,
}

/// The `Kernel` remote call that applies `op`, a short-circuit operator's
/// name — the call [`elixir_kernel_short_circuit_name`] answers `op` for
/// — with its argument list and ancestry, or `None` when `op` sits
/// anywhere else. A token's parents are the `operator_identifier`, then
/// the `dot`, then the call; the `identifier` naming `Kernel.and` sits
/// on the `dot` directly. Any other node fails on the first one or two
/// parents' kinds, so the check is cheap enough to ask of every
/// identifier a walk meets.
#[must_use]
pub fn elixir_applying_remote_call<'a, 'c>(
    op: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, 'c>,
) -> Option<ElixirRemoteCall<'a, 'c>> {
    let mut up = ancestors.iter(op);
    let (mut dot, _) = up.next()?;
    if dot.kind() == "operator_identifier" {
        dot = up.next()?.0;
    }
    if dot.kind() != "dot" {
        return None;
    }
    let (call, call_ancestors) = up.next()?;
    kernel_short_circuit_parts(&call, code)
        .filter(|(name, _)| name.id() == op.id())
        .map(|(_, arguments)| ElixirRemoteCall {
            call,
            arguments,
            ancestors: call_ancestors,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElixirCode;
    use crate::node::Tree;
    use crate::test_support::for_each_node_with_chain;

    #[cfg(feature = "elixir")]
    #[test]
    fn short_circuit_name_requires_the_identifiers_source_span() {
        let source = b"Kernel.and(a, b)";
        let tree = Tree::new::<ElixirCode>(source);
        let name = tree
            .get_root()
            .preorder()
            .find(|node| {
                node.kind_id() == Elixir::Identifier as u16 && node.utf8_text(source) == Some("and")
            })
            .expect("fixture contains the operator name");
        assert_eq!(
            elixir_short_circuit_name(&name, source),
            Some(Elixir::AMPAMP)
        );
        assert_eq!(
            elixir_short_circuit_name(&name, &source[..name.end_byte() - 1]),
            None
        );
    }

    #[cfg(feature = "elixir")]
    #[test]
    fn remote_call_lookup_rejects_a_root_and_keeps_its_argument_list() {
        let source = b"Kernel.and(a, b)\nKernel.||(c, d)\n";
        let mut recognized = Vec::new();
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            let known = elixir_applying_remote_call(node, source, Ancestors::known(chain));
            let unknown = elixir_applying_remote_call(node, source, Ancestors::unknown());
            assert_eq!(
                known.as_ref().map(|r| (r.call.id(), r.arguments.id())),
                unknown.map(|r| (r.call.id(), r.arguments.id()))
            );
            if chain.is_empty() {
                assert!(known.is_none());
            }
            if let Some(remote) = known {
                assert_eq!(remote.arguments.kind(), "arguments");
                assert_eq!(
                    remote.ancestors.parent(&remote.call).map(|p| p.kind()),
                    Some("source")
                );
                recognized.push((
                    node.utf8_text(source)
                        .expect("operator is UTF-8")
                        .to_owned(),
                    remote
                        .arguments
                        .utf8_text(source)
                        .expect("arguments are UTF-8")
                        .to_owned(),
                ));
            }
        });
        assert_eq!(
            recognized,
            [
                ("and".to_owned(), "(a, b)".to_owned()),
                ("||".to_owned(), "(c, d)".to_owned())
            ]
        );
    }

    // The memoised fold answers as the climb it replaced does, on every
    // `call` of a fixture that has quoted and unquoted `def`s at several
    // depths, a `quote` nested in a `quote`, and a `def` *after* a
    // `quote` closes — where a fold that leaked state across siblings
    // would still say "inside". Both spellings are asked through the
    // walker's chain, one with a lent cache and one without. A fold that
    // dropped the parent itself (`skip` of 1) is not observable here: a
    // `def` is never a direct child of its `quote` call, since the
    // grammar puts a `do_block` or an `arguments` list between them, so
    // the two spellings agree on that perturbation by construction.
    #[cfg(feature = "elixir")]
    #[test]
    fn inside_quote_fold_answers_as_the_climb_does() {
        use crate::node::{FoldCache, Tree};
        let source = b"defmodule M do\n  def a, do: 1\n  defmacro m do\n    quote do\n      def b, do: 2\n      quote do\n        def c, do: 3\n      end\n      if true do\n        def d, do: 4\n      end\n    end\n    def e, do: 5\n  end\n  if true do\n    if true do\n      def f, do: 6\n    end\n  end\nend\n";
        let tree = Tree::new::<ElixirCode>(source);
        assert!(!tree.get_root().has_error());
        let cache = FoldCache::default();
        let mut answers = Vec::new();
        let mut stack = vec![(tree.get_root(), 0_usize)];
        let mut chain: Vec<Node> = Vec::new();
        while let Some((node, depth)) = stack.pop() {
            chain.truncate(depth);
            if let Some(kw) = elixir_call_keyword(&node, source) {
                let folded = elixir_is_inside_quote_block(
                    &node,
                    source,
                    Ancestors::known(&chain).with_folds(&cache),
                );
                let climbed = elixir_is_inside_quote_block(&node, source, Ancestors::known(&chain));
                assert_eq!(folded, climbed, "`{kw}` at depth {depth}");
                if ElixirCallTarget::from_text(kw).is_method_macro() {
                    answers.push((kw.to_owned(), folded));
                }
            }
            chain.push(node);
            let children: Vec<_> = node.children().collect();
            stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
        }
        // Keyed on the `def`s only, so the expectation reads as the
        // fixture does: `b`, `c`, `d` are quoted; `a`, `e`, `f` are not.
        let defs: Vec<_> = answers.iter().map(|(_, inside)| *inside).collect();
        assert_eq!(answers.len(), 7, "{answers:?}");
        assert_eq!(
            defs,
            [false, false, true, true, true, false, false],
            "{answers:?}"
        );
    }

    // The word slot's budget (#1697): asked about one `call` several
    // times in a row — as cyclomatic, cognitive, ABC and nexits ask on
    // one visit — a walk that lent a cache reads the target once per
    // `call`, and one that did not reads it every time. The fixture has
    // no ask about another node between the repeats, so misses == calls
    // exactly; the answers agree either way, so only the counter tells
    // the two apart.
    #[cfg(feature = "elixir")]
    #[test]
    fn call_target_is_read_once_per_call_under_a_cache() {
        use crate::node::FoldCache;
        const ASKS: usize = 4;
        let source = b"defmodule M do\n  def f(a) do\n    if a, do: throw(:x)\n    unless a, do: raise(\"y\")\n    case a do\n      _ -> g()\n    end\n    Kernel.exit(1)\n  end\nend\n";
        let cache = FoldCache::default();
        let mut calls = 0;
        let mut cached = Vec::new();
        let before = call_keyword_reads::observed();
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            if node.kind_id() != Elixir::Call as u16 {
                return;
            }
            calls += 1;
            let chain = Ancestors::known(chain).with_folds(&cache);
            let first = elixir_call_target(node, source, chain);
            for _ in 1..ASKS {
                assert_eq!(elixir_call_target(node, source, chain), first);
            }
            cached.push(first);
        });
        let cached_reads = call_keyword_reads::observed() - before;
        let mut uncached = Vec::new();
        let before = call_keyword_reads::observed();
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            if node.kind_id() == Elixir::Call as u16 {
                let chain = Ancestors::known(chain);
                for _ in 0..ASKS {
                    uncached.push(elixir_call_target(node, source, chain));
                }
            }
        });
        let uncached_reads = call_keyword_reads::observed() - before;
        assert_eq!(
            cached,
            [
                ElixirCallTarget::Defmodule,
                ElixirCallTarget::Def,
                ElixirCallTarget::Other, // f(a)
                ElixirCallTarget::If,
                ElixirCallTarget::Throw,
                ElixirCallTarget::Unless,
                ElixirCallTarget::Raise,
                ElixirCallTarget::Case,
                ElixirCallTarget::Other, // g()
                ElixirCallTarget::Other, // Kernel.exit
            ]
        );
        assert_eq!(calls, cached.len());
        assert_eq!(
            uncached,
            cached
                .iter()
                .flat_map(|&t| std::iter::repeat_n(t, ASKS))
                .collect::<Vec<_>>()
        );
        assert_eq!((cached_reads, uncached_reads), (calls, ASKS * calls));
    }

    // A node that is not a `call` is `Other` before any read, and the
    // counter shows no `target` field was read for it.
    // `elixir_call_keyword`'s own kind check is what the counter
    // observes; the outer early return in `elixir_call_target` is
    // unobservable through it while the inner check stands (a slot
    // write saved, not a correctness guard).
    #[cfg(feature = "elixir")]
    #[test]
    fn call_target_of_a_non_call_is_other_without_a_read() {
        let source = b"defmodule M do\n  def f(a), do: a + 1\nend\n";
        let before = call_keyword_reads::observed();
        // Collected rather than asserted per node: an `assert_eq!` with a
        // formatted message inside the closure leaves its never-taken
        // panic arm as uncovered lines in the coverage report, which
        // reads as a gap in production code.
        let mut seen = Vec::new();
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            if node.kind_id() != Elixir::Call as u16 {
                seen.push((
                    node.kind(),
                    elixir_call_keyword(node, source).is_some(),
                    call_keyword_reads::observed() - before,
                    elixir_call_target(node, source, Ancestors::known(chain)),
                ));
            }
        });
        assert!(
            seen.len() > 5,
            "the fixture has non-call nodes to ask about"
        );
        let read = seen
            .iter()
            .filter(|&&(_, keyword, reads, target)| {
                keyword || reads != 0 || target != ElixirCallTarget::Other
            })
            .map(|&(kind, ..)| kind)
            .collect::<Vec<_>>();
        assert_eq!(
            read,
            Vec::<&str>::new(),
            "a non-call was read or classified"
        );
        assert_eq!(call_keyword_reads::observed(), before);
    }

    // Every discriminant the slot can hold converts back to the variant
    // it came from; a value past the table — unreachable while `ALL`
    // and the enum agree — is `Other`, never a panic.
    #[test]
    fn call_target_round_trips_through_the_slot() {
        for target in ElixirCallTarget::ALL {
            assert_eq!(ElixirCallTarget::from_slot(target as u8), target);
        }
        assert_eq!(
            ElixirCallTarget::from_slot(u8::try_from(ElixirCallTarget::ALL.len()).expect("fits")),
            ElixirCallTarget::Other
        );
        for (index, target) in ElixirCallTarget::ALL.iter().enumerate() {
            assert_eq!(*target as usize, index, "`ALL` is in discriminant order");
        }
    }
}
