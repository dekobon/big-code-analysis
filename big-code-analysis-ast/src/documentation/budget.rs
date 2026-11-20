//! Fallible work accounting shared by source kernels and their infallible callers.

use std::cell::Cell;
use std::convert::Infallible;

use super::SourceError;

pub(crate) const STEP_INTERVAL: usize = 1_024;
pub(crate) const BYTE_INTERVAL: usize = 16 * 1_024;

pub(crate) trait WorkBudget {
    type Error: Into<SourceError>;

    fn checkpoint(&self) -> Result<(), Self::Error>;
    fn step(&self) -> Result<(), Self::Error>;
    fn bytes(&self, count: usize) -> Result<(), Self::Error>;
}

pub(crate) struct NoBudget;

impl WorkBudget for NoBudget {
    type Error = Infallible;

    #[inline]
    fn checkpoint(&self) -> Result<(), Infallible> {
        Ok(())
    }

    #[inline]
    fn step(&self) -> Result<(), Infallible> {
        Ok(())
    }

    #[inline]
    fn bytes(&self, _: usize) -> Result<(), Infallible> {
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct CollectionBudget {
    steps: Cell<usize>,
    bytes: Cell<usize>,
}

// These count actual checkpoints, not calls to a generic NoBudget wrapper.
crate::observation::counter!(polls, cfg(any(test, feature = "test-support")));

impl CollectionBudget {
    fn charge(
        &self,
        counter: &Cell<usize>,
        count: usize,
        interval: usize,
    ) -> Result<(), SourceError> {
        let mut remaining = count;
        while remaining >= interval - counter.get() {
            remaining -= interval - counter.get();
            counter.set(0);
            self.checkpoint()?;
        }
        counter.set(counter.get() + remaining);
        Ok(())
    }
}

impl WorkBudget for CollectionBudget {
    type Error = SourceError;

    fn checkpoint(&self) -> Result<(), SourceError> {
        polls::record();
        #[cfg(test)]
        cancel_at_checkpoint();
        self.steps.set(0);
        self.bytes.set(0);
        if crate::cancel::requested() {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn step(&self) -> Result<(), SourceError> {
        self.charge(&self.steps, 1, STEP_INTERVAL)
    }

    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.charge(&self.bytes, count, BYTE_INTERVAL)
    }
}

#[cfg(test)]
thread_local! {
    static CANCEL_AT: Cell<Option<usize>> = const { Cell::new(None) };
}

#[cfg(test)]
fn cancel_at_checkpoint() {
    CANCEL_AT.with(|at| {
        if at.get() == Some(polls::observed()) {
            crate::cancel::cancel_installed_for_test();
        }
    });
}

#[cfg(test)]
pub(super) fn cancel_after<T>(checkpoints: usize, action: impl FnOnce() -> T) -> T {
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CANCEL_AT.with(|at| at.set(self.0));
        }
    }
    let previous = CANCEL_AT.with(|at| at.replace(Some(polls::observed() + checkpoints)));
    let _restore = Restore(previous);
    crate::cancel::ParseCancel::new().run(action)
}

pub(crate) fn infallible<T>(result: Result<T, Infallible>) -> T {
    match result {
        Ok(value) => value,
        Err(never) => match never {},
    }
}

impl From<Infallible> for SourceError {
    fn from(never: Infallible) -> Self {
        match never {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_work_and_explicit_checkpoints_have_bounded_intervals() {
        let budget = CollectionBudget::default();
        let before = polls::observed();
        for _ in 0..STEP_INTERVAL - 1 {
            budget.step().unwrap();
        }
        budget.bytes(BYTE_INTERVAL - 1).unwrap();
        assert_eq!(polls::observed() - before, 0);
        budget.step().unwrap();
        assert_eq!(polls::observed() - before, 1);
        budget.bytes(BYTE_INTERVAL).unwrap();
        assert_eq!(polls::observed() - before, 2);
        budget.checkpoint().unwrap();
        assert_eq!(polls::observed() - before, 3);
    }

    #[test]
    fn no_budget_never_polls_even_under_active_cancellation() {
        let flag = crate::cancel::ParseCancel::new();
        flag.cancel();
        let before = polls::observed();
        flag.run(|| {
            infallible(NoBudget.checkpoint());
            infallible(NoBudget.step());
            infallible(NoBudget.bytes(usize::MAX));
        });
        assert_eq!(polls::observed() - before, 0);
    }
}
