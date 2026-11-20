//! Asking the parses and AST exports one thread runs to stop (#1939).
//!
//! `bca-web` gives each request a deadline, but the analysis runs on a
//! blocking thread that nothing outside it can interrupt. A request past
//! its deadline used to run to completion as an orphan, holding a worker
//! for as long as its parse took: minutes for some grammar shapes, which
//! a handful of small requests turned into a server answering 503.
//!
//! A [`ParseCancel`] installed with [`ParseCancel::run`] is read by every
//! parse the closure runs on this thread, through tree-sitter's progress
//! callback, which the runtime calls every hundred parse operations.
//! Once it is cancelled the parse stops and yields an empty tree, so the
//! walks after it finish at once. An AST export already under way reads
//! it once per node and yields no tree, since its walk over a wide
//! `ERROR` node can be quadratic (#1942). The answer is wrong by design:
//! the one party that asks to stop is the one that has stopped waiting
//! for it.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A flag that asks the parses and AST exports of the closure it is
/// installed for to stop. Clones share the flag, so the thread that
/// cancels holds one clone and the thread that parses another.
#[derive(Clone, Debug, Default)]
pub struct ParseCancel(Arc<AtomicBool>);

thread_local! {
    /// The flag the closure running on this thread was given, if any.
    static INSTALLED: Cell<Option<ParseCancel>> = const { Cell::new(None) };
}

impl ParseCancel {
    /// A flag that has not been cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks every parse and AST export running under this flag, or
    /// started under it later, to stop.
    pub fn cancel(&self) {
        // The flag publishes no data, only that the parse should stop, so
        // no ordering with other memory is needed.
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether [`cancel`](Self::cancel) has been called on any clone.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Runs `f` with this flag installed for the parses `f` runs on the
    /// current thread, and restores whatever was installed before.
    pub fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        let previous = INSTALLED.try_with(|slot| slot.replace(Some(self.clone())));
        let _restore = Restore(previous.ok().flatten());
        f()
    }
}

/// Puts the previously installed flag back when the closure returns or
/// unwinds.
struct Restore(Option<ParseCancel>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        // `try_with`: a closure run from another thread-local's destructor
        // may outlive this slot, and `with` would panic there.
        let _ = INSTALLED.try_with(|slot| slot.set(previous));
    }
}

/// Whether the flag installed on this thread, if any, has been
/// cancelled. Read by the parser's progress callback and by the AST
/// export walk.
pub(crate) fn requested() -> bool {
    INSTALLED
        .try_with(|slot| {
            let flag = slot.take();
            let cancelled = flag.as_ref().is_some_and(ParseCancel::is_cancelled);
            slot.set(flag);
            cancelled
        })
        .unwrap_or(false)
}

#[cfg(test)]
pub(crate) fn cancel_installed_for_test() {
    INSTALLED.with(|slot| {
        let flag = slot.take();
        if let Some(flag) = &flag {
            flag.cancel();
        }
        slot.set(flag);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the flag installed on this thread counts, only while its
    /// closure runs, and an inner `run` restores the outer flag.
    #[test]
    fn a_flag_counts_only_inside_its_run() {
        let outer = ParseCancel::new();
        let inner = ParseCancel::new();
        inner.cancel();
        assert!(!requested(), "nothing is installed yet");
        outer.run(|| {
            assert!(!requested(), "the outer flag is not cancelled");
            inner.run(|| assert!(requested(), "the inner flag is"));
            assert!(!requested(), "the outer flag is back");
            outer.cancel();
            assert!(requested(), "a cancel reaches the running closure");
        });
        assert!(!requested(), "nothing is installed after the run");
    }

    /// A clone shares the flag, which is how the thread that cancels
    /// reaches the thread that parses. The parsing thread reads the flag
    /// only after the test has cancelled it and said so over a channel,
    /// so the order is fixed rather than raced.
    #[test]
    fn a_cancel_reaches_a_clone_on_another_thread() {
        let flag = ParseCancel::new();
        let parsing = flag.clone();
        let (cancelled, wait_for_cancel) = std::sync::mpsc::channel::<()>();
        let seen = std::thread::spawn(move || {
            parsing.run(|| {
                wait_for_cancel
                    .recv()
                    .expect("the test thread reports the cancel");
                requested()
            })
        });
        flag.cancel();
        cancelled
            .send(())
            .expect("the parsing thread waits for the report");
        assert!(seen.join().expect("the parsing thread must not panic"));
    }
}
