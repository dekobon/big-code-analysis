//! Shared test scaffolding for the text-output walks.
//!
//! The `dump`, `dump_metrics`, and `dump_ops` walks all render through a
//! [`WriteColor`] and propagate its errors with `?`. Every one of those
//! `?`s is a branch no test takes while the sink is an infallible
//! `Vec<u8>`, so the failure half of each write is unexercised — which
//! matters, because `bca dump | head` and `bca ops | head` close the
//! pipe mid-stream.

use std::io::{ErrorKind, Write};

use termcolor::{ColorSpec, WriteColor};

/// A [`WriteColor`] that succeeds for `budget` operations and then fails
/// every later one with [`ErrorKind::BrokenPipe`].
///
/// Both `write` and `set_color` are counted. A walk reaches its writer
/// two ways — `write!` / `writeln!`, and the `color` / `intense_color`
/// helpers, which are `set_color` calls — so counting only one of them
/// would leave half the `?` sites unreachable.
pub(crate) struct FailAfter {
    budget: usize,
    attempts: usize,
}

impl FailAfter {
    /// A sink that permits `budget` operations before it starts failing.
    /// `usize::MAX` never fails, which is how a caller measures how many
    /// operations a full render performs.
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            budget,
            attempts: 0,
        }
    }

    /// How many operations were *attempted*, including those refused
    /// after the budget ran out.
    ///
    /// Counting past the failure is what lets a caller tell "the walk
    /// stopped at the first error" from "the walk swallowed it and kept
    /// writing".
    pub(crate) fn attempts(&self) -> usize {
        self.attempts
    }

    fn step(&mut self) -> std::io::Result<()> {
        self.attempts += 1;
        if self.attempts > self.budget {
            return Err(std::io::Error::new(ErrorKind::BrokenPipe, "sink closed"));
        }
        Ok(())
    }
}

impl Write for FailAfter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.step()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.step()
    }
}

impl WriteColor for FailAfter {
    fn supports_color(&self) -> bool {
        true
    }

    fn set_color(&mut self, _spec: &ColorSpec) -> std::io::Result<()> {
        self.step()
    }

    fn reset(&mut self) -> std::io::Result<()> {
        self.step()
    }
}

/// Assert that `render` surfaces an I/O error raised at *any* single
/// write position, and stops the walk there.
///
/// Sweeping the failure across every operation is what makes this
/// discriminating: one `let _ = write!(..)` anywhere in the walk leaves
/// exactly one budget in the sweep returning `Ok`, and a swallow that
/// let the walk continue shows up as attempts beyond the failing one.
///
/// `min_operations` guards the fixture rather than the code under test —
/// a fixture that shrank to a couple of writes would still pass every
/// assertion below while covering almost nothing.
pub(crate) fn assert_io_error_propagates_at_every_write(
    min_operations: usize,
    mut render: impl FnMut(&mut FailAfter) -> std::io::Result<()>,
) {
    let mut unlimited = FailAfter::new(usize::MAX);
    render(&mut unlimited).expect("a sink that never fails must not produce an error");
    let total = unlimited.attempts();
    assert!(
        total >= min_operations,
        "fixture must exercise at least {min_operations} write positions, got {total}"
    );

    for budget in 0..total {
        let mut sink = FailAfter::new(budget);
        let Err(err) = render(&mut sink) else {
            panic!("failing operation #{budget} of {total} must surface as an error");
        };
        assert_eq!(
            err.kind(),
            ErrorKind::BrokenPipe,
            "operation #{budget} must propagate the writer's own error kind"
        );
        assert_eq!(
            sink.attempts(),
            budget + 1,
            "the walk must stop at operation #{budget}, not keep writing past it"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The harness counts every fallible `WriteColor` operation,
    /// including the two no current walk uses.
    ///
    /// Pinned here so the sweep keeps its reach if a walk ever starts
    /// flushing or resetting, and so the harness cannot quietly become a
    /// writer that is incapable of failing in two of its entry points.
    #[test]
    fn every_fallible_operation_counts_against_the_budget() {
        let mut sink = FailAfter::new(3);
        assert!(sink.supports_color());

        sink.write_all(b"first").expect("within budget");
        sink.flush().expect("within budget");
        sink.set_color(&ColorSpec::new()).expect("within budget");
        assert_eq!(sink.attempts(), 3);

        assert_eq!(
            sink.reset().map_err(|e| e.kind()),
            Err(ErrorKind::BrokenPipe),
            "the fourth operation is past the budget"
        );
        assert_eq!(
            sink.attempts(),
            4,
            "a refused operation still counts as an attempt"
        );
    }
}
