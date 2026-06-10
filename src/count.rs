// bca: suppress-file(halstead)
// Metric-count arithmetic over the whole metric set; file-level halstead is
// a many-fn aggregation artifact, not per-function logic complexity.

// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use num_format::{Locale, ToFormattedString};
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::traits::ParserTrait;

/// Counts the types of nodes specified in the input slice and the
/// number of nodes in a code. Crate-internal walk core reached through
/// the [`crate::Ast::count`] seam.
pub(crate) fn count<T: ParserTrait>(parser: &T, filters: &[String]) -> (usize, usize) {
    let filters = parser.filters(filters);
    let node = parser.root();
    let mut cursor = node.cursor();
    let mut stack = Vec::new();
    let mut good = 0;
    let mut total = 0;

    stack.push(node);

    while let Some(node) = stack.pop() {
        total += 1;
        if filters.any(&node) {
            good += 1;
        }
        cursor.reset(&node);
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    (good, total)
}

/// Opaque, shareable collector that accumulates a [`Count`] across the
/// worker threads of a [`crate::ConcurrentRunner`] walk.
///
/// Wraps the shared `Arc<Mutex<Count>>` behind a newtype so callers do
/// not handle the synchronization machinery directly. [`Clone`] is a
/// cheap reference-count bump, so each worker
/// can hold its own handle to the same tally while the config still
/// satisfies the `'static + Send + Sync` bound of
/// [`crate::ConcurrentRunner`]. Recover the final tally with
/// [`CountCollector::into_count`] once every worker has joined.
#[derive(Debug, Clone)]
pub struct CountCollector(Arc<Mutex<Count>>);

impl CountCollector {
    /// Creates an empty collector.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Count::default())))
    }

    /// Creates a collector seeded with an existing tally.
    #[must_use]
    pub fn with_count(count: Count) -> Self {
        Self(Arc::new(Mutex::new(count)))
    }

    /// Add a per-file `(good, total)` tally into the shared collector.
    ///
    /// The aggregation is two monotonically-incremented counters, so a
    /// peer worker that panicked mid-update leaves at worst a slightly
    /// low tally — never an unsafe state. Recover the poisoned guard
    /// (issue #445) and clear the poison so this and later callers — and
    /// the collector's final [`CountCollector::into_count`] — degrade
    /// rather than cascade into a pool-wide abort the way `.unwrap()`
    /// would.
    pub fn add(&self, good: usize, total: usize) {
        let mut results = self.0.lock().unwrap_or_else(|poisoned| {
            self.0.clear_poison();
            poisoned.into_inner()
        });
        results.good += good;
        results.total += total;
    }

    /// Consumes the collector, returning the accumulated [`Count`].
    ///
    /// Call this only after every worker sharing a clone of this
    /// collector has joined, so the underlying `Arc` reference count is
    /// back to one. Degrades rather than panics in the unlikely event
    /// that a worker panicked mid-update and poisoned the inner mutex
    /// (issue #445): the recovered guard still holds the fully-applied
    /// tally because the aggregation is two monotonically-incremented
    /// counters. If the `Arc` is unexpectedly still shared (a worker
    /// failed to join), falls back to a clone of the current tally
    /// rather than panicking.
    #[must_use]
    pub fn into_count(self) -> Count {
        match Arc::try_unwrap(self.0) {
            Ok(mutex) => mutex
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            Err(shared) => {
                let guard = shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Count {
                    good: guard.good,
                    total: guard.total,
                }
            }
        }
    }
}

impl Default for CountCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Count of different types of nodes in a code.
#[derive(Debug, Default)]
pub struct Count {
    /// The number of specific types of nodes searched in a code
    pub good: usize,
    /// The total number of nodes in a code
    pub total: usize,
}

impl fmt::Display for Count {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(
            f,
            "Total nodes: {}",
            self.total.to_formatted_string(&Locale::en)
        )?;
        writeln!(
            f,
            "Found nodes: {}",
            self.good.to_formatted_string(&Locale::en)
        )?;
        write!(
            f,
            "Percentage: {:.2}%",
            (self.good as f64) / (self.total as f64) * 100.
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    // Regression test for issue #445: a poisoned `stats` mutex must not
    // cascade into a pool-wide panic. A worker that panics while holding
    // the shared guard poisons the lock; `CountCollector::add` used to
    // re-panic on `.lock().unwrap()`. Verified by revert per
    // `.claude/rules/testing.md`: reverting the recovery makes this test
    // panic instead of applying the tally.
    #[test]
    fn add_degrades_on_poisoned_stats_mutex() {
        let stats = Arc::new(Mutex::new(Count::default()));

        // Poison the mutex: panic while holding the guard on a helper
        // thread, mirroring the dispatch_preproc #425 regression test.
        let poisoner = stats.clone();
        let handle = thread::spawn(move || {
            let _guard = poisoner.lock().expect("fresh mutex is unpoisoned");
            panic!("intentional panic to poison the stats mutex");
        });
        assert!(
            handle.join().is_err(),
            "poisoner thread should have panicked"
        );
        assert!(stats.is_poisoned(), "test setup failed to poison the mutex");

        // Adding into a poisoned collector must degrade (recover the
        // guard, clear the poison) rather than panic on `.lock()`.
        let collector = CountCollector(stats.clone());
        collector.add(2, 5);

        // The recovery clears the poison so later peers and the
        // collector's final `into_count()` see a usable, fully-applied
        // tally.
        assert!(
            !stats.is_poisoned(),
            "recovery should clear the poison flag"
        );
        let recovered = stats.lock().expect("poison cleared, lock must succeed");
        assert_eq!(
            (recovered.good, recovered.total),
            (2, 5),
            "the surviving worker's counts must still be applied"
        );
    }

    // `into_count` must surface the tally accumulated by every worker
    // sharing a clone of the collector, after they have all joined.
    #[test]
    fn into_count_returns_accumulated_tally() {
        let collector = CountCollector::new();

        let mut handles = Vec::new();
        for _ in 0..4 {
            let worker = collector.clone();
            handles.push(thread::spawn(move || {
                let mut guard = worker.0.lock().expect("fresh mutex is unpoisoned");
                guard.good += 1;
                guard.total += 10;
            }));
        }
        for handle in handles {
            handle.join().expect("worker thread must not panic");
        }

        let count = collector.into_count();
        assert_eq!(count.good, 4, "every worker's good count must be summed");
        assert_eq!(count.total, 40, "every worker's total count must be summed");
    }

    // `into_count` degrades to the recovered tally when the inner mutex
    // is poisoned, mirroring the #445 invariant for the extraction side.
    #[test]
    fn into_count_degrades_on_poisoned_mutex() {
        let collector = CountCollector::with_count(Count { good: 3, total: 7 });

        let poisoner = collector.clone();
        let handle = thread::spawn(move || {
            let _guard = poisoner.0.lock().expect("fresh mutex is unpoisoned");
            panic!("intentional panic to poison the collector mutex");
        });
        assert!(
            handle.join().is_err(),
            "poisoner thread should have panicked"
        );

        let count = collector.into_count();
        assert_eq!(count.good, 3, "poison recovery must preserve the tally");
        assert_eq!(count.total, 7, "poison recovery must preserve the tally");
    }
}
