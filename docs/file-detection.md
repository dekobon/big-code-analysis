# File and language detection

How `big-code-analysis` decides which language a file is written in, and
what it reads off disk before parsing. All of the logic lives in
[`src/tools.rs`](../src/tools.rs), [`src/langs.rs`](../src/langs.rs),
and the macros in [`src/macros/mod.rs`](../src/macros/mod.rs).

## Reading the file

Three public readers normalise input before any detection or parsing
happens:

| Function | Behaviour |
|----------|-----------|
| `read_file(path)` | Reads the whole file. Normalises CRLF and lone CR to LF in place. Buffer ends with exactly one trailing `\n`. |
| `read_file_with_eol(path)` | Same normalisation, plus: returns `None` for files ≤ 3 bytes and for files with a UTF-16 BOM (#803); strips a leading UTF-8 BOM; byte-validates the first ~64 bytes with `std::str::from_utf8` and returns `None` on invalid UTF-8 (#746, #758; a lossy U+FFFD check was rejected because U+FFFD can legitimately appear in source). |
| `read_file_with_eol_classified(path)` | The same read, with the skip *named*: `Ok(Err(SkipReason))` in place of `Ok(None)`. `read_file_with_eol` is this function's `.map(Result::ok)`. |

`SkipReason` is `#[non_exhaustive]` with one variant per gate above —
`Empty`, `TooSmall`, `Utf16Bom`, `NotUtf8` — and its `Display` renders a
noun phrase a front-end can splice into a diagnostic (`skipping {reason}:
{path}`). Prefer it wherever the skip is reported to a user: the
undifferentiated `None` is why `bca` announced a multi-kilobyte binary
and a one-byte source alike as "skipping empty file" (#1287).

Downstream consumers must assume the buffer contains no `\r` bytes. The
metric engine depends on this: passing raw CRLF input to a parser
would shift line numbers and break LoC counts.

## Detecting the language

There are two public entry points, both returning a
[`LANG`](../src/langs.rs) variant:

### `get_language_for_file(path)`: extension only

Lowercases the file's extension and looks it up. Returns `None` if the
path has no extension, the extension is not valid UTF-8, or no language
claims that extension. This is the cheap path; no buffer required.

### `guess_language(buf, path)`: extension + mode line + shebang

Combines the extension lookup with an Emacs/Vim *mode line* scan of the
buffer and a shebang scan of the first line. Returns `(Option<LANG>,
&str)` where the second element is the canonical lowercase language slug
(`"cpp"`, `"csharp"`, `"rust"`, etc.), the same string `LANG::name`
emits and a valid `FromStr` lookup token. Objective-C (`.m`) parses with
the dedicated `tree-sitter-objc` grammar and reports `"objc"`;
Objective-C++ (`.mm`) stays on the C++ grammar and reports `"cpp"` (see
[Objective-C and Objective-C++](#objective-c-and-objective-c) below).

Mode line scanning runs three regexes (compiled once via `OnceLock`):

| Regex | Matches | Example |
|-------|---------|---------|
| `(?i)-\*-.*[^-\w]mode\s*:\s*([^:;\s]+)` | Emacs `mode:` declaration | `// -*- mode: c++ -*-` |
| `-\*-\s*([^:;\s]+)\s*-\*-` | Bare Emacs mode | `// -*- c++ -*-` |
| `(?i)vim\s*:.*[^\w]ft\s*=\s*([^:\s]+)` | Vim `ft=` modeline | `// vim: set ts=4 ft=c++` |

The scan checks the **first 5 lines** for any of the three patterns,
then the **last 5 lines** for the Vim pattern only (Vim modelines are
conventionally at the bottom of the file, Emacs ones at the top).
"Lines" here means LF-delimited segments; `guess_language` relies on
the CRLF/CR → LF normalisation performed by the readers above.

#### Resolution rules

Given an extension result and a mode result, `guess_language` resolves
as follows:

1. **Both agree**: return that language.
2. **Both disagree**: extension wins. The mode line is treated as
   advisory; the display name comes straight from the extension's
   `LANG`.
3. **Only extension matches**: return it.
4. **Only mode matches**: return it.
5. **Only shebang matches**: return it. The shebang signal is
   consulted **after** the extension and mode-line lookups have both
   come back empty, so an explicit `.py` extension or `mode: python`
   line on a script with `#!/bin/sh` still resolves to Python.
6. **Nothing matches**: return `(None, "")`.

Each fallback is evaluated lazily: because the extension wins rules 1–3
outright, the mode-line scan runs only when the extension resolves to
nothing, and the shebang scan only when the mode line does too (#1111).
The resolution order above is the observable contract; the skipped work
cannot change an answer.

#### Shebang scan

The shebang scan handles extensionless scripts whose interpreter is
unambiguous. It triggers only when the buffer starts with `#!` and
recognises both the bare (`#!/bin/bash`) and `env`-wrapped
(`#!/usr/bin/env python3`) forms. For `env`-wrapped shebangs, leading
`-FLAG` tokens (including `-S`) and `NAME=value` assignments are
skipped to find the actual interpreter. Trailing version digits and
dots are stripped (`python3` → `python`, `lua5.1` → `lua`,
`perl5.36` → `perl`) before lookup.

| Interpreter basename | LANG |
|----------------------|------|
| `sh`, `bash`, `dash`, `ksh`, `zsh` | `Bash` |
| `python`, `python2`, `python3` | `Python` |
| `perl`, `perl5` | `Perl` |
| `lua`, `lua5.x`, `luajit` | `Lua` |
| `php`, `php-cgi` | `Php` |
| `node`, `nodejs` | `Javascript` |
| `tclsh`, `wish` | `Tcl` |

A non-UTF-8 shebang line yields `None` (no panic). Anything other
than the interpreters above is unrecognised and falls through to the
final `(None, "")` result.

### Objective-C and Objective-C++

Since #724, Objective-C has its own dedicated grammar:

- **`.m` (and the `objc` / `objective-c` Emacs modes)** → `LANG::Objc`,
  the upstream `tree-sitter-objc` grammar. Objective-C is a strict
  superset of C, so `tree-sitter-objc` parses `.m` files fully and
  reports the `"objc"` slug.
- **`.mm` (and the `objc++` / `objective-c++` Emacs modes)** →
  `LANG::Cpp`, reporting `"cpp"`. Objective-C++ mixes Objective-C and
  C++; the `tree-sitter-objc` grammar parses the Objective-C and C
  parts but not C++ constructs (templates, namespaces, classes, `::`).
  Because a `.mm` file is `.mm` precisely because it contains C++ (the
  larger, more pervasive surface), the C++ grammar degrades more
  gracefully on `.mm` than the Objective-C grammar would (it only trips
  on the Objective-C glue, not the whole C++ half). This is the same
  asymmetric-failure trade-off #721 used to keep `.h` on `LANG::Cpp`.
  Metrics for the Objective-C portions of a `.mm` file are therefore
  approximate, a known limitation tracked in the original #724 decision.

The earlier `fake::get_true` overlay, which folded both extensions onto
the `"cpp"` slug while they shared one grammar (#540), was retired in
the #724 change: `.m` now reports `"objc"` natively and `.mm` reports
`"cpp"` natively, so no override is needed.

## Where the extension and mode tables come from

The per-language extension list and Emacs mode list are declared as the
last two tuple fields of each `mk_langs!` entry in
[`src/langs.rs`](../src/langs.rs):

```rust
(
    Rust,
    "The `Rust` language",
    "rust",
    RustCode,
    RustParser,
    tree_sitter_rust,
    [rs],          // <-- file extensions
    ["rust"]       // <-- emacs modes
),
```

The `mk_extensions!` and `mk_emacs_mode!` macros in
[`src/macros/mod.rs`](../src/macros/mod.rs) expand these into the public
`get_from_ext(ext) -> Option<LANG>` and
`get_from_emacs_mode(mode) -> Option<LANG>` lookup functions. Both are
plain `match` arms: no fuzzy matching, no fallback.

To add or change an alias, edit the `mk_langs!` invocation; the
generated lookups update automatically.

## How callers use detection

- **Library**: `guess_language` is the standard entry point; the CLI
  and REST server both go through it. `get_language_for_file` is
  available for callers that have only a path.
- **CLI (`bca`)**: auto-detects via `guess_language`
  unless the user passes `--language <name>` (short form `-l`; the old
  `--language-type` spelling stays as a hidden alias for one release
  cycle). The flag value is resolved by trying `LANG`'s `FromStr`
  (canonical name, e.g. `rust`) first, then `get_from_ext` (extension,
  e.g. `rs`), with an `Action::PreprocProduce` short-circuit. An
  unrecognised value is a hard error (exit 1) that lists the valid
  language names; it no longer silently disables analysis. See
  [`big-code-analysis-cli/src/main.rs`](../big-code-analysis-cli/src/main.rs).
- **REST (`bca-web`)**: every endpoint that takes a path plus
  buffer calls `guess_language` to resolve the language before
  dispatching to a parser.
- **Tests**: `tests/common/mod.rs` falls back to `guess_language` when
  the test harness cannot infer the language another way.

## Detection failures

If `guess_language` returns `(None, _)`:

- The CLI logs and skips the file.
- The REST server returns an error to the caller.
- The library leaves it to the caller; there is no default parser.

Beyond the shebang scan described above, there is no content-based
heuristic and no MIME sniffing. Add a missing extension or Emacs mode
to `mk_langs!` rather than working around it at the call site, and
extend the shebang interpreter table in `src/tools.rs` if a new
script interpreter needs to be recognised.
