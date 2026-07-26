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

impl Loc for PerlCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool) {
        use Perl as P;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            P::SourceFile
            | P::Block
            | P::StandaloneBlock
            | P::PodContent
            // Internal string tokens — already accounted for by the
            // parent string node's start row.
            | P::SQUOTE
            | P::DQUOTE
            | P::StringContent
            | P::StringSingleQuotedContent
            | P::StringSingleQQuotedContent
            | P::StringQqQuotedContent
            | P::StringDoubleQuotedContent
            | P::EscapeSequence
            | P::EscapeSequenceToken1
            | P::Interpolation => {}
            // Multi-line-capable string literals: their interior rows are
            // real code, not blank lines, so credit every spanned row to
            // PLOC to match Python's #415 decision (#778). `HeredocBodyStatement`
            // / `HeredocContent` are the body of a `<<EOT … EOT` heredoc; the
            // quoted forms span rows when their literal text contains newlines.
            P::HeredocBodyStatement
            | P::HeredocContent
            | P::StringSingleQuoted
            | P::StringDoubleQuoted
            | P::StringQQuoted
            | P::StringQqQuoted
            | P::BacktickQuoted
            | P::CommandQxQuoted => {
                add_multiline_string_ploc(node, stats, start, end);
            }
            P::Comments | P::PodStatement => {
                add_cloc_lines(stats, start, end);
            }
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
                stats.lloc.logical_lines += 1;
            }
            P::SEMI => {
                // A `;` at top of `source_file` / a function `block` ends a
                // statement (Perl wraps simple expressions in semicolons
                // rather than emitting a dedicated statement kind), so it
                // contributes one LLOC. Then fall through to the same PLOC
                // bookkeeping the catch-all arm does.
                if let Some(parent) = node.parent()
                    && matches!(parent.kind_id().into(), P::SourceFile | P::Block)
                {
                    stats.lloc.logical_lines += 1;
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
