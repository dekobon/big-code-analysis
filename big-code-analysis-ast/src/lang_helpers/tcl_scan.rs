//! One forward scan of a Tcl script by Tcl's own rules for commands and
//! words (Tcl(n), rules 1 to 12, as far as they say where a command and a
//! word end). It lists the commands of the script's own level.
//!
//! Both dialects' grammars read a comment, a quoted word and a bracket
//! wherever they stand, so a node can start or end where no command or word
//! of Tcl does, and the tree need not hold an error node. The scan reads
//! bytes only: it takes no node and no kind id.
//!
//! It reads a file as `source` does. The first byte `0x1a` ends the file.
//! CR LF, a CR alone and LF are each one line end. A byte order mark is a
//! byte like any other, so it is part of the first word.

use crate::documentation::budget::WorkBudget;
use crate::documentation::{SourceContext, SourceError, SourcePieces, byte_scan};
use std::cell::Cell;
use std::ops::Range;

mod machine;
use machine::{At, Machine, Mark, Nest, Signal, Text};

/// A command of the script's own level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Command {
    /// Its first byte.
    pub(crate) start: usize,
    /// The first four of its words, each from its first byte to the byte
    /// after its last.
    pub(crate) words: [(usize, usize); 4],
    /// How many words it has. Five stands for five or more.
    pub(crate) count: u8,
    /// One of its words has the expansion prefix `{*}`.
    pub(crate) expands: bool,
}

/// What the scan says about one byte of the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Listed {
    /// A command of the script's own level starts at the byte, and the scan
    /// has read it to its end.
    Command(Command),
    /// No such command starts there: the byte stands in a command, a word
    /// or a comment that started earlier, the command that starts there has
    /// a parse error, or the scan had stopped before the byte.
    Nothing,
    /// Excluded bytes hide the answer: the state of the scan is not known
    /// at the byte, or the command that starts there reaches an excluded
    /// byte before it ends.
    Unknown,
}

// A bracket or an array index that is open around the place of the scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    bracket: bool,
    // The text the scan returns to when it closes.
    resume: Text,
}

// What became of the command that started last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Fate {
    #[default]
    Unseen,
    Open,
    Done,
    // It reached an excluded byte before it ended.
    Cut,
    // It has a parse error.
    Broken,
}

/// The commands of a script's own level, listed by one forward scan of the
/// included bytes. The scan goes on only as far as a question needs, so
/// questions are asked in source order.
///
/// It stops at the first parse error of Tcl and at the end of the file, and
/// lists no command at or after that place. At an excluded byte its state
/// is not known. It is known again after the first later line end that the
/// included bytes prove unescaped, unless a brace, a quote, a bracket or an
/// index was open at the excluded byte: then it is never known again.
#[derive(Debug, Default)]
pub(crate) struct CommandScan<'s> {
    // The included bytes, once the first question has been asked.
    pieces: Option<SourcePieces<'s>>,
    // The piece that holds the next byte, and that byte.
    piece: usize,
    at: usize,
    machine: Machine,
    // One entry for each bracket and each array index that is open.
    open: Vec<Frame>,
    // The command of the script's own level that started last.
    command: Command,
    fate: Fate,
    // The first byte since which the state is not known.
    dark: Option<usize>,
    // The last run of bytes at which it was not known, and is known again.
    lost: Range<usize>,
}

impl<'s> CommandScan<'s> {
    /// What the scan lists at the byte `start`. It reads on to that byte,
    /// and to the end of the command that starts there. For a byte that it
    /// has passed it reads nothing: it still knows the command that started
    /// last, and answers [`Listed::Nothing`] for the start of an earlier one.
    pub(crate) fn command_at<B: WorkBudget>(
        &mut self,
        source: &SourceContext<'s>,
        start: usize,
        budget: &B,
    ) -> Result<Listed, SourceError> {
        let end = source.bytes().len();
        if self.pieces.is_none() {
            self.pieces = Some(source.pieces_with_budget(0..end, budget)?);
        }
        // Every round reads a byte, leaves a piece or ends the scan, and a
        // scan that has ended answers.
        loop {
            if let Some(listed) = self.answer(start) {
                return Ok(listed);
            }
            self.advance(end, start, budget).map_err(Into::into)?;
        }
    }

    // The answer, if the scan has read far enough to give it. A backslash at
    // the byte starts a word unless a line end follows: the next byte says.
    fn answer(&self, start: usize) -> Option<Listed> {
        if self.command.start == start && self.fate != Fate::Unseen {
            return self.fated();
        }
        let slash = matches!(self.machine.at, At::Slash { .. }) && start + 1 == self.at;
        match self.machine.at {
            _ if start < self.at && !slash => Some(self.passed(start)),
            At::Dead => Some(Listed::Unknown),
            At::Stopped => Some(Listed::Nothing),
            _ => None,
        }
    }

    // What became of the command that started last, once that is known.
    fn fated(&self) -> Option<Listed> {
        match self.fate {
            Fate::Done => Some(Listed::Command(self.command)),
            Fate::Cut => Some(Listed::Unknown),
            Fate::Broken => Some(Listed::Nothing),
            Fate::Open | Fate::Unseen => None,
        }
    }

    // A byte the scan has passed, at which no command of this level starts.
    fn passed(&self, byte: usize) -> Listed {
        if self.dark.is_some_and(|from| from <= byte) || self.lost.contains(&byte) {
            Listed::Unknown
        } else {
            Listed::Nothing
        }
    }
}

// Reading on.
impl<'s> CommandScan<'s> {
    // The piece that holds the next byte, if an included byte is left.
    fn piece(&self) -> Option<(Range<usize>, &'s [u8])> {
        let piece = self.pieces.as_ref()?.pieces().get(self.piece)?;
        Some((piece.range(), piece.bytes()))
    }

    // Reads on: to the next byte that changes the list, to the byte asked
    // for, or, in the command that starts there, to the end of the piece.
    fn advance<B: WorkBudget>(
        &mut self,
        end: usize,
        asked: usize,
        budget: &B,
    ) -> Result<(), B::Error> {
        let Some((range, bytes)) = self.piece() else {
            self.run_out(end);
            return Ok(());
        };
        if self.at < range.start {
            self.hide();
            self.at = range.start;
            return Ok(());
        }
        let following = self.fate == Fate::Open && self.command.start == asked;
        let until = if following {
            range.end
        } else {
            range.end.min(asked.max(self.at) + 1)
        };
        let rest = &bytes[self.at - range.start..until - range.start];
        if let Some(signal) = self.read(rest, budget)? {
            self.signal(signal, self.at - 1, budget)?;
        }
        self.piece += usize::from(self.at == range.end);
        Ok(())
    }

    // Reads these bytes up to the first that changes more than the place of
    // the scan, and says what that byte does.
    fn read<B: WorkBudget>(
        &mut self,
        bytes: &[u8],
        budget: &B,
    ) -> Result<Option<Signal>, B::Error> {
        let machine = Cell::new(self.machine);
        let signal = Cell::new(Signal::default());
        let stop = byte_scan::position(
            bytes,
            |byte| {
                let (next, made) = machine.get().feed(byte);
                machine.set(next);
                signal.set(made);
                made != Signal::default()
            },
            budget,
        )?;
        self.machine = machine.get();
        self.at += stop.map_or(bytes.len(), |read| read + 1);
        Ok(stop.map(|_| signal.get()))
    }

    // No included byte is left: excluded bytes follow, or the source ends.
    fn run_out(&mut self, end: usize) {
        if self.at < end {
            self.hide();
            self.at = end;
        } else {
            self.ends(end);
        }
    }

    // The byte at `self.at` is excluded.
    fn hide(&mut self) {
        match self.machine.at {
            At::Dead | At::Stopped => return,
            At::Dark { .. } => {}
            _ => {
                if self.fate == Fate::Open {
                    self.fate = Fate::Cut;
                }
                self.dark = Some(self.at);
            }
        }
        let open = !self.open.is_empty() || self.machine.holds();
        let at = if open {
            At::Dead
        } else {
            At::Dark { seen: 0, run: 0 }
        };
        self.machine = Machine {
            at,
            ..Machine::default()
        };
    }

    // The file ends at `at`: at its last byte, or at the byte `0x1a`.
    fn ends(&mut self, at: usize) {
        let last = self.machine.last().filter(|_| self.open.is_empty());
        self.machine.at = At::Stopped;
        for mark in last.into_iter().flatten() {
            self.mark(mark, at);
        }
        // A command that is still open holds a word or a bracket that never
        // closes. No command is open once the scan has ended.
        if self.fate == Fate::Open {
            self.fate = Fate::Broken;
        }
    }

    fn signal<B: WorkBudget>(
        &mut self,
        signal: Signal,
        at: usize,
        budget: &B,
    ) -> Result<(), B::Error> {
        self.mark(signal.mark, at);
        match signal.nest {
            Nest::None => {}
            Nest::Bracket(resume) | Nest::Index(resume) => {
                budget.step()?;
                let bracket = matches!(signal.nest, Nest::Bracket(_));
                self.open.push(Frame { bracket, resume });
                self.enter(bracket);
            }
            Nest::Shut => {
                if let Some(frame) = self.open.pop() {
                    self.machine.at = At::Text(frame.resume);
                    self.machine.nested = self.open.last().is_some_and(|frame| frame.bracket);
                }
            }
            Nest::Fault => {
                self.machine.at = At::Stopped;
                if self.fate == Fate::Open {
                    self.fate = Fate::Broken;
                }
            }
            Nest::End => self.ends(at),
            // The line end itself was read while the state was not known.
            Nest::Dawn => self.lost = self.dark.take().unwrap_or(at)..at + 1,
        }
        Ok(())
    }

    // The scan enters a bracket, where a script starts, or an array index.
    fn enter(&mut self, bracket: bool) {
        self.machine.nested = bracket;
        self.machine.at = if bracket {
            At::Lead
        } else {
            At::Text(Text::Index)
        };
    }

    // Records what a byte at `at` does to the command of this level. What
    // stands in a bracket or in an index is no word of this level.
    fn mark(&mut self, mark: Mark, at: usize) {
        if !self.open.is_empty() {
            return;
        }
        let word = self.command.words.get_mut(usize::from(self.command.count));
        match mark {
            Mark::None => {}
            Mark::Starts(back) if self.fate == Fate::Open => {
                if let Some(word) = word {
                    word.0 = at - back;
                }
            }
            Mark::Starts(back) => {
                self.fate = Fate::Open;
                self.command = Command {
                    start: at - back,
                    ..Command::default()
                };
                self.command.words[0].0 = at - back;
            }
            Mark::Ends(back) | Mark::Closes(back) => {
                if let Some(word) = word {
                    word.1 = at - back;
                }
                self.command.count = self.command.count.saturating_add(1).min(5);
                if matches!(mark, Mark::Closes(_)) {
                    self.fate = Fate::Done;
                }
            }
            Mark::Over => self.fate = Fate::Done,
            Mark::Expands => self.command.expands = true,
        }
    }
}
