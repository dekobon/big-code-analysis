//! Fuzz C-family include *resolution*: harvest several files, then
//! resolve the include graph across them.
//!
//! # Why this is separate from `preproc_macro`
//!
//! `preproc_macro` covers the harvest-and-mask half of the preprocessor:
//! it drives `preprocess` over one buffer and re-analyses the same bytes
//! with the results attached. It never calls `fix_includes`, which is
//! the sole entry point for `build_include_graph`, `collapse_scc`,
//! `record_indirect_includes`, and the `min_distance_candidates` →
//! `guess_file` path — and the only producer of
//! `PreprocFile::indirect_includes` in production code, so under
//! `preproc_macro` the include loop in `visible_macros` never runs its
//! body (#1288).
//!
//! Kept as its own target rather than folded in so the two failure modes
//! stay attributable: a crash here is in graph resolution, a crash there
//! is in the macro lexer.
//!
//! # Why it is safe to fuzz an order-dependent function
//!
//! `fix_includes` walks `HashMap`-ordered collections, which is why
//! #1288 held this back pending a measurement: a crash that depends on
//! iteration order would not reproduce from its own artifact, defeating
//! the `-runs`-not-`-max_total_time` rule the rest of the harness
//! follows.
//!
//! Measured before writing this: across 40 runs of one 8-file input the
//! diagnostic *sequence* varied every time while the *set* and the
//! mutated `files` map were identical. Content is deterministic; only
//! the returned `Vec`'s order was not, and that is now sorted at the
//! source. So a crash here depends on input, not on iteration order,
//! and its artifact replays.
//!
//! # The input
//!
//! The buffer is split on NUL into up to [`MAX_FILES`] chunks, each
//! becoming one file. A single-byte separator is deliberate: a mutator
//! finds it by chance, where a multi-byte marker would have to be
//! learned. Bytes reach `preprocess` unmodified, as everywhere else in
//! this crate.
//!
//! Each file is registered in `all_files` twice — under its own name and
//! under a shared one. `guess_file` runs for *every* include, not only
//! ambiguous ones (measured: the `simple_chain` seed, which includes a
//! unique name, reaches it), so the shared entry is not what makes that
//! function reachable. What it adds is a lookup returning **several**
//! candidates, so `min_distance_candidates` has a tie to break rather
//! than a single answer to pass through.

#![no_main]

use std::collections::HashMap;
use std::path::PathBuf;

use big_code_analysis::{PreprocResults, fix_includes, preprocess};
use libfuzzer_sys::fuzz_target;

/// Greatest number of files one input may describe.
///
/// The graph work is roughly quadratic in this: every file's includes
/// are resolved against every candidate path. Eight keeps a run
/// comfortably inside the `-timeout=10` the fuzz runs use while still
/// allowing cycles long enough to exercise the SCC collapse.
const MAX_FILES: usize = 8;

/// Distinct per-file names. Short and predictable so a seed — and a
/// mutator that has seen one — can spell `#include "a.h"` and have it
/// resolve.
const NAMES: [&str; MAX_FILES] = ["a.h", "b.h", "c.h", "d.h", "e.h", "f.h", "g.h", "h.h"];

/// The name every file is *also* registered under, so an include of it
/// has several candidates and resolution has to choose.
const AMBIGUOUS_NAME: &str = "dup.h";

fuzz_target!(|data: &[u8]| {
    let mut results = PreprocResults::default();
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for (index, chunk) in data.split(|byte| *byte == 0).take(MAX_FILES).enumerate() {
        let name = NAMES[index];
        // Distinct parent directories: `guess_file` scores candidates by
        // path distance from the including file, so a flat layout would
        // leave that comparison with nothing to distinguish.
        let path = PathBuf::from(format!("d{index}/{name}"));

        preprocess(chunk.to_vec(), &path, &mut results);

        all_files
            .entry(name.to_owned())
            .or_default()
            .push(path.clone());
        all_files
            .entry(AMBIGUOUS_NAME.to_owned())
            .or_default()
            .push(path);
    }

    // The diagnostics are the return value; dropping them is fine, but
    // the call must not be optimised away.
    let diagnostics = fix_includes(&mut results.files, &all_files);
    std::hint::black_box(diagnostics);
});
