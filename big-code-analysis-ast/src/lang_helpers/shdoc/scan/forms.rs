//! The forms that the scan reads only where they close on their row, the
//! word behind `=~` and the delimiter word of a here-document.

use super::{
    Entry, Frame, Heredoc, LF, Mode, Place, Prev, Read, Scan, Word, breaks, forms_opened, name,
};
use crate::documentation::budget::WorkBudget;

/// What the byte before the one read can still combine with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Before {
    Other,
    Dollar,
    /// `<` or `>`: a `(` behind it opens a process substitution.
    Angle,
    /// `<&` or `>&`, possibly followed by blanks: `-` ends the token.
    Dup,
}

/// What one byte does to the innermost open form.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    /// A byte of a word.
    Word(Before),
    /// A byte that ends a word for the command reading.
    Break(Before),
    /// A backslash: it takes the byte behind it.
    Escape,
    /// `<<<`: two more bytes of the operator follow.
    Here,
    Open(Frame),
    Close,
    /// No rule reads the form.
    Halt,
}

/// The quoted string that `byte` opens where quotes nest. `plain`: in
/// `${ }` in a double-quoted string, where a single quote is a quote for
/// Bash and a byte for its POSIX mode.
fn quote(byte: u8, dollar: bool, plain: bool) -> Option<Frame> {
    match byte {
        b'\'' if plain => Some(Frame::Plain),
        b'\'' if dollar => Some(Frame::Ansi),
        b'\'' => Some(Frame::Single),
        b'"' => Some(Frame::Double),
        b'`' => Some(Frame::Back),
        _ => None,
    }
}

fn halt_if(halts: bool) -> Step {
    if halts {
        Step::Halt
    } else {
        Step::Word(Before::Other)
    }
}

impl Scan<'_> {
    /// One entry of the stack for a form that opens: one charge.
    fn push<B: WorkBudget>(
        &mut self,
        frame: Frame,
        quoted: bool,
        budget: &B,
    ) -> Result<(), B::Error> {
        budget.step()?;
        forms_opened::record();
        self.guards += usize::from(frame == Frame::Guarded);
        self.balanced += usize::from(frame == Frame::Balanced);
        self.stack.push(Entry {
            frame,
            word: !matches!(frame, Frame::Command | Frame::Guarded),
            quoted: match frame {
                Frame::Double => true,
                Frame::Brace | Frame::Bracket => quoted,
                _ => false,
            },
        });
        Ok(())
    }

    /// The innermost form closes with `byte`.
    fn pop(&mut self, byte: u8) {
        if let Some(closed) = self.stack.pop() {
            self.guards -= usize::from(closed.frame == Frame::Guarded);
            self.balanced -= usize::from(closed.frame == Frame::Balanced);
        }
        if let Some(outer) = self.stack.last_mut()
            && outer.frame.commands()
        {
            // Behind `)` a word starts for the command reading.
            outer.word = byte != b')';
        }
    }

    /// Reads the byte at `at` of the form or word that starts at `opener`
    /// and must end on its row. `None`: the scan stopped.
    fn on_row<B: WorkBudget>(
        &mut self,
        opener: usize,
        at: usize,
        budget: &B,
    ) -> Result<Option<u8>, B::Error> {
        match self.peek(at) {
            None => self.cut(opener),
            Some(LF) => self.halt(opener),
            Some(_) => return self.take(at, budget),
        };
        Ok(None)
    }

    /// Reads the form whose opening byte, at `at`, is read: to the byte
    /// that closes it on its row.
    pub(super) fn form<B: WorkBudget>(
        &mut self,
        at: usize,
        frame: Frame,
        budget: &B,
    ) -> Read<B::Error> {
        // bca: suppress(cognitive, halstead, nexits) -- one loop over one stack of open forms;
        // each exit is a place where the scan stops, or a charge.
        if self.unknown.is_some() {
            self.opened_tentative = true;
        }
        self.stack.clear();
        self.guards = 0;
        self.balanced = 0;
        self.push(frame, self.mode == Mode::Double, budget)?;
        let mut before = Before::Other;
        let mut next = at + 1;
        while let Some(&top) = self.stack.last() {
            let Some(byte) = self.on_row(at, next, budget)? else {
                return Ok(None);
            };
            let step = self.in_form(next, top, byte, before);
            let mut word = top.word;
            before = Before::Other;
            match step {
                Step::Halt => return Ok(self.halt(at)),
                Step::Word(then) => (word, before) = (true, then),
                Step::Break(then) => (word, before) = (false, then),
                Step::Escape | Step::Here => {
                    // The byte a backslash takes, or the rest of `<<<`.
                    let more = if step == Step::Here { 2 } else { 1 };
                    for ahead in 1..=more {
                        if self.on_row(at, next + ahead, budget)?.is_none() {
                            return Ok(None);
                        }
                    }
                    (word, next) = (step == Step::Escape, next + more);
                }
                Step::Open(frame) => word |= !frame.commands(),
                Step::Close => {}
            }
            next += 1;
            if let Some(entry) = self.stack.last_mut() {
                entry.word = word;
            }
            match step {
                Step::Open(frame) => self.push(frame, top.quoted, budget)?,
                Step::Close => self.pop(byte),
                _ => {}
            }
        }
        self.unit = (at..next, Place::Form);
        Ok(Some(next))
    }

    /// What the byte at `at` does to the innermost open form `top`.
    fn in_form(&self, at: usize, top: Entry, byte: u8, before: Before) -> Step {
        // bca: suppress(cyclomatic, halstead, nexits) -- one arm per open form and class of byte.
        let (dollar, guarded) = (before == Before::Dollar, self.guards > 0);
        if byte == b'#' && self.balanced > 0 {
            return Step::Halt;
        }
        if byte == top.frame.closer() {
            return Step::Close;
        }
        if byte == b'\\' {
            return match top.frame {
                Frame::Single => Step::Word(Before::Other),
                Frame::Plain | Frame::Bracket => Step::Halt,
                _ => Step::Escape,
            };
        }
        let open = match top.frame {
            Frame::Single | Frame::Ansi | Frame::Back => return Step::Word(Before::Other),
            // Bytes that the reader of `${ }` does not read as plain.
            Frame::Plain => return halt_if(b"\"`${}()<>".contains(&byte)),
            Frame::Bracket => {
                let parenthesis = guarded && matches!(byte, b'(' | b')');
                return halt_if(parenthesis || b"'\"$[`".contains(&byte));
            }
            Frame::Double if byte == b'`' => Some(Frame::Back),
            Frame::Double => None,
            Frame::Brace => quote(byte, dollar, top.quoted),
            _ => quote(byte, dollar, false),
        };
        match (open, top.frame, byte) {
            (Some(frame), ..) => Step::Open(frame),
            (_, _, b'$') if !dollar => Step::Word(Before::Dollar),
            (_, _, b'(' | b'{' | b'[') if dollar => Step::Open(self.dollar_frame(byte, at)),
            (_, Frame::Balanced, b'(') => Step::Open(Frame::Balanced),
            (_, Frame::Balanced | Frame::Double, _) => Step::Word(Before::Other),
            (_, Frame::Brace, _) => Self::in_braces(byte, before, guarded),
            _ => self.in_commands(at, top, byte, before),
        }
    }

    /// A byte of `${ }` that opens no string and no `$` form.
    fn in_braces(byte: u8, before: Before, guarded: bool) -> Step {
        match byte {
            b'{' => Step::Halt,
            b'(' if before == Before::Angle => Step::Open(Frame::Command),
            b'(' | b')' if guarded => Step::Halt,
            b'<' | b'>' if before != Before::Angle => Step::Word(Before::Angle),
            _ => Step::Word(Before::Other),
        }
    }

    /// A byte of a form that Bash's command reader reads for one of its
    /// readings, where the byte opens no string and no `$` form.
    fn in_commands(&self, at: usize, top: Entry, byte: u8, before: Before) -> Step {
        // bca: suppress(cyclomatic, halstead) -- one arm per class of byte of Bash's command reader.
        let subscript = top.frame == Frame::Subscript;
        match byte {
            b'&' if before == Before::Angle => Step::Break(Before::Dup),
            b' ' | b'\t' if before == Before::Dup => Step::Break(Before::Dup),
            b'-' if before == Before::Dup => Step::Break(Before::Other),
            b'[' if subscript => Step::Open(Frame::Subscript),
            b'(' if !subscript => Step::Open(top.frame),
            // In a subscript `<(` is a substitution; any other parenthesis
            // is a byte for one reading and a token for the other.
            b'(' if before == Before::Angle => Step::Open(Frame::Command),
            b'(' => Step::Open(Frame::Guarded),
            // `<` and `<<<` are operators; `<<` opens a here-document for
            // the command reading.
            b'<' => match (1..4)
                .take_while(|ahead| self.peek(at + ahead) == Some(b'<'))
                .count()
            {
                0 => Step::Break(Before::Angle),
                2 => Step::Here,
                _ => Step::Halt,
            },
            // A comment for the command reading: it hides the close.
            b'#' if !top.word => Step::Halt,
            b'>' if before != Before::Angle => Step::Break(Before::Angle),
            b' ' | b'\t' | b'|' | b'&' | b';' | b'>' | b')' => Step::Break(Before::Other),
            b'c' | b'e' if !top.word && !subscript && self.case_word(at) => Step::Halt,
            _ => Step::Word(Before::Other),
        }
    }

    /// Whether the word `case` or `esac` stands at `at`: Bash's reader of
    /// a command substitution then counts no parenthesis.
    fn case_word(&self, at: usize) -> bool {
        let word = self.code.get(at..(at + 4).min(self.end));
        matches!(word, Some(b"case" | b"esac")) && self.peek(at + 4).is_none_or(breaks)
    }

    /// What the byte at `at` of the word behind `=~` does. `named`: the
    /// bytes of the word before it start with a name.
    fn in_pattern(&self, at: usize, byte: u8, before: Before, named: bool) -> Step {
        let dollar = before == Before::Dollar;
        match byte {
            b'\\' => Step::Escape,
            // For the command reading `|` ends the word.
            b'|' => Step::Halt,
            b'(' | b'{' | b'[' if dollar => Step::Open(self.dollar_frame(byte, at)),
            b'(' if before == Before::Angle => Step::Open(Frame::Command),
            // A name, `=` and `(` are a compound assignment for the
            // command reading.
            b'(' if named && self.code[at - 1] == b'=' => Step::Halt,
            b'(' => Step::Open(Frame::Guarded),
            b'$' if !dollar => Step::Word(Before::Dollar),
            b'<' | b'>' => Step::Word(Before::Angle),
            _ => quote(byte, dollar, false).map_or(Step::Word(Before::Other), Step::Open),
        }
    }

    /// Reads the word behind `=~`, whose first byte, at `at`, is read:
    /// to the blank or operator that ends it for both readings of Bash.
    pub(super) fn pattern<B: WorkBudget>(
        &mut self,
        at: usize,
        first: u8,
        budget: &B,
    ) -> Read<B::Error> {
        // bca: suppress(cognitive, cyclomatic, halstead, nexits) -- one word read once for two
        // readings of Bash; each exit is a place where the scan stops, or a charge.
        if self.array {
            return Ok(self.halt(at));
        }
        let (mut next, mut byte) = (at, first);
        let (mut word, mut named, mut before) = (false, false, Before::Other);
        loop {
            let step = self.in_pattern(next, byte, before, word && named);
            before = Before::Other;
            next += 1;
            match step {
                Step::Open(frame) => {
                    let Some(end) = self.form(next - 1, frame, budget)? else {
                        return Ok(None);
                    };
                    if frame == Frame::Guarded && self.peek(end) == Some(b'#') {
                        return Ok(self.halt(end));
                    }
                    (next, word, named) = (end, frame != Frame::Guarded, false);
                }
                Step::Escape => {
                    if self.on_row(at, next, budget)?.is_none() {
                        return Ok(None);
                    }
                    (next, word, named) = (next + 1, true, false);
                }
                Step::Word(then) => {
                    named = if word { named } else { name(byte, true) };
                    (word, before) = (true, then);
                }
                _ => return Ok(self.halt(at)),
            }
            match self.peek(next) {
                None if self.end < self.code.len() => return Ok(self.cut(at)),
                None => break,
                Some(b'<' | b'>') if self.peek(next + 1) == Some(b'(') => {}
                Some(ahead) if breaks(ahead) && !matches!(ahead, b'(' | b'|') => break,
                Some(_) => {}
            }
            match self.take(next, budget)? {
                Some(read) => byte = read,
                None => return Ok(None),
            }
        }
        (self.word, self.line, self.prev) = (Word::Other, false, Prev::None);
        self.unit = (at..next, Place::Form);
        Ok(Some(next))
    }

    /// Reads a quoted part of the delimiter word that starts at `opener`,
    /// from the quote at `at` to the quote that closes it, and adds its
    /// bytes to `word`. The answer is the place of the closing quote.
    fn delimiter_quote<B: WorkBudget>(
        &mut self,
        opener: usize,
        at: usize,
        word: &mut Vec<u8>,
        budget: &B,
    ) -> Read<B::Error> {
        let quote = self.code[at];
        let mut next = at + 1;
        loop {
            let Some(byte) = self.on_row(opener, next, budget)? else {
                return Ok(None);
            };
            if byte == quote {
                return Ok(Some(next));
            }
            let expands = quote == b'"' && matches!(byte, b'\\' | b'$' | b'`');
            if expands || matches!(byte, 1 | 0x7f) {
                return Ok(self.halt(opener));
            }
            word.push(byte);
            next += 1;
        }
    }

    /// Reads the delimiter word of a here-document, whose first byte, at
    /// `at`, is read: its bytes after quote removal, and whether a part
    /// of it was quoted. The scan stops at a word whose quote removal
    /// needs a shell, and at the two bytes Bash keeps for its own quoting.
    pub(super) fn delimiter<B: WorkBudget>(
        &mut self,
        at: usize,
        first: u8,
        tabs: bool,
        budget: &B,
    ) -> Read<B::Error> {
        // bca: suppress(abc, cognitive, cyclomatic, halstead, nexits) -- one arm per spelling of
        // a delimiter word; each exit is a place where the scan stops, or a charge.
        let mut word = Vec::new();
        let (mut next, mut byte, mut quoted) = (at, first, false);
        if breaks(first) || first == b'#' {
            return Ok(self.halt(at));
        }
        loop {
            match byte {
                1 | 0x7f | b'`' => return Ok(self.halt(at)),
                b'$' if matches!(self.peek(next + 1), Some(b'(' | b'{' | b'[' | b'\'' | b'"')) => {
                    return Ok(self.halt(at));
                }
                b'\'' | b'"' => {
                    let Some(close) = self.delimiter_quote(at, next, &mut word, budget)? else {
                        return Ok(None);
                    };
                    (next, quoted) = (close, true);
                }
                b'\\' => match self.on_row(at, next + 1, budget)? {
                    None => return Ok(None),
                    Some(1 | 0x7f) => return Ok(self.halt(at)),
                    Some(taken) => {
                        word.push(taken);
                        (next, quoted) = (next + 1, true);
                    }
                },
                _ => word.push(byte),
            }
            next += 1;
            match self.peek(next) {
                None if self.end < self.code.len() => return Ok(self.cut(at)),
                Some(b'(') => return Ok(self.halt(at)),
                Some(ahead) if !breaks(ahead) => {}
                _ => break,
            }
            match self.take(next, budget)? {
                Some(read) => byte = read,
                None => return Ok(None),
            }
        }
        budget.step()?;
        forms_opened::record();
        let tabs = tabs && word.first() != Some(&b'\t');
        self.waiting.push_back(Heredoc { word, tabs, quoted });
        (self.word, self.line, self.prev) = (Word::None, false, Prev::None);
        self.unit = (at..next, Place::Form);
        Ok(Some(next))
    }
}
