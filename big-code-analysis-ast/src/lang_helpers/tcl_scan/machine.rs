//! Tcl's rules for where a command and a word end, as a state machine over
//! the bytes of a script: the state at one byte, and what the byte does to
//! the list of commands and to the brackets and array indexes around it.
//!
//! The machine keeps no list and no stack. [`super::CommandScan`] feeds it
//! the bytes, keeps both, and enters and leaves a bracket's script for it.

const END_OF_FILE: u8 = 0x1a;

// The three kinds of text in which Tcl substitutes. Tcl reads all of them
// with one loop; they differ in the byte that ends them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Text {
    // A word with no quote and no brace around it. White space, a line end
    // or `;` ends it, and so does `]` in a bracket.
    Bare,
    // Between two `"`.
    Quoted,
    // The index of an array element, after `$name(`. Only `)` ends it.
    Index,
}

// Where the scan is in a variable reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Var {
    // Directly after `$`.
    Dollar,
    Name,
    // After one `:`. Only two or more are part of a name.
    Colon,
    Colons,
    // After `${`: everything to the next `}` is the name.
    Braced,
}

// What a braced word has held so far. Only `{*}` can be an expansion prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Inside {
    Nothing,
    Star,
    More,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Brace {
    depth: usize,
    // A backslash hides this byte.
    hidden: bool,
    inside: Inside,
}

// Where the scan stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum At {
    // Before a command: white space, line ends, `;` and comments.
    #[default]
    Lead,
    Comment {
        hidden: bool,
    },
    // In a command, between two words.
    Gap,
    // After a backslash where a word may start. With a line end the two are
    // white space; any other byte makes the backslash the start of a word.
    Slash {
        lead: bool,
    },
    Braced(Brace),
    // Directly after the `"` or `}` that closes a word. `star` says that
    // the word is `{*}` and may be an expansion prefix; `slash`, that a
    // backslash followed.
    Closed {
        star: bool,
        slash: bool,
    },
    Text(Text),
    // After a backslash in such text.
    Hidden(Text),
    Var(Var, Text),
    // Behind an excluded byte. `seen` counts the bytes read since the last
    // excluded one, and `run` the backslashes directly before this byte.
    Dark {
        seen: usize,
        run: usize,
    },
    // Behind an excluded byte that a brace, a quote, a bracket or an index
    // reached: the state is never known again.
    Dead,
    // After a parse error or the end of the file.
    Stopped,
}

// What a byte does to the list of commands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mark {
    #[default]
    None,
    // A word starts this many bytes before the byte.
    Starts(usize),
    // The word ends this many bytes before the byte.
    Ends(usize),
    // The word and the command end.
    Closes(usize),
    // The command ends; its last word has ended before.
    Over,
    // The word has the expansion prefix.
    Expands,
}

// What a byte does to the state around the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Nest {
    #[default]
    None,
    // `[` opens a script in text of this kind.
    Bracket(Text),
    // `(` opens an array index in text of this kind.
    Index(Text),
    // `]` or `)` closes what was opened last.
    Shut,
    // A parse error of Tcl.
    Fault,
    // The byte that ends the file.
    End,
    // The state is known again after this byte.
    Dawn,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Signal {
    pub(super) mark: Mark,
    pub(super) nest: Nest,
}

const fn blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | 0x0b | 0x0c)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Machine {
    pub(super) at: At,
    // The byte before was a carriage return: a line feed belongs to it.
    pub(super) after_cr: bool,
    // The script being read stands in a bracket, and `]` ends it.
    pub(super) nested: bool,
    // The word being read has had its expansion prefix.
    pub(super) expanded: bool,
}

impl Machine {
    fn to(self, at: At) -> (Self, Signal) {
        (Self { at, ..self }, Signal::default())
    }

    fn marks(self, at: At, mark: Mark) -> (Self, Signal) {
        let nest = Nest::None;
        (Self { at, ..self }, Signal { mark, nest })
    }

    fn nests(self, nest: Nest) -> (Self, Signal) {
        let mark = Mark::None;
        (self, Signal { mark, nest })
    }

    // Reads one byte of the file as `source` reads it: the byte `0x1a` ends
    // the file, and a carriage return, with or without a line feed after it,
    // is one line feed.
    pub(super) fn feed(mut self, byte: u8) -> (Self, Signal) {
        if byte == END_OF_FILE {
            return self.nests(Nest::End);
        }
        let after_cr = std::mem::replace(&mut self.after_cr, byte == b'\r');
        if after_cr && byte == b'\n' {
            return self.to(self.at);
        }
        self.step(if byte == b'\r' { b'\n' } else { byte })
    }

    fn step(self, byte: u8) -> (Self, Signal) {
        match self.at {
            At::Lead | At::Gap => self.between(byte),
            At::Comment { hidden } => self.comment(hidden, byte),
            At::Slash { lead } => self.slash(lead, byte),
            At::Braced(brace) => self.braced(brace, byte),
            At::Closed { star, slash: false } => self.closed(star, byte),
            At::Closed { star, slash: true } => self.closed_slash(star, byte),
            At::Text(kind) => self.text(kind, byte),
            At::Hidden(kind) => self.hidden(kind, byte),
            At::Var(var, kind) => self.variable(var, kind, byte),
            At::Dark { seen, run } => self.dark(seen, run, byte),
            At::Dead | At::Stopped => self.to(self.at),
        }
    }

    // Where a command or a word may start.
    fn between(self, byte: u8) -> (Self, Signal) {
        let lead = self.at == At::Lead;
        match byte {
            _ if blank(byte) => self.to(self.at),
            b'\n' | b';' if lead => self.to(At::Lead),
            b'\n' | b';' => self.marks(At::Lead, Mark::Over),
            b'#' if lead => self.to(At::Comment { hidden: false }),
            b'\\' => self.to(At::Slash { lead }),
            b']' if self.nested => self.nests(Nest::Shut),
            _ => self.word(byte, Mark::Starts(0)),
        }
    }

    // A comment runs to the first line end that no backslash hides.
    fn comment(self, hidden: bool, byte: u8) -> (Self, Signal) {
        match byte {
            _ if hidden => self.to(At::Comment { hidden: false }),
            b'\\' => self.to(At::Comment { hidden: true }),
            b'\n' => self.to(At::Lead),
            _ => self.to(self.at),
        }
    }

    fn slash(self, lead: bool, byte: u8) -> (Self, Signal) {
        match byte {
            b'\n' if lead => self.to(At::Lead),
            b'\n' => self.to(At::Gap),
            // The backslash started a bare word, and hides this byte.
            _ => self.fresh().marks(At::Text(Text::Bare), Mark::Starts(1)),
        }
    }

    fn fresh(self) -> Self {
        let expanded = false;
        Self { expanded, ..self }
    }

    // The first byte of a word, or of what an expansion prefix expands.
    fn word(self, byte: u8, mark: Mark) -> (Self, Signal) {
        let word = if mark == Mark::Expands {
            self
        } else {
            self.fresh()
        };
        match byte {
            b'"' => word.marks(At::Text(Text::Quoted), mark),
            b'{' => {
                let brace = Brace {
                    depth: 1,
                    hidden: false,
                    inside: Inside::Nothing,
                };
                word.marks(At::Braced(brace), mark)
            }
            _ => {
                let (next, signal) = word.text(Text::Bare, byte);
                (next, Signal { mark, ..signal })
            }
        }
    }

    // The state is known again after a line end that the bytes read prove
    // unescaped: an even run of backslashes stands before it, and that run
    // and the byte before it were read.
    fn dark(self, seen: usize, run: usize, byte: u8) -> (Self, Signal) {
        let seen = seen.saturating_add(1);
        match byte {
            b'\n' if run.is_multiple_of(2) && seen > run + 1 => {
                let known = Self {
                    at: At::Lead,
                    after_cr: self.after_cr,
                    ..Self::default()
                };
                known.nests(Nest::Dawn)
            }
            b'\\' => self.to(At::Dark { seen, run: run + 1 }),
            _ => self.to(At::Dark { seen, run: 0 }),
        }
    }
}

// In a word.
impl Machine {
    // In a braced word Tcl counts braces and nothing else. A backslash
    // hides the next byte.
    fn braced(self, brace: Brace, byte: u8) -> (Self, Signal) {
        let Brace { depth, inside, .. } = brace;
        let (depth, hidden, inside) = match byte {
            _ if brace.hidden => (depth, false, Inside::More),
            b'\\' => (depth, true, Inside::More),
            b'{' => (depth + 1, false, Inside::More),
            b'}' if depth == 1 => {
                let star = inside == Inside::Star && !self.expanded;
                return self.to(At::Closed { star, slash: false });
            }
            b'}' => (depth - 1, false, Inside::More),
            b'*' if inside == Inside::Nothing => (depth, false, Inside::Star),
            _ => (depth, false, Inside::More),
        };
        self.to(At::Braced(Brace {
            depth,
            hidden,
            inside,
        }))
    }

    // After a closing `"` or `}` Tcl accepts white space or the end of the
    // command. After `{*}` any other byte starts the word that the prefix
    // expands; after any other word it is a parse error.
    fn closed(self, star: bool, byte: u8) -> (Self, Signal) {
        let expanded = true;
        match byte {
            _ if blank(byte) => self.marks(At::Gap, Mark::Ends(0)),
            b'\n' | b';' => self.marks(At::Lead, Mark::Closes(0)),
            b']' if self.nested => self.nests(Nest::Shut),
            b'\\' => self.to(At::Closed { star, slash: true }),
            _ if star => Self { expanded, ..self }.word(byte, Mark::Expands),
            _ => self.nests(Nest::Fault),
        }
    }

    // The byte after a backslash that follows a closed word. With a line
    // end the two are white space. Any other byte is a word byte: of the
    // expanded word after `{*}`, and a parse error after any other word.
    fn closed_slash(self, star: bool, byte: u8) -> (Self, Signal) {
        let expanded = true;
        match byte {
            b'\n' => self.marks(At::Gap, Mark::Ends(1)),
            _ if star => Self { expanded, ..self }.marks(At::Text(Text::Bare), Mark::Expands),
            _ => self.nests(Nest::Fault),
        }
    }

    fn text(self, kind: Text, byte: u8) -> (Self, Signal) {
        match (kind, byte) {
            (_, b'$') => self.to(At::Var(Var::Dollar, kind)),
            (_, b'[') => self.nests(Nest::Bracket(kind)),
            (_, b'\\') => self.to(At::Hidden(kind)),
            (Text::Bare, _) if blank(byte) => self.marks(At::Gap, Mark::Ends(0)),
            (Text::Bare, b'\n' | b';') => self.marks(At::Lead, Mark::Closes(0)),
            (Text::Bare, b']') if self.nested => self.nests(Nest::Shut),
            (Text::Quoted, b'"') => {
                let (star, slash) = (false, false);
                self.to(At::Closed { star, slash })
            }
            (Text::Index, b')') => self.nests(Nest::Shut),
            _ => self.to(At::Text(kind)),
        }
    }

    // The byte after a backslash stands for itself. Only a line end after a
    // bare word does not: the two are white space, and the word ended
    // before the backslash.
    fn hidden(self, kind: Text, byte: u8) -> (Self, Signal) {
        if kind == Text::Bare && byte == b'\n' {
            self.marks(At::Gap, Mark::Ends(1))
        } else {
            self.to(At::Text(kind))
        }
    }

    // A name is ASCII letters, digits, `_` and runs of two or more `:`. A
    // `(` after it opens an array index, also after no name at all.
    fn variable(self, var: Var, kind: Text, byte: u8) -> (Self, Signal) {
        let name = byte.is_ascii_alphanumeric() || byte == b'_';
        match (var, byte) {
            (Var::Braced, b'}') => self.to(At::Text(kind)),
            (Var::Braced, _) => self.to(self.at),
            (Var::Dollar, b'{') => self.to(At::Var(Var::Braced, kind)),
            (Var::Dollar | Var::Name | Var::Colons, _) if name => self.to(At::Var(Var::Name, kind)),
            (Var::Dollar | Var::Name, b':') => self.to(At::Var(Var::Colon, kind)),
            (Var::Colon | Var::Colons, b':') => self.to(At::Var(Var::Colons, kind)),
            (Var::Dollar | Var::Name | Var::Colons, b'(') => self.nests(Nest::Index(kind)),
            // The reference has ended, or was a `$` alone: the byte is text.
            _ => self.text(kind, byte),
        }
    }
}

// Where the bytes that can be read end.
impl Machine {
    // Whether a brace, a quote or an index is open at this place.
    pub(super) fn holds(self) -> bool {
        match self.at {
            At::Braced(_) | At::Var(Var::Braced, _) => true,
            At::Text(kind) | At::Hidden(kind) | At::Var(_, kind) => kind != Text::Bare,
            _ => false,
        }
    }

    // What the end of the file does to the command being read: the marks
    // it makes, or nothing if a word is still open.
    pub(super) fn last(self) -> Option<[Mark; 2]> {
        let word = Some([Mark::None, Mark::Closes(0)]);
        match self.at {
            At::Lead | At::Comment { .. } | At::Dark { .. } | At::Dead | At::Stopped => {
                Some([Mark::None; 2])
            }
            At::Gap => Some([Mark::None, Mark::Over]),
            At::Slash { .. } => Some([Mark::Starts(1), Mark::Closes(0)]),
            At::Closed { slash: false, .. } | At::Text(Text::Bare) | At::Hidden(Text::Bare) => word,
            At::Var(var, Text::Bare) if var != Var::Braced => word,
            At::Closed { star: true, .. } => Some([Mark::Expands, Mark::Closes(0)]),
            _ => None,
        }
    }
}
