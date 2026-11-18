use super::*;

#[cfg(feature = "rust")]
fn tree_with_ranges(source: &[u8], ranges: &[std::ops::Range<usize>]) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    let ranges: Vec<_> = ranges
        .iter()
        .map(|range| tree_sitter::Range {
            start_byte: range.start,
            end_byte: range.end,
            start_point: tree_sitter::Point::new(0, range.start),
            end_point: tree_sitter::Point::new(0, range.end),
        })
        .collect();
    parser.set_included_ranges(&ranges).unwrap();
    parser.parse(source, None).unwrap()
}

#[cfg(feature = "rust")]
#[test]
fn empty_included_ranges_do_not_add_per_query_scan_work() {
    let source = b"///aXb";
    let mut ranges = Vec::with_capacity(258);
    ranges.push(0..4);
    ranges.extend(std::iter::repeat_n(4..4, 256));
    ranges.push(5..6);
    let tree = tree_with_ranges(source, &ranges);
    assert_eq!(tree.included_ranges().len(), 258);
    let context = SourceContext::new(&tree, source);
    let before = included_range_visits::observed();
    let pieces = context.pieces(0..6).unwrap();
    // expected: the two nonempty intersections, with no following stop candidate.
    assert_eq!(included_range_visits::observed() - before, 2);
    assert_eq!(pieces.pieces().len(), 2);
    assert_eq!(pieces.pieces()[0].range(), 0..4);
    assert_eq!(pieces.pieces()[0].bytes(), b"///a");
    assert_eq!(pieces.pieces()[1].range(), 5..6);
    assert_eq!(pieces.pieces()[1].bytes(), b"b");
    assert!(pieces.has_excluded_gap());
}

#[cfg(feature = "rust")]
#[test]
fn late_intersection_seeks_past_earlier_nonempty_ranges() {
    let source = b"a ".repeat(256);
    let ranges: Vec<_> = (0..256).map(|i| i * 2..i * 2 + 1).collect();
    let tree = tree_with_ranges(&source, &ranges);
    let context = SourceContext::new(&tree, &source);
    let before = included_range_visits::observed();
    let pieces = context.pieces(510..511).unwrap();
    // expected: only the last included range intersects the requested byte.
    assert_eq!(included_range_visits::observed() - before, 1);
    assert_eq!(pieces.pieces().len(), 1);
    assert_eq!(pieces.pieces()[0].range(), 510..511);
    assert_eq!(pieces.pieces()[0].bytes(), b"a");
    assert!(!pieces.has_excluded_gap());
}

#[cfg(feature = "rust")]
#[test]
fn empty_only_included_ranges_do_not_become_an_unrestricted_parse() {
    let source = b"fn f() {}";
    let tree = tree_with_ranges(source, &[0..0, 4..4]);
    assert_eq!(tree.included_ranges().len(), 2);
    let context = SourceContext::new(&tree, source);
    let before = included_range_visits::observed();
    assert_eq!(context.pieces(0..9), Err(SourceError::Excluded));
    assert_eq!(included_range_visits::observed() - before, 0);
}
