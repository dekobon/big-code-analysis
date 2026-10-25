# Commands

`bca` is the command-line tool. Each subcommand performs one analysis
and takes its own flags. This section covers what every subcommand
shares: installation, how flags and input paths are placed, which files
the walk visits, and the exit-code contract. The subcommands themselves
each have a page: [Metrics](metrics.md), [Change-history (VCS)
metrics](vcs.md), [Report](report.md), [Check](check.md), [Suppression
markers](suppression.md), [Nodes](nodes.md), the [REST API](rest.md),
and [Operating bca-web](web-server.md).

## Installation {#installation}

The `bca` command-line tool ships as a
[pip](https://pip.pypa.io/en/stable/)-installable wheel. The
distribution name is `big-code-analysis-cli` and the installed command
is `bca`. The two differ on purpose: the `bca` name on
[PyPI](https://pypi.org/) belongs to an unrelated project, and
`big-code-analysis` is this project's importable *library* bindings.

```bash
pip install big-code-analysis-cli   # installs the `bca` command on PATH
bca --version
```

This puts the compiled `bca` binary on your `PATH` the way
`pip install ruff` gives you the [`ruff`](https://docs.astral.sh/ruff/)
command, with no Rust toolchain required. The wheel carries the full `all-languages` grammar set, so
every [supported language](../languages.md) works out of the box. A
single `py3-none-<platform>` wheel covers every CPython 3.x (and PyPy)
on that platform. Prebuilt wheels ship for Linux (`manylinux_2_28`
`x86_64` / `aarch64`), macOS (`x86_64` / `arm64`), and Windows
(`x86_64`). On any other platform `pip` falls back to a source build,
which needs a Rust toolchain.

This is the binary CLI, distinct from the importable
[Python bindings](../python/installation.md)
(`pip install big-code-analysis`). The repository README describes the
other install paths: [Homebrew](https://brew.sh/), `.deb` / `.rpm` /
`.apk` packages,
prebuilt release archives, and `cargo install big-code-analysis-cli`.

The wheel build and publish matrix is defined in
[`.github/workflows/python-cli-wheels.yml`](https://github.com/dekobon/big-code-analysis/blob/main/.github/workflows/python-cli-wheels.yml).

## Metrics {#metrics}

Metrics are quantitative measures of source code. They help to:

- compare programming languages;
- describe the quality of a codebase;
- show developers where their code is hardest to handle;
- surface potential problems early in development.

`bca` computes its metrics from the source text alone, without running
the program; such metrics are called *static metrics*. See
[Metrics](metrics.md) for the command and
[Supported Metrics](../metrics.md) for each metric's definition.

## Nodes {#nodes}

To represent the structure of program code, `bca` builds an
<a href="https://en.wikipedia.org/wiki/Abstract_syntax_tree" target="_blank">Abstract Syntax Tree (AST)</a>.
A **node** is an element of this tree and denotes one syntactic
construct of the language. The [Nodes](nodes.md) commands use them to:

- print the syntactic structure of a source file;
- find whether a construct is present in the analyzed code;
- count the constructs of a given kind;
- detect syntax errors in the source.

## REST API {#rest-api}

`bca-web` runs a server offering a [REST API](rest.md). Clients send
source code over HTTP and receive the corresponding metrics as JSON.

## Flag placement and input paths {#flag-placement-and-input-paths}

Most subcommands read the input they analyze as a trailing positional
path, so the common case reads like every other code tool
([`tokei`](https://github.com/XAMPPRocky/tokei),
[`cloc`](https://github.com/AlDanial/cloc),
[`scc`](https://github.com/boyter/scc),
[`rg`](https://github.com/BurntSushi/ripgrep)). The exceptions:
`report` and `vcs` select input with `--paths`, `diff` compares two
result sets, and `init` targets a directory via `--dir`.

```bash
bca metrics src/            # analyze the src/ tree
bca check src/ tests/       # gate two subtrees
bca find -t function_item . # find every function in the current tree
```

Flags are **scoped to the subcommand that consumes them** and must be
written **after** the subcommand token:

```bash
bca metrics --exclude '*.generated.rs' src/   # correct
bca --exclude '*.generated.rs' metrics src/   # ERROR (exit 1)
```

Only `-w` / `--warnings` and `--report-skipped` are universal and accepted
in any position. Every other shared flag lives in a help-grouped section
on the subcommands that read it:

- *Input selection*: `-p` / `--paths`, `-I` / `--include`, `-X` /
  `--exclude`, `-l` / `--language`, `--paths-from`, `--exclude-from`,
  `--no-ignore`, `--no-skip-generated`, `--no-config`.
- *Walker tuning*: `-j` / `--jobs`, `--exclude-tests`,
  `--cyclomatic-count-try`, `--mi-charges-docs`.
- *Preprocessor*: `--preproc-data`.
- *Output*: `--color`.

A flag passed to a subcommand that never consumes it is a hard usage
error (exit 1) rather than a silent no-op, so
`bca vcs commit --exclude-tests` and `bca list-metrics --paths` both
error, and `bca list-metrics --help` does not advertise walker flags.

The `-p` / `--paths` flag still works and is **unioned** with the
positional paths, so `bca metrics a.rs --paths b.rs` walks both. The
[`find`](nodes.md) and [`count`](nodes.md) subcommands take their node
kinds via a repeatable `-t` / `--type` flag (so the positional slot is
free for paths): `bca find -t function_item -t struct_item src/`.

## Skipping generated code {#skipping-generated-code}

Generated files inflate metrics for code no human will refactor. By
default, `bca` scans the first ~50 lines / 5 KiB of each file for a
generated-code marker and skips matches **before** parsing, so the
skipped file pays no
[tree-sitter](https://tree-sitter.github.io/tree-sitter/) parse cost.

Recognized markers (case-insensitive):

- `@generated`: a marker emitted by code generators.
- `DO NOT EDIT`: a marker used by
  [Go's generated-code convention](https://go.dev/s/generatedcode) and
  other generators.
- `GENERATED CODE`: [Lizard](https://github.com/terryyin/lizard)'s
  marker, recognized for compatibility.

A marker phrase that appears only deep in the file body (past the scan
window) does **not** trigger the skip; the detector deliberately looks
only at the file header.

The skip applies uniformly to `bca metrics`, `bca report`, and the
threshold engine.

### Flags {#flags}

- `--no-skip-generated`: disable the auto-skip, so every file is
  parsed.
- `--report-skipped`: log `skipped (generated): <path>` to stderr for
  each file the detector excludes, so you can audit the exclusions. If
  a file is wrongly tagged, pass `--no-skip-generated`; an include
  filter does not override the detector.

`bca check` additionally prints a one-line stderr summary whenever the
detector dropped files from a gate run (`bca: 1 file not checked
(1 generated) — pass --report-skipped to list them`), and its
[`--strict` profile](check.md#strict-mode) disables the detector
outright. A marker is content the branch under test controls, so a
pull-request gate should not let it shrink the checked set.

## Respecting `.gitignore` {#respecting-gitignore}

When a directory is passed to `--paths`, `bca` walks it with
`.gitignore` awareness by default. Files matched by any of the
following are skipped before parsing:

- `.gitignore` files inside the walked tree.
- `.ignore` files (the [ripgrep](https://github.com/BurntSushi/ripgrep)
  / [`fd`](https://github.com/sharkdp/fd) convention).
- `.git/info/exclude`.
- The global gitignore (`~/.config/git/ignore`, or whatever
  `core.excludesFile` points at).
- `.gitignore` files in ancestor directories of the seed (so
  `bca metrics src/` from a project root picks up the project's
  top-level `.gitignore`).

The walker honors `.gitignore` even outside a checked-in git
repository, so an extracted source tarball with a `.gitignore` file
gets the same treatment as a fresh `git clone`.

Hidden files (those whose basename starts with `.`) are filtered
during the walk.

### Explicit paths bypass the filter {#explicit-paths-bypass-the-filter}

Files passed by name (via `--paths`, `--paths-from`, or a trailing
positional path) are always analyzed, even when the project's ignore
rules cover them. This makes it safe to run `bca metrics --paths-from -`
from a `git diff --name-only` pipeline without losing files that happen
to match a wildcard ignore rule.

The override is deliberate, and it is the same rule `rg` and `fd`
apply: a path you named is a direct request. It spans every deny-set
the walk consults: `.gitignore` and friends, `-X` / `--exclude`,
`--exclude-from`, a `.bcaignore`, and a manifest `exclude` list alike.
`rg --glob '!x.txt' pattern x.txt` searches `x.txt` for exactly this
reason.

Two boundaries on it:

- **`-I` / `--include` still applies.** The allow-list narrows *which*
  named files are analyzed, so `bca metrics -I '*.rs' notes.md` analyzes
  nothing. Only the deny-sets are overridden.
- **`[check] exclude` still applies.** The gate-exemption set
  (`--check-exclude` / `--check-exclude-from`, or `exclude` under
  `[check]` in `bca.toml`) is not a walk filter: it drops *violations*,
  not files. It therefore survives an explicit path and reports
  `bca: skipped N violations via [check.exclude]` when it fires. A glob
  written in `bca.toml` resolves against the manifest's directory
  whichever directory you run from (#1164); one passed as
  `--check-exclude` resolves against the working directory. See
  [What a relative glob is relative to](check.md#what-a-relative-glob-is-relative-to).

That second point is the one to reach for. A walker exclude shapes what
gets **analyzed**; a check exclude shapes what gets **gated**. Anything
you want kept out of the threshold gate permanently (dev tooling,
generated code you still want measured, a subtree under active
rewrite) belongs in `[check] exclude`, because any caller that names
paths one at a time bypasses the walker excludes by design. Per-file
callers are not hypothetical: that is the shape the
[agent feedback hooks](../recipes/agent-feedback.md) use on every edit.

When an explicitly named path does override a walker exclude, `bca`
says so on stderr and names the glob:

```text
warning: utils/gate.py matches an exclude pattern (./utils/**) but was named explicitly; analyzing anyway
```

### Path discovery flags {#path-discovery-flags}

- `--no-ignore`: disable `.gitignore` / `.ignore` / global-gitignore
  awareness when expanding directory seeds. `bca check` counts the
  analyzable files ignore rules dropped from a gate run in its
  not-checked summary, lists every dropped entry under
  `--report-skipped` (`note: skipped (ignored): <path>`,
  `note: skipped (ignored directory): <path>`), and disables the
  awareness under its [`--strict` profile](check.md#strict-mode),
  since an ignore file committed in the branch under test otherwise
  shrinks the checked set.
- `--paths-from <FILE>`: read newline-separated input paths from
  `<FILE>`, or from stdin when `<FILE>` is `-`. Combined as a union
  with any `--paths` values. `-I` globs still apply; `-X` globs do not
  reach an entry that names a file directly, per
  [Explicit paths bypass the filter](#explicit-paths-bypass-the-filter).
  Blank lines are skipped; `#` is treated as a path character (not a
  comment). To pass a file literally named `-`, write `./-`.
- `--exclude-from <FILE>`: read newline-separated `--exclude` glob
  patterns from `<FILE>`, or from stdin when `<FILE>` is `-`.
  Patterns are unioned with any inline `--exclude` / `-X` values
  into a single deny-set; order does not matter. `.gitignore`-style:
  blank lines and lines whose first non-whitespace character is `#`
  are skipped, and a leading UTF-8 BOM is stripped. Convention is a
  `.bcaignore` at the repo root, mirroring `.gitignore` /
  `.dockerignore`. To pass a file literally named `-`, write `./-`.

## Exit codes {#exit-codes}

`bca` follows one exit-code convention across every subcommand, so CI
scripts can branch on the process status without inspecting output:

| Code | Meaning |
|------|---------|
| `0`  | Success. |
| `1`  | Tool error: a bad flag / threshold / glob spec, unreadable input, or a parse failure. This includes **usage errors** (unknown flag, bad subcommand, a malformed `--threshold` value rejected by clap). **Never** a metric signal. |
| `2`  | Metric gate: [`check`](check.md) thresholds were exceeded, [`vcs commit --fail-above`](vcs.md) was breached, or [`diff`](../recipes/exporting-data.md) / [`diff-baseline`](../recipes/baselines.md) under `--exit-code` found a non-empty filtered diff. |
| `3`–`5` | [`check --exit-codes=tiered`](check.md#tiered-exit-codes---exit-codestiered) only: tiered violation severity (regression-only / mixed / hard-breach; in tiered mode code `2` means new-only). |

Codes `2`–`5` are gate signals, emitted only by [`check`](check.md),
[`vcs commit --fail-above`](vcs.md), and `diff` / `diff-baseline` under
the opt-in `--exit-code` flag; they report a metric result, not a
failure of the tool. Every other subcommand (`metrics`, `ops`,
`report`, `diff`, `diff-baseline`, `exemptions`, `init`, and the rest)
exits `0` on success and `1` on error. Code `1` is reserved for tool
errors, usage errors included, so a mistyped flag never lands in the
gate band and CI can always tell "the gate found a regression"
(`2`–`5`) from "the tool itself failed" (`1`).

### Unreadable input {#unreadable-input}

Every source-walking subcommand (`metrics`, `ops`, `report`,
`functions`, `find`, `count`, `dump`, `exemptions`, `preproc`,
`strip-comments`, `check`) exits `1` if any selected input file cannot
be read: permission denied, or a path that vanished between the walk
and the read. This includes `init`, which uses `check` to scaffold its
baseline, and `diff --since`. Each failure names the path on stderr as
`error processing <path>: …`, followed by a summary.

The rule is "any read failure", not "no output at all", because a
missing file is invisible in the result. A partial `metrics --output`
document, a `report`, or a `count` looks complete; a `diff --since`
side that lost a file reports it as *added* or *removed* rather than as
the I/O error it was. The exit code is the only place the failure can
be observed, so it has to carry it.

Output *streamed during* the walk is still emitted, because the check
runs after it. A mixed run therefore still shows the files that were
read: the stdout trees of `metrics` / `ops` / `dump` / `find` /
`functions`, and the per-file documents of `--output-dir`.

Output *assembled after* the walk is suppressed entirely, because a
partial one is indistinguishable from a complete one. That covers the
`metrics --output` / `ops --output` aggregate document, the `report`
document, `count`'s tally, `preproc`'s JSON, and the `exemptions`
report; none of them is printed or written.

If a tree legitimately contains files you cannot open, prune them with
`--exclude` (or `--include` a narrower set). A file removed by those
filters is never opened, so it is never a read failure.

### Unlistable directories {#unlistable-directories}

A directory the walk cannot *list* is the same failure one level up, and
carries the same exit `1`. Its whole subtree drops out of the analysed
set before any file is selected, so every count downstream (the metrics
document, the `count` tally, a `diff --since` side, `vcs rank`'s
ranking) is short by an amount nothing in the output reveals. `bca
check` is the worst case: a gate that reports clean on a tree it could
not read is indistinguishable from a gate that passed.

Each unlistable entry warns on stderr as `warning: skipping walk
entry in …`. The walk continues, so one bad directory does not take
down the rest of the tree, and the run ends with a summary line and
exit `1`.

To exempt a directory you knowingly cannot list, name it in an ignore
file (`.gitignore`, `.ignore`) or narrow `--paths` so the walk never
reaches it. **`--exclude` does not work here**, though it is the right
answer for an unreadable *file*: `--exclude` filters the paths the walk
yielded, and a directory that could not be listed yielded none, because
the failure happened before the filter could apply.

Two neighbouring cases stay non-fatal by design:

- **A malformed ignore file, or a pattern in one that will not compile.**
  The walker reports these through the same channel and they warn
  identically, but they describe how the walk was *configured* rather
  than files it lost. Only errors carrying an underlying I/O error are
  counted, so a stray `.gitignore` typo cannot fail a build.
- **A broken symlink discovered by walking.** It is dropped for not
  being a regular file and never surfaces as an error at all. The walk
  does not follow links, so it deliberately does not resolve symlinks
  and has nothing to report. Treating one as fatal would make a stale
  symlink in a vendored tree a hard CI failure.

An explicitly *named* path is the exception to that last point, and a
pre-existing one: `--paths` resolves a symlink seed once, and a seed
that does not exist (a dangling link or a typo) is its own error, also
exit `1`.

### Unwritable output {#unwritable-output}

The mirror image is the same rule, and it holds for every emission
path: a run whose output could not be written exits `1`. That covers a
per-file document under an unwritable `--output-dir`, and a full disk
on stdout: `dump`'s banners and trees, `find`'s matches,
`strip-comments`' rewritten source, `count`'s tally, `preproc`'s JSON,
and the single-document reports from `vcs`, `vcs commit`, and `vcs
trend` alike, in every format. A per-file failure is named on stderr
and counted in a summary line; output assembled after the walk reports
the operating system's error directly.

The one exemption is a closed downstream pipe: `bca dump | head` is
routine rather than a failure, so `BrokenPipe` is swallowed and the run
still exits `0`.
