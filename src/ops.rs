// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use crate::checker::Checker;
use crate::error::MetricsError;
use crate::getter::Getter;
use crate::node::{Ancestors, Node};
use crate::spaces::{SpaceKind, push_children};

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
    /// occurrence. Backed by the keys of a `HashMap`, so entries are
    /// unique and returned in arbitrary order.
    pub operands: Vec<String>,
    /// The **distinct** operators of a space — the deduplicated Halstead
    /// operator vocabulary (`n1`), one entry per unique operator, not
    /// every occurrence. Backed by the keys of a `HashMap`, so entries
    /// are unique and returned in arbitrary order.
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

    fn new<T: Getter>(node: &Node, code: &[u8], kind: SpaceKind) -> Self {
        let (start_position, end_position) = match kind {
            SpaceKind::Unit => {
                if node.child_count() == 0 {
                    (0, 0)
                } else {
                    (node.start_row() + 1, node.end_row())
                }
            }
            _ => (node.start_row() + 1, node.end_row() + 1),
        };
        // The top-level Unit's name is overwritten by `ops_inner` with the
        // caller-supplied name before returning, so computing it here is
        // wasted work. Non-top-level Unit spaces have no resolvable name, so
        // leaving `None` matches the documented "could not be resolved"
        // semantics rather than inventing the `<anonymous>` placeholder the
        // default getter returns. Other kinds keep the AST-derived name.
        // Mirrors the `SpaceKind::Unit` handling in `FuncSpace::new`.
        let name = (kind != SpaceKind::Unit)
            .then(|| T::get_func_space_name(node, code).map(str::to_owned))
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
            ops: Ops::new::<T::Getter>(node, code, SpaceKind::Unit),
            halstead_maps: HalsteadMaps::new(),
        });
    }
}

/// Convert `&[u8]` source text to an owned `String`.
/// Tree-sitter sources are expected to be valid UTF-8; non-UTF-8 bytes
/// are replaced with the Unicode replacement character to keep the entry
/// visible (rather than silently dropping it or using a sentinel string
/// that could collide with a real identifier).
fn bytes_to_string(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn compute_operators_and_operands<T: ParserTrait>(state: &mut State) {
    state.ops.operators = state
        .halstead_maps
        .operators
        .keys()
        .map(|k| T::Getter::get_operator_id_as_str(*k).to_owned())
        .collect();

    // Add primitive-type operators (stored by text in HalsteadMaps)
    state.ops.operators.extend(
        state
            .halstead_maps
            .primitive_operators
            .keys()
            .map(|k| bytes_to_string(k)),
    );

    state.ops.operands = state
        .halstead_maps
        .operands
        .keys()
        .map(|k| bytes_to_string(k))
        .collect();
}

fn finalize<T: ParserTrait>(state_stack: &mut Vec<State>, diff_level: usize) {
    if state_stack.is_empty() {
        return;
    }

    for _ in 0..diff_level {
        if state_stack.len() == 1 {
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

    // Compute ops for the remaining parent from its fully-merged
    // HalsteadMaps. This runs once instead of per-iteration, and
    // produces the deduplicated union of all operators/operands.
    if let Some(last_state) = state_stack.last_mut() {
        compute_operators_and_operands::<T>(last_state);
    }
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
    let mut children = Vec::new();
    let mut state_stack: Vec<State> = Vec::new();
    let mut last_level = 0;

    // Mirror `metrics_inner`: wrap a non-`Unit` (e.g. `ERROR`) root in a
    // synthetic `Unit` frame so the walk always has a frame to populate.
    // Without this, an `ERROR`-root parse drains the state stack and
    // `ops_inner` returns `EmptyRoot` for inputs where `metrics()`
    // succeeds (issue #789).
    push_synthetic_unit_root::<T>(&mut state_stack, &node, code);

    stack.push((node, 0));

    while let Some((node, level)) = stack.pop() {
        if level < last_level {
            finalize::<T>(&mut state_stack, last_level - level);
            last_level = level;
        }

        let kind = T::Getter::get_space_kind(&node);

        let func_space = T::Checker::is_func(&node) || T::Checker::is_func_space(&node);

        let new_level = if func_space {
            let state = State {
                ops: Ops::new::<T::Getter>(&node, code, kind),
                halstead_maps: HalsteadMaps::new(),
            };
            state_stack.push(state);
            last_level = level + 1;
            last_level
        } else {
            level
        };

        if let Some(state) = state_stack.last_mut() {
            T::Halstead::compute(&node, code, &mut state.halstead_maps);
        }

        // Shared with `metrics_inner` (issue #969): `push_children` is
        // State-independent — it only moves the cursor over child nodes —
        // so unlike the local `finalize` / `push_synthetic_unit_root`
        // mirrors (which differ by `State` payload) it is reused directly
        // rather than duplicated. The `children.drain(..).rev()` ordering
        // it encapsulates is load-bearing for suppression attribution.
        // The returned child slice is only useful to `metrics_inner`,
        // which seeds their cognitive nesting; `ops` just walks them.
        push_children(&mut cursor, &node, new_level, &mut children, &mut stack);
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

        let mut operators_str: Vec<&str> = ops.operators.iter().map(AsRef::as_ref).collect();
        let mut operands_str: Vec<&str> = ops.operands.iter().map(AsRef::as_ref).collect();

        // Sorting out operators because they are returned in arbitrary order
        operators_str.sort_unstable();
        correct_operators.sort_unstable();

        assert_eq!(&operators_str[..], correct_operators);

        // Sorting out operands because they are returned in arbitrary order
        operands_str.sort_unstable();
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
                "a",
                "b",
                "c",
                "avg",
                "x",
                "1",
                "3",
                "5",
                "console.log",
                "console",
                "log",
                "\"{}\"",
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
                "main",
                "a",
                "b",
                "c",
                "avg",
                "x",
                "1",
                "3",
                "5",
                "console.log",
                "console",
                "log",
                "\"{}\"",
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
                "a",
                "b",
                "c",
                "avg",
                "x",
                "1",
                "3",
                "5",
                "console.log",
                "console",
                "log",
                "\"{}\"",
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
                "main",
                "a",
                "b",
                "c",
                "avg",
                "x",
                "1",
                "3",
                "5",
                "console.log",
                "console",
                "log",
                "\"{}\"",
            ],
        );
    }

    #[test]
    fn typescript_ops() {
        // Issue #313: the `: string` annotation's `String2` child now
        // emits a `"string"` operand alongside the `string`
        // primitive-typed operator (PredefinedType wrapper). Other
        // type-keyword annotations (`: number`, `: boolean`) are not
        // string-named kinds, so they only contribute an operator.
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
                "console.log",
                "console",
                "log",
                "\"{}\"",
                "string",
            ],
        );
    }

    #[test]
    fn typescript_function_ops() {
        // Issue #313: see `typescript_ops` — the `string` type keyword
        // appears as both an operator (primitive-typed) and an operand
        // (text `"string"`) once Checker/Getter parity is enforced.
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
                "console.log",
                "console",
                "log",
                "\"{}\"",
                "string",
            ],
        );
    }

    #[test]
    fn tsx_ops() {
        // Issue #313: TSX exposes the `: string` type-keyword child as
        // `String3` (vs. TS's `String2`); both are now in the operand
        // classification, so `"string"` appears as a TSX operand for
        // the same reason as the TS case above.
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
                "console.log",
                "console",
                "log",
                "\"{}\"",
                "string",
            ],
        );
    }

    #[test]
    fn tsx_function_ops() {
        // Issue #313: see `tsx_ops` — TSX::String3 (type-keyword
        // `string`) is now an operand.
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
                "console.log",
                "console",
                "log",
                "\"{}\"",
                "string",
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
        use crate::traits::ParserTrait;
        use crate::{RustCode, RustParser, SpaceKind};

        let code = b"fn f() {}\n";
        let parser = RustParser::new(code.to_vec(), std::path::Path::new("foo.rs"), None);
        let root = parser.root();
        // The Rust `source_file` root is a `Unit` and has no `name`/`type`
        // field, so the default getter would invent `<anonymous>`.
        assert_eq!(SpaceKind::Unit, RustCode::get_space_kind(&root));

        let ops = super::Ops::new::<RustCode>(&root, code, SpaceKind::Unit);
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
}
