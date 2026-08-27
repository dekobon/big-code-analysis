// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use std::borrow::Cow;

use crate::checker::Checker;
use crate::error::MetricsError;
use crate::getter::Getter;
use crate::node::{Ancestors, Node};
use crate::spaces::{SpaceKind, line_span, push_children};

use crate::halstead::{Halstead, HalsteadMaps};

use crate::traits::ParserTrait;

/// All operands and operators of a space.
#[derive(Debug, Clone)]
pub struct Ops {
    /// The name of a function space.
    ///
    /// For the top-level (file-level) `Ops` the value is whatever
    /// `Source::name` the caller supplied to the [`crate::Ast::ops`]
    /// seam — `Some` or `None`.
    ///
    /// For nested spaces, `None` means an error occurred in parsing the
    /// name of the function space from the AST.
    pub name: Option<String>,
    /// `true` when [`Ops::name`] was produced by lossy conversion (the
    /// original path contained non-UTF-8 bytes and was rendered using
    /// U+FFFD replacement characters). The explicit-name
    /// [`crate::Ast::ops`] seam never sets it, since a caller-supplied
    /// `String` name is UTF-8 by construction, so it is always `false`
    /// in current code paths. Retained as a wire field for forward
    /// compatibility; skipped from JSON output when `false` so existing
    /// schemas keep their shape.
    pub name_was_lossy: bool,
    /// The first line of a function space.
    pub start_line: usize,
    /// The last line of a function space.
    pub end_line: usize,
    /// The space kind.
    pub kind: SpaceKind,
    /// All subspaces contained in a function space.
    pub spaces: Vec<Ops>,
    /// The **distinct** operands of a space — the deduplicated Halstead
    /// operand vocabulary (`n2`), one entry per unique operand, not every
    /// occurrence. Sorted in byte-lexicographic order, so the same input
    /// always yields the same sequence (#1091).
    pub operands: Vec<String>,
    /// The **distinct** operators of a space — the deduplicated Halstead
    /// operator vocabulary (`n1`), one entry per unique operator, not
    /// every occurrence. Sorted in byte-lexicographic order, so the same
    /// input always yields the same sequence (#1091).
    pub operators: Vec<String>,
}

// Space nesting is caller-controlled, so the compiler-generated `Drop`
// glue would recurse once per level and abort the process on a deep tree
// (#1056). See [`crate::recursion`].
crate::recursion::impl_iterative_drop!(Ops, spaces);

impl Ops {
    /// Project this tree into its [`crate::wire::Ops`] form — the
    /// plain, `Deserialize`-capable record that defines the serialized
    /// shape.
    #[must_use]
    pub fn to_wire(&self) -> crate::wire::Ops {
        crate::wire::Ops::from(self)
    }

    fn new<'a, T: Getter>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        kind: SpaceKind,
    ) -> Self {
        let (start_position, end_position) = line_span(node, kind);
        // The top-level Unit's name is overwritten by `ops_inner` with the
        // caller-supplied name before returning, so computing it here is
        // wasted work. Non-top-level Unit spaces have no resolvable name, so
        // leaving `None` matches the documented "could not be resolved"
        // semantics rather than inventing the `<anonymous>` placeholder the
        // default getter returns. Other kinds keep the AST-derived name.
        // Mirrors the `SpaceKind::Unit` handling in `FuncSpace::new`.
        let name = (kind != SpaceKind::Unit)
            .then(|| T::get_func_space_name(node, code, ancestors).map(str::to_owned))
            .flatten();
        Self {
            name,
            name_was_lossy: false,
            spaces: Vec::new(),
            kind,
            start_line: start_position,
            end_line: end_position,
            operators: Vec::new(),
            operands: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct State<'a> {
    ops: Ops,
    halstead_maps: HalsteadMaps<'a>,
}

/// Pushes a synthetic `Unit` root onto the state stack when the grammar
/// hands us a non-`Unit` root.
///
/// Mirrors [`crate::spaces::push_synthetic_unit_root`] on the metrics
/// seam: some grammars (e.g. tree-sitter-lua / tree-sitter-mozcpp on
/// unparseable input) return an `ERROR` root that is not classified as a
/// function space, so without this push the walk would never open a
/// frame and `ops_inner` would return [`MetricsError::EmptyRoot`] for an
/// input where `metrics()` succeeds (issue #789). A `Unit` root needs no
/// wrapper, so nothing is pushed in that case.
fn push_synthetic_unit_root<T: ParserTrait>(
    state_stack: &mut Vec<State>,
    node: &Node,
    code: &[u8],
) {
    // `Ancestors::unknown()`: `node` is the tree root here, so it has
    // no ancestors to hand over either way.
    if T::Getter::get_space_kind_with_code(node, code, Ancestors::unknown()) != SpaceKind::Unit {
        state_stack.push(State {
            ops: Ops::new::<T::Getter>(node, code, Ancestors::unknown(), SpaceKind::Unit),
            halstead_maps: HalsteadMaps::new(),
        });
    }
}

// Space-kind classifications `ops_inner` has performed on this thread.
//
// The classification only ever reaches `Ops::new`, so running it on a
// node that opens no space is work thrown away — and nothing about the
// walk's *output* can tell the two apart. The counter is the
// observable: it reads one per space after the lookup was moved inside
// the `func_space` branch, and one per *node* before, which is what
// makes hoisting it back out a test failure rather than a silent
// regression (#1110).
crate::observation::counter!(space_kind_lookups);

/// Classifies a node that is about to open a function space.
///
/// Classification happens after the decision that a space opens, the
/// same way [`crate::spaces::compute`]'s `open_func_space` does it
/// (#522), and through the same source-aware classifier, so a space
/// both seams open carries the same [`SpaceKind`] in either walk.
/// The `_with_code` variant is what lets Elixir's macro-shaped
/// `defmodule` / `def` declarations — plain `Call` nodes distinguished
/// only by their target identifier text — come back as `Class` /
/// `Function` rather than `Unknown` (#275, #1130).
///
/// The lookup is a per-language `match` on the node's kind for most
/// grammars, but C#'s reaches a child scan for a bodied indexer or
/// property and Elixir's reads the `Call` target text and scans the
/// ancestor chain for an enclosing `quote` block, so it is not free on
/// every node either.
fn classify_space_kind<'a, T: ParserTrait>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> SpaceKind {
    space_kind_lookups::record();
    T::Getter::get_space_kind_with_code(node, code, ancestors)
}

/// Render a space's vocabulary: byte-lexicographically ordered, one
/// owned `String` per distinct entry.
///
/// # Order
///
/// `HashMap`'s hasher is randomly seeded per instance, so key order
/// differs between two runs — and even between two parses in one
/// process. Without a canonical order the same input renders and
/// serializes differently every time, which makes `bca ops` output
/// undiffable and unusable as a cache key (#1091).
///
/// Byte-lexicographic, not first-appearance, order: `finalize` merges
/// a child space's maps into its parent, so an insertion-ordered map
/// would give the parent "what it saw directly, then whatever each
/// child contributed" — a walk artifact that shifts when nesting
/// changes. Sorting is also stable across platforms and hasher
/// versions, which insertion order via a different map type is not.
///
/// # Why the sort runs before the `String`s exist
///
/// Every ancestor of a space re-renders that space's whole vocabulary:
/// `finalize` merges each child's Halstead maps into its parent, so a
/// parent's key set is a superset of every descendant's and an entry
/// nested `D` spaces deep is rendered `D + 1` times. Sorting the
/// borrowed keys first makes each swap a fat pointer rather than a
/// 24-byte `String`, and — because the `String`s are then allocated in
/// output order — it hands every later pass over them (the wire
/// projection, serialization, the `dump_ops` tree) a heap laid out in
/// the order it reads. Issue #1110 measured both effects.
///
/// # Non-UTF-8 keys
///
/// Tree-sitter sources are expected to be valid UTF-8; non-UTF-8 bytes
/// are replaced with the Unicode replacement character to keep the entry
/// visible (rather than silently dropping it or using a sentinel string
/// that could collide with a real identifier). That rendering is not
/// order-preserving — `b"\xffA"` sorts after `"\u{fffd}B"` by raw bytes
/// and before it once both are rendered — so a vocabulary that actually
/// lost bytes is re-sorted on the rendered text, which is the order the
/// pre-#1110 render-then-sort code produced. Valid UTF-8, which is every
/// key in practice, takes the single sort.
fn sorted_vocabulary(mut keys: Vec<&[u8]>) -> Vec<String> {
    keys.sort_unstable();
    let mut lossy = false;
    let mut rendered: Vec<String> = keys
        .into_iter()
        .map(|key| match String::from_utf8_lossy(key) {
            Cow::Borrowed(text) => text.to_owned(),
            Cow::Owned(text) => {
                lossy = true;
                text
            }
        })
        .collect();
    if lossy {
        rendered.sort_unstable();
    }
    rendered
}

fn compute_operators_and_operands<T: ParserTrait>(state: &mut State) {
    let maps = &state.halstead_maps;

    // Primitive-type operators live in a second map (keyed by text rather
    // than by token id), so the operator vocabulary is the concatenation
    // of both key sets.
    let operators = maps
        .operators
        .keys()
        .map(|k| T::Getter::get_operator_id_as_str(*k).as_bytes())
        .chain(maps.primitive_operators.keys().copied())
        .collect();

    state.ops.operators = sorted_vocabulary(operators);
    state.ops.operands = sorted_vocabulary(maps.operands.keys().copied().collect());
}

/// Close up to `diff_level` open spaces, folding each into its parent.
///
/// Only the states this pops get their vocabularies computed. The
/// bottom state is never popped here, so the root's vocabulary is built
/// once by [`ops_inner`] after the final drain — computing it on every
/// call would rebuild (and, since #1091, re-sort) the whole file's
/// vocabulary once per level-drop in the walk, and every result but the
/// last would be overwritten.
fn finalize<T: ParserTrait>(state_stack: &mut Vec<State>, diff_level: usize) {
    for _ in 0..diff_level {
        if state_stack.len() < 2 {
            break;
        }
        let mut state = state_stack
            .pop()
            .expect("state_stack verified to have len >= 2");
        let last_state = state_stack
            .last_mut()
            .expect("state_stack verified to have len >= 1 after pop");

        // Populate the child's ops from its HalsteadMaps before
        // recording it as a sub-space of the parent.
        compute_operators_and_operands::<T>(&mut state);

        // Merge child's Halstead maps into parent and record child space.
        last_state.halstead_maps.merge(&state.halstead_maps);
        last_state.ops.spaces.push(state.ops);
    }
}

/// Context the ops walk carries down the tree alongside each node.
///
/// A named pair rather than a `(usize, usize)`: the two counts advance
/// on different events and swapping them is silent — `level` only moves
/// at space boundaries while `depth` counts every AST step. Mirrors
/// [`crate::spaces::compute`]'s `Walk`, minus the comment flag this walk
/// has no use for.
#[derive(Clone, Copy)]
struct Walk {
    /// Nesting level, used to close op-spaces on the way back up.
    level: usize,
    /// AST depth — the number of ancestors this node has, so the root
    /// sits at `0`. Indexes the ancestor chain the walk maintains.
    depth: usize,
}

/// Explicit-name core of the operator/operand walk backing the
/// [`crate::Ast::ops`] `Source`-based seam. The top-level [`Ops::name`]
/// is whatever the caller passes in `name`; `name_was_lossy` is left at
/// its `false` default because an explicit `String` name is never lossy.
/// Mirrors [`crate::spaces::metrics_inner`].
pub(crate) fn ops_inner<T: ParserTrait>(
    parser: &T,
    name: Option<String>,
) -> Result<Ops, MetricsError> {
    let code = parser.code();
    let node = parser.root();
    let mut cursor = node.cursor();
    let mut stack = Vec::new();
    // Ancestor chain of the node currently being visited, root first,
    // maintained by the same truncate/push rule as
    // `spaces::compute::metrics_inner` (#1084).
    let mut chain: Vec<Node<'_>> = Vec::new();
    let mut state_stack: Vec<State> = Vec::new();
    let mut last_level = 0;

    // Mirror `metrics_inner`: wrap a non-`Unit` (e.g. `ERROR`) root in a
    // synthetic `Unit` frame so the walk always has a frame to populate.
    // Without this, an `ERROR`-root parse drains the state stack and
    // `ops_inner` returns `EmptyRoot` for inputs where `metrics()`
    // succeeds (issue #789).
    push_synthetic_unit_root::<T>(&mut state_stack, &node, code);

    stack.push((node, Walk { level: 0, depth: 0 }));

    while let Some((node, Walk { level, depth })) = stack.pop() {
        chain.truncate(depth);

        if level < last_level {
            finalize::<T>(&mut state_stack, last_level - level);
            last_level = level;
        }

        let ancestors = Ancestors::checked(&chain, &node);

        // Same predicate `spaces::compute::metrics_inner` opens on, so
        // the two walks agree on which nodes become spaces. The
        // byte-less `is_func || is_func_space` this replaced could not
        // see Elixir's macro-shaped declarations, which are `Call`
        // nodes identified by their target text, so `bca ops` opened no
        // space for a `defmodule` / `def` / `defp` / `defmacro` — only
        // for `Source` and an explicit `fn … -> … end` (#1130).
        let func_space = T::Checker::promotes_to_func_space_with_code(&node, code, ancestors);

        let new_level = if func_space {
            let kind = classify_space_kind::<T>(&node, code, ancestors);
            let state = State {
                ops: Ops::new::<T::Getter>(&node, code, ancestors, kind),
                halstead_maps: HalsteadMaps::new(),
            };
            state_stack.push(state);
            last_level = level + 1;
            last_level
        } else {
            level
        };

        if let Some(state) = state_stack.last_mut() {
            T::Halstead::compute(&node, code, ancestors, &mut state.halstead_maps);
        }

        chain.push(node);

        // Shared with `metrics_inner` (issue #969): `push_children` is
        // State-independent — it only moves the cursor over child nodes —
        // so unlike the local `finalize` / `push_synthetic_unit_root`
        // mirrors (which differ by `State` payload) it is reused directly
        // rather than duplicated. The source-order-then-reverse ordering
        // it encapsulates is load-bearing for suppression attribution.
        // The returned child slice is only useful to `metrics_inner`,
        // which seeds their cognitive nesting; `ops` just walks them.
        push_children(
            &mut cursor,
            &node,
            Walk {
                level: new_level,
                depth: depth + 1,
            },
            &mut stack,
        );
    }

    finalize::<T>(&mut state_stack, usize::MAX);

    // Reserved error path: `MetricsError::EmptyRoot` is unreachable
    // today because the synthetic Unit push above (and every supported
    // language's root being recognised as a `func_space`) keeps the
    // state stack non-empty for every input, including ERROR-root,
    // empty, whitespace-only, and comment-only sources — matching
    // `metrics_inner`. The `ok_or` is retained so a future walker change
    // that legitimately drains the stack surfaces a distinct error
    // variant rather than a bare `None`. See `MetricsError::EmptyRoot`
    // for the matching variant doc.
    let mut state = state_stack.pop().ok_or(MetricsError::EmptyRoot)?;
    // The root is the one state `finalize` never pops, so its vocabulary
    // is built here — once, from the fully-merged maps.
    compute_operators_and_operands::<T>(&mut state);
    state.ops.name = name;
    Ok(state.ops)
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
    use super::Ops;
    use crate::{Ast, LANG, Source};

    #[inline]
    fn check_ops(
        lang: LANG,
        source: &str,
        file: &str,
        correct_operators: &mut [&str],
        correct_operands: &mut [&str],
    ) {
        let mut trimmed_bytes = source.trim_end().trim_matches('\n').as_bytes().to_vec();
        trimmed_bytes.push(b'\n');
        let ops = Ast::parse(Source::new(lang, &trimmed_bytes).with_name(Some(file.to_owned())))
            .expect("language feature enabled")
            .ops()
            .expect("ops walk must yield a top-level Ops");

        let operators_str: Vec<&str> = ops.operators.iter().map(AsRef::as_ref).collect();
        let operands_str: Vec<&str> = ops.operands.iter().map(AsRef::as_ref).collect();

        // Only the *expectations* are sorted here: `Ops` is documented to
        // come back byte-lexicographically ordered (#1091), so comparing
        // against a sorted expectation without re-sorting the actual value
        // makes every `check_ops` caller an ordering regression test for
        // its language.
        correct_operators.sort_unstable();
        assert_eq!(&operators_str[..], correct_operators);

        correct_operands.sort_unstable();
        assert_eq!(&operands_str[..], correct_operands);
    }

    #[test]
    fn python_ops() {
        check_ops(
            LANG::Python,
            "if True:
                 a = 1 + 2",
            "foo.py",
            &mut ["if", "=", "+"],
            &mut ["True", "a", "1", "2"],
        );
    }

    #[test]
    fn perl_pattern_operations_render_as_source_spellings() {
        // #1314 classifies `s///` and `tr///` as Halstead operators.
        // They are *named* nodes rather than punctuation tokens, so the
        // `get_operator!` macro's fallback would render each kind's own
        // name — `substitution_pattern_s`, `transliteration_tr_or_y` —
        // into `bca ops`, which reads as a bug rather than as Perl.
        // `PerlCode::get_operator_id_as_str` is hand-written to map
        // them, and this is what pins that mapping: the counts alone
        // cannot see it.
        //
        // `y///` is a synonym of `tr///` and shares one kind, so the
        // two source spellings collapse to a single `tr///` entry —
        // asserted here by the *absence* of a `y///` row in an
        // exhaustive expectation, not by a negative assertion.
        //
        // The trailing `/pat/` keeps a pattern *value* in the fixture,
        // so the test also shows the split: the operation spellings are
        // operators while the value is an operand.
        check_ops(
            LANG::Perl,
            "$s =~ s/a/b/;\n$s =~ tr/c/d/;\n$s =~ y/e/f/;\n$t = /pat/;",
            "foo.pl",
            &mut ["$", ";", "=", "=~", "s///", "tr///"],
            &mut ["$s", "$t", "/pat/"],
        );
    }

    #[test]
    fn python_function_ops() {
        check_ops(
            LANG::Python,
            "def foo():
                 def bar():
                     def toto():
                        a = 1 + 1
                     b = 2 + a
                 c = 3 + 3",
            "foo.py",
            &mut ["def", "=", "+"],
            &mut ["foo", "bar", "toto", "a", "b", "c", "1", "2", "3"],
        );
    }

    #[test]
    fn cpp_ops() {
        check_ops(
            LANG::Cpp,
            "int a, b, c;
             float avg;
             avg = (a + b + c) / 3;",
            "foo.c",
            &mut ["int", "float", "()", "=", "+", "/", ",", ";"],
            &mut ["a", "b", "c", "avg", "3"],
        );
    }

    #[test]
    fn cpp_function_ops() {
        check_ops(
            LANG::Cpp,
            "main()
            {
              int a, b, c, avg;
              scanf(\"%d %d %d\", &a, &b, &c);
              avg = (a + b + c) / 3;
              printf(\"avg = %d\", avg);
            }",
            "foo.c",
            &mut ["()", "{}", "int", "&", "=", "+", "/", ",", ";"],
            &mut [
                "main",
                "a",
                "b",
                "c",
                "avg",
                "scanf",
                "\"%d %d %d\"",
                "3",
                "printf",
                "\"avg = %d\"",
            ],
        );
    }

    #[test]
    fn rust_ops() {
        check_ops(
            LANG::Rust,
            "let: usize a = 5; let b: f32 = 7.0; let c: i32 = 3;",
            "foo.rs",
            &mut ["let", "usize", "=", ";", "f32", "i32"],
            &mut ["a", "b", "c", "5", "7.0", "3"],
        );
    }

    #[test]
    fn rust_function_ops() {
        check_ops(
            LANG::Rust,
            "fn main() {
              let a = 5; let b = 5; let c = 5;
              let avg = (a + b + c) / 3;
              println!(\"{}\", avg);
            }",
            "foo.rs",
            &mut ["fn", "()", "{}", "let", "=", "+", "/", ";", "!", ","],
            &mut ["main", "a", "b", "c", "avg", "5", "3", "println", "\"{}\""],
        );
    }

    #[test]
    fn javascript_ops() {
        check_ops(
            LANG::Javascript,
            "var a, b, c, avg;
             let x = 1;
             a = 5; b = 5; c = 5;
             avg = (a + b + c) / 3;
             console.log(\"{}\", avg);",
            "foo.js",
            &mut ["()", "var", "let", "=", "+", "/", ",", ".", ";"],
            &mut [
                "a", "b", "c", "avg", "x", "1", "3", "5", "console", "log", "\"{}\"",
            ],
        );
    }

    #[test]
    fn javascript_function_ops() {
        check_ops(
            LANG::Javascript,
            "function main() {
              var a, b, c, avg;
              let x = 1;
              a = 5; b = 5; c = 5;
              avg = (a + b + c) / 3;
              console.log(\"{}\", avg);
            }",
            "foo.js",
            &mut [
                "function", "()", "{}", "var", "let", "=", "+", "/", ",", ".", ";",
            ],
            &mut [
                "main", "a", "b", "c", "avg", "x", "1", "3", "5", "console", "log", "\"{}\"",
            ],
        );
    }

    #[test]
    fn mozjs_ops() {
        check_ops(
            LANG::Mozjs,
            "var a, b, c, avg;
             let x = 1;
             a = 5; b = 5; c = 5;
             avg = (a + b + c) / 3;
             console.log(\"{}\", avg);",
            "foo.js",
            &mut ["()", "var", "let", "=", "+", "/", ",", ".", ";"],
            &mut [
                "a", "b", "c", "avg", "x", "1", "3", "5", "console", "log", "\"{}\"",
            ],
        );
    }

    #[test]
    fn mozjs_function_ops() {
        check_ops(
            LANG::Mozjs,
            "function main() {
              var a, b, c, avg;
              let x = 1;
              a = 5; b = 5; c = 5;
              avg = (a + b + c) / 3;
              console.log(\"{}\", avg);
            }",
            "foo.js",
            &mut [
                "function", "()", "{}", "var", "let", "=", "+", "/", ",", ".", ";",
            ],
            &mut [
                "main", "a", "b", "c", "avg", "x", "1", "3", "5", "console", "log", "\"{}\"",
            ],
        );
    }

    #[test]
    fn typescript_ops() {
        // Issue #1261: the `: string` annotation counts exactly once,
        // as the text-keyed `string` operator (PredefinedType wrapper)
        // — symmetric with `: number` / `: boolean`. Under #313 its
        // `String2` child also emitted a `"string"` operand, so one
        // source token tallied twice.
        check_ops(
            LANG::Typescript,
            "var a, b, c, avg;
             let age: number = 32;
             let name: string = \"John\"; let isUpdated: boolean = true;
             a = 5; b = 5; c = 5;
             avg = (a + b + c) / 3;
             console.log(\"{}\", avg);",
            "foo.ts",
            &mut [
                "()", "var", "let", "string", "number", "boolean", ":", "=", "+", "/", ",", ".",
                ";",
            ],
            &mut [
                "a",
                "b",
                "c",
                "avg",
                "age",
                "name",
                "isUpdated",
                "32",
                "\"John\"",
                "true",
                "3",
                "5",
                "console",
                "log",
                "\"{}\"",
            ],
        );
    }

    #[test]
    fn typescript_function_ops() {
        // Issue #1261: see `typescript_ops` — the `string` type keyword
        // contributes only the primitive-typed operator, never a
        // `"string"` operand.
        check_ops(
            LANG::Typescript,
            "function main() {
              var a, b, c, avg;
              let age: number = 32;
              let name: string = \"John\"; let isUpdated: boolean = true;
              a = 5; b = 5; c = 5;
              avg = (a + b + c) / 3;
              console.log(\"{}\", avg);
            }",
            "foo.ts",
            &mut [
                "function", "()", "{}", "var", "let", "string", "number", "boolean", ":", "=", "+",
                "/", ",", ".", ";",
            ],
            &mut [
                "main",
                "a",
                "b",
                "c",
                "avg",
                "age",
                "name",
                "isUpdated",
                "32",
                "\"John\"",
                "true",
                "3",
                "5",
                "console",
                "log",
                "\"{}\"",
            ],
        );
    }

    #[test]
    fn tsx_ops() {
        // Issue #1261: TSX exposes the `: string` type-keyword child as
        // `String3` (vs. TS's `String2`); like TS, the keyword counts
        // only as the `string` operator, never as an operand.
        check_ops(
            LANG::Tsx,
            "var a, b, c, avg;
             let age: number = 32;
             let name: string = \"John\"; let isUpdated: boolean = true;
             a = 5; b = 5; c = 5;
             avg = (a + b + c) / 3;
             console.log(\"{}\", avg);",
            "foo.ts",
            &mut [
                "()", "var", "let", "string", "number", "boolean", ":", "=", "+", "/", ",", ".",
                ";",
            ],
            &mut [
                "a",
                "b",
                "c",
                "avg",
                "age",
                "name",
                "isUpdated",
                "32",
                "\"John\"",
                "true",
                "3",
                "5",
                "console",
                "log",
                "\"{}\"",
            ],
        );
    }

    #[test]
    fn tsx_function_ops() {
        // Issue #1261: see `tsx_ops` — TSX::String3 (type-keyword
        // `string`) is an operator only, never an operand.
        check_ops(
            LANG::Tsx,
            "function main() {
              var a, b, c, avg;
              let age: number = 32;
              let name: string = \"John\"; let isUpdated: boolean = true;
              a = 5; b = 5; c = 5;
              avg = (a + b + c) / 3;
              console.log(\"{}\", avg);
            }",
            "foo.ts",
            &mut [
                "function", "()", "{}", "var", "let", "string", "number", "boolean", ":", "=", "+",
                "/", ",", ".", ";",
            ],
            &mut [
                "main",
                "a",
                "b",
                "c",
                "avg",
                "age",
                "name",
                "isUpdated",
                "32",
                "\"John\"",
                "true",
                "3",
                "5",
                "console",
                "log",
                "\"{}\"",
            ],
        );
    }

    // Issue #453: a `void` return type (a `predefined_type` wrapper over a
    // `void` token) and an expression `void` (`void 0`) must collapse to a
    // single distinct `"void"` operator. `check_ops` asserts the exact
    // operator list, so a duplicate `"void"` — the pre-fix symptom, where
    // the wrapper keyed `primitive_operators["void"]` and the inner token
    // keyed `operators[Void]` — trips the assertion. This pins the lesson-4
    // `n1 == dedupe(ops.operators)` invariant for the two `void` forms in
    // one file.
    #[test]
    fn typescript_void_return_and_expression_single_operator_453() {
        check_ops(
            LANG::Typescript,
            "function f(): void { return void 0; }",
            "foo.ts",
            &mut ["function", "()", "{}", ":", "void", "return", ";"],
            &mut ["f", "0"],
        );
    }

    #[test]
    fn tsx_void_return_and_expression_single_operator_453() {
        check_ops(
            LANG::Tsx,
            "function f(): void { return void 0; }",
            "foo.tsx",
            &mut ["function", "()", "{}", ":", "void", "return", ";"],
            &mut ["f", "0"],
        );
    }

    #[test]
    fn java_ops() {
        check_ops(
            LANG::Java,
            "public class Main {
                public static void main(string args[]) {
                      int a, b, c, avg;
                      a = 5; b = 5; c = 5;
                      avg = (a + b + c) / 3;
                      MessageFormat.format(\"{0}\", avg);
                    }
                }",
            "foo.java",
            &mut [
                "{}", "void", "()", "[]", ",", ".", ";", "int", "=", "+", "/",
            ],
            &mut [
                "Main",
                "main",
                "args",
                "a",
                "b",
                "c",
                "avg",
                "5",
                "3",
                "MessageFormat",
                "format",
                "\"{0}\"",
            ],
        );
    }

    #[test]
    fn java_primitive_ops() {
        check_ops(
            LANG::Java,
            "public class Prims {
                byte a = 1;
                short b = 2;
                int c = 3;
                long d = 4;
                char e = 'x';
                float f = 1.0f;
                double g = 2.0;
                boolean h = true;
                boolean i = false;
            }",
            "foo.java",
            // All 8 primitive-type keywords must appear as distinct operators.
            // true/false appear as operands.
            &mut [
                "{}",
                ";",
                "=",
                "byte",
                "short",
                "int",
                "long",
                "char",
                "float",
                "double",
                "boolean_type",
            ],
            &mut [
                "Prims", "a", "b", "c", "d", "e", "f", "g", "h", "i", "1", "2", "3", "4", "'x'",
                "1.0f", "2.0", "true", "false",
            ],
        );
    }

    /// A `Unit` space must never carry the synthetic `<anonymous>`
    /// placeholder that the default getter invents for nodes without a
    /// `name` field. The public docs describe `None` as the
    /// "name could not be resolved" state, and the metrics-side
    /// `FuncSpace::new` already special-cases `SpaceKind::Unit` the same
    /// way; this pins the `Ops::new` mirror so a regression to the old
    /// `Some("<anonymous>")` initialisation fails here rather than only
    /// surfacing for a (currently unreachable) non-top-level `Unit`
    /// space, where `ops_inner`'s top-level override would not rescue it.
    /// See issue #755.
    #[cfg(feature = "rust")]
    #[test]
    fn unit_space_name_is_none_not_anonymous() {
        use crate::getter::Getter;
        use crate::node::Ancestors;
        use crate::traits::ParserTrait;
        use crate::{RustCode, RustParser, SpaceKind};

        let code = b"fn f() {}\n";
        let parser = RustParser::new(code.to_vec(), std::path::Path::new("foo.rs"), None);
        let root = parser.root();
        // The Rust `source_file` root is a `Unit` and has no `name`/`type`
        // field, so the default getter would invent `<anonymous>`.
        assert_eq!(SpaceKind::Unit, RustCode::get_space_kind(&root));

        let ops = super::Ops::new::<RustCode>(&root, code, Ancestors::unknown(), SpaceKind::Unit);
        assert_eq!(
            ops.name, None,
            "Unit space must preserve name = None, not invent <anonymous>"
        );
    }

    /// Issue #789: an `ERROR`-root parse (here Lua partial input) where
    /// `metrics()` succeeds must make `ops()` succeed too — the two seams
    /// should agree. Before the synthetic-Unit-root mirror in `ops_inner`,
    /// `ops()` returned `Err(MetricsError::EmptyRoot)` because the ERROR
    /// root is not classified as a function space, so no frame was ever
    /// pushed. This pins their agreement: both succeed, and the resulting
    /// top-level `Ops` is a `Unit` whose name is the caller-supplied
    /// `Source::name` (the intrinsic Unit name stays `None` per #755 until
    /// `ops_inner` overrides the top-level name).
    #[cfg(feature = "lua")]
    #[test]
    fn lua_error_root_ops_agrees_with_metrics_789() {
        use crate::{MetricsOptions, SpaceKind};

        // tree-sitter-lua surfaces an ERROR root for this partial input.
        let src = b"function foo(x)\n  return x +\n".to_vec();
        let name = "partial.lua".to_owned();

        let ast = Ast::parse(Source::new(LANG::Lua, &src).with_name(Some(name.clone())))
            .expect("lua feature enabled");

        // metrics() must succeed (it already wrapped a synthetic Unit root).
        let space = ast
            .metrics(MetricsOptions::default())
            .expect("metrics must yield a top-level space");
        assert_eq!(space.kind, SpaceKind::Unit);

        // ops() must now succeed in the same case rather than returning
        // Err(EmptyRoot).
        let ops = ast
            .ops()
            .expect("ops must agree with metrics and yield a top-level Ops");
        assert_eq!(ops.kind, SpaceKind::Unit);
        assert_eq!(
            ops.name.as_deref(),
            Some(name.as_str()),
            "top-level Ops name is the caller-supplied Source::name"
        );
    }

    /// Issue #790: `Ops::operands` / `Ops::operators` are the *distinct*
    /// (deduplicated) Halstead operand/operator vocabularies (`n2` / `n1`),
    /// not every occurrence. Pin the documented dedup semantics: each
    /// vector's length equals its unique-element count. The fixture
    /// repeats `+`, `;`, and `=` operators and the `a` operand so a
    /// regression to non-deduplicated collection would make `len` exceed
    /// the unique count.
    #[cfg(feature = "rust")]
    #[test]
    fn ops_vocabularies_are_distinct_790() {
        use std::collections::HashSet;

        let src = b"fn main() { let a = 1 + 1; let b = a + a; }\n".to_vec();
        let ops = Ast::parse(Source::new(LANG::Rust, &src).with_name(Some("foo.rs".to_owned())))
            .expect("rust feature enabled")
            .ops()
            .expect("ops walk must yield a top-level Ops");

        let unique_operators: HashSet<&String> = ops.operators.iter().collect();
        assert_eq!(
            ops.operators.len(),
            unique_operators.len(),
            "Ops::operators must be the distinct operator vocabulary (n1)"
        );

        let unique_operands: HashSet<&String> = ops.operands.iter().collect();
        assert_eq!(
            ops.operands.len(),
            unique_operands.len(),
            "Ops::operands must be the distinct operand vocabulary (n2)"
        );
    }

    /// Assert that every space in the tree carries sorted vocabularies,
    /// and return how many spaces were checked so a caller can prove the
    /// walk actually descended.
    ///
    /// The length floor is what keeps this from going quietly vacuous:
    /// `is_sorted` is trivially true for an empty or single-entry
    /// vector, so without it a fixture that stopped producing real
    /// vocabularies would keep passing while covering nothing.
    fn assert_sorted_spaces(ops: &Ops, lang: LANG) -> usize {
        /// Smallest vocabulary in which an ordering is observable.
        const MIN_OBSERVABLE: usize = 2;

        let mut stack = vec![ops];
        let mut visited = 0;

        while let Some(space) = stack.pop() {
            visited += 1;
            for (field, values) in [
                ("operators", &space.operators),
                ("operands", &space.operands),
            ] {
                assert!(
                    values.len() >= MIN_OBSERVABLE && values.is_sorted(),
                    "{lang:?} {field} of space {:?} (@{}) must hold at least \
                     {MIN_OBSERVABLE} entries and be sorted: {values:?}",
                    space.name,
                    space.start_line
                );
            }
            stack.extend(space.spaces.iter());
        }

        visited
    }

    /// Every space's vocabularies come back sorted, in every language.
    ///
    /// The operator vocabulary is the union of two maps — one keyed by
    /// token id, one keyed by text, which is where primitive types such
    /// as C++ `int` land — and sorting is what interleaves them rather
    /// than leaving the second concatenated onto the first (#1091). The
    /// `spans_both_maps` pair per case names one entry from each map, so
    /// a fixture that stopped exercising the text-keyed map fails here
    /// instead of silently narrowing the test's reach.
    #[test]
    fn ops_vocabularies_are_sorted_1091() {
        /// `(language, file name, source, (text-keyed operator,
        /// token-id-keyed operator that must sort after it))`.
        type Case = (
            LANG,
            &'static str,
            &'static str,
            (&'static str, &'static str),
        );

        let cases: &[Case] = &[
            #[cfg(feature = "rust")]
            (
                LANG::Rust,
                "rust.rs",
                "fn zeta(quux: u32) -> u32 { let mid = quux + 1; \
                 let alpha = |beta: u32| beta * mid; alpha(mid) - quux }\n",
                ("u32", "|"),
            ),
            #[cfg(feature = "cpp")]
            (
                LANG::Cpp,
                "cpp.cpp",
                "int zeta(int quux) { double mid = quux + 1; \
                 char alpha = 'z'; return quux - mid + alpha; }\n",
                ("int", "return"),
            ),
            #[cfg(feature = "java")]
            (
                LANG::Java,
                "Java.java",
                "class Zeta { int quux(int mid) { long alpha = mid + 1; \
                 boolean beta = alpha > 2; return beta ? mid : 0; } }\n",
                ("long", "return"),
            ),
            #[cfg(feature = "python")]
            (
                LANG::Python,
                "python.py",
                "def zeta(quux):\n    mid = quux + 1\n    \
                 def alpha(beta):\n        return beta * mid\n    return alpha(mid) - quux\n",
                // Python has no primitive-type operators; both entries
                // come from the token-id map, so this pair only pins the
                // ordering, not the interleaving.
                ("def", "return"),
            ),
            #[cfg(feature = "typescript")]
            (
                LANG::Typescript,
                "ts.ts",
                "function zeta(quux: number): number { const mid: number = quux + 1; \
                 const alpha = (beta: number) => beta * mid; return alpha(mid) - quux; }\n",
                ("number", "return"),
            ),
        ];

        for (lang, file, source, (from_text_map, sorts_after)) in cases {
            let ops = Ast::parse(
                Source::new(*lang, source.as_bytes()).with_name(Some((*file).to_owned())),
            )
            .expect("language feature enabled")
            .ops()
            .expect("ops walk must yield a top-level Ops");

            let position = |needle: &str| {
                ops.operators
                    .iter()
                    .position(|op| op == needle)
                    .unwrap_or_else(|| {
                        panic!(
                            "{lang:?} operators must contain {needle:?}: {:?}",
                            ops.operators
                        )
                    })
            };
            assert!(
                position(from_text_map) < position(sorts_after),
                "{lang:?} must order {from_text_map:?} before {sorts_after:?}: {:?}",
                ops.operators
            );

            assert!(
                assert_sorted_spaces(&ops, *lang) > 1,
                "{lang:?} sample must nest at least one sub-space"
            );
        }
    }

    /// Two parses of the same bytes in one process must agree exactly.
    ///
    /// `RandomState` bumps its thread-local seed per instance, so the
    /// pre-fix code could — and did — order two `HashMap`s built from
    /// identical keys differently within a single run. This is the
    /// in-process form of the cross-run churn in #1091.
    #[test]
    #[cfg(feature = "rust")]
    fn ops_are_stable_across_repeated_parses_1091() {
        use std::fmt::Write as _;

        // Enough distinct operands, in enough distinct spaces, that
        // agreement by coincidence is not a plausible explanation for a
        // pass.
        let mut src = String::new();
        for i in 0..40 {
            writeln!(src, "fn name{i}(arg{i}: u32) -> u32 {{ arg{i} + {i} }}")
                .expect("writing to a String cannot fail");
        }

        let parse = || {
            Ast::parse(Source::new(LANG::Rust, src.as_bytes()).with_name(Some("foo.rs".to_owned())))
                .expect("rust feature enabled")
                .ops()
                .expect("ops walk must yield a top-level Ops")
        };

        let (first, second) = (parse(), parse());
        assert!(
            first.operands.len() >= 40 && first.spaces.len() >= 40,
            "sample must have a wide vocabulary across many spaces, got {} operands \
             in {} spaces",
            first.operands.len(),
            first.spaces.len()
        );
        // `Ops` has no `PartialEq`, and the nested spaces are the half a
        // top-level vector comparison would miss, so compare the whole
        // serialized tree.
        let render =
            |ops: &Ops| serde_json::to_string(&ops.to_wire()).expect("wire Ops serializes to JSON");
        assert_eq!(render(&first), render(&second));
    }

    /// A vocabulary that lost bytes is ordered by its *rendered* text.
    ///
    /// #1110 moved the sort ahead of the lossy UTF-8 rendering, which is
    /// only order-preserving while every key is valid UTF-8. Here it is
    /// not: the two string operands are `"\xffA"` and `"\u{fffd}B"`, so
    /// by raw bytes the first sorts *after* the second (`0xff` > `0xef`)
    /// and by rendered text — both start `U+FFFD`, then `A` before `B` —
    /// it sorts before. The rendered order is what the pre-#1110
    /// render-then-sort code produced and what `Ops` documents, so
    /// dropping the fallback re-sort flips this pair and fails here.
    #[test]
    #[cfg(feature = "rust")]
    fn ops_vocabulary_orders_lossy_entries_by_rendered_text_1110() {
        let mut src = b"fn f() { let a = \"".to_vec();
        src.push(0xff);
        src.extend_from_slice(b"A\"; let b = \"");
        src.extend_from_slice(&[0xef, 0xbf, 0xbd]);
        src.extend_from_slice(b"B\"; }\n");

        let ops = Ast::parse(Source::new(LANG::Rust, &src).with_name(Some("foo.rs".to_owned())))
            .expect("rust feature enabled")
            .ops()
            .expect("ops walk must yield a top-level Ops");

        let position = |needle: &str| {
            ops.operands
                .iter()
                .position(|operand| operand == needle)
                .unwrap_or_else(|| panic!("operands must contain {needle:?}: {:?}", ops.operands))
        };
        assert!(
            position("\"\u{fffd}A\"") < position("\"\u{fffd}B\""),
            "lossy entries must be ordered by rendered text, got {:?}",
            ops.operands
        );
        assert!(
            ops.operands.is_sorted(),
            "the whole vocabulary must be sorted as rendered, got {:?}",
            ops.operands
        );
    }

    /// The walk classifies a space kind once per space, not once per node.
    ///
    /// Nothing in the output distinguishes the two: the classification
    /// only ever reaches `Ops::new`, so running it on every node produces
    /// the same tree and merely throws the extra answers away. #1110
    /// moved the call inside the `func_space` branch, mirroring
    /// `spaces::compute::open_func_space`; the counter is what makes
    /// hoisting it back out a failure. The node-count assertion is what
    /// makes the counts distinguishable — a fixture whose nodes and
    /// spaces were equal in number could not tell the two apart.
    #[test]
    // Gated on the language that guarantees a non-empty case list, so
    // the emptiness assertion below cannot fire on a minimal build.
    #[cfg(feature = "rust")]
    fn ops_classifies_space_kind_once_per_space_1110() {
        let cases: &[(LANG, &str, &str)] = &[
            #[cfg(feature = "rust")]
            (
                LANG::Rust,
                "foo.rs",
                "fn outer(a: u32) -> u32 { fn inner(b: u32) -> u32 { b + 1 } inner(a) * 2 }\n",
            ),
            #[cfg(feature = "python")]
            (
                LANG::Python,
                "foo.py",
                "def outer(a):\n    def inner(b):\n        return b + 1\n    return inner(a) * 2\n",
            ),
            #[cfg(feature = "cpp")]
            (
                LANG::Cpp,
                "foo.cpp",
                "struct S { int m(int a) { return a + 1; } }; int f(int b) { return b * 2; }\n",
            ),
            #[cfg(feature = "java")]
            (
                LANG::Java,
                "Foo.java",
                "class C { int m(int a) { return a + 1; } int n(int b) { return b * 2; } }\n",
            ),
            #[cfg(feature = "javascript")]
            (
                LANG::Javascript,
                "foo.js",
                "function outer(a) { function inner(b) { return b + 1; } return inner(a) * 2; }\n",
            ),
        ];
        crate::test_support::assert_fixtures_present(cases);

        for (lang, file, source) in cases {
            let ast = crate::test_support::parse_named(*lang, file, source);

            let before = super::space_kind_lookups::observed();
            let ops = ast.ops().expect("ops walk must yield a top-level Ops");
            let lookups = super::space_kind_lookups::observed() - before;

            let mut spaces = 0;
            let mut stack = vec![&ops];
            while let Some(space) = stack.pop() {
                spaces += 1;
                stack.extend(space.spaces.iter());
            }

            let mut nodes = 0;
            let mut cursor = vec![ast.as_tree_sitter().root_node()];
            while let Some(node) = cursor.pop() {
                nodes += 1;
                let mut walker = node.walk();
                cursor.extend(node.children(&mut walker));
            }

            assert!(
                nodes > spaces * 4,
                "{lang:?} fixture must have many more nodes ({nodes}) than spaces ({spaces}) \
                 for the two counts to be distinguishable"
            );
            assert_eq!(
                lookups, spaces,
                "{lang:?} must classify once per space, not once per node ({nodes} nodes)"
            );
        }
    }

    /// One flattened space: `(depth, kind, name, start_line, end_line)`.
    #[cfg(feature = "elixir")]
    type FlatSpace = (usize, crate::SpaceKind, String, usize, usize);

    /// Flattens an `Ops` tree in preorder, so a test can pin the whole
    /// tree in one `assert_eq!` and see the surrounding spaces when one
    /// is wrong.
    ///
    /// `end_line` is carried as well as `start_line` because a change to
    /// the promote predicate can move a space's *extent* without moving
    /// its head — a `def` that swallows its sibling would keep the same
    /// start line.
    #[cfg(feature = "elixir")]
    fn flatten(ops: &Ops, depth: usize, out: &mut Vec<FlatSpace>) {
        out.push((
            depth,
            ops.kind,
            ops.name.clone().unwrap_or_else(|| "<none>".to_owned()),
            ops.start_line,
            ops.end_line,
        ));
        for child in &ops.spaces {
            flatten(child, depth + 1, out);
        }
    }

    #[cfg(feature = "elixir")]
    fn elixir_ops_tree(source: &str) -> Vec<FlatSpace> {
        let ops = crate::test_support::parse_named(LANG::Elixir, "foo.ex", source)
            .ops()
            .expect("ops walk must yield a top-level Ops");
        let mut flat = Vec::new();
        flatten(&ops, 0, &mut flat);
        flat
    }

    /// Issue #1130: Elixir's `defmodule` / `def` are `Call` nodes whose
    /// target identifier text spells the keyword, so only the
    /// source-aware promote predicate can recognise them. Before the
    /// fix `ops()` returned the bare file-level `Unit` for this input
    /// while `metrics()` returned the full module/function tree.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_ops_opens_module_and_function_spaces_1130() {
        use crate::SpaceKind::{Class, Function, Unit};

        assert_eq!(
            elixir_ops_tree("defmodule Foo do\n  def bar(x) do\n    x + 1\n  end\nend\n"),
            vec![
                (0, Unit, "foo.ex".to_owned(), 1, 5),
                (1, Class, "Foo".to_owned(), 1, 5),
                (2, Function, "bar".to_owned(), 2, 4),
            ],
        );
    }

    /// An `AnonymousFunction` is the one Elixir space the byte-less
    /// predicate could already see, so this pins that the source-aware
    /// predicate did not lose it — and that it nests under the `def`
    /// space rather than being reparented to the file root.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_ops_opens_anonymous_function_space() {
        use crate::SpaceKind::{Class, Function, Unit};

        assert_eq!(
            elixir_ops_tree(
                "defmodule Foo do\n  def bar(list) do\n    \
                 Enum.map(list, fn x -> x * 2 end)\n  end\nend\n"
            ),
            vec![
                (0, Unit, "foo.ex".to_owned(), 1, 5),
                (1, Class, "Foo".to_owned(), 1, 5),
                (2, Function, "bar".to_owned(), 2, 4),
                (3, Function, "<anonymous>".to_owned(), 3, 3),
            ],
        );
    }

    /// Issue #310: a `def` inside `quote do … end` is a code *template*
    /// emitted later by macro expansion, not a declaration of the
    /// enclosing module, so it opens no space. This is the case only the
    /// source-aware predicate can get right — the byte-less one never
    /// saw any `def` at all, so it was accidentally "correct" here while
    /// being wrong everywhere else.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_ops_skips_def_inside_quote_block_310() {
        use crate::SpaceKind::{Class, Function, Unit};

        // The quoted `def` heads line 4. Its name resolves to the
        // `<anonymous>` placeholder (the head is `unquote(name)`, not a
        // literal identifier), so the absence of a fourth entry — not a
        // name match — is what pins it out of the tree.
        assert_eq!(
            elixir_ops_tree(
                "defmodule Foo do\n  defmacro gen(name) do\n    quote do\n      \
                 def unquote(name)(x) do\n        x + 1\n      end\n    end\n  end\nend\n",
            ),
            vec![
                (0, Unit, "foo.ex".to_owned(), 1, 9),
                (1, Class, "Foo".to_owned(), 1, 9),
                (2, Function, "gen".to_owned(), 2, 8),
            ],
        );
    }
}
