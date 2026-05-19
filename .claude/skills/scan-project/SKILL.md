---
name: scan-project
description: Workspace-wide scan for logic errors, security issues, and metrics calculation bugs across all crates and all language modules. Runs 6 parallel sub-agents, one per scope partition, and applies a 50-question checklist that encodes every lesson in docs/development/lessons_learned.md. Files GitHub issues with fix plans. READ-ONLY — no commits, no file modifications.
---

# Scan Project

Scan the entire `big-code-analysis` workspace for logic errors, security
issues, and metrics calculation bugs.

Unlike `audit-crate` (which audits one crate at a time with a general
checklist), this skill:

- **Covers the whole workspace** in one invocation.
- **Runs 6 sub-agents in parallel**, one per scope partition.
- **Applies a 50-question checklist** that encodes every lesson documented in
  `docs/development/lessons_learned.md`, with 20 questions dedicated to
  metrics calculation bugs that the general checklist does not cover.

## ABSOLUTE CONSTRAINTS

**This skill is READ-ONLY. It MUST NOT leave any trace on the filesystem.**

- **NEVER commit code.** No `git commit`, no `git add`, no staging. Zero commits.
- **NEVER leave uncommitted files.** No new files, no modified files, no temp files
  in the worktree. If you accidentally create or modify a file, revert it
  immediately with `git checkout -- .`.
- **NEVER modify source files.** Not even "harmless" formatting or comment fixes.
- **NEVER push branches.**
- The ONLY side effects of this skill are: GitHub issues filed, Serena memories
  updated, and terminal output printed.

---

## Before Starting

Check existing open GitHub issues (`gh issue list --limit 200`) to avoid filing
duplicates. Note relevant open issues as context, but do NOT let them constrain
the scan.

---

## Step 0: Launch isolated agent

**This step is MANDATORY and must be the very first action.**

### Environment detection

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
if [[ "$PROJECT_ROOT" == *".claude/worktrees/"* ]]; then
  ISOLATION_MODE="worktree"
else
  ISOLATION_MODE="branch"
fi
```

- **Worktree mode**: Already inside a worktree. Keep existing behavior.
- **Branch mode**: Main project directory. Agents run without worktree isolation.
  Serena LSP works correctly in this mode.

### Branch mode prerequisites

```bash
if [[ "$ISOLATION_MODE" == "branch" ]]; then
  DIRTY="$(git status --porcelain)"
  if [[ -n "$DIRTY" ]]; then
    echo "Error: branch mode requires a clean repository." >&2
    echo "$DIRTY" >&2
    exit 1
  fi
fi
```

### Launch the orchestrating agent

If you are the top-level orchestrator (invoked by the user), immediately launch
a single Agent that executes Steps 1-10. Pass the full workspace root path.

- **Worktree mode**: Launch the Agent with `isolation: "worktree"`.
- **Branch mode**: Launch the Agent WITHOUT `isolation: "worktree"`.

Sub-agents launched in Step 3 inherit the parent context (read-only) and do
NOT need their own `isolation: "worktree"`.

### Agent verification (MANDATORY)

Every agent must verify its environment before doing any work:

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
if [[ "$PROJECT_ROOT" == *".claude/worktrees/"* ]]; then
  ISOLATION_MODE="worktree"
else
  ISOLATION_MODE="branch"
fi
echo "ISOLATION_MODE=$ISOLATION_MODE PROJECT_ROOT=$PROJECT_ROOT"
```

In branch mode, also confirm the working tree is clean:

```bash
git status --porcelain
```

If any output appears, abort: "ABORTED: Branch mode requires a clean working
tree."

---

## Step 1: Load scan history

Invoke the Serena MCP tool `serena:read_memory` with
`memory_name: "scan-project-state"`. If the Serena MCP server is not active,
call `serena:activate_project` first.

The memory key is a fixed token (no `$ARGUMENTS` interpolation) because
scope is workspace-wide. This sidesteps the slash-sanitization rule
`audit-crate` documents for path-shaped arguments — no normalization is
needed here, but do not parameterize this key in future edits without
re-applying that rule.

The memory, if it exists, contains a per-partition coverage table:

```
# Scan State: scan-project
last_scan: YYYY-MM-DD
last_model: <model-id>

## Partition Coverage
<partition_name> | <depth> | <date> | <findings_count> findings | <model-id>
```

Where `<depth>` is: `full`, `partial`, `skimmed`, or `none`.

Use the same priority table as `audit-crate`:

| Priority | Condition |
|----------|-----------|
| 1 | `none` — never scanned |
| 2 | `skimmed` |
| 3 | `partial` AND older than 30 days |
| 4 | `partial` AND recent |
| 5 | `full` AND recent (< 3 days) — skip unless changed since last scan |

New partitions not in the memory default to priority 1.

---

## Step 2: Build baseline and collect metrics

### 2a: Build all crates

```bash
cargo build --workspace 2>&1 | tail -10
cargo test  --workspace --all-features 2>&1 | tail -30
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -30
```

Record: passing test count, existing warnings, existing clippy findings.
Existing failures are context — not your problem to fix.

### 2b: Self-hosted metrics

Run the project's own CLI across the library source tree to surface
high-defect-probability hotspots. These direct sub-agent attention to the
highest-risk functions regardless of checklist order.

The CLI binary is `bca` (declared as `[[bin]] name = "bca"` in
`big-code-analysis-cli/Cargo.toml`), not `big-code-analysis-cli`.

```bash
if cargo build -p big-code-analysis-cli 2>/tmp/scan-cli-build.log; then
  ./target/debug/bca -m -O json -p src/ \
    > /tmp/scan-metrics.json 2>/tmp/scan-metrics.err || true
fi
```

If `/tmp/scan-metrics.json` is missing or empty after this step, surface
the relevant log (`/tmp/scan-cli-build.log` or `/tmp/scan-metrics.err`)
in the Step 8 summary as "metric hotspot input unavailable: <reason>" so
the run is not silently degraded.

Flags for triage (feed to sub-agents as the first set of targets):

- **Halstead estimated bugs > 0.5**: highest defect probability — audit at depth `full`.
- **Cyclomatic complexity > 10**: complex branching, likely missed edge cases.
- **Cognitive complexity > 15**: hard-to-read code where logic errors hide.
- **Functions > 100 SLOC**: decomposition candidates.
- **MI < 10**: high regression risk on any change.

### 2c: Discover workspace layout

```bash
FD=$(command -v fd 2>/dev/null || command -v fdfind 2>/dev/null)
"$FD" --type f -e rs . src/
"$FD" --type f -e rs . big-code-analysis-cli/src/
"$FD" --type f -e rs . big-code-analysis-web/src/
"$FD" --type f -e rs . tests/
```

---

## Step 3: Partition scope and launch sub-agents

Launch all six sub-agents **in parallel**. Use the `Agent` tool with
`subagent_type: "general-purpose"`, one call per partition, all six in a
single message so they run concurrently. Do **not** pass
`isolation: "worktree"` to the sub-agents — Step 0 already arranged
isolation at the orchestrator level, and sub-agents inherit the
parent's read-only context.

Each agent receives:

1. Its file list (below).
2. The full 50-question checklist (Step 4).
3. The metrics hotspot list from Step 2b (audit hotspots first within each agent).
4. The list of existing open issues from `gh issue list`.
5. Instruction to record each finding in the standard format:

```
FINDING: <short title>
FILE: <path>:<line range>
CHECKLIST: <question number(s)>
EVIDENCE: <what is wrong and why, with code snippet>
CATEGORY: bug | security | enhancement | documentation
```

Use exactly these four categories (they map to GitHub labels).

### Partition A — Metrics core

Files:
- `src/metrics/abc.rs`
- `src/metrics/cognitive.rs`
- `src/metrics/cyclomatic.rs`
- `src/metrics/exit.rs`
- `src/metrics/halstead.rs`
- `src/metrics/loc.rs`
- `src/metrics/mi.rs`
- `src/metrics/nargs.rs`
- `src/metrics/nom.rs`
- `src/metrics/npa.rs`
- `src/metrics/npm.rs`
- `src/metrics/tokens.rs`
- `src/metrics/wmc.rs`
- `src/checker.rs`
- `src/getter.rs`
- `src/alterator.rs`

Checklist focus: all 50 questions. Special attention to Q31–Q36, Q39–Q42,
Q44–Q46, Q50 (metrics-specific section).

### Partition B — JS-family language modules

Files:
- `src/languages/language_mozjs.rs`
- `src/languages/language_javascript.rs`
- `src/languages/language_typescript.rs`
- `src/languages/language_tsx.rs`

Checklist focus: all 50 questions. Special attention to Q32–Q36, Q38–Q45
(aliased variants, sibling parity, Halstead, dispatch gaps).

After reading all four files, **build the sibling-parity audit table** for
each metric (cognitive, cyclomatic, halstead) by comparing match arms across
the four modules for:
- Ternary / conditional expressions
- For/for-of/for-in/for-each loops
- Arrow function classification
- Modern operators (`=>`, `...`, `?.`, `??`, `**`)
- `typeof` / `instanceof` / `void` classification

Report any discrepancy as a separate FINDING.

### Partition C — C-family language modules

Files:
- `src/languages/language_cpp.rs`
- `src/languages/language_csharp.rs`
- `src/languages/language_java.rs`
- `src/languages/language_kotlin.rs`

Checklist focus: all 50 questions. Special attention to Q32, Q37–Q42
(grammar root, else-if structural model, cross-language parity, dispatch gaps).

Build the same sibling-parity audit table as Partition B for this language
family. The parity table covers the C/C++ side (`cpp` only — there is no
`language_c.rs` or `language_mozcpp.rs` in this workspace) and the JVM/
managed side (`csharp`, `java`, `kotlin`).

### Partition D — Other language modules

Files (every `src/languages/language_*.rs` not in B or C):
- `src/languages/language_python.rs`
- `src/languages/language_rust.rs`
- `src/languages/language_go.rs`
- `src/languages/language_bash.rs`
- `src/languages/language_php.rs`
- `src/languages/language_ruby.rs`
- `src/languages/language_lua.rs`
- `src/languages/language_perl.rs`
- `src/languages/language_tcl.rs`
- `src/languages/language_elixir.rs`
- `src/languages/language_groovy.rs`
- `src/languages/language_ccomment.rs`
- `src/languages/language_preproc.rs`

If `ls src/languages/language_*.rs` reveals a module not in this list
(or in Partitions B/C), add it to this partition and flag the omission
in the Step 8 summary so the file list can be refreshed.

Checklist focus: all 50 questions. Special attention to Q31–Q32, Q38–Q42,
Q44 (no-op impls, aliased variants, structural divergence, text-keyed dispatch,
dispatch gaps).

### Partition E — Core infrastructure

Files:
- `src/spaces.rs`
- `src/node.rs`
- `src/parser.rs`
- `src/traits.rs`
- `src/macros.rs`
- `src/c_macro.rs`
- `src/lib.rs`

Checklist focus: all 50 questions. Special attention to Q5–Q7, Q28, Q41,
Q44, Q48 (recursion bounds, API stability, cursor misuse, text-keyed dispatch,
serialize field count).

### Partition F — CLI and web crates

Files:
- `big-code-analysis-cli/src/**` (all Rust source files)
- `big-code-analysis-web/src/**` (all Rust source files)

Checklist focus: all 50 questions. Special attention to Q8–Q13, Q20, Q47,
Q49, Q50 (security, path handling, orphan-task DoS, CBOR serialize, NaN
panics).

---

## Step 4: Scan checklist

Apply every applicable question to every file in scope. **Start with functions
flagged by code metrics** from Step 2b — these have the highest defect
probability. For each file, audit metric-flagged functions first, then scan
the remainder.

Track depth per file: `full` | `partial` | `skimmed`.

---

### Section A — Logic and Correctness (Q1–Q7)

1. Does every code path produce the correct output for its documented contract?
2. Are there off-by-one errors in ranges, indexes, or boundary checks?
3. Are there match arms, if-else branches, or rule bodies that are unreachable
   or logically dead?
4. Are error cases handled, or silently swallowed (`continue`, `_ => {}`,
   `Err(_)` arms, ignored `Result`)?
5. Are there false positives on valid inputs, or false negatives on invalid
   inputs?
6. Are all edge cases handled: empty input, max-size input, non-UTF-8 paths,
   missing files, deeply nested ASTs?
7. Is recursion bounded? AST traversal and tree-sitter walks must not blow the
   stack on pathological input.

---

### Section B — Security (Q8–Q13)

8. Can a malicious or pathological input (e.g., a deliberately deep or
   recursive source file) cause stack overflow, infinite loop, or OOM?
9. Is there path traversal risk in any directory-walking or file-loading logic?
10. Are symlinks handled safely?
11. Is `to_string_lossy()` used on paths that are map keys, JSON output, or
    error-correlation identifiers? (`to_string_lossy()` is acceptable only for
    log/display output; use `to_str()` with explicit error handling for
    identifiers.)
12. Does any feature or API allow user-supplied content to execute arbitrary
    code or make network calls?
13. Are there hardcoded secrets, tokens, or environment-specific paths?

---

### Section C — Incorrect Comments and Documentation (Q14–Q17)

14. Do doc comments on public items accurately describe behavior, parameters,
    return values, and errors?
15. Are inline comments factually correct and not stale (old architectures,
    wrong line numbers, removed features)?
16. Do doc-test examples compile and run correctly?
17. Are there TODO/FIXME/HACK comments indicating known tech debt that should
    be filed as issues?

---

### Section D — Code Smells and Unnecessary Complexity (Q18–Q23)

18. Are there duplicated helper functions across test files that should be in a
    shared module?
19. Are there overly verbose patterns that could be simplified (unnecessary
    clones, verbose match arms)?
20. Are there unused imports, functions, or struct fields?
21. Are there functions longer than ~50 lines of logic that should be
    decomposed?
22. Are there hardcoded strings that should be named constants?
23. Are any hardcoded lists a maintenance hazard (e.g., per-language node-type
    lists that must stay in sync with the upstream tree-sitter grammar)?

---

### Section E — Dependency and Build Concerns (Q24–Q26)

24. Are all `Cargo.toml` dependencies necessary? Are any runtime dependencies
    only used by a binary or optional feature?
25. Are feature flags on dependencies minimal and appropriate?
26. Is `default-features = false` used where the full default feature set is
    not needed?

---

### Section F — Project-Specific Baseline (Q27–Q30)

27. Per-language modules under `src/languages/` deliberately mirror each other.
    Does any change introduce a discrepancy that one language exhibits and
    another does not (different metric formula, different node-type handling,
    different operator/operand classification) without justification?
28. The crate is published on crates.io. Does any change break the public API
    (`lib.rs` re-exports, public traits, `Metrics` / `FuncSpace` / language
    enum shapes) without an intentional version bump?
29. Tree-sitter grammar versions are pinned with `=X.Y.Z` in the root
    `Cargo.toml`. Does any change loosen these pins, or use grammar features
    not available in the pinned version?
30. Are there `unwrap()`, `expect()`, `assert!()`, or `panic!()` calls in
    non-test code? Document any non-test `expect()` with the invariant that
    makes the panic unreachable.

---

### Section G — Metrics Calculation Bugs (Q31–Q50)

These questions encode every lesson in `docs/development/lessons_learned.md`.
Apply them to every file in Partitions A–D.

#### No-op trait implementations (Lesson 1)

31. For every `implement_metric_trait!(X, LangCode)` entry in `src/metrics/`:
    is this a *real* no-op (the language genuinely has no such construct, with
    a comment capturing the reason) or a *placeholder* (the language has the
    construct but no impl was written)? Each placeholder must have a smoke test
    in `mod tests` that pins the current `0` value so the assertion fires when
    the real impl lands. Any entry lacking both a justification comment and a
    smoke test is a finding.

#### Aliased kind_id variants (Lesson 2)

32. For every `match node.kind_id()` block in `getter.rs`, `checker.rs`,
    `alterator.rs`, `spaces.rs`, and `src/metrics/*.rs`: are all aliased
    numeric-suffix variants (`Kind2`, `Kind3`, … `Kind17`) of every matched
    rule either explicitly listed or explicitly excluded with a comment? Run:
    ```bash
    rg 'Lang::([A-Za-z]+)\b' src/getter.rs src/checker.rs \
      src/alterator.rs src/spaces.rs src/metrics/
    ```
    then cross-reference against the regenerated `language_<lang>.rs` to
    confirm every suffixed variant is accounted for.

#### Sibling module parity (Lesson 3)

33. For each symbol or match arm that was changed or is under review in one
    JS-family module (`mozjs`, `javascript`, `typescript`, `tsx`): is the same
    arm present and correct in all three siblings? Run:
    ```bash
    rg '<symbol_or_match_arm>' \
      src/languages/language_{javascript,mozjs,typescript,tsx}.rs \
      src/{getter,checker}.rs
    ```
    Apply the same check to C-family siblings (`c`, `cpp`, `mozcpp`) and to
    any change in a per-metric file (`cognitive.rs`, `cyclomatic.rs`, etc.):
    a defensive guard added to one metric file must be propagated across the
    metric family.

#### `_min` sentinel leak (Lesson 3 — extended)

34. For every `_min()` accessor across all metric files (`cognitive_min`,
    `cyclomatic_min`, `exit_min`, `abc_min`, `nom_min`, `nargs_min`, and any
    others): is the `usize::MAX → 0.0` collapse applied (as in
    `src/metrics/tokens.rs:115-127` and `src/metrics/loc.rs`)? Any accessor
    that returns `usize::MAX as f64` or `f64::MAX` to callers (including JSON
    serialization) when no value was observed is a bug.

#### Halstead n1/n2 invariant (Lesson 4)

35. Does `operators.len() == n1` and `operands.len() == n2` hold after every
    finalize pass for every language? Is any `kind_id` classified as BOTH
    operator AND operand in `getter.rs`? If so, it must appear in only one
    arm — a `HalsteadType` that maps to both is a bug.

36. For every language that has multi-keyword kinds (e.g., `PrimitiveType`
    covering `int`/`float`/`double`, `PredefinedType` covering
    `string`/`number`/`boolean`): are these stored by **source text** in the
    Halstead operator map (not by kind_id), so that `n1` correctly counts
    distinct token texts rather than collapsing N keywords to 1? Check that
    `--ops` output agrees with `n1` for a representative fixture.

#### Grammar root assumption (Lesson 9)

37. Does every language's `Parser` implementation push a synthetic `Unit` root
    space whenever the grammar's actual root node kind is not the language's
    canonical translation-unit kind? Verify the invariant `blank ≥ 0` holds
    for all files in the test corpus: any `blank < 0` in a snapshot is a sign
    that the root is misidentified.

#### `is_else_if` structural model (Lesson 10)

38. Does `is_else_if()` for each language implement the correct structural
    model for that grammar? The four known models are:

    | Model | Languages | Check |
    |-------|-----------|-------|
    | `else_clause` wrapper | C++, Mozjs, JS, TS, TSX, Rust | `parent().kind_id() == ElseClause` |
    | `Else` keyword sibling | Java, C#, Kotlin | `prev_sibling().kind_id() == Else` |
    | Nested `if_statement` | Go | `parent().kind_id() == IfStatement` |
    | Dedicated clause node | Python, Perl, Lua, Bash, Tcl, PHP | `node.kind_id() == ElseClause` |

    Any language that returns a constant (`false` or `true`) unconditionally
    is a finding. Write a test that verifies a non-trivial `else if` chain
    produces a lower cognitive score than the same depth with independent `if`
    blocks.

#### Cross-language metric parity (Lesson 11)

39. For each of the following constructs, build a per-language audit table and
    confirm every language counts it consistently:

    | Construct | Expected behaviour |
    |-----------|-------------------|
    | 2-arm switch/match with wildcard/default | Standard CCN +1 for non-wildcard arm only; wildcard/default should NOT increment |
    | `for`-each / enhanced-for / range-based-for | +1 for the loop boundary; any language missing the specific grammar node kind is a finding |
    | Ternary / conditional expression `?:` | +1 for cognitive; confirm every language's dispatch table includes the grammar's ternary node kind |
    | `else if` chain (3 levels deep) | Cognitive: flat +1 per continuation, not +nesting per level |
    | `case…esac` / switch container | Container node itself should NOT increment; only arms increment |

    Any discrepancy not documented in `lessons_learned.md` as a known exception
    is a finding.

#### Interpolation guard in Halstead (Lesson 11 — extended)

40. Does every language's `get_op_type()` in `getter.rs` include the
    `is_child(<Lang>::Interpolation as u16)` guard for string-literal nodes
    that contain interpolated sub-expressions? The contract: an interpolated
    string literal contributes *only* its inner expressions as Halstead
    operands — the wrapping literal itself should be `Unknown`. Any language
    whose string classification unconditionally returns `Operand` for its
    string-literal kind is a finding if that string kind can contain
    interpolation.

#### Tree-sitter cursor misuse (Lesson 12)

41. In every AST helper that takes a `TreeCursor` argument: does the iteration
    method call the **right** node? Specifically, is there any pattern where
    `self.0.children(cursor)` is called when `parent.children(&mut
    parent.walk())` was intended? The compiler accepts both; only a test that
    distinguishes "iterates self's children" from "iterates parent's children"
    catches this. Flag any helper whose argument is a cursor that is
    immediately reset by the called method (i.e., the cursor argument is dead
    weight).

#### Dispatch table coverage (Lesson 19)

42. For each metric × language combination, scan the language module for node
    kinds whose names suggest they should contribute to the metric but that are
    absent from the dispatch table. Use these `rg` patterns as a starting
    point:

    ```bash
    # Loops that might be missing from cyclomatic/cognitive dispatch:
    rg 'For[A-Z]' src/languages/
    # Ternary/conditional that might be missing:
    rg 'Conditional|Ternary' src/languages/
    # Enhanced/range-based loop variants:
    rg 'Enhanced|Range|For[A-Z]' src/languages/
    # Pattern matching / structural match:
    rg 'Match|Case[A-Z]|Pattern' src/languages/
    # Nullish / short-circuit operators:
    rg 'Nullish|NullCoal' src/languages/
    ```

    For each candidate kind found, confirm it is either:
    - Explicitly matched in the metric's dispatch arm, OR
    - Explicitly excluded with a comment explaining why it should not count.

    Any kind that silently falls through to a non-contributing arm without a
    comment is a finding.

#### Hidden-rule alias byte ranges (Lesson 21)

43. In any test that pins Halstead operand counts on a fixture containing
    language interpolation (string templates, `$name` sigils, `#{expr}`,
    heredoc splices): has the test author verified the actual byte range of
    each identifier node by dumping the AST? Any count derived by assuming
    `node.utf8_text(src) == node.kind()` for an interpolated identifier is
    suspect — sigil-prefixed hidden rules make the byte range include the
    delimiter. The fix is to derive expected counts empirically from the actual
    parse, with an explanatory comment.

#### Text-keyed dispatch needs `&[u8]` (Lesson 22)

44. For each language that encodes branch type, visibility (`private`/`public`/
    `protected`), or attribute semantics as **bare identifier text** rather
    than a distinct token kind: does the per-language metric impl accept
    `&[u8]` in its compute signature? Any impl that classifies a node as
    contributing to a metric based on its kind alone, when the same kind is
    used for all identifiers in the language, is a finding — the text content
    is required to distinguish the semantic.

#### Compensation constants in parity tests (Lesson 23)

45. Do any cross-language or cross-metric parity tests contain a named constant
    (e.g., `PYTHON_ELSE_BUG_OFFSET`, `C_TERNARY_DELTA`) that adds an offset
    to one language's expected value to make the test pass? Such constants
    blind the test to future regressions in the same metric path. If found:
    either fix the underlying bug (and remove the constant), `#[ignore]` the
    test with the tracking issue number, or FIXME-lock the wrong expected value
    per the pattern in Lesson 19.

#### Per-metric gating must cover finalize helpers (Lesson 24)

46. For any selective-metric gating mechanism (`MetricsOptions::with_only` or
    equivalent): is the gate applied to **every** finalize helper
    (`compute_minmax`, `compute_sum`, `compute_averages`,
    `compute_halstead_mi_and_wmc`) as well as to `compute_per_node`? Any
    finalize helper that runs unconditionally for unselected metrics will
    propagate a non-zero default (e.g., Cyclomatic's `1.0` McCabe baseline)
    into the headline value. A test that verifies "this metric was skipped"
    must assert `== 0.0` — not `> 0` — on the unselected metric whose default
    is non-zero.

#### `spawn_blocking` orphan-task DoS (Lesson 13)

47. In `big-code-analysis-web`: does every `spawn_blocking` (or
    `actix_web::web::block`) call that processes user-controlled input have
    protection against orphan-task pool saturation? `tokio::time::timeout`
    cancels the *await* of the join handle, not the blocking task itself.
    Verify that one of the following holds:
    - An orphan-task counter rejects new requests once the threshold is
      exceeded, OR
    - The input is size-bounded such that worst-case runtime is a small
      multiple of the timeout.
    A semaphore alone is not sufficient.

#### Forked enum identity collapse (Lesson 14)

48. In any helper that branches on `LANG::get_name()` output: are tests driven
    **through the enum** (`LANG::Tsx`, `LANG::Javascript`) rather than through
    literal strings (`"tsx"`, `"javascript"`)? Variants whose `get_name()`
    collapses to another variant's name (TSX → `"typescript"`, Mozjs →
    `"javascript"`) have unreachable arms in literal-keyed helpers; literal-
    string tests exercise those dead arms without triggering the real code
    path. Any helper tested only with literal strings that does branching on
    `get_name()` output is suspect.

#### Hand-rolled `Serialize` field count (Lesson 28)

49. Does any hand-rolled `Serialize` impl (`impl Serialize for X`) pre-count
    fields for a length-prefixed format (CBOR, MessagePack)? If yes:
    - Is the pre-count tally derived from the same predicate set as the
      conditional body arms?
    - Is there an end-to-end CBOR or MessagePack round-trip test (not just
      JSON)? JSON quietly tolerates a wrong field count; only a
      length-prefixed format catches the tally mismatch at `st.end()`.

#### NaN/infinity panics in sort and comparison (Lesson 5)

50. Is any `partial_cmp().unwrap()` on `f64` metric values reachable with NaN
    or infinity input? Sort functions must use `f64::total_cmp` (or an
    equivalent total ordering). Tests that verify NaN-safety must call the sort
    function **directly** with NaN input — wrapper-level end-to-end tests
    almost always have an upstream filter that removes NaN before it reaches
    the sort, causing the test to pass for the wrong reason.

---

## Step 5: Merge and deduplicate findings

Collect all FINDING records from all six sub-agents. For each finding:

1. Re-read and confirm the evidence is concrete (file + line + reasoning).
2. Cross-check against existing open issues (`gh issue list`). Drop exact
   duplicates; note the existing issue number.
3. Confirm the category (`bug`, `security`, `enhancement`, `documentation`) —
   this is the GitHub label Step 6 will apply.

**Upstream-grammar findings**: if the root cause lives in an upstream
tree-sitter grammar crate, still file the issue but add the
`upstream-grammar` label (see Step 6).

---

## Step 6: File GitHub issues

### 6a: Ensure category labels exist

```bash
ensure_label() {
  local name="$1" color="$2" desc="$3"
  if ! gh label list --limit 200 --json name --jq '.[].name' | grep -qx "$name"; then
    gh label create "$name" --color "$color" --description "$desc"
  fi
}

ensure_label bug              "d73a4a" "Something isn't working"
ensure_label enhancement      "a2eeef" "New feature or request"
ensure_label documentation    "0075ca" "Improvements or additions to documentation"
ensure_label security         "ee0701" "Security-relevant finding"
ensure_label upstream-grammar "fbca04" "Cannot be fixed locally; needs upstream tree-sitter grammar change"
```

### 6b: Create one issue per surviving finding

Template:

```markdown
## Summary

<One-sentence description of the problem>

## Location

- `<file path>:<line numbers>`

## Evidence

<Code snippet or reasoning showing the problem>

## Expected Behavior

<What should happen instead>

## Actual Behavior

<What currently happens>

## Impact

<Who is affected and how>
```

Write to a temp file:

```bash
cat > /tmp/issue-body.md <<'EOF'
...body here...
EOF
gh issue create --title "scope: short description" \
  --label "bug" \
  --body-file /tmp/issue-body.md
```

For upstream-grammar findings, add both the category label and the
`upstream-grammar` label, and include an `## Upstream Grammar` section in the
body naming the grammar crate and version and explaining why a local fix is
partial or impossible.

---

## Step 7: Add fix-plan comments

For each issue filed, add a comment:

```markdown
## General Plan for Fix

### Root Cause

<Why the problem exists>

### Implementation Steps

1. <Smallest change that fixes the problem>
2. <Next step if needed>

### Tests to Add/Update

- <Test 1>
- <Test 2>

### Verification

- [ ] `cargo test --workspace --all-features` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] Manual verification: <specific scenario>
```

```bash
cat > /tmp/fix-plan.md <<'EOF'
...plan here...
EOF
gh issue comment <NUMBER> --body-file /tmp/fix-plan.md
```

---

## Step 8: Summary table

Print to the terminal:

```
| # | Title | Category | File | Issue |
|---|-------|----------|------|-------|
```

Also list:
- Any findings dropped as duplicates of existing issues.
- Any partitions skipped (priority 5 — recently scanned and unchanged).
- Any partitions whose scope listed files that did not exist on disk —
  name the missing files. This catches drift between the skill's
  partition tables and `src/languages/`.

If `/tmp/scan-metrics.json` from Step 2b is missing or empty, prefix the
summary with a `DEGRADED: metric hotspots unavailable — <reason>` banner
(quoting the relevant line from `/tmp/scan-cli-build.log` or
`/tmp/scan-metrics.err`). A degraded run must not be confused with a
clean one in `scan-project-state`.

---

## Step 9: Save scan state

Invoke `serena:write_memory` with `memory_name: "scan-project-state"` and the
merged content as `content`.

Build the memory by **merging** prior state (from Step 1) with the current
session's partition coverage:

- For partitions scanned in this session: update depth, date, model, findings count.
- For partitions NOT scanned: preserve existing record unchanged.
- For new files discovered but not audited: add with depth `none`.

A partition may be recorded as `full` only if every file in its scope
was successfully read. If a sub-agent reported "no such file" for any
listed path (e.g., a stale entry in this skill's partition tables),
record the partition as `partial` and append a `missing: <comma-
separated paths>` annotation to that row so the gap is visible in the
next run's Step 1.

Format:

```
# Scan State: scan-project
last_scan: YYYY-MM-DD
last_model: <model-id>

## Partition Coverage
A-metrics-core   | full    | 2026-05-19 | 3 findings  | claude-opus-4-6
B-js-family      | full    | 2026-05-19 | 2 findings  | claude-opus-4-6
C-c-family       | partial | 2026-05-19 | 1 finding   | claude-opus-4-6
D-other-langs    | none    | -          | -           | -
E-infrastructure | full    | 2026-05-19 | 0 findings  | claude-opus-4-6
F-cli-web        | full    | 2026-05-19 | 1 finding   | claude-opus-4-6
```

---

## Step 10: Verify clean working tree

**This step is MANDATORY and must be the last action before returning.**

```bash
git status --porcelain
```

If any output appears:

- For **modified tracked files** (`M` lines): revert with
  `git checkout -- <path>`. Do NOT use a blanket `git checkout -- .` in
  worktree mode.
- For **untracked files** (`??` lines): do NOT delete them. Surface their paths
  in the final summary for user review. Never run `git clean` from this skill.

Report anomalies (which files, which step likely produced them) in your final
summary.

---

## Guardrails

All ABSOLUTE CONSTRAINTS (top of this document) apply. Additionally:

- **NEVER delete worktrees.** Only the Claude Code runtime may do that.
- Do NOT implement fixes. This is scan-only.
- Do NOT file findings without concrete evidence (file + line + reasoning).
- Do NOT file duplicate issues. Always check `gh issue list` first.
- Use `--body-file` for all `gh issue create` and `gh issue comment` calls.
- Sub-agents share the parent context (read-only); they do NOT need separate
  isolation.
- When a question asks you to "build an audit table," produce it as a markdown
  table in your findings output even if no issues result — the table is
  evidence of coverage.
