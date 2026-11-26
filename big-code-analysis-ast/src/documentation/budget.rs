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
    fn phase(&self) -> WorkPhase {
        WorkPhase::Collection
    }
    fn set_phase(&self, _: WorkPhase) {}
}

pub(crate) fn binary_search_end<B: WorkBudget>(
    mut end: usize,
    predicate: impl Fn(usize) -> bool,
    budget: &B,
) -> Result<usize, B::Error> {
    let mut first = 0;
    while first < end {
        budget.step()?;
        let middle = first + (end - first) / 2;
        if predicate(middle) {
            first = middle + 1;
        } else {
            end = middle;
        }
    }
    Ok(first)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WorkPhase {
    #[default]
    Collection,
    Classification,
    Projection,
    Ranges,
    Batch,
    Text,
    Decoration,
    Names,
    Ordering,
}

pub(crate) fn in_phase<T, B: WorkBudget>(
    budget: &B,
    phase: WorkPhase,
    action: impl FnOnce() -> T,
) -> T {
    struct Restore<'b, B: WorkBudget>(&'b B, WorkPhase);
    impl<B: WorkBudget> Drop for Restore<'_, B> {
        fn drop(&mut self) {
            self.0.set_phase(self.1);
        }
    }
    let _restore = Restore(budget, budget.phase());
    budget.set_phase(phase);
    action()
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
    phase: Cell<WorkPhase>,
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
        cancel_at_checkpoint(self.phase.get());
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
    fn phase(&self) -> WorkPhase {
        self.phase.get()
    }
    fn set_phase(&self, phase: WorkPhase) {
        self.phase.set(phase);
    }
}

#[cfg(test)]
thread_local! {
    static CANCEL_AT: Cell<Option<usize>> = const { Cell::new(None) };
    static CANCEL_PHASE: Cell<Option<WorkPhase>> = const { Cell::new(None) };
}

#[cfg(test)]
fn cancel_at_checkpoint(phase: WorkPhase) {
    CANCEL_AT.with(|at| {
        if at.get() == Some(polls::observed()) {
            crate::cancel::cancel_installed_for_test();
        }
    });
    CANCEL_PHASE.with(|wanted| {
        if wanted.get() == Some(phase) {
            crate::cancel::cancel_installed_for_test();
        }
    });
}

#[cfg(test)]
pub(super) fn cancel_in_phase<T>(phase: WorkPhase, action: impl FnOnce() -> T) -> T {
    struct Restore(Option<WorkPhase>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CANCEL_PHASE.with(|slot| slot.set(self.0));
        }
    }
    let _restore = Restore(CANCEL_PHASE.with(|slot| slot.replace(Some(phase))));
    crate::cancel::ParseCancel::new().run(action)
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

pub(crate) fn reverse<T, B: WorkBudget>(items: &mut [T], budget: &B) -> Result<(), B::Error> {
    for at in 0..items.len() / 2 {
        budget.step()?;
        items.swap(at, items.len() - at - 1);
    }
    Ok(())
}

pub(crate) fn child<'s, B: WorkBudget>(
    node: &crate::Node<'s>,
    predicate: impl Fn(&crate::Node<'s>) -> bool,
    budget: &B,
) -> Result<Option<crate::Node<'s>>, B::Error> {
    for child in node.children() {
        budget.step()?;
        if predicate(&child) {
            return Ok(Some(child));
        }
    }
    Ok(None)
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
