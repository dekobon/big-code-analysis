//! Where perl starts and ends a `format`, read from the bytes.
//!
//! The grammar knows no format. perl reads the lines of one as picture
//! text, and behind a picture line that holds a field it reads one line of
//! values, which is code (`perlform`). A literal or a here-document that
//! opens in a value line can run over the dot that would end the format,
//! so the end is taken only where the bytes tell it.

use std::ops::Range;

use crate::documentation::budget::WorkBudget;
use crate::documentation::byte_scan as scan;

/// Where the body of a format starts in `after`, the bytes behind the
/// keyword `format`, if they open one as perl reads it: white space and
/// comments, an optional name, `=`, blanks, and the end of that line.
pub(super) fn format_body<B: WorkBudget>(
    after: &[u8],
    budget: &B,
) -> Result<Option<usize>, B::Error> {
    Ok(match format_equals(after, budget)? {
        Some(equals) => rest_of_line(&after[equals..], budget)?.map(|length| equals + length),
        None => None,
    })
}

/// The offset behind the `=` of a format's opener.
fn format_equals<B: WorkBudget>(after: &[u8], budget: &B) -> Result<Option<usize>, B::Error> {
    let named = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'\'');
    let name = space(after, budget)?;
    let rest = &after[name..];
    let word = name + scan::position(rest, |byte| !named(byte), budget)?.unwrap_or(rest.len());
    let equals = word + space(&after[word..], budget)?;
    Ok((after.get(equals) == Some(&b'=')).then_some(equals + 1))
}

/// The length of a line's rest, line end included, if it holds only blanks
/// and carriage returns, with or without a comment behind them.
fn rest_of_line<B: WorkBudget>(rest: &[u8], budget: &B) -> Result<Option<usize>, B::Error> {
    let blank = |byte| matches!(byte, b' ' | b'\t' | b'\r');
    let at = scan::position(rest, |byte| !blank(byte), budget)?.unwrap_or(rest.len());
    if !matches!(rest.get(at), Some(b'\n' | b'#')) {
        return Ok(None);
    }
    let end = scan::position(&rest[at..], |byte| byte == b'\n', budget)?;
    Ok(Some(end.map_or(rest.len(), |end| at + end + 1)))
}

/// The length of the white space and `#` comments at the start of `text`.
fn space<B: WorkBudget>(text: &[u8], budget: &B) -> Result<usize, B::Error> {
    let blank = |byte: u8| byte.is_ascii_whitespace() || byte == b'\x0b';
    let mut at = 0;
    loop {
        at += scan::position(&text[at..], |byte| !blank(byte), budget)?.unwrap_or(text.len() - at);
        if text.get(at) != Some(&b'#') {
            return Ok(at);
        }
        at += scan::position(&text[at..], |byte| byte == b'\n', budget)?.unwrap_or(text.len() - at);
    }
}

/// What one line of the body of a format is for perl.
enum Line {
    /// The line that ends the format.
    Last,
    /// Picture text without a field, a comment, or a line of values that
    /// opens nothing: picture text may follow.
    Picture,
    /// Picture text that holds `@` or `^`: perl reads the next line as
    /// values, which is code.
    Fields,
    /// A line of values that may open a literal or a here-document, or go
    /// on behind its line end.
    Unknown,
}

/// A line that starts with a dot, as far as it is read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dot {
    /// No dot at the start, or another byte behind it.
    No,
    /// A dot and at most one carriage return: it ends a format also where
    /// perl reads values.
    Bare,
    /// A dot, then blanks, tabs and carriage returns: it ends a format
    /// where picture text may stand.
    Blank,
}

/// What the bytes of one line show, noted byte by byte.
struct Seen {
    dot: Dot,
    /// The line holds `@` or `^`.
    fields: bool,
    /// A list of named variables, so far.
    list: bool,
    /// The byte before.
    before: u8,
}

impl Seen {
    fn note(&mut self, at: usize, byte: u8) {
        self.fields |= matches!(byte, b'@' | b'^');
        self.dot = match (self.dot, at, byte) {
            (dot, 0, _) => dot,
            (Dot::Bare, 1, b'\r') => Dot::Bare,
            (Dot::Bare | Dot::Blank, _, b' ' | b'\t' | b'\r') => Dot::Blank,
            _ => Dot::No,
        };
        self.list &= listed(self.before, byte);
        self.before = byte;
    }

    /// What the line is. `values`: perl reads values here, not picture
    /// text. `fed`: a line feed ends the line. `comment`: it starts with `#`.
    fn line(&self, values: bool, fed: bool, comment: bool) -> Line {
        if values {
            match (
                self.dot == Dot::Bare && fed,
                self.list && !sigil(self.before),
            ) {
                (true, _) => Line::Last,
                (false, true) => Line::Picture,
                (false, false) => Line::Unknown,
            }
        } else if self.dot != Dot::No {
            Line::Last
        } else if self.fields && !comment {
            Line::Fields
        } else {
            Line::Picture
        }
    }
}

fn sigil(byte: u8) -> bool {
    matches!(byte, b'$' | b'@' | b'%')
}

/// Whether `byte` may stand behind `before` in a list of named variables:
/// names behind `$`, `@`, `%` or `:`, and `,`, `;`, blanks, tabs and
/// carriage returns. Such a line opens nothing and ends at its line feed.
fn listed(before: u8, byte: u8) -> bool {
    let name = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    match byte {
        b':' => true,
        b'$' | b'@' | b'%' | b',' | b';' | b' ' | b'\t' | b'\r' => !sigil(before),
        _ => {
            name(byte)
                && (name(before) || before == b':' || (sigil(before) && !byte.is_ascii_digit()))
        }
    }
}

/// Reads one line of the body of a format, once, and returns its length
/// with its line feed. `values`: perl reads values here, not picture text.
///
/// Picture text: a dot and then only blanks, tabs and carriage returns ends
/// the format; a line that starts with `#` is a comment. Values: a dot
/// directly before the line end ends the format. Any other line of values
/// is read only if it is a list of named variables.
fn format_line<B: WorkBudget>(
    body: &[u8],
    values: bool,
    budget: &B,
) -> Result<(usize, Line), B::Error> {
    let mut seen = Seen {
        dot: if body.first() == Some(&b'.') {
            Dot::Bare
        } else {
            Dot::No
        },
        fields: false,
        list: true,
        before: b' ',
    };
    let comment = body.first() == Some(&b'#');
    for (at, &byte) in body.iter().enumerate() {
        scan::one(budget)?;
        if byte == b'\n' {
            return Ok((at + 1, seen.line(values, true, comment)));
        }
        seen.note(at, byte);
    }
    Ok((body.len(), seen.line(values, false, comment)))
}

/// The line that ends a format for perl, in the format's body, where perl's
/// rule tells it from the bytes. `None`: no line ends the format, or a line
/// of values is not read, so that the end is not known: a literal or a
/// here-document that opens there can run over a dot.
pub(super) fn format_end<B: WorkBudget>(
    body: &[u8],
    budget: &B,
) -> Result<Option<Range<usize>>, B::Error> {
    let mut start = 0;
    let mut values = false;
    while start < body.len() {
        let (length, line) = format_line(&body[start..], values, budget)?;
        match line {
            Line::Last => return Ok(Some(start..start + length)),
            Line::Unknown => return Ok(None),
            Line::Fields => values = true,
            Line::Picture => values = false,
        }
        start += length;
    }
    Ok(None)
}
