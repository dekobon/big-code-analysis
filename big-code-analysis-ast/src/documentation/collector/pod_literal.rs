//! Where perl starts and ends a quote-like literal, read from the bytes.
//!
//! Where a literal starts and ends is a claim of the grammar. perl finds
//! the end of a literal before it looks at the text (`perlop`, "Gory details
//! of parsing quoted constructs"): a backslash hides the next byte; behind
//! an opening bracket the literal ends at the matching closing bracket, and
//! brackets of that kind nest; behind any other delimiter it ends at the
//! next occurrence that is not hidden. The reader here reads the spelling
//! of a literal node by that rule, each byte once, and says whether perl
//! ends the literal where the node ends.

use std::ops::Range;

use crate::documentation::SourceContext;
use crate::documentation::budget::WorkBudget;
use crate::documentation::byte_scan as scan;

/// A quote-like literal, as perl reads its spelling.
#[derive(Clone, Copy)]
pub(super) struct Quote {
    /// The operator before the first delimiter. `tr` also stands for `y`.
    word: &'static [u8],
    /// `s`, `tr` and `y` have two delimited parts.
    parts: usize,
    /// perl interpolates or runs the text, so it can open a here-document.
    evaluated: bool,
    /// ASCII letters may follow the last delimiter.
    flags: bool,
    /// What the bytes before the literal must show.
    start: Start,
}

/// Where perl starts a literal of a kind. An operator starts one wherever
/// perl reads the operator; the two delimiters that stand alone do not.
#[derive(Clone, Copy)]
enum Start {
    Anywhere,
    /// `'` behind a name is the old package separator, as in `Foo'bar`.
    NotInName,
    /// `/` is a division where perl expects an operator.
    Term,
}

impl Start {
    /// How many bytes before the literal are read.
    fn window(self) -> usize {
        match self {
            Self::Anywhere => 0,
            Self::NotInName => 1,
            Self::Term => 32,
        }
    }
}

/// What perl makes of a literal node.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reading {
    /// perl starts and ends the literal where the node does.
    Same,
    /// The same, and the text holds a here-document opener that perl
    /// evaluates.
    Opener,
    /// perl starts no literal here, or ends it elsewhere; or the node has
    /// an excluded byte, in it or in the bytes before it that are read.
    Different,
}

impl Quote {
    /// The literal that a node of this kind is, if it is one.
    pub(super) fn of(kind: &str) -> Option<Self> {
        let quote = |word: &'static [u8], parts, evaluated, flags| Self {
            word,
            parts,
            evaluated,
            flags,
            start: Start::Anywhere,
        };
        let alone = |evaluated, flags, start| Self {
            start,
            ..quote(b"", 1, evaluated, flags)
        };
        Some(match kind {
            "string_single_quoted" => alone(false, false, Start::NotInName),
            "string_double_quoted" | "backtick_quoted" => alone(true, false, Start::Anywhere),
            "pattern_matcher" => alone(true, true, Start::Term),
            "command_qx_quoted" => quote(b"qx", 1, true, false),
            "string_q_quoted" => quote(b"q", 1, false, false),
            "string_qq_quoted" => quote(b"qq", 1, true, false),
            "word_list_qw" => quote(b"qw", 1, false, false),
            "pattern_matcher_m" => quote(b"m", 1, true, true),
            "regex_pattern_qr" => quote(b"qr", 1, true, true),
            "substitution_pattern_s" => quote(b"s", 2, true, true),
            "transliteration_tr_or_y" => quote(b"tr", 2, false, true),
            _ => return None,
        })
    }

    /// What perl makes of the node with this `spelling`.
    pub(super) fn reading<B: WorkBudget>(
        self,
        source: &SourceContext<'_>,
        spelling: Range<usize>,
        budget: &B,
    ) -> Result<Reading, B::Error> {
        let code = source.bytes();
        let before = spelling.start.saturating_sub(self.start.window())..spelling.start;
        if !source.covers_with_budget(before.start..spelling.end, budget)? {
            return Ok(Reading::Different);
        }
        for _ in before.clone() {
            scan::one(budget)?;
        }
        let whole = before.start == 0;
        let before = &code[before];
        let starts = match self.start {
            Start::Anywhere => true,
            Start::NotInName => !before.last().is_some_and(|&byte| name(byte)),
            Start::Term => expects_term(before, whole),
        };
        let (ends, opener) = perl_reads(&code[spelling], self, budget)?;
        Ok(match (starts && ends, self.evaluated && opener) {
            (false, _) => Reading::Different,
            (true, true) => Reading::Opener,
            (true, false) => Reading::Same,
        })
    }
}

/// Whether `word` spells a quote-like operator.
pub(super) fn operator(word: &[u8]) -> bool {
    matches!(
        word,
        b"q" | b"qq" | b"qw" | b"qx" | b"m" | b"qr" | b"s" | b"tr" | b"y"
    )
}

fn name(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether perl expects a term behind `before`, the bytes before a `/`, so
/// that the `/` starts a pattern and is no division. `whole`: `before`
/// starts at the start of the input. This is taken only at the start of the
/// input; behind `(`, `,`, `=`, `~`, `!`, `&`, `|`, `?`, `:`, `;` or `{`
/// that is not the name of a variable such as `$?`; and behind a keyword
/// that takes a term. White space may stand between.
pub(super) fn expects_term(before: &[u8], whole: bool) -> bool {
    let white = |byte: &u8| byte.is_ascii_whitespace() || *byte == b'\x0b';
    let end = before.iter().rposition(|byte| !white(byte));
    let text = &before[..end.map_or(0, |at| at + 1)];
    let word = text.iter().rposition(|&byte| !name(byte));
    let (lead, word) = text.split_at(word.map_or(0, |at| at + 1));
    if word.is_empty() {
        behind_mark(lead, whole)
    } else {
        behind_keyword(lead, word, whole)
    }
}

/// Behind a byte that is no part of a name: whether perl expects a term.
fn behind_mark(text: &[u8], whole: bool) -> bool {
    let term = |mark: u8| {
        matches!(
            mark,
            b'(' | b',' | b'=' | b'~' | b'!' | b'&' | b'|' | b'?' | b':' | b';' | b'{'
        )
    };
    match *text {
        [] => whole,
        [mark] => term(mark) && whole,
        [.., sigil, mark] => term(mark) && !matches!(sigil, b'$' | b'@' | b'%'),
    }
}

/// Behind the name `word`, with `lead` before it: whether the name is a
/// keyword behind which perl expects a term, and no part of a longer name.
fn behind_keyword(lead: &[u8], word: &[u8], whole: bool) -> bool {
    let keyword = matches!(
        word,
        b"split"
            | b"grep"
            | b"map"
            | b"if"
            | b"unless"
            | b"while"
            | b"until"
            | b"and"
            | b"or"
            | b"not"
            | b"xor"
            | b"return"
    );
    match lead.last() {
        None => keyword && whole,
        Some(b'$' | b'@' | b'%' | b'&' | b'*' | b'>' | b':' | b'\'') => false,
        Some(_) => keyword,
    }
}

/// perl's reading of the spelling of a literal node: whether perl ends the
/// literal at the last byte of the spelling, and whether the text holds
/// `<<` and the start of a here-document tag. Each byte is read once.
pub(super) fn perl_reads<B: WorkBudget>(
    spelling: &[u8],
    quote: Quote,
    budget: &B,
) -> Result<(bool, bool), B::Error> {
    let mut reader = Reader {
        rest: spelling,
        budget,
        shift: Shift::None,
        opener: false,
    };
    let ends = reader.ends(quote)?;
    Ok((ends, reader.opener))
}

/// Where `<<` and the start of a here-document tag stand in a literal.
#[derive(Clone, Copy)]
enum Shift {
    None,
    /// One `<`.
    Half,
    /// `<<`.
    Whole,
    /// `<<~`.
    Tilde,
    /// Blanks or tabs behind one of the two.
    Blank,
    /// A backslash behind one of the two.
    Escape,
}

/// Reads the spelling of a literal forward, one charged byte at a time.
struct Reader<'t, 'b, B> {
    rest: &'t [u8],
    budget: &'b B,
    shift: Shift,
    opener: bool,
}

impl<B: WorkBudget> Reader<'_, '_, B> {
    fn next(&mut self) -> Result<Option<u8>, B::Error> {
        let Some((&byte, rest)) = self.rest.split_first() else {
            return Ok(None);
        };
        scan::one(self.budget)?;
        self.rest = rest;
        Ok(Some(byte))
    }

    /// Notes a byte of the text between the delimiters, for the search for
    /// `<<` and a tag: behind an optional `~`, a quote behind optional
    /// blanks and tabs, or a letter, a digit or `_` behind an optional
    /// backslash.
    fn text(&mut self, byte: u8) {
        let quote = matches!(byte, b'"' | b'\'' | b'`');
        let word = name(byte);
        self.opener |= match self.shift {
            Shift::Whole | Shift::Tilde => quote || word,
            Shift::Blank => quote,
            Shift::Escape => word,
            Shift::None | Shift::Half => false,
        };
        self.shift = match (self.shift, byte) {
            (Shift::Half | Shift::Whole, b'<') => Shift::Whole,
            (_, b'<') => Shift::Half,
            (Shift::Whole, b'~') => Shift::Tilde,
            (Shift::Whole | Shift::Tilde | Shift::Blank, b' ' | b'\t') => Shift::Blank,
            (Shift::Whole | Shift::Tilde, b'\\') => Shift::Escape,
            _ => Shift::None,
        };
    }

    /// Whether perl ends the literal at the last byte of the spelling.
    fn ends(&mut self, quote: Quote) -> Result<bool, B::Error> {
        let named = !quote.word.is_empty();
        Ok(match self.opening(quote.word)? {
            Some(open) => {
                self.part(open, named && open == b'=')?
                    && (quote.parts == 1 || self.second(open)?)
                    && self.flags(quote.flags)?
            }
            None => false,
        })
    }

    /// Reads the operator and the delimiter behind it. `None` where the
    /// operator is not spelled here, where no delimiter follows, and where
    /// perl reads the word as a name: before a closing bracket, as in
    /// `$h{y}`, `(y)` and `<y>`.
    fn opening(&mut self, word: &[u8]) -> Result<Option<u8>, B::Error> {
        let Some((&first, mut rest)) = word.split_first() else {
            return self.next();
        };
        let read = self.next()?;
        let mut spelled = read == Some(first);
        if first == b't' && read == Some(b'y') {
            // `y` is the other spelling of `tr`.
            (spelled, rest) = (true, &rest[..0]);
        }
        for &letter in rest {
            spelled &= self.next()? == Some(letter);
        }
        let open = self.delimiter()?;
        Ok(open.filter(|open| spelled && !matches!(open, b'}' | b')' | b']' | b'>')))
    }

    /// Reads the second part of `s`, `tr` or `y`. Behind a bracketed first
    /// part it has a delimiter of its own; otherwise the delimiter that
    /// ended the first part starts it.
    fn second(&mut self, open: u8) -> Result<bool, B::Error> {
        let second = if closing(open) == open {
            Some(open)
        } else {
            self.delimiter()?
        };
        match second {
            Some(second) => self.part(second, false),
            None => Ok(false),
        }
    }

    /// Whether nothing follows the last delimiter but flags, where the
    /// literal takes flags.
    fn flags(&mut self, taken: bool) -> Result<bool, B::Error> {
        while let Some(flag) = self.next()? {
            if !(taken && flag.is_ascii_alphabetic()) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The delimiter behind an operator or behind a bracketed first part:
    /// perl skips white space and comments before it. A `#` directly behind
    /// the operator or the part is the delimiter itself.
    fn delimiter(&mut self) -> Result<Option<u8>, B::Error> {
        let mut byte = self.next()?;
        let mut direct = true;
        loop {
            match byte {
                Some(b'#') if !direct => {
                    while byte.is_some_and(|byte| byte != b'\n') {
                        byte = self.next()?;
                    }
                }
                Some(white) if white.is_ascii_whitespace() || white == b'\x0b' => {
                    byte = self.next()?;
                }
                _ => return Ok(byte),
            }
            direct = false;
        }
    }

    /// Reads one part behind its opening delimiter, and notes its text.
    /// Returns whether the part ends in the spelling. With `arrow`, a `>`
    /// directly behind the delimiter makes it the `=>` behind a name.
    fn part(&mut self, open: u8, arrow: bool) -> Result<bool, B::Error> {
        let close = closing(open);
        let mut depth = 1_usize;
        let mut first = arrow;
        self.shift = Shift::None;
        while let Some(byte) = self.next()? {
            if byte == close {
                depth -= 1;
            } else if byte == open {
                depth += 1;
            }
            if depth == 0 {
                return Ok(true);
            }
            if first && byte == b'>' {
                return Ok(false);
            }
            first = false;
            self.text(byte);
            if byte == b'\\'
                && let Some(hidden) = self.next()?
            {
                self.text(hidden);
            }
        }
        Ok(false)
    }
}

/// The delimiter that closes a part: the closing bracket for an opening
/// one, and the delimiter itself for any other.
fn closing(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        other => other,
    }
}
