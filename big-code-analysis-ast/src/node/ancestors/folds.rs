//! The folds and memos an [`Ancestors`] chain answers through, and the
//! [`FoldCache`] a walk lends for them. A fold answers a question about
//! a node's whole ancestry down the chain once per walk, where a climb
//! from every node that asks is quadratic in depth (#1597). A memo
//! keeps one node's answer for the walk, and a word slot the visited
//! node's. Tested in the parent module's `tests`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use super::Ancestors;
use super::chain::Chain;
use crate::checker::Checker;
use crate::node::Node;

impl<'tree, 'chain> Ancestors<'tree, 'chain> {
    /// The state of a fold of `step` down `node`'s ancestry, leaving out
    /// the `skip` ancestors nearest `node`.
    ///
    /// The fold starts at `init` above the root and, for each ancestor
    /// `a` from the root down, applies `step(state, a, a's chain, below)`,
    /// where `below` is the next node on the path to `node` — the ancestor
    /// under `a`, or `node` itself under its parent. A predicate that
    /// would otherwise climb every ancestor of every candidate node, and
    /// so cost `O(candidates × depth)`, asks this instead (#1597).
    ///
    /// Under a walk that lent a [`FoldCache`], the state at each ancestor
    /// is stored per depth, keyed by the ancestor's id, so each one is
    /// folded once per walk and a query costs amortised `O(1)` steps. A
    /// state is the fold over the ancestors *above* its key, so `step`
    /// runs for an ancestor once per child of it the walk asks from, and
    /// for every ancestor on the chain whatever it answers. A step that
    /// scans is therefore rescanned per child; a question the nearest
    /// ancestor settles belongs on
    /// [`nearest_level_answer`](Self::nearest_level_answer) (#1863).
    ///
    /// A node fixes its own ancestry, so a stored state whose id matches
    /// the chain is exact; a slot left by a node the walk has moved past
    /// holds another id and is refolded. `step` must answer the same for
    /// every call on one `lane` within a walk. It may fold on another
    /// lane, memoised; a fold on its own lane is answered unmemoised.
    /// The state at `node` (`skip == 0`) depends on `node`, so it is
    /// folded fresh. Without a known chain the ancestry is climbed with
    /// [`Node::parent`] first, which is `O(depth²)` per query; only tests
    /// and chainless callers reach that path, never the metrics walk.
    pub(crate) fn fold_down(
        self,
        node: &Node<'tree>,
        skip: usize,
        lane: FoldLane,
        init: u8,
        step: impl Fn(u8, &Node<'tree>, Ancestors<'tree, 'chain>, &Node<'tree>) -> u8,
    ) -> u8 {
        let Some(chain) = self.chain else {
            return fold_climbed(node, skip, init, step);
        };
        let end = chain.len().saturating_sub(skip);
        // The cache travels into `outer`, as it does through
        // `AncestorIter`, so a step that folds on another lane is
        // memoised too. Each lane is its own cell, so that fold borrows
        // nothing this one holds; a step folding on *this* lane finds it
        // borrowed and runs unmemoised instead (`FoldCache::fold`).
        let step_at = |state: u8, depth: usize| {
            step(
                state,
                &chain[depth],
                self.above(depth),
                chain.get(depth + 1).unwrap_or(node),
            )
        };
        match self.folds {
            Some(cache) => cache.fold(lane, chain, end, init, step_at),
            None => (0..end).fold(init, step_at),
        }
    }

    /// Whether an ancestor of `node` satisfies `check` before one
    /// satisfies `stop`, nearest first, an `else if`'s inner `if` never
    /// counting — what `node.count_specific_ancestors::<C>(self, check,
    /// stop) > 0` answers.
    ///
    /// That climb stops at the nearest `stop`, which bounds it only where
    /// one is near. A shape with none — a declaration in nested C++
    /// namespaces, any TypeScript declaration asked whether it is
    /// ambient — climbs to the root from every node and is quadratic in
    /// depth (#1767). Under a walk that lent a [`FoldCache`] this folds
    /// the answer down the chain on `lane` instead, each ancestor once
    /// per child the walk asks under: a `stop` resets it, a `check` sets
    /// it, and both are kind tests. Elsewhere the
    /// climb is the cheaper of the two and is what runs, so a caller
    /// with no cache keeps the climb's cost.
    ///
    /// A lane memoises whatever its first asker folded, so each
    /// [`GateLane`] belongs to one question: one `check` / `stop` pair
    /// and checker per walk. Both are plain `fn`s because the memo is
    /// only sound for a pure function of the ancestor; a closure that
    /// captured the asking node would hand every later node the first
    /// one's answer. The `else if` exclusion is kept for parity with the
    /// climb; no current asker can observe it.
    #[doc(hidden)]
    pub fn has_ancestor_before_stop<C: Checker>(
        self,
        node: &Node<'tree>,
        lane: GateLane,
        check: fn(&Node) -> bool,
        stop: fn(&Node) -> bool,
    ) -> bool {
        self.ancestor_before_stop::<C>(node, lane.fold_lane(), check, stop)
    }

    /// [`has_ancestor_before_stop`](Self::has_ancestor_before_stop) on any
    /// lane, for this crate's own questions.
    pub(crate) fn ancestor_before_stop<C: Checker>(
        self,
        node: &Node<'tree>,
        lane: FoldLane,
        check: fn(&Node) -> bool,
        stop: fn(&Node) -> bool,
    ) -> bool {
        if !self.memoises_folds() {
            return node.count_specific_ancestors::<C>(self, check, stop) > 0;
        }
        self.fold_down(node, 0, lane, 0, |held, ancestor, outer, _| {
            if stop(ancestor) {
                0
            } else if check(ancestor) && !C::is_else_if(ancestor, outer) {
                1
            } else {
                held
            }
        }) == 1
    }

    /// Whether any ancestor of `node` has one of `kinds` — the question
    /// with no stop at all, which a nearest-first scan answers in a step
    /// or two where such an ancestor is near and by reading the whole
    /// chain where there is none: a function in each of `n` nested
    /// containers scanned `n²` entries (#1864). Under a walk that lent a
    /// [`FoldCache`] the answer is folded down the chain on `lane`, each
    /// ancestor's kind once per child the walk asks under; elsewhere the
    /// scan is what runs, since an unmemoised fold would start at the
    /// root for every ask.
    ///
    /// A lane memoises whatever its first asker folded, so every ask on
    /// `lane` within one walk must pass the same `kinds`: each
    /// [`KindLane`] belongs to one question and one list. The list is
    /// `'static` data rather than a predicate so that no ask can build
    /// it from the asking node; choosing between two lists per node is
    /// still the caller's to avoid. `make chain-audit` checks every
    /// folded answer against the scan, which is what catches a lane
    /// asked with a second list wherever the two answer differently.
    #[doc(hidden)]
    #[must_use]
    pub fn has_ancestor_of_kind(
        self,
        node: &Node<'tree>,
        lane: KindLane,
        kinds: &'static [u16],
    ) -> bool {
        let scan = || {
            self.iter(node)
                .any(|(ancestor, _)| kinds.contains(&ancestor.kind_id()))
        };
        if !self.memoises_folds() {
            return scan();
        }
        let folded = self.fold_down(node, 0, lane.fold_lane(), 0, |held, ancestor, _, _| {
            u8::from(held == 1 || kinds.contains(&ancestor.kind_id()))
        }) == 1;
        // Opt-in only, as the exact check in `for_walk_chain_only` is:
        // the scan is the `O(depth)` per ask the fold exists to remove.
        #[cfg(chain_audit)]
        assert_eq!(
            folded,
            scan(),
            "the {lane:?} fold disagrees with the scan on a {} node: a lane \
             holds one kind list per walk",
            node.kind()
        );
        folded
    }

    /// The answer of the nearest ancestor of `node` that has one, or
    /// `false` where none does: what
    /// `self.iter(node).find_map(|(level, chain)| answer(code, &level,
    /// chain)).unwrap_or(false)` answers. `node` itself is not asked.
    ///
    /// That climb stops at the first ancestor that answers, which bounds
    /// it only where one is near. A node asked from each level of a nest
    /// whose levels answer nothing — a parenthesis in each of `n`
    /// parentheses around a Python docstring, each asking whether code
    /// precedes it on its logical line — climbs the nest from every
    /// level and is quadratic in its depth (#1863). Under a walk that
    /// lent a [`FoldCache`] the climb is memoised per level on
    /// `question`: it stops at the first level that answers or already
    /// holds an answer, and hands what it found to every level it
    /// passed. So `answer` runs at most once per level per walk, and
    /// never on a level above the nearest one that answers.
    ///
    /// A fold ([`fold_down`](Self::fold_down)) does not fit this
    /// question: it evaluates every ancestor, the ones above the nearest
    /// answer included, and stores an ancestor's state under the child
    /// on the path, so a level whose answer is a long scan is scanned
    /// again for each child asked from.
    ///
    /// The memo is keyed by the level alone and hands every later asker
    /// the first one's answer. Three things follow. Each
    /// [`LevelQuestion`] belongs to one `answer` per walk. `answer` is a
    /// plain `fn` of the source, the ancestor and the ancestor's own
    /// chain: a closure that captured the asking node would answer for
    /// it at every level. And `self` must be `node`'s whole ancestry: a
    /// chain re-rooted part-way (`below`) runs off its top early, and
    /// "no level answers" would be stored on levels whose real ancestors
    /// do answer. Nothing checks that last one, since the audit's climb
    /// reads the same chain; no caller asks from a re-rooted chain.
    /// `make chain-audit` checks every memoised answer against the
    /// climb.
    #[doc(hidden)]
    #[must_use]
    pub fn nearest_level_answer(
        self,
        node: &Node<'tree>,
        question: LevelQuestion,
        code: &[u8],
        answer: fn(&[u8], &Node<'tree>, Ancestors<'tree, 'chain>) -> Option<bool>,
    ) -> bool {
        let climb = || {
            self.iter(node)
                .find_map(|(level, chain)| answer(code, &level, chain))
                .unwrap_or(false)
        };
        let Some(memo) = self.node_memo(question.memo_question()) else {
            return climb();
        };
        // The levels passed without an answer are counted, not listed:
        // they are the first `deferred` of the same iteration.
        let mut deferred = 0;
        let mut found = false;
        for (level, chain) in self.iter(node) {
            if let Some(known) = memo.get(&level) {
                found = known;
                break;
            }
            if let Some(own) = answer(code, &level, chain) {
                memo.set(&level, own);
                found = own;
                break;
            }
            deferred += 1;
        }
        for (level, _) in self.iter(node).take(deferred) {
            memo.set(&level, found);
        }
        // Opt-in only: the climb is the `O(depth)` per ask the memo
        // exists to remove.
        #[cfg(chain_audit)]
        assert_eq!(
            found,
            climb(),
            "the {question:?} memo disagrees with the climb on a {} node: a \
             question holds one answer fn per walk",
            node.kind()
        );
        found
    }

    /// The walk's memo for `question`, when it lent a [`FoldCache`].
    pub(crate) fn node_memo(self, question: MemoQuestion) -> Option<&'chain NodeMemo> {
        self.folds.map(|cache| &cache.nodes[question as usize])
    }

    /// `answer` for `node` under `question`, read from the walk's memo
    /// where it lent one and recorded there after the first ask; computed
    /// every time on a chain with no cache.
    pub(crate) fn memoised(
        self,
        question: MemoQuestion,
        node: &Node<'_>,
        answer: impl FnOnce() -> bool,
    ) -> bool {
        let memo = self.node_memo(question);
        if let Some(known) = memo.and_then(|memo| memo.get(node)) {
            return known;
        }
        let answer = answer();
        if let Some(memo) = memo {
            memo.set(node, answer);
        }
        answer
    }

    /// [`memoised`](Self::memoised) on a [`NodeQuestion`], for the metric
    /// crate's own per-node questions. `answer` must be a pure function
    /// of `node` and the source for one walk: the memo hands every later
    /// asker the first one's answer.
    #[doc(hidden)]
    pub fn memoised_answer(
        self,
        question: NodeQuestion,
        node: &Node<'_>,
        answer: impl FnOnce() -> bool,
    ) -> bool {
        self.memoised(question.memo_question(), node, answer)
    }

    /// `classify(node)`'s discriminant under `slot`. When `node` is the
    /// one the walk is visiting, as its constructor recorded
    /// ([`for_walk`](Self::for_walk),
    /// [`for_walk_without_siblings`](Self::for_walk_without_siblings)), the
    /// answer is read from the walk's word slot after the first ask and
    /// recorded there by it; an ask about any other node is computed and
    /// not stored, so it never evicts the visited node's answer (#1737).
    /// Computed every time on a chain with no cache, or one lent without
    /// a visit. The caller converts the `u8` back to its enum; `classify`
    /// must be a pure function of the node and the source for one walk.
    pub(crate) fn classified(
        self,
        slot: WordSlot,
        node: &Node<'_>,
        classify: impl FnOnce() -> u8,
    ) -> u8 {
        let Some(cache) = self.folds.filter(|cache| cache.visited.get() == node.id()) else {
            return classify();
        };
        let cell = &cache.words[slot as usize];
        let (id, answer) = cell.get();
        // Id 0 marks an empty slot: a tree-sitter node id is its address,
        // never 0.
        if id == node.id() {
            return answer;
        }
        let answer = classify();
        cell.set((node.id(), answer));
        answer
    }

    /// Whether [`fold_down`](Self::fold_down) is memoised on this chain:
    /// one a walk lent a [`FoldCache`] with. Otherwise every fold runs
    /// from the root, so a predicate whose own climb is bounded takes that
    /// instead.
    pub(crate) fn memoises_folds(self) -> bool {
        self.folds.is_some() && self.chain.is_some()
    }
}

// The deepest depth at or above `from` whose slot still holds the state of
// the ancestor the chain has there, or 0 when none does: a node fixes its
// ancestry, so a matching id is an exact state.
fn deepest_stored(slots: &[(usize, u8)], chain: Chain<'_, '_>, from: usize) -> usize {
    (1..=from)
        .rev()
        .find(|&depth| {
            fold_slot_probes::record();
            slots[depth].0 == chain[depth].id()
        })
        .unwrap_or(0)
}

// `Ancestors::fold_down` for a caller that holds no chain: the ancestry is
// climbed first and folded unmemoised.
fn fold_climbed<'tree, 'chain>(
    node: &Node<'tree>,
    skip: usize,
    init: u8,
    step: impl Fn(u8, &Node<'tree>, Ancestors<'tree, 'chain>, &Node<'tree>) -> u8,
) -> u8
where
    'tree: 'chain,
{
    let climbed = climbed_chain(node);
    let end = climbed.len().saturating_sub(skip);
    (0..end).fold(init, |state, depth| {
        let below = climbed.get(depth + 1).unwrap_or(node);
        step(state, &climbed[depth], Ancestors::unknown(), below)
    })
}

// `node`'s ancestors, root first, climbed with `Node::parent` for a caller
// that holds no chain. tree-sitter finds a parent by descending from the
// root, so the climb is `O(depth²)`.
pub(super) fn climbed_chain<'tree>(node: &Node<'tree>) -> Vec<Node<'tree>> {
    let mut climbed: Vec<_> = std::iter::successors(node.parent(), Node::parent).collect();
    climbed.reverse();
    climbed
}

/// The ancestor questions the metric crate asks through
/// [`Ancestors::has_ancestor_before_stop`], one per lane of the walk's
/// [`FoldCache`]. A subset of [`FoldLane`], so a caller outside this
/// crate cannot name a lane another fold owns.
#[doc(hidden)]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateLane {
    /// Whether a declaration sits in a loop or `if` header — a `for`
    /// initializer — with no statement block between: the C family,
    /// C#, Go, Groovy and Java. One lane serves all because a walk
    /// parses one language.
    InsideHeader,
    /// JS family: whether a declaration sits in an `export` statement
    /// with no statement block between.
    InsideExport,
    /// TypeScript: whether a declaration sits anywhere inside an
    /// ambient `declare`.
    InsideAmbient,
    /// Python: whether a boolean operator sits inside another before a
    /// `lambda`, so a chain is paid for once, at its outermost operator.
    InsideBooleanChain,
}

impl GateLane {
    const fn fold_lane(self) -> FoldLane {
        match self {
            Self::InsideHeader => FoldLane::InsideHeader,
            Self::InsideExport => FoldLane::InsideExport,
            Self::InsideAmbient => FoldLane::InsideAmbient,
            Self::InsideBooleanChain => FoldLane::InsideBooleanChain,
        }
    }
}

/// The ancestor questions the metric crate asks through
/// [`Ancestors::has_ancestor_of_kind`], one per lane of the walk's
/// [`FoldCache`]. Its own enum rather than more [`GateLane`] variants:
/// both entry points store the same two states, so a lane passed to the
/// wrong one would hand each question the other's memo.
#[doc(hidden)]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindLane {
    /// Whether a function definition sits inside another function, at
    /// any distance: the cognitive depth surcharge of every language
    /// with a function-definition kind. One lane serves all because a
    /// walk parses one language, and each passes one list.
    InsideFunction,
}

impl KindLane {
    const fn fold_lane(self) -> FoldLane {
        match self {
            Self::InsideFunction => FoldLane::InsideFunction,
        }
    }
}

/// Which memoised fold a [`FoldCache`] slot holds. A language may run
/// more than one fold over the same walk, and two folds' states must not
/// share a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FoldLane {
    /// Whether an ancestor holds a doc comment in a function body.
    DocInBody,
    /// A positional language's declaration scope: Go's package level,
    /// Ruby's YARD namespace. One lane serves both because a walk parses
    /// one language.
    DocScope,
    /// Python: the climb from a node through the parentheses above it to
    /// a bare string statement (#1610).
    BareStringClimb,
    /// Elixir: whether an ancestor is a `quote do … end` call, under
    /// which a `def` is a template rather than a definition.
    InsideQuote,
    /// [`GateLane::InsideHeader`].
    InsideHeader,
    /// [`GateLane::InsideExport`].
    InsideExport,
    /// [`GateLane::InsideAmbient`].
    InsideAmbient,
    /// [`GateLane::InsideBooleanChain`].
    InsideBooleanChain,
    /// JS family: whether a function expression or arrow meets a name
    /// binding before a positional frame.
    NamedBinding,
    /// Python: how many `lambda`s enclose a node before the nearest
    /// statement boundary, saturating (#1767).
    LambdaDepth,
    /// [`KindLane::InsideFunction`].
    InsideFunction,
}

impl FoldLane {
    /// How many lanes a [`FoldCache`] holds; one per variant above.
    const COUNT: usize = 11;
}

// A lane past `COUNT` would index out of the array at run time, on the
// first walk that folds on it; this makes a variant added without the
// bump a build error instead.
const _: () = assert!(FoldLane::InsideFunction as usize + 1 == FoldLane::COUNT);

/// Which memoised per-node question a [`FoldCache`] [`NodeMemo`] holds.
/// Two questions asked of the same node must not share a slot, so each
/// has a memo of its own. A [`NodeQuestion`] or one of this crate's own
/// stores the node's answer; a [`LevelQuestion`] the nearest answer at
/// or above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemoQuestion {
    /// Doxygen: whether a preprocessor conditional holds an entry, in
    /// any branch and at any depth (#1613).
    ConditionalDeclares,
    /// Tcl family: whether a `word_list`'s own slot sequence can be read
    /// positionally — no `ERROR` or MISSING child of its own (#1686).
    ArgumentSlotsReadable,
    /// Tcl: whether a `word_list` is a brace-list `switch`'s — holds
    /// exactly one `braced_word`, the arm list — asked once per arm by
    /// the `CaseArm` row (#1695).
    SwitchArmList,
    /// Elixir: whether a `when` operator sits in a guard position — the
    /// outermost of a repeated-guard chain decides for every nested one,
    /// so each is asked once rather than climbing the chain per token
    /// (#1695).
    GuardOperator,
    /// JS family: whether an arrow's *parent* holds a `property_identifier`
    /// child — the name an unbound arrow is bound to. Asked of the parent
    /// once per arrow under it, and the scan is `O(width)` over the
    /// parent's children, so `run(() => 1, …, () => n)` asked it `n`
    /// times of one `arguments` and was quadratic (#1696 review).
    NamesAnArrow,
    /// Elixir: whether a `keywords` list holds a `do:` pair — what makes
    /// its `else:` / `rescue:` / `catch:` siblings a block-equivalent
    /// section. Asked of the list once per pair in it, and the scan is
    /// `O(width)` over the list (#1602).
    KeywordListHasDo,
    /// Elixir: whether a Call argument is followed by no operand — the
    /// position a keyword list must hold to be the Call's sections.
    /// Asked of the argument once per pair in it, and the scan is
    /// `O(siblings)` over whatever trails it, comments included (#1602
    /// review).
    ArgumentIsTrailing,
    /// Elixir: whether a bracketed argument's keyword list is the first
    /// operand in its brackets — `[do: 1, else: 2]` holds pairs alone,
    /// `[1, do: 1]` is a plain list. Asked of the list once per pair in
    /// it, and the scan is `O(siblings)` over what precedes the pairs,
    /// comments included (#1697 cost audit).
    KeywordsLeadTheirList,
    /// [`NodeQuestion::BareStringParenthesisLine`].
    BareStringParenthesisLine,
    /// [`LevelQuestion::CodeBeforeOnLine`].
    CodeBeforeOnLine,
    /// [`LevelQuestion::InConstBinding`].
    InConstBinding,
}

impl MemoQuestion {
    /// How many memos a [`FoldCache`] holds; one per variant above.
    const COUNT: usize = 11;
}

const _: () = assert!(MemoQuestion::InConstBinding as usize + 1 == MemoQuestion::COUNT);

/// The per-node questions the metric crate memoises through
/// [`Ancestors::memoised_answer`], one per [`MemoQuestion`] memo of the
/// walk's [`FoldCache`]. A subset of [`MemoQuestion`], as [`GateLane`]
/// is of [`FoldLane`], so a caller outside this crate cannot name a memo
/// another question owns.
#[doc(hidden)]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeQuestion {
    /// Python: whether a bare `\` run that starts a logical line inside
    /// a parenthesised bare string's parentheses is the string's line
    /// with no code before the string on it. A function of the
    /// parenthesis alone, asked once per run it holds; each ask climbed
    /// from the run past every earlier run and comment in the
    /// parenthesis, quadratic in them (fourth re-review of #1819).
    BareStringParenthesisLine,
}

impl NodeQuestion {
    const fn memo_question(self) -> MemoQuestion {
        match self {
            Self::BareStringParenthesisLine => MemoQuestion::BareStringParenthesisLine,
        }
    }
}

/// The "nearest ancestor that answers" questions the metric crate asks
/// through [`Ancestors::nearest_level_answer`], one per [`MemoQuestion`]
/// memo of the walk's [`FoldCache`]. Its own enum, apart from
/// [`NodeQuestion`]: that entry point stores a node's own answer and
/// this one the nearest answer at or above it, so a question passed to
/// the wrong one would hand each the other's memo.
#[doc(hidden)]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelQuestion {
    /// Python: whether code precedes a node on its logical line, read
    /// at the nearest level above it that has a token before its node.
    CodeBeforeOnLine,
    /// JS family: whether a destructuring pattern belongs to a `const`
    /// binding, read at the nearest ancestor that is no pattern layer.
    /// A default's `=` at each of `n` nested patterns climbed to the
    /// declarator from every level (#1868). One question serves the
    /// four grammars because a walk parses one language.
    InConstBinding,
}

impl LevelQuestion {
    const fn memo_question(self) -> MemoQuestion {
        match self {
            Self::CodeBeforeOnLine => MemoQuestion::CodeBeforeOnLine,
            Self::InConstBinding => MemoQuestion::InConstBinding,
        }
    }
}

/// The per-depth states of the walk's memoised folds, for
/// `Ancestors::fold_down` (#1597), the per-node memos for
/// `Ancestors::node_memo`, and the word slots keyed on the node the
/// walk is visiting (`Ancestors::classified`).
///
/// One slot per depth and lane, keyed by the id of the ancestor at that
/// depth: the state folded over every ancestor above it. Interior
/// mutability, because [`Ancestors`] lends it shared, as it lends a
/// [`SiblingCache`]. One cell per lane, because a fold's step may ask a
/// question another lane memoises — the JS family's doc-in-body fold
/// asks whether each ancestor is a function — and that fold must not
/// find its lane borrowed by the one in progress.
///
/// [`SiblingCache`]: super::SiblingCache
#[derive(Debug, Default)]
pub struct FoldCache {
    lanes: [RefCell<Vec<(usize, u8)>>; FoldLane::COUNT],
    nodes: [NodeMemo; MemoQuestion::COUNT],
    words: [Cell<(usize, u8)>; WordSlot::COUNT],
    /// The id of the node the walk is visiting, as [`Ancestors::for_walk`]
    /// or [`Ancestors::for_walk_without_siblings`] last recorded it; 0 —
    /// no node's id — until a walk records one.
    visited: Cell<usize>,
}

/// The text-classified constructs whose answer for the visited node a
/// walk keeps (#1697, #1737). A grammar that spells `if`, `switch` or
/// `return` as a generic `call` / `command` told apart only by its
/// target text is asked the same question of the same node by every
/// metric that runs on it — four reads of one field and one text
/// compare where a kind-based language pays none. One slot holding the
/// visited node's id and its answer serves them all, however many asks
/// about *other* nodes arrive in between: an ask about a node the walk
/// is not visiting (a consumer reading the enclosing call, a module
/// reading its statements) is computed and not stored, so it costs its
/// one read and nothing else. Keying on the visit, which
/// [`Ancestors::for_walk`] and [`Ancestors::for_walk_without_siblings`]
/// record, is what makes that so — before
/// #1737 the slot held the *last node asked* and such an ask evicted
/// the visited node's answer, which made "may this site use the cached
/// read" a judgement per call site. No map, no allocation, `O(1)`
/// memory, dropped with the walk.
///
/// The bound is one read per (consumer, target) per visit. The
/// last-asked slot served two cross-visit shapes for free that this
/// one does not — a keyword pair's enclosing call asked by each of its
/// sections, an `else` token's `if` — so a file of `N` keyword
/// sections or `N` if-else blocks pays `N` more reads than it did,
/// linear either way (`cognitive/elixir-keyword-sections` prices it:
/// 1.03 → 1.12, within noise on every fixture measured). That is
/// accepted because the visited path dominates: an Elixir `call` is
/// asked four to seven times while visited, and the typed
/// `NodeMemo<T>` fold #1737 offered as its second option would put a
/// `HashMap` probe on every one of those asks to save the tens of
/// nanoseconds the two cross-visit shapes re-read — measured on the
/// #1737 branch, not assumed, which is why the slot stays a `Cell`.
///
/// The slot holds the classification's `#[repr(u8)]` discriminant; the
/// enum is the language helper's (`ElixirCallTarget`, `TclCommandWord`),
/// which also owns the conversion back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WordSlot {
    /// Elixir: what a `call`'s `target` identifier spells.
    ElixirCallTarget,
    /// Tcl: what a `command`'s leading `simple_word` spells.
    TclCommandWord,
}

impl WordSlot {
    /// How many slots a [`FoldCache`] holds; one per variant above.
    const COUNT: usize = 2;
}

const _: () = assert!(WordSlot::TclCommandWord as usize + 1 == WordSlot::COUNT);

/// Answers that are a fact of one node, memoised for one walk and keyed
/// by node id, which is unique within a tree. Most are about the node's
/// own subtree: Doxygen's "does this preprocessor conditional hold an
/// entry", asked of every conditional a doc comment's stretch meets;
/// the Tcl family's "can this argument list be read by position", asked
/// by every braced word in it. A [`LevelQuestion`]'s is about the
/// node's ancestry instead: the nearest answer at or above it, which
/// [`Ancestors::nearest_level_answer`] stores only from a whole chain.
/// Unlike a [`FoldLane`] slot the entry is not keyed by depth, so an
/// answer stored for one sibling subtree serves every later question
/// about it (#1613). Each holds one [`MemoQuestion`].
#[derive(Debug, Default)]
pub(crate) struct NodeMemo(RefCell<HashMap<usize, bool>>);

impl NodeMemo {
    /// The stored answer for `node`, if any.
    pub(crate) fn get(&self, node: &Node<'_>) -> Option<bool> {
        self.0.borrow().get(&node.id()).copied()
    }

    /// Stores `answer` for `node`.
    pub(crate) fn set(&self, node: &Node<'_>, answer: bool) {
        self.0.borrow_mut().insert(node.id(), answer);
    }
}

impl FoldCache {
    /// Records `node` as the one the walk is visiting, which the word
    /// slots serve. The walk constructors call it, and nothing else
    /// writes the visit (#1737).
    pub(super) fn record_visit(&self, node: &Node<'_>) {
        self.visited.set(node.id());
    }

    // The state after folding `step(state, depth)` over `chain[..end]`,
    // reusing and storing the per-depth states on `lane`.
    //
    // The lane stays borrowed while `step` runs, so a step that folds on
    // the same lane would otherwise panic on the second borrow; it folds
    // unmemoised instead, which answers the same. No step does today, and
    // the cost of one that did is the step's own: a step that re-enters
    // at one ancestor pays a plain fold of that prefix, one that re-enters
    // at every ancestor pays `2^depth`, since nothing a re-entered lane
    // can store cuts the recursion short. Neither shows as a wrong answer,
    // so debug builds refuse the shape: the whole test corpus then guards
    // the contract.
    fn fold(
        &self,
        lane: FoldLane,
        chain: Chain<'_, '_>,
        end: usize,
        init: u8,
        step: impl Fn(u8, usize) -> u8,
    ) -> u8 {
        let Ok(mut slots) = self.lanes[lane as usize].try_borrow_mut() else {
            debug_assert!(
                same_lane_reentry::allowed(),
                "a fold step re-entered its own lane {lane:?}"
            );
            return (0..end).fold(init, step);
        };
        // `slots[d]` is the state at `chain[d]`: folded over `chain[..d]`.
        // Depth 0 is `init`; the state at `node` itself is never stored.
        let memo_end = end.min(chain.len().saturating_sub(1));
        if slots.len() <= memo_end {
            // Id 0 marks an empty slot: a tree-sitter node id is its
            // address, never 0.
            slots.resize(memo_end + 1, (0, 0));
        }
        let mut depth = deepest_stored(&slots, chain, memo_end);
        let mut state = if depth == 0 { init } else { slots[depth].1 };
        while depth < memo_end {
            fold_steps::record();
            state = step(state, depth);
            depth += 1;
            slots[depth] = (chain[depth].id(), state);
        }
        if end > memo_end {
            state = step(state, memo_end);
        }
        state
    }
}

// Ancestor states a memoised fold computes and stores, so a test can see
// a walk fold each ancestor once rather than once per asking node: the
// answers are identical either way (#1767). Readable from the root
// crate's tests through `test-support`, since most walks that lend the
// cache live there.
crate::observation::counter!(fold_steps, cfg(any(test, feature = "test-support")));

// Stored states a memoised fold compares against the chain to find where
// to resume. `fold_steps` cannot see a search that reads more slots than
// it then refolds — one started at the root, say — which costs `O(depth)`
// per ask with every state still stored once (#1864 review).
crate::observation::counter!(fold_slot_probes, cfg(any(test, feature = "test-support")));

/// Whether a fold may re-enter its own lane: refused everywhere, except
/// on a thread where the one test that pins the fallback has opted in.
/// A flag in every build rather than a `cfg(test)` branch, because the
/// root crate's tests compile this crate without `cfg(test)`, and a
/// branch they can never take reads as a line no test reaches.
pub(super) mod same_lane_reentry {
    use std::cell::Cell;

    thread_local! {
        static ALLOWED: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn allowed() -> bool {
        ALLOWED.get()
    }

    /// Opts the current thread in (`true`) or back out.
    #[cfg(test)]
    pub(in crate::node::ancestors) fn allow(allowed: bool) {
        ALLOWED.set(allowed);
    }
}
