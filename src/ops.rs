use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::checker::Checker;
use crate::getter::Getter;
use crate::node::Node;
use crate::spaces::SpaceKind;

use crate::halstead::{Halstead, HalsteadMaps};

use crate::dump_ops::*;
use crate::traits::*;

/// All operands and operators of a space.
#[derive(Debug, Clone, Serialize)]
pub struct Ops {
    /// The name of a function space.
    ///
    /// For the top-level (file-level) `Ops`, this is the file path
    /// supplied to [`operands_and_operators`] converted via lossy UTF-8
    /// conversion, so it is always `Some`. Non-UTF-8 path components on
    /// Linux (or invalid UTF-16 on Windows) become U+FFFD replacement
    /// characters; in that case [`Ops::name_was_lossy`] is `true` and
    /// downstream consumers must treat the name as display-only — never
    /// as a map key or for error correlation.
    ///
    /// For nested spaces, `None` means an error occurred in parsing the
    /// name of the function space from the AST.
    pub name: Option<String>,
    /// `true` when [`Ops::name`] was produced by lossy conversion (the
    /// original path contained non-UTF-8 bytes and was rendered using
    /// U+FFFD replacement characters). Always `false` for nested spaces
    /// and for top-level spaces with valid-UTF-8 paths. Skipped from
    /// JSON output when `false` so existing schemas keep their shape.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
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

/// Retrieves all the operators and operands of a code.
///
/// If `None`, it was not possible to retrieve the operators and operands
/// of a code.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
///
/// use big_code_analysis::{operands_and_operators, CppParser, ParserTrait};
///
/// # fn main() {
/// let source_code = "int a = 42;";
///
/// // The path to a dummy file used to contain the source code
/// let path = PathBuf::from("foo.c");
/// let source_as_vec = source_code.as_bytes().to_vec();
///
/// // The parser of the code, in this case a CPP parser
/// let parser = CppParser::new(source_as_vec, &path, None);
///
/// // Returns the operands and operators of each space in a code.
/// operands_and_operators(&parser, &path).unwrap();
/// # }
/// ```
pub fn operands_and_operators<'a, T: ParserTrait>(parser: &'a T, path: &'a Path) -> Option<Ops> {
    let code = parser.get_code();
    let node = parser.get_root();
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

    state_stack.pop().map(|mut state| {
        // See `FuncSpace::name` rationale in `spaces.rs`: lossy conversion
        // keeps the top-level `Ops` identifiable for non-UTF-8 paths
        // rather than collapsing into the parse-error sentinel `None`.
        // The `name_was_lossy` flag lets downstream consumers detect
        // and avoid using the U+FFFD-bearing name as an identifier.
        let was_lossy = path.to_str().is_none();
        state.ops.name = Some(path.to_string_lossy().into_owned());
        state.ops.name_was_lossy = was_lossy;
        state.ops
    })
}

/// Configuration options for retrieving
/// all the operands and operators in a code.
#[derive(Debug)]
pub struct OpsCfg {
    /// Path to the file containing the code.
    pub path: PathBuf,
}

pub struct OpsCode {
    _guard: (),
}

impl Callback for OpsCode {
    type Res = std::io::Result<()>;
    type Cfg = OpsCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        if let Some(ops) = operands_and_operators(parser, &cfg.path) {
            dump_ops(&ops)
        } else {
            Ok(())
        }
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
    use std::path::PathBuf;

    use crate::{LANG, get_ops};

    #[inline]
    fn check_ops(
        lang: LANG,
        source: &str,
        file: &str,
        correct_operators: &mut [&str],
        correct_operands: &mut [&str],
    ) {
        let path = PathBuf::from(file);
        let mut trimmed_bytes = source.trim_end().trim_matches('\n').as_bytes().to_vec();
        trimmed_bytes.push(b'\n');
        let ops = get_ops(&lang, trimmed_bytes, &path, None).unwrap();

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
            ],
        );
    }

    #[test]
    fn typescript_function_ops() {
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
            ],
        );
    }

    #[test]
    fn tsx_ops() {
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
            ],
        );
    }

    #[test]
    fn tsx_function_ops() {
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
            ],
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

    /// Regression for issue #128 — non-UTF-8 paths must not collapse the
    /// top-level `Ops::name` into `None`, which is reserved for AST-name
    /// parse failures.
    #[cfg(unix)]
    #[test]
    fn non_utf8_path_yields_lossy_top_level_name() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let raw_bytes: &[u8] = b"foo_\xFF\xFE_bar.py";
        let path = PathBuf::from(OsStr::from_bytes(raw_bytes));
        assert!(
            path.to_str().is_none(),
            "test premise broken: path must be non-UTF-8 for this test to be meaningful"
        );

        let ops = get_ops(&LANG::Python, b"a = 1\n".to_vec(), &path, None)
            .expect("get_ops must yield a top-level Ops");

        let name = ops
            .name
            .as_deref()
            .expect("top-level Ops name must be Some, not the parse-error sentinel None");
        assert!(
            name.contains('\u{FFFD}'),
            "expected U+FFFD replacement char in lossy name, got {name:?}"
        );
        assert!(
            name.starts_with("foo_") && name.ends_with("_bar.py"),
            "lossy name must preserve the surrounding ASCII bytes, got {name:?}"
        );
        assert!(
            ops.name_was_lossy,
            "name_was_lossy must be true when the source path was non-UTF-8"
        );
    }

    /// Top-level `Ops` with valid UTF-8 paths must NOT have
    /// `name_was_lossy` set.
    #[test]
    fn utf8_path_does_not_set_name_was_lossy() {
        let path = PathBuf::from("foo.py");
        let ops = get_ops(&LANG::Python, b"a = 1\n".to_vec(), &path, None)
            .expect("get_ops must yield a top-level Ops");
        assert!(
            !ops.name_was_lossy,
            "name_was_lossy must be false for valid-UTF-8 paths"
        );
    }
}
