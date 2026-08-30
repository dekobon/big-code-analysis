// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]
// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use crate::spaces::{Nesting, NestingMap};

use std::fmt;

use crate::checker::Checker;
use crate::macros::implement_metric_trait;
use crate::*;

// TODO: Find a way to increment the cognitive complexity value
// for recursive code. For some kind of languages, such as C++, it is pretty
// hard to detect, just parsing the code, if a determined function is recursive
// because the call graph of a function is solved at runtime.
// So a possible solution could be searching for a crate which implements
// a light language interpreter, computing the call graph, and then detecting
// if there are cycles. At this point, it is possible to figure out if a
// function is recursive or not.

/// The `Cognitive Complexity` metric.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Stats {
    structural: usize,
    structural_sum: usize,
    structural_min: usize,
    structural_max: usize,
    nesting: usize,
    total_space_functions: usize,
    boolean_seq: BoolSequence,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            structural: 0,
            structural_sum: 0,
            structural_min: usize::MAX,
            structural_max: 0,
            nesting: 0,
            total_space_functions: 1,
            boolean_seq: BoolSequence::default(),
        }
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "sum: {}, average: {}, min:{}, max: {}",
            self.cognitive(),
            self.cognitive_average(),
            self.cognitive_min(),
            self.cognitive_max()
        )
    }
}

impl Stats {
    /// Merges a second `Cognitive Complexity` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.structural_min = self.structural_min.min(other.structural_min);
        self.structural_max = self.structural_max.max(other.structural_max);
        self.structural_sum += other.structural_sum;
    }

    /// Returns the `Cognitive Complexity` metric value
    #[must_use]
    pub fn cognitive(&self) -> u64 {
        self.structural as u64
    }
    /// Returns the `Cognitive Complexity` sum metric value
    #[must_use]
    pub fn cognitive_sum(&self) -> u64 {
        self.structural_sum as u64
    }

    /// Returns the `Cognitive Complexity` minimum metric value.
    ///
    /// Collapses the `usize::MAX` sentinel that `Stats::default()` plants
    /// into `structural_min` to `0`, so a never-observed space
    /// serializes to a meaningful number rather than `1.8446744e19`.
    #[must_use]
    pub fn cognitive_min(&self) -> u64 {
        if self.structural_min == usize::MAX {
            0
        } else {
            self.structural_min as u64
        }
    }
    /// Returns the `Cognitive Complexity` maximum metric value
    #[must_use]
    pub fn cognitive_max(&self) -> u64 {
        self.structural_max as u64
    }

    /// Returns the `Cognitive Complexity` metric average value
    ///
    /// This value is computed dividing the `Cognitive Complexity` value
    /// for the total number of functions/closures in a space.
    ///
    /// The per-function divisor (shared with `cyclomatic`/`exit`/`nargs`,
    /// #512) is guarded with `.max(1)` via the shared `average` helper, so
    /// a space with no counted functions (or one where `Nom` was not
    /// selected) degrades to `sum / 1` instead of producing `inf`/`NaN`
    /// (#428).
    #[must_use]
    pub fn cognitive_average(&self) -> f64 {
        crate::metrics::average(self.cognitive_sum() as f64, self.total_space_functions)
    }
    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.structural_sum += self.structural;
    }
    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.structural_min = self.structural_min.min(self.structural);
        self.structural_max = self.structural_max.max(self.structural);
        self.compute_sum();
    }

    pub(crate) fn finalize(&mut self, total_space_functions: usize) {
        self.total_space_functions = total_space_functions;
    }
}

#[doc(hidden)]
/// Per-language computation of the cognitive complexity metric.
pub(crate) trait Cognitive
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    ///
    /// `code` is the source bytes underlying the parsed tree. Most
    /// languages ignore it: their control-flow constructs surface as
    /// distinct grammar productions (`IfStatement`, `WhileStatement`,
    /// …) and a `kind_id()` match is enough. Elixir is the exception
    /// — `if` / `unless` / `case` / `cond` / `for` / `while` / `with`
    /// all surface as `Call` nodes whose keyword target lives only in
    /// the source text (the `target` field is an `Identifier`). This
    /// matches the `Cyclomatic` / `Halstead` / `Exit` pattern of
    /// taking `code` so the same source-text dispatch can run here.
    ///
    /// `ancestors` is the chain the walker descended through. The
    /// grammars that spell `else if` as a nested `if` need an ancestor
    /// to recognise the continuation, and Python needs one to find the
    /// outermost operator of a boolean chain; resolving either from the
    /// node alone costs `O(depth)` per node (#1084).
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    );
}

/// Walks `node.children()` and folds each child whose `kind_id`
/// satisfies `is_op` into the boolean-sequence counter. The predicate
/// is the only thing that differs across the per-language short-
/// circuit helpers (`compute_*_booleans`); inlining the predicate as
/// a `Fn` closure lets each language declare its operator set with a
/// `matches!` pattern at the call site without duplicating the walk.
fn compute_booleans_with<F: Fn(u16) -> bool>(node: &Node, stats: &mut Stats, is_op: F) {
    let enclosing_end = node.end_byte();
    for child in node.children() {
        let id = child.kind_id();
        if is_op(id) {
            stats.structural =
                stats
                    .boolean_seq
                    .eval_based_on_prev(id, enclosing_end, stats.structural);
        }
    }
}

/// Two-operator specialization. Most call sites match exactly two
/// enum variants (`&&` + `||`, or `and` + `or`); this signature
/// keeps those call sites as plain `(node, stats, A, B)` rather than
/// forcing a closure.
fn compute_booleans<T: PartialEq + From<u16>>(node: &Node, stats: &mut Stats, typs1: T, typs2: T) {
    compute_booleans_with(node, stats, |id| {
        let converted: T = id.into();
        typs1 == converted || typs2 == converted
    });
}

#[derive(Debug, Default, Clone, PartialEq)]
struct BoolSequence {
    boolean_op: Option<(u16, usize)>,
}

impl BoolSequence {
    fn reset(&mut self) {
        // Structural boundaries (new branches, nesting increments) end the current sequence.
        self.boolean_op = None;
    }

    fn eval_based_on_prev(
        &mut self,
        bool_id: u16,
        enclosing_end: usize,
        structural: usize,
    ) -> usize {
        match self.boolean_op {
            // Same operator type and enclosing_end fits inside the previously seen
            // binary_expression span (pre-order: parent visited before child) →
            // continuation of the same sequence, no extra cost.
            Some((prev_id, prev_end)) if prev_id == bool_id && enclosing_end <= prev_end => {
                structural
            }
            _ => {
                self.boolean_op = Some((bool_id, enclosing_end));
                structural + 1
            }
        }
    }
}

#[inline]
fn increment(stats: &mut Stats) {
    stats.structural += stats.nesting + 1;
}

#[inline]
fn increment_by_one(stats: &mut Stats) {
    stats.structural += 1;
}

#[inline]
fn increment_branch_extension(stats: &mut Stats) {
    stats.structural += 1;
    stats.boolean_seq.reset();
}

/// Returns the [`Nesting`] `node` inherits from its parent.
///
/// The map is keyed so that a node's own slot holds what it *inherits*:
/// the walker seeds each child's slot from its parent's slot after the
/// parent's `compute` has run (see `propagate_nesting_to_children` in
/// `spaces::compute`). Reading `node.parent()` here instead would cost
/// `O(depth)` per node — `Node::parent` walks down from the root — which
/// made this metric quadratic in nesting depth (#1062).
fn get_nesting_from_map(node: &Node, nesting_map: &NestingMap) -> Nesting {
    nesting_map.get(&node.id()).copied().unwrap_or_default()
}

/// Adds one to `depth` when `node` is lexically nested inside another
/// function, where `stops` lists the grammar kinds that count as a
/// function for this language.
///
/// The scan reads the walker's ancestor chain. Climbing with
/// [`Node::parent`] instead costs `O(depth)` per step — tree-sitter
/// stores no parent pointer — which made `Cognitive` `O(depth²)` on
/// nested function definitions (#1062, deferred out of #1084). A nested
/// function's enclosing function is a couple of levels up, so the scan
/// stops immediately there; a function with *no* enclosing function
/// scans its whole chain, but each step is a slice index rather than a
/// descent from the root.
fn increment_function_depth<'a, T: PartialEq + From<u16>>(
    depth: &mut usize,
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stops: &[T],
) {
    if ancestors
        .iter(node)
        .any(|(ancestor, _)| stops.contains(&T::from(ancestor.kind_id())))
    {
        *depth += 1;
    }
}

/// Applies the function-boundary rule at `node`, which every language
/// with a syntactic function-definition kind shares (#696).
///
/// It moves all three of [`Nesting`]'s channels. Structural nesting and
/// the lambda surcharge restart at zero, so control flow written inside
/// this function is charged against its own depth rather than against
/// whatever enclosed the definition; and the function-depth surcharge
/// rises when this definition is itself lexically nested in one of
/// `stops`. Byte-equivalent constructs therefore score the same across
/// languages, which is the property the book's per-language deviations
/// list states.
///
/// The lambda reset was the JS macro's alone until #1187. Every other
/// language carried the enclosing closure's surcharge into a function
/// *declared inside* it, so the same body scored 3 or 2 depending on
/// whether something two levels up happened to be a closure — measured
/// in Rust, Java, C++, PHP and C#, where a `LocalFunctionStatement`
/// inside a lambda is idiomatic. A function declaration is a new lexical
/// scope whatever encloses it, so the reset belongs to every boundary,
/// and living here is what stops a language opting out by accident —
/// which is how the gap arose.
///
/// The two statements were spelled out longhand in eighteen modules
/// before #1103. One caller still spells them out: `elixir.rs` takes the
/// resets and deliberately skips the depth bump, and says why at its own
/// site.
fn enter_function_boundary<'a, T: PartialEq + From<u16>>(
    nesting: &mut Nesting,
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stops: &[T],
) {
    nesting.conditional = 0;
    nesting.lambda = 0;
    increment_function_depth(&mut nesting.function_depth, node, ancestors, stops);
}

/// Charges `node`'s construct at the current nesting level and opens a
/// new structural level for its children.
///
/// Takes the whole [`Nesting`] rather than its three same-typed fields
/// positionally: the previous signature was
/// `(stats, &mut nesting, depth, lambda)` at 43 call sites, where any
/// two of the trailing arguments could be transposed silently (#1086).
#[inline]
fn increase_nesting(stats: &mut Stats, nesting: &mut Nesting) {
    stats.nesting = nesting.total();
    increment(stats);
    nesting.conditional += 1;
    stats.boolean_seq.reset();
}

/// Whether `node` is a Python `lambda` expression, under either of the
/// grammar's two aliased kind_ids: `Lambda` (196, the concrete
/// production emitted today) and `Lambda2` (197, the currently-unseen
/// hidden alias). `Lambda3` (73) is the `lambda` *keyword* token, not a
/// closure node, and is intentionally excluded.
///
/// This is the single normalization chokepoint for the lambda-alias set
/// — mirroring `npa::python_is_block` for the block aliases (#419). It
/// is reused by the cognitive lambda-scope walks below and by
/// [`PythonCode::is_closure`](crate::checker), so a future grammar bump
/// that promotes `Lambda2` to a concrete node is handled in exactly one
/// place rather than drifting across sites (#422). The
/// `python_hidden_block_and_lambda_aliases_stay_unseen` drift guard in
/// `checker.rs` trips on such a bump.
pub(crate) fn python_is_lambda(node: &Node) -> bool {
    matches!(node.kind_id().into(), Python::Lambda | Python::Lambda2)
}

macro_rules! js_cognitive {
    ($lang:ident) => {
        fn compute<'a>(
            node: &Node<'a>,
            _code: &'a [u8],
            ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
            nesting_map: &mut NestingMap,
        ) {
            use $lang::*;
            let mut nesting = get_nesting_from_map(node, nesting_map);

            match node.kind_id().into() {
                IfStatement if !Self::is_else_if(node, ancestors) => {
                    increase_nesting(stats, &mut nesting);
                }
                ForStatement | ForInStatement | WhileStatement | DoStatement | SwitchStatement
                | CatchClause | TernaryExpression => {
                    increase_nesting(stats, &mut nesting);
                }
                // `Else` here is the `else` keyword token, which the
                // grammar also emits for the `else` of an `else if` —
                // so this arm covers both.
                Else => {
                    increment_by_one(stats);
                }
                // Per SonarSource Cognitive Complexity §B2, a labeled
                // `break LABEL` / `continue LABEL` is an unstructured jump
                // and adds +1. The JS-family grammar exposes the label as a
                // `StatementIdentifier` child (not the plain `Identifier`
                // Java uses), so gate on that kind; plain `break;` /
                // `continue;` have no such child and add +0.
                BreakStatement | ContinueStatement if node.is_child(StatementIdentifier as u16) => {
                    increment_by_one(stats);
                }
                ExpressionStatement => {
                    // Reset the boolean sequence
                    stats.boolean_seq.reset();
                }
                BinaryExpression => {
                    // `??` (`QMARKQMARK`) short-circuits like `&&` /
                    // `||`, so a chain of `??` collapses to a single
                    // boolean-sequence increment under Sonar B1.
                    compute_booleans_with(node, stats, |id| {
                        matches!(id.into(), AMPAMP | PIPEPIPE | QMARKQMARK)
                    });
                }
                AugmentedAssignmentExpression => {
                    // Compound short-circuit assignments `&&=`, `||=`,
                    // `??=` are semantically `x = x op y` and each carries
                    // one boolean-sequence decision, parallel to the
                    // cyclomatic fix from #231. The operator token sits
                    // inside the augmented-assignment node rather than a
                    // `BinaryExpression`, so it needs its own arm (#236).
                    compute_booleans_with(node, stats, |id| {
                        matches!(id.into(), AMPAMPEQ | PIPEPIPEEQ | QMARKQMARKEQ)
                    });
                }
                FunctionDeclaration
                | MethodDefinition
                | FunctionExpression
                | GeneratorFunctionDeclaration
                | GeneratorFunction
                    if Self::is_func(node, ancestors) =>
                {
                    // The kind set is `is_js_func!` minus `ArrowFunction`,
                    // and the `function_expression` half is re-derived by
                    // asking `Self::is_func` rather than copied flat, because
                    // `function_expression` covers both a function and a
                    // closure. `check_if_func!` is what separates them: its
                    // ancestor walk marks the expression a function when a
                    // binding frame (`var x = …`, `x = …`, `label:`, object
                    // `pair`) is reached before a positional one, and its
                    // `$extra` disjunct additionally marks any expression
                    // carrying its own `identifier` name child. So
                    // `const f = function () {}` and `run(function f () {})`
                    // are functions, while `run(function () {})` is a closure
                    // and must keep falling through to `_`. That
                    // classification is inherited from `is_func`, not
                    // endorsed here — it is also what makes `nom` call the
                    // same node a function or a closure, so cognitive
                    // disagreeing with it would be the larger bug.
                    // `ArrowFunction` stays out because it owns the lambda
                    // channel in the arm below. Listing `FunctionDeclaration`
                    // alone left a method or a bound function expression
                    // inheriting the enclosing conditional nesting (#1159).
                    //
                    // `stops` takes bare kinds, so re-applying that gate to
                    // an *ancestor* would mean changing
                    // `increment_function_depth`'s signature. Leaving it
                    // ungated is deliberate rather than a shortcut: an
                    // anonymous IIFE is a lexical function scope —
                    // `get_space_kind` maps every `function_expression` to
                    // `SpaceKind::Function` — so a `function` declared inside
                    // one really is nested in a function.
                    //
                    // `ArrowFunction` is in the list since #1187, which is
                    // what makes `(function () { function g() {…} })()` and
                    // `(() => { function g() {…} })()` both charge `g` a
                    // depth of 1; the arrow form charged 0 while the kind
                    // was absent. It cannot double-charge, because the sole
                    // caller resets `nesting.lambda` first.
                    //
                    // Both generator kinds are in the arm and in `stops`
                    // since #1186, which moved them from `is_js_closure!`
                    // to `is_js_func!`. The two halves are independent
                    // and had to move together: the arm decides whether
                    // `function* g()` resets its own inherited nesting,
                    // while `stops` decides whether a plain `function`
                    // nested *inside* a generator gets a depth surcharge.
                    // `GeneratorFunction` is gated by `Self::is_func` for
                    // the same reason `FunctionExpression` is — it has an
                    // optional name and covers both a function and a
                    // closure — while `GeneratorFunctionDeclaration`,
                    // like `FunctionDeclaration`, is unconditional.
                    enter_function_boundary(
                        &mut nesting,
                        node,
                        ancestors,
                        &[
                            FunctionDeclaration,
                            MethodDefinition,
                            FunctionExpression,
                            GeneratorFunctionDeclaration,
                            GeneratorFunction,
                            ArrowFunction,
                            ClassStaticBlock,
                        ],
                    );
                }
                // A class static block is a function boundary but is
                // deliberately *not* in `is_func` (#1184), so it needs
                // its own ungated arm rather than joining the gated one
                // above — gated, it would never fire. It is in the
                // `stops` list below for the same reason a
                // `function_expression` is: a `function` declared inside
                // a `static { … }` really is nested in one.
                ClassStaticBlock => {
                    enter_function_boundary(
                        &mut nesting,
                        node,
                        ancestors,
                        &[
                            FunctionDeclaration,
                            MethodDefinition,
                            FunctionExpression,
                            GeneratorFunctionDeclaration,
                            GeneratorFunction,
                            ArrowFunction,
                            ClassStaticBlock,
                        ],
                    );
                }
                ArrowFunction => {
                    nesting.lambda += 1;
                }
                _ => {}
            }
            nesting_map.insert(node.id(), nesting);
        }
    };
}

// Per-language `Cognitive` impls live in sibling modules. The `mod`
// declarations sit after the local `macro_rules! js_cognitive!` so
// textual macro scoping reaches the JS-family child files (mirrors
// `getter.rs`, `metrics::npm`, `metrics::cyclomatic`).
mod bash;
mod c;
mod cpp;
mod csharp;
mod elixir;
mod go;
mod groovy;
mod irules;
mod java;
mod javascript;
mod kotlin;
mod lua;
mod mozcpp;
mod mozjs;
mod objc;
mod perl;
mod php;
mod python;
mod ruby;
mod rust;
mod tcl;
mod tsx;
mod typescript;

// Reads the text of the `target` field of an Elixir `Call` node.
//
// Most of Elixir's control-flow constructs (`if`, `unless`, `for`,
// `while`, `case`, `cond`, `with`, `try`) and method-defining macros
// (`def`, `defp`, `defmacro`, …) parse as `Call` nodes whose `target`
// is an `Identifier` whose source text spells the keyword. The
// `Cyclomatic` and `Exit` impls already follow this pattern; this
// helper centralises the byte-text lookup so `Cognitive` and `Abc`
// can share it.
//
// Returns `None` for Calls whose target is not a simple identifier
// (e.g. `Module.func(…)` parses as `RemoteCallWithParentheses` with
// the dotted name as target) or when the bytes are not valid UTF-8.
pub(crate) fn elixir_call_keyword<'a>(node: &'a Node<'a>, code: &'a [u8]) -> Option<&'a str> {
    if node.kind_id() != Elixir::Call as u16 {
        return None;
    }
    let target = node.child_by_field_name("target")?;
    if target.kind_id() != Elixir::Identifier as u16 {
        return None;
    }
    target.utf8_text(code)
}

// Reads the leading word of a Tcl `command` node when it is a plain
// `simple_word` (`switch`, `for`, `puts`, …). Returns `None` for any other
// node kind, for commands whose leading word is computed (`$cmd`, `[cmd]`
// parse it as `variable_substitution` / `command_substitution`, never
// statically resolvable to a builtin), and for non-UTF-8 bytes. Shared by
// the out-of-band control-flow detectors below (grammar-dispatch §10:
// identity questions read the bytes).
//
// A *literal* name in a quoted or braced spelling is also unresolved, and
// that is a deliberate limitation: `"for" {set i 0} {$i < 3} {incr i} {…}`
// and `{for} …` are legal Tcl that still invoke the builtin, but the
// grammar parses their name as `quoted_word` / `braced_word` rather than
// `simple_word`, so they score as plain commands. The `simple_word` gate
// is what keeps the computed forms out; matching the quoted spellings
// would mean unquoting the bytes for a style no real Tcl uses.
//
// Callers dispatch on the returned name so each `command` node resolves it
// exactly once per metric walk — the helpers below take the resolved
// identity as a precondition rather than re-deriving it.
pub(crate) fn tcl_command_name<'a>(node: &'a Node<'a>, code: &'a [u8]) -> Option<&'a str> {
    if node.kind_id() != Tcl::Command as u16 {
        return None;
    }
    let name = node.child_by_field_name("name")?;
    if name.kind_id() != Tcl::SimpleWord as u16 {
        return None;
    }
    name.utf8_text(code)
}

// Tcl's `switch` is a generic `command` (no dedicated kind_id, unlike
// `if`/`while`/`foreach`/`catch`), so the kind-dispatch in the Cognitive
// and Cyclomatic impls never sees it (issue #467, lesson 19). Both metrics
// reach it through `tcl_command_name` and then through this pair: the arm
// list locates the supported form, and `tcl_switch_decision_arms` counts
// it. Cognitive needs only the former — it charges one structure whatever
// the arm count is — so the count stays out of its path.
//
// Grammar shape (tree-sitter-tcl 0.x), canonical brace-list form:
//
//   (command name: (simple_word "switch")
//     (word_list <options…> <value> (braced_word (command (simple_word PAT) …) …)))
//
// The arm list is the sole `braced_word` argument inside the command's
// `word_list`, which makes the lookup robust to leading options (`-exact`,
// `-glob`, `-regexp`, `-nocase`, `--`) and the matched value, all of which
// precede it and never parse as `braced_word`. The rarer split form
// (`switch $x a {b} c {d}` — arms as separate `word_list` arguments rather
// than wrapped in one `braced_word`) produces *several* sibling
// `braced_word`s, one per arm body, so requiring exactly one distinguishes
// the brace-list form and excludes the split form, where the last
// `braced_word` is merely a body rather than the full arm list. The split
// form is intentionally NOT counted: its body braces are sibling
// arguments, not nested commands, so there is no reliable arm node to
// count. Idiomatic Tcl uses the brace-list form, so this scoping
// under-counts only the uncommon style.
//
// **Precondition**: `node` is a `command` whose leading word resolved to
// `"switch"` — the callers' `tcl_command_name` dispatch establishes that,
// and re-checking it here would resolve the name a second time on every
// switch.
pub(crate) fn tcl_switch_arm_list<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let word_list = node
        .children()
        .find(|child| child.kind_id() == Tcl::WordList as u16)?;
    let mut braced_words = word_list
        .children()
        .filter(|child| child.kind_id() == Tcl::BracedWord as u16);
    let arm_list = braced_words.next()?;
    braced_words.next().is_none().then_some(arm_list)
}

// The number of *decision* arms of a `switch` — every arm but `default`,
// which is the fallback and does not contribute a decision point, matching
// the C-family `default:` convention (lesson 11). Each arm is a nested
// `command` whose leading word is the pattern.
//
// Carries [`tcl_switch_arm_list`]'s precondition, and returns `None` for
// the same unsupported split form, so callers leave it untouched exactly
// as they leave a non-switch command.
pub(crate) fn tcl_switch_decision_arms(node: &Node, code: &[u8]) -> Option<usize> {
    let decision_arms = tcl_switch_arm_list(node)?
        .children()
        .filter(|arm| arm.kind_id() == Tcl::Command as u16)
        .filter(|arm| {
            arm.child_by_field_name("name")
                .and_then(|pat| pat.utf8_text(code))
                != Some("default")
        })
        .count();
    Some(decision_arms)
}

// Tcl's `try` has a dedicated kind (unlike `switch`/`for`), but its error
// handler is a flat run of sibling tokens rather than a wrapper node
// (issue #1266):
//
//   (try "try" <body> ["on" "error" (arguments) <body>] [(finally)])
//
// The vendored grammar permits at most one `on error` handler and has no
// `trap` rule at all — Tcl 8.6's multi-handler and `trap` forms degrade to
// an `ERROR` node, a grammar limitation like the `for` condition in #1264.
// The scan still tolerates repetition so a grammar bump that adds those
// forms keeps counting without an edit here.
//
// Each handler body is located by structural position, never fixed index
// (grammar-dispatch §3): it is the child directly following the handler's
// `arguments` node. That makes the scan pairwise — every child paired with
// its predecessor, yielding the successor of each `arguments` — rather
// than a state machine over the `on` / `error` / `arguments` run. The
// grammar's extras are whitespace-only, so nothing can interpose between
// the `arguments` node and the body it introduces, and `arguments` appears
// among a `try`'s direct children only in handler position. Repetition
// costs nothing here: a grammar bump that admits several handlers (or
// `trap`) is counted without an edit, because each `arguments` yields its
// own successor. The `finally` clause is a wrapper kind (`Tcl::Finally`)
// and is never yielded — unconditional cleanup costs nothing, matching the
// cross-language `finally` convention (#416).
//
// Shared by Cognitive (which also seeds each body's nesting slot) and
// Cyclomatic (which counts the bodies); both call it from their `Tcl::Try`
// dispatch arm.
pub(crate) fn tcl_try_handler_bodies<'a>(node: &Node<'a>) -> impl Iterator<Item = Node<'a>> {
    node.children()
        .zip(node.children().skip(1))
        .filter(|(previous, _)| previous.kind_id() == Tcl::Arguments as u16)
        .map(|(_, body)| body)
}

// iRules counterpart to [`tcl_switch_decision_arms`]. Unlike Tcl, the iRules
// grammar models `switch` as a dedicated node with `switch_arm` children, so
// the arms are read off the tree directly instead of re-parsing a generic
// command. Returns the number of non-`default` arms (each a decision point in
// standard CCN); `None` when `node` is not a `switch`. The `default` arm is the
// fallback and does not contribute a branch (the Java/C-family wildcard
// convention — lesson 11, #106).
pub(crate) fn irules_switch_decision_arms(node: &Node, code: &[u8]) -> Option<usize> {
    if node.kind_id() != Irules::Switch as u16 {
        return None;
    }
    let decision_arms = node
        .children()
        .filter(|arm| arm.kind_id() == Irules::SwitchArm as u16)
        .filter(|arm| {
            arm.child_by_field_name("pattern")
                .and_then(|pat| pat.utf8_text(code))
                != Some("default")
        })
        .count();
    Some(decision_arms)
}

// Method-defining macros (`def`, `defp`, `defmacro`, `defmacrop`). The set
// is duplicated across checker, getter, and several metric impls
// because each consults it from a different trait surface; centralising
// the literal here keeps future additions (e.g. `defguard`) consistent.
#[inline]
pub(crate) fn elixir_is_method_macro(kw: &str) -> bool {
    matches!(kw, "def" | "defp" | "defmacro" | "defmacrop")
}

// Class-defining macro (`defmodule`). Paired with [`elixir_is_method_macro`]
// where a caller needs both ("any space-opening declaration").
#[inline]
pub(crate) fn elixir_is_class_macro(kw: &str) -> bool {
    kw == "defmodule"
}

// Returns true when `node` is lexically nested inside the `do_block` of a
// `quote do … end` Call (Elixir's metaprogramming template). A `def` /
// `defp` / `defmacro` / `defmacrop` inside `quote` does not define a
// method of any enclosing module — the syntax tree is a code template
// emitted later, when the surrounding macro is invoked. Treating those
// quoted Calls as methods inflates `Wmc` and disagrees with `Npm`'s
// direct-children classification (#310).
//
// Walks the ancestor chain looking for a `quote` Call ancestor. Stops at
// the first match (true) or at the root (false). Each step is a single
// `child_by_field_name("target")` + identifier byte compare, so the cost
// is O(steps) when `ancestors` is known — with `Ancestors::unknown` each
// step additionally pays `Node::parent`'s O(depth) (#1084).
pub(crate) fn elixir_is_inside_quote_block<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors
        .iter(node)
        .any(|(n, _)| elixir_call_keyword(&n, code) == Some("quote"))
}

// Iterates the direct-child `Call` nodes inside the `do_block` of an
// Elixir Call (typically a `defmodule`). Used by `Npm` / `Npa` to scan
// a module body for method-defining macros / `defstruct` without
// descending into nested modules. Yields no items when the Call has
// no `do_block`.
pub(crate) fn elixir_do_block_call_children<'a>(
    node: &'a Node<'a>,
) -> impl Iterator<Item = Node<'a>> + 'a {
    node.children()
        .filter(|child| child.kind_id() == Elixir::DoBlock as u16)
        .flat_map(|do_block| do_block.children())
        .filter(|stmt| stmt.kind_id() == Elixir::Call as u16)
}

implement_metric_trait!(Cognitive, PreprocCode, CcommentCode);

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
    use crate::test_support::{
        check_func_space_only_shim, check_metrics_only_shim, child_space, function_space,
    };

    use super::*;

    // Cognitive's dependency closure adds Nom, the divisor behind
    // `cognitive_average`.
    check_metrics_only_shim!(check_metrics, Cognitive);
    check_func_space_only_shim!(check_func_space, Cognitive);
    // The Python-comprehension tests (#417/#421) assert the cyclomatic
    // count alongside the cognitive one, to show where the two metrics
    // agree and where nesting makes them diverge. They are the only
    // cross-metric assertions here, so they get their own shim rather
    // than widening the module-wide selection.
    check_metrics_only_shim!(check_cognitive_and_cyclomatic, Cognitive, Cyclomatic);

    /// The walker must hand `is_else_if` the node's own parent at every
    /// AST depth.
    ///
    /// A bare `{ … }` block carries no cognitive weight in either
    /// language, so burying an `if / else if / else if` chain under more
    /// of them cannot move the score — unless the chain of ancestors the
    /// walker propagates (#1084) drifts. Then the inner `if`s stop
    /// reading as continuations of the branch above, each pays a fresh
    /// nesting penalty, and the total climbs with depth.
    ///
    /// Both `is_else_if` shapes are covered: C resolves the enclosing
    /// `else_clause` through the parent, Java through the preceding
    /// `else` token, which the chain answers by scanning the parent's
    /// children.
    #[test]
    fn else_if_is_recognised_at_every_nesting_depth() {
        use crate::test_support::metrics_verbatim;

        // 1 for the `if`, plus 1 for each `else if` as a branch
        // extension. No nesting penalty: an `else if` continues the
        // chain rather than nesting inside it.
        const CHAIN_COGNITIVE: u64 = 3;

        let chain = "if (a) { } else if (b) { } else if (c) { }";
        for depth in 0..=6 {
            let (open, close) = ("{ ".repeat(depth), " }".repeat(depth));
            for (lang, source) in [
                (LANG::C, format!("void f() {{ {open}{chain}{close} }}\n")),
                (
                    LANG::Java,
                    format!("class A {{ void m() {{ {open}{chain}{close} }} }}\n"),
                ),
            ] {
                let metrics = metrics_verbatim(lang, source.as_bytes(), MetricsOptions::default());
                assert_eq!(
                    metrics.cognitive.cognitive_sum(),
                    CHAIN_COGNITIVE,
                    "{lang:?}: `else if` chain under {depth} plain blocks scored \
                     {} instead of {CHAIN_COGNITIVE}",
                    metrics.cognitive.cognitive_sum(),
                );
            }
        }
    }

    /// A `Stats::default()` that never sees an
    /// observation must not leak the `usize::MAX` sentinel for
    /// `structural_min`. The getter collapses the sentinel to `0.0`
    /// so JSON never emits `1.8446744e19`.
    #[test]
    fn cognitive_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.cognitive_min(), 0);
    }

    #[test]
    fn python_no_cognitive() {
        check_metrics::<PythonParser>("a = 42", "foo.py", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn rust_no_cognitive() {
        check_metrics::<RustParser>("let a = 42;", "foo.rs", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn c_no_cognitive() {
        check_metrics::<CParser>("int a = 42;", "foo.c", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn mozjs_no_cognitive() {
        check_metrics::<MozjsParser>("var a = 42;", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn javascript_no_cognitive() {
        check_metrics::<JavascriptParser>("var a = 42;", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn python_simple_function() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b:  # +2 (+1 and)
                   return 1
                if c and d: # +2 (+1 and)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    /// Python `match`/`case` (PEP 634, 3.10+) opens cognitive nesting
    /// the same way Rust's `match_expression` and the C-family
    /// `switch_statement` do. A 2-arm match with one explicit arm
    /// plus a wildcard contributes one cognitive decision point.
    /// Regression test for #212.
    #[test]
    fn python_match_two_arm_wildcard() {
        check_metrics::<PythonParser>(
            "def f(x):
    match x:
        case 1:
            return 'one'
        case _:
            return 'other'
",
            "foo.py",
            |metric| {
                // The `match_statement` contributes one decision point;
                // case arms inside add no extra nesting (mirrors Rust /
                // C-family switch). cognitive_max = 1.
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_expression_statement() {
        // Boolean expressions containing `And` and `Or` operators were not
        // considered in assignments
        check_metrics::<PythonParser>(
            "def f(a, b):
                c = True and True",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_tuple() {
        // Boolean expressions containing `And` and `Or` operators were not
        // considered inside tuples
        check_metrics::<PythonParser>(
            "def f(a, b):
                return \"%s%s\" % (a and \"Get\" or \"Set\", b)",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_elif_function() {
        // Boolean expressions containing `And` and `Or` operators were not
        // considered in `elif` statements
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b:  # +2 (+1 and)
                   return 1
                elif c and d: # +2 (+1 and)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_more_elifs_function() {
        // Boolean expressions containing `And` and `Or` operators were not
        // considered when there were more `elif` statements
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b:  # +2 (+1 and)
                   return 1
                elif c and d: # +2 (+1 and)
                   return 1
                elif e and f: # +2 (+1 and)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_if_elif_elif_else_chain() {
        // Regression for #274: `if/elif/elif/else` must score as a flat
        // branch chain (each continuation contributes +1 with no extra
        // nesting). `ElifClause` is a dedicated node handled directly
        // by the cognitive dispatch as a branch extension, and the
        // generic `count_specific_ancestors` nesting walk does not
        // include `ElifClause` in its kind sets, so no ancestor-side
        // suppression via `is_else_if` is required.
        // expected: outer if +1, elif +1, elif +1, else +1 = 4.
        check_metrics::<PythonParser>(
            "def f(a, b, c, d):
                if a:
                   return 1
                elif b:
                   return 2
                elif c:
                   return 3
                else:
                   return 4",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_else_if_chain_matches_elif() {
        // Regression for #276: `else: if x:` (no `elif`) is semantically
        // an else-if chain and must score the same as the `elif`
        // equivalent. Before the fix, the inner `if_statement` was
        // double-counted (nesting +2 instead of +1), inflating the
        // cognitive score linearly with chain length.
        // expected: outer if +1, boolean `and` +1, else_clause +1,
        //   inner if suppressed by is_else_if, inner boolean `and` +1
        //   = 4 — matching the `elif` form above (python_elif_function).
        check_metrics::<PythonParser>(
            "def f(a, b, c, d):
                if a and b:
                   return 1
                else:
                   if c and d:
                      return 1",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_try_except_finally_finally_is_free() {
        // Regression for #416: a `finally` clause is structured cleanup that
        // always runs and must add 0 per the SonarSource Cognitive Complexity
        // spec. try/except/finally must score the same as try/except.
        // expected: except +1, finally +0 = 1.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    x = risky()
                except ValueError:
                    x = 1
                finally:
                    cleanup()
                return x",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_try_except_matches_try_except_finally() {
        // Companion to #416: try/except (no finally) scores the same as the
        // try/except/finally form above, proving `finally` is free.
        // expected: except +1 = 1.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    x = risky()
                except ValueError:
                    x = 1
                return x",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_comprehension_matches_explicit_loop() {
        // Regression for #417: a list comprehension's `for`/`if` clauses must
        // carry the same cognitive load as the explicit loop+condition they
        // desugar to. `[x for x in xs if x > 0]` was scoring 0 while the
        // equivalent explicit `for`/`if` scored 3.
        // expected: for_in_clause +1 (nesting 0), if_clause +2 (1 base +
        // 1 nesting under the for) = 3 — equal to the explicit form below.
        check_cognitive_and_cyclomatic::<PythonParser>(
            "def f(xs):
                return [x for x in xs if x > 0]",
            "foo.py",
            |metric| {
                // cyclomatic 4 = unit base 1 + for 1 + if 1 + function base 1.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4);
            },
        );
        check_cognitive_and_cyclomatic::<PythonParser>(
            "def g(xs):
                out = []
                for x in xs:
                    if x > 0:
                        out.append(x)
                return out",
            "foo.py",
            |metric| {
                // The explicit loop+if form the comprehension above desugars
                // to: for +1, nested if +2 = 3 (cognitive), matching f.
                // cyclomatic 4 matches f as well, confirming agreement.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4);
            },
        );
    }

    #[test]
    fn python_comprehension_plain_no_filter() {
        // A comprehension with no `if` filter scores just the loop.
        // expected: for_in_clause +1 = 1.
        check_cognitive_and_cyclomatic::<PythonParser>(
            "def f(xs):
                return [x for x in xs]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                // cyclomatic 3 = unit base 1 + for 1 + function base 1.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3);
            },
        );
    }

    #[test]
    fn python_comprehension_nested_for() {
        // Two `for` clauses are nested loops: the second nests under the
        // first, mirroring explicit nested `for` statements.
        // expected: for #1 +1 (nesting 0), for #2 +2 (1 base + 1 nesting) = 3.
        check_cognitive_and_cyclomatic::<PythonParser>(
            "def f(xs, ys):
                return [a for a in xs for b in ys]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                // cyclomatic 4 = unit base 1 + for 1 + for 1 + function base 1.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4);
            },
        );
    }

    #[test]
    fn python_comprehension_multiple_filters() {
        // Each `if` filter is an independent condition nested under the for.
        // Cognitive penalizes the nesting, so it exceeds cyclomatic here; the
        // two metrics legitimately diverge once filters multiply.
        // expected cognitive: for +1, if #1 +2, if #2 +2 = 5.
        check_cognitive_and_cyclomatic::<PythonParser>(
            "def f(xs):
                return [x for x in xs if a if b]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                // cyclomatic 5 = unit base 1 + for 1 + if 1 + if 1 + fn base 1.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5);
            },
        );
    }

    #[test]
    fn python_comprehension_variants_consistent() {
        // dict / set / generator comprehensions reuse the same for_in_clause /
        // if_clause node kinds as the list form, so all must score identically
        // to `[x for x in xs if x > 0]` (cognitive 3).
        // expected: for +1, if +2 = 3 for each variant.
        for body in [
            "{x: y for x, y in xs if x > 0}",
            "{x for x in xs if x > 0}",
            "(x for x in xs if x > 0)",
        ] {
            check_cognitive_and_cyclomatic::<PythonParser>(
                &format!("def f(xs):\n                return {body}"),
                "foo.py",
                |metric| {
                    assert_eq!(metric.cognitive.cognitive_sum(), 3);
                    // cyclomatic 4 = unit base 1 + for 1 + if 1 + fn base 1,
                    // identical to the list form, for every variant.
                    assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4);
                },
            );
        }
    }

    #[test]
    fn python_comprehension_nested_in_element() {
        // Regression for #421: a comprehension in another comprehension's
        // element position must carry the full nesting of the outer loop+
        // filter, not the shallow depth the #417 sibling write-back left it
        // with (it under-counted at 6). The element is traversed before the
        // outer clauses, so the depth is established on the comprehension node
        // itself, independent of sibling traversal order.
        // expected cognitive: outer for +1 (nesting 0), outer if +2
        // (nesting 1), inner for +3 (nesting 2), inner if +4 (nesting 3) = 10.
        check_metrics::<PythonParser>(
            "def f(xs):
                return [[y for y in x if y] for x in xs if x]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 10);
            },
        );
        // The explicit doubly-nested loop+if form it desugars to: for +1,
        // if +2, for +3, if +4 = 10, matching the comprehension above.
        check_metrics::<PythonParser>(
            "def g(xs):
                out = []
                for x in xs:
                    if x:
                        for y in x:
                            if y:
                                out.append(y)
                return out",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 10);
            },
        );
    }

    #[test]
    fn python_comprehension_three_levels_nested() {
        // Three comprehensions nested through each other's element positions
        // must equal their explicit triply-nested loop+if form at every depth.
        // expected cognitive: for/if pairs at nesting 0..5 =
        // 1+2+3+4+5+6 = 21.
        check_metrics::<PythonParser>(
            "def f(xss):
                return [[[z for z in y if z] for y in x if y] for x in xss if x]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 21);
            },
        );
        check_metrics::<PythonParser>(
            "def g(xss):
                out = []
                for x in xss:
                    if x:
                        for y in x:
                            if y:
                                for z in y:
                                    if z:
                                        out.append(z)
                return out",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 21);
            },
        );
    }

    #[test]
    fn python_generator_in_comprehension_element() {
        // #421 edge case: a generator passed to a call (`sum(...)`) in a
        // comprehension's element still inherits the outer loop+filter depth
        // through the intervening call/argument_list nodes.
        // expected cognitive: outer for +1, outer if +2, inner for +3,
        // inner if +4 = 10.
        check_metrics::<PythonParser>(
            "def f(xs):
                return [sum(y for y in x if y) for x in xs if x]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 10);
            },
        );
        check_metrics::<PythonParser>(
            "def g(xs):
                out = []
                for x in xs:
                    if x:
                        out.append(sum(y for y in x if y))
                return out",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 10);
            },
        );
    }

    #[test]
    fn python_try_finally_no_except_is_free() {
        // #416: try/finally with no except clause scores 0 — neither the try
        // body nor the finally cleanup carries any cognitive cost on its own.
        // expected: 0.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    x = risky()
                finally:
                    cleanup()
                return x",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 0,
                  "value": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_constructs_inside_finally_still_count() {
        // #416 guard: making `finally` free must not make its body invisible.
        // The finally clause itself carries no nesting increment (it never
        // called `increase_nesting`), so an `if` directly inside it is at
        // nesting depth 0 and contributes its +1 base cost.
        // expected: if inside finally = +1.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    x = risky()
                finally:
                    if x:
                        cleanup()",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_simple_function() {
        check_metrics::<RustParser>(
            "fn f() {
                 if a && b { // +2 (+1 &&)
                     println!(\"test\");
                 }
                 if c && d { // +2 (+1 &&)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_simple_function() {
        check_metrics::<CParser>(
            "void f() {
                 if (a && b) { // +2 (+1 &&)
                     printf(\"test\");
                 }
                 if (c && d) { // +2 (+1 &&)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_simple_function() {
        check_metrics::<MozjsParser>(
            "function f() {
                 if (a && b) { // +2 (+1 &&)
                     window.print(\"test\");
                 }
                 if (c && d) { // +2 (+1 &&)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn javascript_simple_function() {
        check_metrics::<JavascriptParser>(
            "function f() {
                 if (a && b) { // +2 (+1 &&)
                     console.log(\"test\");
                 }
                 if (c || d) { // +2 (+1 ||)
                     console.log(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_sequence_same_booleans() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b and True:  # +2 (+1 sequence of and)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_sequence_same_booleans() {
        check_metrics::<RustParser>(
            "fn f() {
                 if a && b && true { // +2 (+1 sequence of &&)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );

        check_metrics::<RustParser>(
            "fn f() {
                 if a || b || c || d { // +2 (+1 sequence of ||)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    // Regression for issue #396: in Rust 2024 let-chains, the `&&`
    // tokens are direct children of the `_let_chain` / `let_chain`
    // node rather than a `BinaryExpression`. Before #396 these
    // tokens were invisible to the cognitive boolean-sequence
    // counter (cyclomatic already counted them via AMPAMP).
    #[test]
    fn rust_let_chain_sequence_booleans() {
        // expected: +1 for the `if`, +1 for the chain of two `&&`
        // tokens (sequence of same operator collapses to one).
        // Equivalent shape to `if a && b && true { ... }` above,
        // which scores 2.0.
        check_metrics::<RustParser>(
            "fn f(a: Option<i32>, b: Option<i32>) {
                 if let Some(x) = a && let Some(y) = b && x > y { // +2 (+1 sequence of &&)
                     println!(\"both\");
                 }
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_let_chain_vs_nested_if_let() {
        // Companion to `rust_let_chain_sequence_booleans`. The nested
        // `if let` form has no `&&` and so is unaffected by the #396
        // LetChain dispatch; this test pins that the pre-existing
        // nesting scoring (+1 outer `if`, +2 nested `if` at nesting=1)
        // still yields 3 and that the LetChain arm did not alter it.
        check_metrics::<RustParser>(
            "fn f(a: Option<i32>, b: Option<i32>) {
                 if let Some(x) = a { // +1
                     if let Some(y) = b { // +2 (nesting=1)
                         println!(\"{} {}\", x, y);
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_sequence_same_booleans() {
        check_metrics::<CParser>(
            "void f() {
                 if (a && b && 1 == 1) { // +2 (+1 sequence of &&)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );

        check_metrics::<CppParser>(
            "void f() {
                 if (a || b || c || d) { // +2 (+1 sequence of ||)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_sequence_same_booleans() {
        check_metrics::<MozjsParser>(
            "function f() {
                 if (a && b && 1 == 1) { // +2 (+1 sequence of &&)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );

        check_metrics::<MozjsParser>(
            "function f() {
                 if (a || b || c || d) { // +2 (+1 sequence of ||)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_not_booleans() {
        check_metrics::<RustParser>(
            "fn f() {
                 if !a && !b { // +2 (+1 &&)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );

        check_metrics::<RustParser>(
            // `!` does not break boolean sequences (issue #392): the
            // outer and inner `&&`s are folded into a single sequence
            // because pre-order visits the outer BinaryExpression first
            // (recording `&&` at its end_byte) and the inner `&&` lies
            // within that span. The `!` arm was dead anyway — it fired
            // after both BinaryExpressions had already been counted.
            "fn f() {
                 if a && !(b && c) { // +2 (+1 if, +1 outer &&; inner && continues)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );

        check_metrics::<RustParser>(
            "fn f() {
                 if !(a || b) && !(c || d) { // +4 (+1 ||, +1 &&, +1 ||)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_not_does_not_affect_boolean_sequence_392() {
        // Regression test for issue #392: `!` does not affect cognitive
        // scoring for a same-operator boolean sequence. `!a && !b && !c`
        // must score identically to `a && b && c` — both are a single
        // `&&` chain under SonarSource's rule B1 (only operator switches
        // start a new sequence). The previously dead `UnaryExpression`
        // arm could not have affected this case either way (pre-order
        // visits the BinaryExpressions before the UnaryExpressions), so
        // this asserts the new and old behaviour agree where it matters.
        // if(+1) + && sequence(+1) = 2; the two trailing `&&`s are
        // continuations because all three share the outer pre-order
        // parent's end_byte.
        check_metrics::<RustParser>(
            "fn f() {
                 if !a && !b && !c {
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
        check_metrics::<RustParser>(
            "fn f() {
                 if a && b && c {
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                // Same sum as the negated form above: `!` is not a
                // boolean-sequence boundary.
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_not_booleans() {
        // `!` does not break boolean sequences (issue #392): the inner
        // `&&` is folded into the outer `&&`'s span because pre-order
        // visits the outer `binary_expression` first.
        check_metrics::<CParser>(
            "void f() {
                 if (a && !(b && c)) { // +2 (+1 if, +1 outer &&; inner && continues)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );

        check_metrics::<CppParser>(
            "void f() {
                 if (!(a || b) && !(c || d)) { // +4 (+1 ||, +1 &&, +1 ||)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_not_booleans() {
        // `!` does not break boolean sequences (issue #392): inner `&&`
        // continues the outer `&&` sequence (pre-order visits the outer
        // BinaryExpression first, so its end_byte already covers the
        // inner one).
        check_metrics::<MozjsParser>(
            "function f() {
                 if (a && !(b && c)) { // +2 (+1 if, +1 outer &&; inner && continues)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );

        check_metrics::<MozjsParser>(
            "function f() {
                 if (!(a || b) && !(c || d)) { // +4 (+1 ||, +1 &&, +1 ||)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_sequence_different_booleans() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b or True:  # +3 (+1 and, +1 or)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_sequence_different_booleans() {
        check_metrics::<RustParser>(
            "fn f() {
                 if a && b || true { // +3 (+1 &&, +1 ||)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_sequence_different_booleans() {
        check_metrics::<CParser>(
            "void f() {
                 if (a && b || 1 == 1) { // +3 (+1 &&, +1 ||)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_sequence_different_booleans() {
        check_metrics::<MozjsParser>(
            "function f() {
                 if (a && b || 1 == 1) { // +3 (+1 &&, +1 ||)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_formatted_sequence_different_booleans() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if (  # +1
                    a and b and  # +1
                    (c or d)  # +1
                ):
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_1_level_nesting() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a:  # +1
                    for i in range(b):  # +2
                        return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_1_level_nesting() {
        check_metrics::<RustParser>(
            "fn f() {
                 if true { // +1
                     if true { // +2 (nesting = 1)
                         println!(\"test\");
                     } else if 1 == 1 { // +1
                         if true { // +3 (nesting = 2)
                             println!(\"test\");
                         }
                     } else { // +1
                         if true { // +3 (nesting = 2)
                             println!(\"test\");
                         }
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 11,
                  "value": 0,
                  "average": 11.0,
                  "min": 0,
                  "max": 11
                }
                "#
                );
            },
        );

        check_metrics::<RustParser>(
            "fn f() {
                 if true { // +1
                     match true { // +2 (nesting = 1)
                         true => println!(\"test\"),
                         false => println!(\"test\"),
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_1_level_nesting() {
        check_metrics::<CParser>(
            "void f() {
                 if (1 == 1) { // +1
                     if (1 == 1) { // +2 (nesting = 1)
                         printf(\"test\");
                     } else if (1 == 1) { // +1
                         if (1 == 1) { // +3 (nesting = 2)
                             printf(\"test\");
                         }
                     } else { // +1
                         if (1 == 1) { // +3 (nesting = 2)
                             printf(\"test\");
                         }
                     }
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 11,
                  "value": 0,
                  "average": 11.0,
                  "min": 0,
                  "max": 11
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_1_level_nesting() {
        check_metrics::<MozjsParser>(
            "function f() {
                 if (1 == 1) { // +1
                     if (1 == 1) { // +2 (nesting = 1)
                         window.print(\"test\");
                     } else if (1 == 1) { // +1
                         if (1 == 1) { // +3 (nesting = 2)
                             window.print(\"test\");
                         }
                     } else { // +1
                         if (1 == 1) { // +3 (nesting = 2)
                             window.print(\"test\");
                         }
                     }
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 11,
                  "value": 0,
                  "average": 11.0,
                  "min": 0,
                  "max": 11
                }
                "#
                );
            },
        );
    }

    #[test]
    fn javascript_nesting() {
        check_metrics::<JavascriptParser>(
            "function f() {
                 if (a) { // +1
                     for (let i = 0; i < 10; i++) { // +2 (nesting = 1)
                         while (b) { // +3 (nesting = 2)
                             console.log(\"test\");
                         }
                     }
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_2_level_nesting() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a:  # +1
                    for i in range(b):  # +2
                        if b:  # +3
                            return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_2_level_nesting() {
        check_metrics::<RustParser>(
            "fn f() {
                 if true { // +1
                     for i in 0..4 { // +2 (nesting = 1)
                         match true { // +3 (nesting = 2)
                             true => println!(\"test\"),
                             false => println!(\"test\"),
                         }
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_try_construct() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                try:
                    for foo in bar:  # +1
                        return a
                except Exception:  # +1
                    if a < 0:  # +2
                        return a",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_flat_try_except() {
        // Regression for #242: flat try/except at function top level
        // must still score +1 for the except clause (no enclosing
        // control-flow nesting). Before the fix this happened to be
        // correct because `stats.nesting` was zero; after the fix the
        // value is the same — `increase_nesting` records nesting=0 and
        // bumps structural by 0+1.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    pass
                except Exception:  # +1
                    pass",
            "foo.py",
            |metric| {
                // expected: only the except clause contributes (+1).
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_except_inside_if() {
        // Regression for #242: try/except nested inside an `if` must
        // apply a nesting penalty to the except clause. Before the
        // fix, the except contributed +1 because `stats.nesting` was
        // stale (0 from the previous `increase_nesting` call on the
        // if). After the fix the except sees nesting=1 and contributes
        // +2.
        check_metrics::<PythonParser>(
            "def f(x):
                if x:  # +1
                    try:
                        pass
                    except Exception:  # +2 (nesting = 1)
                        pass",
            "foo.py",
            |metric| {
                // expected: if (+1) + except inside if (+2) = 3
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_except_inside_for() {
        // Regression for #242: try/except nested inside a `for` must
        // apply the for's nesting penalty to the except clause.
        check_metrics::<PythonParser>(
            "def f(xs):
                for x in xs:  # +1
                    try:
                        pass
                    except Exception:  # +2 (nesting = 1)
                        pass",
            "foo.py",
            |metric| {
                // expected: for (+1) + except inside for (+2) = 3
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_multi_except_inside_if() {
        // Regression for #242: every clause in a multi-except chain
        // nested inside an `if` must reflect the nesting penalty.
        // Before the fix, all three except clauses contributed +1;
        // after the fix each contributes +2 (nesting = 1 from the
        // enclosing if).
        check_metrics::<PythonParser>(
            "def f(x):
                if x:  # +1
                    try:
                        pass
                    except ValueError:    # +2
                        pass
                    except TypeError:     # +2
                        pass
                    except Exception:     # +2
                        pass",
            "foo.py",
            |metric| {
                // expected: if (+1) + 3 * except inside if (+2 each) = 7
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 7);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 7,
                  "value": 0,
                  "average": 7.0,
                  "min": 0,
                  "max": 7
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_try_construct() {
        check_metrics::<MozjsParser>(
            "function asyncOnChannelRedirect(oldChannel, newChannel, flags, callback) {
                 for (const collector of this.collectors) {
                     try {
                         collector._onChannelRedirect(oldChannel, newChannel, flags);
                     } catch (ex) {
                         console.error(
                             \"StackTraceCollector.onChannelRedirect threw an exception\",
                              ex
                         );
                     }
                 }
                 callback.onRedirectVerifyCallback(Cr.NS_OK);
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn javascript_try_construct() {
        check_metrics::<JavascriptParser>(
            "function f() {
                 for (let i = 0; i < 10; i++) { // +1
                     try {
                         doSomething(i);
                     } catch (ex) { // +2 (nesting = 1)
                         if (ex instanceof TypeError) { // +3 (nesting = 2)
                             console.error(\"type error\");
                         }
                     } finally {
                         cleanup();
                     }
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    // The tree-sitter-javascript / -typescript grammars fold both
    // `for...in` and `for...of` into the same `for_in_statement` node
    // (only the keyword token differs). The four regression tests below
    // lock that in across every JS-family parser, so any future grammar
    // bump that splits `for...of` into its own node kind would surface
    // here rather than silently scoring `for...of` loops as 0 cognitive.

    #[test]
    fn javascript_for_of_loop() {
        check_metrics::<JavascriptParser>(
            "function f(xs) {
                 let s = 0;
                 for (const x of xs) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_for_of_loop() {
        check_metrics::<MozjsParser>(
            "function f(xs) {
                 let s = 0;
                 for (const x of xs) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_for_of_loop() {
        check_metrics::<TypescriptParser>(
            "function f(xs: number[]): number {
                 let s = 0;
                 for (const x of xs) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn tsx_for_of_loop() {
        check_metrics::<TsxParser>(
            "function f(xs: number[]): number {
                 let s = 0;
                 for (const x of xs) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_break_continue() {
        // Only labeled break and continue statements are considered
        check_metrics::<RustParser>(
            "fn f() {
                 'tens: for ten in 0..3 { // +1
                     '_units: for unit in 0..=9 { // +2 (nesting = 1)
                         if unit % 2 == 0 { // +3 (nesting = 2)
                             continue;
                         } else if unit == 5 { // +1
                             continue 'tens; // +1
                         } else if unit == 6 { // +1
                             break;
                         } else { // +1
                             break 'tens; // +1
                         }
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 11,
                  "value": 0,
                  "average": 11.0,
                  "min": 0,
                  "max": 11
                }
                "#
                );
            },
        );
    }

    // Regression for #389: Rust's `loop {}` has a dedicated grammar node
    // (LoopExpression) distinct from WhileExpression. The cognitive nesting
    // arm previously matched only For/While/Match, so `loop {}` silently
    // contributed neither a structural +1 nor a nesting bump.
    #[test]
    fn rust_loop_single() {
        check_metrics::<RustParser>(
            "fn f() {
                 loop { // +1
                     if true { // +2 (nesting = 1)
                         break;
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                // expected: loop=+1, nested if=+2 (1 + nesting depth 1) = 3
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    // Regression for #389: nested `loop` blocks must accrue nesting just
    // like nested `while`/`for` would.
    #[test]
    fn rust_loop_nested() {
        check_metrics::<RustParser>(
            "fn f() {
                 loop { // +1
                     loop { // +2 (nesting = 1)
                         if true { // +3 (nesting = 2)
                             break;
                         }
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                // expected: outer loop=+1, inner loop=+2, inner if=+3 = 6
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 6);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_nested_function_resets_nesting_and_adds_depth() {
        // Regression for #696: a method defined on a local struct declared
        // two `if`s deep inside an outer method must reset nesting to 0 and
        // gain a function-depth surcharge — not inherit the enclosing
        // nesting.
        //
        // expected: outer `if` (+1, nesting=0) + inner `if` (+2, nesting=1)
        // + Inner::g's `if` (+1 base + 1 depth = +2, nesting=0, depth=1) = 5.
        // Before the fix, `g` inherited nesting=2 from the two enclosing
        // `if`s, scoring its inner `if` at nesting 2 (+3) for a sum of 6.
        // The two-deep nesting is load-bearing: one level deep, the
        // inherited nesting (1) coincidentally equals the depth bump (1).
        check_metrics::<CppParser>(
            "struct S {
                void outer(bool a) {
                    if (a) {
                        if (a) {
                            struct Inner {
                                void g(bool b) {
                                    if (b) { h(); }
                                }
                            };
                        }
                    }
                }
            };",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
            },
        );
    }

    #[test]
    fn c_goto() {
        check_metrics::<CParser>(
            "void f() {
             OUT: for (int i = 1; i <= max; ++i) { // +1
                      for (int j = 2; j < i; ++j) { // +2 (nesting = 1)
                          if (i % j == 0) { // +3 (nesting = 2)
                              goto OUT; // +1
                          }
                      }
                  }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 7,
                  "value": 0,
                  "average": 7.0,
                  "min": 0,
                  "max": 7
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_switch() {
        check_metrics::<CParser>(
            "void f() {
                 switch (1) { // +1
                     case 1:
                         printf(\"one\");
                         break;
                     case 2:
                         printf(\"two\");
                         break;
                     case 3:
                         printf(\"three\");
                         break;
                     default:
                         printf(\"all\");
                         break;
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_ternary() {
        // Sonar's rule scores the ternary `?:` as +1 (and +nesting), matching
        // the JS/Java/Python/Rust families. The cognitive walker matches the
        // `conditional_expression` node, so the operator participates in nesting
        // like any other conditional construct.
        check_metrics::<CParser>(
            "int f(int a) {
                 if (a) { // +1
                     return a > 0 ? 1 : -1; // +2 (1 + nesting 1)
                 }
                 return a > 0 ? 0 : -1; // +1
             }",
            "foo.c",
            // expected: 1 (if) + 2 (nested ternary, nesting=1) + 1 (top-level
            // ternary) = 4. max is 4 for the only function.
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_try_catch_single() {
        check_metrics::<CppParser>(
            "void f() {
                 try {
                     g();
                 } catch (const std::exception& e) { // +1
                     h();
                 }
             }",
            "foo.cpp",
            |metric| {
                // Single catch clause +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_try_multiple_catches() {
        check_metrics::<CppParser>(
            "void f() {
                 try {
                     g();
                 } catch (const std::runtime_error& e) { // +1
                     h();
                 } catch (const std::logic_error& e) { // +1
                     i();
                 } catch (...) { // +1
                     j();
                 }
             }",
            "foo.cpp",
            |metric| {
                // Three catch clauses, each +1 at nesting 0 → 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_try_catch_in_loop() {
        check_metrics::<CppParser>(
            "void f() {
                 for (int i = 0; i < 10; ++i) { // +1
                     try {
                         g();
                     } catch (const std::exception& e) { // +2 (nesting = 1)
                         h();
                     }
                 }
             }",
            "foo.cpp",
            |metric| {
                // for +1, catch +2 (nesting = 1) → 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_range_based_for() {
        check_metrics::<CppParser>(
            "int sum(const std::vector<int>& v) {
                 int s = 0;
                 for (int x : v) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.cpp",
            |metric| {
                // C++11 range-based `for (auto x : v)` parses as
                // `for_range_loop`; it is a control-flow construct and
                // counts the same as a classic `for_statement` → +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_nested_range_based_for() {
        check_metrics::<CppParser>(
            "void f(const std::vector<std::vector<int>>& vv) {
                 for (const auto& row : vv) { // +1
                     for (int x : row) { // +2 (nesting = 1)
                         g(x);
                     }
                 }
             }",
            "foo.cpp",
            |metric| {
                // Nested range-fors compound by nesting, matching the
                // behaviour of nested classic `for` loops: 1 + 2 = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_nested_for() {
        check_metrics::<CParser>(
            "void f(int n, int m) {
                 for (int i = 0; i < n; ++i) { // +1
                     for (int j = 0; j < m; ++j) { // +2 (nesting = 1)
                         for (int k = 0; k < 4; ++k) { // +3 (nesting = 2)
                             g(i, j, k);
                         }
                     }
                 }
             }",
            "foo.c",
            |metric| {
                // Three nested `for` loops → 1 + 2 + 3 = 6.
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
                assert_eq!(metric.cognitive.cognitive_max(), 6);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_nested_while() {
        check_metrics::<CParser>(
            "void f(int n) {
                 while (n > 0) { // +1
                     while (n % 2 == 0) { // +2 (nesting = 1)
                         n /= 2;
                     }
                     n -= 1;
                 }
             }",
            "foo.c",
            |metric| {
                // Two nested `while` loops → 1 + 2 = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_recursion() {
        // Sonar's rule scores each recursive call to the enclosing function
        // as +1, but the file-level comment in `cognitive.rs` documents that
        // recursion is not tracked for C/C++ because the call graph is only
        // resolvable at run time. The body of `fact` therefore costs only
        // the explicit `if`.
        check_metrics::<CParser>(
            "int fact(int n) {
                 if (n <= 1) { // +1
                     return 1;
                 }
                 return n * fact(n - 1); // recursion: currently not counted
             }",
            "foo.c",
            |metric| {
                // Only the `if` contributes; recursion is a documented gap.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_goto_sibling_jump() {
        check_metrics::<CParser>(
            "void f(int n) {
                 if (n < 0) { // +1
                     goto err; // +1
                 }
                 if (n > 100) { // +1
                     goto err; // +1
                 }
                 return;
             err:
                 abort();
             }",
            "foo.c",
            |metric| {
                // Two `if` (+1 each) and two `goto` (+1 each) at nesting 0
                // (the `goto` cost is flat, not multiplied by nesting) → 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_lambda_inside_function() {
        // Per `increase_nesting`, entering a lambda bumps the effective nesting
        // by one — so an `if` directly inside a top-level lambda is +2 charged
        // to the enclosing function (Cpp lambdas are not split into a separate
        // FuncSpace by `getter.rs`, so the `if` is not double-counted).
        // The lambda *is* counted as a closure by NoM, so the cognitive
        // average is sum / (1 function + 1 closure) = 2 / 2 = 1.0.
        check_metrics::<CppParser>(
            "int f(const std::vector<int>& v) {
                 auto pred = [](int x) {
                     if (x > 0) { // +2 (lambda nesting = 1)
                         return true;
                     }
                     return false;
                 };
                 return std::count_if(v.begin(), v.end(), pred);
             }",
            "foo.cpp",
            |metric| {
                // Single `if` inside lambda at lambda-nesting 1 → +2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    /// The `mozcpp` fork must charge a lambda the same nesting surcharge
    /// as upstream C++ — the same source through `CppParser`
    /// (`cpp_lambda_inside_function` above) scores identically.
    ///
    /// `mozcpp`'s `LambdaExpression` arm had no cognitive test before
    /// this, so the whole arm measured zero-coverage even though the
    /// fork is expected to stay metric-equivalent to `cpp`.
    #[test]
    fn mozcpp_lambda_inside_function() {
        check_metrics::<MozcppParser>(
            "int f(const std::vector<int>& v) {
                 auto pred = [](int x) {
                     if (x > 0) { // +2 (lambda nesting = 1)
                         return true;
                     }
                     return false;
                 };
                 return std::count_if(v.begin(), v.end(), pred);
             }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
            },
        );
    }

    #[test]
    fn c_switch_fall_through() {
        // A `case` without `break` (fall-through) does not add cognitive cost
        // beyond the enclosing `switch` itself: only `switch` is in the match
        // arm. Same accounting as `c_switch` above — switch +1 only.
        check_metrics::<CParser>(
            "void f(int n) {
                 switch (n) { // +1
                     case 1:
                     case 2:
                         g();
                         // fall-through
                     case 3:
                         h();
                         break;
                     default:
                         i();
                         break;
                 }
             }",
            "foo.c",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_switch_in_loop() {
        check_metrics::<CParser>(
            "void f(int n) {
                 for (int i = 0; i < n; ++i) { // +1
                     switch (i % 3) { // +2 (nesting = 1)
                         case 0:
                             a();
                             break;
                         case 1:
                             b();
                             break;
                         default:
                             c();
                             break;
                     }
                 }
             }",
            "foo.c",
            |metric| {
                // for +1, switch +2 (nesting = 1) → 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_macro_expanded_control_flow() {
        // Per the file-level comment in `cognitive.rs`, macro expansion is not
        // tracked for C/C++ — macros are treated as opaque tokens. This is the
        // defensive case: a control-flow-bearing macro contributes nothing on
        // its own; only the explicit `if` in the function body is counted.
        check_metrics::<CParser>(
            "#define CHECK(x) do { if (!(x)) return; } while (0)
             void f(int a, int b) {
                 CHECK(a);              // expansion is opaque: 0
                 if (b < 0) {           // +1
                     return;
                 }
             }",
            "foo.c",
            |metric| {
                // Only the explicit `if` contributes.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_switch() {
        check_metrics::<MozjsParser>(
            "function f() {
                 switch (1) { // +1
                     case 1:
                         window.print(\"one\");
                         break;
                     case 2:
                         window.print(\"two\");
                         break;
                     case 3:
                         window.print(\"three\");
                         break;
                     default:
                         window.print(\"all\");
                         break;
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn javascript_switch() {
        check_metrics::<JavascriptParser>(
            "function f() {
                 switch (x) { // +1
                     case 1:
                         console.log(\"one\");
                         break;
                     case 2:
                         console.log(\"two\");
                         break;
                     default:
                         console.log(\"other\");
                         break;
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_ternary_operator() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 if a % 2:  # +1
                     return 'c' if a else 'd'  # +2
                 return 'a' if a else 'b'  # +1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    /// Cognitive cost of a boolean sequence inside a `lambda`, under
    /// each statement kind that can enclose one.
    ///
    /// This pins the scores, not the stop set.
    /// `python_apply_boolean_operator`'s enclosing-lambda walk stops at
    /// `ExpressionList | IfStatement | ForStatement | WhileStatement`,
    /// and no fixture below discriminates any of those arms: none has a
    /// `lambda` above the stop node, so halting there and running to the
    /// module root give the same count. Do not "strengthen" this test by
    /// asserting on the arms — the one arm that can differ,
    /// `ExpressionList`, is discriminated by
    /// `python_boolean_in_expression_list_under_lambda` (#1090).
    #[test]
    fn python_boolean_in_lambda_scores_under_each_enclosing_statement() {
        use crate::test_support::metrics_verbatim;

        let cognitive_sum = |source: &str| {
            metrics_verbatim(
                crate::LANG::Python,
                source.as_bytes(),
                MetricsOptions::default(),
            )
            .cognitive
            .cognitive_sum()
        };

        // No enclosing branch statement: +1 boolean sequence, +1 for the
        // one enclosing lambda = 2.
        assert_eq!(cognitive_sum("y = (lambda x: x and x)(1)\n"), 2);

        // Each branch statement adds its own +1 nesting on top of that
        // same 2.
        for (label, source) in [
            ("if", "if (lambda x: x and x)(1):\n    pass\n"),
            ("for", "for i in (lambda x: x and x)(1):\n    pass\n"),
            ("while", "while (lambda x: x and x)(1):\n    break\n"),
            (
                "for over a comma list",
                "for i in (lambda x: x and x)(1), 2:\n    pass\n",
            ),
        ] {
            assert_eq!(
                cognitive_sum(source),
                3,
                "{label}: +1 statement nesting, +1 lambda, +1 boolean sequence"
            );
        }

        // A second enclosing lambda adds one more, which is what the
        // enclosing-lambda walk is actually for.
        assert_eq!(
            cognitive_sum("f = lambda a: ((lambda x: x and x)(1), 2)\n"),
            3
        );
    }

    /// The `ExpressionList` arm of `python_apply_boolean_operator`'s
    /// stop set — the only one of its four arms that can change a score.
    ///
    /// Two grammar productions can put an `expression_list` under a
    /// `lambda`: a parenthesised `yield`, and an f-string interpolation
    /// (`_f_expression`). Every other site tree-sitter-python spells
    /// `expression_list` at is either a statement (`return`, `del`,
    /// `raise`, `for … in`) or an assignment right-hand side, and a
    /// lambda body is a single expression, so it can contain none of
    /// them. In both fixtures the `expression_list` sits directly above
    /// the `boolean_operator` and stops the enclosing-lambda walk before
    /// the `lambda` is counted, leaving the +1 boolean sequence alone.
    ///
    /// Measured, not derived: deleting only the `ExpressionList` arm
    /// takes both fixtures from 1 to 2, while the doubly-nested lambda
    /// in `python_boolean_in_lambda_scores_under_each_enclosing_statement`
    /// stays at 3 (#1090). Whether 1 or 2 is the *right* score is a
    /// separate question — this pins current behaviour, and the
    /// per-lambda surcharge itself is under review in #1150.
    #[test]
    fn python_boolean_in_expression_list_under_lambda() {
        use crate::test_support::metrics_verbatim;

        for (route, source) in [
            ("parenthesised yield", "k = lambda q: (yield a and b, c)\n"),
            (
                "f-string interpolation",
                "m = lambda q: f\"{a and b, c}\"\n",
            ),
        ] {
            let metrics = metrics_verbatim(
                crate::LANG::Python,
                source.as_bytes(),
                MetricsOptions::default(),
            );

            assert_eq!(
                metrics.cognitive.cognitive_sum(),
                1,
                "{route}: +1 boolean sequence only — the `expression_list` \
                 stops the enclosing-lambda walk before it reaches the `lambda`"
            );
        }
    }

    #[test]
    fn python_nested_functions_lambdas() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 def foo(a):
                     if a:  # +2 (+1 nesting)
                         return 1
                 # +3 (+1 for boolean sequence +2 for lambda nesting)
                 bar = lambda a: lambda b: b or True or True
                 return bar(foo(a))(a)",
            "foo.py",
            |metric| {
                // 2 functions + 2 lambdas = 4
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 5,
                  "value": 0,
                  "average": 1.25,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    /// #1149: a `def` nested inside a conditional is scored against its
    /// own depth, not the enclosing function's.
    ///
    /// Python was the only language with a syntactic function-definition
    /// node that never reset `nesting.conditional` at the boundary, so
    /// `inner` charged base(1) + inherited-conditional(1) +
    /// function-depth(1) = 3 where every sibling charges base(1) +
    /// function-depth(1) = 2. `python_nested_functions_lambdas` missed it
    /// because its nested `def` sits at function top level, where
    /// `conditional` is already 0.
    ///
    /// The Java companion is the byte-equivalent construct — Java has no
    /// local function, so a method reaches the inside of an `if` only
    /// through a class body declared there — and pins the book's
    /// "byte-equivalent constructs therefore score identically across
    /// languages" claim with a test rather than prose.
    ///
    /// Both fixtures nest the definition **two** conditionals deep, not
    /// one. At one level the Java assertion cannot discriminate: reset +
    /// depth-surcharge and no-reset + no-surcharge both yield 2, so
    /// deleting both lines from `cognitive/java.rs` leaves it green. At
    /// two levels the correct answer stays 2 while an unreset
    /// implementation gives 4 (Python, which also bumps depth) or 3
    /// (depth dropped as well).
    #[test]
    fn python_nested_def_inside_conditional_scores_like_java() {
        fn cognitive_of(space: &FuncSpace, name: &str) -> u64 {
            function_space(space, name).metrics.cognitive.cognitive()
        }

        check_func_space::<PythonParser, _>(
            "def outer(a, b, c):
                 if a:  # +1
                     if b:  # +2 (+1 nesting)
                         def inner(c):
                             if c:  # +1 base, +1 function depth, +0 inherited
                                 return 1
                         return inner",
            "nested.py",
            |space| {
                assert_eq!(cognitive_of(&space, "outer"), 3, "python outer");
                assert_eq!(cognitive_of(&space, "inner"), 2, "python inner");
            },
        );

        check_func_space::<JavaParser, _>(
            "class N {
                 int outer(boolean a, boolean b, boolean c) {
                     if (a) {  // +1
                         if (b) {  // +2 (+1 nesting)
                             class I {
                                 int inner(boolean c) {
                                     if (c) {  // +1 base, +1 function depth
                                         return 1;
                                     }
                                     return 0;
                                 }
                             }
                         }
                     }
                     return 0;
                 }
             }",
            "N.java",
            |space| {
                assert_eq!(cognitive_of(&space, "outer"), 3, "java outer");
                assert_eq!(cognitive_of(&space, "inner"), 2, "java inner");
            },
        );
    }

    #[test]
    fn python_real_function() {
        check_metrics::<PythonParser>(
            "def process_raw_constant(constant, min_word_length):
                 processed_words = []
                 raw_camelcase_words = []
                 for raw_word in re.findall(r'[a-z]+', constant):  # +1
                     word = raw_word.strip()
                         if (  # +2 (+1 if and +1 nesting)
                             len(word) >= min_word_length
                             and not (word.startswith('-') or word.endswith('-')) # +2 operators
                         ):
                             if is_camel_case_word(word):  # +3 (+1 if and +2 nesting)
                                 raw_camelcase_words.append(word)
                             else: # +1 else
                                 processed_words.append(word.lower())
                 return processed_words, raw_camelcase_words",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 9,
                  "value": 0,
                  "average": 9.0,
                  "min": 0,
                  "max": 9
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_if_let_else_if_else() {
        check_metrics::<RustParser>(
            "pub fn create_usage_no_title(p: &Parser, used: &[&str]) -> String {
                 debugln!(\"usage::create_usage_no_title;\");
                 if let Some(u) = p.meta.usage_str { // +1
                     String::from(&*u)
                 } else if used.is_empty() { // +1
                     create_help_usage(p, true)
                 } else { // +1
                     create_smart_usage(p, used)
                }
            }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_if_else_if_else() {
        check_metrics::<TypescriptParser>(
            "function foo() {
                 if (this._closed) return Promise.resolve(); // +1
                 if (this._tempDirectory) { // +1
                     this.kill();
                 } else if (this.connection) { // +1
                     this.kill();
                 } else { // +1
                     throw new Error(`Error`);
                }
                helper.removeEventListeners(this._listeners);
                return this._processClosing;
            }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_no_cognitive() {
        check_metrics::<JavaParser>("int a = 42;", "foo.java", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn java_single_branch_function() {
        check_metrics::<JavaParser>(
            "class X {
                public static void print(boolean a){  
                if(a){ // +1
                  System.out.println(\"test1\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_multiple_branch_function() {
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b){  
                if(a){ // +1
                  System.out.println(\"test1\");
                }
                if(b){ // +1
                  System.out.println(\"test2\");
                }
                else { // +1
                  System.out.println(\"test3\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_compound_conditions() {
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b, boolean c, boolean d){  
                if(a && b){ // +2 (+1 &&)
                  System.out.println(\"test1\");
                }
                if(c && d){ // +2 (+1 &&)
                  System.out.println(\"test2\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_switch_statement() {
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b, boolean c, boolean d){
                switch(expr){ //+1
                  case 1:
                    System.out.println(\"test1\");
                    break;
                  case 2:
                    System.out.println(\"test2\");
                    break;
                  default:
                    System.out.println(\"test\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_switch_expression() {
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b, boolean c, boolean d){
                switch(expr){ // +1
                  case 1 -> System.out.println(\"test1\");
                  case 2 -> System.out.println(\"test2\");
                  default -> System.out.println(\"test\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_not_booleans() {
        // `!` does not break boolean sequences (issue #392): pre-order
        // visits the outer `&&` BinaryExpression first; the inner `&&`
        // lies within that span and is a continuation, not a new
        // sequence.
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b, boolean c, boolean d){
                if (a && !(b && c)) { // +2 (+1 if, +1 outer &&; inner && continues)
                  printf(\"test\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_enhanced_for_statement() {
        check_metrics::<JavaParser>(
            "class X {
              public static int sum(int[] xs) {
                int s = 0;
                for (int x : xs) { // +1
                  s += x;
                }
                return s;
              }
            }",
            "foo.java",
            |metric| {
                // Java's enhanced-for `for (T x : c)` parses as
                // `enhanced_for_statement`; it is a control-flow construct
                // and counts the same as a classic `for_statement` → +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_nested_enhanced_for_statement() {
        check_metrics::<JavaParser>(
            "class X {
              public static void f(int[][] xss) {
                for (int[] xs : xss) { // +1
                  for (int x : xs) { // +2 (nesting = 1)
                    g(x);
                  }
                }
              }
            }",
            "foo.java",
            |metric| {
                // Nested enhanced-fors compound by nesting, matching the
                // behaviour of nested classic `for` loops: 1 + 2 = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_ternary() {
        // Java's ternary `?:` (grammar `ternary_expression`) is a
        // conditional construct: +1 base + nesting, matching the
        // SonarSource Cognitive Complexity §2 rule and the C++/JS
        // siblings.
        check_metrics::<JavaParser>(
            "class X {
              public static boolean check(int a) {
                  return a > 0 ? true : false; // +1
              }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_nested_ternary() {
        // Nested ternaries inside an `if` block compound by nesting,
        // matching the C++ regression test for issue #172.
        // expected: if (+1, nesting=0) + outer ternary (+1+1=+2,
        // nesting=1) + inner ternary (+1+2=+3, nesting=2) = 6.
        check_metrics::<JavaParser>(
            "class X {
              public static String classify(int a, int b) {
                  if (a > 0) { // +1
                      return b > 0 ? (b > 10 ? \"big\" : \"small\") : \"neg\"; // +2, +3
                  }
                  return \"zero\";
              }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
                assert_eq!(metric.cognitive.cognitive_max(), 6);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_nested_method_resets_nesting_and_adds_depth() {
        // Regression for #696: a local-class method declared two `if`s deep
        // inside an outer method must NOT inherit the enclosing nesting. The
        // method-declaration boundary resets nesting to 0 and bumps the
        // function-depth surcharge by 1 (it is nested inside `outer`).
        //
        // expected: outer `if` (+1, nesting=0) + inner `if` (+2, nesting=1)
        // + Local.f's `if` (+1 base + 1 depth = +2, nesting=0, depth=1) = 5.
        // Before the fix, `f` inherited nesting=2 from the two enclosing
        // `if`s, scoring its inner `if` at nesting 2 (+3) for a sum of 6.
        // The two-deep nesting is load-bearing: at one level deep the
        // inherited nesting (1) coincidentally equals the depth bump (1) so
        // the bug is invisible.
        check_metrics::<JavaParser>(
            "class Outer {
                void outer(boolean a) {
                    if (a) {
                        if (a) {
                            class Local {
                                void f(boolean b) {
                                    if (b) { g(); }
                                }
                            }
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
            },
        );
    }

    /// Regression for #1160: a record's compact constructor is its own
    /// grammar kind (`compact_constructor_declaration`), which was absent
    /// from `is_func`, `get_space_kind`, and the boundary arm in
    /// `cognitive/java.rs`. It therefore opened no function space and its
    /// control flow was charged to the enclosing class, so `bca check`
    /// could never flag one however complex it got.
    ///
    /// expected: each `if` is +1 at nesting 0, so `function R` scores 2
    /// while `class R` scores 0 of its own. Both halves are asserted:
    /// checking only the new space would still pass if the class kept a
    /// duplicate count of the same two branches.
    #[test]
    fn java_record_compact_constructor_opens_function_space() {
        check_func_space::<JavaParser, _>(
            "record R(int a, int b) {
                 R {
                     if (a < 0) { throw new IllegalArgumentException(); }
                     if (b < 0) { throw new IllegalArgumentException(); }
                 }
                 int sum() { return a + b; }
             }",
            "R.java",
            |space| {
                let class = child_space(&space, "R");
                assert_eq!(class.kind, SpaceKind::Class, "record opens a class space");
                assert_eq!(class.metrics.cognitive.cognitive(), 0, "class R own score");
                // Also pins the space name: the compact form carries a
                // `name` field holding the record's simple name, so the
                // default `get_func_space_name` reports `R` rather than
                // `<anonymous>`.
                assert_eq!(
                    function_space(&space, "R").metrics.cognitive.cognitive(),
                    2,
                    "compact constructor own score",
                );
            },
        );
    }

    /// The compact constructor is a *function boundary*, not merely a
    /// space: #1160 added it to the arm that resets structural nesting and
    /// to the `stops` set behind the function-depth surcharge. The
    /// reproducer above cannot see either — at nesting 0 with no enclosing
    /// function both lines are no-ops — so each gets a fixture that only
    /// it can satisfy.
    ///
    /// The first nests a *local record* (Java 16+) two conditionals deep,
    /// per the two-level rule in
    /// `java_nested_method_resets_nesting_and_adds_depth`: at one level
    /// the reset and the surcharge cancel out.
    /// expected: outer `if` +1, inner `if` +2, the compact constructor's
    /// `if` +1 base +1 depth (it is lexically inside `m`) = 5. Without the
    /// reset the last `if` inherits nesting 2 and scores +3, for 6.
    ///
    /// The second inverts the nesting: a local class method inside a
    /// compact constructor. Only the `stops` entry makes the constructor
    /// count as `f`'s enclosing function.
    /// expected: `f`'s `if` is +1 base +1 depth = 2. Without the `stops`
    /// entry the surcharge is 0 and it scores 1.
    #[test]
    fn java_record_compact_constructor_is_a_function_boundary() {
        check_func_space::<JavaParser, _>(
            "class C {
                 void m(boolean f) {
                     if (f) {
                         if (f) {
                             record R(int a) {
                                 R {
                                     if (a < 0) { throw new IllegalArgumentException(); }
                                 }
                             }
                         }
                     }
                 }
             }",
            "C.java",
            |space| {
                assert_eq!(
                    space.metrics.cognitive.cognitive_sum(),
                    5,
                    "local record's compact constructor restarts nesting",
                );
                assert_eq!(
                    function_space(&space, "R").metrics.cognitive.cognitive(),
                    2,
                    "compact constructor: +1 base, +1 function depth",
                );
            },
        );

        check_func_space::<JavaParser, _>(
            "record R(int a) {
                 R {
                     class L {
                         void f(boolean b) {
                             if (b) { g(); }
                         }
                     }
                 }
             }",
            "R.java",
            |space| {
                assert_eq!(
                    function_space(&space, "f").metrics.cognitive.cognitive(),
                    2,
                    "a compact constructor is `f`'s enclosing function",
                );
            },
        );
    }

    #[test]
    fn java_labeled_break_continue() {
        // Per SonarSource Cognitive Complexity §B2 (issue #225), labeled
        // `break LABEL` and `continue LABEL` each add +1 because they break
        // structured control flow. Mirrors `go_labeled_break_continue` and
        // `rust_break_continue_labeled`.
        // expected: outer for (+1, nesting=0) + inner for (+2, nesting=1)
        // + if (+3, nesting=2) + continue outer (+1)
        // + if (+3, nesting=2) + break outer (+1) = 11.
        check_metrics::<JavaParser>(
            "class X {
                void scan(int[][] m) {
                    outer:
                    for (int i = 0; i < m.length; i++) {        // +1
                        for (int j = 0; j < m[i].length; j++) {  // +2
                            if (m[i][j] < 0) continue outer;     // +3, +1
                            if (m[i][j] > 100) break outer;      // +3, +1
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 11);
                assert_eq!(metric.cognitive.cognitive_max(), 11);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 11,
                  "value": 0,
                  "average": 11.0,
                  "min": 0,
                  "max": 11
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_unlabeled_break_continue_not_counted() {
        // Negative test for issue #225: plain `break;` / `continue;` are
        // *not* unstructured jumps under SonarSource Cognitive Complexity
        // §B2 and must add 0. Only the surrounding `for` + `if` contribute.
        // expected: for (+1) + if (+2) + if (+2) = 5.
        check_metrics::<JavaParser>(
            "class X {
                void scan(int[] m) {
                    for (int i = 0; i < m.length; i++) {  // +1
                        if (m[i] < 0) continue;            // +2, +0
                        if (m[i] > 100) break;             // +2, +0
                    }
                }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                assert_eq!(metric.cognitive.cognitive_max(), 5);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 5,
                  "value": 0,
                  "average": 5.0,
                  "min": 0,
                  "max": 5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_no_cognitive() {
        check_metrics::<CsharpParser>("int a = 42;", "foo.cs", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn csharp_single_branch_function() {
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(bool a) {
                    if (a) {
                        System.Console.WriteLine(\"test1\");
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // Single `if` at nesting 0 → +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_multiple_branch_function() {
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(bool a, bool b) {
                    if (a) {
                        System.Console.WriteLine(\"test1\");
                    }
                    if (b) {
                        System.Console.WriteLine(\"test2\");
                    } else {
                        System.Console.WriteLine(\"test3\");
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // First `if` +1, second `if` +1, `else` +1 → 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_compound_conditions() {
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(bool a, bool b, bool c, bool d) {
                    if (a && b) {
                        System.Console.WriteLine(\"test1\");
                    }
                    if (c && d) {
                        System.Console.WriteLine(\"test2\");
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // Two ifs (+1 each) + two `&&` (+1 each, fresh chain per if) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_switch_statement() {
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(int expr) {
                    switch (expr) {
                        case 1:
                            System.Console.WriteLine(\"test1\");
                            break;
                        case 2:
                            System.Console.WriteLine(\"test2\");
                            break;
                        default:
                            System.Console.WriteLine(\"test\");
                            break;
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // Single `switch` +1; cases / default do not increment.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_switch_expression() {
        check_metrics::<CsharpParser>(
            "class X {
                public static string Name(int expr) =>
                    expr switch {
                        1 => \"one\",
                        2 => \"two\",
                        _ => \"other\"
                    };
            }",
            "foo.cs",
            |metric| {
                // `switch` expression +1; arms do not increment.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_not_booleans() {
        // `!` does not break boolean sequences (issue #392): pre-order
        // visits the outer `&&` BinaryExpression first, so the inner
        // `&&` lies within its span and is a continuation.
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(bool a, bool b, bool c) {
                    if (a && !(b && c)) {
                        System.Console.WriteLine(\"test\");
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // `if` +1, outer `&&` +1, inner `&&` continues outer span → 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_ternary() {
        // C#'s ternary `?:` (grammar `conditional_expression`) is a
        // conditional construct: +1 base + nesting. Regression test for
        // issue #224.
        check_metrics::<CsharpParser>(
            "class X {
                public static bool Check(int a) {
                    return a > 0 ? true : false; // +1
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_nested_ternary() {
        // Nested ternaries inside an `if` compound by nesting (mirrors
        // the C++ regression test for #172).
        // expected: if (+1) + outer ternary (+2, nesting=1) + inner
        // ternary (+3, nesting=2) = 6.
        check_metrics::<CsharpParser>(
            "class X {
                public static string Classify(int a, int b) {
                    if (a > 0) { // +1
                        return b > 0 ? (b > 10 ? \"big\" : \"small\") : \"neg\"; // +2, +3
                    }
                    return \"zero\";
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
                assert_eq!(metric.cognitive.cognitive_max(), 6);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_local_function_in_if_does_not_inherit_nesting() {
        // Regression for #696 (the acute C# case): a `local_function_statement`
        // declared two `if`s deep must reset nesting to 0 and gain a
        // function-depth surcharge — not inherit `nesting = 2` from the
        // enclosing `if`s. C# has dedicated `LocalFunctionStatement(342)` /
        // `LocalFunctionDeclaration(343)` nodes that previously went
        // unhandled by the cognitive walker.
        //
        // expected: outer `if` (+1, nesting=0) + inner `if` (+2, nesting=1)
        // + Local's `if` (+1 base + 1 depth = +2, nesting=0, depth=1) = 5.
        // Before the fix, `Local` inherited nesting=2, scoring its inner
        // `if` at nesting 2 (+3) for a sum of 6. The two-deep nesting is
        // load-bearing: one level deep, the inherited nesting (1)
        // coincidentally equals the depth bump (1) and the bug is invisible.
        check_metrics::<CsharpParser>(
            "class C {
                void Outer(bool flag) {
                    if (flag) {
                        if (flag) {
                            void Local() {
                                if (flag) {
                                    System.Console.WriteLine(\"x\");
                                }
                            }
                            Local();
                        }
                    }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
            },
        );
    }

    #[test]
    fn csharp_goto_statement() {
        // Per SonarSource Cognitive Complexity §B2 (issue #225), any `goto`
        // is an unstructured jump and adds +1. Mirrors C++'s `GotoStatement`
        // and Go's `GotoStatement` handling.
        // expected: if (+1, nesting=0) + goto neg (+1) = 2.
        check_metrics::<CsharpParser>(
            "class X {
                int Classify(int x) {
                    if (x < 0) goto neg;  // +1, +1
                    return x;
                    neg:
                    return -x;
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_goto_case_and_default() {
        // `goto case` and `goto default` inside a `switch` are also
        // unstructured jumps (+1 each) per SonarSource §B2.
        // expected: switch (+1, nesting=0) + goto case 2 (+1)
        // + goto default (+1) = 3.
        check_metrics::<CsharpParser>(
            "class X {
                int Walk(int x) {
                    switch (x) {  // +1
                        case 1: goto case 2;     // +1
                        case 2: return 2;
                        case 3: goto default;    // +1
                        default: return 0;
                    }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_unlabeled_break_not_counted() {
        // Negative test for issue #225: C#'s grammar does not allow
        // labeled `break`/`continue` (those are syntactically rejected),
        // and plain `break;` / `continue;` are not unstructured jumps under
        // SonarSource §B2 — they must add 0. Only the `for` + `if`
        // contribute.
        // expected: for (+1) + if (+2) = 3.
        check_metrics::<CsharpParser>(
            "class X {
                void Scan(int[] m) {
                    for (int i = 0; i < m.Length; i++) {  // +1
                        if (m[i] < 0) break;               // +2, +0
                    }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_no_cognitive() {
        check_metrics::<PerlParser>("my $a = 42;", "foo.pl", |metric| {
            insta::assert_json_snapshot!(metric.cognitive, @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#);
        });
    }

    #[test]
    fn perl_simple_function() {
        check_metrics::<PerlParser>(
            "sub f {
                return 1;
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 0,
                  "value": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_sequence_same_booleans() {
        check_metrics::<PerlParser>(
            "sub f {
                if ($a && $b && $c) { # +1 if, +1 first &&-chain
                    print 'x';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_sequence_different_booleans() {
        check_metrics::<PerlParser>(
            "sub f {
                if ($a && $b || $c) { # +1 if, +1 &&, +1 ||
                    print 'x';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_compound_short_circuit_assignment_249() {
        // Regression for issue #249: `&&=`, `||=`, `//=` are compound
        // short-circuit assignments (e.g. `$x //= 1` ≡ `$x = $x // 1`)
        // and each carries one boolean-sequence decision. The grammar
        // exposes the operator token inside `binary_expression`, so the
        // existing arm picks them up once `compute_perl_booleans`
        // recognises the three `*EQ` tokens.
        check_metrics::<PerlParser>(
            "sub f {
                 my ($x, $y, $z) = @_;
                 $x ||= 1; # +1 (||=)
                 $y &&= 2; # +1 (&&=)
                 $z //= 3; # +1 (//=)
                 return $x;
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_not_booleans() {
        // `!` does not break boolean sequences (issue #392): pre-order
        // visits the outer `&&` BinaryExpression first, so the inner
        // `&&` lies within its span and is a continuation.
        check_metrics::<PerlParser>(
            "sub f {
                if ($a && !($b && $c)) { # +1 if, +1 outer &&; inner && continues
                    print 'x';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_1_level_nesting() {
        check_metrics::<PerlParser>(
            "sub f {
                for my $i (1..3) { # +1 for
                    if ($i % 2) { # +2 if (nested 1)
                        print $i;
                    }
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_2_level_nesting() {
        check_metrics::<PerlParser>(
            "sub f {
                for my $i (1..3) { # +1 for
                    while ($n > 0) { # +2 while (nested 1)
                        if ($n % 2) { # +3 if (nested 2)
                            $n--;
                        }
                    }
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_break_continue() {
        // Perl's `last`/`next` are loop-control statements; per Sonar's
        // cognitive rule, they do not add complexity in their bare form
        // (the surrounding loop already contributes +1).
        check_metrics::<PerlParser>(
            "sub f {
                while (1) { # +1 while (nesting becomes 1)
                    last if $done; # +2 postfix-if at nesting=1
                    next; # +0 bare loop control
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_if_elsif_else() {
        check_metrics::<PerlParser>(
            "sub f {
                if ($x) { # +1 if
                    print 'a';
                } elsif ($y) { # +1 elsif
                    print 'b';
                } else { # +1 else
                    print 'c';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_function_definition_without_sub_depth() {
        // Regression: FunctionDefinitionWithoutSub must be a stop in
        // increment_function_depth so that a `sub` nested inside a `method`
        // block gets depth=1, making its structural elements cost +2 instead
        // of +1.  `method name { }` (Method::Signatures style) is what
        // tree-sitter-perl parses as function_definition_without_sub.
        check_metrics::<PerlParser>(
            "method outer {
                sub inner {
                    if (1) { } # +2 (depth=1)
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_goto_single_increment() {
        // Regression (#450): `goto LABEL;` parses as `goto_expression`
        // wrapping the anonymous `goto` keyword token. The walker visits
        // both, so matching `Goto | GotoExpression` counted the jump twice
        // (cognitive 2). Matching only `GotoExpression` scores the correct
        // +1.
        check_metrics::<PerlParser>("sub f { goto LABEL; LABEL: return; }", "foo.pl", |metric| {
            // expected: one `goto` jump (§B2) = +1
            assert_eq!(metric.cognitive.cognitive_sum(), 1);
            insta::assert_json_snapshot!(metric.cognitive, @r#"
            {
              "sum": 1,
              "value": 0,
              "average": 1.0,
              "min": 0,
              "max": 1
            }
            "#);
        });
    }

    #[test]
    fn perl_labeled_loop_control() {
        // Regression (#450): the jump target of `last/next/redo LABEL` is
        // carried as an `Identifier` child of `loop_control_statement`
        // (`Label` is the loop-*definition* node `OUTER:`). Gating on
        // `Label` was a dead arm — labeled jumps scored +0. Each labeled
        // form is now +1 (§B2). The bare forms below stay +0.
        check_metrics::<PerlParser>(
            "OUTER: for my $i (@a) { # +1 for
                 last OUTER;  # +1 labeled
                 next OUTER;  # +1 labeled
                 redo OUTER;  # +1 labeled
             }",
            "foo.pl",
            |metric| {
                // expected: +1 for-loop, +1 each labeled last/next/redo = 4
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 4,
                  "value": 4,
                  "average": 4.0,
                  "min": 4,
                  "max": 4
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_bare_loop_control_zero() {
        // Bare `last;` / `next;` / `redo;` have no `Identifier` jump-target
        // child and must stay +0 — only the surrounding loop counts (§B2).
        check_metrics::<PerlParser>(
            "for my $i (@a) { # +1 for
                 last;  # +0
                 next;  # +0
                 redo;  # +0
             }",
            "foo.pl",
            |metric| {
                // expected: only the +1 for-loop; bare jumps add nothing
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1,
                  "value": 1,
                  "average": 1.0,
                  "min": 1,
                  "max": 1
                }
                "#);
            },
        );
    }

    #[test]
    fn tsx_nested_if_for_with_booleans() {
        check_metrics::<TsxParser>(
            "function process(items: number[]) {
                 if (items.length > 0) { // +1
                     for (let i = 0; i < items.length; i++) { // +2 (nesting=1)
                         if (items[i] > 0 && items[i] < 100) { // +3 (nesting=2) +1 (&&)
                             console.log(items[i]);
                         }
                     }
                 }
             }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 7,
                  "value": 0,
                  "average": 7.0,
                  "min": 0,
                  "max": 7
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_nested_if_with_boolean_sequence() {
        check_metrics::<TypescriptParser>(
            "function validate(input: string, strict: boolean): boolean {
                 if (input.length > 0) { // +1
                     if (strict && input.trim() === input) { // +2 (nesting=1) +1 (&&)
                         return true;
                     }
                 }
                 return false;
             }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_try_catch_with_nesting() {
        check_metrics::<TypescriptParser>(
            "function fetchData(url: string): string {
                 try {
                     if (url.length === 0) { // +1
                         throw new Error('empty url');
                     }
                     return url;
                 } catch (e) { // +1
                     if (e instanceof Error) { // +2 (nesting=1)
                         return e.message;
                     }
                     return 'unknown error';
                 }
             }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn kotlin_cognitive_control_flow() {
        check_metrics::<KotlinParser>(
            "fun process(x: Int, y: Int): String {
                if (x > 0) {                // +1
                    for (i in 1..x) {       // +2 (nesting=1)
                        if (i % 2 == 0) {   // +3 (nesting=2)
                            println(i)
                        }
                    }
                } else if (x < 0) {        // +1 (else-if: flat +1 for else, if not counted as else-if)
                    when (y) {              // +2 (nesting=1)
                        1 -> println(\"one\")
                        2 -> println(\"two\")
                        else -> println(\"other\")
                    }
                } else {                    // +1
                    while (y > 0) {         // +2
                        println(y)
                    }
                }
                return if (x > y) \"big\" else \"small\"
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 14,
                  "value": 0,
                  "average": 14.0,
                  "min": 0,
                  "max": 14
                }
                "#
                );
            },
        );
    }

    #[test]
    fn kotlin_no_cognitive() {
        check_metrics::<KotlinParser>("fun main() { val x = 42 }", "foo.kt", |metric| {
            insta::assert_json_snapshot!(metric.cognitive, @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#);
        });
    }

    #[test]
    fn kotlin_simple_if_with_boolean() {
        check_metrics::<KotlinParser>(
            "fun test(a: Boolean, b: Boolean) { if (a && b) { val x = 1 } }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_nesting() {
        check_metrics::<KotlinParser>(
            "fun test(items: List<Int>) {
                if (items.isNotEmpty()) {
                    for (i in items) {
                        if (i > 0) {
                            println(i)
                        }
                    }
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_when_expression() {
        check_metrics::<KotlinParser>(
            "fun test(x: Int) { when { x > 10 -> val a = 1; x > 5 -> val b = 2; else -> val c = 3 } }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_when_else_no_increment() {
        check_metrics::<KotlinParser>(
            "fun test(x: Int) {
                when (x) {
                    1 -> println(\"one\")
                    2 -> println(\"two\")
                    else -> println(\"other\")
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_labeled_break_continue() {
        // Regression (#450): tree-sitter-kotlin-ng has no break/continue
        // jump-statement kind — `break@outer` / `continue@outer` are
        // `labeled_expression` nodes. The Kotlin impl had no arm for them,
        // so labeled jumps scored +0. Each labeled jump is now +1 (§B2);
        // the bare `break` below (a plain identifier) stays +0.
        check_metrics::<KotlinParser>(
            "fun f() {
                 outer@ for (i in 1..10) { // +1 for
                     break@outer     // +1 labeled
                     continue@outer  // +1 labeled
                     break           // +0 bare
                 }
             }",
            "foo.kt",
            |metric| {
                // expected: +1 for-loop, +1 each labeled break/continue = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_labeled_nonjump_expression_not_counted() {
        // Regression (#450 follow-up): tree-sitter-kotlin-ng models ANY
        // labeled expression as `labeled_expression`, not only labeled
        // jumps. The original #450 arm was unconditional, so a labeled
        // non-jump (`lbl@ run { … }`) wrongly scored +1. The arm now gates
        // on the `label` token being the fused jump keyword `break@` /
        // `continue@`; an ordinary `name@` label must contribute +0.
        // Pre-fix this scored 1.0; verified via test-via-revert.
        check_metrics::<KotlinParser>("fun f() { lbl@ run { println(1) } }", "foo.kt", |metric| {
            // expected: labeled non-jump is not a structured-control-flow
            // break, so it adds nothing.
            assert_eq!(metric.cognitive.cognitive_sum(), 0);
        });
    }

    #[test]
    fn kotlin_else_in_if_still_increments() {
        check_metrics::<KotlinParser>(
            "fun test(x: Int) {
                if (x > 0) {
                    println(\"positive\")
                } else {
                    println(\"non-positive\")
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_else_if_chain() {
        check_metrics::<KotlinParser>(
            "fun test(x: Int) {
                if (x > 10) {
                } else if (x > 5) {
                } else if (x > 0) {
                } else {
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_lambda_nesting() {
        check_metrics::<KotlinParser>(
            "fun test() { val f = { if (true) { } } }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_secondary_constructor_depth() {
        // Regression: SecondaryConstructor must be a stop in increment_function_depth so
        // that a local `fun` nested inside it gets depth=1, making its structural elements
        // cost +2 instead of +1.
        check_metrics::<KotlinParser>(
            "class Foo {
                constructor(x: Int) {
                    fun inner(): Boolean {
                        if (x > 0) { return true } // +2 (depth=1)
                        return false
                    }
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }

    #[test]
    fn go_no_cognitive() {
        check_metrics::<GoParser>("package main\nvar x = 42", "foo.go", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn go_simple_function() {
        check_metrics::<GoParser>(
            "package main
            func f(a, b bool) {
                if a && b {    // +1 (if) +1 (&&)
                    return
                }
                if a || b {    // +1 (if) +1 (||)
                    return
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_nesting() {
        check_metrics::<GoParser>(
            "package main
            func f(x int, items []int) {
                if x > 0 {                    // +1 (nesting 0)
                    for _, v := range items {  // +2 (nesting 1)
                        if v > 0 {             // +3 (nesting 2)
                            println(v)
                        }
                    }
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_switch() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) {
                switch x {         // +1 (nesting 0)
                case 1:
                    if x > 0 {     // +2 (nesting 1)
                        println(x)
                    }
                default:
                    println(x)
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_goto() {
        check_metrics::<GoParser>(
            "package main
            func f(n int) {
                if n > 10 {    // +1 (nesting 0)
                    goto end   // +1 (goto)
                }
            end:
                return
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_else_if_chain() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) {
                if x > 0 {           // +1 (nesting 0)
                    println(x)
                } else if x < 0 {    // +1 (else-if)
                    println(-x)
                } else {              // +1 (else)
                    println(0)
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_labeled_break_continue() {
        check_metrics::<GoParser>(
            "package main
            func f() {
            outer:
                for i := 0; i < 3; i++ {       // +1 (nesting 0)
                    for j := 0; j < 3; j++ {    // +2 (nesting 1)
                        if i == j {              // +3 (nesting 2)
                            continue outer       // +1 (labeled continue)
                        }
                    }
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 7,
                  "value": 0,
                  "average": 7.0,
                  "min": 0,
                  "max": 7
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_method_declaration() {
        // Coverage: MethodDeclaration is processed as a function boundary (nesting
        // reset) identically to FunctionDeclaration.  The depth-stop fix from
        // 081f893 (adding MethodDeclaration to increment_function_depth's stop
        // list) cannot be regression-tested with valid Go because method
        // declarations cannot be nested inside other functions or methods.
        check_metrics::<GoParser>(
            "package main
            type T struct{ val int }
            func (t T) positive() bool {
                if t.val > 0 { // +1
                    return true
                }
                return false
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#);
            },
        );
    }

    #[test]
    fn bash_no_cognitive() {
        check_metrics::<BashParser>("a=42", "foo.sh", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn bash_simple_if() {
        check_metrics::<BashParser>(
            "f() {
                 if [ -z \"$1\" ]; then  # +1
                     echo empty
                 fi
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_if_elif_else() {
        check_metrics::<BashParser>(
            "f() {
                 if [ \"$1\" = a ]; then     # +1
                     echo a
                 elif [ \"$1\" = b ]; then   # +1
                     echo b
                 else                         # +1
                     echo other
                 fi
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_nested_loops() {
        check_metrics::<BashParser>(
            "f() {
                 for i in 1 2 3; do            # +1
                     while [ \"$x\" -lt 10 ]; do  # +2 (nested)
                         x=$((x+1))
                     done
                 done
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_until_loop() {
        // `until` parses to `Bash::WhileStatement`; this test pins that
        // assumption so a future grammar bump that adds a dedicated
        // `UntilStatement` variant is caught.
        check_metrics::<BashParser>(
            "f() {
                 until [ -z \"$x\" ]; do  # +1
                     x=$(pop)
                 done
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_case() {
        // `case` adds +1 nesting; case arms do not contribute extra cognitive
        // cost (matching Kotlin's `WhenExpression` treatment).
        check_metrics::<BashParser>(
            "f() {
                 case \"$1\" in       # +1
                     a) echo a ;;
                     b) echo b ;;
                     *) echo other ;;
                 esac
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_arithmetic_ternary_increases_nesting() {
        // Regression for #1268: Bash's only ternary form scored zero
        // cognitive complexity. It nests like the C-family
        // `ConditionalExpression`. Both arithmetic contexts are covered —
        // the `$(( … ))` expansion and the bare `(( … ))` statement.
        check_metrics::<BashParser>(
            "f() {
                 local m=$(( a > b ? a : b ))  # +1 ternary
             }
             g() {
                 (( x = a ? b : c ))  # +1 ternary
             }",
            "foo.sh",
            |metric| {
                // One increment per function, at nesting 0 in each.
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_nested_arithmetic_ternary_charges_the_inner_one_twice() {
        // A ternary inside a ternary is +1 for the outer and +2 for the
        // inner (one increment plus one nesting level), the same charge
        // every nesting construct carries (#1268).
        check_metrics::<BashParser>(
            "h() {
                 local n=$(( a ? b : c ? d : e ))  # +1 outer, +2 inner
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_boolean_sequence() {
        // First if: a chain of `&&` is one boolean increment regardless of
        // length (consecutive same-operator chain). Second if: `&& … ||` is
        // two operator transitions, so two boolean increments.
        check_metrics::<BashParser>(
            "f() {
                 if [[ -n \"$x\" ]] && [[ -n \"$y\" ]] && [[ -n \"$z\" ]]; then
                     # +1 if, +1 boolean (one && chain)
                     echo all
                 fi
                 if [[ -n \"$x\" ]] && [[ -n \"$y\" ]] || [[ -n \"$z\" ]]; then
                     # +1 if, +2 boolean (&& then ||)
                     echo mixed
                 fi
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 5,
                  "value": 0,
                  "average": 5.0,
                  "min": 0,
                  "max": 5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn tcl_no_cognitive() {
        // No proc, no control flow → cognitive complexity is zero everywhere.
        check_metrics::<TclParser>("set x 1", "foo.tcl", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0);
            assert_eq!(metric.cognitive.cognitive_max(), 0);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn tcl_simple_function() {
        // proc with one if and one &&: if(+1) + &&(+1) = 2.
        check_metrics::<TclParser>(
            "proc f {a} {
    if {$a > 0 && $a < 10} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_sequence_same_booleans() {
        // Sequences of the same boolean operator count as a single increment.
        // `$a && $b && $c` → +1 (one && group), not +2.
        check_metrics::<TclParser>(
            "proc f {a b c d} {
    if {$a && $b && $c} {
        puts yes
    }
    if {$a || $b || $c || $d} {
        puts no
    }
}",
            "foo.tcl",
            |metric| {
                // Two ifs (+1 each) + two single-op chains (+1 each) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_sequence_different_booleans() {
        // Switching operator type increments again: `$a && $b || $c` → +2 (one &&, one ||).
        check_metrics::<TclParser>(
            "proc f {a b c} {
    if {$a && $b || $c} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + &&(+1) + ||(+1) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_not_booleans() {
        // `!` does not contribute cognitive cost on its own (issue
        // #392). The single `&&` between the two negations contributes
        // +1, plus +1 for the surrounding `if`.
        check_metrics::<TclParser>(
            "proc f {a b} {
    if {!$a && !$b} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + &&(+1) = 2; the `!` operators do not increment.
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_1_level_nesting() {
        // while(+1) then if at depth 1 (+2) = 3 for the proc.
        check_metrics::<TclParser>(
            "proc f {x} {
    while {$x > 0} {
        if {$x > 10} {
            set x [expr {$x - 1}]
        }
    }
}",
            "foo.tcl",
            |metric| {
                // while(+1) + if at depth 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_2_level_nesting() {
        // while(+1) + foreach at depth 1 (+2) + if at depth 2 (+3) = 6.
        check_metrics::<TclParser>(
            "proc f {x} {
    while {$x > 0} {
        foreach y {1 2 3} {
            if {$y > $x} {
                puts found
            }
        }
    }
}",
            "foo.tcl",
            |metric| {
                // while(+1) + foreach at depth 1 (+2) + if at depth 2 (+3) = 6.
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
                assert_eq!(metric.cognitive.cognitive_max(), 6);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_catch_cognitive() {
        // `catch` is a conditional handler: +1 at nesting 0, then body at nesting 1.
        // Nested if inside catch body: +2 (depth 1).
        check_metrics::<TclParser>(
            "proc f {x} {
    catch {
        if {$x < 0} {
            error negative
        }
    } msg
}",
            "foo.tcl",
            |metric| {
                // catch(+1) + if at depth 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_if_elseif_else() {
        // if(+1) + elseif(+1) + else(+1) = 3; nesting does not increase for elseif/else.
        check_metrics::<TclParser>(
            "proc f {x} {
    if {$x > 10} {
        puts big
    } elseif {$x > 5} {
        puts medium
    } else {
        puts small
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + elseif(+1) + else(+1) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_not_booleans_nested() {
        // `$a && !($b && $c)`: `!` does not break boolean sequences
        // (issue #392); inner `&&` is a continuation of the outer.
        check_metrics::<TclParser>(
            "proc f {a b c} {
    if {$a && !($b && $c)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + outer &&(+1); inner && continues outer's span → 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_not_booleans_double_nested() {
        // `!($a || $b) && !($c || $d)`: the two `||` sub-expressions and
        // the connecting `&&` are at distinct positions with distinct
        // operator tokens, so each starts a new boolean sequence
        // regardless of the `!` wrapping (issue #392). if(+1) + &&(+1)
        // + first ||(+1) + second ||(+1) = 4.
        check_metrics::<TclParser>(
            "proc f {a b c d} {
    if {!($a || $b) && !($c || $d)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + &&(+1) + first || (+1) + second || (+1) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_nested_procedure_cognitive() {
        // Inner proc is at depth=1; its `if` adds +1+1=2 instead of +1+0=1.
        check_metrics::<TclParser>(
            "proc outer {x} {
    proc inner {y} {
        if {$y > 0} {
            puts positive
        }
    }
    inner $x
}",
            "foo.tcl",
            |metric| {
                // Aggregated: inner proc's `if` at depth 1 contributes 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_ternary_cognitive() {
        // Ternary `? :` inside expr is a conditional expression: adds +1+depth.
        // At proc body depth 0: +1. Inside a while (depth 1): +2.
        check_metrics::<TclParser>(
            "proc f {x} {
    set y [expr {$x > 0 ? $x : -$x}]
    while {$y > 10} {
        set y [expr {$y > 5 ? $y - 1 : 0}]
    }
}",
            "foo.tcl",
            |metric| {
                // outer ternary(+1) + while(+1) + inner ternary at depth 1 (+2) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_switch_cognitive() {
        // Tcl `switch` is a generic command, not a dedicated kind. As a
        // switch-like structure it adds +1 plus current nesting once; the arm
        // count and the `default` arm do not add cognitive cost, matching
        // C-family `SwitchStatement` and Bash `case` (issue #467, lesson 11).
        check_metrics::<TclParser>(
            "proc f {x} {
    switch $x {
        1 { puts a }
        2 { puts b }
        default { puts c }
    }
}",
            "foo.tcl",
            |metric| {
                // One switch structure at proc-body nesting 0 → +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
            },
        );
    }

    #[test]
    fn tcl_switch_cognitive_nested() {
        // A `switch` nested inside an outer `switch` arm pays the nesting
        // penalty: outer +1 (nesting 0), inner +1+1 (nesting 1) = 3 (issue #467).
        check_metrics::<TclParser>(
            "proc f {x y} {
    switch $x {
        1 {
            switch $y {
                a { puts p }
                b { puts q }
            }
        }
        2 { puts b }
    }
}",
            "foo.tcl",
            |metric| {
                // outer switch(+1) + inner switch at nesting 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
            },
        );
    }

    #[test]
    fn tcl_switch_split_form_adds_no_cognitive() {
        // The split arm form passes each arm body as its own sibling
        // `braced_word` argument, so `tcl_switch_arm_list` finds no
        // single wrapping arm list and declines the construct (issue
        // #467). Cognitive then treats it as an ordinary command: no
        // structural increment and no nesting for its bodies, which is
        // why the inner `if` here charges 1 rather than 2.
        check_metrics::<TclParser>(
            "proc f {x} {
    switch $x a { puts a } b { if {$x} { puts b } }
}",
            "foo.tcl",
            |metric| {
                // The `switch` adds nothing; the `if` sits at proc-body
                // nesting 0 and charges +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
            },
        );
    }

    #[test]
    fn tcl_for_cognitive() {
        // Tcl `for` is a generic command — the grammar has no `for` rule —
        // so it is detected by leading word (issue #1264): a loop adds +1
        // plus current nesting, matching `while`/`foreach`.
        check_metrics::<TclParser>(
            "proc f {n} {
    for {set i 0} {$i < $n} {incr i} {
        puts $i
    }
}",
            "foo.tcl",
            |metric| {
                // One `for` at proc-body nesting 0 → +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
            },
        );
    }

    #[test]
    fn tcl_for_cognitive_nested() {
        // The `for` also nests its body: constructs inside it pay the
        // nesting penalty the missing loop previously swallowed (#1264).
        check_metrics::<TclParser>(
            "proc f {n} {
    for {set i 0} {$i < $n} {incr i} {
        if {$i == 2} {
            puts $i
        }
    }
}",
            "foo.tcl",
            |metric| {
                // for(+1, nesting 0) + if at nesting 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
            },
        );
    }

    #[test]
    fn tcl_for_cognitive_name_gate() {
        // The detection reads the command's `name` field: a command whose
        // name merely starts with "for" (`format`, with `for`-shaped braced
        // arguments) and a `for` word in argument position (`puts for`) must
        // both stay at zero (issue #1264).
        check_metrics::<TclParser>(
            "proc f {} {
    format {a} {b} {c} {d}
    puts for
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0);
                assert_eq!(metric.cognitive.cognitive_max(), 0);
            },
        );
    }

    #[test]
    fn tcl_irules_for_parity() {
        // iRules models `for` as a dedicated kind counted by the kind
        // dispatch; Tcl detects it by leading word (issue #1264). The same
        // loop must score identically in both — and the iRules figure also
        // pins that its dedicated kind is not double-counted through the
        // Tcl command-name path. One loop at nesting 0 → +1 in each.
        check_metrics::<TclParser>(
            "proc f {} {
    for {set i 0} {$i < 10} {incr i} {
        puts $i
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
            },
        );
        check_metrics::<IrulesParser>(
            "when HTTP_REQUEST {
    for {set i 0} {$i < 10} {incr i} {
        puts $i
    }
}
",
            "foo.irule",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
            },
        );
    }

    #[test]
    fn tcl_try_on_error_cognitive() {
        // Tcl `try`'s `on error` handler is a conditional error path:
        // +1 plus current nesting (issue #1266), matching `catch`;
        // `finally` is unconditional and free.
        check_metrics::<TclParser>(
            "proc f {} {
    try {
        risky
    } on error {msg} {
        puts $msg
    } finally {
        cleanup
    }
}",
            "foo.tcl",
            |metric| {
                // One handler at proc-body nesting 0 → +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
            },
        );
    }

    #[test]
    fn tcl_try_finally_only_cognitive() {
        // A `try` with only a `finally` has no conditional path and must
        // stay at zero (issue #1266, the cross-language `finally`
        // convention of #416).
        check_metrics::<TclParser>(
            "proc f {} {
    try {
        risky
    } finally {
        cleanup
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0);
                assert_eq!(metric.cognitive.cognitive_max(), 0);
            },
        );
    }

    #[test]
    fn tcl_try_handler_nesting_cognitive() {
        // Only the handler body nests (issue #1266): the `try` body and
        // `finally` body run unconditionally and stay at the inherited
        // level, so an `if` inside each of the three blocks is charged
        // differently. This pins the per-child nesting seed — charging
        // `increase_nesting` at the whole `try` node instead would score
        // the try-body and finally-body `if`s +2 each (sum 7).
        check_metrics::<TclParser>(
            "proc f {} {
    try {
        if {1} {
            a
        }
    } on error {msg} {
        if {1} {
            b
        }
    } finally {
        if {1} {
            c
        }
    }
}",
            "foo.tcl",
            |metric| {
                // handler(+1) + try-body if(+1, nesting 0)
                // + handler-body if(+2, nesting 1)
                // + finally-body if(+1, nesting 0) = 5.
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                assert_eq!(metric.cognitive.cognitive_max(), 5);
            },
        );
    }

    #[test]
    fn irules_try_trap_cognitive() {
        // iRules wraps each `try` handler in a dedicated `on_handler` /
        // `trap_handler` node: +1 each plus nesting, and the handler body
        // nests (issue #1266). Before this the handlers opened anonymous
        // function spaces, which reset nesting and hid the construct from
        // the enclosing proc entirely.
        check_metrics::<IrulesParser>(
            "proc f {} {
    try {
        risky
    } on error {msg} {
        puts $msg
    } trap {POSIX} {msg} {
        if {1} {
            puts $msg
        }
    }
}",
            "foo.irule",
            |metric| {
                // on(+1) + trap(+1) + if at nesting 1 inside trap (+2) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
            },
        );
    }

    #[test]
    fn tcl_irules_try_parity() {
        // The same single-handler `try` must score identically in Tcl
        // (flat `on`/`error` tokens under `try`, per-child nesting seed)
        // and iRules (a dedicated `on_handler` wrapper) — issue #1266.
        // handler(+1) + if at nesting 1 inside the handler (+2) = 3.
        let source = "proc f {} {
    try {
        risky
    } on error {msg} {
        if {1} {
            puts $msg
        }
    }
}";
        check_metrics::<TclParser>(source, "foo.tcl", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 3);
            assert_eq!(metric.cognitive.cognitive_max(), 3);
        });
        check_metrics::<IrulesParser>(source, "foo.irule", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 3);
            assert_eq!(metric.cognitive.cognitive_max(), 3);
        });
    }

    /// Drift marker for #1266 (lesson 34 / grammar-dispatch §2): the
    /// iRules `on_handler` / `trap_handler` kinds are emitted **only**
    /// under `try`.
    ///
    /// Statement-level `on <event> { … }` and `trap { … }` inside an event
    /// body are generic `command` nodes at the pinned grammar, which is
    /// what lets Cognitive and Cyclomatic charge those two kinds
    /// unconditionally as catch clauses. A bump that promoted the
    /// statement-level spelling to the same kinds would silently turn
    /// every one of those commands into a decision point — and, since the
    /// checker treats handler kinds as part of the `try` shape, would
    /// re-run the mistake #1266 fixed, where the handlers were read as
    /// `when`-style event handlers and opened function spaces of their
    /// own. Nothing else in the suite would go red.
    #[test]
    fn irules_try_handler_kinds_appear_only_under_try() {
        use std::path::PathBuf;

        use crate::test_support::ast_has_kind_id;

        let path = PathBuf::from("foo.irule");

        let statement_level = "when HTTP_REQUEST {\n\
                               \x20   on scriptbody { log \"x\" }\n\
                               \x20   trap { log \"y\" }\n\
                               }\n";
        let parser = IrulesParser::new(statement_level.as_bytes().to_vec(), &path, None);
        // Non-vacuity: both spellings must really be in this parse, as
        // `command`s whose leading word is the handler keyword. Asserting
        // only the absence below would pass just as well against a source
        // the grammar rejected outright.
        let command_names: Vec<&str> = parser
            .root()
            .preorder()
            .filter(|node| node.kind_id() == Irules::Command as u16)
            .filter_map(|node| {
                node.child_by_field_name("name")
                    .and_then(|name| name.utf8_text(statement_level.as_bytes()))
            })
            .collect();
        assert!(
            command_names.contains(&"on") && command_names.contains(&"trap"),
            "statement-level `on` / `trap` no longer parse as commands: {command_names:?}",
        );
        assert!(
            !ast_has_kind_id(&parser, Irules::OnHandler as u16),
            "statement-level `on` now emits `on_handler`; re-derive the \
             Cognitive / Cyclomatic handler arms and the checker's `try` \
             shape before trusting them",
        );
        assert!(
            !ast_has_kind_id(&parser, Irules::TrapHandler as u16),
            "statement-level `trap` now emits `trap_handler`; re-derive the \
             Cognitive / Cyclomatic handler arms and the checker's `try` \
             shape before trusting them",
        );

        // And the kinds are reachable, so the assertions above discriminate
        // rather than naming variants the grammar never emits at all.
        let under_try = "when HTTP_REQUEST {\n\
                         \x20   try { risky } on error {e} { log $e } trap {POSIX} {e} { log $e }\n\
                         }\n";
        let parser = IrulesParser::new(under_try.as_bytes().to_vec(), &path, None);
        assert!(
            ast_has_kind_id(&parser, Irules::OnHandler as u16),
            "`on error` under `try` no longer emits `on_handler`",
        );
        assert!(
            ast_has_kind_id(&parser, Irules::TrapHandler as u16),
            "`trap` under `try` no longer emits `trap_handler`",
        );
    }

    #[test]
    fn lua_cognitive_no_cognitive() {
        // Top-level local assignment, no control flow → cognitive complexity is zero.
        check_metrics::<LuaParser>("local x = 42", "foo.lua", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0,
              "value": 0,
              "average": 0.0,
              "min": 0,
              "max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn lua_cognitive_simple_function() {
        // Two `if … and …` statements at function scope: each contributes
        // +1 (if) + 1 (and) = 2; total 4.
        check_metrics::<LuaParser>(
            "local function f(a, b, c, d)
    if a and b then
        return 1
    end
    if c and d then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_sequence_same_booleans() {
        // Sequences of the same boolean operator count as a single increment.
        // `a and b and c` → +1 (one and-group), `a or b or c or d` → +1.
        // Plus +1 per `if` ⇒ 4 total.
        check_metrics::<LuaParser>(
            "local function f(a, b, c, d)
    if a and b and c then
        return 1
    end
    if a or b or c or d then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_not_booleans() {
        // `not a and not b`: `not` does not contribute cognitive cost
        // on its own (issue #392); the single `and` between the two
        // negations contributes +1. if(+1) + and(+1) = 2.
        check_metrics::<LuaParser>(
            "local function f(a, b)
    if not a and not b then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_sequence_different_booleans() {
        // Switching operator type increments again: `a and b or c`
        // → if(+1) + and(+1) + or(+1) = 3.
        check_metrics::<LuaParser>(
            "local function f(a, b, c)
    if a and b or c then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_1_level_nesting() {
        // for at depth 0 (+1) + if at depth 1 (+2) = 3.
        check_metrics::<LuaParser>(
            "local function f(t)
    for i = 1, #t do
        if t[i] > 0 then
            return t[i]
        end
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_2_level_nesting() {
        // outer for (+1) + inner for at depth 1 (+2) + if at depth 2 (+3) = 6.
        check_metrics::<LuaParser>(
            "local function f(t)
    for i = 1, #t do
        for j = 1, #t do
            if t[i] > t[j] then
                return t[i]
            end
        end
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_break_continue() {
        // Lua's `break` is always unlabeled (the grammar has no labeled
        // break and no `continue`), so per SonarSource Cognitive Complexity
        // §B2 it adds +0 — issue #435. for(+1) + if at depth 1 (+2) = 3.
        check_metrics::<LuaParser>(
            "local function f(t)
    for i = 1, #t do
        if t[i] < 0 then
            break
        end
    end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_goto_counted() {
        // `goto label` is a genuinely unstructured jump and adds +1 per
        // SonarSource §B2, even though Lua's unlabeled `break` does not
        // (issue #435). Only the `goto` contributes: +1.
        check_metrics::<LuaParser>(
            "local function f()
    ::top::
    goto top
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_elseif_nesting() {
        // Lua-specific: `elseif_statement` is a dedicated grammar node that
        // stays at the same nesting level as the enclosing `if`. Chain:
        // if(+1) + elseif(+1) + elseif(+1) + else(+1) = 4.
        check_metrics::<LuaParser>(
            "local function classify(x)
    if x > 0 then
        return 1
    elseif x < 0 then
        return -1
    elseif x == 0 then
        return 0
    else
        return 0
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_switch_statement() {
        check_metrics::<TypescriptParser>(
            "function describe(x: number): string {
                 switch (x) {   // +1
                     case 1:
                         return 'one';
                     case 2:
                         return 'two';
                     default:
                         return 'other';
                 }
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn typescript_no_cognitive() {
        check_metrics::<TypescriptParser>(
            "function f(a: number, b: number): number {
                 return a + b;
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0);
                assert_eq!(metric.cognitive.cognitive_max(), 0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_no_cognitive() {
        check_metrics::<TsxParser>(
            "function f(a: number, b: number): number {
                 return a + b;
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0);
                assert_eq!(metric.cognitive.cognitive_max(), 0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_simple_if() {
        check_metrics::<TsxParser>(
            "function f(x: number): number {
                 if (x > 0) {  // +1
                     return x;
                 }
                 return 0;
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_boolean_sequence() {
        check_metrics::<TsxParser>(
            "function f(a: boolean, b: boolean, c: boolean): boolean {
                 return a && b && c;  // +1 (&&, sequence)
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_2_level_nesting() {
        check_metrics::<TsxParser>(
            "function f(a: number[], n: number): number {
                 for (let i = 0; i < a.length; i++) {  // +1
                     if (a[i] > n) {  // +2 (nesting=1)
                         return a[i];
                     }
                 }
                 return -1;
             }",
            "foo.tsx",
            |metric| {
                // for(+1) + if at depth 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_else_if_chain() {
        check_metrics::<TsxParser>(
            "function classify(x: number): string {
                 if (x < 0) {         // +1
                     return 'neg';
                 } else if (x === 0) { // +1 (else if = structural, not nesting)
                     return 'zero';
                 } else {              // +1
                     return 'pos';
                 }
             }",
            "foo.tsx",
            |metric| {
                // if(+1) + else-if(+1) + else(+1) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn js_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a *new* sequence (sibling, not nested),
        // so it should score +1, giving a total of 3 (&&, ||, &&).
        // The pre-existing bug stored only (kind_id) and treated the right && as a
        // continuation of the earlier && sequence, incorrectly yielding 2.
        check_metrics::<JavascriptParser>(
            "function f(a, b, c, d) {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn js_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested inside ||, so they form
        // one sequence and only the first should score +1. Total = 2 (||, &&).
        check_metrics::<JavascriptParser>(
            "function f(a, b, c, d) {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn python_sibling_bool_sequences() {
        // Python uses keyword boolean operators (`and`/`or`), routed through a
        // different `T` instantiation of `compute_booleans` than the JS `&&`/`||`
        // tests. Verifies the sibling-detection fix applies across operator kinds.
        // (a and b) or (c and d) — the right-hand `and` is a sibling, not nested.
        // Expected: and_left(+1) + or(+1) + and_right(+1) = 3.
        check_metrics::<PythonParser>(
            "def f(a, b, c, d):
                 return (a and b) or (c and d)  # +1(and) +1(or) +1(and) = 3
             ",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn python_nested_bool_same_op() {
        // a or (b and c and d) — the inner `and` operators are nested inside `or`,
        // forming one sequence. Expected: or(+1) + and(+1) = 2.
        check_metrics::<PythonParser>(
            "def f(a, b, c, d):
                 return a or (b and c and d)  # +1(or) +1(and) = 2
             ",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn perl_sibling_bool_sequences() {
        // Perl uses `compute_perl_booleans` (a separate function supporting five
        // operator kinds including `//`). Verifies the sibling-detection fix also
        // covers that code path.
        // ($a && $b) || ($c && $d) — the right-hand `&&` is a sibling.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<PerlParser>(
            "sub f {
                 my ($a, $b, $c, $d) = @_;
                 return ($a && $b) || ($c && $d);  # +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn perl_nested_bool_same_op() {
        // $a || ($b && $c && $d) — the inner `&&` operators are nested inside `||`,
        // forming one sequence. Exercises the `compute_perl_booleans` continuation
        // guard (the only path distinct from `compute_booleans`).
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<PerlParser>(
            "sub f {
                 my ($a, $b, $c, $d) = @_;
                 return $a || ($b && $c && $d);  # +1(||) +1(&&) = 2
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn rust_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<RustParser>(
            "fn f(a: bool, b: bool, c: bool, d: bool) -> bool {
                 (a && b) || (c && d)  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn rust_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<RustParser>(
            "fn f(a: bool, b: bool, c: bool, d: bool) -> bool {
                 a || (b && c && d)  // +1(||) +1(&&) = 2
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn c_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<CParser>(
            "int f(int a, int b, int c, int d) {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.c",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn c_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<CParser>(
            "int f(int a, int b, int c, int d) {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.c",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn mozjs_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<MozjsParser>(
            "function f(a, b, c, d) {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn mozjs_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<MozjsParser>(
            "function f(a, b, c, d) {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn typescript_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<TypescriptParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean): boolean {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn typescript_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<TypescriptParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean): boolean {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<TsxParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean): boolean {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<TsxParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean): boolean {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn javascript_nullish_coalescing_chain_230() {
        // Regression for issue #230: `??` is a short-circuit operator and
        // must form a boolean sequence. `a ?? b ?? c` is a single chain
        // of identical operators and collapses to a single +1 under
        // Sonar B1 (same rule as `&&` / `||`).
        check_metrics::<JavascriptParser>(
            "function pick(a, b, c) {
                 return a ?? b ?? c; // +1 (chain of ??)
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_nullish_coalescing_with_if_230() {
        // Regression for issue #230: the example from the issue body.
        // Boolean sequences pay a flat +1 (no nesting penalty) per Sonar
        // B1, so the issue body's stated total of 3 was wrong — the
        // correct answer is if(+1) + ?? chain (+1) = 2. Previously the
        // `??` chain was not counted at all (= 1).
        check_metrics::<TypescriptParser>(
            "function risky(x: string | null, fallback: string | null): string {
                 if (x === \"y\") { // +1
                     return x ?? fallback ?? \"unknown\"; // +1 (chain of ??)
                 }
                 return \"no\";
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn tsx_nullish_coalescing_chain_230() {
        // Regression for issue #230: TSX parity with JS/TS for `??`.
        check_metrics::<TsxParser>(
            "function pick(a: number | null, b: number | null, c: number): number {
                 return a ?? b ?? c; // +1 (chain of ??)
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_nullish_coalescing_chain_230() {
        // Regression for issue #230: Mozjs parity with JS for `??`.
        check_metrics::<MozjsParser>(
            "function pick(a, b, c) {
                 return a ?? b ?? c; // +1 (chain of ??)
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_null_coalescing_cognitive_230() {
        // Regression for issue #230: C# `??` must form a boolean sequence
        // just like `&&` / `||`. Boolean sequences pay a flat +1 (no
        // nesting penalty) per Sonar B1.
        // if(+1) + ?? chain (+1) = 2. Previously the `??` chain
        // contributed nothing and the function scored 1.
        check_metrics::<CsharpParser>(
            "class C {
                 string Risky(string x, string fallback) {
                     if (x == \"y\") { // +1
                         return x ?? fallback ?? \"unknown\"; // +1 (chain of ??)
                     }
                     return \"no\";
                 }
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn php_null_coalescing_cognitive_230() {
        // Regression for issue #230: PHP `??` must form a boolean sequence
        // just like `&&` / `||`. Parallels the PHP cyclomatic
        // null-coalescing handling. Boolean sequences pay a flat +1 (no
        // nesting penalty) per Sonar B1.
        // if(+1) + ?? chain (+1) = 2.
        check_metrics::<PhpParser>(
            "<?php
            function risky($x, $fallback) {
                if ($x === \"y\") { // +1
                    return $x ?? $fallback ?? \"unknown\"; // +1 (chain of ??)
                }
                return \"no\";
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    // Companions to `php_null_coalescing_cognitive_230`: the PHP
    // cognitive operator set extends past `&&` / `||` / `??` to include
    // the word-form `and` / `or` / `xor`, mirroring PHP cyclomatic. A
    // chain of identical word-form operators collapses to a single
    // boolean-sequence increment under Sonar B1, the same way `&&` /
    // `||` chains do. Each word-form gets its own test so a regression
    // that drops a single variant (e.g. only `Or`) is still caught.

    #[test]
    fn php_word_form_and_forms_boolean_sequence_230() {
        check_metrics::<PhpParser>(
            "<?php
            function check_and($a, $b, $c, $d) {
                if ($a and $b and $c and $d) { // +1 (if) + 1 (and chain)
                    return true;
                }
                return false;
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
            },
        );
    }

    #[test]
    fn php_word_form_or_forms_boolean_sequence_230() {
        check_metrics::<PhpParser>(
            "<?php
            function check_or($a, $b, $c, $d) {
                if ($a or $b or $c or $d) { // +1 (if) + 1 (or chain)
                    return true;
                }
                return false;
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
            },
        );
    }

    #[test]
    fn php_word_form_xor_forms_boolean_sequence_230() {
        check_metrics::<PhpParser>(
            "<?php
            function check_xor($a, $b, $c, $d) {
                if ($a xor $b xor $c xor $d) { // +1 (if) + 1 (xor chain)
                    return true;
                }
                return false;
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
            },
        );
    }

    #[test]
    fn java_cognitive_else_if_chain() {
        // Regression for #115: else-if chains must not receive a nesting
        // increment for the `if` inside `else if`. Expected breakdown:
        // if(+1) + else(+1) + else(+1) + else(+1) = 4.
        check_metrics::<JavaParser>(
            "class X {
                public static void f(int x) {
                    if (x > 10) {
                    } else if (x > 5) {
                    } else if (x > 0) {
                    } else {
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_cognitive_nested_else_if() {
        // Regression for #115: else-if inside a loop must still respect
        // the loop's nesting for the initial `if`, but the `else if`
        // branch should only pay a flat +1 via the `else` keyword.
        // for(+1) + if at nesting=1(+2) + else(+1) + else(+1) = 5.
        check_metrics::<JavaParser>(
            "class X {
                public static void f(int x) {
                    for (int i = 0; i < x; i++) {
                        if (i > 10) {
                        } else if (i > 5) {
                        } else {
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 5,
                  "value": 0,
                  "average": 5.0,
                  "min": 0,
                  "max": 5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_cognitive_if_inside_else_block_is_not_else_if() {
        // Regression for #115: an `if` whose previous sibling is the block's
        // opening brace (not the `else` keyword) is a nested independent
        // statement, NOT an else-if continuation. It must pay the full
        // nesting penalty.
        // if(+1, nesting=0) + else(+1) + inner if(+2, nesting=1) = 4.
        check_metrics::<JavaParser>(
            "class X {
                public static void f(int a, int c) {
                    if (a > 0) {
                    } else {
                        if (c > 0) {
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<JavaParser>(
            "class X {
                 boolean f(boolean a, boolean b, boolean c, boolean d) {
                     return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
                 }
             }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn java_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<JavaParser>(
            "class X {
                 boolean f(boolean a, boolean b, boolean c, boolean d) {
                     return a || (b && c && d);  // +1(||) +1(&&) = 2
                 }
             }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn groovy_no_cognitive() {
        check_metrics::<GroovyParser>("class A { int x = 42 }", "foo.groovy", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0);
        });
    }

    #[test]
    fn groovy_single_branch_function() {
        check_metrics::<GroovyParser>(
            "void f(int x) {
                if (x > 0) {
                    println(x)
                }
            }",
            "foo.groovy",
            |metric| {
                // if = +1
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_nested_if() {
        check_metrics::<GroovyParser>(
            "void f(int x, int y) {
                if (x > 0) {
                    if (y > 0) {
                        println(x)
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // outer if (+1) + inner if (+2 for nesting depth 1) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
            },
        );
    }

    #[test]
    fn groovy_else_if_chain() {
        // Regression for the #115 / #239 stub pattern: an `else if`
        // chain must NOT receive a nesting increment for the `if`
        // inside `else if`. Without the sibling-`Else` pattern in
        // `Checker::is_else_if`, this would have scored higher.
        check_metrics::<GroovyParser>(
            "class X {
                static void f(int x) {
                    if (x > 10) {
                    } else if (x > 5) {
                    } else if (x > 0) {
                    } else {
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // if(+1) + else(+1) + else(+1) + else(+1) = 4
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
            },
        );
    }

    #[test]
    fn groovy_else_if_chain_lower_than_nested_ifs() {
        // The `else if` chain in `groovy_else_if_chain` MUST score
        // lower than an equivalent depth of nested `if` blocks — this
        // is the inequality the test exists to defend (lesson 10).
        check_metrics::<GroovyParser>(
            "class X {
                static void f(int x) {
                    if (x > 10) {
                        if (x > 5) {
                            if (x > 0) {
                            }
                        }
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // 3 nested `if`s: 1 + 2 + 3 = 6 (each deeper layer
                // pays a higher nesting cost). The chain in
                // `groovy_else_if_chain` produces 4, so this MUST
                // exceed it.
                assert!(metric.cognitive.cognitive_sum() > 4);
            },
        );
    }

    #[test]
    fn groovy_sequence_booleans_same_op() {
        // SonarSource B1: a chain of identical short-circuit ops counts as one.
        check_metrics::<GroovyParser>(
            "void f(boolean a, boolean b, boolean c) {
                if (a && b && c) { println(a) }
            }",
            "foo.groovy",
            |metric| {
                // if (+1) + boolean sequence (+1) = 2
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
            },
        );
    }

    #[test]
    fn groovy_sequence_booleans_mixed_ops() {
        // A `&&` followed by `||` is two distinct sequences = +2.
        check_metrics::<GroovyParser>(
            "void f(boolean a, boolean b, boolean c) {
                if (a && b || c) { println(a) }
            }",
            "foo.groovy",
            |metric| {
                // if (+1) + && (+1) + || (+1) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
            },
        );
    }

    #[test]
    fn groovy_not_operator_negation() {
        // SonarSource: `!` negation flips a boolean sequence's polarity
        // but doesn't add cognitive cost on its own.
        check_metrics::<GroovyParser>(
            "void f(boolean a, boolean b) {
                if (a && !b) { println(a) }
            }",
            "foo.groovy",
            |metric| {
                // if(+1) + && (+1) = 2
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
            },
        );
    }

    #[test]
    fn groovy_for_while_do_loops() {
        check_metrics::<GroovyParser>(
            "void f(int n) {
                for (int i = 0; i < n; i++) {
                    while (i > 0) {
                        i--
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // for(+1) + while inside for(+2) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
            },
        );
    }

    #[test]
    fn groovy_enhanced_for() {
        check_metrics::<GroovyParser>(
            "void f(List items) {
                for (item in items) {
                    println(item)
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_try_catch_nesting() {
        check_metrics::<GroovyParser>(
            "void f() {
                try {
                    risky()
                } catch (Exception e) {
                    handle(e)
                }
            }",
            "foo.groovy",
            |metric| {
                // catch(+1) = 1
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_ternary_expression() {
        check_metrics::<GroovyParser>(
            "void f(int x) {
                def y = (x > 0) ? 1 : 2
            }",
            "foo.groovy",
            |metric| {
                // ternary(+1) = 1
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_elvis_chain_246() {
        // Regression for issue #246: Groovy's Elvis operator `?:` is
        // a short-circuit nullish operator analogous to Kotlin's `?:`
        // (#239) and JS `??`. `a ?: b ?: c` is a single chain of
        // identical operators and collapses to a single +1 under
        // SonarSource Cognitive Complexity B1 — the same rule applied
        // to `&&` / `||`. Closed by swapping the prior amaanq grammar
        // (which mis-parsed Elvis as `ternary_expression` + MISSING
        // identifier) for `dekobon-tree-sitter-groovy`, which models
        // Elvis as a distinct `elvis_expression` node.
        check_metrics::<GroovyParser>(
            "def pick(a, b, c) {
                return a ?: b ?: c // +1 (Elvis chain)
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
            },
        );
    }

    #[test]
    fn groovy_elvis_inside_if_246() {
        // Regression for issue #246: Elvis chain inside an `if` body.
        // Boolean sequences pay a flat +1 (no nesting penalty) per
        // SonarSource B1: if(+1) + Elvis chain(+1) = 2.
        check_metrics::<GroovyParser>(
            "def f(a, b) {
                if (a != null) { // +1
                    return a ?: b ?: 'x' // +1 (Elvis chain)
                }
                return 'no'
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
            },
        );
    }

    #[test]
    fn groovy_labeled_break_continue() {
        // SonarSource B2: labeled break/continue each add +1.
        check_metrics::<GroovyParser>(
            "void f() {
                outer:
                for (int i = 0; i < 10; i++) {
                    inner:
                    for (int j = 0; j < 10; j++) {
                        if (i == j) break outer
                        if (i < j) continue inner
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // for(+1) + for(+2 nested) + if(+3) + break label(+1)
                // + if(+3) + continue label(+1) = 11
                assert_eq!(metric.cognitive.cognitive_sum(), 11);
            },
        );
    }

    #[test]
    fn groovy_multiple_branch_function() {
        // Sibling `if` statements at the same nesting level each
        // contribute +1; an `else` at the same level adds another
        // +1 via the Else arm.
        check_metrics::<GroovyParser>(
            "class X {
                static void print(boolean a, boolean b) {
                    if (a) {
                        println 'test1'
                    }
                    if (b) {
                        println 'test2'
                    } else {
                        println 'test3'
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // if(+1) + if(+1) + else(+1) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
            },
        );
    }

    #[test]
    fn groovy_unlabeled_break_continue_not_counted() {
        // SonarSource B2: plain `break` / `continue` are NOT
        // unstructured jumps and must add 0 — only labeled forms
        // pay the +1. Matches Java's identical fixture.
        check_metrics::<GroovyParser>(
            "class X {
                void scan(int[] m) {
                    for (int i = 0; i < m.length; i++) {
                        if (m[i] < 0) continue
                        if (m[i] > 100) break
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // for(+1) + if(+2) + if(+2) = 5 (break/continue add 0)
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
            },
        );
    }

    #[test]
    fn groovy_cognitive_closure_body_counts_lambda_nesting() {
        // #519: control flow inside a Groovy closure must pay the same
        // lambda-nesting surcharge as Java's `LambdaExpression`, so the
        // byte-equivalent construct scores identically across languages
        // (lesson #11). The byte-for-byte Java equivalent
        // (`list.forEach(item -> { if (a) { while (b) {} } })`) also
        // reports cognitive sum 5.0.
        //
        // Test-via-revert (.claude/rules/testing.md): removing the
        // `Closure => { lambda += 1; }` arm drops this to 3.0 — the
        // missing +2 lambda surcharge on the nested `if`/`while`.
        check_metrics::<GroovyParser>(
            "class X {
                static void f(java.util.List list, boolean a, boolean b) {
                    list.each { if (a) { while (b) {} } }
                }
            }",
            "foo.groovy",
            |metric| {
                // closure(lambda=1) -> if at nesting=1(+2)
                // -> while at nesting=2(+3) = 5
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
            },
        );
    }

    #[test]
    fn groovy_nested_method_resets_nesting_and_adds_depth() {
        // Regression for #696: a local-class method declared two `if`s deep
        // inside an outer method must reset nesting to 0 and gain a
        // function-depth surcharge — not inherit the enclosing nesting.
        //
        // expected: outer `if` (+1, nesting=0) + inner `if` (+2, nesting=1)
        // + Local.f's `if` (+1 base + 1 depth = +2, nesting=0, depth=1) = 5.
        // Before the fix, `f` inherited nesting=2 from the two enclosing
        // `if`s, scoring its inner `if` at nesting 2 (+3) for a sum of 6.
        // The two-deep nesting is load-bearing: one level deep, the
        // inherited nesting (1) coincidentally equals the depth bump (1).
        check_metrics::<GroovyParser>(
            "class Outer {
                void outer(boolean a) {
                    if (a) {
                        if (a) {
                            class Local {
                                void f(boolean b) {
                                    if (b) { g() }
                                }
                            }
                        }
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
            },
        );
    }

    #[test]
    fn groovy_cognitive_top_level_typed_method_parity() {
        // Regression for the upstream grammar defect
        // tree-sitter-groovy#20, fixed in =0.2.2: a top-level method
        // with an explicit return type whose body contained a `;`
        // (e.g. a C-style `for`) misparsed into identifier + call +
        // standalone closure, so it was not recognized as a function and
        // its body brace-block was a `Closure` — which the new lambda arm
        // would have spuriously surcharged. Post-fix the typed form must
        // parse as a real method and score identically to the `def` form.
        let typed = "void f(int n) {
            for (int i = 0; i < n; i++) {
                if (i > 0) { }
            }
        }";
        let untyped = "def f(int n) {
            for (int i = 0; i < n; i++) {
                if (i > 0) { }
            }
        }";
        // for(+1) + if at nesting=1(+2) = 3; no lambda surcharge because
        // the body is a `block`, not a misparsed `Closure`.
        check_metrics::<GroovyParser>(typed, "foo.groovy", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 3);
        });
        check_metrics::<GroovyParser>(untyped, "foo.groovy", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 3);
        });
    }

    #[test]
    fn groovy_cognitive_nested_else_if() {
        // Regression for the #115 stub pattern at deeper nesting:
        // an `else if` chain inside a `for` loop must still respect
        // the loop's nesting for the initial `if`, but each
        // `else`-chained branch pays a flat +1 via the Else arm.
        // Matches Java's identical fixture.
        check_metrics::<GroovyParser>(
            "class X {
                static void f(int x) {
                    for (int i = 0; i < x; i++) {
                        if (i > 10) {
                        } else if (i > 5) {
                        } else {
                        }
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // for(+1) + if at nesting=1(+2) + else(+1) + else(+1) = 5
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
            },
        );
    }

    #[test]
    fn groovy_cognitive_if_inside_else_block_is_not_else_if() {
        // Regression for #115 — an inner `if` whose previous sibling
        // is the block's opening brace (not the `else` keyword) is a
        // nested independent statement, NOT an else-if continuation,
        // so it pays the full nesting penalty. Matches Java's
        // identical fixture.
        check_metrics::<GroovyParser>(
            "class X {
                static void f(int a, int c) {
                    if (a > 0) {
                    } else {
                        if (c > 0) {
                        }
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // if(+1, nesting=0) + else(+1) + inner if(+2, nesting=1) = 4
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
            },
        );
    }

    #[test]
    fn groovy_nested_ternary() {
        // Nested ternaries inside an `if` compound by nesting — same
        // rule as Java's `java_nested_ternary` (which itself mirrors
        // the C++ regression for #172).
        check_metrics::<GroovyParser>(
            "class X {
                static String classify(int a, int b) {
                    if (a > 0) {
                        return b > 0 ? (b > 10 ? 'big' : 'small') : 'neg'
                    }
                    return 'zero'
                }
            }",
            "foo.groovy",
            |metric| {
                // if(+1, nesting=0) + outer ternary(+1+1=+2, nesting=1)
                // + inner ternary(+1+2=+3, nesting=2) = 6
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
            },
        );
    }

    #[test]
    fn csharp_cognitive_else_if_chain() {
        // Regression for #115: else-if chains must not receive a nesting
        // increment for the `if` inside `else if`. Expected breakdown:
        // if(+1) + else(+1) + else(+1) + else(+1) = 4.
        check_metrics::<CsharpParser>(
            "class X {
                public static void F(int x) {
                    if (x > 10) {
                    } else if (x > 5) {
                    } else if (x > 0) {
                    } else {
                    }
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_cognitive_nested_else_if() {
        // Regression for #115: else-if inside a loop must still respect
        // the loop's nesting for the initial `if`, but the `else if`
        // branch should only pay a flat +1 via the `else` keyword.
        // for(+1) + if at nesting=1(+2) + else(+1) + else(+1) = 5.
        check_metrics::<CsharpParser>(
            "class X {
                public static void F(int x) {
                    for (int i = 0; i < x; i++) {
                        if (i > 10) {
                        } else if (i > 5) {
                        } else {
                        }
                    }
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 5,
                  "value": 0,
                  "average": 5.0,
                  "min": 0,
                  "max": 5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_cognitive_if_inside_else_block_is_not_else_if() {
        // Regression for #115: an `if` whose previous sibling is the block's
        // opening brace (not the `else` keyword) is a nested independent
        // statement, NOT an else-if continuation. It must pay the full
        // nesting penalty.
        // if(+1, nesting=0) + else(+1) + inner if(+2, nesting=1) = 4.
        check_metrics::<CsharpParser>(
            "class X {
                public static void F(int a, int c) {
                    if (a > 0) {
                    } else {
                        if (c > 0) {
                        }
                    }
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 4,
                  "value": 0,
                  "average": 4.0,
                  "min": 0,
                  "max": 4
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<CsharpParser>(
            "class X {
                bool F(bool a, bool b, bool c, bool d) {
                    return (a && b) || (c && d);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<CsharpParser>(
            "class X {
                bool F(bool a, bool b, bool c, bool d) {
                    return a || (b && c && d);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn kotlin_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<KotlinParser>(
            "fun f(a: Boolean, b: Boolean, c: Boolean, d: Boolean) =
                 (a && b) || (c && d)  // +1(&&) +1(||) +1(&&) = 3",
            "foo.kt",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn kotlin_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<KotlinParser>(
            "fun f(a: Boolean, b: Boolean, c: Boolean, d: Boolean) =
                 a || (b && c && d)  // +1(||) +1(&&) = 2",
            "foo.kt",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn kotlin_elvis_chain_239() {
        // Regression for issue #239: Kotlin's Elvis operator `?:` is a
        // short-circuit nullish operator analogous to JS `??` and must
        // form a boolean sequence. `a ?: b ?: c` is a single chain of
        // identical operators and collapses to a single +1 under Sonar
        // B1 (same rule as `&&` / `||`). Previously the Elvis chain was
        // not counted at all (= 0).
        check_metrics::<KotlinParser>(
            "fun pick(a: String?, b: String?, c: String): String = a ?: b ?: c // +1 (Elvis chain)",
            "foo.kt",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn kotlin_elvis_inside_if_239() {
        // Regression for issue #239: Elvis chain inside an `if` body.
        // Boolean sequences pay a flat +1 (no nesting penalty) per
        // Sonar B1: if(+1) + ?: chain(+1) = 2. Previously the Elvis
        // chain was not counted at all and the function scored 1.
        check_metrics::<KotlinParser>(
            "fun f(a: String?, b: String?): String {
                 if (a != null) { // +1
                     return a ?: b ?: \"x\" // +1 (Elvis chain)
                 }
                 return \"no\"
             }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<GoParser>(
            "package main
            func f(a, b, c, d bool) bool {
                return (a && b) || (c && d)  // +1(&&) +1(||) +1(&&) = 3
            }",
            "foo.go",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn go_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<GoParser>(
            "package main
            func f(a, b, c, d bool) bool {
                return a || (b && c && d)  // +1(||) +1(&&) = 2
            }",
            "foo.go",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_sibling_bool_sequences() {
        // ($a && $b) || ($c && $d) — the right-hand && is a sibling, not nested.
        // Expected: if(+1) + ||(+1) + &&(+1) + &&(+1) = 4.
        check_metrics::<TclParser>(
            "proc f {a b c d} {
    if {($a && $b) || ($c && $d)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_nested_bool_same_op() {
        // $a || ($b && $c && $d) — the inner && operators are nested, one sequence.
        // Expected: if(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<TclParser>(
            "proc f {a b c d} {
    if {$a || ($b && $c && $d)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn lua_sibling_bool_sequences() {
        // (a and b) or (c and d) — the right-hand `and` is a sibling, not nested.
        // Expected: if(+1) + or(+1) + and(+1) + and(+1) = 4.
        check_metrics::<LuaParser>(
            "local function f(a, b, c, d)
    if (a and b) or (c and d) then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn lua_nested_bool_same_op() {
        // a or (b and c and d) — the inner `and` operators are nested, one sequence.
        // Expected: if(+1) + or(+1) + and(+1) = 3.
        check_metrics::<LuaParser>(
            "local function f(a, b, c, d)
    if a or (b and c and d) then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn bash_sibling_bool_sequences() {
        // [[ a ]] && [[ b ]] || [[ c ]] && [[ d ]] — bash is left-associative so this
        // parses as ((a&&b)||c)&&d with three distinct operator-type transitions.
        // Expected: if(+1) + &&(+1) + ||(+1) + &&(+1) = 4.
        check_metrics::<BashParser>(
            "f() {
                 if [[ -n \"$a\" ]] && [[ -n \"$b\" ]] || [[ -n \"$c\" ]] && [[ -n \"$d\" ]]; then
                     echo test
                 fi
             }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                assert_eq!(metric.cognitive.cognitive_max(), 4);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn bash_nested_bool_same_op() {
        // [[ a ]] || [[ b ]] && [[ c ]] && [[ d ]] — bash left-associativity gives
        // ((a||b)&&c)&&d: the two && operators are parent/child so the second is
        // a continuation (no extra increment).
        // Expected: if(+1) + &&(+1, outer chain) + ||(+1) = 3.
        check_metrics::<BashParser>(
            "f() {
                 if [[ -n \"$a\" ]] || [[ -n \"$b\" ]] && [[ -n \"$c\" ]] && [[ -n \"$d\" ]]; then
                     echo test
                 fi
             }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_no_cognitive() {
        check_metrics::<PhpParser>("<?php $a = 42;", "foo.php", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0);
            assert_eq!(metric.cognitive.cognitive_max(), 0);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn php_simple_function() {
        // Single `if` inside a function: +1.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a): void {
                if ($a) {
                    echo 'hi';
                }
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_nested_function_resets_nesting_775() {
        // Regression for #775 (the #696 gap): a PHP named function defined
        // inside control flow must reset structural nesting to 0 at the
        // definition boundary and pick up the +1 function-depth surcharge,
        // exactly like Java/C/Rust/etc. Before the fix, `inner` inherited
        // `outer`'s leaked nesting (2 by the time the definition is reached)
        // and scored its body against it: `inner` was 7 (and the file 10).
        //
        // After the fix, inside `inner` nesting resets to 0 and depth = 1
        // (it is nested in `outer`), so:
        //   inner `if ($b)`: structural += (nesting 0 + depth 1) + 1 = 2
        //   inner `if ($d)`: structural += (nesting 1 + depth 1) + 1 = 3
        //   => inner = 5
        // `outer` itself (excluding the nested space) is:
        //   `if ($a)`: +1 (nesting 0→1); `if ($c)`: +2 (nesting 1→2) => 3
        // so the file sum is 3 + 5 = 8, max = 5 (the `inner` space).
        check_metrics::<PhpParser>(
            "<?php
            function outer() {
                if ($a) {
                    if ($c) {
                        function inner() {
                            if ($b) {
                                if ($d) {
                                    echo 'x';
                                }
                            }
                        }
                    }
                }
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 8);
                assert_eq!(metric.cognitive.cognitive_max(), 5);
            },
        );
    }

    #[test]
    fn php_top_level_function_unchanged_775() {
        // Regression guard paired with `php_nested_function_resets_nesting_775`:
        // a *top-level* PHP function with the same body is unaffected by the
        // #775 fix — nesting is already 0 and depth is 0 there. The two
        // `if` statements score +1 and +2 respectively, so cognitive = 3.
        // If the #775 boundary arm ever over-fires on top-level functions,
        // this value moves and the test fails.
        check_metrics::<PhpParser>(
            "<?php
            function inner() {
                if ($b) {
                    if ($d) {
                        echo 'x';
                    }
                }
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
            },
        );
    }

    #[test]
    fn php_if_elseif_else() {
        // PHP exposes `elseif` as a dedicated `else_if_clause` node, scored
        // as a branch extension (+1, no nesting) via the `ElseIfClause` arm
        // — parallel to bash/perl/ruby. An `if … elseif … else` chain is
        // therefore +1 each = 3. PHP previously had no cognitive test for
        // the `elseif` dispatch; this pins it.
        //
        // Note: the one-word `elseif` parses as its own `else_if_clause`
        // node, dispatched directly to the branch-extension arm, so
        // `is_else_if` is never consulted on this path. PHP's *two-word*
        // `else if` is the nested-`if` shape that C++/JS/Java have, and it
        // does go through the `IfStatement if !Self::is_else_if` guard —
        // see `php_two_word_else_if_529` (#529).
        check_metrics::<PhpParser>(
            "<?php
            function f(int $a): void {
                if ($a > 0) {        // +1
                    echo 'pos';
                } elseif ($a < 0) {  // +1
                    echo 'neg';
                } else {             // +1
                    echo 'zero';
                }
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn php_two_word_else_if_529() {
        // PHP's two-word `else if` parses as an `else_clause` wrapping a
        // nested `if_statement` (`else_clause → if_statement`), unlike the
        // one-word `elseif` keyword which is a dedicated `else_if_clause`
        // node. Before #529 the nested `IfStatement` fell through PHP's
        // unguarded cognitive `IfStatement` arm: it fired `increase_nesting`
        // (+1, plus an inflated nesting level for later arms) on top of the
        // wrapping `else_clause`'s branch extension (+1), so the chain below
        // scored 5 instead of the correct 3 — and worse for deeper chains.
        //
        // Correct SonarSource value: `if` +1, `else if` +1 branch
        // extension, `else` +1 = 3. The fix adds the
        // `IfStatement if !Self::is_else_if(node)` guard and teaches PHP's
        // `is_else_if` to recognize the `else_clause → if_statement` shape.
        // This test guards both halves: it scores identically to the
        // one-word `php_if_elseif_else` form. Verified by revert — against
        // pre-#529 code it asserts 5 and fails.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $a): void {
                if ($a == 1) {        // +1
                    echo 'one';
                } else if ($a == 2) { // +1 branch extension, no nesting
                    echo 'two';
                } else {              // +1 branch extension
                    echo 'zero';
                }
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn php_two_word_else_if_chain_nesting_529() {
        // A genuinely nested `if` inside a two-word `else if` arm must still
        // pay its nesting penalty — the #529 guard suppresses only the
        // else-if-continuation `IfStatement`, not real nesting. Here the
        // inner `if ($a > 0)` sits one level deep inside the `else if` arm:
        // `if` +1, `else if` +1, inner `if` +2 (base + nesting), final
        // `else` +1 = 5. Pre-#529 the misattributed nesting inflated this
        // super-linearly; this pins the corrected total and confirms the
        // guard does not over-suppress real nesting.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $a): void {
                if ($a == 1) {         // +1
                    echo 'one';
                } else if ($a == 2) {  // +1
                    if ($a > 0) {      // +2 (base + nesting)
                        echo 'pos';
                    }
                } else {               // +1
                    echo 'zero';
                }
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                assert_eq!(metric.cognitive.cognitive_max(), 5);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 5,
                  "value": 0,
                  "average": 5.0,
                  "min": 0,
                  "max": 5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn php_alternative_syntax_elseif_529() {
        // PHP's alternative (colon) syntax `if …: … elseif …: … else: …
        // endif;` requires the one-word `elseif` keyword — two-word
        // `else if` is a PHP fatal parse error there (the grammar emits an
        // `ERROR` node). The valid one-word form parses as the dedicated
        // `else_if_clause` node, scored as a branch extension (+1, no
        // nesting) just like the brace form. Discovered while fixing #529:
        // pins that the colon-syntax `elseif` chain scores 3, the same as
        // the brace `php_if_elseif_else` form, and guards against a future
        // change that mishandles the alternative-syntax dispatch.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $a): void {
                if ($a > 0):        // +1
                    echo 'pos';
                elseif ($a < 0):    // +1 branch extension, no nesting
                    echo 'neg';
                else:               // +1 branch extension
                    echo 'zero';
                endif;
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn php_ternary() {
        // PHP's ternary `?:` (grammar `conditional_expression`) is a
        // conditional construct: +1 base + nesting. Regression test for
        // issue #224. Note: this differs from PHP's
        // `match_conditional_expression` (the `match` expression),
        // which is handled separately by `MatchExpression`.
        check_metrics::<PhpParser>(
            "<?php
            function check(int $a): bool {
                return $a > 0 ? true : false; // +1
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn php_nested_ternary() {
        // Nested ternaries inside an `if` compound by nesting (mirrors
        // the C++ regression test for #172).
        // expected: if (+1) + outer ternary (+2, nesting=1) + inner
        // ternary (+3, nesting=2) = 6.
        check_metrics::<PhpParser>(
            "<?php
            function classify(int $a, int $b): string {
                if ($a > 0) { // +1
                    return $b > 0 ? ($b > 10 ? 'big' : 'small') : 'neg'; // +2, +3
                }
                return 'zero';
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
                assert_eq!(metric.cognitive.cognitive_max(), 6);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 6,
                  "value": 0,
                  "average": 6.0,
                  "min": 0,
                  "max": 6
                }
                "#
                );
            },
        );
    }

    #[test]
    fn php_sequence_same_booleans() {
        // Sequence of same-operator booleans collapses: a chain of `&&`
        // counts as +1 total, not per-operand.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a, bool $b, bool $c): bool {
                return $a && $b && $c;
            }",
            "foo.php",
            |metric| {
                // Chain of identical && collapses to a single +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_sequence_different_booleans() {
        // Mix of `&&` and `||` — each operator switch costs +1.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a, bool $b, bool $c): bool {
                return $a && $b || $c;
            }",
            "foo.php",
            |metric| {
                // && chain (+1) + switch to || (+1) = 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_not_booleans() {
        // `!` does not break boolean sequences (issue #392): pre-order
        // visits the outer `&&` BinaryExpression first, so the inner
        // `&&` lies within its span and is a continuation.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a, bool $b, bool $c): bool {
                return $a && !($b && $c);
            }",
            "foo.php",
            |metric| {
                // Outer && (+1); inner && continues outer's span → 1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_1_level_nesting() {
        // if-inside-loop: outer for (+1) + inner if at depth 1 (+2) = +3.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                for ($i = 0; $i < $n; $i++) {
                    if ($i % 2 === 0) {
                        return $i;
                    }
                }
                return -1;
            }",
            "foo.php",
            |metric| {
                // for(+1) + if at depth 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_2_level_nesting() {
        // for + while + if = +1 +2 +3 = +6.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                for ($i = 0; $i < $n; $i++) {
                    while ($i > 0) {
                        if ($i % 2 === 0) {
                            return $i;
                        }
                    }
                }
                return -1;
            }",
            "foo.php",
            |metric| {
                // for(+1) + while at depth 1 (+2) + if at depth 2 (+3) = 6.
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
                assert_eq!(metric.cognitive.cognitive_max(), 6);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_break_continue() {
        // PHP `break` and `continue` are not cognitive drivers in this
        // impl; only the surrounding loops count.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                for ($i = 0; $i < $n; $i++) {
                    if ($i % 2 === 0) {
                        continue;
                    }
                    if ($i > 100) {
                        break;
                    }
                }
                return 0;
            }",
            "foo.php",
            |metric| {
                // for(+1) + first if at depth 1 (+2) + second if at depth 1 (+2) = 5.
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                assert_eq!(metric.cognitive.cognitive_max(), 5);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_goto_counted() {
        // `goto label;` is a genuinely unstructured jump and adds +1 per
        // SonarSource Cognitive Complexity §B2 (issue #435), matching
        // C++/C#/Go/Perl/Lua goto handling.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                if ($n < 0) {
                    goto done;
                }
                done:
                return 0;
            }",
            "foo.php",
            |metric| {
                // if(+1) + goto(+1) = 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_numeric_break_not_counted() {
        // PHP has no labeled break/continue; only the numeric level form
        // `break N;` / `continue N;`, which exits N enclosing loops already
        // accounted for by nesting. Per issue #435 the numeric form is a
        // structured loop-level exit and adds +0.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                for ($i = 0; $i < $n; $i++) {
                    while (true) {
                        if ($i > 100) {
                            break 2;
                        }
                    }
                }
                return 0;
            }",
            "foo.php",
            |metric| {
                // for(+1) + while at depth 1 (+2) + if at depth 2 (+3) = 6;
                // `break 2` adds +0.
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
                assert_eq!(metric.cognitive.cognitive_max(), 6);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // ----- Elixir -----

    // No control flow → cognitive complexity is 0.
    #[test]
    fn elixir_empty_function() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    x\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 0,
                  "value": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#
                );
            },
        );
    }

    // `if cond do … end`: single-branch construct → +1 nesting at depth
    // 0 inside `def` body → cognitive 1.
    #[test]
    fn elixir_simple_if() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    if x > 0 do\n      :pos\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `if cond do … else … end`: +1 nesting for `if`, +1 for `else` token
    // (matches Java/Kotlin) → cognitive 2.
    #[test]
    fn elixir_if_else() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    if x > 0 do\n      :pos\n    else\n      :neg\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: if (+1) + else (+1) = 2
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `case x do … end` with three arms: only the container Call earns
    // a nesting bump (matches Java's `SwitchBlock` rule). Individual
    // `stab_clause` arms add no extra cost. Expected cognitive 1.
    #[test]
    fn elixir_case_arms_count_once() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    case x do\n      1 -> :one\n      2 -> :two\n      _ -> :other\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: case +1 (one nesting bump on the container)
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `cond do … end` is structurally identical to `case` for our
    // purposes: container Call earns +1 nesting; arms add nothing.
    #[test]
    fn elixir_cond_counts_once() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    cond do\n      x > 0 -> :pos\n      x < 0 -> :neg\n      true -> :zero\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: cond +1
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // Nested `if` inside another `if`: outer +1, inner +2 (nested
    // depth 1) → cognitive 3.
    #[test]
    fn elixir_nested_if_amplifies() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x, y) do\n    if x > 0 do\n      if y > 0 do\n        :both\n      end\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: outer if (+1) + nested if (+2 because nesting=1)
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `try` with `rescue` and `catch`: the `try` wrapper itself does
    // NOT bump nesting (matches Java / C#'s "try is a wrapper" rule);
    // each `rescue` / `catch` block bumps +1 nesting at depth 0. The
    // single `stab_clause` inside each block adds no extra cost.
    #[test]
    fn elixir_try_rescue_catch() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    try do\n      :ok\n    rescue\n      _ -> :err\n    catch\n      _ -> :thrown\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: rescue (+1) + catch (+1) = 2
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // Short-circuit booleans: `x && y || z` is two operator types in
    // sequence — `&&` once, `||` once → +2. The `if` container that
    // surrounds them adds +1 → total cognitive 3.
    #[test]
    fn elixir_boolean_sequence() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x, y, z) do\n    if x && y || z do\n      :hit\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: if (+1) + && (+1) + || (+1) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `Enum.reduce` (and friends) are higher-order calls, NOT control
    // flow per the SonarSource spec. They contribute nothing to
    // cognitive complexity. The anonymous function body inside
    // contributes +1 lambda nesting, but its only operation is a
    // function call (no control flow) → cognitive 0.
    #[test]
    fn elixir_enum_reduce_is_zero() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def sum(xs) do\n    Enum.reduce(xs, 0, fn x, acc -> acc + x end)\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: 0 — Enum.reduce is a function call, not
                // syntactic control flow; the `fn` body has no decisions.
                assert_eq!(metric.cognitive.cognitive_sum(), 0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // Recursion: a `def` whose body calls itself by name. Per the
    // SonarSource spec recursion is +1, but our impl skips it for
    // scope reasons (documented). The body's lone Call earns nothing,
    // so cognitive stays at 0. This test pins the documented omission
    // so any future recursion work has to update it deliberately.
    #[test]
    fn elixir_recursion_is_zero_documented_limitation() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def fact(0), do: 1\n  def fact(n), do: n * fact(n - 1)\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_match_cognitive() {
        // `match` is treated like `switch`: a single nesting bump for the
        // whole construct, not per arm.
        check_metrics::<PhpParser>(
            "<?php
            function color(string $c): int {
                return match ($c) {
                    'red' => 1,
                    'green' => 2,
                    default => 0,
                };
            }",
            "foo.php",
            |metric| {
                // `match` is treated like `switch`: a single +1 for the construct.
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                assert_eq!(metric.cognitive.cognitive_max(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_no_cognitive() {
        check_metrics::<RubyParser>("a = 42\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn ruby_simple_function() {
        // A function body with no branching scores zero cognitive.
        check_metrics::<RubyParser>("def foo\n  a = 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn ruby_1_level_nesting() {
        // Single `if` inside a function: +1.
        check_metrics::<RubyParser>("def foo\n  if a\n    b\n  end\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 1);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn ruby_2_level_nesting() {
        // expected: outer `if` (+1) + inner `if` (+2, nested) = 3.
        check_metrics::<RubyParser>(
            "def foo\n  if a\n    if b\n      c\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_sequence_same_booleans() {
        // `a && b && c`: same operator collapses to a single boolean
        // sequence (+1). Plus the enclosing `if` (+1) → 2.
        check_metrics::<RubyParser>(
            "def foo\n  if a && b && c\n    d\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_sequence_different_booleans() {
        // `a && b || c`: alternating operators add per change.
        check_metrics::<RubyParser>(
            "def foo\n  if a && b || c\n    d\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_not_booleans() {
        // `!a` (Unary) is the not-operator: it doesn't add cognitive
        // load by itself. Only the enclosing `if` counts.
        check_metrics::<RubyParser>(
            "def foo\n  if !a\n    b\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_break_next() {
        // Ruby has no labeled loops, so `break`/`next` are always
        // unlabeled. Per SonarSource Cognitive Complexity §B2 an unlabeled
        // break/continue adds +0 (issue #435) — only the enclosing `while`
        // (+1) counts → 1.
        check_metrics::<RubyParser>(
            "def foo\n  while a\n    break\n    next\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_redo_retry_counted() {
        // `redo` (restart the current loop iteration) and `retry` (re-run a
        // rescued `begin` block) are genuinely unstructured jumps with no
        // structured equivalent, so each adds +1 per SonarSource §B2
        // (issue #435) even though `break`/`next` do not.
        check_metrics::<RubyParser>(
            "def foo\n  while a\n    redo\n  end\n  begin\n    work\n  rescue\n    retry\n  end\nend\n",
            "foo.rb",
            |metric| {
                // while(+1) + redo(+1) + rescue(+1) + retry(+1) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_else_if_chain() {
        // `elsif` extends the parent branch (no extra nesting). An
        // `if/elsif/elsif/else` chain scores strictly LESS than the
        // same number of nested `if` blocks. tree-sitter-ruby gives
        // `elsif` its own clause node, so the lesson-10 trap (a buggy
        // `is_else_if` that returns false makes `elsif` nest like
        // `if`) doesn't apply directly here — the test still pins the
        // chain vs nested cost difference so a future refactor that
        // mis-classifies `Elsif` would regress it.
        // expected: chain = 1 (`if`) + 2 (two `elsif`) + 1 (`else`) = 4;
        // nested = 1 + 2 + 3 = 6. The literal `4 < 6` asserts the
        // intended relationship.
        check_metrics::<RubyParser>(
            "def foo\n  if a\n    1\n  elsif b\n    2\n  elsif c\n    3\n  else\n    4\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
        check_metrics::<RubyParser>(
            "def foo\n  if a\n    if b\n      if c\n        1\n      end\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
            },
        );
    }

    #[test]
    fn ruby_case_else_no_extra_increment() {
        // #451: the `else` arm of a `case/when` is the default arm of a
        // switch-like construct. The `case` node already pays nesting
        // (+1), so the default arm must add +0 — adding `else` to a
        // `case` must not change the cognitive score.
        //
        // Pre-fix, the shared `R::Elsif | R::Else` arm added +1 to the
        // case-`else`, scoring 2 (revert-verified). Now both forms score 1.
        let case_with_else = "case x\nwhen 1 then 1\nelse 0\nend\n";
        let case_without_else = "case x\nwhen 1 then 1\nwhen 2 then 2\nend\n";
        check_metrics::<RubyParser>(case_with_else, "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 1);
            insta::assert_json_snapshot!(metric.cognitive, @r#"
            {
              "sum": 1,
              "value": 1,
              "average": 1.0,
              "min": 1,
              "max": 1
            }
            "#);
        });
        check_metrics::<RubyParser>(case_without_else, "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 1);
        });
    }

    #[test]
    fn ruby_case_else_matches_kotlin_when_and_java_switch() {
        // #451 cross-language parity (lesson #11): the catch-all arm of a
        // switch-like construct scores identically across languages. Ruby
        // `case`/`else`, Kotlin `when`/`else`, and Java `switch`/`default`
        // must all report cognitive == 1 on the equivalent two-branch
        // construct (one match arm + the default arm).
        check_metrics::<RubyParser>("case x\nwhen 1 then 1\nelse 0\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 1);
        });
        check_metrics::<KotlinParser>(
            "fun f(x: Int): Int {\n    return when (x) {\n        1 -> 1\n        else -> 0\n    }\n}\n",
            "foo.kt",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
            },
        );
        check_metrics::<JavaParser>(
            "class C {\n  int f(int x) {\n    switch (x) {\n      case 1: return 1;\n      default: return 0;\n    }\n  }\n}\n",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
            },
        );
    }

    #[test]
    fn ruby_if_else_still_counts() {
        // #451 over-suppression guard: the `else` of an `if`/`elsif` chain
        // is *not* switch-like (its parent is the `if`/`elsif` clause, not a
        // `case`), so it must still add +1. `if`(+1) + `else`(+1) = 2.
        check_metrics::<RubyParser>("if a\n  1\nelse\n  2\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 2);
        });
        // `begin`/`rescue`/`else` is the no-exception branch, mirroring
        // Python `try`/`except`/`else` (+1), not a switch default. The
        // `rescue`(+1) and `else`(+1) both count: total 2.
        check_metrics::<RubyParser>(
            "begin\n  foo\nrescue\n  bar\nelse\n  baz\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
            },
        );
    }

    #[test]
    fn ruby_stabby_and_keyword_lambda_nesting_parity() {
        // A stabby lambda parses as a `Lambda` node CONTAINING its own
        // body `Block`; the keyword form parses as a `Call` carrying the
        // block argument. Only the `Lambda` wrapper pays the lambda
        // nesting surcharge — its body block must not pay again, or the
        // same logical construct scores differently across the two
        // spellings (lesson #11; the #1257 shared discriminator).
        //
        // expected: `if`(+1) + one level of lambda nesting(+1) = 2.
        check_metrics::<RubyParser>("f = ->(a) { if a then 1 end }\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 2);
        });
        // expected: identical derivation for the keyword form — the
        // `Block` under the `lambda` call is the sole closure node:
        // `if`(+1) + lambda nesting(+1) = 2.
        check_metrics::<RubyParser>("f = lambda { |a| if a then 1 end }\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 2);
        });
    }

    #[test]
    fn ruby_if_inside_stabby_lambda_inside_method() {
        // expected: the method boundary resets nesting for its contents
        // (top-level method, so function_depth stays 0); the stabby
        // lambda adds exactly ONE lambda-nesting level (`Lambda` wrapper
        // only — its body `Block` is gated out); the `if` then charges
        // 1 + nesting(0 conditional + 0 function_depth + 1 lambda) = 2.
        // Before the gate the body block paid a second surcharge and
        // this scored 3.
        check_metrics::<RubyParser>(
            "def foo\n  f = ->(a) { if a then 1 end }\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
            },
        );
    }

    #[test]
    fn ruby_do_block_spelling_matches_brace_spelling() {
        // The lambda-nesting arm is gated on `Block | DoBlock`, and the
        // two alternatives are separate dispatch paths: every other Ruby
        // cognitive fixture uses the brace spelling, so the `do … end`
        // half of both the arm and its stabby-lambda-body gate went
        // unexercised. A block's spelling is pure syntax — the same
        // closure must score identically either way (lesson 11).
        //
        // expected: the `Lambda` wrapper pays the one lambda-nesting
        // level and its `do_block` body is gated out, so `if` charges
        // 1 + nesting(1) = 2 — the same figure the brace form reports in
        // `ruby_stabby_and_keyword_lambda_nesting_parity`.
        check_metrics::<RubyParser>(
            "f = ->(a) do\n  if a then 1 end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
            },
        );
        // expected: an iterator `do … end` has a `Call` parent, not a
        // `Lambda`, so the gate lets it through and the `do_block` is
        // itself the closure: `if`(+1) + lambda nesting(+1) = 2. This is
        // the arm-taken direction of the same gate.
        check_metrics::<RubyParser>(
            "[1].each do |x|\n  if x then 1 end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
            },
        );
    }

    #[test]
    fn javascript_labeled_break_continue() {
        // Per SonarSource Cognitive Complexity §B2 (issue #435), a labeled
        // `break LABEL` / `continue LABEL` is an unstructured jump and adds
        // +1. The JS-family grammar exposes the label as a
        // `statement_identifier` child of the break/continue node.
        check_metrics::<JavascriptParser>(
            "function scan(m) {
                outer:
                for (let i = 0; i < m.length; i++) {      // +1
                    for (let j = 0; j < m[i].length; j++) { // +2
                        if (m[i][j] < 0) continue outer;    // +3, +1
                        if (m[i][j] > 100) break outer;     // +3, +1
                    }
                }
            }",
            "foo.js",
            |metric| {
                // outer for(+1) + inner for(+2) + if(+3) + continue outer(+1)
                // + if(+3) + break outer(+1) = 11.
                assert_eq!(metric.cognitive.cognitive_sum(), 11);
                assert_eq!(metric.cognitive.cognitive_max(), 11);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 11,
                  "value": 0,
                  "average": 11.0,
                  "min": 0,
                  "max": 11
                }
                "#
                );
            },
        );
    }

    #[test]
    fn javascript_unlabeled_break_continue_not_counted() {
        // Negative test for issue #435: plain `break;` / `continue;` are
        // not unstructured jumps under SonarSource §B2 and add +0. Only the
        // surrounding `for` + two `if`s contribute.
        check_metrics::<JavascriptParser>(
            "function scan(m) {
                for (let i = 0; i < m.length; i++) { // +1
                    if (m[i] < 0) continue;           // +2, +0
                    if (m[i] > 100) break;            // +2, +0
                }
            }",
            "foo.js",
            |metric| {
                // for(+1) + if(+2) + if(+2) = 5.
                assert_eq!(metric.cognitive.cognitive_sum(), 5);
                assert_eq!(metric.cognitive.cognitive_max(), 5);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 5,
                  "value": 0,
                  "average": 5.0,
                  "min": 0,
                  "max": 5
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_labeled_break_continue() {
        // TS parity with JS for labeled jumps (issue #435): labeled
        // break/continue each add +1 via the `statement_identifier` child.
        check_metrics::<TypescriptParser>(
            "function scan(m: number[][]) {
                outer:
                for (let i = 0; i < m.length; i++) {      // +1
                    for (let j = 0; j < m[i].length; j++) { // +2
                        if (m[i][j] < 0) continue outer;    // +3, +1
                        if (m[i][j] > 100) break outer;     // +3, +1
                    }
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 11);
                assert_eq!(metric.cognitive.cognitive_max(), 11);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 11,
                  "value": 0,
                  "average": 11.0,
                  "min": 0,
                  "max": 11
                }
                "#
                );
            },
        );
    }

    /// Asserts the JS-family function-boundary rule over every shape
    /// #1159 moves, for one instantiating language.
    ///
    /// `js_cognitive!` listed `FunctionDeclaration` alone, so a
    /// `method_definition` or a bound `function_expression` opened its own
    /// `SpaceKind::Function` space — `get_space_kind` maps both — while
    /// inheriting the enclosing conditional nesting and skipping the
    /// function-depth surcharge. Its `stops` list was short by the same
    /// two kinds, which is a separately observable bug: the reset shows on
    /// a definition nested in *conditionals*, the `stops` entry on one
    /// nested in another *function*. Both are covered below, plus the two
    /// shapes the fix must leave alone.
    fn check_js_function_boundary<T: ParserTrait>(filename: &str) {
        fn score(space: &FuncSpace, name: &str) -> u64 {
            function_space(space, name).metrics.cognitive.cognitive()
        }

        // expected: `outer`'s two `if`s are +1 and +2, so `outer` scores
        // 3. The definition nested inside them restarts structural
        // nesting at 0, so its own `if` costs +1 base plus +1 function
        // depth (it is lexically inside `outer`) = 2 — the score the same
        // body written as a `function_declaration` already had, which is
        // why that form is asserted here as the control.
        //
        // Two conditional levels are load-bearing. At one level the
        // missing reset (+1) and the missing surcharge (-1) cancel and
        // both implementations report 2, so a one-level fixture cannot
        // discriminate.
        for (label, definition) in [
            (
                "function_declaration",
                "function inner(c) { if (c) { return 1; } }",
            ),
            (
                "method_definition",
                "class I { inner(c) { if (c) { return 1; } } }",
            ),
            (
                "function_expression",
                "const inner = function (c) { if (c) { return 1; } };",
            ),
            // Generators reach this arm since #1186. Before it,
            // `is_js_func!` called them closures, so neither form
            // reached the boundary and each scored 3.
            (
                "generator_function_declaration",
                "function* inner(c) { if (c) { yield 1; } }",
            ),
            (
                "generator_function",
                "const inner = function* (c) { if (c) { yield 1; } };",
            ),
        ] {
            let source =
                format!("function outer(a, b) {{ if (a) {{ if (b) {{ {definition} }} }} }}");
            check_func_space::<T, _>(&source, filename, |space| {
                assert_eq!(
                    score(&space, "outer"),
                    3,
                    "{label}: enclosing function's own score",
                );
                assert_eq!(
                    score(&space, "inner"),
                    2,
                    "{label}: nested definition restarts structural nesting",
                );
            });
        }

        // The `stops` half, on a definition nested in another function
        // rather than in conditionals.
        // expected: `inner`'s `if` is +1 base plus +1 function depth = 2.
        // With the enclosing kind absent from `stops` the surcharge is 0
        // and `inner` scores 1.
        for (label, source) in [
            (
                "method_definition",
                "class I { m() { function inner(c) { if (c) { return 1; } } } }",
            ),
            (
                "function_expression",
                "const m = function () { function inner(c) { if (c) { return 1; } } };",
            ),
            // The independent half of #1186: a plain `function` nested
            // inside a *generator* got no depth surcharge, because the
            // generator was excluded from `stops` by the same
            // `is_js_func!` gate. This scored 1 before the fix while the
            // non-generator control above scored 2.
            (
                "generator_function_declaration",
                "function* m() { function inner(c) { if (c) { return 1; } } }",
            ),
            (
                "generator_function",
                "const m = function* () { function inner(c) { if (c) { return 1; } } };",
            ),
        ] {
            check_func_space::<T, _>(source, filename, |space| {
                assert_eq!(
                    score(&space, "inner"),
                    2,
                    "{label}: +1 base, +1 depth from the enclosing definition",
                );
            });
        }

        // An *anonymous* `function_expression` used positionally fails
        // `check_if_func!` and is a *closure*, so it must keep falling
        // through to `_` and inheriting the enclosing nesting. This is what
        // pins that the gate was re-derived rather than the kind list
        // copied flat: an ungated arm resets here and reports 2, because
        // `outer` is a `FunctionDeclaration` and so a `stops` entry.
        // Anonymity is load-bearing — `check_if_func!`'s `$extra` disjunct
        // makes `run(function named (c) {…})` a function, and `nom` agrees.
        // expected: nesting.conditional 2 from `outer`'s two `if`s, so the
        // callback's own `if` costs +3. The `nom` assertion states the
        // premise — that this shape really is on the closure side of
        // `is_func` / `is_closure` — rather than leaving it implied.
        check_func_space::<T, _>(
            "function outer(a, b) {
                 if (a) { if (b) { run(function (c) { if (c) { return 1; } }); } }
             }",
            filename,
            |space| {
                assert_eq!(
                    space.metrics.nom.closures_sum(),
                    1,
                    "a positional function expression is a closure",
                );
                assert_eq!(
                    score(&space, "<anonymous>"),
                    3,
                    "a closure inherits the enclosing conditional nesting",
                );
            },
        );

        // `ArrowFunction` was deliberately left out of the boundary set —
        // it owns the lambda channel in `js_cognitive!`'s `ArrowFunction`
        // arm — so sweeping it in is the other way to get this fix wrong.
        // expected: nesting.conditional 2 from `outer`'s two `if`s plus
        // nesting.lambda 1 from the arrow, so its `if` costs +4. A
        // boundary arm that swept `ArrowFunction` in would report 2.
        check_func_space::<T, _>(
            "function outer(a, b) {
                 if (a) { if (b) { const inner = (c) => { if (c) { return 1; } }; } }
             }",
            filename,
            |space| {
                assert_eq!(
                    score(&space, "inner"),
                    4,
                    "an arrow function keeps the lambda channel",
                );
            },
        );
    }

    // One `#[test]` per language instantiating `js_cognitive!`: the macro
    // body is shared but each grammar's `kind_id`s are its own, so a
    // per-language enum drift is invisible from a single language's run.
    #[test]
    fn javascript_function_boundary_covers_methods_and_function_expressions_1159() {
        check_js_function_boundary::<JavascriptParser>("foo.js");
    }

    #[test]
    fn mozjs_function_boundary_covers_methods_and_function_expressions_1159() {
        check_js_function_boundary::<MozjsParser>("foo.js");
    }

    #[test]
    fn typescript_function_boundary_covers_methods_and_function_expressions_1159() {
        check_js_function_boundary::<TypescriptParser>("foo.ts");
    }

    #[test]
    fn tsx_function_boundary_covers_methods_and_function_expressions_1159() {
        check_js_function_boundary::<TsxParser>("foo.tsx");
    }

    #[test]
    fn javascript_compound_short_circuit_assignment_236() {
        // Regression for issue #236: `&&=`, `||=`, `??=` are compound
        // short-circuit assignments (e.g. `x ??= y` ≡ `x = x ?? y`)
        // and each carries one boolean-sequence decision. Each lives
        // inside its own `expression_statement`, so the boolean
        // sequence resets between them and all three count.
        check_metrics::<JavascriptParser>(
            "function f(x) {
                 x ??= 1; // +1 (??=)
                 x &&= 2; // +1 (&&=)
                 x ||= 3; // +1 (||=)
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_compound_short_circuit_assignment_236() {
        // Regression for issue #236: TS parity with JS for `&&=`,
        // `||=`, `??=`.
        check_metrics::<TypescriptParser>(
            "function f(x: number | null) {
                 x ??= 1; // +1 (??=)
                 x &&= 2; // +1 (&&=)
                 x ||= 3; // +1 (||=)
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn tsx_compound_short_circuit_assignment_236() {
        // Regression for issue #236: TSX parity with JS/TS for `&&=`,
        // `||=`, `??=`.
        check_metrics::<TsxParser>(
            "function f(x: number | null) {
                 x ??= 1; // +1 (??=)
                 x &&= 2; // +1 (&&=)
                 x ||= 3; // +1 (||=)
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_compound_short_circuit_assignment_236() {
        // Regression for issue #236: Mozjs (SpiderMonkey-flavoured JS)
        // shares the JS macro and must score `&&=` / `||=` / `??=`
        // identically.
        check_metrics::<MozjsParser>(
            "function f(x) {
                 x ??= 1; // +1 (??=)
                 x &&= 2; // +1 (&&=)
                 x ||= 3; // +1 (||=)
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                assert_eq!(metric.cognitive.cognitive_max(), 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_compound_short_circuit_assignment_236() {
        // Regression for issue #236: C#'s grammar only provides `??=`
        // among the short-circuit assignments (no `&&=` / `||=`). The
        // operator lives inside `assignment_expression` rather than a
        // `BinaryExpression`, so without the #236 fix it was silently
        // skipped.
        check_metrics::<CsharpParser>(
            "class C {
                 int? F(int? x) {
                     x ??= 1; // +1 (??=)
                     return x ?? 0;
                 }
             }",
            "foo.cs",
            |metric| {
                // Outer `??` chain (+1) + `??=` (+1) = 2 at function max.
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn php_compound_short_circuit_assignment_236() {
        // Regression for issue #236: PHP's only compound short-circuit
        // assignment is `??=` (no `&&=` / `||=`). It lives inside
        // `augmented_assignment_expression` rather than a
        // `BinaryExpression`, so without the #236 fix it was silently
        // skipped.
        check_metrics::<PhpParser>(
            "<?php
            function f($x) {
                $x ??= 1; // +1 (??=)
                return $x ?? 0; // +1 (??)
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                assert_eq!(metric.cognitive.cognitive_max(), 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#
                );
            },
        );
    }

    /// A handler with no control flow has zero cognitive complexity.
    #[test]
    fn irules_no_cognitive() {
        check_metrics::<IrulesParser>("when X { set a 1 }\n", "foo.irule", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0);
        });
    }

    /// A single `if` adds one.
    #[test]
    fn irules_simple_function() {
        check_metrics::<IrulesParser>(
            "when X { if { $a } { log local0. \"hi\" } }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
            },
        );
    }

    /// A run of the *same* boolean operator (`$a && $b && $c`) is one
    /// sequence: `if` (1) + boolean sequence (1) = 2.
    #[test]
    fn irules_sequence_same_booleans() {
        check_metrics::<IrulesParser>(
            "when X { if { $a && $b && $c } { log local0. \"hi\" } }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
            },
        );
    }

    /// Switching operator (`$a && $b || $c`) starts a new sequence: `if` (1)
    /// + `&&` sequence (1) + `||` sequence (1) = 3.
    #[test]
    fn irules_sequence_different_booleans() {
        check_metrics::<IrulesParser>(
            "when X { if { $a && $b || $c } { log local0. \"hi\" } }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
            },
        );
    }

    /// Unary negation (`!`) does not itself add cognitive cost; only the
    /// boolean sequence does: `if` (1) + `&&` sequence (1) = 2.
    #[test]
    fn irules_not_booleans() {
        check_metrics::<IrulesParser>(
            "when X { if { !$a && !$b } { log local0. \"hi\" } }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
            },
        );
    }

    /// One level of nesting: `while` (1) + `if` (1 + nesting 1 = 2) = 3.
    #[test]
    fn irules_1_level_nesting() {
        check_metrics::<IrulesParser>(
            "when X { while { $a } { if { $b } { log local0. \"hi\" } } }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
            },
        );
    }

    /// Two levels: `while` (1) + `if` (2) + `foreach` (1 + nesting 2 = 3) = 6.
    #[test]
    fn irules_2_level_nesting() {
        check_metrics::<IrulesParser>(
            "when X { while { $a } { if { $b } { foreach z $l { log local0. \"hi\" } } } }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 6);
            },
        );
    }

    /// The lesson-10 guard for `is_else_if`: an `if … elseif … elseif …
    /// else` chain (each clause +1 at the same level = 4) must score
    /// *lower* than the same number of `if`s nested inside one another
    /// (1 + 2 + 3 = 6, paying the nesting penalty). A broken `is_else_if`
    /// predicate that treated `elseif` like a fresh nested `if` would push
    /// the chain's score up toward the nested value, so the strict `<`
    /// assertion catches the regression that #115 found in Java/C#.
    #[test]
    fn irules_else_if_chain() {
        use std::cell::Cell;

        let chain = "when X { if { $a } { set r 1 } elseif { $b } { set r 2 } elseif { $c } { set r 3 } else { set r 4 } }\n";
        let nested = "when X { if { $a } { if { $b } { if { $c } { set r 1 } } } }\n";

        // Capture each measured sum through a `Cell` (check_func_space takes an
        // `Fn` closure) so the final `<` assertion compares the *actual*
        // values rather than restating constants.
        let chain_cog = Cell::new(-1.0);
        check_func_space::<IrulesParser, _>(chain, "chain.irule", |fs| {
            chain_cog.set(fs.metrics.cognitive.cognitive_sum() as f64);
        });
        let nested_cog = Cell::new(-1.0);
        check_func_space::<IrulesParser, _>(nested, "nested.irule", |fs| {
            nested_cog.set(fs.metrics.cognitive.cognitive_sum() as f64);
        });

        assert_eq!(chain_cog.get(), 4.0);
        assert_eq!(nested_cog.get(), 6.0);
        assert!(
            chain_cog.get() < nested_cog.get(),
            "else-if chain ({}) must score lower than equivalently nested ifs ({})",
            chain_cog.get(),
            nested_cog.get(),
        );
    }

    /// A `switch` nested in an `if`: `if` (1) + `switch` (1 + nesting 1 = 2)
    /// = 3. Confirms `switch` participates in nesting like other branches.
    #[test]
    fn irules_switch_nesting() {
        check_metrics::<IrulesParser>(
            "when X { if { $a } { switch $h { a { log local0. \"a\" } b { log local0. \"b\" } } } }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
            },
        );
    }

    /// `catch` is a conditional error handler — its body runs only when
    /// the guarded command errors — so it pays nesting like any other
    /// branch. Flat: 1. Nested in an `if`: `if` (1) + `catch`
    /// (1 + nesting 1 = 2) = 3, matching `irules_switch_nesting`.
    ///
    /// The `Catch` arm had no test before this: the whole arm measured
    /// zero-coverage while every other iRules branch kind was exercised.
    #[test]
    fn irules_catch_nesting() {
        check_metrics::<IrulesParser>("when X { catch { foo } }\n", "foo.irule", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 1);
        });
        check_metrics::<IrulesParser>(
            "when X { if { $a } { catch { foo } } }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
            },
        );
    }

    /// Objective-C straight-line method body has zero cognitive
    /// complexity.
    #[test]
    fn objc_no_cognitive() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (int)bar {
    int a = 1;
    return a;
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 0,
                  "value": 0,
                  "average": 0.0,
                  "min": 0,
                  "max": 0
                }
                "#);
            },
        );
    }

    /// Objective-C single `if` at method top level: +1, no nesting
    /// surcharge.
    #[test]
    fn objc_simple_if() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)bar:(int)x {
    if (x > 0) {
        [self use:x];
    }
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 1
                }
                "#);
            },
        );
    }

    /// Objective-C chained booleans `a && b && c`: SonarSource counts
    /// one for the first `&&` and zero for each additional same-operator
    /// link in the sequence, so the whole `if (a && b && c)` is +1 (if)
    /// + 1 (one boolean sequence) = 2.
    #[test]
    fn objc_sequence_same_booleans() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)bar:(int)a b:(int)b c:(int)c {
    if (a && b && c) {
        [self use:a];
    }
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 2.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }

    /// Objective-C nesting surcharge: an `if` nested inside a `for`
    /// scores `for` (+1) + `if` (+1 base +1 nesting) = 3.
    #[test]
    fn objc_nested() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)bar:(NSArray *)arr {
    for (id x in arr) {
        if ([x boolValue]) {
            [self use:x];
        }
    }
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3,
                  "value": 0,
                  "average": 3.0,
                  "min": 0,
                  "max": 3
                }
                "#);
            },
        );
    }

    #[test]
    fn objc_block_nesting() {
        // A decision inside an ObjC block `^{ … }` picks up the lambda
        // surcharge: the `if` scores base (1) + lambda nesting (1) = 2,
        // exercising the `BlockLiteral => lambda += 1` path (the ObjC
        // closure analogue of the C++ lambda).
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)bar {
    void (^blk)(int) = ^(int x) {
        if (x > 0) {
            [self use];
        }
    };
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2,
                  "value": 0,
                  "average": 1.0,
                  "min": 0,
                  "max": 2
                }
                "#);
            },
        );
    }

    /// Objective-C `if / else if / else if / else` chain must score
    /// LOWER than the same number of singly-nested `if`s, because
    /// else-if links add no nesting surcharge while deepening `if`s do.
    /// This guards the `is_else_if` predicate (a regression that failed
    /// to recognise the else-if extension would inflate the chain to the
    /// nested score).
    #[test]
    fn objc_else_if_chain() {
        use std::cell::Cell;

        let chain_sum = Cell::new(u64::MAX);
        check_func_space::<ObjcParser, _>(
            "@implementation Foo
- (int)bar:(int)x {
    if (x == 1) {
        return 1;
    } else if (x == 2) {
        return 2;
    } else if (x == 3) {
        return 3;
    } else {
        return 0;
    }
}
@end
",
            "foo.m",
            |fs| chain_sum.set(fs.metrics.cognitive.cognitive_sum()),
        );

        let nested_sum = Cell::new(u64::MAX);
        check_func_space::<ObjcParser, _>(
            "@implementation Foo
- (int)bar:(int)x {
    if (x == 1) {
        if (x == 2) {
            if (x == 3) {
                return 3;
            }
        }
    }
    return 0;
}
@end
",
            "foo.m",
            |fs| nested_sum.set(fs.metrics.cognitive.cognitive_sum()),
        );

        // expected chain (matches the C-family else-if structure): each
        // `else if`/`else` adds +1 with NO nesting surcharge because
        // `is_else_if` recognises the else-clause-nested `if_statement` as
        // a branch extension — if(+1) + else-if(+1) + else-if(+1) +
        // else(+1) = 4. Were the predicate broken, the nested
        // `if_statement`s would accrue nesting (+2, +3) and the chain
        // would climb to 7. expected nested: if(+1) + if(+1+1) +
        // if(+1+2) = 6. The chain must remain strictly cheaper.
        assert_eq!(chain_sum.get(), 4, "else-if chain cognitive sum");
        assert_eq!(nested_sum.get(), 6, "triple-nested if cognitive sum");
        assert!(
            chain_sum.get() < nested_sum.get(),
            "else-if chain ({}) must score lower than triple-nested ifs ({})",
            chain_sum.get(),
            nested_sum.get(),
        );
    }

    /// Pins that `function_depth` and `lambda` are distinguishable.
    ///
    /// They are summed symmetrically almost everywhere, so most inputs
    /// cannot tell them apart. The asymmetric operation is
    /// `enter_function_boundary`'s `lambda = 0`, which clears one field
    /// while `increment_function_depth` raises the other — since #1187
    /// that pair runs for every language, not only the JS macro.
    ///
    /// The doubled arrow is still load-bearing, for the reason it always
    /// was: a swap at the write site transposes the pair at every node
    /// on the way down, and the plain
    /// `arrow -> statement_block -> function_declaration` chain has odd
    /// parity and totals the same either way. A mutant writing
    /// `function_depth = 0` in place of `lambda = 0` leaves `lambda 2,
    /// function_depth 1` here and charges the `if` 4 rather than 2.
    #[test]
    fn javascript_function_depth_and_lambda_are_distinguishable() {
        // expected: `inner` takes the boundary, so `conditional` and
        // `lambda` both reset to 0. `ArrowFunction` joined the `stops`
        // list in #1187, so `inner` earns a function-depth surcharge of
        // 1 — `increment_function_depth` asks whether *any* ancestor is a
        // stop, not how many, so two arrows still give 1. The `if` costs
        // 1 base + 1 depth = 2, up from 1 before the arrow entered
        // `stops`.
        check_metrics::<JavascriptParser>(
            "const f = () => () => { function inner() { if (a) { } } };",
            "nest.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
            },
        );
    }

    /// The `ArrowFunction` arm's own `lambda += 1`, pinned separately:
    /// with no `function_declaration` between the arrow and the `if`,
    /// nothing resets lambda, so the arrow's level reaches the `if`.
    #[test]
    fn javascript_arrow_contributes_lambda_nesting() {
        // expected: 1 for the `if`, +1 for the enclosing arrow level.
        check_metrics::<JavascriptParser>(
            "const f = () => { if (a) { } };",
            "arrow.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2);
            },
        );
    }

    /// Nesting is still inherited correctly thousands of levels deep
    /// (#1062).
    ///
    /// `get_nesting_from_map` used to recover a node's inherited nesting
    /// via `node.parent()`, which is `O(depth)` — tree-sitter stores no
    /// parent pointer — making the metric `O(nodes × depth)`. The walker
    /// now seeds each child's slot from its parent's, so the lookup is
    /// `O(1)`.
    ///
    /// Both versions produce identical numbers, only at different
    /// speeds, so what this test pins is correctness at depth. The
    /// *cost* is pinned by the `cognitive/nested-while` probe in the
    /// benchmark harness (#1068), which asserts the complexity class —
    /// `cargo bench -p big-code-analysis-bench --bench scaling`.
    ///
    /// The wall-clock half used to live here and produced a false
    /// failure in four environments: `windows-latest` in CI (10.9 s
    /// against an 8 s absolute budget), a local `make pre-commit`
    /// running clippy and rustdoc alongside the suite (5.6x), the same
    /// host under heavy parallel load (3.9x), and `cargo llvm-cov`,
    /// whose instrumentation skewed even a best-of-three ratio to 3.5x.
    /// The `coverage` job runs in CI, so leaving it armed redded the
    /// build on a measurement artefact rather than on a regression. A
    /// ratio between two depths is host-independent but not
    /// load-independent, and the fix for that is interleaved
    /// measurement at three depths, which belongs in a bench target
    /// and not in the unit suite.
    ///
    /// Uses `while`, deliberately, **not** `if`: when this test was
    /// written `Checker::is_else_if` still called `node.parent()` for
    /// every `if_statement`, so nested `if`s were quadratic for reasons
    /// that fix did not touch. #1084 moved that predicate onto the
    /// walker's ancestor chain, and the harness now measures the `if`
    /// shape under the same linear bound as this one.
    #[test]
    fn cognitive_nesting_is_inherited_at_depth() {
        // Restricted to `Cognitive` — which pulls in `Nom` as a declared
        // dependency, so this narrows the work rather than isolating it.
        fn cognitive_of(source: &str) -> u64 {
            crate::test_support::metrics_verbatim(
                crate::LANG::C,
                source.as_bytes(),
                crate::MetricsOptions::default().with_only(&[crate::Metric::Cognitive]),
            )
            .cognitive
            .cognitive_sum()
        }

        // Each level adds its own nesting penalty, so cognitive grows as
        // 1 + 2 + … + n. Asserting the closed form at depth is what pins
        // that nesting is still inherited rather than recomputed.
        let nested_whiles = |n: usize| -> String {
            format!(
                "int main(){{ {} 1; {} }}\n",
                "while (a) { ".repeat(n),
                "} ".repeat(n)
            )
        };
        let expected = |n: u64| n * (n + 1) / 2;

        assert_eq!(cognitive_of(&nested_whiles(3)), expected(3), "1 + 2 + 3");
        assert_eq!(cognitive_of(&nested_whiles(2_000)), expected(2_000));
    }

    /// Function-nesting depth is still counted correctly thousands of
    /// levels deep (#1062).
    ///
    /// `increment_function_depth` asks whether any ancestor of a
    /// function node is itself a function. It used to climb with
    /// `node.parent()`, which is `O(depth)` per step, so `Cognitive`
    /// stayed `O(depth²)` on nested definitions after the nesting-map
    /// half of #1062 was fixed. The scan now walks the ancestor chain
    /// the walker hands down.
    ///
    /// Both versions answer identically, so what this pins is the
    /// arithmetic at depth; the *cost* is pinned by the
    /// `cognitive/nested-fn` probe in the benchmark harness
    /// (`cargo bench -p big-code-analysis-bench --bench scaling`),
    /// which asserts the complexity class.
    #[test]
    fn cognitive_function_depth_is_inherited_at_depth() {
        fn cognitive_of(source: &str) -> u64 {
            crate::test_support::metrics_verbatim(
                crate::LANG::Rust,
                source.as_bytes(),
                crate::MetricsOptions::default().with_only(&[crate::Metric::Cognitive]),
            )
            .cognitive
            .cognitive_sum()
        }

        // The function at level k has k enclosing functions, so its
        // `if` costs k + 1 and the file totals 1 + 2 + … + n. The `if`
        // is what makes the depth observable: a chain of bare functions
        // scores zero however the depth is computed.
        let nested_fns = |n: usize| -> String {
            format!(
                "{}let x = 1;{}\n",
                "fn f() { if a {} ".repeat(n),
                "} ".repeat(n)
            )
        };
        let expected = |n: u64| n * (n + 1) / 2;

        assert_eq!(cognitive_of(&nested_fns(3)), expected(3), "1 + 2 + 3");
        // Half the depth of `cognitive_nesting_is_inherited_at_depth`
        // because each level here also opens a `FuncSpace`. The debug
        // build is no longer the reason: #1122 took the `Node::parent`
        // re-derivation out of `Ancestors::checked`, so an unoptimised
        // walk is linear like the release one and this case dropped from
        // ~1.0 s to ~0.02 s. `make chain-audit` puts the quadratic
        // assertion back deliberately.
        assert_eq!(cognitive_of(&nested_fns(1_000)), expected(1_000));
    }

    /// A function nested inside another makes its `if` cost one more
    /// than the same `if` at the top level, in every language that can
    /// express the nesting (#1062).
    ///
    /// That surcharge has exactly one source: `increment_function_depth`,
    /// which asks whether any ancestor of a function node is itself a
    /// function. #1062 moved the scan off `Node::parent` — `O(depth)` per
    /// step, and so quadratic over a deeply nested file — and onto the
    /// ancestor chain the walker hands down. The flat source in each row
    /// is the control: the same `if` with nothing enclosing its function,
    /// which must stay at 1, so the pair measures the surcharge and not
    /// the body.
    ///
    /// These are the languages whose function-depth arm had no test of
    /// its own; C++, C#, Groovy, Java, Kotlin, Perl, PHP, Python, Rust
    /// and Tcl are covered by dedicated tests elsewhere in this module.
    /// Go is deliberately absent: its stop set is `function_declaration`
    /// / `method_declaration` and the grammar allows neither inside a
    /// function body, so the surcharge is unreachable there — a nested
    /// Go function is a `func_literal`, which takes the `lambda` path.
    #[test]
    fn function_depth_surcharge_holds_across_languages() {
        use crate::test_support::metrics_verbatim;

        fn cognitive_of(lang: LANG, source: &str) -> u64 {
            metrics_verbatim(lang, source.as_bytes(), MetricsOptions::default())
                .cognitive
                .cognitive_sum()
        }

        // Every row must parse cleanly. Several of these snippets lean
        // on a grammar's less-travelled corners — GNU nested functions
        // in C, a `function` statement inside another in Lua, a `proc`
        // inside a `proc` in iRules — and a grammar bump that stopped
        // accepting one would leave that row measuring `tree_sitter`'s
        // error recovery while still reporting 1 and 2. Verified: with
        // trailing garbage appended to the C source, the costs below
        // are unmoved and the test stays green without this check.
        fn parses_cleanly(lang: LANG, source: &str) -> bool {
            crate::Ast::parse(crate::Source::new(lang, source.as_bytes()))
                .is_ok_and(|ast| !ast.as_tree_sitter().root_node().has_error())
        }

        // C: a GNU nested function definition.
        const C_FLAT: &str = "void f(int a) { if (a) { } }\n";
        const C_NESTED: &str = "void f(int a) { void g(int b) { if (b) { } } }\n";
        // Objective-C reuses C's `function_definition` stop but adds
        // `method_definition`, which only a real `@implementation`
        // reaches — running the C source here would duplicate the row
        // above and leave that extra stop untested.
        const OBJC_FLAT: &str = "@implementation A\n- (void)m:(int)a { if (a) { } }\n@end\n";
        const OBJC_NESTED: &str =
            "@implementation A\n- (void)m:(int)a { void g(int b) { if (b) { } } }\n@end\n";
        // C++ has no nested function definitions; a method on a local
        // struct is the nesting the grammar does admit (see
        // `cpp_nested_function_resets_nesting_and_adds_depth`).
        const CPP_FLAT: &str = "struct S { void f(bool a) { if (a) { } } };\n";
        const CPP_NESTED: &str =
            "struct S { void f(bool a) { struct I { void g(bool b) { if (b) { } } }; } };\n";
        const JS_FLAT: &str = "function f(a) { if (a) { } }\n";
        const JS_NESTED: &str = "function f(a) { function g(b) { if (b) { } } }\n";
        const RUBY_FLAT: &str = "def f\nif a\nend\nend\n";
        const RUBY_NESTED: &str = "def f\ndef g\nif a\nend\nend\nend\n";
        const LUA_FLAT: &str = "function f() if a then end end\n";
        const LUA_NESTED: &str = "function f() function g() if a then end end end\n";
        const BASH_FLAT: &str = "f() {\nif [ -n \"$a\" ]; then :; fi\n}\n";
        const BASH_NESTED: &str = "f() {\ng() {\nif [ -n \"$a\" ]; then :; fi\n}\n}\n";
        const TCL_FLAT: &str = "proc outer {x} {\nif {$x > 0} {\nputs positive\n}\n}\n";
        const TCL_NESTED: &str =
            "proc outer {x} {\nproc inner {y} {\nif {$y > 0} {\nputs positive\n}\n}\n}\n";

        let rows = [
            (LANG::C, C_FLAT, C_NESTED),
            (LANG::Objc, OBJC_FLAT, OBJC_NESTED),
            (LANG::Mozcpp, CPP_FLAT, CPP_NESTED),
            (LANG::Javascript, JS_FLAT, JS_NESTED),
            (LANG::Mozjs, JS_FLAT, JS_NESTED),
            (LANG::Typescript, JS_FLAT, JS_NESTED),
            (LANG::Tsx, JS_FLAT, JS_NESTED),
            (LANG::Ruby, RUBY_FLAT, RUBY_NESTED),
            (LANG::Lua, LUA_FLAT, LUA_NESTED),
            (LANG::Bash, BASH_FLAT, BASH_NESTED),
            (LANG::Irules, TCL_FLAT, TCL_NESTED),
        ];

        // Whole vectors rather than a per-row `assert_eq!`: when this
        // shared walk breaks it breaks for every language at once, and
        // comparing the columns shows all of them instead of stopping
        // at the first. Hand-rolling that diagnostic with a `wrong`
        // accumulator would work too, but its `push` arm is a branch no
        // passing run ever takes — dead weight that reads as a coverage
        // hole and never gets exercised.
        let measured: Vec<(LANG, u64, u64)> = rows
            .iter()
            .map(|&(lang, flat, nested)| {
                for (label, source) in [("flat", flat), ("nested", nested)] {
                    assert!(
                        parses_cleanly(lang, source),
                        "{lang:?}: the {label} source must parse without an ERROR node:\n{source}",
                    );
                }
                (lang, cognitive_of(lang, flat), cognitive_of(lang, nested))
            })
            .collect();
        let expected: Vec<(LANG, u64, u64)> = rows.iter().map(|&(lang, ..)| (lang, 1, 2)).collect();

        assert_eq!(
            measured, expected,
            "an `if` must cost 1 in a top-level function and 2 one function deeper",
        );
    }

    /// Every [`Nesting`] channel contributes to `total()`.
    ///
    /// Distinct powers of two, so dropping a channel or summing one
    /// twice — the failure modes of the open-coded
    /// `conditional + function_depth + lambda` this method replaced at
    /// its two sites (#1086) — is distinguishable from the total alone
    /// rather than just reading as a bad number.
    #[test]
    fn nesting_total_sums_every_channel() {
        assert_eq!(
            Nesting {
                conditional: 1,
                function_depth: 2,
                lambda: 4,
            }
            .total(),
            7,
            "1/2/4 encoding: short by 1 means `conditional` was dropped, \
             by 2 `function_depth`, by 4 `lambda`; over by the same \
             amount means that channel was summed twice",
        );
        // Weak on its own — every field is zero, so this survives any
        // linear combination of them. It only rules out a `total()` that
        // returns a nonzero constant.
        assert_eq!(Nesting::default().total(), 0);
    }

    /// `increase_nesting` charges the *summed* level but advances only
    /// the `conditional` channel.
    ///
    /// Before #1086 this helper took `&mut usize` for the conditional
    /// channel and `depth` / `lambda` as by-value non-`mut` params, so
    /// bumping the wrong one was *inert* rather than unrepresentable:
    /// `depth += 1` needed a `mut` added to compile, and then wrote to a
    /// copy nobody read. It now holds the whole struct, which makes
    /// `function_depth += 1` a one-character slip that persists —
    /// invisible in the *charge* itself, since `total()` is symmetric,
    /// and detectable only downstream where the channels are read apart.
    ///
    /// Perturbing the production line to `function_depth += 1` fails this
    /// test plus five nested-function tests (`java_nested_method_…`,
    /// `cpp_nested_function_…`, `groovy_…`, `php_…`,
    /// `csharp_local_function_in_if_…`) — those catch it only because a
    /// function boundary resets `conditional` alone, leaving the misplaced
    /// increment behind. This test pins it at the helper, where the
    /// mistake is, rather than five languages away from it.
    #[test]
    fn increase_nesting_charges_the_total_but_advances_only_conditional() {
        let mut nesting = Nesting {
            conditional: 1,
            function_depth: 2,
            lambda: 4,
        };
        // Both fields are seeded rather than defaulted. From
        // `Stats::default()` the `boolean_seq` assertion holds even with
        // `reset()` deleted, and the `structural` assertion cannot tell
        // `increment`'s `+=` from a plain `=`, since both start at zero.
        let mut stats = Stats {
            structural: 5,
            boolean_seq: BoolSequence {
                boolean_op: Some((1, 0)),
            },
            ..Stats::default()
        };

        increase_nesting(&mut stats, &mut nesting);

        // Charged at the inherited level (7), and `increment`
        // accumulates `nesting + 1` onto the seeded 5.
        assert_eq!(stats.nesting, 7);
        assert_eq!(stats.structural, 13);
        assert_eq!(stats.boolean_seq, BoolSequence::default());
        assert_eq!(
            nesting,
            Nesting {
                conditional: 2,
                function_depth: 2,
                lambda: 4,
            }
        );
    }
}

/// The nameless constructs from #1184 are function boundaries, so a
/// deeply-nested one must score what the same body scores as an
/// ordinary method in the same position (#1184).
///
/// Each opens a `FuncSpace`, and without a cognitive boundary arm it
/// reached none and inherited the enclosing conditional nesting: a
/// Kotlin accessor nested two `if`s deep scored 7 where the method
/// beside it scored 5.
///
/// **Two levels of nesting are load-bearing.** At one level the fixture
/// reports the same number either way, which is the trap the issue's own
/// checklist warns about — a first draft of this test used one `if` and
/// could not discriminate the fix from its absence.
///
/// The comparison is against a *sibling method* rather than an absolute
/// number, so the assertion states the property (these are ordinary
/// function boundaries) rather than a value that moves with any
/// unrelated re-tuning. The absolute is pinned too, so a regression
/// moving both equally still fails.
#[cfg(test)]
mod nameless_construct_boundaries {
    use crate::test_support::space_verbatim;
    use crate::{FuncSpace, LANG, MetricsOptions};

    fn score(lang: LANG, source: &str, name: &str) -> u64 {
        fn find(s: &FuncSpace, name: &str) -> Option<u64> {
            if s.name.as_deref() == Some(name) {
                return Some(s.metrics.cognitive.cognitive());
            }
            s.spaces.iter().find_map(|c| find(c, name))
        }
        let root = space_verbatim(lang, source.as_bytes(), MetricsOptions::default());
        find(&root, name)
            .unwrap_or_else(|| panic!("{lang:?}: no space named {name:?} in the fixture"))
    }

    /// `(language, source, construct name, sibling method name)`. Each
    /// fixture nests a class two `if`s deep and gives it both the
    /// nameless construct and an ordinary method with a byte-identical
    /// body.
    fn cases() -> Vec<(LANG, &'static str, &'static str, &'static str)> {
        vec![
            (
                LANG::Kotlin,
                "fun outer(a: Boolean) { if (a) { if (a) { class D {\n\
                 \x20   var q: Int = 0\n\
                 \x20       get() { if (q > 0) { if (q > 1) { return 2 } }; return 0 }\n\
                 \x20   fun m(): Int { if (q > 0) { if (q > 1) { return 2 } }; return 0 }\n\
                 } } } }\n",
                "<get>",
                "m",
            ),
            (
                LANG::Java,
                "class K { void outer(boolean a) { if (a) { if (a) { class D {\n\
                 \x20   static int x;\n\
                 \x20   static { if (x > 0) { if (x > 1) { x = 2; } } }\n\
                 \x20   void m() { if (x > 0) { if (x > 1) { x = 2; } } }\n\
                 } } } } }\n",
                "<static-init>",
                "m",
            ),
            (
                LANG::Javascript,
                "function outer(a) { if (a) { if (a) { class D {\n\
                 \x20   static x;\n\
                 \x20   static { if (D.x > 0) { if (D.x > 1) { D.x = 2; } } }\n\
                 \x20   m() { if (D.x > 0) { if (D.x > 1) { D.x = 2; } } }\n\
                 } } } }\n",
                "<static-init>",
                "m",
            ),
        ]
    }

    #[test]
    fn a_nested_nameless_construct_scores_like_a_sibling_method() {
        let mut checked = 0;
        for (lang, source, construct, method) in cases() {
            if !lang.is_enabled() {
                continue;
            }
            checked += 1;
            let (got, want) = (score(lang, source, construct), score(lang, source, method));
            assert_eq!(
                got, want,
                "{lang:?}: {construct} scored {got} where the sibling method scored {want}; \
                 the construct is inheriting the enclosing nesting",
            );
            // expected: two `if`s at +1 and +2 = 3, plus +1 each for the
            // function-depth surcharge from `outer` = 5. Pinned so a
            // regression that moved both sides equally still fails.
            assert_eq!(want, 5, "{lang:?}: the baseline itself moved");
        }
        assert!(
            checked > 0,
            "no language enabled; this test asserted nothing"
        );
    }
}
