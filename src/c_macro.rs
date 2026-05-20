use std::collections::HashSet;

use crate::c_langs_macros::is_predefined_macros;

#[inline]
fn is_identifier_part(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'
}

#[inline]
fn is_identifier_starter(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_lowercase() || c == b'_'
}

#[inline]
fn is_macro<S: ::std::hash::BuildHasher>(mac: &str, macros: &HashSet<String, S>) -> bool {
    macros.contains(mac) || is_predefined_macros(mac)
}

/// Lexical state for [`replace`]'s single-pass byte scanner.
///
/// The macro-masking prepass intentionally re-implements just enough
/// C/C++ lexing to know when an identifier run could be expanded by the
/// preprocessor. Strings, character literals, comments, and raw strings
/// are all regions where the preprocessor leaves identifiers alone, so
/// we skip identifier scanning while in those states.
#[derive(Clone, Copy, Debug)]
enum LexState {
    Normal,
    String,
    Char,
    LineComment,
    BlockComment,
    /// Raw string literal `R"delim(...)delim"` (C++11). `delim_start`
    /// and `delim_len` reference the delimiter bytes inside `code`,
    /// avoiding a heap allocation for the typical empty / short delim.
    RawString {
        delim_start: usize,
        delim_len: usize,
    },
}

fn step_normal<S: ::std::hash::BuildHasher>(
    code: &[u8],
    i: usize,
    k_start: &mut usize,
    code_start: &mut usize,
    new_code: &mut Vec<u8>,
    macros: &HashSet<String, S>,
    state: &mut LexState,
) -> usize {
    let c = code[i];

    // Identifier-run termination must happen *before* state transitions
    // — a sequence like `DBG//` or `DBG"` must classify `DBG` as a macro
    // even though the very next byte also starts a new lexical state.
    if *k_start != 0 && !is_identifier_part(c) {
        let start = *k_start - 1;
        *k_start = 0;
        let keyword = str::from_utf8(&code[start..i])
            .expect("invariant: bytes filtered to ASCII-only by is_identifier_part");
        if c == b'"' && is_raw_string_prefix(&code[start..i]) {
            // Raw-string prefix (`R"…`, `uR"…`, etc.) — DO NOT mask the
            // prefix even if it happens to also be a macro name, and
            // enter raw-string mode instead of plain string mode.
            return enter_raw_string(code, i, state);
        }
        if is_macro(keyword, macros) {
            new_code.extend(&code[*code_start..start]);
            new_code.resize(new_code.len() + (i - start), b'$');
            *code_start = i;
        }
    }

    // Comment openers: `//` and `/*`. `/` is not an identifier byte, so
    // by the time we get here `k_start` has already been classified.
    if c == b'/' && i + 1 < code.len() {
        match code[i + 1] {
            b'/' => {
                *state = LexState::LineComment;
                return 2;
            }
            b'*' => {
                *state = LexState::BlockComment;
                return 2;
            }
            _ => {}
        }
    }

    if c == b'"' {
        *state = LexState::String;
        return 1;
    }
    if c == b'\'' {
        *state = LexState::Char;
        return 1;
    }
    if *k_start == 0 && is_identifier_starter(c) {
        *k_start = i + 1;
    }
    1
}

/// `i` points at the opening `"` of a raw string whose prefix
/// identifier (`R`, `uR`, …) has already been consumed. Parse the
/// delimiter (bytes between `"` and `(`) and transition to
/// `RawString`. Returns the number of bytes consumed up to and
/// including the `(`.
fn enter_raw_string(code: &[u8], i: usize, state: &mut LexState) -> usize {
    debug_assert_eq!(code[i], b'"');
    let delim_start = i + 1;
    let mut delim_end = delim_start;
    while delim_end < code.len() && code[delim_end] != b'(' {
        // C++ caps the d-char-sequence at 16 chars. Defensive bail-out:
        // if we hit EOF or 16-char limit without finding `(`, fall back
        // to plain-string semantics so we still skip identifiers.
        if delim_end - delim_start >= 16 {
            *state = LexState::String;
            return 1;
        }
        delim_end += 1;
    }
    if delim_end >= code.len() {
        *state = LexState::String;
        return 1;
    }
    *state = LexState::RawString {
        delim_start,
        delim_len: delim_end - delim_start,
    };
    // Consumed `"`, the delim bytes, and the `(`.
    delim_end - i + 1
}

/// True if `ident` is a recognized C++ raw-string prefix (the part
/// before the `"`). `R`, `uR`, `UR`, `LR`, or `u8R`.
fn is_raw_string_prefix(ident: &[u8]) -> bool {
    matches!(ident, b"R" | b"uR" | b"UR" | b"LR" | b"u8R")
}

/// Step inside a `"..."` string or `'.'` char literal. `quote` is the
/// terminating byte; backslash-escapes (including line continuations)
/// are consumed in one step so `\"` / `\'` do not exit the literal.
fn step_quoted(code: &[u8], i: usize, quote: u8, state: &mut LexState) -> usize {
    let c = code[i];
    if c == b'\\' && i + 1 < code.len() {
        return 2;
    }
    if c == quote {
        *state = LexState::Normal;
    }
    1
}

fn step_line_comment(code: &[u8], i: usize, state: &mut LexState) -> usize {
    let c = code[i];
    if c == b'\\' && i + 1 < code.len() && code[i + 1] == b'\n' {
        // Backslash-newline continues the comment onto the next
        // physical line — do not exit the comment.
        return 2;
    }
    if c == b'\n' {
        *state = LexState::Normal;
    }
    1
}

fn step_block_comment(code: &[u8], i: usize, state: &mut LexState) -> usize {
    if code[i] == b'*' && i + 1 < code.len() && code[i + 1] == b'/' {
        *state = LexState::Normal;
        return 2;
    }
    1
}

fn step_raw_string(
    code: &[u8],
    i: usize,
    delim_start: usize,
    delim_len: usize,
    state: &mut LexState,
) -> usize {
    // Raw strings end at the first `)<delim>"`. No escape processing
    // happens inside, so this is a literal match.
    if code[i] == b')' {
        let close_quote = i + 1 + delim_len;
        if close_quote < code.len()
            && code[close_quote] == b'"'
            && code[i + 1..close_quote] == code[delim_start..delim_start + delim_len]
        {
            *state = LexState::Normal;
            return close_quote - i + 1;
        }
    }
    1
}

pub fn replace<S: ::std::hash::BuildHasher>(
    code: &[u8],
    macros: &HashSet<String, S>,
) -> Option<Vec<u8>> {
    let mut new_code = Vec::with_capacity(code.len());
    let mut code_start = 0;
    let mut k_start = 0;
    let mut state = LexState::Normal;
    let mut i = 0;

    while i < code.len() {
        match state {
            LexState::Normal => {
                let consumed = step_normal(
                    code,
                    i,
                    &mut k_start,
                    &mut code_start,
                    &mut new_code,
                    macros,
                    &mut state,
                );
                i += consumed;
            }
            LexState::String => i += step_quoted(code, i, b'"', &mut state),
            LexState::Char => i += step_quoted(code, i, b'\'', &mut state),
            LexState::LineComment => i += step_line_comment(code, i, &mut state),
            LexState::BlockComment => i += step_block_comment(code, i, &mut state),
            LexState::RawString {
                delim_start,
                delim_len,
            } => i += step_raw_string(code, i, delim_start, delim_len, &mut state),
        }
    }

    // Trailing identifier at end of input (only if we end in Normal state
    // and were tracking an identifier when input ran out).
    if k_start != 0 && matches!(state, LexState::Normal) {
        let start = k_start - 1;
        let end = code.len();
        let keyword = str::from_utf8(&code[start..end])
            .expect("invariant: bytes filtered to ASCII-only by is_identifier_part");
        if is_macro(keyword, macros) {
            new_code.extend(&code[code_start..start]);
            new_code.resize(new_code.len() + (end - start), b'$');
            code_start = end;
        }
    }

    if code_start == 0 {
        None
    } else {
        // `code[code_start..]` is `&[]` when `code_start == code.len()`,
        // so the extend is a no-op in that case — no branch needed.
        new_code.extend(&code[code_start..]);
        Some(new_code)
    }
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
    use super::*;

    #[test]
    fn test_replace() {
        let mut mac = HashSet::new();
        mac.insert("abc".to_string());

        assert!(replace(b"def ghi jkl", &mac).is_none());
        assert_eq!(
            b"$$$ def ghi jkl".to_vec(),
            replace(b"abc def ghi jkl", &mac).unwrap()
        );
        assert_eq!(
            b"def $$$ ghi jkl".to_vec(),
            replace(b"def abc ghi jkl", &mac).unwrap()
        );
        assert_eq!(
            b"def ghi $$$ jkl".to_vec(),
            replace(b"def ghi abc jkl", &mac).unwrap()
        );
        assert_eq!(
            b"def ghi jkl $$$".to_vec(),
            replace(b"def ghi jkl abc", &mac).unwrap()
        );

        mac.insert("z9_".to_string());
        assert_eq!(
            b"$$$ def ghi $$$ jkl".to_vec(),
            replace(b"abc def ghi z9_ jkl", &mac).unwrap()
        );
    }

    #[test]
    fn replace_at_old_buffer_size() {
        let name = "a".repeat(2048);
        let mut mac = HashSet::new();
        mac.insert(name.clone());

        assert_eq!(vec![b'$'; 2048], replace(name.as_bytes(), &mac).unwrap());
    }

    #[test]
    fn replace_just_past_old_buffer_size() {
        let name = "a".repeat(2049);
        let mut mac = HashSet::new();
        mac.insert(name.clone());

        assert_eq!(vec![b'$'; 2049], replace(name.as_bytes(), &mac).unwrap());
    }

    #[test]
    fn replace_long_macro_in_middle() {
        let name = "a".repeat(10_000);
        let mut mac = HashSet::new();
        mac.insert(name.clone());

        let source = format!("int x = {name}; ");
        let mut expected = b"int x = ".to_vec();
        expected.extend(std::iter::repeat_n(b'$', 10_000));
        expected.extend(b"; ");

        assert_eq!(expected, replace(source.as_bytes(), &mac).unwrap());
    }

    fn dbg_macros() -> HashSet<String> {
        let mut mac = HashSet::new();
        mac.insert("DBG".to_string());
        mac
    }

    #[test]
    fn macro_in_string_literal_not_replaced() {
        // Real preprocessor never expands inside `"..."`; our prepass
        // must follow suit so the parser sees the original string.
        let mac = dbg_macros();
        assert!(replace(b"const char *s = \"DBG\";", &mac).is_none());
    }

    #[test]
    fn macro_in_char_literal_not_replaced() {
        let mut mac = HashSet::new();
        mac.insert("D".to_string());
        assert!(replace(b"char c = 'D';", &mac).is_none());
    }

    #[test]
    fn macro_in_line_comment_not_replaced() {
        let mac = dbg_macros();
        assert!(replace(b"int x; // DBG goes here\n", &mac).is_none());
    }

    #[test]
    fn macro_in_block_comment_not_replaced() {
        let mac = dbg_macros();
        assert!(replace(b"int x; /* DBG */ int y;", &mac).is_none());
    }

    #[test]
    fn macro_outside_string_replaced() {
        let mac = dbg_macros();
        assert_eq!(b"$$$ x;".to_vec(), replace(b"DBG x;", &mac).unwrap());
    }

    #[test]
    fn escaped_quote_in_string_not_replaced() {
        // `"\"DBG\""` is a single string literal whose escaped inner
        // quotes must not be mistaken for end-of-string.
        let mac = dbg_macros();
        assert!(replace(b"const char *s = \"\\\"DBG\\\"\";", &mac).is_none());
    }

    #[test]
    fn raw_string_literal_skipped() {
        // `R"(DBG)"` is a C++11 raw string literal — DBG inside is not
        // a macro reference.
        let mac = dbg_macros();
        assert!(replace(b"auto s = R\"(DBG)\";", &mac).is_none());
    }

    #[test]
    fn raw_string_literal_with_delim_skipped() {
        let mac = dbg_macros();
        assert!(replace(b"auto s = R\"xy(DBG)xy\";", &mac).is_none());
    }

    #[test]
    fn nested_block_comment_not_supported_but_handled_gracefully() {
        // C/C++ block comments do not nest. The outer `/*` opens, the
        // first `*/` closes — everything after that closer is back in
        // normal lexer state, so `DBG` after the closer must be
        // replaced. The trailing `*/` becomes a stray sequence (which
        // is what a real C compiler would also see).
        let mac = dbg_macros();
        let src = b"/* /* nested */ DBG */";
        let expected = b"/* /* nested */ $$$ */".to_vec();
        assert_eq!(expected, replace(src, &mac).unwrap());
    }

    #[test]
    fn macro_after_string_still_replaced() {
        // Ensure exiting string state does not break subsequent macro
        // replacement (length preservation is the key invariant).
        let mac = dbg_macros();
        let src = b"char *s = \"hello\"; DBG x;";
        let expected = b"char *s = \"hello\"; $$$ x;".to_vec();
        assert_eq!(expected, replace(src, &mac).unwrap());
    }

    #[test]
    fn macro_after_line_comment_still_replaced() {
        let mac = dbg_macros();
        let src = b"// no DBG here\nDBG x;\n";
        let expected = b"// no DBG here\n$$$ x;\n".to_vec();
        assert_eq!(expected, replace(src, &mac).unwrap());
    }

    #[test]
    fn macro_after_block_comment_still_replaced() {
        let mac = dbg_macros();
        let src = b"/* no DBG */ DBG x;";
        let expected = b"/* no DBG */ $$$ x;".to_vec();
        assert_eq!(expected, replace(src, &mac).unwrap());
    }

    #[test]
    fn line_continuation_extends_line_comment() {
        // A backslash-newline at the end of a `//` comment continues
        // it onto the next physical line, so `DBG` is still inside the
        // comment and must not be replaced.
        let mac = dbg_macros();
        assert!(replace(b"// continues\\\nDBG still in comment\n", &mac).is_none());
    }

    #[test]
    fn macro_immediately_before_string_opener_still_replaced() {
        // `DBG"x"` — the macro identifier ends at `"`, which then opens
        // a string. Make sure DBG is masked even though no whitespace
        // separates it from the string.
        let mac = dbg_macros();
        let src = b"DBG\"x\";";
        let expected = b"$$$\"x\";".to_vec();
        assert_eq!(expected, replace(src, &mac).unwrap());
    }

    #[test]
    fn macro_immediately_before_line_comment_still_replaced() {
        // `DBG//tail` — identifier ends at `/`, which then opens a `//`
        // comment. The macro must be masked first.
        let mac = dbg_macros();
        let src = b"DBG//tail\n";
        let expected = b"$$$//tail\n".to_vec();
        assert_eq!(expected, replace(src, &mac).unwrap());
    }

    #[test]
    fn raw_string_prefix_not_masked_even_if_macro_named_r() {
        // If `R` itself is in the macro set, the `R"(...)"` prefix is
        // still a raw-string introducer and must not be masked — that
        // would corrupt the literal length and confuse the parser.
        let mut mac = HashSet::new();
        mac.insert("R".to_string());
        mac.insert("DBG".to_string());
        // Length is preserved; the `R` survives because we recognized
        // the prefix before applying macro substitution.
        assert!(replace(b"auto s = R\"(DBG)\";", &mac).is_none());
    }

    #[test]
    fn raw_string_exit_resumes_normal_lexing() {
        // After a raw string closes, the lexer must return to Normal
        // state so a trailing macro is still masked. This catches a
        // class of bugs where exiting the `RawString` state leaks into
        // subsequent code (e.g., staying in raw-string mode forever).
        let mut mac = HashSet::new();
        mac.insert("R".to_string());
        mac.insert("DBG".to_string());
        let src = b"auto s = R\"(DBG)\"; DBG x;";
        // The `R` prefix and the inner DBG are untouched; only the
        // trailing DBG outside the raw string is masked.
        let expected = b"auto s = R\"(DBG)\"; $$$ x;".to_vec();
        assert_eq!(expected, replace(src, &mac).unwrap());
    }
}
