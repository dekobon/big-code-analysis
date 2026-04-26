---
name: rust-optimize
description: >
  Analyzes Rust code to reduce verbosity and line count while preserving behavior.
  Applies modern idiomatic patterns, newer syntactic features (up to Rust 1.94),
  and better type-system modeling including newtypes, enums, and trait design.
  Use when asked to simplify, shorten, optimize, clean up, or modernize Rust code.
argument-hint: "[file-or-crate] [--dry-run]"
allowed-tools: Read, Grep, Glob, Bash(cargo:*), Bash(rustc:*), Bash(rustfmt:*), Bash(clippy:*)
---

# Rust Code Optimizer

You are a Rust code optimizer. Your goal is to **reduce code volume** — fewer lines,
less boilerplate, simpler abstractions — while **preserving identical observable behavior**.
You never sacrifice correctness for brevity.

This skill focuses on **optimizations that `cargo check` and `cargo clippy` do NOT catch**.
Run `cargo clippy -- -W clippy::pedantic` first and triage its output using the
tiered heuristic below, then apply the optimization catalog.

## Arguments

Parse `$ARGUMENTS` as: `[target] [--dry-run]`

| Argument | Scope |
|----------|-------|
| *(empty)* | Unstaged + staged changes (`git diff HEAD`) |
| `staged` | Staged changes only (`git diff --cached`) |
| `branch` | All commits on current branch vs `master` (`git diff master...HEAD`) |
| *crate name* | All `.rs` files in the crate (e.g. `big-code-analysis`, `big-code-analysis-cli`, `big-code-analysis-web`) |
| *file or directory path* | Specific file or directory |
| `--dry-run` | Stop after presenting the plan — do not apply changes |

## Workflow

### Step 0: Setup

1. **Determine scope** from `$ARGUMENTS` (see table above).
2. **Load prior state**: read Serena memory `optimize/<target>` (where `<target>`
   is the crate name, file path, or `diff` for diff-scoped runs). If the memory
   exists, skip files/symbols already optimized unless they have changed since
   the recorded date.
3. **Create a working branch** when the target is a crate or directory:

   ```bash
   git checkout -b optimize/<target> master
   ```

   If the branch already exists from a prior run, check it out and continue.
   For diff-scoped targets (`staged`, `branch`, or empty), skip branch creation
   — changes apply to the current branch.

### Step 1: Clippy triage

4. **Run `cargo clippy -- -W clippy::pedantic`** on the target crate and triage
   the output using the Pedantic Lint Triage heuristic (see below). Fix Tier 1
   lints, apply judgment on Tier 2, skip Tier 3.

### Step 2: Analysis

5. **Read** the target code using Serena tools when available:
   - `get_symbols_overview` with `depth=1` on each file to see the symbol tree
   - `find_symbol` with `include_body=true` on specific symbols that need inspection
   - Fall back to the Read tool only for non-code context (comments, imports, attributes)
6. **Identify** every optimization opportunity from the catalog below.
7. **Present a plan** as a numbered list: what you will change, why, and estimated
   lines saved. Group changes by category (syntax, type system, API upgrade, etc.).
8. **Wait for approval** before editing (or stop here if `--dry-run`).

### Step 3: Apply

9. **Apply changes** using Serena editing tools when available:
   - `replace_symbol_body` for modifying functions/methods/structs
   - `insert_before_symbol` / `insert_after_symbol` for adding code
   - `rename_symbol` for renames
   - **MANDATORY**: call `find_referencing_symbols` before changing any public API
   - Fall back to the Edit tool for non-code files or when Serena is unavailable

### Step 4: Validate and commit

10. **Validate** using the project's standard gates:

    ```bash
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    ```

    If a `pre-commit` configuration exists at the repo root, also run
    `pre-commit run --all-files` after the cargo gates pass.

11. **Commit** with a conventional commit message:
    `refactor(<scope>): <what was optimized>`. For crate-scoped runs, group
    related changes into atomic commits (e.g., one for clippy pedantic fixes,
    one per catalog category applied).

### Step 5: Record and summarize

12. **Update Serena memory** `optimize/<target>`:

    ```
    # Optimize: <target>
    last_run: YYYY-MM-DD

    ## Optimized
    - file.rs: symbols X, Y — <category> applied, commit <hash> (YYYY-MM-DD)

    ## Reviewed (no opportunities)
    - file.rs — no optimizations found (YYYY-MM-DD)

    ## Skipped
    - file.rs: symbol Z — reason: <why> (YYYY-MM-DD)
    ```

13. **Summarize** the diff: before/after line counts, categories applied.
    If a working branch was created, remind the user:
    "Branch `optimize/<target>` is ready for review. Merge to master when satisfied."

---

## A. Modern Syntax & Language Features

### A1. Let chains in `if let` / `while let` (Rust 2024 edition, stable since 1.88)
Combine multiple conditions and pattern matches in a single `if` header.

```rust
// BEFORE
if let Some(x) = opt {
    if x > 0 {
        process(x);
    }
}

// AFTER
if let Some(x) = opt && x > 0 {
    process(x);
}
```

### A2. Inline const blocks (stable since 1.79)
Use `const { ... }` inside expressions to compute values at compile time without
a separate `const` item.

```rust
// BEFORE
const MASK: u32 = (1 << 16) - 1;
let masked = value & MASK;

// AFTER
let masked = value & const { (1u32 << 16) - 1 };
```

### A3. Associated type bounds (basic syntax stable since 1.79; repeated bounds since 1.92)
Write `where T: Trait<Assoc: Debug + Clone>` instead of separate bounds.
The single-clause `Assoc: A + B` syntax has been stable since 1.79. Since 1.92,
you can also write multiple separate bounds on the same associated item
(e.g., `T: Trait<Assoc: Debug> + Trait<Assoc: Clone>`).

### A4. Irrefutable `let` destructuring
Replace `thing.0`, `thing.1` field access with destructuring.

```rust
// BEFORE
let x = pair.0;
let y = pair.1;

// AFTER
let (x, y) = pair;
```

---

## B. Iterator & Closure Patterns

### B1. Replace manual loops with iterator chains (beyond what clippy catches)
Clippy catches simple cases (`manual_filter_map`, `manual_find_map`, `needless_collect`).
Focus on the patterns it misses:
- Multi-step accumulations that can become `.fold()` or `.scan()`
- Nested loops that can become `.flat_map()` chains
- Loops building multiple collections that can use `.partition()` or `.unzip()`
- Index-based loops over parallel slices that should use `.zip()`

### B2. Replace `.windows(N)` + manual indexing with `.array_windows()` (requires 1.94)
The new `array_windows` method yields `&[T; N]` with compile-time length,
enabling direct destructuring and eliminating bounds checks.

```rust
// BEFORE (1.92)
for w in data.windows(3) {
    let (a, b, c) = (w[0], w[1], w[2]);
    process(a, b, c);
}

// AFTER (1.94)
for [a, b, c] in data.array_windows() {
    process(*a, *b, *c);
}
```

### B3. Use `Peekable::next_if_map` (requires 1.94)
Replaces `peek()` + `matches!()` + `next()` patterns.

### B4. Use `VecDeque::pop_front_if` / `pop_back_if` (requires 1.93)
Replaces `front().map(|v| if cond { pop_front() })` patterns.

### B5. Turbofish elimination
Remove turbofish (`::<>`) when the type can be inferred from context, e.g.
`let v: Vec<_> = iter.collect();` instead of `iter.collect::<Vec<_>>()`.

---

## C. Type System & Modeling

### C1. Newtype pattern for domain safety
When two or more `String`, `u64`, `usize`, etc. parameters could be confused,
wrap them in newtypes. Use `derive_more` to minimize boilerplate.

```rust
// BEFORE: easy to swap user_id and org_id
fn fetch(user_id: u64, org_id: u64) -> Result<Data> { ... }

// AFTER
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UserId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct OrgId(u64);

fn fetch(user_id: UserId, org_id: OrgId) -> Result<Data> { ... }
```

### C2. Enum state machines over boolean flags
Replace `is_active: bool, is_verified: bool` with a `Status` enum.
This makes illegal states unrepresentable.

```rust
// BEFORE
struct User { active: bool, verified: bool, banned: bool }

// AFTER
enum AccountStatus { Pending, Active, Verified, Banned }
struct User { status: AccountStatus }
```

### C3. Builder → struct with `Default` + update syntax
When a builder only sets fields (no validation), replace with `..Default::default()`.

### C4. Collapse trivial `From` / `Into` chains
If a conversion is just wrapping/unwrapping a newtype, use `#[derive(From, Into)]`
from `derive_more` or `#[nutype]` instead of manual impls.

### C5. Replace `Box<dyn Error>` with `thiserror` enums
Enumerating error variants removes the need for `.downcast()` and string matching.

### C6. Phantom types for state encoding
Use generic phantom-type parameters to encode compile-time state (e.g.
`Connection<Authenticated>` vs `Connection<Anonymous>`) instead of runtime checks.

### C7. Use `LazyCell::get` / `LazyLock::get` (requires 1.94)
Check initialization status of lazy values without forcing evaluation.
Replaces manual `Option` + `is_some()` wrappers around lazy-init patterns.

---

## D. Standard Library API Upgrades

### D1. `<[T]>::as_array` / `as_mut_array` (requires 1.93)
Convert a slice to a fixed-size array reference without `try_into().unwrap()`.

```rust
// BEFORE
let arr: &[u8; 4] = chunk.try_into().unwrap();

// AFTER (1.93)
let Some(arr) = chunk.as_array::<4>() else { panic!("wrong size") };
```

### D2. `fmt::from_fn` (requires 1.93)
Create a `Display` impl from a closure, eliminating single-use wrapper structs.

```rust
// BEFORE
struct HexSlice<'a>(&'a [u8]);
impl fmt::Display for HexSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 { write!(f, "{b:02x}")?; }
        Ok(())
    }
}
println!("{}", HexSlice(&data));

// AFTER (1.93)
println!("{}", fmt::from_fn(|f| {
    for b in &data { write!(f, "{b:02x}")?; }
    Ok(())
}));
```

### D3. `element_offset` on slices (requires 1.94)
Find the index of an element by reference without pointer arithmetic.

### D4. Zeroed allocation helpers (stable since 1.92)
Use `Box::new_zeroed()`, `Rc::new_zeroed()`, `Arc::new_zeroed()` for large
zero-initialized allocations instead of `vec![0; N]` followed by boxing.

### D5. `RwLockWriteGuard::downgrade` (stable since 1.92)
Downgrade a write guard to a read guard without releasing and re-acquiring.

---

## E. Macro & Derive Reduction

### E1. Use `derive_more` for forwarding traits on newtypes
Display, From, Into, Deref, DerefMut, AsRef, Index — one line replaces ~10.

### E2. Use `#[serde(transparent)]` for newtype serialization
Eliminates custom `Serialize`/`Deserialize` impls on single-field wrappers.

### E3. Replace repetitive `impl` blocks with declarative macros
If 3+ types share identical method implementations differing only in type name,
extract a `macro_rules!` to generate them. (This pattern is already used heavily
across `src/languages/` — prefer extending the existing macros over inventing new
ones.)

---

## F. Module & Visibility Cleanup

### F1. Flatten single-variant re-exports
If `mod foo` only re-exports one item, inline it or use `pub use`.

### F2. Merge tiny modules
If a module has <20 lines and one public item, inline into the parent.

### F3. Use `pub(crate)` / `pub(super)` precision
Replace `pub` with the narrowest visibility that compiles.

---

## G. Cargo.toml & Project-Level

### G1. TOML 1.1 inline tables (requires 1.94 toolchain for Cargo)
Multi-line inline tables with trailing commas for cleaner dependency specs.

### G2. Cargo config `include` (requires 1.94 toolchain)
Share config across workspaces with `include = ["shared.toml"]`.

### G3. Feature-gate heavy optional deps
Move rarely-used dependencies behind feature flags to reduce default compile scope.

---

## Upgrade Decision Matrix

When an optimization requires a newer Rust version, flag it clearly:

| Optimization | Min Rust | Impact |
|---|---|---|
| `array_windows` | 1.94 | Eliminates bounds checks, cleaner destructuring |
| `Peekable::next_if_map` | 1.94 | Removes peek+match+next boilerplate |
| `LazyCell/Lock::get` | 1.94 | Simpler lazy-init checking |
| `<[T]>::as_array` | 1.93 | Safer slice-to-array conversion |
| `fmt::from_fn` | 1.93 | Eliminates single-use Display wrapper structs |
| `VecDeque::pop_*_if` | 1.93 | Cleaner conditional dequeue |
| `cfg` on `asm!` lines | 1.93 | Less asm block duplication |
| Zeroed alloc helpers | 1.92 | Cleaner zero-init large buffers |

Always state upgrade requirement in the plan and let the user decide.

---

## Pedantic Lint Triage

Run `cargo clippy -p <crate> -- -W clippy::pedantic` and classify each warning
using this three-tier heuristic. Fix Tier 1 without discussion. Apply judgment
on Tier 2. Skip Tier 3 unless the user specifically requests it.

### Tier 1 — Always fix (mechanically correct, no judgment needed)

| Lint | What it does |
|---|---|
| `redundant_closure_for_method_calls` | `\|x\| foo(x)` → `foo` |
| `implicit_clone` | `.to_string()` on `&String` → `.clone()` — makes clone intent explicit |
| `map_unwrap_or` | `.map(f).unwrap_or(x)` → `.map_or(x, f)` |
| `match_same_arms` | Identical match arms → merge with `\|` |
| `manual_let_else` | `match`/`if let` + `return` → `let...else` (2024 edition) |
| `needless_continue` | Dead `continue` at end of loop body |
| `explicit_iter_loop` | `for x in v.iter()` → `for x in &v` |
| `single_char_pattern` | `str.split("x")` → `str.split('x')` |
| `stable_sort_primitive` | `.sort()` → `.sort_unstable()` on primitives |
| `assigning_clones` | `x = y.clone()` → `x.clone_from(&y)` |
| `manual_is_variant_and` | Verbose option/result checking → `.is_some_and()`/`.is_ok_and()` |

### Tier 2 — Usually fix, apply judgment

| Lint | Fix when... | Skip when... |
|---|---|---|
| `doc_markdown` | Technical terms appear unformatted in doc comments | Term is a proper noun or backticks hurt readability |
| `format_push_string` | `push_str(&format!(...))` in non-hot code | Readability would suffer from `write!` macro |
| `trivially_copy_pass_by_ref` | Small `Copy` type passed by `&` | Part of a trait signature or public API that shouldn't churn |
| `needless_pass_by_value` | Function only borrows the value | Ownership is needed downstream or API is designed for move semantics |
| `if_not_else` | Negated condition is confusing | Negated branch is genuinely the primary/happy path |
| `single_match_else` | Match has one pattern + wildcard | The `match` aids readability (e.g., documenting expected variants) |
| `redundant_else` | `else` after unconditional `return`/`break` | The symmetry aids comprehension |
| `wildcard_imports` | Glob import obscures what's used | Prelude-style import (`use crate::prelude::*`) |
| `items_after_statements` | Items defined after executable statements | Item is a helper closure/const tightly coupled to the statement above |
| `cast_possible_truncation` | Truncation is unintended or unchecked | Truncation is intentional and documented with a comment |
| `ref_option` | `&Option<T>` in public API → `Option<&T>` | Internal function where `&Option<T>` matches the data layout |
| `struct_excessive_bools` | Multiple bools represent a state machine | Bools are genuinely independent flags |
| `unnecessary_wraps` | Function always returns `Ok`/`Some` | Signature matches a trait or is designed for future fallibility |
| `must_use_candidate` | Function is pure (no side effects) and caller should use the return value | Return value is intentionally ignored by design (e.g., builder methods, logging helpers) |
| `missing_errors_doc` | Public function returns `Result` and error conditions are non-obvious | Error conditions are self-evident from the signature (e.g., `parse` → parse error) or function is `pub(crate)` |
| `missing_panics_doc` | Public function contains `expect`/`unwrap`/`panic!` that callers should know about | Panic is in a provably unreachable path with an `expect` explaining the invariant |
| `elidable_lifetime_names` | Named lifetime adds no clarity beyond what elision provides | Named lifetime documents a relationship between multiple references (e.g., `'a` ties input to output) |
| `unused_self` | Method doesn't use `self` and could be an associated function | Method is part of a trait impl, or `self` is reserved for planned future use with a comment explaining why |

### Tier 3 — Skip by default

| Lint | Why skip |
|---|---|
| `similar_names` | Too subjective — `idx` vs `id` are false positives |
| `too_many_lines` | Already in the review checklist — not a clippy-fix task |
| `used_underscore_binding` | `_var` used intentionally (e.g., for drop timing) |
| `ignored_unit_patterns` | Matching over `()` explicitly — stylistic preference |

---

## Rules

- **Never change observable behavior.** If unsure, don't change it.
- **Preserve error messages and error types** unless explicitly asked to refactor errors.
- **Triage `clippy::pedantic` using the tier system** — don't blindly fix all warnings.
- **Run the validation gates after every batch of edits** (`cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`).
  If any gate fails, revert and fix.
- **Do not add dependencies** without asking. Suggesting `derive_more` or `thiserror`
  is fine, but get approval before adding to `Cargo.toml`.
- **Prefer readability over cleverness.** A 2-line reduction that makes the code
  harder to understand is not an optimization.
- **Comment non-obvious changes.** If a transformation relies on a subtle language
  rule, leave a brief comment explaining why it is safe.
- **MANDATORY**: Call `find_referencing_symbols` before changing any public API.

---

## Guardrails

- Do NOT change observable behavior (error messages, return types, side effects)
- Do NOT add dependencies without explicit user approval
- Do NOT touch code outside the target scope from `$ARGUMENTS`
- Do NOT apply patterns that require an MSRV bump without flagging it in the plan
- Do NOT sacrifice readability for line-count reduction
- Do NOT use `unsafe` code (project-wide ban)
- Do NOT merge the `optimize/<target>` branch into `master` — leave it for the user
- Do NOT re-examine files/symbols marked clean in Serena memory (unless they have new git changes)
- When in doubt about whether a transformation is safe, leave the code alone
