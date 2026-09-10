//! Thread-local counters that make an invisible optimization testable.
//!
//! Several optimizations change no output at all: reusing one
//! `tree_sitter::Parser` per thread (#1118), skipping a space-kind
//! lookup on a node that opens no space (#1110), serializing `Ops`
//! through a borrowed projection rather than an owned clone (#1110),
//! deferring the modeline scan behind a resolving extension (#1111).
//! Every assertion on the *result* of those paths holds just as well
//! once the optimization is reverted, so a revert is silent unless
//! something counts the work.
//!
//! # The invariant
//!
//! The counter and the function that bumps it are **unconditional**;
//! only the accessor is gated. Gating the counter itself would leave the
//! test observing a build production never ships — the counted branch
//! compiled under `cfg(test)` and the shipped one under nothing — which
//! is the exact failure these counters exist to catch. Four sites grew
//! that rule independently and each stated it in prose; `counter!`
//! makes it structural instead, emitting the three items together so the
//! wrong one cannot be gated.
//!
//! The cost is one `Cell` increment on a path that already does far more
//! (building a parser, classifying a node, projecting a whole tree),
//! which is why it is affordable to leave in the shipped build.
//!
//! # Two crates, one macro
//!
//! `big-code-analysis` declares counters of its own (in its `ops`,
//! `wire` and `spaces` walks) with the same macro, which is why it is
//! `#[macro_export]`ed. The accessor's gate is a parameter because the
//! two crates need different ones: a counter declared *here* must also be
//! readable by the root crate's tests, which see this crate as an
//! ordinary dependency (no `cfg(test)`), so it opens under the
//! `test-support` feature as well.

/// Declares a thread-local observation counter as a module: a private
/// `Cell`, an unconditional `record()`, and a gated `observed()`.
///
/// Takes the module's name and, optionally, the `cfg` predicate that
/// gates the accessor (`cfg(test)` when omitted). It deliberately
/// carries no narrative — *which* optimization a counter observes, and
/// why no assertion on the output can distinguish it, belongs in a
/// comment above each invocation.
///
/// A module rather than three free items so a call site names the
/// counter once (`parsers_built::record()`), which keeps the invocation
/// to one line and leaves no room for the recorder and the accessor to
/// drift apart.
#[macro_export]
#[doc(hidden)]
macro_rules! observation_counter {
    ($name:ident) => {
        $crate::observation_counter!($name, cfg(test));
    };
    ($name:ident, $gate:meta) => {
        #[doc(hidden)]
        pub mod $name {
            thread_local! {
                static COUNT: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) };
            }

            /// Records one occurrence on this thread.
            ///
            /// `pub(super)` on purpose: only the module that owns the
            /// counted path may bump it, so a counter cannot drift into
            /// meaning "whatever any caller felt like recording".
            #[inline]
            pub(super) fn record() {
                COUNT.with(|count| count.set(count.get() + 1));
            }

            /// Occurrences recorded on this thread. Only this accessor
            /// is gated; see the `observation` module docs.
            ///
            /// Wider than `record` because the guarded path and the test
            /// that guards it need not share a module: `child_scan_cursors`
            /// counts a cursor hoist in `node`, but the metric dump in
            /// `big-code-analysis` is one of the walks that has to hold
            /// one, and a counter only reachable from its own module
            /// silently leaves such a caller unguarded.
            #[$gate]
            pub fn observed() -> usize {
                COUNT.with(::std::cell::Cell::get)
            }
        }
    };
}

pub use observation_counter as counter;
