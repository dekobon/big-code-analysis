# Supported Languages

This is the list of programming languages parsed by
**big-code-analysis**. Each entry below is a real `LANG` variant
(defined by the `mk_langs!` invocation in `src/langs.rs`) and is
gated behind the matching per-language Cargo feature documented in
[Per-language Cargo features](./library/cargo-features.md).

- [x] Bash
- [x] C
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
- [x] Mozcpp
- [x] Mozjs
- [x] Objective-C
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
JSX syntax enabled and reports the distinct slug `tsx`. Since #721 C
has its own variant `C` (slug `c`, upstream `tree-sitter-c`), owning
`.c` and the `c` emacs mode; the `C/C++` variant (slug `cpp`, upstream
`tree-sitter-cpp` since #720) keeps `.cpp` / `.cc` / `.h` and the rest.
`.h` deliberately stays on `Cpp`: a C++ header through the C grammar
ERROR-cascades on `class` / `template`, whereas a C header through the
C++ grammar only trips on C++-keyword identifiers. The Mozilla/Gecko
C++ dialect is the opt-in `Mozcpp` variant (slug `mozcpp`), which owns
no file extensions and is selected only by name — exactly as `Mozjs`
relates to `JavaScript` (C# reports `csharp`). Since #724 `Objective-C`
(slug `objc`, upstream `tree-sitter-objc`) owns `.m` and the `objc` /
`objective-c` emacs modes. Objective-C++ (`.mm`) stays on `Cpp`: a
`.mm` file mixes Objective-C with C++, and the `tree-sitter-objc`
grammar cannot parse the C++ half (templates, namespaces, `::`), so the
C++ grammar — which only stumbles on the Objective-C glue — degrades
more gracefully there, the same trade-off `.h` uses. Metrics for the
Objective-C parts of a `.mm` file are therefore approximate. Every
variant's slug is
its `LANG::name`, lowercase and punctuation-free so it round-trips
through `FromStr`.

## Internal helper variants

The following `LANG` variants are not user-facing languages — they
are internal helpers in the C-family analysis pipeline (they ride
every C-family Cargo feature: `cpp`, `c`, and `mozcpp`) and are not
selected directly when analysing source files:

- `Ccomment` — focuses on C/C++ comments.
- `Preproc` — focuses on C/C++ preprocessor macros.

> **Note:** Since #720 the Mozilla/Gecko C++ dialect is exposed as the
> `Mozcpp` `LANG` variant (backed by the vendored `bca-tree-sitter-mozcpp`
> crate, pulled in by the opt-in `mozcpp` feature). It is a fully public,
> name-selectable language that owns no file extensions — unlike the
> `Ccomment` / `Preproc` helpers above, it is *not* internal.
