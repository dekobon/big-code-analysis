//! Fuzz the C-family preprocessor path: harvest macros, then re-analyse
//! the same bytes through the macro-masking lexer.
//!
//! # Why this target exists
//!
//! It is the direct residue of #1152. Adopting
//! `clippy::indexing_slicing` on `src/c_macro.rs` left nine per-function
//! `#[allow]`s — every one a computed index into attacker-controlled
//! bytes whose bound is asserted by a human comment rather than by the
//! compiler. That is precisely the population a fuzzer is for, and #126
//! (a 2049-byte macro identifier slicing a fixed 2048-byte array) is the
//! bug that class already produced once.
//!
//! # Why the plain `parse_cpp` target cannot reach it
//!
//! `c_macro::replace` is called from `parser::get_fake_code`, which
//! returns `None` unless `Source::preproc` is set *and* the language is
//! `C`, `Cpp` or `Mozcpp`. `analyze(Source::from_bytes(LANG::Cpp, …))`
//! leaves `preproc` at `None`, so every byte-level index in that module
//! is unreachable from the per-language targets. Attaching results is
//! the whole point of this file.
//!
//! # Non-vacuity
//!
//! `replace` runs its lexer over every byte whichever macros were
//! harvested — the set is consulted only at identifier boundaries, and
//! an empty one still drives `step_normal`, `step_quoted`,
//! `step_line_comment`, `step_block_comment`, `step_raw_string` and
//! `enter_raw_string`. So the nine allowed sites are exercised by any
//! input, not only by one containing a `#define`. Inputs that *do*
//! define a macro additionally reach the masking branches in `replace`
//! itself, which is what the `#define`-bearing corpus seeds are for.
//!
//! The path handed to `preprocess` and to `with_preproc_path` must be
//! the same value: `preprocess` keys `PreprocResults::files` by it, and
//! `visible_macros` looks the file up by that key. A mismatch yields an
//! empty macro set — still not vacuous, per above, but it would silently
//! stop testing the harvest half.

#![no_main]

use std::path::Path;
use std::sync::Arc;

use big_code_analysis::{Ast, LANG, PreprocResults, Source, preprocess};
use big_code_analysis_fuzz::walk_parsed;
use libfuzzer_sys::fuzz_target;

/// Shared key for the harvest and the lookup. See the module docs.
const FUZZ_PATH: &str = "fuzz_input.cpp";

fuzz_target!(|data: &[u8]| {
    if !LANG::Cpp.is_enabled() {
        return;
    }
    let path = Path::new(FUZZ_PATH);

    // First pass: parse the input with the preprocessor grammar and
    // harvest its `#define` / `#undef` / `#include` directives. This is
    // itself a parse of attacker bytes, so it is under test too.
    let mut results = PreprocResults::default();
    preprocess(data.to_vec(), path, &mut results);

    // Second pass: re-analyse the same bytes as C++ with those results
    // attached, which routes them through `c_macro::replace`.
    let source = Source::from_bytes(LANG::Cpp, data.to_vec())
        .with_preproc_path(Some(path))
        .with_preproc(Some(Arc::new(results)));
    let Ok(ast) = Ast::parse(source) else {
        return;
    };
    walk_parsed(&ast);
});
