# Fuzzing

`big-code-analysis` runs [cargo-fuzz][cf] against the parse-and-walk
layer: the `fuzz/` crate holds eleven [libFuzzer][lf] targets that feed
arbitrary bytes to `Ast::parse` and then run every public walk over the
result. Coverage-guided mutation reaches malformed and adversarial input
shapes that a fixture suite samples only where someone thought to look.

[cf]: https://rust-fuzz.github.io/book/cargo-fuzz.html
[lf]: https://llvm.org/docs/LibFuzzer.html

## What is targeted, and why only this

The target set is deliberately narrow. Two pieces of evidence bound it.

**The static lints already cover the known class.** #1152 adopted
`clippy::arithmetic_side_effects` on the `loc` metric module and
`clippy::indexing_slicing` on `src/c_macro.rs`, and would have caught the
five-byte input that panicked the library (#1051) at compile time.
What a lint cannot check is the residue it could not discharge: the nine
per-function `#[allow(clippy::indexing_slicing)]` sites in
`src/c_macro.rs`, each a computed index into attacker-controlled bytes
whose bound is asserted by a human comment. That population is what
`preproc_macro` exists for.

**The encoding and IO layer is already hardened.** A 112-file
adversarial byte corpus — invalid UTF-8, NUL bytes, BOMs, mixed EOL,
unterminated constructs, 2 MB single tokens — across seven subcommands
found zero panics there. Blanket byte fuzzing would mostly re-confirm
that. Do not re-litigate it.

So the targets are the parse-and-walk layer, which that corpus exercised
only incidentally.

## The targets

Each `fuzz_targets/*.rs` is about five lines; the shared fan-out lives in
`fuzz/src/lib.rs`. Read the module doc comment on any target for its
specific rationale.

| Target | What it drives |
|---|---|
| `parse_bash`, `parse_c`, `parse_cpp`, `parse_javascript`, `parse_perl`, `parse_python`, `parse_rust`, `parse_tcl` | `Ast::parse` for one language, then the full walk fan-out |
| `preproc_macro` | `preprocess` harvest → `Source::with_preproc` → the C-family macro-masking lexer |
| `preproc_includes` | `preprocess` over several files → `fix_includes` → include-graph resolution, SCC collapse, candidate scoring |
| `nested_depth` | A structured generator for the deep-nesting complexity class |

Three design points are load-bearing.

**The bytes reach the parser unmodified.** Targets call
`Source::from_bytes`, which applies no normalisation. The file-reading
path (`read_file`, `read_file_with_eol`, `normalize_eol`) runs
`normalize_line_endings` first and guarantees a trailing newline, which
makes a node ending at EOF unreachable — exactly how #1051 survived the
test suite. A harness that normalised first would pass vacuously while
looking like coverage. `Ast::from_path` normalises; no target uses it.

**One parse, every walk.** `ops`, `dump` and comment removal are seams on
an already-built `Ast`, not separate entry points, so each target parses
once and then fans out over `metrics`, `ops`, `strip_comments`, `dump`,
`functions`, `suppressions`, `count` and `find`. It also serialises the
results, which is the only thing in the fan-out that reaches
`recursion::serialize_bounded` and `wire::map_tree` — the recursion
bounded by #1056, after nested input was found able to abort the process
from `Serialize`.

**The C grammars are instrumented too.** cargo-fuzz sanitizes the Rust
side; the tree-sitter grammars are C compiled by build scripts and are
not covered unless asked, which would leave the largest body of
memory-unsafe code in the graph outside the sanitizer. `make fuzz-check`
sets `CFLAGS_<host-triple>`. It has to be the triple-qualified spelling:
`cc` resolves `<VAR>_<target>`, then `<VAR>_<target_with_underscores>`,
then `<BUILD_KIND>_<VAR>`, then bare `<VAR>`, and picks `HOST_` rather
than `TARGET_` for the build kind when it is not cross-compiling — so
`TARGET_CFLAGS` is silently inert for a native fuzz build.

### Fuzzing an order-dependent function

`fix_includes` walks `HashMap`-ordered collections, which held
`preproc_includes` back until it was measured (#1288): a crash whose
occurrence depends on iteration order would not reproduce from its own
artifact, which is the same non-reproducibility the `-runs`-not-
`-max_total_time` rule exists to prevent.

Measured across 40 runs of one 8-file input: the diagnostic **sequence**
varied every time, while the diagnostic **set** and the mutated `files`
map were identical. So the content was already deterministic and only
the returned `Vec`'s order was not — which was also a live defect on its
own, since `bca preproc` prints that `Vec` straight to stderr and so
emitted the same warnings in a different order on every run. It is now
sorted at the source.

The general rule this leaves: before pointing a fuzzer at a function
that iterates an unordered collection, establish which of its outputs
are order-stable. If a crash can depend on the order, the artifact is
not a reproducer and the target is worse than nothing.

### Languages not fuzzed

Recorded so the omission reads as a decision. `Mozjs` was measured
metric-equivalent to upstream JavaScript on real input (#507). `Mozcpp`
and `Preproc` are driven by `preproc_macro`. The remaining grammars are
upstream crates with no evidence pointing at them; adding one is a
five-line file plus a `[[bin]]` stanza, and the bar for doing so is a
reason, not a wish for symmetry.

## Why out of band

`fuzz-check` and `fuzz-smoke` are **not** part of `make pre-commit` /
`make ci` / `make lint`. A nightly libFuzzer build under ASan costs
around a minute even warm, which does not belong in the per-commit loop —
the same call `chain-audit`, `bench-scaling` and mutation testing make.

`.github/workflows/fuzz.yml` runs them instead, and splits the two
questions fuzzing answers. A pull request touching `src/`, `fuzz/`, a
manifest, a vendored grammar or the `Makefile` builds every target and
then **replays the committed seeds** — the regression question, answered
in seconds. The quarterly cron does the **hunt**, at 200 000 runs per
target.

That split is a correction, and the numbers are why. The job first ran
11 targets x 10 000 mutations on every pull-request push: 156 s to build
and 977 s to fuzz, nine times over one pull request, 96 minutes of runner
time. A 10 000-run search is far too short to find what the cron will and
long enough to dominate the job, so the spend bought neither answer. It
also ran again on every push to `main`, re-validating a tree the pull
request had just validated.
It is a separate workflow rather than a job in `ci.yml` because
cargo-fuzz needs nightly and `ci.yml` sets `RUSTFLAGS: "-D warnings"`
workflow-wide; pairing the two turns every new nightly lint into a red X
on unrelated PRs.

One consequence is worth stating: the fuzz workflow is **not** in the
`ci` aggregator's `needs:`, so it is advisory until branch protection
names it.

## Running locally

Install once:

```bash
cargo install cargo-fuzz --locked
```

You also need an `llvm-symbolizer` on `PATH` or under
`/usr/lib/llvm-*/bin` (Debian/Ubuntu: `apt-get install llvm`; macOS: it
comes with Xcode's clang). `rustup component add llvm-tools` ships the
rest of the LLVM binutils but **not** this one.

It is a hard requirement, not a nicety: LSan matches a `leak:`
suppression against the *symbolized* stack, so without one every entry
in `fuzz/lsan-suppressions.txt` stops applying and the known
tree-sitter-perl leak fails the run. The run targets check for it up
front and refuse to start rather than let you read that as a real leak.

Then:

```bash
make fuzz-check                                   # fmt + clippy + test + build
make fuzz-smoke                                   # every target, FUZZ_RUNS each
make fuzz-run FUZZ_TARGET=preproc_macro FUZZ_RUNS=1000000
make fuzz-run FUZZ_TARGET=parse_cpp FUZZ_INPUT=fuzz/artifacts/parse_cpp/<file>
make fuzz-tmin FUZZ_TARGET=parse_cpp FUZZ_INPUT=fuzz/artifacts/parse_cpp/<file>
```

`fuzz-check` is also the crate's only `cargo fmt` gate: `cargo fmt
--all` stops at the workspace, and `fuzz/` is excluded from it.

Both run targets take `FUZZ_RUNS` (default 100 000) and `FUZZ_TIMEOUT`
(default 60 seconds per input). Every recipe skips with a printed reason
when nightly or `cargo-fuzz` is absent, so a stable-only checkout is not
blocked.

**Bound runs with `-runs`, never `-max_total_time`.** A wall-clock bound
makes a failure non-reproducible across machines of different speeds:
the same command finds a crash on one host and not on another, and
neither result can be checked.

`FUZZ_TIMEOUT` is a separate thing — a per-input limit — but it is
wall-clock too, and that limits what it can be trusted to mean. It
reliably catches an *unbounded* loop, where no timeout value would
matter. It is **not** a complexity gate, even on an idle machine: the
slowest *legitimate* input measured so far — `nested_depth`'s 5-byte
seed decoding to 508 alternating `Block`/`Lambda` levels of
JavaScript — takes **11 s** when run solo under ASan plus the harness's
ten-walk fan-out, while the 4.8 KB JavaScript file it generates costs
0.06 s through `bca metrics` in a plain release build. The default of
60 seconds is that measurement plus ~5x headroom (#1308; the previous
default of 10 failed on correct behaviour), and it still assumes the
targets are running one at a time.

Measured, because this bit a parallel sweep for #1154: `nested_depth`
reported a timeout on a 7-byte input while ten sibling targets shared 16
cores. The same input takes **2.4 s** alone under ASan, and the 3 KB
JavaScript file it decodes to — 512 levels of nested parens and arrow
functions — takes **0.07 s** through `bca metrics` in a plain release
build. There was no complexity regression; there was contention. Raise
`FUZZ_TIMEOUT` when running targets in parallel, and treat a timeout
report from a loaded machine as a question rather than a finding.

This is the same lesson as
[Benchmarking](benchmarking.md): a timing assertion on a contended host
measures the host.

## Seed corpora

`fuzz/corpus/<target>/` is checked in, which is why `fuzz/.gitignore`
omits the `corpus` line `cargo fuzz init` writes. Each per-language
directory holds one minimal valid program — carrying a comment, a
string, a call and a function, so the harness's filters have something
to match on the first iteration — plus the adversarial derivatives: no
trailing newline (the #1051 shape), CRLF and lone-CR endings, a UTF-8
BOM, invalid UTF-8, NUL bytes, an unterminated construct, and the empty
input.

Those are byte-exact, and `.gitattributes` marks the directory
`-text -diff` so git never normalises them. Several exist precisely to
carry what it would otherwise rewrite.

`nested_depth`'s seeds are different in kind: that target decodes bytes
into a generator input rather than treating them as source. Its layout
is defined by a hand-written `Arbitrary` impl — byte 0 selects the
language, bytes 1-2 are the raw depth, the rest are shapes — and the
seeds are two per language, one shallow and one at the depth cap.

The impl is hand-written for exactly this reason. Under the derive, the
layout is an implementation detail of the `arbitrary` crate, the seeds
cannot be written deliberately, and the first twelve written against it
all decoded to Rust — a corpus that read as spanning four languages and
covered one. `seeds_cover_every_language` now asserts the decoded set,
which is only checkable because the layout is ours.

**Runs do not write here.** libFuzzer writes every coverage-increasing
input it finds into the *first* corpus directory it is handed and treats
later ones as read-only. The recipes therefore pass
`fuzz/corpus-work/<target>` first and `fuzz/corpus/<target>` second, so
discoveries land in a gitignored directory. Pointed at the seeds
directly — which is cargo-fuzz's default — one 5 000-run pass over ten
targets grew 93 checked-in seeds to over 4 000 untracked files, and
every local run would leave the tree dirty.

Promoting a discovery to a committed seed is therefore a deliberate `cp`,
which is the right bar: seeds are read by every future contributor and
should each be explicable.

`bca` never analyses either directory: `.bcaignore` excludes them from
the walk, because deliberately malformed source measures nothing and
would put ERROR-node parses into `bca report`.

## Triaging a crash

libFuzzer writes the offending input to `fuzz/artifacts/<target>/`. The
CI workflow uploads that directory as the `fuzz-artifacts` artifact, and
the quarterly cron opens an issue.

1. Reproduce:
   `make fuzz-run FUZZ_TARGET=<target> FUZZ_INPUT=fuzz/artifacts/<target>/<file>`.
2. Minimise:
   `make fuzz-tmin FUZZ_TARGET=<target> FUZZ_INPUT=fuzz/artifacts/<target>/<file>`.
3. Fix the defect.
4. **Commit the minimised input as a corpus seed *and* write a normal
   `#[test]`.** The test is the durable half: it enforces the regression
   on every `cargo test`, without anyone running the fuzzer again. Put it
   next to the code that broke, with the bytes inlined as a literal.

Go through the make targets rather than calling `cargo +nightly fuzz
run` / `tmin` directly. They carry the `CFLAGS_<host-triple>` /
`CXXFLAGS_<host-triple>` settings described above, and `cc` emits
`cargo:rerun-if-env-changed` for those variables — so a bare invocation
rebuilds every tree-sitter grammar *without* ASan, which is precisely
the instrumentation that found the C-scanner class in the first place.

They also pass `--target <host-triple>` explicitly, which matters more
than it looks. cargo-fuzz defaults its target to the triple *it* was
built for, not the host's: `cargo install cargo-fuzz` gives you the
host, while a binstall-backed install — what `taiki-e/install-action`
does in the workflow — hands you the prebuilt musl artifact, which then
builds the targets for a statically-linked libc the sanitizer refuses.
Pinning it also keeps the CFLAGS triple and the build triple the same
by construction; keyed to different triples the instrumentation attaches
to nothing and the build still succeeds.

Step 4 does not fit every finding, and the exceptions are worth naming.

**A leak is invisible to `cargo test`**, because nothing outside a
sanitizer build observes it. For those the durable artifact is the seed
plus an entry in `fuzz/lsan-suppressions.txt` explaining the diagnosis.

**A crash you cannot fix yet must not be committed as a seed.** Step 4
assumes the defect gets fixed in the same change. When it is upstream in
a pinned grammar with no release to move to — #1289 is the live example,
a `SIGSEGV` in tree-sitter-bash's scanner — committing the reproducer
makes every future run, local and CI, die on it immediately and stay
red. Record the bytes in the issue instead; the seed and the test land
with the fix.

**A `SIGSEGV` cannot be a normal `#[test]` either.** It kills the test
process rather than failing an assertion, so the regression test has to
drive a subprocess and assert it does not die by signal.

### Leak suppressions

Leak detection stays on. `fuzz/lsan-suppressions.txt` currently holds one
entry, and adding a second should feel expensive: every suppression is a
leak nothing will report again. Suppress by symbol rather than passing
`-detect_leaks=0`, so other leaks on the same path still fail.

## Definition of done

Coverage-guided fuzzing has no natural stopping point, so #1154 fixed
one before the crate was written: `fuzz/` exists, builds in CI, ships
seed corpora, and has either found a real bug or returned clean at
`-runs=1000000` per target. Beyond that, fuzzing here is maintenance
rather than project work — the quarterly cron and the per-PR compile
gate carry it.
