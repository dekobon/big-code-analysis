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

extern crate num_format;

use num_format::{Locale, ToFormattedString};
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::traits::*;

// Hidden from rustdoc because the signature exposes `ParserTrait`,
// which is `#[doc(hidden)]` per issue #256. The CLI's `Count` callback
// remains the documented surface for this functionality.
#[doc(hidden)]
/// Counts the types of nodes specified in the input slice
/// and the number of nodes in a code.
pub fn count<T: ParserTrait>(parser: &T, filters: &[String]) -> (usize, usize) {
    let filters = parser.get_filters(filters);
    let node = parser.get_root();
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

/// Configuration options for counting different
/// types of nodes in a code.
#[derive(Debug)]
pub struct CountCfg {
    /// Types of nodes to count
    pub filters: Arc<[String]>,
    /// Number of nodes of a certain type counted by each thread
    pub stats: Arc<Mutex<Count>>,
}

/// Count of different types of nodes in a code.
#[derive(Debug, Default)]
pub struct Count {
    /// The number of specific types of nodes searched in a code
    pub good: usize,
    /// The total number of nodes in a code
    pub total: usize,
}

impl Callback for Count {
    type Res = std::io::Result<()>;
    type Cfg = CountCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        let (good, total) = count(parser, &cfg.filters);
        // The aggregation is two monotonically-incremented counters, so a
        // peer worker that panicked mid-update leaves at worst a slightly
        // low tally — never an unsafe state. Recover the poisoned guard
        // (issue #445) so one panicked worker does not cascade into a
        // pool-wide abort the way an `.unwrap()` would, and clear the
        // poison so later peers and the CLI's final `into_inner()`
        // (`run_command_count`) also degrade rather than panic.
        let mut results = cfg.stats.lock().unwrap_or_else(|poisoned| {
            cfg.stats.clear_poison();
            poisoned.into_inner()
        });
        results.good += good;
        results.total += total;
        Ok(())
    }
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
    use crate::RustParser;
    use std::path::PathBuf;
    use std::thread;

    // Regression test for issue #445: a poisoned `stats` mutex must not
    // cascade into a pool-wide panic. A worker that panics while holding
    // the shared guard poisons the lock; every subsequent `Count::call`
    // used to re-panic on `.lock().unwrap()`. Verified by revert per
    // `.claude/rules/testing.md`: reverting the recovery makes this test
    // panic instead of returning `Ok(())`.
    #[test]
    fn call_degrades_on_poisoned_stats_mutex() {
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

        let source = b"fn main() { let _ = 1; }".to_vec();
        let parser = RustParser::new(source, &PathBuf::from("poisoned.rs"), None);
        let cfg = CountCfg {
            filters: Arc::from(Vec::<String>::new()),
            stats: stats.clone(),
        };

        let result = Count::call(cfg, &parser);
        assert!(
            result.is_ok(),
            "poisoned stats mutex should degrade to Ok(()), not panic"
        );

        // The recovery clears the poison so later peers and the CLI's
        // final `into_inner()` see a usable, fully-applied tally.
        assert!(
            !stats.is_poisoned(),
            "recovery should clear the poison flag"
        );
        let recovered = stats.lock().expect("poison cleared, lock must succeed");
        assert!(
            recovered.total > 0,
            "the surviving worker's counts must still be applied"
        );
    }
}
