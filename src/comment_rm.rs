// bca: suppress-file(halstead)
// Per-language comment-removal dispatch; file-level halstead is a many-fn
// aggregation artifact, not per-function logic complexity.
// bca: suppress-file(nargs)
// File-level nargs is the sum across the small LineEnding helpers and the
// walk core; each function's own arity is modest — the file total is an
// aggregation artifact (issue #767 added the LineEnding helper), not a
// wide-signature smell.

// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::if_not_else, clippy::wildcard_imports)]

use crate::checker::Checker;
use crate::traits::ParserTrait;

/// Size of the fast-path newline buffers. A removed multi-line comment span
/// is replaced by one newline per interior line break; for spans up to this
/// many lines the bytes come from a pre-filled `const` slice instead of a
/// per-line push. Spans longer than this fall back to `resize_with`.
const NEWLINE_FAST_PATH_LEN: usize = 8192;

/// Fast-path buffer of bare LF newlines (the convention for non-CRLF input).
const LF_NEWLINES: [u8; NEWLINE_FAST_PATH_LEN] = [b'\n'; NEWLINE_FAST_PATH_LEN];

/// Fast-path buffer of CRLF newline pairs, used when the source file's
/// dominant line ending is CRLF so removed-comment lines keep `\r\n` (issue
/// #767). Stored as `2 * NEWLINE_FAST_PATH_LEN` bytes; `lines` newline pairs
/// occupy the first `2 * lines` bytes.
const CRLF_NEWLINES: [u8; 2 * NEWLINE_FAST_PATH_LEN] = {
    let mut buf = [b'\r'; 2 * NEWLINE_FAST_PATH_LEN];
    let mut i = 1;
    while i < buf.len() {
        buf[i] = b'\n';
        i += 2;
    }
    buf
};

/// The line-ending convention detected for an input buffer. Determines which
/// newline sequence is substituted for each line a removed comment spanned, so
/// stripping comments preserves the source file's existing convention.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    /// Unix-style: substitute a single `\n` per removed comment line.
    Lf,
    /// Windows-style: substitute `\r\n` per removed comment line.
    Crlf,
}

impl LineEnding {
    /// Detects the dominant line ending from the first newline in `code`.
    ///
    /// A buffer with mixed conventions is treated as whichever its *first*
    /// newline uses — per-line matching is deliberately out of scope (issue
    /// #767). A buffer with no newline (or none reachable) defaults to `Lf`.
    fn detect(code: &[u8]) -> Self {
        match code.iter().position(|&b| b == b'\n') {
            Some(nl) if nl > 0 && code[nl - 1] == b'\r' => Self::Crlf,
            _ => Self::Lf,
        }
    }

    /// Appends `lines` newline sequences in this convention to `out`, using a
    /// pre-filled `const` fast path for spans up to [`NEWLINE_FAST_PATH_LEN`]
    /// lines and falling back to `resize_with` for longer spans.
    fn extend_newlines(self, out: &mut Vec<u8>, lines: usize) {
        match self {
            Self::Lf => {
                if lines <= NEWLINE_FAST_PATH_LEN {
                    out.extend(&LF_NEWLINES[..lines]);
                } else {
                    out.resize(out.len() + lines, b'\n');
                }
            }
            Self::Crlf => {
                if lines <= NEWLINE_FAST_PATH_LEN {
                    out.extend(&CRLF_NEWLINES[..2 * lines]);
                } else {
                    for _ in 0..lines {
                        out.extend_from_slice(b"\r\n");
                    }
                }
            }
        }
    }
}

/// Removes comments from a code. Crate-internal walk core reached
/// through the [`crate::Ast::strip_comments`] seam.
pub(crate) fn rm_comments<T: ParserTrait>(parser: &T) -> Option<Vec<u8>> {
    let node = parser.root();
    let mut stack = Vec::new();
    let mut cursor = node.cursor();
    let mut spans = Vec::new();

    stack.push(node);

    while let Some(node) = stack.pop() {
        if T::Checker::is_comment(&node) && !T::Checker::is_useful_comment(&node, parser.code()) {
            let lines = node.end_row() - node.start_row();
            spans.push((node.start_byte(), node.end_byte(), lines));
        } else {
            cursor.reset(&node);
            if cursor.goto_first_child() {
                loop {
                    stack.push(cursor.node());
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
    }
    if !spans.is_empty() {
        Some(remove_from_code(parser.code(), spans))
    } else {
        None
    }
}

fn remove_from_code(code: &[u8], mut spans: Vec<(usize, usize, usize)>) -> Vec<u8> {
    // The kept code on either side of a comment retains its own newline
    // bytes verbatim; only the `lines` interior line breaks of the removed
    // comment are substituted. Emitting `\r\n` for a CRLF source keeps the
    // whole buffer single-convention (issue #767).
    let line_ending = LineEnding::detect(code);
    let mut new_code = Vec::with_capacity(code.len());
    let mut code_start = 0;
    for (start, end, lines) in spans.drain(..).rev() {
        new_code.extend(&code[code_start..start]);
        if lines != 0 {
            line_ending.extend_newlines(&mut new_code, lines);
        }
        // A single-line comment node absorbs the `\r` of its terminating
        // CRLF (the grammar ends the comment at the CR, leaving the LF to
        // terminate the line), so removing the span would orphan a bare `\n`
        // in the kept code. Restore the `\r` when the removed span ended on
        // one immediately before that LF (issue #767). LF-only input never
        // contains `\r`, so this is a no-op there.
        if end > start && code[end - 1] == b'\r' && code.get(end).copied() == Some(b'\n') {
            new_code.push(b'\r');
        }
        code_start = end;
    }
    if code_start < code.len() {
        new_code.extend(&code[code_start..]);
    }
    new_code
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use std::path::PathBuf;

    use crate::{CcommentParser, ParserTrait};

    use super::rm_comments;

    const SOURCE_CODE: &str = "/* Remove this code block */\n\
                               int a = 42; // Remove this comment\n\
                               // Remove this comment\n\
                               int b = 42;\n\
                               /* Remove\n\
                                * this\n\
                                * comment\n\
                                */";

    const SOURCE_CODE_NO_COMMENTS: &str = "\n\
                                           int a = 42; \n\
                                           \n\
                                           int b = 42;\n\
                                           \n\
                                           \n\
                                           \n\
                                           \n";

    #[test]
    fn ccomment_remove_comments() {
        let path = PathBuf::from("foo.c");
        let mut trimmed_bytes = SOURCE_CODE.as_bytes().to_vec();
        trimmed_bytes.push(b'\n');
        let parser = CcommentParser::new(trimmed_bytes, &path, None);

        let no_comments = rm_comments(&parser).unwrap();

        assert_eq!(no_comments.as_slice(), SOURCE_CODE_NO_COMMENTS.as_bytes());

        // The LF input must stay LF: no `\r` may sneak into the output.
        assert!(
            !no_comments.contains(&b'\r'),
            "LF source must not gain CR bytes"
        );
    }

    /// Stripping a multi-line comment from a CRLF source must keep every
    /// newline as `\r\n` — including the lines the removed comment spanned
    /// (issue #767). Before the fix, `remove_from_code` substituted bare `\n`
    /// for those lines, producing a mixed-ending buffer.
    #[test]
    fn ccomment_remove_comments_preserves_crlf() {
        let path = PathBuf::from("foo.c");
        // Same shape as SOURCE_CODE but with CRLF endings throughout; the
        // trailing multi-line block comment spans three interior newlines.
        let crlf_source = b"/* Remove this code block */\r\n\
            int a = 42; // Remove this comment\r\n\
            // Remove this comment\r\n\
            int b = 42;\r\n\
            /* Remove\r\n\
            \x20* this\r\n\
            \x20* comment\r\n\
            \x20*/\r\n";
        let parser = CcommentParser::new(crlf_source.to_vec(), &path, None);

        let no_comments = rm_comments(&parser).unwrap();

        // Every `\n` in the output must be preceded by `\r`: no bare LF
        // survives anywhere, including the removed-comment region.
        for (i, &byte) in no_comments.iter().enumerate() {
            if byte == b'\n' {
                assert!(
                    i > 0 && no_comments[i - 1] == b'\r',
                    "bare LF at byte {i} corrupts CRLF line endings"
                );
            }
        }

        // CR-prefix consistency alone would still pass an impl that *dropped*
        // removed-comment lines (fewer, still-CR-prefixed newlines). Comment
        // removal blanks lines, never deletes them, so the newline count must
        // be preserved exactly — pinning the per-line substitution count.
        let lf_count = |buf: &[u8]| buf.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(
            lf_count(&no_comments),
            lf_count(crlf_source),
            "comment removal must preserve the CRLF line count, not drop lines"
        );
    }
}
