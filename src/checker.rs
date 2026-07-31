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
        $node:ident,
        $ancestors:ident,
        [$($up:ident)|+],
        [$($stop:ident)|+],
        $extra:expr $(,)?
    ) => {
        // `Self` is the enclosing `impl Checker for <Lang>Code` type, so
        // the ancestor walk's `is_else_if` filter is bound directly to the
        // language's own `Checker` (no `ParserTrait` round-trip).
        //
        // `$ancestors` is the chain the caller descended through, so each
        // step is a slice index. The walk stops at the first frame that
        // proves positional use — an enclosing block, `return`, `new`, or
        // call — so on ordinary JS it takes one or two steps. That is
        // what made the pre-#1088 cost surprising: `Node::parent` is
        // `O(depth)` per step whatever the count, so even a two-step walk
        // was quadratic over a deeply nested file, which is what the
        // `nom/nested-arrow` probe measures.
        //
        // A chain of *expression*-bodied arrows (`a => b => c => …`) is
        // the one shape no chain can flatten: no `$stop` kind intervenes,
        // so the walk itself is `O(depth)` long. The chain takes it from
        // `O(depth^2)` per node to `O(depth)`; it stays quadratic overall
        // and would need a different rule, not a cheaper lookup, to fix.
        $node.count_specific_ancestors::<Self>(
            $ancestors,
            |node| matches!(node.kind_id().into(), $($up)|+),
            |node| matches!(node.kind_id().into(), $($stop)|+),
        ) > 0
            || $extra
    };
}

macro_rules! check_if_func {
    ($node: ident, $ancestors: ident) => {
        js_ancestor_walk!(
            $node,
            $ancestors,
            [VariableDeclarator | AssignmentExpression | LabeledStatement | Pair],
            [StatementBlock | ReturnStatement | NewExpression | Arguments],
            $node.is_child(Identifier as u16),
        )
    };
}

macro_rules! check_if_arrow_func {
    ($node: ident, $ancestors: ident) => {
        js_ancestor_walk!(
            $node,
            $ancestors,
            [VariableDeclarator | AssignmentExpression | LabeledStatement],
            [StatementBlock | ReturnStatement | NewExpression | CallExpression],
            $node.has_sibling($ancestors, PropertyIdentifier as u16),
        )
    };
}

macro_rules! is_js_func {
    ($node: ident, $ancestors: ident) => {
        match $node.kind_id().into() {
            FunctionDeclaration | MethodDefinition => true,
            FunctionExpression => check_if_func!($node, $ancestors),
            ArrowFunction => check_if_arrow_func!($node, $ancestors),
            _ => false,
        }
    };
}

macro_rules! is_js_closure {
    ($node: ident, $ancestors: ident) => {
        match $node.kind_id().into() {
            GeneratorFunction | GeneratorFunctionDeclaration => true,
            FunctionExpression => !check_if_func!($node, $ancestors),
            ArrowFunction => !check_if_arrow_func!($node, $ancestors),
            _ => false,
        }
    };
}

macro_rules! is_js_func_and_closure_checker {
    ($language: ident) => {
        #[inline]
        fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
            use $language::*;
            is_js_func!(node, ancestors)
        }

        #[inline]
        fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
            use $language::*;
            is_js_closure!(node, ancestors)
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

// Generate an `is_else_if` impl for the grammars that model `else if` as
// an `$if_kind` node nested directly inside an `else_clause` wrapper
// (`else_clause → if`). Covers C++/JS/TS/TSX (`IfStatement`) and Rust
// (`IfExpression`). Per-call rationale (e.g. the `IfExpression` choice)
// is hoisted above each invocation per `.claude/rules/macro-comments.md`.
macro_rules! impl_is_else_if_parent_clause {
    ($lang:ident, $if_kind:ident, $else_clause:ident) => {
        #[inline]
        fn is_else_if<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
            node.kind_id() == $lang::$if_kind
                && ancestors
                    .parent(node)
                    .is_some_and(|parent| parent.kind_id() == $lang::$else_clause)
        }
    };
}

// Generate an `is_else_if` impl for the grammars that emit a bare `else`
// keyword token followed by a nested `$if_kind` sibling, with no wrapping
// clause node. Covers Java/C#/Groovy (`IfStatement`) and Kotlin
// (`IfExpression`).
macro_rules! impl_is_else_if_prev_sibling {
    ($lang:ident, $if_kind:ident, $else_kw:ident) => {
        #[inline]
        fn is_else_if<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
            node.kind_id() == $lang::$if_kind
                && ancestors
                    .previous_sibling(node)
                    .is_some_and(|prev| prev.kind_id() == $lang::$else_kw)
        }
    };
}

// Generate an `is_else_if` impl for the grammars that expose a dedicated
// elseif/elsif clause node, so the clause node itself is the else-if
// (no `if`-nesting or sibling-token shape to inspect). Flat `matches!`
// against one or more variant kinds, analogous to `impl_simple_is_string!`;
// the multi-variant form covers PHP's `ElseIfClause | ElseIfClause2`.
macro_rules! impl_is_else_if_clause {
    ($lang:ident, $first:ident $(, $rest:ident)* $(,)?) => {
        #[inline]
        fn is_else_if(node: &Node, _ancestors: Ancestors<'_, '_>) -> bool {
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
        .get_or_init(|| {
            AhoCorasick::new(vec![b"<div rustbindgen"])
                .expect("constant single-needle AhoCorasick automaton always compiles")
        })
        .is_match(code)
}

/// Per-language AST classification predicates the metric walkers use to
/// recognize comments, function spaces, calls, strings, branches, and so
/// on by node kind.
///
/// Every predicate defaults to `false` — "this node is never of that
/// category." A language implements only the predicates its grammar
/// actually expresses; categories it lacks fall through to the default,
/// so adding a language no longer requires copy-pasting `-> false` stubs.
/// The flip side is that the compiler cannot flag a forgotten override, so
/// each language's per-metric tests are the safety net for completeness.
#[doc(hidden)]
pub(crate) trait Checker {
    #[inline]
    fn is_comment(_: &Node) -> bool {
        false
    }
    /// Whether this comment must survive `strip_comments`.
    ///
    /// `ancestors` is the chain the comment-removal walk descended
    /// through. Rust's impl reads the parent to keep a comment that is
    /// a macro token; reaching it with [`Node::parent`] costs
    /// `O(depth)` per comment (#1096).
    #[inline]
    fn is_useful_comment<'a>(_: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
        false
    }
    #[inline]
    fn is_func_space(_: &Node) -> bool {
        false
    }
    /// Whether `node` is a *named* function definition.
    ///
    /// `ancestors` is the chain the caller descended through. The
    /// JS-family grammars have no distinct production for a named
    /// function: `const f = () => …` and `g(() => …)` share one
    /// `arrow_function` node, and only what encloses it says which is
    /// which. Those steps come off the chain, because `Node::parent`
    /// costs `O(depth)` each however few of them a walk takes (#1088).
    /// Every other grammar answers `is_func` from the node's own kind
    /// and ignores the parameter.
    #[inline]
    fn is_func<'a>(_: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        false
    }
    /// Whether `node` is an anonymous function — the complement of
    /// [`is_func`] on the grammars that express both. Takes `ancestors`
    /// for the same reason, plus one of its own: Ruby's `{ … }` block is
    /// a closure only when its parent is not the `Lambda` that already
    /// counted it.
    #[inline]
    fn is_closure<'a>(_: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        false
    }
    #[inline]
    fn is_call(_: &Node) -> bool {
        false
    }
    #[inline]
    fn is_non_arg(_: &Node) -> bool {
        false
    }
    #[inline]
    fn is_string(_: &Node) -> bool {
        false
    }
    /// Whether `node` is the `if` that continues an `else if` chain,
    /// rather than a freshly nested branch.
    ///
    /// `ancestors` is the chain the caller descended through; the
    /// grammars that model `else if` as nesting need the parent (or the
    /// preceding `else` token) to tell the two apart, and resolving
    /// either from the node alone costs `O(depth)` (#1084). Languages
    /// with a dedicated `elsif` clause node ignore it.
    #[inline]
    fn is_else_if(_: &Node, _: Ancestors<'_, '_>) -> bool {
        false
    }
    #[inline]
    fn is_primitive(_node: &Node) -> bool {
        false
    }

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
    ///
    /// `ancestors` is the chain the caller descended through. Rust's
    /// override needs the parent to read the run of `#[…]` siblings
    /// before an item, and resolving siblings from the node alone
    /// costs `O(depth)` per step (#1100).
    #[inline]
    fn should_skip_subtree<'a>(
        _node: &Node<'a>,
        _code: &[u8],
        _ancestors: Ancestors<'a, '_>,
    ) -> bool {
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
    ///
    /// `ancestors` is the chain the caller descended through, so a
    /// language whose classification depends on an enclosing construct
    /// (Elixir's `quote` template) can answer without paying
    /// `Node::parent`'s `O(depth)` per step (#1084).
    #[inline]
    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        _code: &[u8],
        _ancestors: Ancestors<'a, '_>,
    ) -> bool {
        Self::is_func_space(node)
    }

    /// Source-aware variant of [`is_func`]. Same rationale as
    /// [`is_func_space_with_code`] (#275).
    #[inline]
    fn is_func_with_code<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        Self::is_func(node, ancestors)
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
    fn promotes_to_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        Self::is_func_with_code(node, code, ancestors)
            || Self::is_func_space_with_code(node, code, ancestors)
    }
}

mod bash;
mod c;
mod ccomment;
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
mod preproc;
mod python;
mod ruby;
mod rust;
mod tcl;
mod tsx;
mod typescript;

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

/// Classifies one attribute node's body, given the `#` / `#!` marker
/// its kind carries. Returns `false` for a node whose text is not
/// valid UTF-8 or does not have the `marker[...]` shape.
fn rust_attribute_node_marks_test(node: &Node, code: &[u8], marker: &str) -> bool {
    node.utf8_text(code)
        .and_then(|text| rust_attribute_body(text, marker))
        .is_some_and(rust_attribute_marks_test)
}

fn rust_item_is_test_only<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    rust_outer_attr_marks_test(node, code, ancestors) || rust_inner_attr_marks_test(node, code)
}

/// Children a parent may have before reading its child list costs more
/// than resolving the sibling from `node` itself.
///
/// The two scans below grow on different axes and neither dominates.
/// [`Node::previous_sibling`] resolves the parent by descending from
/// the root, so its cost tracks the node's *depth*; a cursor pass over
/// the parent's children tracks the parent's *width*. Measured on this
/// workspace's grammar build, one cursor sibling step costs ~68 ns
/// against ~420 ns for a whole `previous_sibling` on a shallow node —
/// and that 420 ns did not move with the sibling index, so the
/// break-even sits near six children.
///
/// Cutting at the *shallow* break-even is the conservative choice: the
/// backward walk only gets more expensive with depth, so a parent
/// narrow enough to read forward at depth 1 is still worth reading
/// forward at depth 1_000. That makes the forward scan a strict
/// improvement where it applies (#1100) and leaves a generated file of
/// thousands of top-level items on exactly the walk it had before.
const MAX_FORWARD_ATTRIBUTE_SCAN_CHILDREN: usize = 6;

// The tree-sitter Rust grammar exposes outer attributes (`#[...]`) as
// `AttributeItem` siblings *before* the decorated item, so what marks
// `node` test-only is the run of attributes immediately preceding it —
// a non-attribute sibling (another item, a comment) ends the run. This
// scan covers every item kind, including `mod_item`, so
// `#[cfg(test)] mod tests` (an outer attribute on the module) is caught
// here while `mod tests { #![cfg(test)] }` is caught by the inner scan.
//
// The two readings agree on every node — `rust_outer_attr_scans_agree`
// pins that — so this only picks the cheaper one.
fn rust_outer_attr_marks_test<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    match ancestors.parent(node) {
        Some(parent) if parent.child_count() <= MAX_FORWARD_ATTRIBUTE_SCAN_CHILDREN => {
            rust_attribute_run_under(&parent, node, code)
        }
        _ => rust_attribute_run_before(node, code),
    }
}

/// Reads the attribute run ending at `node` forward from `parent`,
/// which the walker already holds. `O(parent.child_count())`, and
/// independent of how deep `node` sits.
///
/// Two passes and not one: classifying each attribute as the first pass
/// goes by would classify every attribute in the parent rather than
/// only the adjacent run, and classification is itself `O(len)` in the
/// attribute body (#1105).
fn rust_attribute_run_under(parent: &Node, node: &Node, code: &[u8]) -> bool {
    let mut run_start = None;
    for child in parent.children() {
        if child.id() == node.id() {
            break;
        }
        if child.kind_id() == Rust::AttributeItem {
            if run_start.is_none() {
                run_start = Some(child);
            }
        } else {
            run_start = None;
        }
    }
    // No preceding attribute — and also the shape a `parent` that is
    // not `node`'s parent lands in, since the first pass then ends at
    // the parent's own trailing run rather than at `node`. The walker
    // constructs its chain through `Ancestors::checked`, so that is a
    // caller error rather than a shape to answer for.
    let Some(run_start) = run_start else {
        return false;
    };
    parent
        .children()
        .skip_while(|child| child.id() != run_start.id())
        .take_while(|child| child.id() != node.id() && child.kind_id() == Rust::AttributeItem)
        .any(|attribute| rust_attribute_node_marks_test(&attribute, code, "#"))
}

/// Reads the same run backward from `node`. Each step resolves the
/// parent by descending from the root, so this is `O(attributes x
/// depth)` — but it does not touch the siblings it does not need,
/// which is what makes it the cheaper reading under a wide parent.
fn rust_attribute_run_before(node: &Node, code: &[u8]) -> bool {
    let mut sibling = node.previous_sibling();
    while let Some(attribute) = sibling {
        if attribute.kind_id() != Rust::AttributeItem {
            break;
        }
        if rust_attribute_node_marks_test(&attribute, code, "#") {
            return true;
        }
        sibling = attribute.previous_sibling();
    }
    false
}

// `mod_item` additionally accepts inner attributes (`#![cfg(test)]`).
// The grammar nests these inside the module's `declaration_list` body,
// not as direct `mod_item` children, so descend one level via the
// `body` field before scanning. Non-module items have no inner-attribute
// test form, so this returns `false` for them immediately.
fn rust_inner_attr_marks_test(node: &Node, code: &[u8]) -> bool {
    node.kind_id() == Rust::ModItem
        && node.child_by_field_name("body").is_some_and(|body| {
            body.children()
                .filter(|child| child.kind_id() == Rust::InnerAttributeItem)
                .any(|attribute| rust_attribute_node_marks_test(&attribute, code, "#!"))
        })
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
        BashParser, JavascriptCode, JavascriptParser, MozjsParser, PhpParser, TsxParser,
        TypescriptParser,
    };
    use crate::test_support::for_each_node_with_chain;
    use std::fmt::Write as _;
    use std::path::PathBuf;

    fn parse(source: &str) -> BashParser {
        BashParser::new(source.as_bytes().to_vec(), &PathBuf::from("test.sh"), None)
    }

    fn count_strings(source: &str) -> usize {
        count(&parse(source), &["string".to_string()]).0
    }

    // `count`'s filter parser accepts a numeric string as a `kind_id` match
    // (parser.rs `filters`), so `has_kind` reuses the same primitive.
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
        let mut stack = vec![parser.root()];
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
        let mut stack = vec![parser.root()];
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
        let mut stack = vec![parser.root()];
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
        let code = parser.code();
        let node = find_first_kind(&parser, Rust::ModItem as u16).expect("mod_item");

        assert!(rust_item_is_test_only(&node, code, Ancestors::unknown()));
        // The marker lives on the outer scan; the inner scan sees no
        // `#![cfg(test)]` and must report false.
        assert!(rust_outer_attr_marks_test(
            &node,
            code,
            Ancestors::unknown()
        ));
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
        let code = parser.code();
        let node = find_first_kind(&parser, Rust::ModItem as u16).expect("mod_item");

        assert!(rust_item_is_test_only(&node, code, Ancestors::unknown()));
        assert!(!rust_outer_attr_marks_test(
            &node,
            code,
            Ancestors::unknown()
        ));
        assert!(rust_inner_attr_marks_test(&node, code));
    }

    /// A plain, unattributed item is not test-only — neither scan matches.
    #[test]
    fn rust_plain_item_is_not_test_only() {
        let src = "fn foo() {}\n";
        let parser = RustParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.rs"), None);
        let code = parser.code();
        let node = find_first_kind(&parser, Rust::FunctionItem as u16).expect("function_item");

        assert!(!rust_item_is_test_only(&node, code, Ancestors::unknown()));
        assert!(!rust_outer_attr_marks_test(
            &node,
            code,
            Ancestors::unknown()
        ));
        assert!(!rust_inner_attr_marks_test(&node, code));
    }

    /// Reading the `#[…]` run forward from the parent must answer
    /// exactly what walking backward from the item answers, on every
    /// node of every attribute shape.
    ///
    /// This is the acceptance criterion for #1100, not a nicety:
    /// `rust_outer_attr_marks_test` picks between the two by the
    /// parent's width, so a disagreement would make `exclude_tests`
    /// prune a different set of subtrees on one shape than on another
    /// and move published metric values. The comparison is against the
    /// backward reading directly rather than against the dispatcher,
    /// which would compare the backward walk with itself on every wide
    /// parent.
    ///
    /// The counted `marked` / `unmarked` totals are what stop it from
    /// passing vacuously — two readings that both answered `false`
    /// everywhere would agree perfectly.
    #[test]
    fn rust_outer_attr_scans_agree() {
        // One attribute run longer than the file's actual code, so the
        // run length cannot be confused with the child count.
        let long_run = format!("{}fn f() {{}}\n", "#[allow(dead_code)]\n".repeat(64));
        // A parent with hundreds of children: the shallow-but-wide
        // shape a per-step sibling scan would have been worst on.
        let mut wide = String::new();
        for i in 0..200 {
            let _ = writeln!(wide, "#[inline] fn f{i}() {{}}");
        }
        let sources = [
            // Attribute run opening the file, so the marked item is
            // its parent's first non-attribute child.
            "#[cfg(test)]\nfn a() {}\n",
            // Every recognised marker form, plus the `not(test)` form
            // that must *not* mark (#278).
            "#[test]\nfn a() {}\n#[tokio::test]\nfn b() {}\n\
             #[cfg(all(test, feature = \"x\"))]\nfn c() {}\n\
             #[cfg(not(test))]\nfn d() {}\n",
            // A run whose marker is not the attribute adjacent to the
            // item, the same run reversed, and one where an
            // intervening comment ends the run. Both orders matter:
            // only the second distinguishes a scan that stops at the
            // node it was asked about from one that reads the whole
            // run, since the query node here is itself an attribute.
            "#[cfg(test)]\n#[allow(dead_code)]\nfn a() {}\n\
             #[allow(dead_code)]\n#[cfg(test)]\nfn b() {}\n\
             #[cfg(test)]\n// break\nfn c() {}\n",
            // Nested modules, inner attributes, and an attributed item
            // inside a function body — parents other than the root.
            "mod outer {\n#![cfg(test)]\nmod inner {\n#[test]\nfn a() {}\n}\n}\n\
             fn b() {\n#[cfg(test)]\nfn c() {}\nlet x = 1;\n}\n",
            // A bare item: no parent for the root, no preceding
            // sibling for the item.
            "fn a() {}\n",
            &long_run,
            &wide,
        ];

        let (mut marked, mut unmarked, mut widened) = (0_usize, 0_usize, 0_usize);
        for source in sources {
            let code = source.as_bytes();
            let visited = for_each_node_with_chain::<RustCode>(code, |node, chain| {
                let Some(parent) = chain.last() else {
                    return; // The root has no parent to read forward from.
                };
                let backward = rust_attribute_run_before(node, code);
                assert_eq!(
                    rust_attribute_run_under(parent, node, code),
                    backward,
                    "outer-attribute readings disagree on {} at row {}",
                    node.kind(),
                    node.start_row(),
                );
                assert_eq!(
                    rust_outer_attr_marks_test(node, code, Ancestors::known(chain)),
                    backward,
                    "the width dispatch changed the answer on {} at row {}",
                    node.kind(),
                    node.start_row(),
                );
                if backward {
                    marked += 1;
                } else {
                    unmarked += 1;
                }
                if parent.child_count() > MAX_FORWARD_ATTRIBUTE_SCAN_CHILDREN {
                    widened += 1;
                }
            });
            assert!(visited > 0, "fixture must have nodes to compare");
        }
        assert!(
            marked > 0 && unmarked > 0,
            "the comparison saw only one answer ({marked} marked, {unmarked} unmarked), \
             so agreeing proves nothing"
        );
        // The `wide` fixture exists to drive the dispatch's other arm;
        // without a node over the width cut, the second assertion above
        // would only ever compare the backward walk with itself.
        assert!(widened > 0, "no fixture exceeded the forward-scan width");
    }

    /// The same equivalence one level up, on the hook the walker calls.
    ///
    /// `rust_item_is_test_only` folds the inner-attribute scan in, and
    /// `should_skip_subtree` adds the item-kind filter, so pinning it
    /// here is what carries the predicate-level agreement above through
    /// to the set of subtrees `exclude_tests` actually prunes.
    #[test]
    fn rust_should_skip_subtree_matches_the_backward_reading() {
        let source = "#[cfg(test)]\nmod tests {\nfn a() {}\n}\n\
                      #[allow(dead_code)]\nstatic S: i32 = 1;\n\
                      mod inner {\n#![cfg(test)]\nconst C: i32 = 1;\n}\n\
                      #[rstest]\nfn b() {}\nfn c() {}\n";
        let code = source.as_bytes();
        let mut pruned = 0_usize;
        let visited = for_each_node_with_chain::<RustCode>(code, |node, chain| {
            let reference =
                rust_attribute_run_before(node, code) || rust_inner_attr_marks_test(node, code);
            // Spelled out rather than reused from the production hook:
            // the point is to pin which kinds the prune considers, so
            // borrowing the production `matches!` would assert nothing.
            let is_item = matches!(
                node.kind_id().into(),
                Rust::ModItem
                    | Rust::FunctionItem
                    | Rust::ImplItem
                    | Rust::TraitItem
                    | Rust::ConstItem
                    | Rust::StaticItem
            );
            let skipped = RustCode::should_skip_subtree(node, code, Ancestors::known(chain));
            assert_eq!(
                skipped,
                is_item && reference,
                "prune decision moved on {} at row {}",
                node.kind(),
                node.start_row(),
            );
            pruned += usize::from(skipped);
        });
        assert!(visited > 0, "fixture must have nodes to compare");
        // `mod tests`, `mod inner`, and `fn b` — the three test-only
        // items, and no more: a hook that pruned everything would
        // satisfy the equality above only if the oracle agreed, but
        // this pins the count the fixture was written for.
        assert_eq!(pruned, 3, "expected exactly the three test-only items");
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
            GroovyCode::is_else_if(&inner, Ancestors::unknown()),
            "inner if_statement after `else` must be recognised as else-if"
        );
        assert!(
            !GroovyCode::is_else_if(&outer, Ancestors::unknown()),
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
        assert!(!GroovyCode::is_else_if(&node, Ancestors::unknown()));
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
        let mut stack = vec![parser.root()];
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
            PythonCode::is_else_if(&inner, Ancestors::unknown()),
            "if_statement inside else_clause's block must be recognised as else-if"
        );
        assert!(
            !PythonCode::is_else_if(&outer, Ancestors::unknown()),
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
        assert!(!PythonCode::is_else_if(&node, Ancestors::unknown()));
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
        assert!(!PythonCode::is_else_if(&outer, Ancestors::unknown()));
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
            !PythonCode::is_else_if(&inner, Ancestors::unknown()),
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
            PythonCode::is_closure(&lambda, Ancestors::unknown()),
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

    /// The two `is_else_if` implementations that answer a flat `false`
    /// are a behavioural claim, not a placeholder: the language has no
    /// `else if` *chain* to collapse, so every `if` it contains is a
    /// freshly nested branch and pays cognitive nesting.
    ///
    /// Neither is reachable from a metric walk — `Preproc` and
    /// `Ccomment` (which inherit the `Checker` default) have no `if`
    /// construct at all, and Elixir's branching is `cond do` or nested
    /// `if/else` — so nothing else in the suite executes them. #1084
    /// changed both signatures to take `Ancestors`, which is exactly the
    /// kind of edit that can quietly invert a one-line predicate, so pin
    /// the contract directly.
    #[test]
    fn languages_without_else_if_chains_answer_false() {
        // The `Checker` default, via the two grammars that do not
        // override it.
        let preproc = PreprocParser::new(b"#define A 1\n".to_vec(), &PathBuf::from("a.h"), None);
        let ccomment = CcommentParser::new(b"/* c */\n".to_vec(), &PathBuf::from("a.c"), None);
        assert!(
            !PreprocCode::is_else_if(&preproc.root(), Ancestors::unknown()),
            "the Checker default must answer false"
        );
        assert!(
            !CcommentCode::is_else_if(&ccomment.root(), Ancestors::unknown()),
            "the Checker default must answer false"
        );

        // Elixir's explicit override, asserted on a real `if` node so the
        // test would catch an impl that started consulting the ancestor
        // chain and got the answer wrong.
        let parser = ElixirParser::new(
            b"defmodule M do\n  def f(a) do\n    if a do\n      if a do\n        :ok\n      end\n    end\n  end\nend\n".to_vec(),
            std::path::Path::new("m.ex"),
            None,
        );
        let ifs: Vec<_> = parser
            .root()
            .preorder()
            .filter(|n| {
                n.kind() == "call"
                    && n.utf8_text(parser.code())
                        .is_some_and(|t| t.starts_with("if"))
            })
            .collect();
        assert!(
            ifs.len() >= 2,
            "fixture must contain a nested if, else the assertion below is vacuous"
        );
        for node in &ifs {
            assert!(
                !ElixirCode::is_else_if(node, Ancestors::unknown()),
                "Elixir has no else-if chain; every if is a fresh branch"
            );
        }
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
    // walks every language consolidated under `impl_simple_is_string!`
    // with identical per-language shape (parse → count → assert_eq! 0).
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
            IrulesParser, JavaParser, KotlinParser, LuaParser, PerlParser, PreprocParser,
            PythonParser, RubyParser, RustParser, TclParser,
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

        // ---- iRules (3 variants): QuotedWord, BracedWord, BracedWordSimple ----
        // Same Tcl-family braced-word split as Tcl above: the `proc`
        // body is a named `braced_word` (BracedWord), the simple `set`
        // assignment is a `braced_word_simple` (BracedWordSimple), and
        // the quoted literal is a `QuotedWord`.
        let src = b"set a \"quoted\"\nset b {braced}\nproc p {x y} { return $x }\n".to_vec();
        let parser = IrulesParser::new(src, &path, None);
        assert_variants_is_string!(
            &parser,
            Irules,
            IrulesCode,
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
            GroovyParser, IrulesParser, JavaParser, KotlinParser, LuaParser, PerlParser,
            PreprocParser, PythonParser, RubyParser, RustParser, TclParser,
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
        // iRules: same Tcl-family `set` of an unquoted identifier word —
        // the bareword surfaces as `Word`, not any string kind.
        assert_no_string_matches!(IrulesParser, &path, b"set x $y\n", "Irules");
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

    #[test]
    fn go_rune_literal_is_not_a_string() {
        // #699 verdict: Go `RuneLiteral` is an operand in `get_op_type`
        // and is flattened by the alterator, but it is deliberately
        // excluded from `is_string` — a rune is a character (`int32`),
        // not a string, exactly as Rust/Cpp `CharLiteral` are operand +
        // flattened yet absent from their `is_string`. This pins the
        // exclusion: the fixture must produce a `rune_literal` (proving
        // the surrounding grammar still emits it) while `GoCode::is_string`
        // returns false for it. The flattening half lives in
        // `alterator.rs::go_rune_literal_flattened_but_not_a_string_kind`.
        use crate::langs::GoParser;
        let path = PathBuf::from("test.go");
        let src = b"package main\nfunc main() { r := 'x'; _ = r }\n".to_vec();
        let parser = GoParser::new(src, &path, None);
        let rune_id = Go::RuneLiteral as u16;
        assert!(
            ast_has_kind_id(&parser, rune_id),
            "fixture should produce a rune_literal node",
        );
        assert_eq!(
            count_string_matches_for_kind(&parser, rune_id, GoCode::is_string),
            0,
            "Go rune_literal must not match is_string (a rune is a char, not a string)",
        );
    }

    /// Asserts `is_func` / `is_closure` answer the same off a known
    /// chain as off a `Node::parent` climb, for every node of `code`.
    /// Returns the `(functions, closures)` the chain path counted, so a
    /// caller can additionally pin what the predicates actually decided.
    ///
    /// Both predicates fold their ancestor lookups into one boolean, so
    /// a wrong ancestor does not fail — it silently moves a function
    /// into the closure column. The counters keep a fixture honest: both
    /// answers have to occur, for both predicates, or the parity holds
    /// only over nodes the predicates never classify.
    fn assert_func_parity<L: crate::traits::LanguageInfo + Checker>(
        label: &str,
        code: &[u8],
    ) -> (usize, usize) {
        let mut funcs = 0;
        let mut closures = 0;
        let visited = for_each_node_with_chain::<L>(code, |node, chain| {
            let known = Ancestors::known(chain);
            let climbing = Ancestors::unknown();
            assert_eq!(
                L::is_func(node, known),
                L::is_func(node, climbing),
                "{label}: is_func disagrees on {} at row {}",
                node.kind(),
                node.start_row()
            );
            assert_eq!(
                L::is_closure(node, known),
                L::is_closure(node, climbing),
                "{label}: is_closure disagrees on {} at row {}",
                node.kind(),
                node.start_row()
            );
            funcs += usize::from(L::is_func(node, known));
            closures += usize::from(L::is_closure(node, known));
        });
        assert!(visited > 20, "{label}: fixture is too small to prove much");
        assert!(
            funcs > 0 && closures > 0,
            "{label}: fixture must contain both a named function and a closure, \
             else parity holds over nodes the walk never classifies: \
             {funcs} functions, {closures} closures"
        );
        (funcs, closures)
    }

    /// The JS-family `is_func` / `is_closure` must classify a node the
    /// same whether they read the walker's ancestor chain or climb with
    /// `Node::parent` (#1088).
    ///
    /// These are the predicates with the longest ancestor lookup: an
    /// upward walk, its `is_else_if` filter, and a `has_sibling`
    /// adjacency check.
    #[test]
    fn js_func_and_closure_agree_between_known_and_climbing() {
        // One of each shape the walk distinguishes: an arrow bound to a
        // name, an arrow passed positionally, a `function` expression
        // named by an object `pair`, and a nested arrow whose only
        // enclosing frame is a `statement_block`.
        //
        // The returned object literal is load-bearing. It is the one
        // shape here where the trailing `has_sibling` adjacency check
        // decides the answer: the upward walk stops at the `return`
        // having counted no binding, so whether the arrow is a *named*
        // function rests entirely on the `property_identifier` beside
        // it. Everywhere else a binding is found first and the adjacency
        // check is never reached, which would leave a wrong parent
        // lookup invisible.
        let code = concat!(
            "const f = a => { g(() => 1); a => 2; };\n",
            "const o = { m: function () { return 1; } };\n",
            "function h() { return { k: (a) => a + 1 }; }\n",
            "function i() { return function () { return 2; }; }\n",
        )
        .as_bytes();
        // `is_js_func_and_closure_checker!` expands separately against
        // each grammar's own `kind_id` enum, so the four impls are four
        // distinct variant lists rather than one shared body. Checking
        // only JavaScript would leave a drifted id in any of the other
        // three uncovered.
        assert_func_parity::<crate::langs::JavascriptCode>("javascript", code);
        assert_func_parity::<crate::langs::MozjsCode>("mozjs", code);
        assert_func_parity::<crate::langs::TypescriptCode>("typescript", code);
        assert_func_parity::<crate::langs::TsxCode>("tsx", code);
    }

    /// Ruby's `is_closure` is the one non-JS predicate that needs an
    /// ancestor: a `{ … }` block is a closure unless its parent is the
    /// `Lambda` that already counted it (#465), so it reads the chain
    /// for the same reason the JS family does (#1088).
    ///
    /// The stabby lambda is the discriminating shape. Its body block is
    /// the only node here whose answer depends on the parent lookup —
    /// every other block's parent is a `Call`, which the arm accepts
    /// whether the lookup is right or wrong.
    #[test]
    fn ruby_block_closure_agrees_between_known_and_climbing() {
        let code = concat!(
            "def m\n",
            "  f = ->(z) { z + 1 }\n",
            "  [1].each { |x| x }\n",
            "  lambda { 2 }\n",
            "end\n",
        )
        .as_bytes();
        let (funcs, closures) = assert_func_parity::<crate::langs::RubyCode>("ruby", code);
        assert_eq!(funcs, 1, "only `def m` is a named function");
        assert_eq!(
            closures, 3,
            "the stabby lambda, the `each` block and the `lambda` block \
             are three closures — the stabby lambda's own body block must \
             not add a fourth, which is the answer the parent lookup decides"
        );
    }
}
