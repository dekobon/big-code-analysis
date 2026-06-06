# Supported Languages

This is the list of programming languages parsed by
**big-code-analysis**. Each entry below is a real `LANG` variant
(defined by the `mk_langs!` invocation in `src/langs.rs`) and is
gated behind the matching per-language Cargo feature documented in
[Per-language Cargo features](./library/cargo-features.md).

- [x] Bash
- [x] C/C++
- [x] C#
- [x] Elixir
- [x] Go
- [x] Groovy
- [x] Irules
- [x] Java
- [x] JavaScript
- [x] Kotlin
- [x] Lua
- [x] Mozjs
- [x] Perl
- [x] Php
- [x] Python
- [x] Ruby
- [x] Rust
- [x] Tcl
- [x] Tsx
- [x] Typescript

Some entries are variants of a shared grammar pipeline. `JavaScript`
(the upstream `tree-sitter-javascript` grammar) is the default for
`.js`, `.mjs`, `.cjs`, and `.jsx` files; `Mozjs` is the Mozilla /
SpiderMonkey fork, now opt-in — it owns only the `.jsm` (Firefox
module) extension and reports the canonical slug `mozjs`. The two are
metric-equivalent on ordinary JavaScript. `Tsx` is `Typescript` with
JSX syntax enabled and reports the distinct slug `tsx`. C and C++ are
analysed by the single `C/C++` variant, which reports the slug `cpp`
(C# reports `csharp`). Every variant's slug is its `LANG::name`,
lowercase and punctuation-free so it round-trips through `FromStr`.

## Internal helper variants

The following `LANG` variants are not user-facing languages — they
are internal helpers in the C/C++ analysis pipeline (both ride the
`cpp` Cargo feature) and are not selected directly when analysing
source files:

- `Ccomment` — focuses on C/C++ comments.
- `Preproc` — focuses on C/C++ preprocessor macros.

> **Note:** `Mozcpp` is a vendored grammar *crate*
> (`bca-tree-sitter-mozcpp`, pulled in by the `cpp` feature), not a
> `LANG` variant — it backs the `C/C++` variant rather than
> appearing as a separate language.
