//! Fuzz the deep-nesting complexity class with a structured generator.
//!
//! #1052 was kilobyte-scale CPU exhaustion, unauthenticated on
//! `bca-web`; #1056 was a process-aborting stack overflow from
//! `Serialize`'s implicit recursion. Both are fixed, and both are shapes
//! a naive byte mutator reaches only by luck — `((((((…))))))` at depth
//! 500 is not a mutation away from anything in a seed corpus.
//!
//! This target generates the shape instead. See
//! `big_code_analysis_fuzz::nested` for what it emits and why the
//! generated source is deliberately parseable.
//!
//! Run it with `-timeout=10`, so a complexity regression surfaces as a
//! reported failure rather than as a run that is merely slow.

#![no_main]

use big_code_analysis_fuzz::{nested::Nesting, walk_all};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: Nesting| {
    walk_all(input.lang(), input.render());
});
