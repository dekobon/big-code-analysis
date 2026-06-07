// bca: suppress-file(halstead, loc, nargs, nom)
// Per-language operator-extraction dispatch; the offenders are arm-count
// and many-fn aggregation artifacts, not per-function logic complexity.

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
use crate::node::Node;
use crate::spaces::SpaceKind;

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
    /// All operands of a space.
    pub operands: Vec<String>,
    /// All operators of a space.
    pub operators: Vec<String>,
}

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
        Self {
            name: T::get_func_space_name(node, code).map(str::to_owned),
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

        cursor.reset(&node);
        if cursor.goto_first_child() {
            loop {
                children.push((cursor.node(), new_level));
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            for child in children.drain(..).rev() {
                stack.push(child);
            }
        }
    }

    finalize::<T>(&mut state_stack, usize::MAX);

    // Reserved error path: `MetricsError::EmptyRoot` is unreachable
    // today because every supported language's root node is recognised
    // as a `func_space` and pushes a state. The `ok_or` is retained so a
    // future walker change that legitimately drains the stack surfaces
    // a distinct error variant rather than a bare `None`. See
    // `MetricsError::EmptyRoot` for the matching variant doc.
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
}
