# Fuzzing

`big-code-analysis` runs [cargo-fuzz][cf] against the parse-and-walk
layer. The `fuzz/` crate holds [libFuzzer][lf] targets that feed
arbitrary bytes to the parser and exercise its walks and source-documentation
collector. Coverage-guided mutation reaches malformed and adversarial input
shapes that a fixture suite samples only where someone thought to look.

This page is for contributors running or extending the fuzz targets. It
starts with running them and triaging a crash; the target design and the
reasons the job runs outside `make pre-commit` follow.
It assumes familiarity with [Cargo](https://doc.rust-lang.org/cargo/) and the project's
test commands. A fuzz target is a function that libFuzzer repeatedly calls with mutated bytes.

[cf]: https://rust-fuzz.github.io/book/cargo-fuzz.html
[lf]: https://llvm.org/docs/LibFuzzer.html

## Running locally

Install once (cargo-fuzz needs a nightly toolchain):

```bash
cargo install cargo-fuzz --locked
```

You also need an
[`llvm-symbolizer`](https://llvm.org/docs/CommandGuide/llvm-symbolizer.html)
on `PATH` or under
`/usr/lib/llvm-*/bin` (Debian/Ubuntu: `apt-get install llvm`; macOS: it
comes with Xcode's [Clang](https://clang.llvm.org/)).
[`rustup`](https://rust-lang.github.io/rustup/) component `llvm-tools` ships the
rest of the LLVM binutils but **not** this one.

It is a hard requirement, not a nicety:
[LeakSanitizer](https://clang.llvm.org/docs/LeakSanitizer.html) (LSan)
matches a `leak:` suppression against the *symbolized* stack, so
without one every entry
in `fuzz/lsan-suppressions.txt` stops applying and the known
tree-sitter-perl leak fails the run. The run targets check for it up
front and refuse to start rather than let you read that as a real leak.

Then:

```bash
make fuzz-check                                   # fmt + clippy + test + build
make fuzz-replay                                  # committed corpus, no mutation
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

For a controlled run, `FUZZ_CORPUS` selects the read-only corpus root, with one subdirectory per
target. `FUZZ_WORK` selects the writable root. Both `fuzz-run` and `fuzz-replay` pass the writable
directory first. `FUZZ_ARGS` adds shell-quoted libFuzzer arguments after `--`; quote an argument
containing spaces within that value. Recipe-owned `-runs` and `-timeout` arguments come last.
Record the expanded argument list, including duplicate flags, when verifying a campaign's limits.

The recipes can skip with exit 0 when tools are absent. Qualification controllers must also require
the actual target binary and final `stat::number_of_executed_units` output. A bounded randomized
run must execute its requested unit count; a replay must execute its complete seed inventory.
Missing completion evidence is an incomplete stage.

**Bound runs with `-runs`, never `-max_total_time`.** A wall-clock bound
makes a failure non-reproducible across machines of different speeds:
the same command finds a crash on one host and not on another, and
neither result can be checked.

`FUZZ_TIMEOUT` is a separate thing, a per-input limit, but it is
wall-clock too, and that limits what it can be trusted to mean. It
reliably catches an *unbounded* loop, where no timeout value would
matter. It is **not** a complexity gate, even on an idle machine: the
slowest *legitimate* input measured so far (`nested_depth`'s 5-byte
seed decoding to 508 alternating `Block`/`Lambda` levels of
JavaScript) takes **11 s** when run solo under
[AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer.html)
(ASan) plus the harness's walk fan-out, while the 4.8 KB JavaScript
file it generates costs
0.06 s through `bca metrics` in a plain release build. The default of
60 seconds is that measurement plus ~5x headroom (#1308; the previous
default of 10 failed on correct behaviour), and it still assumes the
targets are running one at a time.

Measured, because this bit a parallel sweep for #1154: `nested_depth`
reported a timeout on a 7-byte input while ten sibling targets shared 16
cores. The same input takes **2.4 s** alone under ASan, and the 3 KB
JavaScript file it decodes to (512 levels of nested parens and arrow
functions) takes **0.07 s** through `bca metrics` in a plain release
build. There was no complexity regression; there was contention. Raise
`FUZZ_TIMEOUT` when running targets in parallel, and treat a timeout
report from a loaded machine as a question rather than a finding.

This is the same lesson as
[Benchmarking](benchmarking.md): a timing assertion on a contended host
measures the host.

## Seed corpora

`fuzz/corpus/<target>/` is checked in, which is why `fuzz/.gitignore`
omits the `corpus` line `cargo fuzz init` writes. Each per-language
directory holds one minimal valid program (carrying a comment, a
string, a call and a function, so the harness's filters have something
to match on the first iteration) plus the adversarial derivatives: no
trailing newline (the #1051 shape), CRLF and lone-CR endings, a UTF-8
BOM, invalid UTF-8, NUL bytes, an unterminated construct, and the empty
input.

Those are byte-exact, and `.gitattributes` marks the directory
`-text -diff` so git never normalises them. Several exist precisely to
carry what it would otherwise rewrite.

The `documentation` corpus uses a
[tagged format](#documentation-properties) instead of raw source alone.
`fuzz/documentation-seeds.json` records each header, payload
[SHA-256](https://csrc.nist.gov/pubs/fips/180-4/upd1/final),
fixture provenance, and executed assertion class. Metadata stays outside the
corpus so libFuzzer never treats a registry file as a source seed.
`python3 -B fuzz/generate_documentation_seeds.py` regenerates the deliberate
seeds. Its inputs are the raw documentation fixtures and explicitly written
edge cases; it never obtains expected values from collector output.

`nested_depth`'s seeds are different in kind: that target decodes bytes
into a generator input rather than treating them as source. Its layout
is defined by a hand-written [`Arbitrary`](https://docs.rs/arbitrary)
impl (byte 0 selects the language, bytes 1-2 are the raw depth, the
rest are shapes) and the seeds are two per language, one shallow and
one at the depth cap.

The impl is hand-written for exactly this reason. Under the derive, the
layout is an implementation detail of the `arbitrary` crate, the seeds
cannot be written deliberately, and the first twelve written against it
all decoded to Rust: a corpus that read as spanning four languages and
covered one. `seeds_cover_every_language` now asserts the decoded set,
which is only checkable because the layout is ours.

**Runs do not write here.** libFuzzer writes every coverage-increasing
input it finds into the *first* corpus directory it is handed and treats
later ones as read-only. The recipes therefore pass
`fuzz/corpus-work/<target>` first and `fuzz/corpus/<target>` second, so
discoveries land in a gitignored directory. Pointed at the seeds
directly, which is cargo-fuzz's default, one 5 000-run pass over ten
targets grew 93 checked-in seeds to over 4 000 untracked files, and
every local run would leave the tree dirty.

Promoting a discovery to a committed seed is therefore a deliberate `cp`,
which is the right bar: seeds are read by every future contributor and
should each be explicable.

For `documentation`, register the seed's header, payload, provenance, and assertion
class in `fuzz/generate_documentation_seeds.py`. Add or reuse the matching assertions
in `fuzz/src/documentation/tests.rs`, then regenerate the corpus and registry:

```bash
python3 -B fuzz/generate_documentation_seeds.py
cargo +nightly test --manifest-path fuzz/Cargo.toml --locked --lib documentation::tests
```

Copying a discovery into this corpus alone is insufficient: the generator rejects
unregistered files, and registration tests require every seed to execute its assertion class.
The Rust recovery seeds form a fixed valid/missing-expression pair. Both retain the same
documented neighboring function and assert its unchanged owner spelling and text.
That constructed oracle does not claim documentation parity under arbitrary source edits.

`bca` never analyses either directory: `.bcaignore` excludes them from
the walk, because deliberately malformed source measures nothing and
would put ERROR-node parses into `bca report`.

## Triaging a crash {#triaging-a-crash}

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
`CXXFLAGS_<host-triple>` settings described under
[The targets](#the-targets), and [`cc`](https://docs.rs/cc) emits
`cargo:rerun-if-env-changed` for those variables, so a bare invocation
rebuilds every [tree-sitter](https://tree-sitter.github.io/tree-sitter/) grammar
*without* ASan, which is precisely
the instrumentation that found the C-scanner class in the first place.

They also pass `--target <host-triple>` explicitly, which matters more
than it looks. cargo-fuzz defaults its target to the triple *it* was
built for, not the host's: `cargo install cargo-fuzz` gives you the
host, while a [cargo-binstall](https://github.com/cargo-bins/cargo-binstall)-backed
install (what [`taiki-e/install-action`](https://github.com/taiki-e/install-action)
does in the workflow) hands you the prebuilt musl artifact, which then
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
a pinned grammar with no release to move to (#1289 is the live example,
a `SIGSEGV` in tree-sitter-bash's scanner), committing the reproducer
makes every future run, local and CI, die on it immediately and stay
red. Record the bytes in the issue instead; the seed and the test land
with the fix.

**A `SIGSEGV` cannot be a normal `#[test]` either.** It kills the test
process rather than failing an assertion, so the regression test has to
drive a subprocess and assert it does not die by signal.

### Leak suppressions

Leak detection stays on. Every entry in `fuzz/lsan-suppressions.txt` needs a diagnosis
because it hides future leak reports matching that symbol. Suppress by symbol rather than passing
`-detect_leaks=0`, so other leaks on the same path still fail.

## The targets {#the-targets}

Each `fuzz_targets/*.rs` is about five lines; the shared fan-out lives in
`fuzz/src/lib.rs`. Read the module doc comment on any target for its
specific rationale.

| Target | What it drives |
|---|---|
| `parse_bash`, `parse_c`, `parse_cpp`, `parse_elixir`, `parse_javascript`, `parse_perl`, `parse_python`, `parse_rust`, `parse_tcl` | `Ast::parse` for one language, then the full walk fan-out |
| `preproc_macro` | `preprocess` harvest → `Source::with_preproc` → the C-family macro-masking lexer |
| `preproc_includes` | `preprocess` over several files → `fix_includes` → include-graph resolution, SCC collapse, candidate scoring |
| `nested_depth` | A structured generator for the deep-nesting complexity class |
| `documentation` | Tagged language/profile selection, borrowed source collection, and bounded identity, owner, qualification, and map queries |

Three design points are load-bearing.

**The bytes reach the parser unmodified.** Full-walk targets call
`Source::from_bytes`, which applies no normalisation. The tagged documentation
target passes its unchanged payload to `AnyParser` or a raw included-range parser.
The file-reading
path (`read_file`, `read_file_with_eol`, `normalize_eol`) runs
`normalize_line_endings`, which guarantees a trailing newline. Tests
using those readers cannot exercise a node ending at EOF, and that is
how #1051 survived the test suite. A harness that normalised first
would pass vacuously while looking like coverage. `Ast::from_path`
normalises; no target uses it.

**One parse, every walk.** `ops`, `dump` and comment removal are seams on
an already-built `Ast`, not separate entry points, so each full-walk target parses
once and then fans out over `metrics`, `ops`, `strip_comments`, `dump`,
`functions`, `suppressions`, `count` and `find`. The documentation check uses
that same `Ast`'s language, tree, and held source buffer. Preprocessor targets
therefore supply the masked bytes that actually produced the tree.
The harness also serialises the
results, which is the only thing in the fan-out that reaches
`recursion::serialize_bounded` and `wire::map_tree`, the recursion
bounded by #1056, after nested input was found able to abort the process
from `Serialize`.

**The C grammars are instrumented too.** cargo-fuzz sanitizes the Rust
side; the tree-sitter grammars are C
compiled by build scripts and need separate instrumentation.
`make fuzz-check` sets `CFLAGS_<host-triple>`. It has to be the triple-qualified
spelling: `cc` resolves `<VAR>_<target>`, then `<VAR>_<target_with_underscores>`,
then `<BUILD_KIND>_<VAR>`, then bare `<VAR>`, and picks `HOST_` rather
than `TARGET_` for the build kind when it is not cross-compiling, so
`TARGET_CFLAGS` is silently inert for a native fuzz build.

### Documentation properties {#documentation-properties}

The [source-bound index](documentation_extraction.md#index) retains physical fragments,
logical documents, and associations with owners. An owner is the entity being documented;
a profile selects the documentation convention. Qualifications record known conditional
or recovery facts, while assessments record unfinished adapter work. Included ranges select
which original source bytes the parser reads. Source maps relate extracted text to those bytes;
a map run describes one contiguous output range.

The documentation target has a handwritten version-one three-byte header.
The payload after byte 2 reaches the parser unchanged, including invalid
encoding, NUL bytes, CRLF, and a missing final newline. Unknown tags,
reserved bits, and unknown profile selectors produce distinct skips before
parsing. They never alias a supported language through a modulus.

| Byte | Version-one meaning |
|---|---|
| 0 | Language tag in the stable table below |
| 1, bit 0 | Collect a second index on the same parse; compare results and reject foreign handles |
| 1, bit 1 | Check cloned-tree, shifted-node, and edited-node rejection |
| 1, bits 2–3 | `0`: ordinary collection; `1`: included ranges; `2`: cancelled collection scope followed by restored collection; `3`: active uncancelled scope |
| 1, bits 4–7 | Reserved; setting any bit skips the input |
| 2, bits 0–4 | Profile selector from `PROFILES` in `fuzz/src/documentation/header.rs`; `0` selects the language default |
| 2, bit 5 | Retain unbound candidate evidence |
| 2, bit 6 | Retain directive evidence |
| 2, bit 7 | Reserved version bit; setting it skips the input |

The language tags are part of the corpus format, independent of enum
discriminants or iteration order:

| Tags | Languages in tag order |
|---|---|
| 0–4 | Rust, Python, Java, Kotlin, Groovy |
| 5–9 | Csharp, Php, Javascript, Mozjs, Typescript |
| 10–14 | Tsx, C, Cpp, Mozcpp, Objc |
| 15–19 | Go, Ruby, Lua, Perl, Elixir |
| 20–24 | Bash, Tcl, Irules, Ccomment, Preproc |

Registration tests compare the table with the `LANG` roster and execute every
registered seed through the real target. Positive seeds obtain exact fragment
and owner ranges, parsed declaration kinds, and text from
`tests/fixtures/documentation/inventory.json`. They separately require copied origins
whose bytes equal the extracted text. The Perl seed is a file document:
it takes its fragment range and text from the same inventory row, and
requires one resolved association whose owner is the file. Deferred adapters require
`AdapterUnavailable`; helper grammars require `NoDeclarationModel`.
Recognized deferred profiles differ from `UnsupportedProfile`.
These outcomes do not count as positive source extraction. Future adapters
must add their own positive seeds when their source paths become available.

The checker validates raw pieces against the exact source and included ranges.
It checks observed document, association, and qualification ordering without
sorting the results. Bounded owner and canonical-cursor queries compare
indexed results with the corresponding enumerated records. Pending assessments remain inspectable
alongside known associations; candidate and reference qualifications retain
their distinct identities and reasons. Cross-index handles and noncanonical
nodes must return the documented identity errors.

Candidate qualifications linked to Node/File owners validate the owner's full
`NodeEvidence`. Reference qualifications use document identity and `node.id()` to find
fragment evidence, then validate the full node context. The checker never hashes a
tree-sitter node or scans every fragment for each qualification. Building this index
costs expected linear work in document-fragment memberships and qualifications, plus
the actual comparisons within buckets whose node IDs coincide. Slot, Group, and Virtual
owners retain link/source checks; their exact candidate-node relationship is not yet
established by the supported adapters.

Repeat checking compares borrowed public fields and linked record identities, including
optional node evidence, slot roles and enclosure, association binding/scope, source-cleaned
maps, context, and options. These are API-consistency checks. The registered seeds separately
assert independent semantic outcomes, including ambiguous/conditional resolutions and field slots.

Every complete document gets one map validation. Runs must cover its output;
copied origins match source bytes, transformed queries retain all responsible
token pieces, and synthetic output has no source. Stored runs may divide a
UTF-8 scalar when adjacent included pieces divide it. Query endpoints must
still be scalar boundaries. The query set includes reversed, out-of-bounds,
scalar-splitting, valid, late, and empty-EOF ranges. Unavailable, pending,
metadata, disabled, and explicit-absence content never becomes empty text.
Registered positives reach copied origins, synthetic comment joins, and decoded
CRLF tokens across included pieces. Expanded origins and source-produced metadata,
disabled, and explicit-absence states await their later adapters; defensive match
arms do not establish positive coverage of those forms.

The resource limits are harness policy:

- Inspect source buffers up to 32 KiB. The tagged target skips larger payloads before parsing.
  Ordinary walk targets skip only documentation and continue their other checks.
  Neither path truncates source.
- Perform at most two ordinary collections over the same parse.
- Select at most eight owner/node subjects and eight map ranges per input, including late
  positions. Cross-checking the selected subjects uses up to 20 ordinary owner/node API calls.
  Explicit identity modes add three rejection queries; cross-index handle checks are separate.
- Limit structured included-range inputs to 256 ranges. Each non-NUL payload
  byte becomes one included range, so adjacent ranges can divide a scalar.
  NUL bytes create excluded gaps. Zero included bytes, including empty and all-NUL payloads,
  or more than 256 ranges skip the input. Parsing uses the original payload once.
- Retain the existing `nested_depth` generator's 512-construct limit. The documentation
  operation accepts its generated source when it fits the 32 KiB cap; the 256 limit applies
  to newly constructed included ranges, not inherited nesting.

The checker makes linear or output-proportional passes and uses expected
constant-time hash indexing. Its included-byte prefix table costs linear
source space and avoids scanning every included range for every piece.
Bounded enumeration checks may scan associations for each selected owner;
the selected-owner count is constant. These limits bound harness work, not
library allocation or legitimate documentation fanout. They do not prove an
input fits `FUZZ_TIMEOUT`.

Cancellation uses `AnyParser`'s retained completion state. Deterministic tests
pre-cancel a deliberately large parse and require `ParseCompletion::Cancelled`
after the flag scope ends. Tagged modes check active collection cancellation,
restoration, and an uncancelled flag. A tiny empty root is never evidence that
the parse callback aborted. Large phase-budget cancellation tests remain in
the AST crate; the fuzz target does not expose private phase hooks.

Tests pin every low mode-byte combination independently and record actual parse, collection,
repeat, identity, and query operations in a bounded thread-local recorder. The active-scope
test first finishes normal collection, then cancels the same installed flag and requires raw
collection to return `Cancelled` at entry. This test-only witness creates no additional
successful index and performs no extra call in a fuzz campaign. Recorder scopes restore on
success, rejection, and unwind. Removing scope installation must fail the witness's result
assertion, not just an operation count.

Run `python3 -B fuzz/check_documentation_producers.py --out "$out"` separately to
verify the producer controls. Set `out` to a unique evidence directory created with
`mktemp -d` under an owned location. The driver requires a clean tracked checkout,
temporarily changes the producer, and restores its exact bytes in a `finally` block.
Its `BCA_FUZZ_PRODUCER_CASE` environment variable selects expected outcomes only in
the two `producer_*` tests; leave it unset for ordinary validation. This explicit
verification command is not part of `make fuzz-check`.

Run the finite documentation validation sequence one heavy operation at a time:

```bash
make fuzz-check
make fuzz-replay
make fuzz-smoke FUZZ_RUNS=1000
make fuzz-run FUZZ_TARGET=documentation FUZZ_RUNS=10000
```

Record the revision, nightly and cargo-fuzz versions, input timeout, registered
seed inventory, and actual run counts. A printed tool-availability skip is a
skipped stage. Preserve a failing input and stop investigation runs when a
production crash appears; follow [crash triage](#triaging-a-crash) before
promoting it to a seed.

### Fuzzing an order-dependent function

`fix_includes` walks `HashMap`-ordered collections, which held
`preproc_includes` back until it was measured (#1288): a crash whose
occurrence depends on iteration order would not reproduce from its own
artifact, which is the same non-reproducibility the `-runs`-not-
`-max_total_time` rule exists to prevent.

Measured across 40 runs of one 8-file input: the diagnostic **sequence**
varied every time, while the diagnostic **set** and the mutated `files`
map were identical. So the content was already deterministic and only
the returned `Vec`'s order was not, which was also a live defect on its
own, since `bca preproc` prints that `Vec` straight to stderr and so
emitted the same warnings in a different order on every run. It is now
sorted at the source.

The general rule this leaves: before pointing a fuzzer at a function
that iterates an unordered collection, establish which of its outputs
are order-stable. If a crash can depend on the order, the artifact is
not a reproducer and the target is worse than nothing.

### Language-specific walk targets

The tagged documentation target dispatches every enabled `LANG`. The
language-specific full-walk targets have a narrower scope. `Mozjs` was measured
metric-equivalent to upstream JavaScript on real input (#507). `Preproc`
is driven by `preproc_macro` and `preproc_includes`, whose first pass
is a `PreprocParser` parse of the bytes; the C-family macro-masking
lexer that `preproc_macro` then reaches is the same one `Mozcpp` runs,
reached there through `LANG::Cpp`. The documentation target parses the Gecko
C++ fork through its own tag. The remaining full-walk omissions are
upstream crates with no evidence pointing at them; adding one is a
five-line file plus a `[[bin]]` stanza, and the bar for doing so is a
reason, not a wish for symmetry.

## What is targeted, and why only this

The target set is deliberately narrow. Two pieces of evidence bound it.

**The static lints already cover the known class.** #1152 adopted
`clippy::arithmetic_side_effects` on the `loc` metric module and
`clippy::indexing_slicing` on `big-code-analysis-ast/src/c_macro.rs`,
and would have caught the five-byte input that panicked the library
(#1051) at compile time. What a lint cannot check is the residue it
could not discharge: the per-function
`#[allow(clippy::indexing_slicing)]` sites in
`big-code-analysis-ast/src/c_macro.rs`, each a computed index into
attacker-controlled bytes whose bound is asserted by a human comment. That population is what
`preproc_macro` exists for.

**The encoding and IO layer is already hardened.** A 112-file
adversarial byte corpus (invalid UTF-8, NUL bytes, BOMs, mixed EOL,
unterminated constructs, 2 MB single tokens) across seven subcommands
found zero panics there. Blanket byte fuzzing would mostly re-confirm
that. Do not re-litigate it.

So the targets are the parse-and-walk layer, which that corpus exercised
only incidentally.

## Why out of band

`fuzz-check` and `fuzz-smoke` are **not** part of `make pre-commit` /
`make ci` / `make lint`. A nightly libFuzzer build under ASan costs
around a minute even warm, which does not belong in the per-commit
loop, the same call `chain-audit`, `bench-scaling` and mutation testing
make.

`.github/workflows/fuzz.yml` runs them instead, and splits the two
questions fuzzing answers. A pull request touching `src/`, `fuzz/`, a
manifest, a vendored grammar or the `Makefile` builds every target and
then **replays the committed seeds**: the regression question, answered
in seconds. The quarterly cron does the **hunt**, at 200 000 runs per
target.

The two halves also differ in what they do after a crash. `fuzz-replay`
stops at the first one, because a pull request is already blocked by it.
`fuzz-smoke` runs every remaining target and fails at the end naming all
of them: nobody is watching the cron, and a run that reports one of
three crashes hides the other two until the quarter after the fix.

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

### Advisory, by decision

The fuzz job is **not** a required check, and that is a choice rather
than an oversight. `main`'s `protect-main` ruleset requires signed
commits and linear history, and forbids deletion and force-pushes, but
it names no required status checks, so no CI result blocks a merge
today, and the open question was whether the fuzz job should be the
first to.

Check that with `gh api repos/:owner/:repo/rules/branches/main`, not
with `gh api repos/:owner/:repo/branches/main/protection`. The latter
reads only the legacy branch-protection API and answers `404 Branch not
protected` for a branch governed by a ruleset, which is exactly how this
section came to claim in its first draft that `main` was unprotected.

It should not be, yet. Requiring it puts 15m48s, the measured cost of
one run, in front of every pull request touching `src/`, `fuzz/`, a
manifest or a grammar, to gate a build-and-replay pass rather than a
hunt. It also promotes a nightly-toolchain hiccup or a flaky
`cargo-fuzz` install into a merge blocker, on the one workflow whose
reason for living outside `ci.yml` is that nightly moves underneath it.
And a red cron is not silent: the scheduled run files a GitHub issue
linking the run and the uploaded `fuzz-artifacts` bundle (the `File
issue on a scheduled-run crash` step in `.github/workflows/fuzz.yml`),
so the hunt already reports itself without a required check. The issue
body carries the `make fuzz-run` template rather than the failing
target names; those are in the job log, which the summary line above
now lists in full.

What would change the answer is evidence that the job is boring: a few
quarterly crons in a row going red only for real crashes, never for
toolchain flakes. Revisit it then, by adding a `required_status_checks`
rule to the existing ruleset.

## Definition of done

Coverage-guided fuzzing has no natural stopping point, so #1154 fixed
one before the crate was written: `fuzz/` exists, builds in CI, ships
seed corpora, and has either found a real bug or returned clean at
`-runs=1000000` per target. Beyond that, fuzzing here is maintenance
rather than project work; the quarterly cron and the per-PR compile
gate carry it.
