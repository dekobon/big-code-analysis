# Output-name normalization: postmortem and round-two design input

**Status:** first attempt reverted. The implementation is preserved on
branch `archive/walkfile-name-normalization` (tip `1031be6e`) for
reference. This document records what we were trying to solve, the
design we tried, exactly how and why it failed, and the options for a
second design round.

**Audience:** whoever picks up the next attempt. Read this before
re-deriving the problem — the failure mode is subtle and the obvious
fix (the one we tried) is the one that doesn't work.

---

## 1. The original problem, in detail

### 1.1 The recurring path-anchoring bug class

`bca` discovers source files by walking one or more *seeds* (`--paths`,
`--paths-from`, or a `bca.toml` manifest's `paths`). The walker
(`ignore::WalkBuilder`) emits each file's path **prefixed by the seed as
the user spelled it**. So the *same* on-disk file is emitted as a
different string depending on how the seed was written:

| Invocation (file `/repo/src/foo.rs`) | Emitted path |
|---|---|
| `--paths .` (from `/repo`) | `./src/foo.rs` |
| `--paths /repo` | `/repo/src/foo.rs` |
| `--paths "$PWD"` | `/repo/src/foo.rs` |
| `--paths src` (from `/repo`) | `src/foo.rs` |
| manifest `paths=["."]` resolved to `/repo`, run from `/repo/sub` | `/repo/src/foo.rs` (absolute) |

That emitted path is then consumed, unmodified, by several features that
each need to compare or key on a file's identity:

- **`[check.exclude]` / `--exclude` globs** — patterns are authored in a
  `./`-anchored, walk-root-relative convention (`vendor/**`,
  `./generated/*`).
- **Baseline keys** (`.bca-baseline.toml`) — must be stable across runs
  so a recorded offender is recognized again.
- **`--changed-only` / `--since` diff-scope filtering** — compares a
  violation's path against the set of files git reports as changed.
- **`bca diff`** — pairs the "before" and "after" metric sets by file
  identity.

Because the emitted path form varied with seed spelling, each of these
consumers grew its *own* re-anchoring step, and each one was the subject
of a separate bug:

- **#488** — absolute seeds defeated the `./`-anchored exclude globset.
  Fixed by `reanchor_seed` (collapse an at/under-CWD absolute seed to
  its CWD-relative form before walking).
- **#489** — a manifest root *above* the CWD (run from a subdirectory)
  could not be collapsed by `reanchor_seed`, so the walker emitted
  absolute paths the `./`-anchored deny-set never matched. Fixed by
  `match_path_for` (anchor the glob match to the walk root, per-seed).
- **#493 / #497 (Bug B)** — `[check.exclude]` re-anchored only `--paths`
  seeds, not `--paths-from` ones, so a `--paths-from` offender escaped
  the exclude. Fixed by `anchor_against_seeds` (re-anchor a post-walk
  violation path against the full seed set).
- **#497 (Bug A)** — `bca diff --since <subdir>` mis-paired because the
  before side (a `git archive` extraction) and the after side were
  rooted differently. Fixed by treating the `--since` positional as a
  scope and anchoring both sides at the repo root.

The pattern was clear: **the same "which file is this, in canonical
form?" question was being answered independently, and slightly
differently, at four or five places** — and each new consumer (or each
new seed source) reopened the bug.

### 1.2 The two goals

1. **Structural goal (ours):** dissolve the bug class by computing a
   file's canonical identity *once*, where the seed association is
   still live, so no downstream consumer ever re-derives it. The four
   anchoring helpers (`reanchor_seed`, `match_path_for`,
   `anchor_against_seeds`, plus the `bca diff` process-CWD swap
   `with_cwd`) would collapse into one.

2. **User-facing goal (requested):** **normalize the emitted output
   `name`** so the same file reports the same name regardless of seed
   spelling. Concretely, `bca metrics --paths /abs/repo`, `--paths .`,
   and `--paths "$PWD"` should all emit `name: "./src/foo.rs"` instead
   of three different strings — making two `bca metrics` captures
   directly comparable, and making the JSON output stable.

The user explicitly authorized a `2.0` breaking change to achieve goal 2
("Let's do it all including normalizing the output name. This is against
our gate, but it is a one time exception").

---

## 2. The solution we tried

A **dual-path `WalkFile`** computed at the walk seam:

```rust
pub struct WalkFile {
    pub io_path: PathBuf, // the path the runner opens (always readable)
    pub name: PathBuf,    // the canonical identity reported downstream
}
```

- `FilesData { paths: Vec<PathBuf> }` became `FilesData { files: Vec<WalkFile> }`,
  and the `ConcurrentRunner` callback took `WalkFile` instead of
  `PathBuf` (the breaking API change).
- `expand_seed_paths` (the CLI's single walk seam) computed, for each
  walked file, a **canonical name** = the file's path relative to the
  **analysis root**, `./`-prefixed. The analysis root was the
  **longest common ancestor (LCA) of the directory seeds**, normalized
  lexically (`lexical_abs` → `baseline::lexical_normalize`, *not*
  `fs::canonicalize`, to stay in the walker's path space). Explicit
  single-file seeds kept their spelled name verbatim
  (`WalkFile::verbatim`).
- `act_on_file` read `io_path` but handed `name` to every `dispatch_*`
  arm, so the emitted document name, baseline key, diff key, and
  exclude-match target all used the one canonical `name`.
- The four anchoring helpers and `with_cwd` were deleted.
- A reusable safety tool, `verify-name-only-churn.py`, was written to
  prove the bulk snapshot regen was value-preserving (it is kept; it is
  generally useful and is the one artifact of this attempt worth
  retaining on the main branch).

It compiled, passed the full workspace test suite, both self-scan
tiers, and `make pre-commit`. The spelling-independence goal *appeared*
met: `bca metrics --paths /abs/src`, `src`, `.`, and `$PWD` (all
pointing at the same directory) emitted `./foo.rs`.

---

## 3. How it failed

A maximum-effort review (`/code-review max`) found **three confirmed,
reproduced, silent correctness regressions**. All three trace to one
root cause.

### 3.1 The three regressions

**R1 — `bca diff --since <single-file>` reports the file as added *and*
removed, with zero deltas.** Reproduced:

```text
$ bca diff --since HEAD src/work.rs --format json
buckets: []
added:   ["/tmp/<workingtree>/src/work.rs"]
removed: ["/tmp/<extraction>/src/work.rs"]
```

A single-file scope has no *directory* seed, so the analysis root is
`None` and the file falls into the verbatim branch — emitting the
absolute path of each side's tree. The before side (a `/tmp` `git
archive` extraction) and the after side (the working tree) therefore get
non-matching absolute names and never pair. (The directory-scope form,
`--since HEAD src`, worked, which is why every `diff_since` test — all
of which use directory scopes — stayed green.)

**R2 — subdirectory-scoped baseline keys lose the scope prefix.**
Reproduced:

```text
$ bca check --paths .   --write-baseline b_dot.toml   # key: "src/work.rs"
$ bca check --paths src --write-baseline b_src.toml   # key: "work.rs"   ← wrong
$ bca check --paths src --baseline b_dot.toml         # offender re-fires as [new]
```

With `--paths src`, the analysis root is `src` itself (LCA of a single
seed), so `canonical_name` strips the whole `src/` and emits `./work.rs`
→ baseline key `work.rs`. The same file under `--paths .` keys as
`src/work.rs`. A baseline written under one scope no longer suppresses
under the other; offenders silently re-fire.

**R3 — `--changed-only` from a subdirectory silently drops violations
(gate bypass).** The canonical `name` is anchored to the analysis root,
but `DiffScope::contains` resolves a violation path with
`canonicalize_for_match` (`path.canonicalize()`), which is **CWD-based**.
When the analysis root ≠ CWD (a subdir scope, or a manifest root above
the CWD), the canonical `./`-name resolves to the wrong absolute path,
fails to match the git-changed set, and the violation is dropped — the
gate can exit 0 on a real regression.

### 3.2 The root cause: consumers have *heterogeneous anchors*

The premise — "compute one canonical identity" — is **unsound**, because
the consumers do not share an anchor:

| Consumer | Anchor it needs | Why |
|---|---|---|
| `[check.exclude]` glob | **walk-root**-relative (`./…`) | patterns are authored that way |
| baseline key | **baseline-file dir** | keys must be stable across runs/locations |
| `--changed-only` | **CWD** (resolves to absolute) | compares against git's changed set |
| `bca diff` | **tree-root**-relative (per side) | the two trees live at different absolute roots |

A single string cannot be simultaneously walk-root-relative,
CWD-resolvable, baseline-anchor-relative, and tree-root-relative when
those anchors differ. The LCA-of-seeds `./`-name we chose is
**lossy**: it drops the path segments between the analysis root and the
file, so a consumer that resolves it against a *different* anchor (CWD,
baseline dir) can no longer recover the file's true location.

Crucially, the pre-existing per-consumer design was **not** redundant
boilerplate — each consumer re-anchored *because it genuinely has a
different anchor*. Collapsing them was the wrong altitude: we mistook
four correct, anchor-specific transforms for one duplicated transform.

### 3.3 Why every gate stayed green

The project's own dogfooding always runs `--paths .` (or manifest
`paths=["."]`) from the repo root, where **analysis-root == CWD ==
repo-root == baseline-anchor**. In that one configuration all anchors
coincide, so the lossy name round-trips and nothing breaks. The
regressions only surface for subdir scopes, single-file diffs, and
`--changed-only`/manifest runs from a subdirectory — none of which the
test suite or the self-scan exercised. This is itself a lesson: **the
gate's uniformity hid an anchor-coupling bug.**

---

## 4. Options for round two

### Option A — Do not unify; keep per-consumer anchoring (what we reverted to)

Accept that identity is anchor-specific and keep each consumer's
transform. This is correct and is the current state. The cost is the
"five places answer the same-ish question" maintainability smell that
motivated the attempt — but they are *not* actually the same question,
so the smell is partly illusory. **Lowest risk; abandons goal 2.**

If we stay here, the worthwhile hardening is a **shared, well-tested
"resolve this emitted path to an absolute path" primitive** that each
consumer calls before applying its *own* anchor — i.e. unify the
*resolution* step (lexical-abs, the part that is genuinely identical)
without unifying the *anchoring* step (which is not).

### Option B — Normalize the *display* name only; keep raw paths for keys

Split the two roles the failed design conflated:

- A **display name** (presentation-only): the canonical `./`-form, used
  for the `bca metrics` JSON `name` field and human output. This is what
  goal 2 actually wants.
- A **key/identity path** (the raw, anchor-resolvable path): used by
  baseline, `--changed-only`, diff pairing, and exclude matching, each
  with its existing per-consumer anchor.

`WalkFile` would carry both `io_path` and a `display_name`, but the
*keying* consumers would continue to use `io_path` (or a
resolve-to-absolute of it) and their own anchors — never the display
name. This achieves the user-visible normalization without feeding the
lossy form into the keying paths.

**Risk:** medium. Requires auditing every consumer to confirm it uses
the key path, not the display name. The display name is only safe where
it is purely presentational. (Note: `bca diff`'s pairing key is *also*
display-ish — two captures must pair on the normalized name — so diff
specifically *wants* the normalized form, but anchored per-side at the
tree root, not the LCA. Diff may need its own normalization parameter.)

### Option C — Fix-forward the unified name with an explicit, non-lossy anchor

Keep one `name`, but (1) make it **non-lossy** — anchor it to a single
*fixed* root passed explicitly by the caller (CWD for standalone
check/metrics; the manifest dir when a manifest set the paths; the tree
root for each diff side) rather than the LCA of the scope seeds — and
(2) ensure every consumer resolves it against *that same* fixed root.

The hard part is that the consumers' anchors still differ (baseline-dir
vs CWD vs tree-root). To make one name work for all, you would have to
re-express every consumer's anchor in terms of the chosen fixed root —
e.g. make baseline keys relative to the analysis root instead of the
baseline-file dir (a baseline-format change), and make `--changed-only`
compare in analysis-root space instead of canonicalizing against CWD.
That is a larger, format-touching change with its own migration.

**Risk:** high, and it changes the baseline format and the changed-only
contract. Only worth it if a single canonical identity is judged
valuable enough to re-anchor every consumer onto it deliberately.

### Recommendation for the design discussion

Start from **Option B**. It is the smallest change that delivers the
*actual* user request (a normalized, comparable display/output name)
without touching the keying paths that the failed attempt broke. Reserve
**Option C** for a later, deliberate "single canonical identity"
project if Option B's display/key split proves awkward (e.g. if `bca
diff` consumers want the normalized form as a key). Keep **Option A** as
the fallback if normalization is judged not worth the consumer audit.

Whatever the choice, the next attempt **must** add regression tests for
the three configurations the gate's uniformity hid:

1. a subdirectory scope (`--paths src`) — assert the baseline key and
   emitted name retain the `src/` segment;
2. a single-file `--since` scope — assert a real paired delta;
3. `--changed-only` (or a manifest run) **from a subdirectory** — assert
   in-scope violations are kept.

---

## 5. Reference

- Reverted implementation: branch `archive/walkfile-name-normalization`,
  tip `1031be6e` (`refactor(api)!: normalize output names via dual-path
  WalkFile` + `refactor(cli): share lexical path normalizer; hoist walk
  CWD read`).
- The correct, retained consumer-side fixes for #497 Bug A/B are on the
  main work branch as `f0b83a7d`.
- `verify-name-only-churn.py` (kept) — proves a bulk snapshot regen is
  name-only/value-preserving; reusable for whichever round-two option
  touches emitted paths.
- Related: STABILITY.md (the `2.0` deferral list), issues
  #488 / #489 / #493 / #497, and lesson candidates below.

### Lesson candidates (for `lessons_learned.md`, if accepted)

1. **Re-anchoring at multiple consumers is not necessarily duplication.**
   When N consumers transform the same value, check whether they share a
   *parameter* (here: the anchor). If they don't, "compute it once" is
   the wrong altitude — you will produce a value that is correct for one
   consumer and lossy for the rest.
2. **A uniform gate can hide a coupling bug.** Every self-scan/CI path
   here ran `--paths .` from the repo root, where four distinct anchors
   coincide. The anchor-coupling regression was invisible until a review
   varied the scope. Vary the *invocation shape* in tests, not just the
   inputs.
