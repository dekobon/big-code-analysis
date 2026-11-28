//! One forward scan of a Bash source by Bash's quoting rules. For the
//! byte a node of the grammar starts at, it says what Bash reads there: a
//! command, a comment, a quoted string, the text of a here-document, or a
//! form that the scan read on its row. Where no rule below reads a byte,
//! the scan stops, and nothing at or behind that place is read.
//!
//! The rules are those of Bash's lexer. Over rows the scan reads `'…'`,
//! `"…"`, `$'…'`, `$"…"`, back quotes and here-document texts. Every
//! other form that can hold a line end is read only where it closes on
//! its row. Bash decides some forms by the state of its parser, which
//! the scan does not know (`((`, a subscript, an extended pattern, the
//! word after `=~`): those are read only where the form and the plain
//! commands that the same bytes would be for Bash leave the same state.

mod forms;

use std::collections::VecDeque;
use std::ops::Range;

use crate::documentation::UnresolvedReason;
use crate::documentation::budget::WorkBudget;

// The work of the scan: the bytes it read, and the entries it pushed on
// its stack of open forms and on its queue of here-documents.
crate::observation::counter!(bytes_scanned, cfg(test));
crate::observation::counter!(forms_opened, cfg(test));

const LF: u8 = b'\n';

fn blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

/// A byte that ends a word for Bash.
fn breaks(byte: u8) -> bool {
    blank(byte) || matches!(byte, LF | b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>')
}

fn name(byte: u8, first: bool) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic() || (!first && byte.is_ascii_digit())
}

/// What the scan reads at a byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// At command level. `word`: a word starts at the byte. `line`: only
    /// blanks stand before it on its line.
    Code { word: bool, line: bool },
    /// The `#` of a comment that the scan read to `end`.
    Comment { end: usize, line: bool },
    /// Inside a comment, a quoted string or the text of a here-document.
    Text,
    /// Inside a form that the scan read on its row, or inside back quotes.
    Form,
    /// Behind an excluded byte, before the state is known again.
    Unknown,
    /// At or behind the place where the scan stopped.
    Behind(UnresolvedReason),
}

/// How far the word at command level is read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Word {
    /// No word is being read: one may start.
    None,
    /// A word that is a name so far: `[` behind it opens a subscript.
    Name,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    Single,
    Ansi,
    Double,
    /// Back quotes, and whether they stand inside a double-quoted string.
    Back(bool),
    Text,
}

/// What the last byte at command level can still combine with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    None,
    Dollar,
    Less,
    LessLess,
    Greater,
    Amp,
    AmpGreater,
    Bar,
    Semi,
    SemiSemi,
    /// `(` read as an operator: a second one may open `((`.
    Open,
    /// `=` behind a byte of its word: `(` opens a compound assignment.
    Equal,
    /// One of `@ ? * + !`: `(` opens an extended pattern.
    Pattern,
    /// `=` where a word starts, then `=~`, then the blanks behind it.
    EqualFirst,
    Tilde,
    Regexp,
    /// `<&` or `>&`: a `-` behind it is a token of its own.
    Dup,
    /// The `)` of an extended pattern or of a compound assignment: the
    /// word goes on for one reading of Bash, a word starts for the other.
    Shut,
    /// `<<` or `<<-`: the delimiter word follows.
    Operator(bool),
}

/// A form that must close on its row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// `$( )`, `<( )`, `>( )`: Bash's reader of a command substitution.
    Command,
    /// `$(( ))`: parentheses and quotes only.
    Balanced,
    /// A parenthesis that is a form for one reading of Bash and commands
    /// for another.
    Guarded,
    Subscript,
    Brace,
    Bracket,
    Single,
    /// `'…'` in `${ }` in a double-quoted string: a quoted string for
    /// Bash and plain bytes for its POSIX mode.
    Plain,
    Ansi,
    Double,
    Back,
}

impl Frame {
    /// The byte that closes the form.
    fn closer(self) -> u8 {
        match self {
            Self::Command | Self::Balanced | Self::Guarded => b')',
            Self::Subscript | Self::Bracket => b']',
            Self::Brace => b'}',
            Self::Single | Self::Plain | Self::Ansi => b'\'',
            Self::Double => b'"',
            Self::Back => b'`',
        }
    }

    /// Whether Bash's command reader reads the bytes of the form for one
    /// of its readings: where a word starts matters in it.
    fn commands(self) -> bool {
        matches!(self, Self::Command | Self::Guarded | Self::Subscript)
    }
}

/// An open form: whether a word is being read in it, and whether it
/// stands in a double-quoted string as Bash's reader of `${ }` sees it.
#[derive(Clone, Copy)]
struct Entry {
    frame: Frame,
    word: bool,
    quoted: bool,
}

struct Heredoc {
    word: Vec<u8>,
    tabs: bool,
    quoted: bool,
}

/// The text of a here-document, read a byte at a time: how much of the
/// delimiter the bytes of the line match so far.
struct Text {
    heredoc: Heredoc,
    matched: Option<usize>,
    start: bool,
    slash: bool,
}

impl Text {
    fn new(heredoc: Heredoc) -> Self {
        Self {
            heredoc,
            matched: Some(0),
            start: true,
            slash: false,
        }
    }

    fn feed(&mut self, byte: u8) {
        if self.start && self.heredoc.tabs && byte == b'\t' {
            return;
        }
        self.start = false;
        self.matched = self
            .matched
            .filter(|at| self.heredoc.word.get(*at) == Some(&byte))
            .map(|at| at + 1);
    }
}

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Scan<'s> {
    code: &'s [u8],
    ranges: Vec<Range<usize>>,
    range: usize,
    /// The next byte to read, and the end of the included bytes it lies in.
    at: usize,
    end: usize,
    /// Behind an excluded byte the state is unknown until an unescaped
    /// line feed that the post-gap reading reaches at command level:
    /// the run of backslashes since the gap and whether a byte precedes it.
    unknown: Option<(usize, usize, bool)>,
    opened_tentative: bool,
    mode: Mode,
    word: Word,
    /// A compound assignment is open: `[` where a word starts opens a
    /// subscript, and its `)` ends more than a subshell.
    array: bool,
    dollar: bool,
    line: bool,
    prev: Prev,
    waiting: VecDeque<Heredoc>,
    text: Option<Text>,
    /// The open forms of the form being read on its row, and how many of
    /// them are guarded parentheses.
    stack: Vec<Entry>,
    guards: usize,
    balanced: usize,
    /// The last thing read that is more than one byte, and what a byte
    /// inside it is.
    unit: (Range<usize>, Place),
    stop: Option<(usize, UnresolvedReason)>,
}

type Read<E> = Result<Option<usize>, E>;

impl<'s> Scan<'s> {
    /// `ranges` are the included bytes of `code`, in order, without gaps
    /// inside a range.
    pub(crate) fn new(code: &'s [u8], ranges: Vec<Range<usize>>) -> Self {
        let first = ranges.first().cloned().unwrap_or(code.len()..code.len());
        Self {
            code,
            ranges,
            range: 0,
            at: first.start,
            end: first.end,
            unknown: (first.start > 0 || first.is_empty() && !code.is_empty())
                .then_some((0, 0, false)),
            opened_tentative: false,
            mode: Mode::Code,
            word: Word::None,
            array: false,
            dollar: false,
            line: true,
            prev: Prev::None,
            waiting: VecDeque::new(),
            text: None,
            stack: Vec::new(),
            guards: 0,
            balanced: 0,
            unit: (0..0, Place::Text),
            stop: None,
        }
    }

    /// Reads every byte before `to`.
    pub(crate) fn advance<B: WorkBudget>(&mut self, to: usize, budget: &B) -> Result<(), B::Error> {
        while self.at < to && self.at < self.end && self.stop.is_none() {
            self.step(budget)?;
        }
        Ok(())
    }

    /// Reads the thing that starts at `at`, if the scan stands there.
    pub(crate) fn read<B: WorkBudget>(&mut self, at: usize, budget: &B) -> Result<(), B::Error> {
        self.advance(at, budget)?;
        if self.at == at && self.at < self.end && self.stop.is_none() {
            self.step(budget)?;
        }
        Ok(())
    }

    /// What the scan reads at `at`. `at` is not before the byte of the
    /// last answer: the scan does not go back.
    pub(crate) fn place(&self, at: usize) -> Place {
        match (self.stop, self.unknown) {
            (Some((stop, reason)), _) if at >= stop => Place::Behind(reason),
            (_, Some((from, ..))) if at >= from => Place::Unknown,
            _ if at < self.at && self.unit.0.contains(&at) => match self.unit.1 {
                Place::Comment { .. } if at != self.unit.0.start => Place::Text,
                inside => inside,
            },
            _ => match (self.mode, self.prev) {
                (Mode::Code, Prev::Operator(_) | Prev::Tilde | Prev::Regexp)
                | (Mode::Back(_), _) => Place::Form,
                (Mode::Code, _) => Place::Code {
                    word: self.word == Word::None,
                    line: self.line,
                },
                _ => Place::Text,
            },
        }
    }

    /// Whether the scan, with every byte before `at` read, stands inside
    /// a string, a text or a form at `at`.
    pub(crate) fn inside(&self, at: usize) -> bool {
        self.unknown.is_none() && (self.at != at || self.mode != Mode::Code)
    }

    pub(crate) fn stopped(&self) -> Option<UnresolvedReason> {
        self.stop.map(|(_, reason)| reason)
    }

    /// Stops the scan at `at`, where the grammar could not read the
    /// source: what it reads from there on is no ground for a rule.
    pub(crate) fn stop_at(&mut self, at: usize) {
        self.stop
            .get_or_insert((at, UnresolvedReason::GrammarBoundary));
    }

    fn step<B: WorkBudget>(&mut self, budget: &B) -> Result<(), B::Error> {
        let at = self.at;
        let next = match (self.unknown.is_some(), self.mode) {
            (true, _) => self.resume(at, budget)?,
            (_, Mode::Code) => self.command(at, budget)?,
            (_, Mode::Text) => self.text_byte(at, budget)?,
            (_, Mode::Double) => self.double(at, budget)?,
            _ => self.literal(at, budget)?,
        };
        if let Some(next) = next {
            self.at = next.min(self.end);
            if self.at == self.end {
                self.leave_range();
            }
        }
        Ok(())
    }

    /// The included bytes end at `self.end`. If more follow behind
    /// excluded ones, the state is unknown there; if something is open,
    /// it is never known again.
    fn leave_range(&mut self) {
        if self.end == self.code.len() {
            return;
        }
        let waits = matches!(self.prev, Prev::LessLess | Prev::Operator(_));
        if self.unknown.is_none()
            && (self.mode != Mode::Code || !self.waiting.is_empty() || self.array || waits)
        {
            self.stop = Some((self.end, UnresolvedReason::ExcludedGap));
            return;
        }
        self.unknown = Some((self.unknown.map_or(self.end, |(from, ..)| from), 0, false));
        self.opened_tentative = false;
        (self.mode, self.word, self.prev) = (Mode::Code, Word::None, Prev::None);
        (self.array, self.dollar, self.line) = (false, false, true);
        self.range += 1;
        let next = self.ranges.get(self.range).cloned();
        let next = next.unwrap_or(self.code.len()..self.code.len());
        (self.at, self.end) = (next.start, next.end);
    }

    /// Tentatively reads the included bytes after a gap as commands.
    /// A construct opened in those bytes prevents resumption at its
    /// first line feed. Only a proven, unescaped line feed at command level can
    /// restore the state; an open construct there stops with ExcludedGap.
    fn resume<B: WorkBudget>(&mut self, at: usize, budget: &B) -> Read<B::Error> {
        // bca: suppress(abc, cognitive, cyclomatic, halstead) -- recovery must check every
        // construct opened while tentatively reading the gap before accepting a line feed.
        let Some((from, slashes, seen)) = self.unknown else {
            return Ok(Some(at + 1));
        };
        self.unknown = None;
        let byte = self.code[at];
        if !seen && byte == b'\\' {
            self.stop = Some((at, UnresolvedReason::ExcludedGap));
            return Ok(None);
        }
        let in_comment = matches!(self.unit.1, Place::Comment { end, .. } if at < end);
        let was_open = self.mode != Mode::Code
            || !self.waiting.is_empty()
            || self.array
            || !matches!(self.prev, Prev::None)
            || !self.stack.is_empty();
        self.unknown = Some((from, slashes, seen));
        let next = self.command(at, budget)?;
        self.unknown = None;
        self.opened_tentative |= self.mode != Mode::Code
            || !self.waiting.is_empty()
            || self.array
            || !self.stack.is_empty()
            || matches!(self.prev, Prev::LessLess | Prev::Operator(_));
        if self.stop.is_some() || next.is_none() || self.opened_tentative {
            self.stop = Some((at, UnresolvedReason::ExcludedGap));
            return Ok(None);
        }
        if byte == LF && seen && slashes.is_multiple_of(2) {
            if was_open
                || self.mode != Mode::Code
                || !self.waiting.is_empty()
                || self.array
                || !matches!(self.prev, Prev::None)
                || !self.stack.is_empty()
            {
                self.stop = Some((at, UnresolvedReason::ExcludedGap));
                return Ok(None);
            }
            self.unit = (from..at + 1, Place::Unknown);
            self.opened_tentative = false;
        } else {
            let (slashes, seen) = if in_comment {
                (0, seen)
            } else if byte == b'\\' {
                (slashes + 1, seen)
            } else {
                (0, true)
            };
            self.unknown = Some((from, slashes, seen));
        }
        Ok(next)
    }

    /// Reads the byte at `at`: one charge. A NUL byte is read by no rule.
    fn take<B: WorkBudget>(&mut self, at: usize, budget: &B) -> Result<Option<u8>, B::Error> {
        budget.bytes(1)?;
        bytes_scanned::record();
        let byte = self.code[at];
        if byte == 0 {
            self.stop = Some((at, UnresolvedReason::GrammarBoundary));
            return Ok(None);
        }
        Ok(Some(byte))
    }

    /// Looks at the byte at `at` without reading it, if it is included.
    fn peek(&self, at: usize) -> Option<u8> {
        (at < self.end).then(|| self.code[at])
    }

    fn halt(&mut self, at: usize) -> Option<usize> {
        self.stop = Some((at, UnresolvedReason::GrammarBoundary));
        None
    }

    /// A thing that must end on its row met the end of the included
    /// bytes: behind excluded bytes nobody knows whether it ends.
    fn cut(&mut self, opener: usize) -> Option<usize> {
        if self.end < self.code.len() {
            self.stop = Some((self.end, UnresolvedReason::ExcludedGap));
            None
        } else {
            self.halt(opener)
        }
    }

    /// A backslash at `at` and the byte it takes.
    fn escaped<B: WorkBudget>(&mut self, at: usize, inside: Place, budget: &B) -> Read<B::Error> {
        if at + 1 < self.end && self.take(at + 1, budget)?.is_none() {
            return Ok(None);
        }
        self.unit = (at..at + 2, inside);
        Ok(Some(at + 2))
    }

    /// One byte of `'…'`, of `$'…'` or of back quotes: nothing nests.
    fn literal<B: WorkBudget>(&mut self, at: usize, budget: &B) -> Read<B::Error> {
        let Some(byte) = self.take(at, budget)? else {
            return Ok(None);
        };
        match (self.mode, byte) {
            (Mode::Back(true), b'`') => self.mode = Mode::Double,
            (Mode::Single | Mode::Ansi, b'\'') | (Mode::Back(false), b'`') => {
                self.mode = Mode::Code;
            }
            (Mode::Back(_), b'\\') => return self.escaped(at, Place::Form, budget),
            (Mode::Ansi, b'\\') => return self.escaped(at, Place::Text, budget),
            _ => {}
        }
        Ok(Some(at + 1))
    }

    /// One byte of a double-quoted string.
    fn double<B: WorkBudget>(&mut self, at: usize, budget: &B) -> Read<B::Error> {
        let Some(byte) = self.take(at, budget)? else {
            return Ok(None);
        };
        let dollar = std::mem::take(&mut self.dollar);
        match byte {
            b'"' => self.mode = Mode::Code,
            b'`' => self.mode = Mode::Back(true),
            b'$' => self.dollar = !dollar,
            b'\\' => {
                // A backslash and line feed are no bytes for Bash: `$` and
                // `(` around them are `$(`.
                self.dollar = dollar && self.peek(at + 1) == Some(LF);
                return self.escaped(at, Place::Text, budget);
            }
            b'(' | b'{' | b'[' if dollar => {
                let frame = self.dollar_frame(byte, at);
                return self.form(at, frame, budget);
            }
            _ => {}
        }
        Ok(Some(at + 1))
    }

    fn dollar_frame(&self, byte: u8, at: usize) -> Frame {
        match byte {
            b'(' if self.peek(at + 1) == Some(b'(') => Frame::Balanced,
            b'(' => Frame::Command,
            b'{' => Frame::Brace,
            _ => Frame::Bracket,
        }
    }

    /// One thing at command level.
    fn command<B: WorkBudget>(&mut self, at: usize, budget: &B) -> Read<B::Error> {
        let Some(byte) = self.take(at, budget)? else {
            return Ok(None);
        };
        if byte == b'\\' && self.peek(at + 1) == Some(LF) {
            // The pair is no byte for Bash: what stands around it still
            // combines, and the row below goes on with the line. It opens
            // a line of its own only if blanks alone stand before the pair.
            return self.escaped(at, Place::Text, budget);
        }
        if self.prev == Prev::LessLess && !matches!(byte, b'<' | b'-') {
            // `<<` is the whole operator: the byte is the first behind it.
            self.prev = Prev::Operator(false);
        }
        self.combined(at, byte, budget)
    }

    /// The byte at `at` behind the byte it may combine with: the next
    /// byte of an operator, the opener of a form, or a byte as any other.
    fn combined<B: WorkBudget>(&mut self, at: usize, byte: u8, budget: &B) -> Read<B::Error> {
        // bca: suppress(halstead) -- Bash's operators of two and three bytes, one arm per pair.
        self.prev = match (self.prev, byte) {
            (Prev::Less, b'<') => Prev::LessLess,
            (Prev::LessLess, b'-') => Prev::Operator(true),
            (Prev::Amp, b'>') => Prev::AmpGreater,
            (Prev::Semi, b';') => Prev::SemiSemi,
            (Prev::EqualFirst, b'~') => Prev::Tilde,
            (Prev::Less | Prev::Greater, b'&') | (Prev::Dup, b' ' | b'\t') => Prev::Dup,
            (Prev::Operator(tabs), b' ' | b'\t') => Prev::Operator(tabs),
            // The last byte of an operator, or of `$$`.
            (Prev::Less | Prev::Greater | Prev::AmpGreater, b'>')
            | (Prev::LessLess, b'<')
            | (Prev::Greater | Prev::Bar, b'|')
            | (Prev::Amp | Prev::Bar | Prev::Semi | Prev::SemiSemi, b'&')
            | (Prev::Dollar, b'$') => Prev::None,
            (Prev::Tilde | Prev::Regexp, b' ' | b'\t') => {
                self.word = Word::None;
                Prev::Regexp
            }
            (Prev::Dup, b'-') => {
                self.word = Word::None;
                Prev::None
            }
            (Prev::Dollar, b'\'' | b'"') => {
                self.mode = if byte == b'"' {
                    Mode::Double
                } else {
                    Mode::Ansi
                };
                Prev::None
            }
            (Prev::Equal, b'(') => {
                (self.array, self.word) = (true, Word::None);
                Prev::None
            }
            _ => return self.opened(at, byte, budget),
        };
        Ok(Some(at + 1))
    }

    /// The byte at `at` where it opens a form behind the byte before it,
    /// or stands where no rule reads it; else a byte as any other.
    fn opened<B: WorkBudget>(&mut self, at: usize, byte: u8, budget: &B) -> Read<B::Error> {
        match (std::mem::replace(&mut self.prev, Prev::None), byte) {
            (Prev::Shut, b'#') => Ok(self.halt(at)),
            (Prev::Operator(tabs), _) => self.delimiter(at, byte, tabs, budget),
            (Prev::Tilde, b'(' | b'|') => self.pattern(at, byte, budget),
            (Prev::Regexp, _) if !matches!(byte, LF | b'#' | b';' | b'&' | b')' | b'<' | b'>') => {
                self.pattern(at, byte, budget)
            }
            (Prev::Less | Prev::Greater, b'(') => self.word_form(at, Frame::Command, budget),
            (Prev::Dollar, b'(' | b'{' | b'[') => {
                let frame = self.dollar_frame(byte, at);
                self.word_form(at, frame, budget)
            }
            (Prev::Open, b'(') => self.arithmetic(at, budget),
            (Prev::Pattern, b'(') => {
                let end = self.form(at, Frame::Guarded, budget)?;
                (self.word, self.prev) = (Word::Other, Prev::Shut);
                Ok(end)
            }
            _ => self.plain(at, byte, budget),
        }
    }

    /// The byte at `at`, behind nothing it combines with.
    fn plain<B: WorkBudget>(&mut self, at: usize, byte: u8, budget: &B) -> Read<B::Error> {
        // bca: suppress(cyclomatic, halstead) -- one arm per class of byte at command level.
        let (word, line) = (self.word, self.line);
        (self.word, self.line) = (Word::None, false);
        match byte {
            LF => match self.waiting.pop_front() {
                Some(heredoc) => (self.mode, self.text) = (Mode::Text, Some(Text::new(heredoc))),
                None => self.line = true,
            },
            b' ' | b'\t' => self.line = line,
            b'#' if word == Word::None => return self.comment(at, line, budget),
            b'\\' => {
                self.word = Word::Other;
                return self.escaped(at, Place::Text, budget);
            }
            b'<' => self.prev = Prev::Less,
            b'>' => self.prev = Prev::Greater,
            b'&' => self.prev = Prev::Amp,
            b'|' => self.prev = Prev::Bar,
            b';' => self.prev = Prev::Semi,
            b'(' => self.prev = Prev::Open,
            b')' if self.array => (self.array, self.prev) = (false, Prev::Shut),
            b')' => {}
            b'\'' | b'"' | b'`' => {
                self.word = Word::Other;
                self.mode = match byte {
                    b'\'' => Mode::Single,
                    b'"' => Mode::Double,
                    _ => Mode::Back(false),
                };
            }
            b'[' if word == Word::Name || (word == Word::None && self.array) => {
                return self.word_form(at, Frame::Subscript, budget);
            }
            _ => {
                self.word = match word {
                    Word::None if name(byte, true) => Word::Name,
                    Word::Name if name(byte, false) => Word::Name,
                    _ => Word::Other,
                };
                self.prev = match byte {
                    b'=' if word == Word::None => Prev::EqualFirst,
                    b'=' => Prev::Equal,
                    b'$' => Prev::Dollar,
                    b'@' | b'?' | b'*' | b'+' | b'!' => Prev::Pattern,
                    _ => Prev::None,
                };
            }
        }
        Ok(Some(at + 1))
    }

    /// A form that is part of a word.
    fn word_form<B: WorkBudget>(&mut self, at: usize, frame: Frame, budget: &B) -> Read<B::Error> {
        let end = self.form(at, frame, budget)?;
        (self.word, self.line, self.prev) = (Word::Other, false, Prev::None);
        Ok(end)
    }

    /// A comment from the `#` at `at` to the end of its row.
    fn comment<B: WorkBudget>(&mut self, at: usize, line: bool, budget: &B) -> Read<B::Error> {
        let mut end = at + 1;
        while self.peek(end).is_some_and(|byte| byte != LF) {
            if self.take(end, budget)?.is_none() {
                return Ok(None);
            }
            end += 1;
        }
        self.unit = (at..end, Place::Comment { end, line });
        Ok(Some(end))
    }

    /// `((`: the second parenthesis is at `at`. Read only where `)`
    /// follows its pair. Where none does, Bash reads the bytes again as
    /// nested subshells, and no rule here says what its reader holds then.
    fn arithmetic<B: WorkBudget>(&mut self, at: usize, budget: &B) -> Read<B::Error> {
        let Some(end) = self.form(at, Frame::Guarded, budget)? else {
            return Ok(None);
        };
        if self.peek(end) != Some(b')') {
            return Ok(self.halt(at));
        }
        if self.take(end, budget)?.is_none() {
            return Ok(None);
        }
        self.unit.0.end = end + 1;
        (self.word, self.prev) = (Word::None, Prev::None);
        Ok(Some(end + 1))
    }

    /// One byte of the text of a here-document. For an unquoted
    /// delimiter a backslash and line feed are no bytes of the line.
    fn text_byte<B: WorkBudget>(&mut self, at: usize, budget: &B) -> Read<B::Error> {
        let Some(byte) = self.take(at, budget)? else {
            return Ok(None);
        };
        let Some(text) = self.text.as_mut() else {
            return Ok(Some(at + 1));
        };
        let slash = std::mem::take(&mut text.slash);
        match (slash, byte) {
            (true, LF) => {}
            (true, _) => {
                text.feed(b'\\');
                text.feed(byte);
            }
            (false, b'\\') if !text.heredoc.quoted => text.slash = true,
            (false, LF) if text.matched != Some(text.heredoc.word.len()) => {
                (text.matched, text.start) = (Some(0), true);
            }
            (false, LF) => {
                self.text = self.waiting.pop_front().map(Text::new);
                if self.text.is_none() {
                    (self.mode, self.prev) = (Mode::Code, Prev::None);
                    (self.word, self.line) = (Word::None, true);
                }
            }
            (false, _) => text.feed(byte),
        }
        Ok(Some(at + 1))
    }
}
