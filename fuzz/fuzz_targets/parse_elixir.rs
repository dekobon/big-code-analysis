//! Fuzz Elixir: parse, then every `Ast` walk.
//!
//! Elixir's control flow and declarations are `call` nodes told apart
//! only by their target's source bytes (`if`, `case`, `def`, …), so the
//! metric walk reads text where other languages compare `kind_id`
//! (#275, #1697): the word classification (`ElixirCallTarget`), its
//! per-node cache on the walk, and the keyword-section lookup
//! (`do:` / `else:` / `rescue:` in a trailing keyword list, #1602) all
//! run on whatever bytes the parser recovered into a `call`.
//!
//! The pinned grammar's `identifier` admits no invalid UTF-8: a stray
//! byte interrupting a call target lands in an `ERROR` span (one span
//! may hold several such bytes; a string or comment keeps them as
//! content), so the word readers' `utf8_text → None` arm is defensive
//! at this pin. The `error_in_call_target` seed exercises the
//! `ERROR`-under-`call` shape recovery does produce.

#![no_main]

use big_code_analysis::LANG;
use big_code_analysis_fuzz::walk_all;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    walk_all(LANG::Elixir, data.to_vec());
});
