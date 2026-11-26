//! Parse shapes of the pinned Tcl grammar that a reader of static Ruff! documentation
//! relies on: the children of a `proc`, of its body and of the file, as the cursor names
//! them. The test parses verbatim bytes and collects nothing.

#[cfg(feature = "tcl")]
use crate::{AnyParser, LANG};
#[cfg(feature = "tcl")]
use std::ops::Range;

#[cfg(feature = "tcl")]
fn parsed(source: &[u8]) -> AnyParser {
    AnyParser::parse(LANG::Tcl, source.to_vec(), None, None).unwrap()
}

/// The one node of that kind and byte range.
#[cfg(feature = "tcl")]
fn node_at<'a>(
    tree: &'a tree_sitter::Tree,
    kind: &str,
    range: Range<usize>,
) -> tree_sitter::Node<'a> {
    let mut found = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == kind && node.byte_range() == range {
            found.push(node);
        }
        stack.extend(node.children(&mut node.walk()));
    }
    let [found] = found[..] else {
        panic!("one {kind} at {range:?}, found {found:?}")
    };
    found
}

/// The children of a node as the cursor names them: field, kind, byte range, and
/// whether the parser invented the token.
#[cfg(feature = "tcl")]
fn children(node: tree_sitter::Node<'_>) -> Vec<String> {
    let mut cursor = node.walk();
    let mut lines = Vec::new();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            lines.push(format!(
                "{}{} {}..{}{}{}",
                cursor
                    .field_name()
                    .map(|field| format!("{field}: "))
                    .unwrap_or_default(),
                child.kind().escape_default(),
                child.start_byte(),
                child.end_byte(),
                if child.is_missing() { " missing" } else { "" },
                if child.has_error() && !child.is_missing() {
                    " has_error"
                } else {
                    ""
                },
            ));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    lines
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_parse_shapes_the_ruff_reader_relies_on() {
    let shape = |source: &[u8], kind: &str, range: Range<usize>| {
        let parser = parsed(source);
        children(node_at(parser.ts_tree(), kind, range))
    };
    // Without a final line end the parser invents a zero-width one after the procedure.
    let primary: &[u8] = b"proc f {} {\n# audit\nreturn\n}";
    assert_eq!(
        shape(primary, "source_file", 0..28),
        ["procedure 0..28", "\\n 28..28 missing"]
    );
    assert_eq!(
        shape(primary, "procedure", 0..28),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "arguments: arguments 7..9",
            "body: braced_word 10..28"
        ]
    );
    assert_eq!(
        shape(primary, "braced_word", 10..28),
        [
            "{ 10..11",
            "comment 12..19",
            "\\n 19..20",
            "command 20..26",
            "\\n 26..27",
            "} 27..28"
        ]
    );
    // The comment token of a CRLF row holds the CR.
    let crlf: &[u8] = b"proc f {} {\r\n# audit\r\n#ruff\r\n# more\r\nreturn\r\n}\r\n";
    assert_eq!(
        shape(crlf, "braced_word", 10..46),
        [
            "{ 10..11",
            "comment 13..21",
            "\\n 21..22",
            "comment 22..28",
            "\\n 28..29",
            "comment 29..36",
            "\\n 36..37",
            "command 37..43",
            "\\n 44..45",
            "} 45..46"
        ]
    );
    // One line-end token covers a blank row. A row of spaces gives two tokens.
    assert_eq!(
        shape(
            b"proc f {} {\n# first\n\n# second\nreturn\n}\n",
            "braced_word",
            10..38
        )[1..4],
        ["comment 12..19", "\\n 19..21", "comment 21..29"]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# a\n   \n# b\nreturn\n}\n",
            "braced_word",
            10..32
        )[1..5],
        [
            "comment 12..15",
            "\\n 15..16",
            "\\n 19..20",
            "comment 20..23"
        ]
    );
    // A row that holds only a backslash is no node with LF line ends, and a command
    // with CRLF line ends (#1802). The row below a continued comment is a `procedure`
    // without any recovery node.
    let bare: &[u8] = b"# c \\\n\\\nproc g {} {\n# inner\n}\n";
    assert!(!parsed(bare).root_node().has_error());
    assert_eq!(
        shape(bare, "source_file", 0..30),
        ["comment 0..5", "\\n 5..6", "procedure 8..29", "\\n 29..30"]
    );
    let bare: &[u8] = b"# c \\\r\n\\\r\nproc g {} {\r\n# inner\r\n}\r\n";
    assert!(!parsed(bare).root_node().has_error());
    assert_eq!(
        shape(bare, "source_file", 0..35),
        [
            "comment 0..6",
            "\\n 6..7",
            "command 7..9",
            "\\n 9..10",
            "procedure 10..33",
            "\\n 34..35"
        ]
    );
    assert_eq!(
        shape(bare, "command", 7..9),
        ["name: escaped_character 7..9"]
    );
    // `proc f` and its other words on the next row are one clean `procedure`.
    let split: &[u8] = b"proc f\n{} {\n# audit\n}\nproc g {} {\n# doc g\n}\n";
    assert!(!parsed(split).root_node().has_error());
    assert_eq!(
        shape(split, "procedure", 0..21),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "arguments: arguments 7..9",
            "body: braced_word 10..21"
        ]
    );
    // A word after the body, or a second command on the row, follows an invented
    // zero-width line end.
    assert_eq!(
        shape(
            b"proc f {} {\n# audit\n} extra\nproc g {} {\n# doc g\n}\n",
            "source_file",
            0..50
        )[..3],
        ["procedure 0..21", "\\n 21..21 missing", "command 22..27"]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# a\n} proc g {} {\n# b\n}\n",
            "source_file",
            0..36
        )[..3],
        ["procedure 0..17", "\\n 17..17 missing", "procedure 18..35"]
    );
    assert_eq!(
        shape(b"proc f {} {\n# a\n}   ", "source_file", 0..20),
        ["procedure 0..17", "\\n 17..17 missing"]
    );
    // A `;` after a word is a recovery node or a missing token, though valid Tcl.
    assert_eq!(
        shape(
            b"proc f {} {\n# a\nreturn;# done\n#ruff b\n}\n",
            "braced_word",
            10..39
        )[1..6],
        [
            "comment 12..15",
            "\\n 15..16",
            "ERROR 16..22 has_error",
            "; 22..23",
            "comment 23..29"
        ]
    );
    // A recovery node can hold the first rows of a body, and a NUL byte cuts a comment.
    let hidden: &[u8] = b"proc f {} {\n# a\nset x 1 # text\n#ruff\n# later\n}\n";
    assert_eq!(
        shape(hidden, "braced_word", 10..46)[1..3],
        ["ERROR 12..23 has_error", "comment 24..30"]
    );
    assert_eq!(
        shape(hidden, "ERROR", 12..23),
        ["comment 12..15", "\\n 15..16", "set 16..23"]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# a\n# b\x00c\nreturn\n}\n",
            "braced_word",
            10..30
        )[1..5],
        [
            "comment 12..15",
            "\\n 15..16",
            "comment 16..19",
            "ERROR 19..28 has_error"
        ]
    );
    // A recovery node can start on the row of a command and reach the next row, where
    // it holds a marker row. A NUL byte cuts a marker row after a command as well.
    assert_eq!(
        shape(
            b"proc f {} {\n# lead\nset x 1\n#ruff a\nset y 2 # text\n#ruff c\n}\n",
            "braced_word",
            10..59
        )[5..9],
        [
            "comment 27..34",
            "\\n 34..35",
            "set 35..42",
            "ERROR 43..57 has_error"
        ]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# lead\nset x 1\n#ruff b\x00c\nreturn\n}\n",
            "braced_word",
            10..45
        )[5..7],
        ["comment 27..34", "ERROR 34..43 has_error"]
    );
    // A stray `]` on a row of its own is a recovery node of one row.
    assert_eq!(
        shape(b"proc f {} {\n# a\n]\n# b\n}\n", "braced_word", 10..23)[1..6],
        [
            "comment 12..15",
            "\\n 15..16",
            "ERROR 16..17 has_error",
            "\\n 17..18",
            "comment 18..21"
        ]
    );
    // A comment row in a nested body is no child of the procedure's body.
    assert_eq!(
        shape(
            b"proc f {} {\nif {$x} {\n#ruff nested\n}\n}\n",
            "braced_word",
            10..38
        ),
        ["{ 10..11", "if 12..36", "\\n 36..37", "} 37..38"]
    );
    // A name of two parts gives two `name` children, so the procedure has five.
    assert_eq!(
        shape(b"proc a$b {} {\n# c\n}\n", "procedure", 0..19),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "name: variable_substitution 6..8",
            "arguments: arguments 9..11",
            "body: braced_word 12..19"
        ]
    );
    // A procedure of four children is `proc`, one name word, `arguments` and one body
    // word, in that order, and a body starts and ends with a brace exactly when it is a
    // `braced_word`. The reader tells the words apart by this and asks for no field name.
    for (source, range, name, body) in [
        (
            &b"proc f {} {\n# audit\nreturn\n}"[..],
            0..28,
            "simple_word",
            "braced_word",
        ),
        (
            b"proc ::ns::f {} {\n# n\n}\n",
            0..23,
            "simple_word",
            "braced_word",
        ),
        (
            b"proc f args {\n# a\n}\n",
            0..19,
            "simple_word",
            "braced_word",
        ),
        (
            b"proc $name {} {\n# dyn\n}\n",
            0..23,
            "variable_substitution",
            "braced_word",
        ),
        (
            b"proc [mk] {} {\n# dyn\n}\n",
            0..22,
            "command_substitution",
            "braced_word",
        ),
        (
            b"proc [a;] {} {\n# c\n}\n",
            0..20,
            "command_substitution",
            "braced_word",
        ),
        (
            b"proc \"g\" {} {\n# q\n}\n",
            0..19,
            "quoted_word",
            "braced_word",
        ),
        (
            b"proc {h} {} {\n# b\n}\n",
            0..19,
            "braced_word",
            "braced_word",
        ),
        (
            b"proc f {a;} {\n# c\n}\n",
            0..19,
            "simple_word",
            "braced_word",
        ),
        (
            b"proc f {} \"\n# q\n\"\n",
            0..17,
            "simple_word",
            "quoted_word",
        ),
        (b"proc f {} return\n", 0..16, "simple_word", "simple_word"),
        (
            b"proc f {} $body\n",
            0..15,
            "simple_word",
            "variable_substitution",
        ),
        (
            b"proc f {}\nproc g {} {\n# doc g\nreturn\n}\n",
            0..14,
            "simple_word",
            "simple_word",
        ),
    ] {
        let parser = parsed(source);
        let procedure = node_at(parser.ts_tree(), "procedure", range);
        let words: Vec<_> = procedure.children(&mut procedure.walk()).collect();
        let kinds = |nodes: &[tree_sitter::Node<'_>]| -> Vec<_> {
            nodes.iter().map(tree_sitter::Node::kind).collect()
        };
        assert_eq!(kinds(&words), ["proc", name, "arguments", body]);
        assert_eq!(procedure.child_by_field_name("name"), Some(words[1]));
        assert_eq!(procedure.child_by_field_name("arguments"), Some(words[2]));
        assert_eq!(procedure.child_by_field_name("body"), Some(words[3]));
        let inner = kinds(&words[3].children(&mut words[3].walk()).collect::<Vec<_>>());
        let ends = (inner.first().copied(), inner.last().copied());
        assert_eq!(ends == (Some("{"), Some("}")), body == "braced_word");
    }
    // A backslash with its line end between two children of a body is in no node. The
    // grammar ends a recovery node before it and reads a comment after it; below a
    // continued `if` the comment is itself a recovery node.
    let continued: &[u8] = b"proc f {} {\nset x 1 \\\n#ruff notdoc\n}\n";
    assert_eq!(
        shape(continued, "braced_word", 10..36)[1..3],
        ["ERROR 12..19 has_error", "comment 22..34"]
    );
    assert_eq!(continued[19..22], *b" \\\n");
    assert_eq!(
        shape(
            b"proc f {} {\nif {1} {} \\\n#ruff b\n}\n",
            "braced_word",
            10..33
        )[1..3],
        ["if 12..21", "ERROR 24..31 has_error"]
    );
    // With CRLF the backslash and the CR are a command of their own.
    assert_eq!(
        shape(
            b"proc f {} {\r\nset x 1 \\\r\n#ruff b\r\n}\r\n",
            "braced_word",
            10..34
        )[1..5],
        [
            "ERROR 13..20 has_error",
            "command 21..23",
            "\\n 23..24",
            "comment 24..32"
        ]
    );
    // A row that holds only a backslash, between two comment rows, is no node.
    assert_eq!(
        shape(b"proc f {} {\n# lead\n\\\n# b\n}\n", "braced_word", 10..26)[1..4],
        ["comment 12..18", "\\n 18..19", "comment 21..24"]
    );
    // After a comment that continues the grammar reads `;` and a procedure, with no
    // recovery node. After `;` and an escaped line end it reads a procedure as well.
    let swallowed: &[u8] = b"# c \\\n;proc f {} {\n# doc\n}\n";
    assert!(!parsed(swallowed).root_node().has_error());
    assert_eq!(
        shape(swallowed, "source_file", 0..27),
        [
            "comment 0..5",
            "\\n 5..6",
            "; 6..7",
            "procedure 7..26",
            "\\n 26..27"
        ]
    );
    assert_eq!(
        shape(
            b"set x 1 ;\\\nproc g {} {\n# doc\n}\n",
            "source_file",
            0..31
        ),
        ["set 0..7", "; 8..9", "procedure 11..30", "\\n 30..31"]
    );
    // An unclosed brace gives a command that holds a recovery node, and a `#` that
    // starts no command gives a recovery node that holds a comment. A `;` and a clean
    // procedure follow both.
    assert_eq!(
        shape(b"set x {a ; proc g {} {\n# doc\n}\n", "source_file", 0..31),
        [
            "set 0..8 has_error",
            "; 9..10",
            "procedure 11..30",
            "\\n 30..31"
        ]
    );
    assert_eq!(
        shape(
            b"set x 1 # c \\\n; proc g {} {\n# doc\n}\n",
            "source_file",
            0..36
        ),
        [
            "set 0..7",
            "ERROR 8..13 has_error",
            "\\n 13..14",
            "; 14..15",
            "procedure 16..35",
            "\\n 35..36"
        ]
    );
    // White space for the grammar is a space, a tab, CR, LF, a vertical tab and a form
    // feed, and it is in no node. A CR alone ends no command: the grammar invents a
    // line end before the procedure, and reads `proc f`, CR and the other words as one
    // clean procedure. A no-break space is a byte of a word, as it is for Tcl.
    assert_eq!(
        shape(b"set x 1\rproc g {} {\n# doc\n}\n", "source_file", 0..28),
        [
            "set 0..7",
            "\\n 7..7 missing",
            "procedure 8..27",
            "\\n 27..28"
        ]
    );
    let returned: &[u8] = b"proc f\r{} {\n# doc\n}\n";
    assert!(!parsed(returned).root_node().has_error());
    assert_eq!(
        shape(returned, "procedure", 0..19),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "arguments: arguments 7..9",
            "body: braced_word 10..19"
        ]
    );
    assert_eq!(
        shape(b"\x0cproc g {} {\n# doc\n}\n", "source_file", 1..21),
        ["procedure 1..20", "\\n 20..21"]
    );
    assert_eq!(
        shape(b"\xc2\xa0proc g {} {\n# doc\n}\n", "source_file", 0..22),
        ["command 0..21", "\\n 21..22"]
    );
    // An unclosed parameter list makes the root an `ERROR` that holds a procedure.
    let parser =
        parsed(b"proc f {{a0 { {a1 { {a2 { {a3 {} {\n# audit\n}\nproc g {} {\n# doc g\n}\n");
    assert!(parser.ts_tree().root_node().is_error());
    assert_eq!(
        node_at(parser.ts_tree(), "procedure", 45..66).parent(),
        Some(parser.ts_tree().root_node())
    );
    // Where a braced word ends. The grammar reads a comment and a quoted word inside the
    // word first: a brace in one of them does not count for it, as it does for Tcl. The
    // first body ends at the `}` on the third row, and the root has no error.
    let pair: &[u8] = b"proc g {} {\n# {\n}\nproc f {} {\n# doc\n}\n# }\n";
    assert!(!parsed(pair).root_node().has_error());
    assert_eq!(
        shape(pair, "source_file", 0..42),
        [
            "procedure 0..17",
            "\\n 17..18",
            "procedure 18..37",
            "\\n 37..38",
            "comment 38..41",
            "\\n 41..42"
        ]
    );
    assert_eq!(
        shape(pair, "braced_word", 10..17),
        ["{ 10..11", "comment 12..15", "\\n 15..16", "} 16..17"]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# doc\nset s \"{\"\n}\n# more\n}\n",
            "braced_word",
            10..29
        ),
        [
            "{ 10..11",
            "comment 12..17",
            "\\n 17..18",
            "set 18..27",
            "\\n 27..28",
            "} 28..29"
        ]
    );
    // A braced word is a node whose first child is `{`, also where the parser invents
    // the `}`. In a recovery node an open `{` is a token with no word around it.
    assert_eq!(
        shape(
            b"set x {a ;\nproc g {} {\n# doc\n}\n",
            "braced_word_simple",
            6..8
        ),
        ["{ 6..7", "simple_word 7..8", "} 8..8 missing"]
    );
    assert_eq!(
        shape(b"set s $a# {\nproc f {} {\n# doc\n}\n", "ERROR", 0..11),
        ["set 0..9", "{ 10..11"]
    );
    // Tokens that stand directly before a `{`: a part of a word, where the brace starts
    // no word for Tcl, and `{*}`, `[` and `$`.
    assert_eq!(
        shape(b"set s a{b ;\nproc g {} {\n# doc\n}\n", "set", 0..9),
        [
            "set 0..3",
            "id 4..5",
            "simple_word 6..7",
            "braced_word_simple 7..9 has_error"
        ]
    );
    assert_eq!(
        shape(
            b"list {*}{# {\n}\nproc g {} {\n# doc\n}\n# }\n",
            "word_list",
            5..14
        ),
        ["unpack 5..8", "braced_word 8..14"]
    );
    let bracket: &[u8] = b"set s [{# {\n} a]\nproc g {} {\n# doc\n}\n# }\n";
    assert_eq!(
        shape(bracket, "command_substitution", 6..16),
        ["[ 6..7", "command 7..15", "] 15..16"]
    );
    assert_eq!(
        shape(bracket, "command", 7..15),
        ["name: braced_word 7..13", "arguments: word_list 14..15"]
    );
    assert_eq!(
        shape(
            b"set v ${name}\nproc f {} {\n# doc\n}\n",
            "variable_substitution",
            6..13
        ),
        ["$ 6..7", "{ 7..8", "} 12..13"]
    );
    // Two braced words of a procedure that nothing separates are a clean procedure.
    let glued: &[u8] = b"proc f {}{\n# doc\n}\n";
    assert!(!parsed(glued).root_node().has_error());
    assert_eq!(
        shape(glued, "procedure", 0..18),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "arguments: arguments 7..9",
            "body: braced_word 9..18"
        ]
    );
    // After a command of fixed word count the grammar reads `#` as the start of a
    // comment. For Tcl it is a word, and the `{` after it opens a word.
    assert_eq!(
        shape(
            b"set x 1 # c {\nproc f {} {\n# doc\n}\n}\n",
            "source_file",
            0..36
        ),
        [
            "ERROR 0..7 has_error",
            "comment 8..13",
            "\\n 13..14",
            "procedure 14..33",
            "\\n 33..34",
            "ERROR 34..35 has_error"
        ]
    );
    // The grammar reads a comment to the line feed. For Tcl a carriage return alone ends
    // it, so the brace after the CR is a token of no node.
    assert_eq!(
        shape(
            b"# c\rset x {\nproc f {} {\n# doc\n}\n}\n",
            "source_file",
            0..34
        ),
        [
            "comment 0..11",
            "\\n 11..12",
            "procedure 12..31",
            "\\n 31..32",
            "ERROR 32..33 has_error"
        ]
    );
    // The grammar skips a byte order mark: the root starts behind it.
    assert_eq!(
        shape(b"\xef\xbb\xbfproc f {} {\n# doc\n}\n", "source_file", 3..23),
        ["procedure 3..22", "\\n 22..23"]
    );
    // A comment that continues, and the rows Tcl reads as its text: the grammar gives
    // them nodes of their own, and no error.
    let continued: &[u8] = b"# c \\\nproc g {} {\n# { \\\n}\nproc f {} {\n# doc\n}\n# }\n";
    assert!(!parsed(continued).root_node().has_error());
    assert_eq!(
        shape(continued, "source_file", 0..50),
        [
            "comment 0..5",
            "\\n 5..6",
            "procedure 6..25",
            "\\n 25..26",
            "procedure 26..45",
            "\\n 45..46",
            "comment 46..49",
            "\\n 49..50"
        ]
    );
    // A bracket whose script starts with a comment: the grammar closes the bracket at
    // the end of the row and puts the procedure at the root.
    assert_eq!(
        shape(b"[# c\nproc f {} {\n# doc\n};]", "source_file", 0..26),
        [
            "command 0..4 has_error",
            "\\n 4..5",
            "procedure 5..24",
            "; 24..25",
            "ERROR 25..26 has_error"
        ]
    );
    // A bracket glued before `proc`: a plain command, or, behind a recovery node, a
    // procedure of the root that starts after the bracket.
    assert_eq!(
        shape(b"[]proc f {} {\n# doc\n}\n", "source_file", 0..22),
        ["command 0..21 has_error", "\\n 21..22"]
    );
    assert_eq!(
        shape(b"set x 1];[]proc f {} {\n# doc\n}\n", "source_file", 0..31),
        ["ERROR 0..11 has_error", "procedure 11..30", "\\n 30..31"]
    );
    // A quoted parameter list: a recovery node and no procedure.
    assert_eq!(
        shape(b"proc f \"x\" {\n# doc\n}\n", "source_file", 0..21),
        ["ERROR 0..20 has_error", "\\n 20..21"]
    );
}
