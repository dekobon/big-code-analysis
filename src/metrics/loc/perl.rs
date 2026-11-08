//! `Loc` implementation for Perl.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;
use crate::lang_helpers::perl::{PerlDataSectionRows, perl_data_section_rows};

/// Bills the rows after `__END__` / `__DATA__`, which the grammar holds
/// as one leaf (#1608): a POD block's rows as comment rows, and as
/// documentation unless the block is only `comment` regions, like POD
/// before the marker; every other non-blank row as code. The text is
/// neither, but MI then charges it rather than exempting it.
fn add_data_section_lines(section: &Node, code: &[u8], stats: &mut Stats) {
    let bytes = code
        .get(section.start_byte()..section.end_byte())
        .unwrap_or_default();
    perl_data_section_rows(bytes, section.start_row(), |rows| match rows {
        PerlDataSectionRows::Pod {
            first,
            last,
            documentation,
        } => add_comment_rows(stats, first, last, documentation),
        PerlDataSectionRows::Data(row) => {
            stats.ploc.lines.insert(row);
        }
    });
}

impl Loc for PerlCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        is_func_space: bool,
    ) {
        use Perl as P;

        let start = init(node, stats, is_func_space);

        match node.kind_id().into() {
            P::SourceFile | P::Block | P::StandaloneBlock | P::PodContent => {}
            // Internal string tokens — already accounted for by the
            // parent string node's start row.
            P::SQUOTE
            | P::DQUOTE
            | P::StringContent
            | P::StringSingleQuotedContent
            | P::StringSingleQQuotedContent
            | P::StringQqQuotedContent
            | P::StringDoubleQuotedContent
            | P::EscapeSequence
            | P::EscapeSequenceToken1
            | P::Interpolation
            | P::HASH => {}
            // Multi-line-capable string literals: their interior rows are
            // real code, not blank lines, so credit every spanned row to
            // PLOC to match Python's #415 decision (#778). `HeredocBodyStatement`
            // / `HeredocContent` are the body of a `<<EOT … EOT` heredoc; the
            // quoted forms span rows when their literal text contains newlines.
            // A `qw()` list and the five pattern forms own their rows the
            // same way: an empty row between two `qw` words is covered by
            // no child, and a pattern's text is one `regex_pattern_content`
            // leaf credited at its start row, so a `\` + newline + `\` +
            // newline inside it, or an empty row in a replacement half,
            // fell to `blank` (#1816, the Perl shape of Ruby's `%w[]` and
            // regex). Each has an opening token the catch-all credits on
            // its row, so the helper's same-row skip is safe here too.
            P::HeredocBodyStatement
            | P::HeredocContent
            | P::StringSingleQuoted
            | P::StringDoubleQuoted
            | P::StringQQuoted
            | P::StringQqQuoted
            | P::BacktickQuoted
            | P::CommandQxQuoted
            | P::WordListQw
            | P::RegexPatternQr
            | P::PatternMatcherM
            | P::PatternMatcher
            | P::SubstitutionPatternS
            | P::TransliterationTrOrY => {
                add_multiline_string_ploc(node, ancestors, stats, start);
            }
            P::Comments | P::PodStatement => {
                add_comment_lines::<Self>(node, code, ancestors, stats);
            }
            // The marker's own row is billed by the `__END__` / `__DATA__`
            // token through the catch-all.
            P::DataNotForCompiler => add_data_section_lines(node, code, stats),
            P::SingleLineStatement
            | P::IfStatement
            | P::UnlessStatement
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
            | P::ForStatement2
            | P::LoopControlStatement
            | P::PackageStatement
            | P::RequireStatement
            | P::UseNoStatement
            | P::UseNoFeatureStatement
            | P::UseNoIfStatement
            | P::UseNoSubsStatement
            | P::UseConstantStatement
            | P::UseParentStatement
            | P::UseNoVersion
            | P::EllipsisStatement => {
                stats.lloc.count_logical_line();
            }
            P::SEMI => {
                // A `;` at top of `source_file` / a function `block` ends a
                // statement (Perl wraps simple expressions in semicolons
                // rather than emitting a dedicated statement kind), so it
                // contributes one LLOC. Then fall through to the same PLOC
                // bookkeeping the catch-all arm does.
                if let Some(parent) = ancestors.parent(node)
                    && matches!(parent.kind_id().into(), P::SourceFile | P::Block)
                {
                    stats.lloc.count_logical_line();
                }
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

#[cfg(all(test, feature = "perl"))]
mod tests {
    use crate::LANG;
    use crate::Metric;
    use crate::MetricsOptions;
    use crate::test_support::metrics_verbatim;

    /// `[sloc, ploc, cloc, dloc, blank]` of a whole Perl file.
    fn loc_of(source: &str) -> [u64; 5] {
        let options = MetricsOptions::default().with_only(&[Metric::Loc]);
        let loc = metrics_verbatim(LANG::Perl, source.as_bytes(), options).loc;
        [loc.sloc(), loc.ploc(), loc.cloc(), loc.dloc(), loc.blank()]
    }

    /// The grammar holds everything after `__END__` / `__DATA__` as one
    /// leaf (#1608). A POD block there is billed as POD before the marker
    /// is: every row from its command to its `=cut` is a comment row, and
    /// a documentation row unless the block is only `comment` regions.
    /// Any other non-blank row is code, other blank rows stay blank, and
    /// the marker's row is code.
    #[test]
    fn perl_data_section_rows() {
        // (source, [sloc, ploc, cloc, dloc, blank])
        let cases: &[(&str, [u64; 5])] = &[
            // ploc: rows 1-4; POD: rows 6-10; blank: row 5.
            (
                "package P;\nsub f { 1 }\n1;\n__END__\n\n=head1 NAME\n\nP - thing\n\n=cut\n",
                [10, 4, 5, 5, 1],
            ),
            // ploc: rows 1-3; POD: rows 5-9; blank: row 4.
            (
                "package P;\n1;\n__DATA__\n\n=pod\n\nText.\n\n=cut\n",
                [9, 3, 5, 5, 1],
            ),
            // Two blocks of `comment` regions, rows 3-9 and 11-14: comment
            // rows, not documentation. Row 10 is blank between them.
            (
                "1;\n__END__\n=begin comment\n\nold();\n\n=end comment\n\n=cut\n\n=for comment\nx\n\n=cut\n",
                [14, 2, 11, 0, 1],
            ),
            // Each block is judged on its own text: a `comment` block
            // (rows 3-6) then a documenting one (rows 8-11, dloc).
            (
                "1;\n__END__\n=for comment\nx\n\n=cut\n\n=head1 A\n\na\n=cut\n",
                [11, 2, 8, 4, 1],
            ),
            // No POD: rows 1-3 and 5-6 are code, row 4 blank.
            ("1;\n__DATA__\na b\n\nc d\ne\n", [6, 5, 0, 0, 1]),
            // An unterminated block runs to its last text row, 6; row 7
            // after it is blank, as after an unterminated inline block.
            ("1;\n__END__\nd\n=head1 NAME\n\nP\n\n", [7, 3, 3, 3, 1]),
            // POD (rows 3-6), data (7), blank (8), POD (9-11).
            (
                "1;\n__END__\n=head1 A\n\na\n=cut\nrow\n\n=head1 B\nb\n=cut\n",
                [11, 3, 7, 7, 1],
            ),
            // `=1` and `=head-x` open nothing, and `=cut` outside a block
            // is data.
            ("1;\n__DATA__\n=1\nx\n=head-x\n=cut\n", [6, 6, 0, 0, 0]),
            // The section starts mid-row, after the marker: text there is
            // not a line start, so it opens nothing and leaves the later
            // `=cut` stray. Every row is code.
            ("1;\n__END__=head1 A\nx\n=cut\n", [4, 4, 0, 0, 0]),
            // Any line starting `=cut` closes, trailing text and all: POD
            // rows 3-5 and 7-8, data rows 6 and 9.
            (
                "1;\n__END__\n=head1 A\nx\n=cut foo\ny\n=pod\n=cutting\nz\n",
                [9, 4, 5, 5, 0],
            ),
            // `=cut` ends the block while its `=begin comment` is still
            // open, so rows 3-5 are a comment block; the `=end comment`
            // after it opens a new, documenting block, rows 6-7. Judged on
            // text past its `=cut`, the first block would document too.
            (
                "1;\n__END__\n=begin comment\nx\n=cut\n=end comment\nDoc\n",
                [7, 2, 5, 2, 0],
            ),
            // CRLF line ends: `=head1\r` opens, `=cut\r` closes.
            (
                "1;\r\n__END__\r\n=head1\r\nx\r\n=cut\r\ny\r\n",
                [6, 3, 3, 3, 0],
            ),
            // Outside a block `=cutting` is a command that opens one, and
            // the `^=cut` test closes it on the same row: POD row 3, data
            // rows 4-5.
            ("1;\n__END__\n=cutting\ndata1\ndata2\n", [5, 4, 1, 1, 0]),
            // Perl's `\s` after the command takes a vertical tab, so
            // `=head1<VT>NAME` opens a block that runs to the end of the
            // file: POD rows 3-4.
            ("1;\n__END__\n=head1\x0bNAME\nx\n", [4, 2, 2, 2, 0]),
        ];
        for (source, expected) in cases {
            assert_eq!(loc_of(source), *expected, "{source}");
        }
    }

    /// #1816's Perl shape: a `qw()` list and the five pattern forms span
    /// rows whose interior reaches no child — an empty row between two
    /// `qw` words is covered by nothing, and a pattern's text is one
    /// `regex_pattern_content` leaf credited at its start row — so each
    /// such row fell to `blank = sloc - ploc - cloc`. Every fixture
    /// passes `perl -c` (5.34).
    ///
    /// expected: every row of the literal is code, as the string twin
    /// `q(a` / `` / `b)` already read; `dloc` 0 throughout. Before the
    /// fix each three-row literal read `ploc` one short and `blank` 1,
    /// the five-row `qw` `(5, 4, 0, 0, 1)` and the substitution with a
    /// blank in each half `(5, 3, 0, 0, 2)`. The rows with an empty row
    /// *outside* the literal pin that the credit stops at the closing
    /// delimiter: `blank()` saturates at zero, so a credit one row too
    /// far would hide without them. Perturbed: dropping any one of the
    /// six kinds from the arm fails that kind's rows alone.
    ///
    /// Not pinned: an unterminated `qw(` at EOF is a flat `ERROR` with no
    /// `word_list_qw` node, so its empty row stays blank — a recovery
    /// shape, not this policy's (`grammar_dispatch.md` §6).
    #[test]
    fn perl_literal_rows_are_code() {
        // (source, [sloc, ploc, cloc, dloc, blank])
        let cases: &[(&str, [u64; 5])] = &[
            // `qw()`: an empty row between words, the two-empty-row and
            // five-row shapes, alternate delimiters, CRLF, EOF.
            ("my @x = qw(a\n\nb);\n", [3, 3, 0, 0, 0]),
            ("my @x = qw(a\n\n\nb);\n", [4, 4, 0, 0, 0]),
            ("my @x = qw(\n  a\n\n  b\n);\n", [5, 5, 0, 0, 0]),
            ("my @x = qw[a\n\nb];\n", [3, 3, 0, 0, 0]),
            ("my @x = qw{a\n\nb};\n", [3, 3, 0, 0, 0]),
            ("my @x = qw(a\r\n\r\nb);\r\n", [3, 3, 0, 0, 0]),
            ("my @x = qw(a\n\nb)", [3, 3, 0, 0, 0]),
            // The five pattern forms, each with `\` + newline + `\` +
            // newline inside the pattern.
            ("my $r = qr/ab\\\n\\\ncd/;\n", [3, 3, 0, 0, 0]),
            ("$x =~ m/ab\\\n\\\ncd/;\n", [3, 3, 0, 0, 0]),
            ("$x =~ /ab\\\n\\\ncd/;\n", [3, 3, 0, 0, 0]),
            ("$x =~ s/ab\\\n\\\ncd/e/;\n", [3, 3, 0, 0, 0]),
            ("$x =~ tr/ab\\\n\\\ncd/xy/;\n", [3, 3, 0, 0, 0]),
            // An empty row in the replacement half alone, and in both
            // halves: the two halves are separate content sequences.
            ("$x =~ s/ab/x\n\ny/;\n", [3, 3, 0, 0, 0]),
            ("$x =~ tr/ab/x\n\ny/;\n", [3, 3, 0, 0, 0]),
            ("$x =~ s/ab\\\n\\\ncd/x\n\ny/;\n", [5, 5, 0, 0, 0]),
            // An empty row outside the literal stays blank.
            ("my @x = qw(a\n\nb);\n\nmy $y = 1;\n", [5, 4, 0, 0, 1]),
            ("$x =~ /ab\\\n\\\ncd/;\n\nmy $y = 1;\n", [5, 4, 0, 0, 1]),
            // Controls: a one-row list; a `#` inside `qw()` is a word to
            // perl and to the grammar, so that row was already code; the
            // `q()` twin of the first shape.
            ("my @x = qw(a b);\nmy $y = 1;\n", [2, 2, 0, 0, 0]),
            ("my @x = qw(a\n# word\nb);\n", [3, 3, 0, 0, 0]),
            ("my $s = q(a\n\nb);\n", [3, 3, 0, 0, 0]),
        ];
        for (source, expected) in cases {
            assert_eq!(loc_of(source), *expected, "{source}");
        }
    }

    /// A `(?{ … })` code block inside a pattern is Perl code to perl, and
    /// a `#` in it a comment; the grammar holds the whole pattern as one
    /// `regex_pattern_content` leaf and emits no `comments` node there,
    /// as it emits none inside the `qq{}` and `"…"` interpolations that
    /// read the same way today. The rows are code to this policy; what
    /// `cloc` should read for the `# c` row is the grammar's gap, not
    /// pinned here (lesson 6).
    #[test]
    fn perl_pattern_code_block_rows_are_code() {
        let [sloc, ploc, _cloc, _dloc, blank] = loc_of("$x =~ /a(?{\n# c\n1\n})b/;\n");
        assert_eq!((sloc, ploc, blank), (4, 4, 0));
    }

    /// The literal inside a `sub`: its rows are the sub's own code, so
    /// the nested space reads them, not only the file.
    ///
    /// expected: the `sub` is the file's six rows, `sub f {` through `}`,
    /// and every one is code, in the unit and in the function; before
    /// the fix both read the empty row blank, `(6, 5, 0, 1)`.
    #[test]
    fn perl_literal_rows_in_a_sub_are_its_code() {
        let options = MetricsOptions::default().with_only(&[Metric::Loc]);
        let source = b"sub f {\n  my @x = qw(a\n\nb);\n  return @x;\n}\n";
        let unit = crate::test_support::space_verbatim(LANG::Perl, source, options);
        let rows = |loc: &crate::loc::Stats| (loc.sloc(), loc.ploc(), loc.cloc(), loc.blank());
        assert_eq!(rows(&unit.metrics.loc), (6, 6, 0, 0), "file");
        let sub = unit
            .spaces
            .iter()
            .find(|s| s.name.as_deref() == Some("f"))
            .expect("the sub opens a space");
        assert_eq!(rows(&sub.metrics.loc), (6, 6, 0, 0), "sub f");
    }
}
