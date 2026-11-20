//! Static literal projection interfaces for deferred language adapters.

use super::{TextBuilder, TextError};
use crate::documentation::budget::WorkBudget;
use crate::documentation::{Content, Origin, SourceContext, UnavailableReason};
use std::ops::Range;

#[derive(Clone, Copy)]
pub(super) enum LiteralPolicy {
    PythonCooked,
    Raw,
    TclQuoted,
    TclBraced,
}

#[allow(clippy::too_many_lines)]
pub(super) fn decode<'s, B: WorkBudget>(
    context: &SourceContext<'s>,
    interior: Range<usize>,
    policy: LiteralPolicy,
    output: &mut TextBuilder<'s>,
    budget: &B,
) -> Result<(), TextError> {
    if !context
        .covers_with_budget(interior.clone(), budget)
        .map_err(|error| TextError::Source(error.into()))?
    {
        return Err(TextError::Unavailable(UnavailableReason::ExcludedGap));
    }
    let bytes = context.bytes();
    let mut at = interior.start;
    let mut copied = at;
    while at < interior.end {
        budget
            .bytes(1)
            .map_err(|error| TextError::Source(error.into()))?;
        if matches!(policy, LiteralPolicy::TclQuoted) && matches!(bytes[at], b'$' | b'[') {
            return Err(TextError::Unavailable(UnavailableReason::Dynamic));
        }
        if bytes[at] != b'\\' || matches!(policy, LiteralPolicy::Raw) {
            at += 1;
            continue;
        }
        let next = bytes
            .get(at + 1)
            .copied()
            .filter(|_| at + 1 < interior.end)
            .ok_or(TextError::Unavailable(UnavailableReason::InvalidEncoding))?;
        if matches!(policy, LiteralPolicy::TclBraced) && !matches!(next, b'\n' | b'\r') {
            at += 2;
            continue;
        }
        output.source(context, copied..at, budget)?;
        let start = at;
        at += 2;
        let scalar = match next {
            b'\n' | b'\r' => {
                if next == b'\r' && bytes.get(at) == Some(&b'\n') {
                    at += 1;
                }
                if matches!(policy, LiteralPolicy::TclQuoted | LiteralPolicy::TclBraced) {
                    while at < interior.end && matches!(bytes[at], b' ' | b'\t') {
                        budget
                            .bytes(1)
                            .map_err(|error| TextError::Source(error.into()))?;
                        at += 1;
                    }
                    Some(' ')
                } else {
                    None
                }
            }
            b'n' => Some('\n'),
            b'r' => Some('\r'),
            b't' => Some('\t'),
            b'a' => Some('\u{7}'),
            b'b' => Some('\u{8}'),
            b'f' => Some('\u{c}'),
            b'v' => Some('\u{b}'),
            b'\\' => Some('\\'),
            b'\'' => Some('\''),
            b'"' => Some('"'),
            b'N' if matches!(policy, LiteralPolicy::PythonCooked) => {
                return Err(TextError::Unavailable(
                    UnavailableReason::UnsupportedDecoding,
                ));
            }
            b'x' | b'u' | b'U' => {
                let width = match next {
                    b'x' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                let end = at
                    .checked_add(width)
                    .filter(|end| *end <= interior.end)
                    .ok_or(TextError::Unavailable(UnavailableReason::InvalidEncoding))?;
                let mut value = 0_u32;
                for &digit in &bytes[at..end] {
                    budget
                        .bytes(1)
                        .map_err(|error| TextError::Source(error.into()))?;
                    value = value
                        .checked_mul(16)
                        .and_then(|value| char::from(digit).to_digit(16).map(|digit| value + digit))
                        .ok_or(TextError::Unavailable(UnavailableReason::InvalidEncoding))?;
                }
                at = end;
                Some(
                    char::from_u32(value)
                        .ok_or(TextError::Unavailable(UnavailableReason::NonScalar))?,
                )
            }
            b'0'..=b'7' => {
                let mut value = u32::from(next - b'0');
                for _ in 0..2 {
                    if at >= interior.end || !matches!(bytes[at], b'0'..=b'7') {
                        break;
                    }
                    budget
                        .bytes(1)
                        .map_err(|error| TextError::Source(error.into()))?;
                    value = value * 8 + u32::from(bytes[at] - b'0');
                    at += 1;
                }
                Some(
                    char::from_u32(value)
                        .ok_or(TextError::Unavailable(UnavailableReason::NonScalar))?,
                )
            }
            _ if matches!(policy, LiteralPolicy::PythonCooked) => {
                output.source(context, start..at, budget)?;
                None
            }
            _ => Some(char::from(next)),
        };
        if let Some(scalar) = scalar {
            let mut buffer = [0; 4];
            output.transformed(
                scalar.encode_utf8(&mut buffer),
                Origin::Decoded(context.pieces_with_budget(start..at, budget)?),
                budget,
            )?;
        }
        copied = at;
    }
    output.source(context, copied..interior.end, budget)
}

pub(super) fn expand_tabs<'s, B: WorkBudget>(
    context: &SourceContext<'s>,
    interior: Range<usize>,
    budget: &B,
) -> Result<Content<'s>, TextError> {
    if !context
        .covers_with_budget(interior.clone(), budget)
        .map_err(|error| TextError::Source(error.into()))?
    {
        return Err(TextError::Unavailable(UnavailableReason::ExcludedGap));
    }
    let mut output = TextBuilder::default();
    let mut start = interior.start;
    let mut column = 0;
    for at in interior.clone() {
        budget
            .bytes(1)
            .map_err(|error| TextError::Source(error.into()))?;
        match context.bytes()[at] {
            b'\t' => {
                output.source(context, start..at, budget)?;
                let count = 8 - column % 8;
                output.transformed(
                    &"        "[..count],
                    Origin::Expanded(context.pieces_with_budget(at..at + 1, budget)?),
                    budget,
                )?;
                column += count;
                start = at + 1;
            }
            b'\n' | b'\r' => column = 0,
            byte if byte & 0xc0 != 0x80 => column += 1,
            _ => {}
        }
    }
    output.source(context, start..interior.end, budget)?;
    Ok(output.finish())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::similar_names)]
    use super::*;
    use crate::documentation::budget::NoBudget;
    use crate::documentation::{CompleteText, OriginView};
    use crate::{AnyParser, LANG};

    #[cfg(any(feature = "python", feature = "tcl"))]
    fn view<'i, 's>(content: &'i Content<'s>) -> CompleteText<'i, 's> {
        let Content::Complete { text, mappings } = content else {
            panic!("complete text")
        };
        CompleteText { text, mappings }
    }

    #[cfg(feature = "python")]
    #[test]
    fn python_raw_concatenation_and_escape_origins_are_independent() {
        let source = b"(\"@au\" # ordinary\n \"dit\\x40\")";
        let parser = AnyParser::parse(LANG::Python, source.to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), source);
        let mut builder = TextBuilder::default();
        decode(&context, 2..5, LiteralPolicy::Raw, &mut builder, &NoBudget).unwrap();
        decode(
            &context,
            20..27,
            LiteralPolicy::PythonCooked,
            &mut builder,
            &NoBudget,
        )
        .unwrap();
        let content = builder.finish();
        let text = view(&content);
        assert_eq!(text.text(), "@audit@");
        let pieces: Vec<_> = text.origins(0..7).unwrap().collect();
        assert_eq!(pieces.len(), 3);
        let OriginView::Decoded(tokens) = &pieces[2].origin else {
            panic!("decoded escape")
        };
        assert_eq!(tokens[0].range(), 23..27);
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn tcl_quoted_and_braced_preambles_use_distinct_substitution_rules() {
        for (source, policy, expected) in [
            (
                "{# Heading\\n  detail}",
                LiteralPolicy::TclBraced,
                "# Heading\\n  detail",
            ),
            (
                "\"# Heading\\n  detail\"",
                LiteralPolicy::TclQuoted,
                "# Heading\n  detail",
            ),
            (
                "{# $name [call]}",
                LiteralPolicy::TclBraced,
                "# $name [call]",
            ),
            ("{a\\\n  b}", LiteralPolicy::TclBraced, "a b"),
        ] {
            let parser =
                AnyParser::parse(LANG::Tcl, source.as_bytes().to_vec(), None, None).unwrap();
            let context = SourceContext::new(parser.ts_tree(), source.as_bytes());
            let mut builder = TextBuilder::default();
            decode(
                &context,
                1..source.len() - 1,
                policy,
                &mut builder,
                &NoBudget,
            )
            .unwrap();
            assert_eq!(view(&builder.finish()).text(), expected);
        }
    }

    #[cfg(feature = "python")]
    #[test]
    fn tab_expansion_maps_every_space_to_the_responsible_token() {
        let source = b"\"a\tb\"";
        let parser = AnyParser::parse(LANG::Python, source.to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), source);
        let content = expand_tabs(&context, 1..4, &NoBudget).unwrap();
        let text = view(&content);
        assert_eq!(text.text(), "a       b");
        let portion = text.origins(3..4).unwrap().next().unwrap();
        let OriginView::Expanded(pieces) = portion.origin else {
            panic!("expanded tab")
        };
        assert_eq!(pieces[0].range(), 2..3);
    }

    #[cfg(feature = "bash")]
    #[test]
    fn bash_comment_projection_preserves_literal_backslash_newline() {
        let source = b"# @description a\\\n# b\nf() { :; }";
        let parser = AnyParser::parse(LANG::Bash, source.to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), source);
        let mut output = TextBuilder::default();
        decode(&context, 2..18, LiteralPolicy::Raw, &mut output, &NoBudget).unwrap();
        let Content::Complete { text, .. } = output.finish() else {
            panic!("raw projection")
        };
        assert_eq!(text, "@description a\\\n");
        assert!(matches!(
            parser.documentation(
                &crate::documentation::DocumentationContext::default(),
                &crate::documentation::DocumentationOptions::default()
            ),
            Err(crate::documentation::CollectionError::AdapterUnavailable {
                language: LANG::Bash,
                ..
            })
        ));
    }
}
