//! Pinned Rust grammar evidence for direct documentation literals and recovery.
#![cfg(feature = "rust")]

use std::ops::Range;

#[allow(clippy::naive_bytecount)]
fn dump(source: &[u8], excluded: Option<Range<usize>>) -> tree_sitter::Tree {
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
                    drop(cursor);
                    return tree;
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
        let tree = dump(source.as_bytes(), None);
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
        dump(source.as_bytes(), None);
    }
    dump(b"#[doc = b\"\xff\"] fn f() {}", None);
    dump(b"#[doc = \"\xff\"] fn f() {}", None);
    dump(b"/// a\n#[doXc = \"b\"] fn f() {}", Some(10..11));
    dump(b"/// a\n#[doc = \"bXc\"] fn f() {}", Some(16..17));
}
