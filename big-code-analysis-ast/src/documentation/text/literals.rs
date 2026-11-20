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
        if matches!(policy, LiteralPolicy::TclQuoted)
            && (bytes[at] == b'[' || tcl_variable(&bytes[at..interior.end]))
        {
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
        if matches!(policy, LiteralPolicy::TclBraced)
            && next == b'\r'
            && bytes.get(at + 2).filter(|_| at + 2 < interior.end) != Some(&b'\n')
        {
            at += 2;
            continue;
        }
        output.source(context, copied..at, budget)?;
        let start = at;
        at += 2;
        let scalar = match next {
            b'\n' | b'\r' => {
                let (scalar, consumed) =
                    continuation(&bytes[at..interior.end], next, policy, budget)?;
                at += consumed;
                scalar
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
                let (scalar, count) = hexadecimal(&bytes[at..interior.end], next, policy, budget)?;
                at += count;
                Some(scalar)
            }
            b'0'..=b'7' => {
                let (scalar, consumed) = octal(&bytes[at..interior.end], next, policy, budget)?;
                at += consumed;
                Some(scalar)
            }
            _ if matches!(policy, LiteralPolicy::PythonCooked) => {
                at = start + 1 + scalar_width(&bytes[start + 1..interior.end])?;
                output.source(context, start..at, budget)?;
                None
            }
            _ => {
                let width = scalar_width(&bytes[start + 1..interior.end])?;
                at = start + 1 + width;
                std::str::from_utf8(&bytes[start + 1..at])
                    .map_err(|_| TextError::Unavailable(UnavailableReason::InvalidEncoding))?
                    .chars()
                    .next()
            }
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

fn continuation<B: WorkBudget>(
    tail: &[u8],
    ending: u8,
    policy: LiteralPolicy,
    budget: &B,
) -> Result<(Option<char>, usize), TextError> {
    let mut consumed = usize::from(ending == b'\r' && tail.first() == Some(&b'\n'));
    if matches!(policy, LiteralPolicy::PythonCooked) {
        return Ok((None, consumed));
    }
    if ending == b'\r' && consumed == 0 {
        return Ok((Some('\r'), 0));
    }
    while tail
        .get(consumed)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        budget
            .bytes(1)
            .map_err(|error| TextError::Source(error.into()))?;
        consumed += 1;
    }
    Ok((Some(' '), consumed))
}

fn octal<B: WorkBudget>(
    tail: &[u8],
    first: u8,
    policy: LiteralPolicy,
    budget: &B,
) -> Result<(char, usize), TextError> {
    let mut value = u32::from(first - b'0');
    let mut consumed = 0;
    for &byte in tail.iter().take(2) {
        budget
            .bytes(1)
            .map_err(|error| TextError::Source(error.into()))?;
        if !matches!(byte, b'0'..=b'7') {
            break;
        }
        let next = value * 8 + u32::from(byte - b'0');
        if matches!(policy, LiteralPolicy::TclQuoted) && next > 255 {
            break;
        }
        value = next;
        consumed += 1;
    }
    Ok((
        char::from_u32(value).ok_or(TextError::Unavailable(UnavailableReason::NonScalar))?,
        consumed,
    ))
}

fn tcl_variable(bytes: &[u8]) -> bool {
    bytes.first() == Some(&b'$')
        && bytes.get(1).is_some_and(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'{' | b'(')
                || bytes.get(1..3) == Some(b"::")
        })
}

fn scalar_width(bytes: &[u8]) -> Result<usize, TextError> {
    let width = match bytes.first() {
        Some(0..=0x7f) => 1,
        Some(0xc2..=0xdf) => 2,
        Some(0xe0..=0xef) => 3,
        Some(0xf0..=0xf4) => 4,
        _ => return Err(TextError::Unavailable(UnavailableReason::InvalidEncoding)),
    };
    let scalar = bytes
        .get(..width)
        .ok_or(TextError::Unavailable(UnavailableReason::InvalidEncoding))?;
    std::str::from_utf8(scalar)
        .map_err(|_| TextError::Unavailable(UnavailableReason::InvalidEncoding))?;
    Ok(width)
}

fn hexadecimal<B: WorkBudget>(
    bytes: &[u8],
    marker: u8,
    policy: LiteralPolicy,
    budget: &B,
) -> Result<(char, usize), TextError> {
    let width = match marker {
        b'x' => 2,
        b'u' => 4,
        _ => 8,
    };
    let python = matches!(policy, LiteralPolicy::PythonCooked);
    let mut count = 0;
    let mut value = 0_u32;
    for &byte in bytes.iter().take(width) {
        budget
            .bytes(1)
            .map_err(|error| TextError::Source(error.into()))?;
        let Some(digit) = char::from(byte).to_digit(16) else {
            break;
        };
        let next = value * 16 + digit;
        if !python && marker == b'U' && next > 0x10_ffff {
            break;
        }
        value = next;
        count += 1;
    }
    if python && count != width {
        return Err(TextError::Unavailable(UnavailableReason::InvalidEncoding));
    }
    if count == 0 {
        return Ok((char::from(marker), 0));
    }
    let scalar =
        char::from_u32(value).ok_or(TextError::Unavailable(UnavailableReason::NonScalar))?;
    Ok((scalar, count))
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

    #[cfg(feature = "python")]
    #[test]
    fn python_unknown_unicode_escape_is_copied_and_invalid_scalars_stay_unavailable() {
        for (source, expected) in [
            ("\"\\é\"", Ok("\\é")),
            ("\"\\x4\"", Err(UnavailableReason::InvalidEncoding)),
            ("\"\\ud800\"", Err(UnavailableReason::NonScalar)),
            ("\"\\Uffffffff\"", Err(UnavailableReason::NonScalar)),
            (
                "\"\\N{COMMERCIAL AT}audit\"",
                Err(UnavailableReason::UnsupportedDecoding),
            ),
        ] {
            let parser =
                AnyParser::parse(LANG::Python, source.as_bytes().to_vec(), None, None).unwrap();
            let context = SourceContext::new(parser.ts_tree(), source.as_bytes());
            let mut output = TextBuilder::default();
            let result = decode(
                &context,
                1..source.len() - 1,
                LiteralPolicy::PythonCooked,
                &mut output,
                &NoBudget,
            );
            match expected {
                Ok(expected) => {
                    result.unwrap();
                    let content = output.finish();
                    assert_eq!(view(&content).text(), expected);
                    assert!(matches!(
                        view(&content)
                            .origins(0..expected.len())
                            .unwrap()
                            .next()
                            .unwrap()
                            .origin,
                        OriginView::Copied(_)
                    ));
                }
                Err(reason) => assert!(
                    matches!(result, Err(TextError::Unavailable(actual)) if actual == reason)
                ),
            }
        }
    }

    #[cfg(any(feature = "tcl", feature = "irules"))]
    fn tcl_escapes(language: LANG) {
        for (source, expected, token) in [
            ("\"\\x4!\"", "\u{4}!", 1..4),
            ("\"\\u41!\"", "A!", 1..5),
            ("\"\\U00000041!\"", "A!", 1..11),
            ("\"\\777\"", "?7", 1..4),
            ("\"\\é\"", "é", 1..4),
            ("\"\\xZ\"", "xZ", 1..3),
            ("\"\\\r\n  b\"", " b", 1..6),
        ] {
            let parser =
                AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
            let context = SourceContext::new(parser.ts_tree(), source.as_bytes());
            let mut output = TextBuilder::default();
            decode(
                &context,
                1..source.len() - 1,
                LiteralPolicy::TclQuoted,
                &mut output,
                &NoBudget,
            )
            .unwrap();
            let content = output.finish();
            let Content::Complete { text, mappings } = &content else {
                panic!("complete literal")
            };
            assert_eq!(text, expected);
            let Origin::Decoded(pieces) = &mappings[0].origin else {
                panic!("decoded token")
            };
            assert_eq!(pieces.pieces()[0].range(), token);
        }
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn tcl_variable_width_escapes_preserve_token_origins() {
        tcl_escapes(LANG::Tcl);
    }
    #[cfg(feature = "irules")]
    #[test]
    fn irules_variable_width_escapes_preserve_token_origins() {
        tcl_escapes(LANG::Irules);
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn ruff_preamble_cleanup_preserves_hashes_relative_indent_and_crlf_tokens() {
        for (source, policy) in [
            (
                "{\r\n  # Heading\r\n    detail\r\n}",
                LiteralPolicy::TclBraced,
            ),
            (
                "\"\\n  # Heading\\n    detail\\n\"",
                LiteralPolicy::TclQuoted,
            ),
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
            let content =
                crate::documentation::text::cleanup::ruff(builder.finish(), &NoBudget).unwrap();
            let text = view(&content);
            assert_eq!(text.text(), "# Heading\n  detail");
            let OriginView::Decoded(pieces) = text.origins(9..10).unwrap().next().unwrap().origin
            else {
                panic!("decoded newline")
            };
            assert_eq!(
                pieces
                    .iter()
                    .flat_map(crate::documentation::SourcePiece::bytes)
                    .copied()
                    .collect::<Vec<_>>(),
                if source.starts_with('{') {
                    b"\r\n"
                } else {
                    b"\\n"
                }
            );
        }
    }
}
