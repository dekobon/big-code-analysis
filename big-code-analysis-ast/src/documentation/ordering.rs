//! Bounded chunk sorts followed by an interruptible numeric-key merge.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::budget::{STEP_INTERVAL, WorkBudget};

struct Comparisons<'b, B: WorkBudget> {
    budget: &'b B,
    error: RefCell<Option<B::Error>>,
}

impl<B: WorkBudget> Comparisons<'_, B> {
    fn compare<K: Ord>(&self, left: &K, right: &K) -> std::cmp::Ordering {
        if self.error.borrow().is_none() {
            *self.error.borrow_mut() = self.budget.step().err();
        }
        left.cmp(right)
    }

    fn check(&self) -> Result<(), B::Error> {
        match self.error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

struct Entry<'b, K: Ord, B: WorkBudget> {
    key: (K, usize),
    comparisons: &'b Comparisons<'b, B>,
}

impl<K: Ord, B: WorkBudget> PartialEq for Entry<'_, K, B> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<K: Ord, B: WorkBudget> Eq for Entry<'_, K, B> {}

impl<K: Ord, B: WorkBudget> PartialOrd for Entry<'_, K, B> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<K: Ord, B: WorkBudget> Ord for Entry<'_, K, B> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.comparisons.compare(&self.key, &other.key)
    }
}

pub(super) fn ordered<K: Copy + Ord, B: WorkBudget>(
    mut keys: Vec<K>,
    budget: &B,
) -> Result<Vec<K>, B::Error> {
    // bca: suppress(nexits, halstead) -- each bounded sort and heap operation must propagate its latched cancellation before publishing output.
    let comparisons = Comparisons {
        budget,
        error: RefCell::new(None),
    };
    for chunk in keys.chunks_mut(STEP_INTERVAL) {
        budget.checkpoint()?;
        chunk.sort_unstable_by(|left, right| comparisons.compare(left, right));
        comparisons.check()?;
    }
    let mut heap = BinaryHeap::new();
    for (chunk, values) in keys.chunks(STEP_INTERVAL).enumerate() {
        budget.step()?;
        heap.push(Reverse(Entry {
            key: (values[0], chunk * STEP_INTERVAL),
            comparisons: &comparisons,
        }));
        comparisons.check()?;
    }
    let mut output = Vec::with_capacity(keys.len());
    while let Some(Reverse(entry)) = heap.pop() {
        comparisons.check()?;
        budget.step()?;
        let (key, at) = entry.key;
        output.push(key);
        let next = at + 1;
        if next < keys.len() && next % STEP_INTERVAL != 0 {
            heap.push(Reverse(Entry {
                key: (keys[next], next),
                comparisons: &comparisons,
            }));
            comparisons.check()?;
        }
    }
    budget.checkpoint()?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documentation::budget::NoBudget;
    use std::cell::Cell;

    struct StopAt {
        work: Cell<usize>,
        stop: usize,
    }

    impl WorkBudget for StopAt {
        type Error = crate::documentation::SourceError;
        fn checkpoint(&self) -> Result<(), Self::Error> {
            Ok(())
        }
        fn bytes(&self, _: usize) -> Result<(), Self::Error> {
            self.step()
        }
        fn step(&self) -> Result<(), Self::Error> {
            let next = self.work.get() + 1;
            self.work.set(next);
            if next == self.stop {
                Err(Self::Error::Cancelled)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn chunk_sort_cancellation_latches_without_corrupting_comparator_order() {
        let budget = StopAt {
            work: Cell::new(0),
            stop: 3,
        };
        let input: Vec<_> = (0..3000).rev().collect();
        assert_eq!(
            ordered(input.clone(), &budget),
            Err(crate::documentation::SourceError::Cancelled)
        );
        assert_eq!(budget.work.get(), 3);
        assert_eq!(
            ordered(input, &NoBudget).unwrap(),
            (0..3000).collect::<Vec<_>>()
        );
    }

    #[test]
    fn merge_comparisons_are_budgeted_and_duplicates_preserve_numeric_order() {
        let input: Vec<_> = (0..3000).map(|i| (i * 53 % 137, i)).collect();
        let budget = StopAt {
            work: Cell::new(0),
            stop: usize::MAX,
        };
        let comparisons = Comparisons {
            budget: &budget,
            error: RefCell::new(None),
        };
        for chunk in input.chunks(STEP_INTERVAL) {
            let mut chunk = chunk.to_vec();
            chunk.sort_unstable_by(|a, b| comparisons.compare(a, b));
        }
        let sort_steps = budget.work.get();
        let budget = StopAt {
            work: Cell::new(0),
            stop: sort_steps + 20,
        };
        assert_eq!(
            ordered(input.clone(), &budget),
            Err(crate::documentation::SourceError::Cancelled)
        );
        assert_eq!(budget.work.get(), sort_steps + 20);
        let mut expected = input.clone();
        expected.sort_unstable();
        assert_eq!(ordered(input, &NoBudget).unwrap(), expected);
    }
}
