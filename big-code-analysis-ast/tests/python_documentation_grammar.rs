//! Pinned Python grammar evidence for the docstring adapter: the shape of a string,
//! of the owners and runs the reader walks, of parser recovery around a string, and
//! of parses with excluded ranges, with carriage returns and without a final line feed.
#![cfg(feature = "python")]
#![allow(
    clippy::naive_bytecount,
    clippy::single_range_in_vec_init,
    clippy::too_many_lines
)]

use std::ops::Range;
use tree_sitter::{Node, Tree};

const STRING: u16 = 231;
const STRING_START: u16 = 104;
const STRING_CONTENT: u16 = 232;
const STRING_END: u16 = 107;

/// Parses `source`, whole when `included` is empty and otherwise with exactly those
/// byte ranges. A point is the number of line feeds before the offset and the bytes
/// since the last one.
fn parse(source: &[u8], included: &[Range<usize>]) -> Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    if !included.is_empty() {
        let point = |at: usize| {
            let prefix = &source[..at];
            tree_sitter::Point::new(
                prefix.iter().filter(|&&byte| byte == b'\n').count(),
                prefix
                    .iter()
                    .rposition(|&byte| byte == b'\n')
                    .map_or(at, |lf| at - lf - 1),
            )
        };
        let ranges: Vec<_> = included
            .iter()
            .map(|range| tree_sitter::Range {
                start_byte: range.start,
                end_byte: range.end,
                start_point: point(range.start),
                end_point: point(range.end),
            })
            .collect();
        parser.set_included_ranges(&ranges).unwrap();
    }
    let tree = parser.parse(source, None).unwrap();
    if std::env::var_os("BCA_DUMP_PYTHON_DOC_GRAMMAR").is_some() {
        dump(source, &tree);
    }
    tree
}

fn whole(source: &[u8]) -> Tree {
    parse(source, &[])
}

fn dump(source: &[u8], tree: &Tree) {
    println!("SOURCE {source:?} INCLUDED {:?}", tree.included_ranges());
    for (depth, node) in preorder(tree) {
        println!(
            "{depth}: {} id={} named={} bytes={:?} has_error={} is_error={} missing={}",
            node.kind(),
            node.kind_id(),
            node.is_named(),
            node.byte_range(),
            node.has_error(),
            node.is_error(),
            node.is_missing(),
        );
    }
}

/// Every node of `tree` with its depth, parents before children.
fn preorder(tree: &Tree) -> Vec<(usize, Node<'_>)> {
    let mut nodes = Vec::new();
    let mut cursor = tree.walk();
    let mut depth = 0;
    loop {
        nodes.push((depth, cursor.node()));
        if cursor.goto_first_child() {
            depth += 1;
        } else {
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return nodes;
                }
                depth -= 1;
            }
        }
    }
}

/// The one node of `kind` with exactly `range`.
fn node<'t>(tree: &'t Tree, kind: &str, range: Range<usize>) -> Node<'t> {
    let found: Vec<_> = preorder(tree)
        .into_iter()
        .map(|(_, node)| node)
        .filter(|node| node.kind() == kind && node.byte_range() == range)
        .collect();
    assert_eq!(found.len(), 1, "{kind} {range:?}");
    found[0]
}

fn count(tree: &Tree, kind: &str) -> usize {
    preorder(tree)
        .iter()
        .filter(|(_, node)| node.kind() == kind)
        .count()
}

/// The direct children of `node`: kind and byte range, in order.
fn children(node: Node<'_>) -> Vec<(&'static str, Range<usize>)> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .map(|child| (child.kind(), child.byte_range()))
        .collect()
}

/// No node of `tree` is an `ERROR`, is missing or holds either.
fn clean(tree: &Tree) {
    for (_, node) in preorder(tree) {
        assert!(!node.has_error(), "{} {:?}", node.kind(), node.byte_range());
        assert!(!node.is_error(), "{} {:?}", node.kind(), node.byte_range());
        assert!(
            !node.is_missing(),
            "{} {:?}",
            node.kind(),
            node.byte_range()
        );
    }
}

/// A `string` with exactly `range` whose children are a `string_start`, an optional
/// `string_content` and a `string_end` with the given ranges.
fn string(tree: &Tree, range: Range<usize>, parts: &[Range<usize>]) {
    let string = node(tree, "string", range);
    assert_eq!(string.kind_id(), STRING);
    let kinds: &[(&str, u16)] = if parts.len() == 3 {
        &[
            ("string_start", STRING_START),
            ("string_content", STRING_CONTENT),
            ("string_end", STRING_END),
        ]
    } else {
        &[("string_start", STRING_START), ("string_end", STRING_END)]
    };
    assert_eq!(parts.len(), kinds.len());
    let mut cursor = string.walk();
    let found: Vec<_> = string.children(&mut cursor).collect();
    assert_eq!(found.len(), kinds.len());
    for ((child, (kind, id)), part) in found.iter().zip(kinds).zip(parts) {
        assert_eq!(child.kind(), *kind);
        assert_eq!(child.kind_id(), *id);
        assert_eq!(child.byte_range(), *part);
    }
}

const P1: &[u8] = b"def f():\n    \"audit\"\n    pass";
const P2A: &[u8] = b"class C:\n    \"audit\"\n    pass";
const P2B: &[u8] = b"\"audit\"\nx = 1";
const P3: &[u8] = b"def f():\n    \"\"\n    \"extra\"\n    return 1\n";
const P4A: &[u8] = b"def f():\n    (\"@au\" # ordinary\n     \"dit\")\n";
const P4B: &[u8] = b"def f():\n    R\"a\\nb\" u\"c\"\n";
const P5A: &[u8] = b"\"a\\x40b\"\nx = 1";
const P5B: &[u8] = b"def f():\n    r\"a\\nb\"\n    pass";
const P5F: &[u8] = b"def f():\n    u\"a\\x40b\"\n";
const P7A: &[u8] = b"def f():\n    \"\"\"First.\n\n    Second.\n    \"\"\"\n";
const P8A: &[u8] = b"def f():\r\n    \"\"\"First.\r\n\r\n    Second.\r\n    \"\"\"\r\n";
const P8B: &[u8] = b"def f():\n    \"\"\"a\rb\"\"\"\n";
const P8C: &[u8] = b"def f():\n    (\"a\"\n     \"\"\"b\rc\"\"\")\n";
const P8D: &[u8] = b"def f():\n    \"\"\"a\rb\"\"\"\n    def g():\n        return 1\n";
const P9A: &[u8] = b"\"\"\"a\"\"\"\n\"\"\"b\"\"\"\n# c\n\"\"\"c\"\"\"\nx = 1\n\"\"\"d\"\"\"\n";
const P9C: &[u8] = b"x = 1\n\"x doc\"\n\"more\"\ndef f():\n    return 1\n";
const P9D: &[u8] = b"class C:\n    x = 1\n    \"x doc\"\n    y = = 2\n";
const P11A: &[u8] = b"@d\ndef f():\n    \"outer\"\n    def g():\n        \"inner\"";
const P11C: &[u8] = b"async def f():\n    \"audit\"\n";
const P14A: &[u8] = b"def f():\n    \"audit\"\n    x = = 1\n";
const P14B: &[u8] = b"def f():\n    \"audit\"\n    x = (\n";
const P14C: &[u8] = b"\"doc\"\nx = = 1\n";
const P14D: &[u8] = b"x = [\ndef f():\n    \"doc\"\n";
const P17A: &[u8] = b"\"au\0dit\"\nx = 1";
const P17B: &[u8] = b"\"c\0d\" \"\"\"a\rb\"\"\"\nx = 1";
const P18A: &[u8] = b"\"a\";\n\"b\"\nx = 1\n";
const P18C: &[u8] = b"def f():\n    \"a\"; \"b\"\n    return 1\n";
const P18E: &[u8] = b"x = 1; \"attribute\"\n";
const P20A: &[u8] = b"\"a\"\nx = 1\n\"b\"\ny = 2\n";
const P20B: &[u8] = b"x = 1\n\"doc\"\ndef f():\n    \"inner\"\n";
const P20C: &[u8] = b"def f():\n    x = 1\n    \"doc\"\n    return 1\n";
const P21: &[u8] = b"\"a\" + \"b\"\nx = 1\n";
const P22: &[u8] = b"def f():\n    (\"First.\\n    \" \"Second.\")\n";
const P23A: &[u8] = b"\"\"\"a\"\"\"\n(1 . \"b\")\ny = 2\n";
const P23B: &[u8] = b"x = 1\n(1 . \"b\")\n";
const P24A: &[u8] = b"(\"a\" 1 \"b\")\nx = 1\n";
const P24B: &[u8] = b"(\"a\" \"b\" 1)\nx = 1\n";
const CONTINUATION: &[u8] =
    include_bytes!("../../tests/fixtures/documentation/sources/python-continuation.py");

#[test]
fn python_documentation_grammar_string_shapes() {
    let tree = whole(P1);
    assert_eq!(tree.root_node().end_byte(), 29);
    string(&tree, 13..20, &[13..14, 14..19, 19..20]);
    // The empty string has no `string_content` child.
    string(&whole(P3), 13..15, &[13..14, 14..15]);
    // A raw literal: the escape is plain content, with no child.
    let tree = whole(P5B);
    string(&tree, 13..20, &[13..15, 15..19, 19..20]);
    assert_eq!(node(&tree, "string_content", 15..19).child_count(), 0);
    let tree = whole(P4B);
    assert_eq!(node(&tree, "concatenated_string", 13..25).kind_id(), 230);
    node(&tree, "string_start", 13..15);
    node(&tree, "string_start", 21..23);
    let tree = whole(P5A);
    assert_eq!(
        children(node(&tree, "string_content", 1..7)),
        [("escape_sequence", 2..6)]
    );
    assert_eq!(node(&tree, "escape_sequence", 2..6).kind_id(), 89);
    let tree = whole(P5F);
    node(&tree, "string_start", 13..15);
    node(&tree, "escape_sequence", 16..20);
    let tree = whole(P7A);
    assert_eq!(tree.root_node().end_byte(), 44);
    string(&tree, 13..43, &[13..16, 16..40, 40..43]);
    // With its carriage returns, as the collector tests parse it.
    let tree = whole(P8A);
    assert_eq!(tree.root_node().end_byte(), 49);
    clean(&tree);
    node(&tree, "function_definition", 0..47);
    string(&tree, 14..47, &[14..17, 17..44, 44..47]);
    let tree = whole(P8B);
    assert_eq!(tree.root_node().end_byte(), 23);
    clean(&tree);
    node(&tree, "function_definition", 0..22);
    node(&tree, "string", 13..22);
    for prefix in ["f", "t", "b"] {
        let source = format!("def f():\n    {prefix}\"audit\"\n    pass");
        let tree = whole(source.as_bytes());
        node(&tree, "string", 13..21);
        node(&tree, "string_start", 13..15);
    }
    let tree = whole(b"\"a\" Rb\"b\" \"c\"\n");
    assert_eq!(
        children(node(&tree, "concatenated_string", 0..13)),
        [("string", 0..3), ("string", 4..9), ("string", 10..13)]
    );
    node(&tree, "string_start", 4..7);
    let tree = whole(b"rrrruuuu\"x\"\ny = 1\n");
    node(&tree, "string", 0..11);
    node(&tree, "string_start", 0..9);
    let tree = whole(b"UuUu\"x\"\ny = 1\n");
    node(&tree, "string", 0..7);
    node(&tree, "string_start", 0..5);
    // The grammar takes any run of prefix letters: 20,000 of them are one `string`.
    let source = format!("{}\"x\"\n", "r".repeat(20_000));
    let tree = whole(source.as_bytes());
    clean(&tree);
    assert_eq!(count(&tree, "string"), 1);
    node(&tree, "string", 0..20_003);
    node(&tree, "string_start", 0..20_001);
    let tree = whole(P22);
    assert_eq!(tree.root_node().end_byte(), 40);
    assert_eq!(
        children(node(&tree, "concatenated_string", 14..38)),
        [("string", 14..28), ("string", 29..38)]
    );
    let escape = node(&tree, "escape_sequence", 21..23);
    let content = escape.parent().unwrap();
    assert_eq!(content.kind(), "string_content");
    assert_eq!(content.parent().unwrap().byte_range(), 14..28);
}

#[test]
fn python_documentation_grammar_owner_and_run_shapes() {
    let tree = whole(P4A);
    assert_eq!(tree.root_node().end_byte(), 43);
    let paren = node(&tree, "parenthesized_expression", 13..42);
    assert_eq!(paren.parent().unwrap().kind(), "expression_statement");
    let value = node(&tree, "concatenated_string", 14..41);
    assert_eq!(value.parent().unwrap().id(), paren.id());
    assert_eq!(
        children(value),
        [("string", 14..19), ("comment", 20..30), ("string", 36..41)]
    );
    // Without a final line feed every range that ends the file ends at its length.
    let tree = whole(P11A);
    assert_eq!(tree.root_node().end_byte(), 52);
    let decorated = node(&tree, "decorated_definition", 0..52);
    assert_eq!(decorated.kind_id(), 158);
    let outer = node(&tree, "function_definition", 3..52);
    assert_eq!(outer.kind_id(), 145);
    assert_eq!(outer.parent().unwrap().id(), decorated.id());
    let inner = node(&tree, "function_definition", 28..52);
    let block = node(&tree, "block", 45..52);
    assert_eq!(block.kind_id(), 160);
    assert_eq!(block.parent().unwrap().id(), inner.id());
    node(&tree, "string", 45..52);
    let tree = whole(P1);
    assert_eq!(tree.root_node().end_byte(), 29);
    node(&tree, "function_definition", 0..29);
    node(&tree, "block", 13..29);
    let tree = whole(P2A);
    assert_eq!(tree.root_node().end_byte(), 29);
    let class = node(&tree, "class_definition", 0..29);
    assert_eq!(class.kind_id(), 154);
    assert_eq!(
        node(&tree, "block", 13..29).parent().unwrap().id(),
        class.id()
    );
    assert_eq!(whole(P2B).root_node().byte_range(), 0..13);
    let tree = whole(P11C);
    let function = node(&tree, "function_definition", 0..26);
    assert_eq!(function.child(0).unwrap().kind(), "async");
    let tree = whole(P9A);
    assert_eq!(tree.root_node().end_byte(), 42);
    assert_eq!(tree.root_node().kind_id(), 108);
    assert_eq!(
        children(tree.root_node()),
        [
            ("expression_statement", 0..7),
            ("expression_statement", 8..15),
            ("comment", 16..19),
            ("expression_statement", 20..27),
            ("expression_statement", 28..33),
            ("expression_statement", 34..41),
        ]
    );
    // A `\` row before a statement is no node here, and the unindented `return`
    // after the body is a child of the module.
    let tree = whole(CONTINUATION);
    assert_eq!(count(&tree, "line_continuation"), 0);
    node(&tree, "function_definition", 0..51);
    node(&tree, "block", 19..51);
    let stray = node(&tree, "return_statement", 56..64);
    assert_eq!(stray.parent().unwrap().kind(), "module");
    let tree = whole(P18A);
    assert_eq!(tree.root_node().end_byte(), 15);
    assert_eq!(
        children(tree.root_node()),
        [
            ("expression_statement", 0..3),
            (";", 3..4),
            ("expression_statement", 5..8),
            ("expression_statement", 9..14),
        ]
    );
    let semicolon = node(&tree, ";", 3..4);
    assert_eq!(semicolon.kind_id(), 2);
    assert!(!semicolon.is_named());
    assert!(!semicolon.is_extra());
    assert_eq!(
        children(whole(P18E).root_node()),
        [
            ("expression_statement", 0..5),
            (";", 5..6),
            ("expression_statement", 7..18),
        ]
    );
    assert_eq!(
        children(node(&whole(P18C), "block", 13..34)),
        [
            ("expression_statement", 13..16),
            (";", 16..17),
            ("expression_statement", 18..21),
            ("return_statement", 26..34),
        ]
    );
    // With its carriage return, as the collector tests parse it.
    let tree = whole(P8C);
    assert_eq!(tree.root_node().end_byte(), 34);
    clean(&tree);
    node(&tree, "function_definition", 0..33);
    let paren = node(&tree, "parenthesized_expression", 13..33);
    let value = node(&tree, "concatenated_string", 14..32);
    assert_eq!(value.parent().unwrap().id(), paren.id());
    assert_eq!(children(value), [("string", 14..17), ("string", 23..32)]);
    let tree = whole(P8D);
    assert_eq!(tree.root_node().end_byte(), 53);
    clean(&tree);
    let outer = node(&tree, "function_definition", 0..52);
    let body = outer.child_by_field_name("body").unwrap();
    assert_eq!(
        children(body),
        [
            ("expression_statement", 13..22),
            ("function_definition", 27..52),
        ]
    );
    assert_eq!(
        children(whole(P9C).root_node()),
        [
            ("expression_statement", 0..5),
            ("expression_statement", 6..13),
            ("expression_statement", 14..20),
            ("function_definition", 21..42),
        ]
    );
    // The generated sources of the reader tests: every list is flat.
    for fragments in [3, 1_500] {
        let source = format!("{}\n", vec!["\"a\""; fragments].join(" "));
        let tree = whole(source.as_bytes());
        clean(&tree);
        let value = node(&tree, "concatenated_string", 0..source.len() - 1);
        assert_eq!(value.child_count(), fragments);
        assert!(children(value).iter().all(|(kind, _)| *kind == "string"));
    }
    let tree = whole(b"a.x0, a.x1, a.x2 = v\n\"no\"\n");
    let targets = node(&tree, "pattern_list", 0..16);
    assert_eq!(targets.parent().unwrap().kind(), "assignment");
    assert_eq!(
        children(targets)
            .iter()
            .map(|(kind, _)| *kind)
            .collect::<Vec<_>>(),
        ["attribute", ",", "attribute", ",", "attribute"]
    );
    let tree = whole(b"(\n# c\n# c\n# c\n\"doc\"\n)\n");
    assert_eq!(
        children(node(&tree, "parenthesized_expression", 0..21)),
        [
            ("(", 0..1),
            ("comment", 2..5),
            ("comment", 6..9),
            ("comment", 10..13),
            ("string", 14..19),
            (")", 20..21),
        ]
    );
    let source = format!(
        "class C:\n    def __init__(\n{}        self):\n        self.a = 1\n        \"a doc\"\n",
        "        # c\n".repeat(2)
    );
    let tree = whole(source.as_bytes());
    assert_eq!(tree.root_node().end_byte(), 101);
    assert_eq!(
        children(node(&tree, "parameters", 25..64)),
        [
            ("(", 25..26),
            ("comment", 35..38),
            ("comment", 47..50),
            ("identifier", 59..63),
            (")", 63..64),
        ]
    );
}

#[test]
fn python_documentation_grammar_recovery_and_range_shapes() {
    let tree = whole(P14A);
    assert_eq!(tree.root_node().end_byte(), 33);
    let function = node(&tree, "function_definition", 0..32);
    assert!(function.has_error());
    assert!(!function.is_error());
    assert!(node(&tree, "ERROR", 29..30).is_error());
    let tree = whole(b"def f(:\n    \"audit\"\n    pass\n");
    let missing: Vec<_> = preorder(&tree)
        .into_iter()
        .map(|(_, node)| node)
        .filter(Node::is_missing)
        .collect();
    assert_eq!(missing.len(), 1);
    assert_eq!((missing[0].kind(), missing[0].byte_range()), (")", 6..6));
    let function = preorder(&tree)
        .into_iter()
        .map(|(_, node)| node)
        .find(|node| node.kind() == "function_definition")
        .unwrap();
    assert!(function.has_error());
    assert!(whole(P14B).root_node().is_error());
    let tree = whole(b"def f():\n    \"audit\n    pass\n");
    assert_eq!(count(&tree, "string"), 0);
    assert_eq!(
        node(&tree, "string_start", 13..14).parent().unwrap().kind(),
        "ERROR"
    );
    let tree = whole(P14C);
    assert_eq!(tree.root_node().kind(), "module");
    assert!(tree.root_node().has_error());
    assert!(!node(&tree, "string", 0..5).has_error());
    let tree = whole(P14D);
    assert_eq!(
        children(tree.root_node()),
        [("ERROR", 0..5), ("function_definition", 6..24)]
    );
    assert!(!node(&tree, "function_definition", 6..24).has_error());
    let tree = whole(b"def f():\n    (\"a\"\n     \"b\"\n");
    assert_eq!(count(&tree, "concatenated_string"), 1);
    let value = preorder(&tree)
        .into_iter()
        .map(|(_, node)| node)
        .find(|node| node.kind() == "concatenated_string")
        .unwrap();
    assert_eq!(value.parent().unwrap().kind(), "ERROR");
    assert_eq!(count(&tree, "expression_statement"), 0);
    let tree = whole(P9D);
    assert_eq!(tree.root_node().end_byte(), 43);
    let class = node(&tree, "class_definition", 0..42);
    assert!(class.has_error());
    let body = children(class.child_by_field_name("body").unwrap());
    assert_eq!(body.len(), 3);
    assert_eq!(body[0], ("expression_statement", 13..18));
    assert_eq!(body[1], ("expression_statement", 23..30));
    assert!(!node(&tree, "expression_statement", 23..30).has_error());
    assert_eq!(body[2].0, "expression_statement");
    assert!(node(&tree, "expression_statement", body[2].1.clone()).has_error());
    // An `ERROR` before a string inside the parentheses: the string is a direct
    // child of the parentheses, and not their first named child.
    let tree = whole(P23A);
    assert_eq!(tree.root_node().end_byte(), 24);
    let statement = node(&tree, "expression_statement", 8..17);
    assert_eq!(children(statement), [("parenthesized_expression", 8..17)]);
    assert_eq!(
        children(node(&tree, "parenthesized_expression", 8..17)),
        [
            ("(", 8..9),
            ("ERROR", 9..12),
            ("string", 13..16),
            (")", 16..17),
        ]
    );
    let tree = whole(P23B);
    assert_eq!(tree.root_node().end_byte(), 16);
    assert_eq!(
        children(node(&tree, "expression_statement", 6..15)),
        [("parenthesized_expression", 6..15)]
    );
    assert_eq!(
        children(node(&tree, "parenthesized_expression", 6..15)),
        [
            ("(", 6..7),
            ("ERROR", 7..10),
            ("string", 11..14),
            (")", 14..15),
        ]
    );
    // An `ERROR` inside the value, and one beside it.
    let tree = whole(P24A);
    assert_eq!(tree.root_node().end_byte(), 18);
    let value = node(&tree, "concatenated_string", 1..10);
    assert!(value.has_error());
    assert!(!value.is_error());
    assert_eq!(
        value.parent().unwrap().byte_range(),
        node(&tree, "parenthesized_expression", 0..11).byte_range()
    );
    assert_eq!(
        children(value),
        [("string", 1..4), ("ERROR", 5..6), ("string", 7..10)]
    );
    let tree = whole(P24B);
    assert_eq!(tree.root_node().end_byte(), 18);
    assert!(!node(&tree, "concatenated_string", 1..8).has_error());
    assert_eq!(
        children(node(&tree, "parenthesized_expression", 0..11)),
        [
            ("(", 0..1),
            ("concatenated_string", 1..8),
            ("ERROR", 9..10),
            (")", 10..11),
        ]
    );
    // The empty source.
    let tree = whole(b"");
    clean(&tree);
    assert_eq!(tree.root_node().kind(), "module");
    assert_eq!(tree.root_node().byte_range(), 0..0);
    assert_eq!(tree.root_node().child_count(), 0);
    // Parses with excluded ranges.
    let tree = parse(P17A, &[0..3, 4..14]);
    assert_eq!(tree.root_node().end_byte(), 14);
    clean(&tree);
    assert_eq!(tree.root_node().kind(), "module");
    string(&tree, 0..8, &[0..1, 1..7, 7..8]);
    let tree = parse(P17B, &[0..2, 3..21]);
    assert_eq!(tree.root_node().end_byte(), 21);
    clean(&tree);
    assert_eq!(
        children(node(&tree, "concatenated_string", 0..15)),
        [("string", 0..5), ("string", 6..15)]
    );
    let tree = parse(P20A, &[0..4, 10..20]);
    assert_eq!(tree.root_node().end_byte(), 20);
    clean(&tree);
    assert_eq!(
        children(tree.root_node()),
        [
            ("expression_statement", 0..3),
            ("expression_statement", 10..13),
            ("expression_statement", 14..19),
        ]
    );
    node(&tree, "string", 0..3);
    node(&tree, "string", 10..13);
    let tree = parse(P20B, &[6..33]);
    assert_eq!(tree.root_node().end_byte(), 33);
    clean(&tree);
    assert_eq!(
        children(tree.root_node()),
        [
            ("expression_statement", 6..11),
            ("function_definition", 12..32),
        ]
    );
    assert_eq!(
        children(node(&tree, "block", 25..32)),
        [("expression_statement", 25..32)]
    );
    let tree = parse(P20C, &[0..9, 19..42]);
    assert_eq!(tree.root_node().end_byte(), 42);
    clean(&tree);
    let function = node(&tree, "function_definition", 0..41);
    let block = node(&tree, "block", 23..41);
    assert_eq!(block.parent().unwrap().id(), function.id());
    assert_eq!(
        children(block),
        [
            ("expression_statement", 23..28),
            ("return_statement", 33..41),
        ]
    );
    let tree = parse(P21, &[0..4, 5..16]);
    assert_eq!(tree.root_node().end_byte(), 16);
    clean(&tree);
    assert_eq!(
        children(tree.root_node()),
        [
            ("expression_statement", 0..9),
            ("expression_statement", 10..15),
        ]
    );
    let value = node(&tree, "concatenated_string", 0..9);
    assert_eq!(
        value.parent().unwrap().byte_range(),
        node(&tree, "expression_statement", 0..9).byte_range()
    );
    assert_eq!(children(value), [("string", 0..3), ("string", 6..9)]);
}
