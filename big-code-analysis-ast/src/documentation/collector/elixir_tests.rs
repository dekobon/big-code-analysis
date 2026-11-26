//! Parse shapes of the pinned Elixir grammar that a reader of documentation attributes
//! relies on: the children of an attribute, of its value, of a definition and of a module
//! body. The test parses verbatim bytes and collects nothing.

#[cfg(feature = "elixir")]
use crate::{AnyParser, LANG};

#[cfg(feature = "elixir")]
fn parsed(source: &[u8]) -> AnyParser {
    AnyParser::parse(LANG::Elixir, source.to_vec(), None, None).unwrap()
}

// The children of a node: their kinds, with `!` after a token that the parser invented and
// `?` after a child that holds a syntax error.
#[cfg(feature = "elixir")]
fn shape(node: tree_sitter::Node<'_>) -> String {
    let mut cursor = node.walk();
    let kinds: Vec<_> = node
        .children(&mut cursor)
        .map(|child| {
            let mark = if child.is_missing() {
                "!"
            } else if child.has_error() {
                "?"
            } else {
                ""
            };
            format!("{}{mark}", child.kind())
        })
        .collect();
    kinds.join(" ")
}

// Every node of the tree, in the order of the source.
#[cfg(feature = "elixir")]
fn preorder(tree: &tree_sitter::Tree) -> Vec<tree_sitter::Node<'_>> {
    let mut nodes = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        nodes.push(node);
        let children: Vec<_> = node.children(&mut node.walk()).collect();
        stack.extend(children.into_iter().rev());
    }
    nodes
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_parse_shapes_the_attribute_reader_relies_on() {
    // (the source, a node kind, which node of that kind in source order, its children)
    let shapes: [(&str, &str, usize, &str); 95] = [
        // a module: its word, its name, its body
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "call",
            0,
            "identifier arguments do_block",
        ),
        // a body: `do`, the statements, `end`
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator call end",
        ),
        // an attribute: the operator and its operand
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "unary_operator",
            0,
            "@ call",
        ),
        // the operand: the name and the value
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "call",
            1,
            "identifier arguments",
        ),
        // a value without parentheses
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "arguments",
            1,
            "string",
        ),
        // a plain string
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content \"",
        ),
        // a definition without a block
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "call",
            2,
            "identifier arguments",
        ),
        // its head is the first child
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "arguments",
            2,
            "call , keywords",
        ),
        // a head: the name and the argument list
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "call",
            3,
            "identifier arguments",
        ),
        // an empty argument list
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "arguments",
            3,
            "( )",
        ),
        // a value in parentheses
        (
            "defmodule M do\n@doc(\"audit\")\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "( string )",
        ),
        // after a space the parentheses are a block
        (
            "defmodule M do\n@doc (\"audit\")\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "block",
        ),
        // an empty string has no content
        (
            "defmodule M do\n@doc \"\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" \"",
        ),
        // a heredoc has other quotes
        (
            concat!(
                "defmodule M do\n@doc \"\"\"\naudit\n\"\"\"\ndef f(), do: :ok\nen",
                "d"
            ),
            "string",
            0,
            "\"\"\" quoted_content \"\"\"",
        ),
        // an escape is a child
        (
            "defmodule M do\n@doc \"a\\nb\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content escape_sequence quoted_content \"",
        ),
        // an interpolation is a child
        (
            "defmodule M do\n@doc \"a#{x}b\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content interpolation quoted_content \"",
        ),
        // a line end inside is content
        (
            "defmodule M do\n@doc \"au\ndit\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content \"",
        ),
        // a block after the value belongs to the operand
        (
            "defmodule M do\n@doc \"audit\" do\nend\ndef f(), do: :ok\nend",
            "call",
            1,
            "identifier arguments do_block",
        ),
        // two values
        (
            concat!(
                "defmodule M do\n@doc \"audit\", since: \"1\"\ndef f(), do: :ok\n",
                "end"
            ),
            "arguments",
            1,
            "string , keywords",
        ),
        // a comment before the operand; the operand is last
        (
            "defmodule M do\n@ # c\ndoc \"audit\"\ndef f(), do: :ok\nend",
            "unary_operator",
            0,
            "@ comment call",
        ),
        // a read has no call
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@impl true\n@deprecated \"x\"\n@",
                "foo 1\n@doc\n@doc()\ndef f(), do: :ok\nend"
            ),
            "unary_operator",
            4,
            "@ identifier",
        ),
        // a read with parentheses
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@impl true\n@deprecated \"x\"\n@",
                "foo 1\n@doc\n@doc()\ndef f(), do: :ok\nend"
            ),
            "arguments",
            5,
            "( )",
        ),
        // a bare head
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f\ndef f(), do: :ok\ndef f w",
                "hen true, do: :no\nend"
            ),
            "arguments",
            2,
            "identifier",
        ),
        // a guarded head is the first child
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when x > 0, do: x\ndef ",
                "f(x), do: x\nend"
            ),
            "arguments",
            2,
            "binary_operator , keywords",
        ),
        // the head is the left operand of `when`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when x > 0, do: x\ndef ",
                "f(x), do: x\nend"
            ),
            "binary_operator",
            0,
            "call when binary_operator",
        ),
        // a comment among the arguments
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(a, # c\n b), do: a\ndef f(",
                "a, b), do: b\ndef g(a, b: 1), do: a\ndef g(a, b), do: b\ndef f(a",
                "), do: a\nend"
            ),
            "arguments",
            3,
            "( identifier , comment identifier )",
        ),
        // `do` on the next row belongs to the definition
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x)\ndo\nx\nend\ndef f(y) d",
                "o\ny\nend\nend"
            ),
            "call",
            2,
            "identifier arguments do_block",
        ),
        // a definition call in parentheses starts with `(`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef(g(x), do: x)\ndef f(), do: :",
                "ok\nend"
            ),
            "arguments",
            2,
            "( call , keywords )",
        ),
        // a head without parentheses has no `(`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef g x, y do\nx\nend\ndef f(), ",
                "do: :ok\nend"
            ),
            "arguments",
            3,
            "identifier , identifier",
        ),
        // the name of this head is a call
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef unquote(name)(x), do: x\ndef",
                " f(), do: :ok\nend"
            ),
            "call",
            3,
            "call arguments",
        ),
        // the name of this head is a `dot`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef N.g(y), do: y\ndef f(), do: ",
                ":ok\nend"
            ),
            "call",
            3,
            "dot arguments",
        ),
        // the operator row is no statement
        (
            "defmodule M do\n@doc \"audit\"\n|> g()\ndef f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator call end",
        ),
        // it belongs to the value
        (
            "defmodule M do\n@doc \"audit\"\n|> g()\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "binary_operator",
        ),
        // also behind a comment row
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n# c\n|> g()\ndef f(), do: :ok\ne",
                "nd"
            ),
            "arguments",
            1,
            "binary_operator",
        ),
        // `do` on the next row belongs to the operand
        (
            "defmodule M do\n@doc \"audit\"\ndo\nend\ndef f(), do: :ok\nend",
            "call",
            1,
            "identifier arguments do_block",
        ),
        // a prefix operator starts a statement
        (
            "defmodule M do\n@doc \"audit\"\n- 1\ndef f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator unary_operator call end",
        ),
        // a continued row has no node
        (
            "defmodule M do\n@doc \"audit\"\n\\\ndef f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator call end",
        ),
        // `;` and a comment are children; a line end is none
        (
            concat!(
                "defmodule M do\n@doc \"audit\"; # note\n\n@spec f() :: atom()\nd",
                "ef f(), do: :ok\nend"
            ),
            "do_block",
            0,
            "do unary_operator ; comment unary_operator call end",
        ),
        // two statements on one row: no node shows it
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f() do\n:ok\nend def g(), do",
                ": :ok\nend"
            ),
            "do_block",
            0,
            "do unary_operator call call end",
        ),
        // the same after a heredoc
        (
            "defmodule M do\n@doc \"\"\"\naudit\n\"\"\" def f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator call end",
        ),
        // a missing `end` is an invented child
        (
            concat!(
                "defmodule M do\n@moduledoc \"m\"\n@doc \"audit\"\ndef f(), do: :",
                "ok\n"
            ),
            "do_block",
            0,
            "do unary_operator unary_operator call end!",
        ),
        // an `ERROR` child
        (
            "defmodule M do\n@doc \"audit\"\n1 def f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator ERROR? call end",
        ),
        // an attribute that holds an error
        (
            "defmodule M do\n@doc \"a\" \"b\"\ndef f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator? call end",
        ),
        // the keyword form has no body child
        (
            "defmodule M, do: (@doc \"audit\"; def f(), do: :ok)",
            "call",
            0,
            "identifier arguments",
        ),
        // a section is a child of the body
        (
            concat!(
                "defmodule M do\ndef f(), do: :ok\nelse\n@doc \"audit\"\ndef g(),",
                " do: 1\nend"
            ),
            "do_block",
            0,
            "do call else_block end",
        ),
        // a `quote` with a block
        (
            concat!(
                "defmodule M do\nquote do\n@doc \"audit\"\ndef f(), do: :ok\nend",
                "\nend"
            ),
            "call",
            1,
            "identifier do_block",
        ),
        // a qualified definition starts with a `dot`
        (
            concat!(
                "defmodule M do\nKernel.@(doc \"audit\")\nKernel.def f(), do: :ok",
                "\nend"
            ),
            "call",
            3,
            "dot arguments",
        ),
        // a read with a comment in its parentheses
        (
            "defmodule M do\n@doc( # c\n)\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "( comment )",
        ),
        // a comment before a value in parentheses
        (
            "defmodule M do\n@doc(# c\n\"audit\")\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "( comment string )",
        ),
        // a comment after the value
        (
            "defmodule M do\n@doc(\"audit\" # c\n)\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "( string comment )",
        ),
        // a comment before the `do` of a module
        (
            "defmodule M # c\ndo\n@doc \"audit\"\ndef f(), do: :ok\nend",
            "call",
            0,
            "identifier arguments comment do_block",
        ),
        // a comment before the `do` of a definition
        (
            "defmodule M do\n@doc \"audit\"\ndef f() # c\ndo\n:ok\nend\nend",
            "call",
            2,
            "identifier arguments comment do_block",
        ),
        // a comment after `when`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when # c\n is_atom(x), ",
                "do: x\nend"
            ),
            "binary_operator",
            0,
            "call when comment call",
        ),
        // a comment before `when`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) # c\n when is_atom(x), ",
                "do: x\nend"
            ),
            "binary_operator",
            0,
            "call comment when call",
        ),
        // a comment between the head and the keywords
        (
            "defmodule M do\n@doc \"audit\"\ndef f(), # c\n do: :ok\nend",
            "arguments",
            2,
            "call , comment keywords",
        ),
        // a comment in an empty argument list
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f( # c\n), do: :ok\ndef f(),",
                " do: :error\nend"
            ),
            "arguments",
            3,
            "( comment )",
        ),
        // the file: two modules on one row are two clean children
        (
            concat!(
                "defmodule A do\n@moduledoc \"a\"\nend defmodule X do\n@moduledoc",
                " \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend"
            ),
            "source",
            0,
            "call call",
        ),
        // a body: the same, and no child shows the error
        (
            concat!(
                "defmodule M do\ndef x() do\n:ok\nend defmodule X do\n@moduledoc ",
                "\"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "do_block",
            0,
            "do call call end",
        ),
        // the block of a definition: the expression before is an `ERROR`
        (
            concat!(
                "defmodule M do\ndef g() do\n:ok defmodule X do\n@moduledoc \"x\"",
                "\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
            ),
            "do_block",
            1,
            "do ERROR? call end",
        ),
        // a block of a call: two clean children again
        (
            concat!(
                "if true do\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc",
                " \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "do_block",
            0,
            "do anonymous_function call end",
        ),
        // an `after` block
        (
            concat!(
                "try do\n:ok\nafter\nfn -> :ok end defmodule X do\n@moduledoc \"x",
                "\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "after_block",
            0,
            "after anonymous_function call",
        ),
        // an `else` block
        (
            concat!(
                "if true do\n:ok\nelse\nfn -> :ok end defmodule X do\n@moduledoc ",
                "\"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "else_block",
            0,
            "else anonymous_function call",
        ),
        // a `rescue` block
        (
            concat!(
                "try do\n:ok\nrescue\nfn -> :ok end defmodule X do\n@moduledoc \"",
                "x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "rescue_block",
            0,
            "rescue anonymous_function call",
        ),
        // a `catch` block
        (
            concat!(
                "try do\n:ok\ncatch\nfn -> :ok end defmodule X do\n@moduledoc \"x",
                "\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "catch_block",
            0,
            "catch anonymous_function call",
        ),
        // the body of a clause
        (
            concat!(
                "f = fn ->\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "body",
            0,
            "anonymous_function call",
        ),
        // a parenthesized block
        (
            concat!(
                "(\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\n)"
            ),
            "block",
            0,
            "( anonymous_function call )",
        ),
        // an anonymous function: one clause that holds the second
        (
            concat!(
                "fn\n0 -> :ok 1 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\nend"
            ),
            "anonymous_function",
            0,
            "fn stab_clause? end",
        ),
        // the body of that clause stands behind an `ERROR`
        (
            concat!(
                "fn\n0 -> :ok 1 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\nend"
            ),
            "stab_clause",
            0,
            "arguments -> ERROR? body?",
        ),
        // a section follows the statements with no separator
        (
            concat!(
                "try do :ok rescue _ -> defmodule X do\n@moduledoc \"x\"\n@doc \"",
                "audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "do_block",
            0,
            "do atom rescue_block end",
        ),
        // a `;` or a line end between two statements is no node
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x)\ndef f(0), do: 0\ndef f",
                "(x), do: x\nend"
            ),
            "do_block",
            0,
            "do unary_operator call call call end",
        ),
        // the expression before is an `ERROR` in the operator
        (
            concat!(
                "x = 1 defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
                " do: :ok\nend"
            ),
            "binary_operator",
            0,
            "identifier = ERROR? call",
        ),
        // a string read as the key of a keyword
        (
            concat!(
                "defmodule M do\n\"s\" defmodule X do\n@moduledoc \"x\"\n@doc \"a",
                "udit\"\ndef f(), do: :ok\nend\nend"
            ),
            "pair",
            0,
            "quoted_keyword? call",
        ),
        // that key holds an error which no child shows
        (
            concat!(
                "defmodule M do\n\"s\" defmodule X do\n@moduledoc \"x\"\n@doc \"a",
                "udit\"\ndef f(), do: :ok\nend\nend"
            ),
            "quoted_keyword",
            0,
            "\" quoted_content \"",
        ),
        // the expression before is the left side; the `ERROR` is the next word
        (
            concat!(
                "1 y = defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
                " do: :ok\nend"
            ),
            "binary_operator",
            0,
            "integer ERROR? = call",
        ),
        // two definitions on one row: one call with the block of the second
        (
            concat!(
                "defmodule M do\ndef a(), do: :ok def g() do\ndefmodule X do\n@mo",
                "duledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
            ),
            "call",
            1,
            "identifier arguments? do_block",
        ),
        // the first body is an `ERROR` beside the second definition
        (
            concat!(
                "defmodule M do\ndef a(), do: :ok def g() do\ndefmodule X do\n@mo",
                "duledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
            ),
            "pair",
            0,
            "keyword ERROR? call",
        ),
        // an attribute and a definition on one row: one attribute with the block
        (
            concat!(
                "@doc \"before\" def g() do\ndefmodule X do\n@moduledoc \"x\"\n@d",
                "oc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "unary_operator",
            0,
            "@ call?",
        ),
        // its string is a key that holds an error which no child shows
        (
            concat!(
                "@doc \"before\" def g() do\ndefmodule X do\n@moduledoc \"x\"\n@d",
                "oc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "quoted_keyword",
            0,
            "\" quoted_content \"",
        ),
        // an `ERROR` among the clauses of a function
        (
            concat!(
                "f = fn\n1 1 -> :ok\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "anonymous_function",
            0,
            "fn ERROR? stab_clause stab_clause end",
        ),
        // an `ERROR` child of the file holds the module
        (
            concat!(
                "defmodule A do\n) ) (\ndefmodule M do\n@moduledoc \"m\"\n@doc \"",
                "audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "source",
            0,
            "call ERROR?",
        ),
        // a string without its end: the third quote is a child of an `ERROR`
        (
            concat!(
                "defmodule M do\nx = \"a\n@doc \"LIT\"\ndef lit, do: :ok\n@doc \"",
                "REAL\"\ndef real, do: :ok\nend"
            ),
            "ERROR",
            0,
            "string sigil_name \"",
        ),
        // a quote too many: the same
        (
            concat!(
                "defmodule M do\n@doc \"\"audit\"\ndef f(), do: :ok\n@doc \"next",
                "\"\ndef g(), do: :ok\nend"
            ),
            "ERROR",
            0,
            "\" \" \"",
        ),
        // an `ERROR` in a string holds no quote
        (
            "defmodule M do\n@doc \"a\u{0}b\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content ERROR? \"",
        ),
        // the right side of a match
        (
            concat!(
                "x = defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(), d",
                "o: :ok\nend"
            ),
            "binary_operator",
            0,
            "identifier = call",
        ),
        // an argument of a call
        (
            concat!(
                "IO.inspect(defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef",
                " f(), do: :ok\nend)"
            ),
            "arguments",
            0,
            "( call )",
        ),
        // `?\` before a line feed: a two-byte `char` child before the module
        (
            concat!(
                "?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            "source",
            0,
            "char call",
        ),
        // `?\` at the end of a clause gives two clean `stab_clause` children
        (
            concat!(
                "fn\n1 -> ?\\\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audi",
                "t\"\ndef f(), do: :ok\nend\nend"
            ),
            "anonymous_function",
            0,
            "fn stab_clause stab_clause end",
        ),
        // `x = ?\`: the `char` node is the last leaf of the operator
        (
            concat!(
                "x = ?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"funct",
                "ion\"\n  def f(), do: :ok\nend\n"
            ),
            "binary_operator",
            0,
            "identifier = char",
        ),
        // `?` before a line feed: an `ERROR` leaf of one byte
        (
            concat!(
                "?\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            "ERROR",
            0,
            "ERROR",
        ),
        // `???` before a line feed: an `ERROR` with `char` and an inner `ERROR`
        (
            concat!(
                "???\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            "ERROR",
            0,
            "char ERROR",
        ),
        // `foo,` before a line feed: the last leaf of the `ERROR` is `,`
        (
            concat!(
                "foo,\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            "ERROR",
            0,
            "identifier ,",
        ),
        // `?\` before CR LF: a three-byte `char` child that holds the CR
        (
            concat!(
                "?\\\r\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            "source",
            0,
            "char call",
        ),
        // `?` before CR LF: a two-byte `char` child that holds the CR
        (
            concat!(
                "?\r\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            "source",
            0,
            "char call",
        ),
        // an empty clause body: `b -> defmodule X` is a call with an `ERROR` `->` and
        //     `arguments`
        (
            concat!(
                "case x do\n  a ->\n  b -> defmodule X do\n    @moduledoc \"m\"\n",
                "    @doc \"d\"\n    def f(), do: :ok\n  end\nend\n"
            ),
            "call",
            1,
            "identifier ERROR? arguments do_block",
        ),
        // `-` on the row behind `?\` starts a `unary_operator` statement
        (
            concat!(
                "?\\\n- defmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            "source",
            0,
            "char unary_operator",
        ),
    ];
    for (source, kind, nth, children) in shapes {
        let parser = parsed(source.as_bytes());
        let nodes = preorder(parser.ts_tree());
        let node = nodes.iter().filter(|node| node.kind() == kind).nth(nth);
        assert_eq!(shape(*node.unwrap()), children, "{kind} {nth} of {source}");
        // The reader takes a child by its place among the children that are no comment.
        // The grammar's field names say the same: the word of a call is its first child,
        // an operand is the last child of its operator, the sides of a binary operator
        // stand around it, and the quotes of a string are its first and last child.
        for node in nodes.iter().filter(|node| !node.has_error()) {
            let children: Vec<_> = node
                .children(&mut node.walk())
                .filter(|child| child.kind() != "comment")
                .collect();
            let fields: &[(&str, usize)] = match node.kind() {
                "call" => &[("target", 0)],
                "unary_operator" => &[("operator", 0), ("operand", children.len() - 1)],
                "binary_operator" => &[("left", 0), ("operator", 1), ("right", 2)],
                "string" => &[("quoted_start", 0), ("quoted_end", children.len() - 1)],
                _ => &[],
            };
            for (field, place) in fields {
                assert_eq!(
                    node.child_by_field_name(field),
                    children.get(*place).copied(),
                    "{field} of {kind} in {source}"
                );
            }
        }
    }
    // Two statements on one row: the body holds a syntax error that no child shows. The
    // reader therefore reads the bytes between two statements itself.
    for source in [
        "defmodule M do\n@doc \"audit\"\ndef f() do\n:ok\nend def g(), do: :ok\nend",
        "defmodule M do\n@doc \"\"\"\naudit\n\"\"\" def f(), do: :ok\nend",
    ] {
        let parser = parsed(source.as_bytes());
        let nodes = preorder(parser.ts_tree());
        let body = nodes.iter().find(|node| node.kind() == "do_block").unwrap();
        assert!(body.has_error(), "{source}");
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            assert!(!child.has_error() && !child.is_missing(), "{source}");
        }
    }
}
