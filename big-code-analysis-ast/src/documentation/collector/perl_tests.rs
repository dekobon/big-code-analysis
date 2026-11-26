//! Parse shapes of the pinned Perl grammar that a reader of POD blocks relies on: where a
//! `pod_statement` and the data section start and end, where the grammar gives no block,
//! and how it reads formats, here-documents and quote-like literals. The tests parse
//! verbatim bytes and collect nothing.

#![allow(clippy::single_range_in_vec_init, clippy::naive_bytecount)]

#[cfg(feature = "perl")]
use crate::{AnyParser, LANG};
#[cfg(feature = "perl")]
use std::ops::Range;

#[cfg(feature = "perl")]
const PRIMARY: &[u8] = b"=head2 f\n\nAudit\n\n=cut\n\nsub f {}";
#[cfg(feature = "perl")]
const TWO_BLOCKS: &[u8] = b"my $x = 1;\n=head1 A\n\na\n\n=cut\nmy $y = 2;\n=head1 B\n\nb\n\n=cut\n";
#[cfg(feature = "perl")]
const MIXED_FIXTURE: &[u8] =
    b"=head2 f\n\nAudit\n\n=begin comment\n\nnot prose\n\n=end comment\n\n\
    =cut\n\nsub f { return 1; }\n__END__\n=head1 NAME\n\nData docs\n\n=cut\n";

/// A node as kind, byte range and kind of its parent.
#[cfg(feature = "perl")]
type Leaf = (&'static str, Range<usize>, &'static str);
#[cfg(feature = "perl")]
type Ranges = Vec<Range<usize>>;

/// A node as label, byte range and start row. The label is `ERROR` or `MISSING` for a
/// recovery node and the kind otherwise.
#[cfg(feature = "perl")]
type Shape = (&'static str, Range<usize>, usize);

#[cfg(feature = "perl")]
fn parsed(source: &[u8]) -> AnyParser {
    AnyParser::parse(LANG::Perl, source.to_vec(), None, None).unwrap()
}

/// A raw parse that reads the given byte ranges only.
#[cfg(feature = "perl")]
fn restricted(source: &[u8], ranges: &[Range<usize>]) -> tree_sitter::Tree {
    let point = |offset: usize| {
        let before = &source[..offset];
        tree_sitter::Point::new(
            before.iter().filter(|byte| **byte == b'\n').count(),
            before
                .iter()
                .rev()
                .take_while(|byte| **byte != b'\n')
                .count(),
        )
    };
    let ranges: Vec<_> = ranges
        .iter()
        .map(|range| tree_sitter::Range {
            start_byte: range.start,
            end_byte: range.end,
            start_point: point(range.start),
            end_point: point(range.end),
        })
        .collect();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Perl.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    parser.parse(source, None).unwrap()
}

/// Every node of the given kinds, in source order, with the kind of its parent.
#[cfg(feature = "perl")]
fn nodes_of(root: tree_sitter::Node<'_>, kinds: &[&str]) -> Vec<Leaf> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
        if kinds.contains(&node.kind()) {
            assert_eq!(node.child_count(), 0, "a leaf");
            found.push((
                node.kind(),
                node.byte_range(),
                node.parent().unwrap().kind(),
            ));
        }
    }
    found
}

/// Every node that the rules for literal text read, in tree order: recovery nodes,
/// blocks, the data leaf, here-document nodes, `<<` and `.` tokens, quote-like literals,
/// and identifiers spelled `format` or like a quote-like operator.
#[cfg(feature = "perl")]
fn shapes_of(root: tree_sitter::Node<'_>, source: &[u8]) -> Vec<Shape> {
    const KINDS: [&str; 19] = [
        "pod_statement",
        "data_not_for_compiler",
        "heredoc_initializer",
        "heredoc_body_statement",
        "heredoc_end_identifier",
        "<<",
        ".",
        "string_single_quoted",
        "string_double_quoted",
        "string_qq_quoted",
        "backtick_quoted",
        "command_qx_quoted",
        "pattern_matcher",
        "pattern_matcher_m",
        "regex_pattern_qr",
        "substitution_pattern_s",
        "transliteration_tr_or_y",
        "string_q_quoted",
        "word_list_qw",
    ];
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
        let keyword = node.kind() == "identifier"
            && matches!(
                &source[node.byte_range()],
                b"format" | b"q" | b"qq" | b"qw" | b"qx" | b"m" | b"qr" | b"s" | b"tr" | b"y"
            );
        let label = if node.is_error() {
            "ERROR"
        } else if node.is_missing() {
            "MISSING"
        } else if keyword || KINDS.contains(&node.kind()) {
            node.kind()
        } else {
            continue;
        };
        found.push((label, node.byte_range(), node.start_position().row));
    }
    found
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_pod_parse_shapes_the_reader_relies_on() {
    // expected: a block starts at its `=`. One that a `=cut` line ends runs to the end of
    // that line and holds no line feed after it, but holds the CR of a CRLF. One that no
    // `=cut` line ends runs to the end of the input, final line end included. A NUL byte
    // ends a block where it stands. The text after `__END__` or `__DATA__` is one leaf.
    let block = |range: Range<usize>, parent: &'static str| ("pod_statement", range, parent);
    let file = |range: Range<usize>| ("pod_statement", range, "source_file");
    let cases: [(&[u8], bool, Vec<Leaf>); 39] = [
        (PRIMARY, false, vec![file(0..21)]),
        (
            b"=head1 A\r\n\r\na\r\n\r\n=cut\r\nsub f {}\r\n",
            false,
            vec![file(0..22)],
        ),
        (
            b"my $x = 1;\n\n=head1 NAME\n\nNever closed.\n",
            false,
            vec![file(12..39)],
        ),
        (
            b"my $x = 1;\n\n=head1 NAME\n\nNever closed.",
            false,
            vec![file(12..38)],
        ),
        (
            b"=head1 NAME\n\nNever closed.\n\n\n",
            false,
            vec![file(0..29)],
        ),
        (b"=pod\n\nx\n\n=cut", false, vec![file(0..13)]),
        (b"=pod\r\n\r\nx\r\n", false, vec![file(0..11)]),
        (b"=pod\r\n\r\nx\r\n\r\n=cut\r\n", false, vec![file(0..18)]),
        (b"=pod\n", false, vec![file(0..5)]),
        (b"=pod\n=cut\n", false, vec![file(0..9)]),
        (
            b"=head1 A\n\na\n\n=cut here is text\nmy $y = 2;\n",
            false,
            vec![file(0..30)],
        ),
        (b"=pod\n\ny\n\n=CUT\nmy $x;\n", false, vec![file(0..21)]),
        (TWO_BLOCKS, false, vec![file(11..28), file(40..57)]),
        (
            b"=head1 A\n\na\n\n=cut\n=head1 B\n\nb\n\n=cut\n",
            false,
            vec![file(0..17), file(18..35)],
        ),
        (
            b"sub f {\n    my $x = 1;\n\n=pod\n\nDoc.\n\n=cut\n\n    return $x;\n}\n",
            false,
            vec![block(24..40, "block")],
        ),
        (
            b"sub o {\n  sub i {\n=head2 i\n\nInner.\n\n=cut\n  }\n}\n",
            false,
            vec![block(18..40, "block")],
        ),
        (
            MIXED_FIXTURE,
            false,
            vec![
                file(0..62),
                ("data_not_for_compiler", 91..121, "special_literal"),
            ],
        ),
        (
            b"__END__\n=head1 NAME\n\nAudit\n\n=cut\n",
            false,
            vec![("data_not_for_compiler", 7..33, "special_literal")],
        ),
        (
            b"__DATA__\nrow\n",
            false,
            vec![("data_not_for_compiler", 8..13, "special_literal")],
        ),
        (
            b"__END__",
            false,
            vec![("data_not_for_compiler", 7..7, "special_literal")],
        ),
        (
            b"sub f {\n__END__\n=pod\n\nx\n\n=cut\n",
            true,
            vec![("data_not_for_compiler", 15..30, "special_literal")],
        ),
        // The grammar opens a block wherever a statement may start, at any `=` word.
        (b"my $x = 1;\n=cut\nmy $y = 2;\n", false, vec![file(11..27)]),
        (b"my $x = 1;\n=1\ntext\n=cut\n", false, vec![file(11..23)]),
        (
            b"my $x = 1;\n  =head1 NAME\n\ntext\n\n=cut\n",
            false,
            vec![file(13..36)],
        ),
        (
            b"my $x = 1; =head1 NAME\n\ntext\n\n=cut\n",
            false,
            vec![file(11..34)],
        ),
        (b"\xef\xbb\xbf=pod\n\nx\n\n=cut\n", false, vec![file(3..16)]),
        // `=cutting` ends a block, and the `=cut` below it opens one.
        (
            b"=head1 A\n\ntext\n\n=cutting\nmy $y = 2;\n=cut\n",
            false,
            vec![file(0..24), file(36..41)],
        ),
        (b"=pod\n\na\x00b\n\n=cut\n", true, vec![file(0..7)]),
        (
            b"=pod\n\nx\n\n=cut\x00foo\nmy $y;\n",
            true,
            vec![file(0..13)],
        ),
        (b"=pod\r\rx\r\r=cut\rmy $x;\r", false, vec![file(0..21)]),
        (b"=pod\n\nx\n\n=cut\r", false, vec![file(0..14)]),
        // Recovery beside a block leaves the node of the block as it is.
        (b"%%%% (\n=pod\n\nx\n\n=cut\n", true, vec![file(7..20)]),
        (
            b"my $x = 1,\n=pod\n\nx\n\n=cut\n2;\n",
            true,
            vec![file(11..24)],
        ),
        (
            b"sub f {\n=pod\n\nx\n\n=cut\n",
            true,
            vec![block(8..21, "block")],
        ),
        (
            b"sub f {\nreturn\n=pod\n\nx\n\n=cut\n1;\n}\n",
            true,
            vec![block(15..28, "block")],
        ),
        // No block where the text is part of another node.
        (
            b"my $t = <<'EOT';\n=head1 NAME\n\nnot pod\n\n=cut\nEOT\n",
            false,
            vec![],
        ),
        (b"my $s = \"abc\n=pod\n\nx\n\n=cut\n", true, vec![]),
        (b"my @a = (\n1,\n=pod\n\nx\n\n=cut\n2);\n", true, vec![]),
        (b"=pod", true, vec![]),
    ];
    for (source, recovered, expected) in cases {
        let parser = parsed(source);
        let root = parser.ts_tree().root_node();
        assert_eq!(root.kind(), "source_file", "{}", source.escape_ascii());
        assert_eq!(root.has_error(), recovered, "{}", source.escape_ascii());
        assert_eq!(
            nodes_of(root, &["pod_statement", "data_not_for_compiler"]),
            expected,
            "{}",
            source.escape_ascii()
        );
    }
    // The scanner reads the included bytes as one text: an excluded line feed after
    // `=cut` joins the next row to the block, and an excluded `=cut` joins two blocks.
    let cut: [(&[Range<usize>], Ranges); 8] = [
        (&[0..20, 21..58], vec![11..28, 40..57]),
        (&[0..10, 11..58], vec![11..28, 40..57]),
        (&[0..28, 29..58], vec![11..39, 40..57]),
        (&[0..24, 28..58], vec![11..57]),
        (&[0..5, 6..58], vec![11..28, 40..57]),
        (&[0..57], vec![11..28, 40..57]),
        (&[0..15, 15..58], vec![11..28, 40..57]),
        (&[0..11, 12..58], vec![40..57]),
    ];
    for (ranges, expected) in cut {
        let tree = restricted(TWO_BLOCKS, ranges);
        let blocks: Vec<_> = nodes_of(tree.root_node(), &["pod_statement"])
            .into_iter()
            .map(|(_, range, _)| range)
            .collect();
        assert_eq!(blocks, expected, "{ranges:?}");
    }
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_literal_parse_shapes_the_doubt_rules_rely_on() {
    // expected: per source, whether the tree holds an error, and in tree order the nodes
    // that the rules for literal text read. A format is a call for the grammar: only the
    // identifier `format` and, where the grammar keeps it, the `.` of its last line say
    // that it is there. A here-document that the grammar reads has an initializer, and
    // its body starts on the row after the initializer's row. One that it does not read
    // is a `<<` operator, a recovery node, or part of a literal. A literal node holds no
    // other literal node. A block is a `pod_statement` wherever a statement may start.
    // Where a literal starts and ends is the grammar's claim, and the last rows show
    // claims that perl does not share: `q`, `tr` and `y` behind an escaped opening
    // bracket end late; `CORE::q` is a name and the lines behind it are code; a `/`
    // behind `time` starts a pattern; a `'` behind a name starts a string; an operator
    // word before `=>` or in a subscript starts a literal. `<<`, a blank and a quoted
    // tag is a shift. Behind a literal that the grammar does not parse, a block has no
    // node.
    let cases: [(&[u8], bool, Vec<Shape>); 69] = [
        (
            b"my $x = 1,\n=pod\n\nx\n\n=cut\n2;\n",
            true,
            vec![("MISSING", 10..10, 0), ("pod_statement", 11..24, 1)],
        ),
        (
            b"my $s = <<\"\";\n=pod\n\nprint \"MARK\";\n",
            true,
            vec![
                ("ERROR", 8..10, 0),
                ("<<", 8..10, 0),
                ("string_double_quoted", 10..12, 0),
                ("pod_statement", 14..34, 1),
            ],
        ),
        (
            b"1;\n\x04\n=pod\n\nd\n\n=cut\n",
            true,
            vec![
                ("ERROR", 3..4, 1),
                ("ERROR", 3..4, 1),
                ("pod_statement", 5..18, 2),
            ],
        ),
        (
            b"format STDOUT =\n=pod\n.\n",
            true,
            vec![
                ("ERROR", 0..15, 0),
                ("identifier", 0..6, 0),
                ("pod_statement", 16..23, 1),
            ],
        ),
        (
            b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                ("pod_statement", 19..32, 2),
                (".", 35..36, 8),
            ],
        ),
        (
            b"format =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                ("pod_statement", 12..25, 2),
                (".", 28..29, 8),
            ],
        ),
        (
            b"format STDOUT =\n1\n.\n2;\n\n=pod\n\nreal\n\n=cut\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 18..19, 2),
                ("pod_statement", 24..40, 5),
            ],
        ),
        (
            b"format STDOUT =\n1\n. \t\r\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 18..19, 2),
                ("pod_statement", 27..40, 5),
                ("string_double_quoted", 47..53, 10),
            ],
        ),
        (
            b"format STDOUT =\r\n1\r\n.\r\n2;\r\n\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nprint \"\
          MARK\";\r\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 20..21, 2),
                ("pod_statement", 29..47, 5),
                ("string_double_quoted", 54..60, 10),
            ],
        ),
        (
            b"format A =\nformat B =\n1\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                ("identifier", 11..17, 1),
                (".", 24..25, 3),
                ("pod_statement", 30..43, 6),
                ("string_double_quoted", 50..56, 11),
            ],
        ),
        (
            b"my $o = bless {};\nsub format { 7 }\nmy $v = $o->format;\n\n=pod\n\nd\n\n=cu\
          t\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 22..28, 1),
                ("identifier", 47..53, 2),
                ("pod_statement", 56..69, 4),
                ("string_double_quoted", 76..82, 9),
            ],
        ),
        (
            b"FORMAT STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            false,
            vec![("pod_statement", 19..32, 2), (".", 35..36, 8)],
        ),
        (
            b"format F =\na 'b\n.\nmy $s = ';\n=pod\n\nd\n\n=cut\n';\nprint \"MARK\";\n",
            true,
            vec![
                ("identifier", 0..6, 0),
                ("string_single_quoted", 13..27, 1),
                ("pod_statement", 29..42, 4),
                ("ERROR", 43..60, 9),
            ],
        ),
        (
            b"format F =\n1; print <<EOT;\n.\nmy $s = '\nEOT\n=pod\n\nd\n\n=cut\n';\nprint \
          \"MARK\";\n",
            true,
            vec![
                ("identifier", 0..6, 0),
                ("heredoc_initializer", 20..25, 1),
                ("<<", 20..22, 1),
                ("heredoc_body_statement", 26..42, 1),
                ("heredoc_end_identifier", 39..42, 4),
                ("pod_statement", 43..56, 5),
                ("ERROR", 57..74, 10),
            ],
        ),
        (
            b"my $s = \"@{[ <<EOT ]}\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_double_quoted", 8..22, 0),
                ("pod_statement", 24..37, 1),
                ("string_double_quoted", 48..54, 7),
            ],
        ),
        (
            b"my $s = qq{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_qq_quoted", 8..24, 0),
                ("pod_statement", 26..39, 1),
                ("string_double_quoted", 50..56, 7),
            ],
        ),
        (
            b"my $r = 'a' =~ /(?{ <<EOT })/;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("pattern_matcher", 15..29, 0),
                ("pod_statement", 31..44, 1),
                ("string_double_quoted", 55..61, 7),
            ],
        ),
        (
            b"my $t = 'a';\n$t =~ s/a/<<EOT/e;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("substitution_pattern_s", 19..30, 1),
                ("pod_statement", 32..45, 2),
                ("string_double_quoted", 56..62, 8),
            ],
        ),
        (
            b"my $r = 'a' =~ m{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("pattern_matcher_m", 15..30, 0),
                ("pod_statement", 32..45, 1),
                ("string_double_quoted", 56..62, 7),
            ],
        ),
        (
            b"my $r = qr{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("regex_pattern_qr", 8..24, 0),
                ("pod_statement", 26..39, 1),
                ("string_double_quoted", 50..56, 7),
            ],
        ),
        (
            b"my $s = `@{[ <<EOT ]}`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("backtick_quoted", 8..22, 0),
                ("pod_statement", 24..37, 1),
                ("string_double_quoted", 48..54, 7),
            ],
        ),
        (
            b"my $s = qx{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("command_qx_quoted", 8..24, 0),
                ("pod_statement", 26..39, 1),
                ("string_double_quoted", 50..56, 7),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<\"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"M\
          ARK\";\n",
            false,
            vec![
                ("<<", 29..31, 1),
                ("string_double_quoted", 31..36, 1),
                ("pod_statement", 38..51, 2),
                ("string_double_quoted", 62..68, 8),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<1;\n=pod\n\nd\n\n=cut\n1\nprint \"MARK\";\n",
            true,
            vec![
                ("<<", 29..31, 1),
                ("pod_statement", 34..47, 2),
                ("ERROR", 48..49, 7),
                ("string_double_quoted", 56..62, 8),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<~EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\
          \";\n",
            false,
            vec![
                ("<<", 29..31, 1),
                ("pod_statement", 37..50, 2),
                ("string_double_quoted", 61..67, 8),
            ],
        ),
        (
            b"print {*STDOUT} <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            true,
            vec![
                ("<<", 16..18, 0),
                ("MISSING", 21..21, 0),
                ("pod_statement", 23..36, 1),
                ("string_double_quoted", 47..53, 7),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\
          \";\n",
            true,
            vec![
                ("<<", 29..31, 1),
                ("MISSING", 34..34, 1),
                ("pod_statement", 36..49, 2),
                ("string_double_quoted", 60..66, 8),
            ],
        ),
        (
            b"my $s = 1<<2;\nmy $t = $s << 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("<<", 9..11, 0),
                ("<<", 25..27, 1),
                ("pod_statement", 32..45, 3),
                ("string_double_quoted", 52..58, 8),
            ],
        ),
        (
            b"my $s = <<EOT; \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("pod_statement", 16..29, 1),
                ("heredoc_body_statement", 29..33, 5),
                ("heredoc_end_identifier", 30..33, 6),
                ("string_double_quoted", 40..46, 7),
            ],
        ),
        (
            b"my $s = <<EOT;\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nEOT\r\nprint \"MARK\";\r\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("pod_statement", 16..34, 1),
                ("heredoc_body_statement", 34..38, 5),
                ("heredoc_end_identifier", 35..38, 6),
                ("string_double_quoted", 46..52, 7),
            ],
        ),
        (
            b"my $s = <<EOT; \nlit\nEOT\n;\nmy $t = '\nEOT\n=pod\n\nd\n\n=cut\n';\nprint \
          \"MARK\";\n",
            true,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 25..39, 3),
                ("heredoc_end_identifier", 36..39, 5),
                ("pod_statement", 40..53, 6),
                ("ERROR", 54..71, 11),
            ],
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 14..22, 0),
                ("heredoc_end_identifier", 19..22, 2),
                ("pod_statement", 24..37, 3),
                ("string_double_quoted", 48..54, 9),
            ],
        ),
        (
            b"my $s = <<~EOT;\nlit\nEOT\t\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..14, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 15..23, 0),
                ("heredoc_end_identifier", 20..23, 2),
                ("pod_statement", 25..38, 3),
                ("string_double_quoted", 49..55, 9),
            ],
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 14..22, 0),
                ("heredoc_end_identifier", 19..22, 2),
                ("pod_statement", 24..37, 4),
                ("string_double_quoted", 44..50, 9),
            ],
        ),
        (
            b"my @s = (<<EOT, <<EOU);\na\nEOT\nb\nEOU\n\n=pod\n\nd\n\n=cut\nprint \"MARK\"\
          ;\n",
            false,
            vec![
                ("heredoc_initializer", 9..14, 0),
                ("<<", 9..11, 0),
                ("heredoc_initializer", 16..21, 0),
                ("<<", 16..18, 0),
                ("heredoc_body_statement", 23..29, 0),
                ("heredoc_end_identifier", 26..29, 2),
                ("heredoc_body_statement", 29..35, 2),
                ("heredoc_end_identifier", 32..35, 4),
                ("pod_statement", 37..50, 6),
                ("string_double_quoted", 57..63, 11),
            ],
        ),
        (
            b"my $s = <<EOT; # c\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 18..26, 0),
                ("heredoc_end_identifier", 23..26, 2),
                ("pod_statement", 28..41, 4),
                ("string_double_quoted", 48..54, 9),
            ],
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\r\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 14..22, 0),
                ("heredoc_end_identifier", 19..22, 2),
                ("pod_statement", 25..38, 4),
                ("string_double_quoted", 45..51, 9),
            ],
        ),
        (
            b"my $s = q!a\\!;\n=pod\n\nd\n\n=cut\n!;\nprint \"MARK\";\n",
            true,
            vec![
                ("string_q_quoted", 8..13, 0),
                ("pod_statement", 15..28, 1),
                ("ERROR", 30..31, 6),
                ("string_double_quoted", 38..44, 7),
            ],
        ),
        (
            b"my $s = q{a\\};\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            true,
            vec![
                ("string_q_quoted", 8..13, 0),
                ("pod_statement", 15..28, 1),
                ("ERROR", 29..31, 6),
                ("string_double_quoted", 38..44, 7),
            ],
        ),
        (
            b"my @s = qw!a\\!;\n=pod\n\nd\n\n=cut\n!;\nprint \"MARK\";\n",
            true,
            vec![
                ("word_list_qw", 8..14, 0),
                ("pod_statement", 16..29, 1),
                ("ERROR", 31..32, 6),
                ("string_double_quoted", 39..45, 7),
            ],
        ),
        (
            b"my @s = qw{a{b};\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            true,
            vec![
                ("word_list_qw", 8..15, 0),
                ("pod_statement", 17..30, 1),
                ("ERROR", 31..33, 6),
                ("string_double_quoted", 40..46, 7),
            ],
        ),
        (
            b"my $s = q{a\\\\};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("string_q_quoted", 8..14, 0),
                ("pod_statement", 17..30, 2),
                ("string_double_quoted", 37..43, 7),
            ],
        ),
        (
            b"my $s = q{a{b}c};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("string_q_quoted", 8..16, 0),
                ("pod_statement", 19..32, 2),
                ("string_double_quoted", 39..45, 7),
            ],
        ),
        (
            b"my @s = qw(a b);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("word_list_qw", 8..15, 0),
                ("pod_statement", 18..31, 2),
                ("string_double_quoted", 38..44, 7),
            ],
        ),
        (
            b"my %h; my $s = qq{ $h{qq{ $h{qq{ x }} }} };\n\n=pod\n\nd\n\n=cut\nprint \"MA\
          RK\";\n",
            false,
            vec![
                ("string_qq_quoted", 15..42, 0),
                ("pod_statement", 45..58, 2),
                ("string_double_quoted", 65..71, 7),
            ],
        ),
        (
            b"my $s = <<\"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..15, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 16..34, 0),
                ("heredoc_end_identifier", 31..34, 6),
                ("string_double_quoted", 41..47, 7),
            ],
        ),
        (
            b"my $s = <<EOA;\n@{[ <<EOT ]}\n=pod\n\nd\n\n=cut\nEOT\nEOA\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 14..49, 0),
                ("heredoc_end_identifier", 46..49, 8),
                ("string_double_quoted", 56..62, 9),
            ],
        ),
        (
            b"my $s = q{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            false,
            vec![
                ("string_q_quoted", 8..26, 0),
                ("string_double_quoted", 34..40, 7),
            ],
        ),
        (
            b"format STDOUT =\nline\n.\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            true,
            vec![
                ("identifier", 0..6, 0),
                (".", 21..22, 2),
                ("ERROR", 23..27, 3),
                ("string_double_quoted", 43..49, 8),
            ],
        ),
        (
            b"format Pkg'F =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            true,
            vec![("ERROR", 0..39, 0), ("identifier", 0..6, 0)],
        ),
        (
            b"my $s = q'a\\'b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            true,
            vec![
                ("ERROR", 0..46, 0),
                ("ERROR", 8..13, 0),
                ("string_q_quoted", 8..13, 0),
            ],
        ),
        (
            b"my $x = (1;\nmy $s = \"\n=pod\n\";\n__END__\n=pod\n\nd\n\n=cut\n",
            true,
            vec![
                ("MISSING", 10..10, 0),
                ("string_double_quoted", 20..28, 1),
                ("data_not_for_compiler", 37..52, 4),
            ],
        ),
        (
            b"format F =\n@x;\nqq{@{[ <<TAG ]}};\n1\n.\n2;\n=pod\n\nd\n\n=cut\nTAG\n.\n2;\
          \n",
            false,
            vec![
                ("identifier", 0..6, 0),
                ("string_qq_quoted", 15..31, 2),
                (".", 35..36, 4),
                ("pod_statement", 40..53, 6),
                (".", 58..59, 12),
            ],
        ),
        (
            b"format F =\n@x;\n$a, $b\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 22..23, 3),
                ("pod_statement", 28..41, 6),
                ("string_double_quoted", 48..54, 11),
            ],
        ),
        (
            b"format F =\n@x\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 14..15, 2),
                ("pod_statement", 20..33, 5),
                ("string_double_quoted", 40..46, 10),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint {$fh} <<  \"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \
          \"MARK\";\n",
            false,
            vec![
                ("<<", 31..33, 1),
                ("string_double_quoted", 35..40, 1),
                ("pod_statement", 42..55, 2),
                ("string_double_quoted", 66..72, 8),
            ],
        ),
        (
            b"print STDOUT << \"\";\n=pod\n\nprint \"MARK\";\n",
            false,
            vec![
                ("<<", 13..15, 0),
                ("string_double_quoted", 16..18, 0),
                ("pod_statement", 20..40, 1),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh << \"EOT\";\n\n=pod\n\nd\n\n=cut\nprint \"MAR\
          K\";\n",
            false,
            vec![
                ("<<", 29..31, 1),
                ("string_double_quoted", 32..37, 1),
                ("pod_statement", 40..53, 3),
                ("string_double_quoted", 60..66, 8),
            ],
        ),
        (
            b"my $t = 2; my $x = $t <<  2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("<<", 22..24, 0),
                ("pod_statement", 30..43, 2),
                ("string_double_quoted", 50..56, 7),
            ],
        ),
        (
            b"my $x = q(a\\(b); my $s = \"x);\n=pod\n\nd\n\n=cut\n\";\nprint \"MARK\";\n",
            true,
            vec![
                ("string_q_quoted", 8..28, 0),
                ("pod_statement", 30..43, 1),
                ("string_double_quoted", 44..59, 6),
                ("ERROR", 53..58, 7),
            ],
        ),
        (
            b"my $t = 'a';\n$t =~ tr{a\\{}{b}; my $s = \"x}{y};\n=pod\n\nd\n\n=cut\n\";\np\
          rint \"MARK\";\n",
            true,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("transliteration_tr_or_y", 19..45, 1),
                ("pod_statement", 47..60, 2),
                ("string_double_quoted", 61..76, 7),
                ("ERROR", 70..75, 8),
            ],
        ),
        (
            b"my $t = 'a';\n$t =~ y(a\\()(b); my $s = \"x)(y);\n=pod\n\nd\n\n=cut\n\";\npr\
          int \"MARK\";\n",
            true,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("transliteration_tr_or_y", 19..44, 1),
                ("pod_statement", 46..59, 2),
                ("string_double_quoted", 60..75, 7),
                ("ERROR", 69..74, 8),
            ],
        ),
        (
            b"my $t = 'a';\n$t =~ s{a}\n  {b}g; $t =~ tr{a} {b};\n\n=pod\n\nd\n\n=cut\npri\
          nt \"MARK\";\n",
            false,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("substitution_pattern_s", 19..30, 1),
                ("transliteration_tr_or_y", 38..47, 2),
                ("pod_statement", 50..63, 4),
                ("string_double_quoted", 70..76, 9),
            ],
        ),
        (
            b"my $x = CORE::q{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 14..15, 0),
                ("pod_statement", 17..30, 1),
                ("string_double_quoted", 40..46, 7),
            ],
        ),
        (
            b"my $z = time / 2; my $s = \"/; 1; \"; my $u = \";\n=pod\n\nd\n\n=cut\n\";\n# \
          \"\nprint \"MARK\";\n",
            true,
            vec![
                ("pattern_matcher", 13..28, 0),
                ("string_double_quoted", 33..45, 0),
                ("pod_statement", 47..60, 1),
                ("ERROR", 61..67, 6),
                ("string_double_quoted", 61..67, 6),
                ("string_double_quoted", 74..80, 8),
            ],
        ),
        (
            b"my $x = Foo'bar(1); my $s = '; 1; '; my $u = ';\n=pod\n\nd\n\n=cut\n';\n# '\
          \nprint \"MARK\";\n",
            true,
            vec![
                ("string_single_quoted", 11..29, 0),
                ("string_single_quoted", 34..46, 0),
                ("pod_statement", 48..61, 1),
                ("ERROR", 62..68, 6),
                ("string_single_quoted", 62..68, 6),
                ("string_double_quoted", 75..81, 8),
            ],
        ),
        (
            b"my %k = (y => 1, s => 2, q => 3);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("transliteration_tr_or_y", 9..28, 0),
                ("pod_statement", 35..48, 2),
                ("string_double_quoted", 55..61, 7),
            ],
        ),
        (
            b"my @m = split /,/, 'a,b'; my $n = 1 if $m[0] =~ /a/; my @o = grep /b/, @m;\n\
          \n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("pattern_matcher", 14..17, 0),
                ("string_single_quoted", 19..24, 0),
                ("pattern_matcher", 48..51, 0),
                ("pattern_matcher", 66..69, 0),
                ("pod_statement", 76..89, 2),
                ("string_double_quoted", 96..102, 7),
            ],
        ),
        (
            b"my %k; my $v = $k{q}; my $w = { 1 };\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            true,
            vec![
                ("string_q_quoted", 18..35, 0),
                ("MISSING", 35..35, 0),
                ("pod_statement", 38..51, 2),
                ("string_double_quoted", 58..64, 7),
            ],
        ),
    ];
    for (source, recovered, expected) in cases {
        let parser = parsed(source);
        let root = parser.ts_tree().root_node();
        assert_eq!(root.has_error(), recovered, "{}", source.escape_ascii());
        assert_eq!(
            shapes_of(root, source),
            expected,
            "{}",
            source.escape_ascii()
        );
    }
}
