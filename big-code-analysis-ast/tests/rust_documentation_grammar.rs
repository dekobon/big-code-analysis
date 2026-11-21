//! Pinned Rust grammar evidence for direct documentation literals and recovery.
#![cfg(feature = "rust")]

use std::ops::Range;

#[allow(clippy::naive_bytecount)]
fn parse(source: &[u8], excluded: Option<Range<usize>>) -> tree_sitter::Tree {
    // bca: suppress(halstead) -- the grammar probe records source ranges, fields and recovery flags together.
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    if let Some(gap) = excluded {
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
        let ranges: Vec<_> = [0..gap.start, gap.end..source.len()]
            .into_iter()
            .filter(|range| !range.is_empty())
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
    if std::env::var_os("BCA_DUMP_RUST_DOC_GRAMMAR").is_some() {
        dump(source, &tree);
    }
    tree
}

fn dump(source: &[u8], tree: &tree_sitter::Tree) {
    println!("SOURCE {source:?} INCLUDED {:?}", tree.included_ranges());
    let mut cursor = tree.walk();
    let mut depth = 0;
    loop {
        let node = cursor.node();
        println!(
            "{depth}: {} id={} field={:?} bytes={:?} error={} missing={}",
            node.kind(),
            node.kind_id(),
            cursor.field_name(),
            node.byte_range(),
            node.has_error(),
            node.is_missing(),
        );
        if cursor.goto_first_child() {
            depth += 1;
        } else {
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return;
                }
                depth -= 1;
            }
        }
    }
}

#[test]
fn rust_documentation_grammar_direct_value_and_recovery() {
    for (source, value, arguments) in [
        (
            "#[doc = \"audit\"] fn f() {}",
            Some("string_literal"),
            false,
        ),
        (
            "#[/* extra */ doc = r###\"a\"#b\"###] fn f() {}",
            Some("raw_string_literal"),
            false,
        ),
        ("#[doc(hidden)] fn f() {}", None, true),
        ("#[doc(alias = \"audit\")] fn f() {}", None, true),
        (
            "#[doc = concat!(\"a\", \"b\")] fn f() {}",
            Some("macro_invocation"),
            false,
        ),
    ] {
        let tree = parse(source.as_bytes(), None);
        let item = tree.root_node().named_child(0).unwrap();
        assert_eq!(item.kind_id(), 170);
        let mut cursor = item.walk();
        let attr = item
            .named_children(&mut cursor)
            .find(|node| node.kind() == "attribute")
            .unwrap();
        assert_eq!(attr.kind_id(), 172);
        assert_eq!(
            attr.child_by_field_name("value").map(|node| node.kind()),
            value
        );
        assert_eq!(attr.child_by_field_name("arguments").is_some(), arguments);
    }
    for source in [
        "#[doc = r\"raw\"] fn f() {}",
        "#[doc = b\"\\xff\"] fn f() {}",
        "#[doc = c\"text\"] fn f() {}",
        "#[doc = br#\"raw\"#] fn f() {}",
        "#[doc = cr#\"raw\"#] fn f() {}",
        "#[doc = \"\\u{1_f600}\"] fn f() {}",
        "#[doc = \"\\u{D800}\"] fn f() {}",
        "#[doc = \"\\u{110000}\"] fn f() {}",
        "#[doc = \"\\u0041\"] fn f() {}",
        "#[doc = \"\\x80\"] fn f() {}",
        "#[doc = \"\\x1\"] fn f() {}",
        "#[doc = \"\\xGG\"] fn f() {}",
        "#[doc = \"\\xé1\"] fn f() {}",
        "#[doc = \"\\u{}\"] fn f() {}",
        "#[doc = \"\\u{1234567}\"] fn f() {}",
        "#[doc = \"\\q\"] fn f() {}",
        "#[doc = \"a\\\n \t\r\nb\"] fn f() {}",
        "#[doc = \"a\\\n \rb\"] fn f() {}",
        "#[doc = \"a\r\nb\"] fn f() {}",
        "#[doc = \"a\rb\"] fn f() {}",
        "#[doc = ] fn f() {}\n/// good\nfn g() {}",
        "#[doc \"a\"] fn f() {}\n/// good\nfn g() {}",
        "#[doc = \"a\" fn f() {}\n/// good\nfn g() {}",
        "#[doc = \"a] fn f() {}\n/// good\nfn g() {}",
        "#[doc] fn f() {}",
    ] {
        parse(source.as_bytes(), None);
    }
    parse(b"#[doc = b\"\xff\"] fn f() {}", None);
    parse(b"#[doc = \"\xff\"] fn f() {}", None);
    parse(b"/// a\n#[doXc = \"b\"] fn f() {}", Some(10..11));
    parse(b"/// a\n#[doc = \"bXc\"] fn f() {}", Some(16..17));
}

fn child<'t>(parent: tree_sitter::Node<'t>, kind: &str) -> tree_sitter::Node<'t> {
    let mut cursor = parent.walk();
    let matches: Vec<_> = parent
        .children(&mut cursor)
        .filter(|node| node.kind() == kind)
        .collect();
    assert_eq!(matches.len(), 1, "{kind} under {}", parent.kind());
    matches[0]
}

fn shape(node: tree_sitter::Node<'_>, kind: &str, range: Range<usize>, flags: (bool, bool, bool)) {
    assert_eq!(node.kind(), kind);
    assert_eq!(node.byte_range(), range);
    assert_eq!(
        (node.has_error(), node.is_error(), node.is_missing()),
        flags
    );
}

#[test]
fn rust_documentation_grammar_clean_invalid_escape_retains_direct_value() {
    let tree = parse(br#"#[doc = "\q"] fn f() {}"#, None);
    assert!(!tree.root_node().has_error());
    let item = child(tree.root_node(), "attribute_item");
    shape(item, "attribute_item", 0..13, (false, false, false));
    let attribute = child(item, "attribute");
    shape(attribute, "attribute", 2..12, (false, false, false));
    shape(
        child(attribute, "identifier"),
        "identifier",
        2..5,
        (false, false, false),
    );
    let value = attribute.child_by_field_name("value").unwrap();
    shape(value, "string_literal", 8..12, (false, false, false));
    shape(
        child(value, "escape_sequence"),
        "escape_sequence",
        9..11,
        (false, false, false),
    );
    assert!(attribute.child_by_field_name("arguments").is_none());
}

#[test]
fn rust_documentation_grammar_short_hex_and_underscore_unicode_recover_differently() {
    let tree = parse(br#"#[doc = "\x1"] fn f() {}"#, None);
    let item = child(tree.root_node(), "attribute_item");
    shape(item, "attribute_item", 0..14, (true, false, false));
    let attribute = child(item, "attribute");
    shape(attribute, "attribute", 2..5, (false, false, false));
    shape(
        child(attribute, "identifier"),
        "identifier",
        2..5,
        (false, false, false),
    );
    assert!(attribute.child_by_field_name("value").is_none());
    let error = child(item, "ERROR");
    shape(error, "ERROR", 6..13, (true, true, false));
    shape(child(error, "ERROR"), "ERROR", 9..12, (false, true, false));
    shape(child(item, "]"), "]", 13..14, (false, false, false));

    let tree = parse(br#"#[doc = "\u{1_f600}"] fn f() {}"#, None);
    let item = child(tree.root_node(), "attribute_item");
    shape(item, "attribute_item", 0..21, (true, false, false));
    let attribute = child(item, "attribute");
    shape(attribute, "attribute", 2..20, (true, false, false));
    shape(
        child(attribute, "identifier"),
        "identifier",
        2..5,
        (false, false, false),
    );
    shape(
        child(attribute, "ERROR"),
        "ERROR",
        8..19,
        (true, true, false),
    );
    let value = attribute.child_by_field_name("value").unwrap();
    shape(value, "string_literal", 19..20, (true, false, false));
    let mut cursor = value.walk();
    let missing: Vec<_> = value
        .children(&mut cursor)
        .filter(tree_sitter::Node::is_missing)
        .collect();
    assert_eq!(missing.len(), 1);
    shape(missing[0], "\"", 20..20, (true, false, true));
}

#[test]
fn rust_documentation_grammar_missing_bracket_keeps_neighbor_boundaries() {
    let tree = parse(b"#[doc = \"a\" fn f() {}\n/// good\nfn g() {}", None);
    let root = tree.root_node();
    let item = child(root, "attribute_item");
    shape(item, "attribute_item", 0..11, (true, false, false));
    let attribute = child(item, "attribute");
    shape(attribute, "attribute", 2..11, (false, false, false));
    shape(
        attribute.child_by_field_name("value").unwrap(),
        "string_literal",
        8..11,
        (false, false, false),
    );
    shape(child(item, "]"), "]", 11..11, (true, false, true));
    assert_eq!(root.named_child_count(), 4);
    shape(
        root.named_child(1).unwrap(),
        "function_item",
        12..21,
        (false, false, false),
    );
    shape(
        root.named_child(2).unwrap(),
        "line_comment",
        22..31,
        (false, false, false),
    );
    shape(
        root.named_child(3).unwrap(),
        "function_item",
        31..40,
        (false, false, false),
    );
}

#[test]
fn rust_documentation_grammar_path_and_value_gaps_keep_separate_fields() {
    for (source, gap, item_span, path_span, value_span, function_span) in [
        (
            b"/// a\n#[doXc = \"b\"] fn f() {}".as_slice(),
            10..11,
            6..19,
            8..12,
            15..18,
            20..29,
        ),
        (
            b"/// a\n#[doc = \"bXc\"] fn f() {}".as_slice(),
            16..17,
            6..20,
            8..11,
            14..19,
            21..30,
        ),
    ] {
        let tree = parse(source, Some(gap.clone()));
        assert!(!tree.root_node().has_error());
        assert_eq!(
            tree.included_ranges()
                .iter()
                .map(|range| range.start_byte..range.end_byte)
                .collect::<Vec<_>>(),
            vec![0..gap.start, gap.end..source.len()]
        );
        let item = child(tree.root_node(), "attribute_item");
        shape(item, "attribute_item", item_span, (false, false, false));
        let attribute = child(item, "attribute");
        shape(
            child(attribute, "identifier"),
            "identifier",
            path_span,
            (false, false, false),
        );
        shape(
            attribute.child_by_field_name("value").unwrap(),
            "string_literal",
            value_span,
            (false, false, false),
        );
        assert!(attribute.child_by_field_name("arguments").is_none());
        shape(
            child(tree.root_node(), "function_item"),
            "function_item",
            function_span,
            (false, false, false),
        );
    }
}
