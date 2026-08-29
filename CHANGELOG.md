# Changelog

All notable changes to `big-code-analysis` are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html)
from the fork onwards.

Stability note: the crate is on the `2.x` line. The public Rust
API surface (`big-code-analysis` library re-exports, the `bca` CLI
argument grammar, and the `bca-web` REST schema) is held stable
across patch and minor bumps; breaking shape changes are reserved
for the next major bump and appear under **(breaking)**. The
`2.0.0` release is the first major break since the `1.x` line — its
section below opens with consolidated migration notes (a serialized
key map and the metric-value re-baseline) ahead of the detailed
**(breaking)** entries. Metric *values* may still drift across minor
bumps when a grammar pin moves or a metric definition is fixed —
each drift is called out in the entry that introduces it.

See [STABILITY.md](./STABILITY.md) for the full contract. Entries
in `0.x` sections below describe pre-policy behaviour and are kept
for historical reference.

## [Unreleased]

### Added

- `bca check` now reports what it declined to look at (#1055). When the
  gate skipped files — a generated-code marker (`@generated`,
  `DO NOT EDIT`, `GENERATED CODE`) or a VCS ignore file committed in
  the tree under test — a one-line stderr summary names the counts:
  `bca: 2 files not checked (1 generated, 1 ignored) — pass
  --report-skipped to list them`. Either input could previously remove
  a file from a pull-request gate with nothing on stderr. The counts
  are measured at the walk's prune points and cover only files a
  parser owns (a generated `Cargo.lock` or an ignored log file was
  never a gate bypass), never a file named explicitly on the command
  line; a whole ignored directory is reported — under
  `--report-skipped` only, since ignored build trees exist in
  essentially every checkout — as its own
  `N ignored directories not walked` clause and is never entered, so
  the summary stays cheap and small on trees with large ignored build
  output. Clean runs stay silent, exit codes are unchanged, and
  `--report-skipped` also lists each ignore-dropped entry
  (`note: skipped (ignored): …`,
  `note: skipped (ignored directory): …`).
- `bca check --strict`, plus a matching `[check] strict` manifest key
  (#1055): the untrusted-input gate profile. Flips both skip defaults
  in one flag (equivalent to `--no-skip-generated --no-ignore`), so a
  PR gate opts out of content sniffing and in-tree ignore files without
  remembering two flags. The manifest key is presence-only (it can turn
  the profile on, never off) so a project opts in once rather than per
  workflow.

- A `cargo-fuzz` crate (`fuzz/`, workspace-excluded) with eleven libFuzzer
  targets over the parse-and-walk layer, committed seed corpora, and an
  out-of-band `fuzz` workflow (`make fuzz-check` / `fuzz-smoke` /
  `fuzz-run`; see
  [`docs/development/fuzzing.md`](docs/development/fuzzing.md)). The
  target set is scoped to what the static lints adopted in #1152 cannot
  reach — chiefly the nine per-function `indexing_slicing` carve-outs in
  the C-family macro lexer, which only the `preproc_macro` target
  exercises — rather than to blanket byte fuzzing, which a 112-file
  adversarial corpus already showed the encoding/IO layer survives.
  Targets call `Source::from_bytes` so the bytes reach the parser
  unnormalised; the file-reading path appends a trailing newline, and a
  harness that inherited it would pass vacuously against the very class
  of bug (#1051) that motivated this. The tree-sitter grammars are
  compiled with `-fsanitize=address` too, so the C scanners are covered
  and not just the Rust. Its first bounded run found a bounded leak in
  `tree-sitter-perl` 1.1.2's heredoc scanner, whose
  `external_scanner_destroy` has its queue frees commented out — upstream
  in the grammar, recorded in `fuzz/lsan-suppressions.txt` with the
  measurement showing it is per-thread rather than per-parse. No library
  behaviour changes (#1154). A `preproc_includes` target covers the
  include-resolution half of the preprocessor — `fix_includes` and the
  graph building, SCC collapse and candidate scoring behind it, none of
  which `preproc_macro` reaches (#1288).
- A `check-ruff-lockstep` gate (`make check-ruff-lockstep`, wired into
  `make lint` / `pre-commit` / `ci` and the pre-commit hooks) holds the one
  adopted ruff version together across the four files that declare it.
  `big-code-analysis-py/uv.lock` is the anchor, because the hash-pinned
  `requirements/dev.txt` CI installs from is *generated* from it — gating on
  the export would bless a stale export rather than catch one. The
  `ruff-pre-commit` `rev:` in `.pre-commit-config.yaml` must be `v` + the
  locked version, the export must pin it, and `pyproject.toml`'s bound must
  match the one uv recorded resolving against. The `rev:` was previously held
  by a comment alone and had already drifted silently once (`v0.15.14`
  against a lockfile resolving 0.15.22), which stays invisible until the two
  versions disagree and then presents as "works locally, red in CI" (#1230).
- A `check-safety-doc-pin` gate (`make check-safety-doc-pin`, wired into
  `make lint` / `pre-commit` / `ci`, the pre-commit hooks, and its own CI
  step) holds the `tree-sitter` version cited by the `unsafe` soundness
  argument in `big-code-analysis-py/src/node.rs` equal to the version
  `[workspace.dependencies]` pins. That module doc is the canonical
  justification for the workspace's only sanctioned `unsafe` block and
  reasons about a *named* release — the `Node<'tree>` layout, `Tree::edit`
  taking `&mut self`, `Send + Sync` — so a pin that moves while the literal
  does not leaves an argument reading as verified against a crate nobody
  compiles. The gate also fails when the literal is dropped altogether,
  since a version-free phrasing hides the staleness rather than fixing it.
  The scanner reads every `//!` line in the file rather than stopping at
  the first non-doc line, so a multi-line `#![allow(…)]` between doc
  paragraphs cannot silently truncate the scan in either direction
  (#1345). No library behaviour changes (#1057).

- `SkipReason` (`#[non_exhaustive]`, with a `Display` that renders a
  noun phrase) and `read_file_with_eol_classified`, the reason-naming
  sibling of `read_file_with_eol` — `io::Result<Result<Vec<u8>,
  SkipReason>>`, of which the old function is now the `.map(Result::ok)`
  projection (#1287). The four variants map 1:1 onto the reader's skip
  gates: `Empty`, `TooSmall` (1–3 bytes, or a file that shrank below the
  probe window), `Utf16Bom`, and `NotUtf8`. Prefer the classified
  variant wherever a skip is reported to a user; the shape is recorded
  in `STABILITY.md`, whose per-variant `Display` wording is not
  SemVer-protected.

- **web:** `error_kind` token `vcs_invalid_author_hash_key`. The
  `STABILITY.md` vocabulary list also gains `not_acceptable` and
  `serialize_failed`, added by #657 and never documented (#1245).

### Fixed

- Bash command names are no longer counted twice in Halstead `N2`
  (#1351). `BashCode::get_op_type` classified both the `command_name`
  wrapper and the `word` / `string` / `expansion` it wraps as operands,
  so `ls bar` reported `N2` 3 for two operands, and the spellings whose
  wrapper text differs from the child's (`"$cmd"`, `${cmd}`) also planted
  a spurious second `n2` entry. The wrapper arm is gone; the child
  carries the count for all fourteen spellings the pinned grammar
  admits. The dead `Bash::Concat` arm — the hidden `_concat` external
  token, never emitted — was removed with it and pinned by a drift
  marker. **Metric drift:** Bash `N2` falls by one per command name,
  moving `length`, `volume`, `difficulty`, `level`, `effort`, `time`
  and `bugs`.
- Ruby `%w[…]` / `%i[…]` arrays and adjacent-string concatenation no
  longer count an extra Halstead operand for the wrapper (#1353).
  `RubyCode::get_op_type` suppressed a string-like literal only when it
  carried an `Interpolation` child, but `chained_string`, `string_array`
  and `symbol_array` hold classified operand children instead, so
  `%w[x y]` scored `n2` 4 / `N2` 4 for three operands — a vocabulary
  that depended on how the literals were grouped. The three kinds are
  gated rather than dropped, because `%w[]` parses to a wrapper holding
  nothing but its delimiters. `bare_symbol` joins `bare_string` under
  the same guard, fixing an asymmetry where `%I[a#{n}b]` billed both the
  element and the interpolated `n`. **Metric drift:** Ruby `halstead.*`
  and the derived values fall.
- Perl no longer counts a qualified name once per part *and* once as a
  whole (#1355). `package_name`, `package_variable` and `typeglob` were
  operands alongside the operand kinds they contain, and they nest: `use
  strict;` scored `N2` 2 for one name, `our $Foo::count = 3;` scored
  `n2` 5 / `N2` 6 for two, and the vocabulary carried a bare `::` entry.
  A parent-keyed guard now subsumes the contained operands.
  **Metric drift:** Perl `halstead.*` and the derived values fall.
- Tcl and iRules no longer bill a braced word twice (#1354, #1317). A
  braced *literal* (`set x {literal here}`) was a Halstead operand and so
  was every word inside it, so `set a {a b}` scored three operands
  against its `set a "a b"` synonym's one. Tcl evaluates nothing between
  braces, so the literal is now the single operand its parts belong to.
  A braced *script* — a `proc`, `if` or `when` body — was additionally
  billed as one operand spanning its entire text beside the commands the
  walk already counts, so `n2` grew with the size and uniqueness of the
  code rather than with its vocabulary; it is now billed through its
  contents alone, except when it holds no command (`lappend l {}` is an
  empty list whose brace pair is its only carrier, and a comment-only
  body scores like an empty one). **Metric drift:**
  Tcl and iRules `halstead.*`, the derived values, and hence `mi` fall
  for any file containing a braced word or block. A value-position
  braced literal still reports a `{}` operator (#1318).
- C, C++, Mozcpp and Objective-C character literals are Halstead
  operands (#1316). `char_literal` was in neither the operator nor the
  operand arm of those four getters, so a character literal contributed
  nothing at all and `char b = 'x';` billed `b` alone — while Rust,
  Java, Kotlin, C#, Go and Elixir each counted theirs. All prefixed
  spellings (`L'x'`, `u'x'`, `U'x'`, `u8'x'`) count as distinct
  operands, and a multi-character constant such as `'ab'` counts once.
  `Checker::is_string` is deliberately unchanged: a character is not a
  string, the same split Rust and Go apply. **Metric drift:** `n2` /
  `N2` rise by one per character literal, moving the derived values and
  the three maintainability-index variants.
- A plain C function written inside an Objective-C `@implementation`,
  `@interface` or `@protocol` no longer contributes to that container's
  `wmc` (#1356). Such a function is a file-static helper, not a method —
  no receiver, absent from the method table, unsendable — so `npm` never
  counted it, but the space tree nests its `Function` space inside the
  container's and `wmc` weighted every such space. The three metrics
  disagreed about one container: `npm.class_methods` 1 against
  `wmc.class_wmc_sum` 3. This is the C++ `friend` divergence #1301
  removed, surviving in a sibling language. **Metric drift:** ObjC
  `wmc.class_wmc` / `class_wmc_sum` / `interface_wmc_sum` / `total` fall
  by the cyclomatic complexity of each such helper. The helper keeps its
  own `Function` space and metrics, `nom` still counts it, and the
  file-level `cyclomatic` sum is unchanged.
- The JS-family ABC assignment count no longer depends on the previous
  statement's terminator (#1277). The counter cleared its declaration
  sentinel only on a `SEMI` token, so a `const` written without a
  semicolon (ASI) left a stale sentinel that suppressed every subsequent
  `=` until the next `;` — `const a = 1` then `x = 2` scored zero
  assignments. `const` initializers are now identified structurally from
  the `=` token's ancestor chain, matching the Kotlin fix in #455; the
  same change closes TypeScript's `x as const`, which promoted a live
  `let` slot and leaked even in fully semicolon-terminated code. A
  destructuring default under a `const` declarator (`const {a = 1} = o`)
  stays part of the declaration, as it was under the sentinel. Affects
  Javascript, Mozjs, Typescript and Tsx. **Metric drift:** an assignment
  nested inside a `const` initializer's *value* (`const x = (o.p = v)`,
  `const x = a || (b = 1)`) was blanket-suppressed by the sentinel and now
  counts, so `abc.assignments` rises in semicolon-terminated code that
  uses that shape — the change behind the pdf.js corpus refresh. Java,
  Groovy and C# now use the same structural rule for `final` / `const`
  initializers instead of a sentinel stack of their own; that stack
  stayed live from the keyword to the next `;`, so in Java and Groovy
  every `=` inside a `final` initializer — a lambda or closure body's
  `x = 1`, an array initializer's — was suppressed with the declarator's
  own (`final Runnable r = () -> { x = 1; };` scored 0). **Metric
  drift:** Java and Groovy `abc.assignments` rise by one per assignment
  nested in a `final` initializer; C# is unchanged, since a `const`
  initializer cannot contain one.
- A `?` used as type syntax is no longer counted as an ABC ternary
  condition in C#, TypeScript and TSX (#1275). The ternary `?` and the
  type-syntax `?` are the same anonymous token, so C# scored `int? x`
  and `where T : class?`, and TS/TSX scored every optional parameter,
  property, method, class field and tuple element, plus conditional
  types (`T extends U ? X : Y`), as decisions. C# safe navigation
  (`a?.b`, `a?[0]`) shares the same token and deliberately still counts,
  keeping ABC in step with C# cyclomatic — so C# gates by denylist and
  TS/TSX by allowlist, a polarity difference the arm comments record.
  Completes the type-syntax work #1274 began for Java and Groovy.
- `abc.conditions` now counts the `for` header's condition slot
  (#1276). A bare, negated or parenthesised loop condition
  (`for (; a; )`, `for (; !a; )`, `for (; (a); )`) scored zero in C,
  C++, Mozcpp, Objective-C, JavaScript, Mozjs, TypeScript, TSX, PHP and
  Perl's C-style `for (my $i = 0; $ok; $i++)` while the same predicate in
  an `if` scored one; comparison-shaped conditions (`i < n`) were
  unaffected. Go's three-clause `for init; cond; post` had the same gap,
  and its header slot was additionally mis-read for a bare `for {}` and
  shifted by a header comment. **Metric drift:** `abc.conditions` — and
  therefore `abc.magnitude` / `abc.value` — rises for any function with
  such a loop in those eleven languages. An **empty** condition (`for (;;)`,
  `for (init; ; update)`) now counts **zero** in every language: Java
  and Groovy previously scored one vacuous condition and drop by one per
  such loop. Java's and Groovy's `for` condition also no longer goes
  unread when a comment appears in the loop header.

- Objective-C message sends are boolean terminals in ABC condition
  slots. `cpp_bool_terminal_kinds!` listed `call_expression` but not
  ObjC's `message_expression`, so `if ([a ok])`, `while (![a ok])`,
  `for (; [a ok]; )`, `[a ok] ? x : y` and each operand of a `&&` /
  `||` chain scored zero conditions where the C-call twin `if (ok())`
  scored one. **Metric drift:** ObjC `abc.conditions` rises for any
  function with a message send in a decision slot.
- Groovy's elvis operator `a ?: c` is an ABC condition. Groovy
  cyclomatic already counted the token and Kotlin's ABC arm counts its
  identical one, so a method whose only branching was elvis chains
  reported cyclomatic above one with zero conditions; one condition per
  `?:`, so `abc.conditions` stays equal to `cyclomatic() - 1` on the
  chain. **Metric drift:** Groovy `abc.conditions` rises by one per
  elvis.
- Perl `qw()` lists are Halstead operands. Neither the elements, the
  wrapper nor the `qw` keyword had a classification, so `my @a = qw(a b
  c)` billed `@a` alone where `("a", "b", "c")` billed four operands.
  Each element is one operand and the empty `qw()` is one, the rule
  #1353 set for Ruby's `%w[]`; the delimiter choice cannot move the
  score. **Metric drift:** Perl `n2` / `N2` rise by one per `qw`
  element.
- iRules array references are no longer billed three times. The
  `array_index` wrapper of `$arr(k)` was an operand beside the reference
  and the index it wraps, so one reference contributed three operands
  where the Tcl twin contributed two. **Metric drift:** iRules `N2` falls
  by one per array reference, and `n2` by one per distinct index
  spelling.
- The `@` of an Objective-C string literal is no longer a Halstead
  operator. `@"…"` is one `string_literal` holding its `@` as a child,
  so the marker was billed as an operator while the operand key already
  carried it; `NSString *s = @"str";` scored an `@` in `n1` that `@42`
  and `@[…]` legitimately keep. **Metric drift:** ObjC `n1` / `N1` fall
  by one per string literal.
- `bca` no longer announces every file the reader declines as
  `skipping empty file` (#1287). Emptiness is only one of four gates: a
  1–3-byte valid source, a multi-kilobyte binary, and a UTF-16-BOM file
  were all reported as empty, which sent anyone triaging a skipped file
  in the wrong direction. The `-w` diagnostic now names the gate that
  fired — `skipping empty file`, `skipping file too small to analyze
  (3 bytes or fewer)`, `skipping file with non-UTF-8 contents`,
  `skipping UTF-16 file (unsupported encoding)` — rendered from the new
  `SkipReason` (see Added). Exit codes and which files are skipped are
  unchanged.

- `bca check --paths-from -` no longer silently defeats `[check.exclude]`
  / `--check-exclude` globs (#1306). The gate re-read `--paths-from` to
  re-anchor exclude globs against the seed list, but `-` resolves to
  stdin and the walk had already drained it, so the second read returned
  an empty list and every violation anchored against the bare `--paths`
  set — a `git diff --name-only | bca check --paths-from -` gate failed
  on files the project had exempted, with nothing on stderr. The list is
  now materialized once, before the walk, and carried to the exclude
  stage; the remediation footer still echoes the caller's original
  `--paths-from -` spelling, and the no-exclude fast path still does no
  glob-set build.

- `bca metrics -O json` / `bca ops -O json` to **stdout** now emit their
  per-file documents in the walk's file order rather than
  worker-completion order (#1303), so redirecting stdout to a file
  yields byte-identical output on every run at `--jobs > 1` — the
  sibling of #1244's `--output <FILE>` fix, on the destination pipelines
  reach for most. A reorder buffer keyed on each file's index in the
  resolved list holds out-of-order completions; skipped, unreadable, and
  unparseable files release their slot with an empty marker so the drain
  never stalls, and a post-join flush turns a missed release into a late
  document rather than a lost one. Throughput is unchanged within noise
  (measured on a 13,583-file tree); peak memory rises by the documents
  buffered while an earlier file is still being analyzed.

- A C++ `friend` defined inline no longer contributes to the enclosing
  class's `wmc` (#1301). A friend is a free function the class grants
  access to, not a member of it, so `npm` never counted it as a method
  — but the space tree nests its `Function` space inside the class
  space, and `wmc` weighted every such space it found. The three
  metrics disagreed about the same class: for `class R { friend void
  amigo() { if (1) { } } void mine() { } };`, `npm.class_methods` was
  1 while `wmc.class_wmc_sum` was 3. This is the divergence #1258
  removed for templated member bodies, surviving for `friend`. Both
  `friend_declaration > function_definition` and its templated
  `template_declaration > friend_declaration > function_definition`
  form are covered, in `LANG::Cpp` and `LANG::Mozcpp` alike; a friend
  *declared* without a body opens no space and was never affected.
  **Metric values move**: `wmc.class_wmc` / `class_wmc_sum` / `total`
  fall by the cyclomatic complexity of each inline friend, for classes
  that have one. Nothing else changes — the friend keeps its own
  `Function` space and its own metrics, `nom` still counts it (it
  counts free functions wherever they appear), and the file-level
  `cyclomatic` sum is unchanged.
- The file-level unit's `loc.sloc` and `loc.blank` now count blank lines
  above the first token (#1247). The unit anchors its reported span at
  line 1 because the unit *is* the file (#1195), but its row span was
  still measured from the root node, which tree-sitter starts at the
  first token — so `"\n\n\nfn a() {}\n"` reported `sloc 1, blank 0`
  against a reported span of `1..4`, while the same file shifted down by
  a leading comment reported `sloc 4, blank 2`, comments being in the
  tree where blank rows are not. The unit's `sloc` is now derived from
  the span it reports, so the two can no longer disagree. The anchor is
  applied once at space finalization rather than in the twenty-odd
  per-language `Loc` implementations, none of which knows the space kind.
  **Metric values move**, for files whose first token is not on line 1:
  `loc.sloc` and `loc.blank` rise by the number of leading blank rows,
  as do the `sloc_average` / `sloc_max` / `blank_average` / `blank_max`
  aggregates, and `mi` falls slightly through its `ln(sloc)` term. Files
  opening with code or a comment are unaffected, as are all nested
  spaces, which keep their measured spans.
- Whitespace-only files now report their real row count regardless of a
  trailing newline (#1087, #1247). Most grammars collapse the root node
  to a zero-width point at end-of-input, so `"  \n \n"` measured no rows
  at all and reported `sloc 0` while its unterminated twin reported
  `sloc 1` — accepted in #1087 as an upstream-owned carve-out, with the
  five grammars that behave otherwise (Elixir, Tcl, iRules, `preproc`,
  `ccomment`) pinned as the exception. Anchoring the unit's row span
  removed the premise: where the root node starts is no longer
  observable in LoC, and all twenty-five grammars now answer alike.
  **Metric values move**: a newline-terminated whitespace-only file goes
  from `sloc 0, blank 0` to its real `sloc n, blank n`.
- JavaScript, MozJS, TypeScript and TSX now count a `var` / `let` /
  `const` declaration as a logical line (#1283). Neither
  `variable_declaration` nor `lexical_declaration` had an LLOC arm, so a
  file of nothing but declarations reported `lloc 0` and every real
  JS/TS file under-reported `loc.lloc` by one per declaration statement
  — while Java's `LocalVariableDeclaration`, Rust's `let` and Python's
  assignments all counted the equivalent construct. The two JavaScript
  modules also count `using_declaration` (`using r = open();`), the
  third member of the grammar's `declaration` supertype that runs an
  initializer; the TypeScript and TSX grammars pinned here emit no such
  node. Two enclosing constructs that already count the row carve the
  declaration out, mirroring Java's for-header rule: a classic
  `for (let i = 0; …)` header — recognised as the `for_statement`'s
  `initializer` field, so a brace-less body (`for (…) var s = i;`)
  still counts as its own line — and an `export const a = 1;`
  (including TypeScript's `export declare const y: string;`, where an
  `ambient_declaration` sits between the export and the declaration).
  In TypeScript and TSX a declaration under an `ambient_declaration`
  (`declare const x: number;`, the body of a `declare namespace` or
  `declare module`) counts nothing: it has no initializer to run.
  `for (const x of …)` and `for (var k in …)` need no carve-out — the
  grammar inlines the keyword and emits no declaration node.
  **Metric values move**: `loc.lloc` rises for JS/TS/TSX/JSM input, and
  with it the `lloc_average` / `lloc_min` / `lloc_max` aggregates. No
  other metric is affected — `mi` does not consume `lloc`.

- A member access or qualified name in JavaScript, MozJS, TypeScript,
  TSX, C# and Groovy no longer counts as a Halstead operand on top of
  the identifier leaves it contains (#1263). `var r = a.b;` reported
  the operands `a`, `b` **and** `a.b`, because the `member_expression`
  wrapper was classified while the walker independently counted both
  children — so a chain like `a.b.c` billed two composites on top of
  its three leaves. The same shape covered TS/TSX `nested_identifier`
  (`namespace N.M`), C# `qualified_name` (`using System.Text;`),
  `generic_name` (`List<int>`) and `alias_qualified_name`
  (`global::Foo`), and Groovy `qualified_name` (`package com.example`).
  The convention is now the one C, C++, Java, Rust, Python, Go, Kotlin,
  Ruby, Lua and PHP already followed and is recorded in the getter
  macro's doc comment: a member access is its leaves plus the `.` /
  `::` operator, never the composite text as well. In the same change
  JavaScript's `private_property_identifier` (`#x`) becomes an operand,
  closing the gap the composite had been masking — a private field's
  declaration counted nothing at all, and `this.#x` counted only the
  composite. `meta_property` (`import.meta`, `new.target`) is the one
  composite kept, as a single operand like `this`: its `meta` /
  `target` leaves are anonymous tokens no arm classifies, so dropping
  it would have left the meta-object with no operand at all.
  **Metric values shift**: affected files report lower
  `halstead` `n2` / `N2` (−29% and −18% across the pdf.js and C#
  integration corpora) and therefore lower volume and a higher `mi`.
  Note `difficulty` and `effort` move *up*, because `n2` falls faster
  than `N2` and difficulty is `(n1/2)·(N2/n2)`. Refresh affected
  baselines. Follows #1293, which fixed the same wrapper/leaf shape in
  PHP.

- PHP's Halstead operand count no longer bills a type or qualified name
  once per wrapper node (#1293). `primitive_type`, `optional_type`,
  `named_type`, `union_type`, `intersection_type`,
  `disjunctive_normal_form_type`, `qualified_name`, `relative_name` and
  `namespace_name` all nest around the leaves they contain, and every
  level was listed as an operand — so `int` scored 2, `?int` scored 3,
  `Foo\Bar\Baz` scored 5, and `?A\B` scored 6 for two identifiers. Two
  design calls settle which node keeps the operand: a type is counted
  at its innermost concrete form (`?int` is the operand `int`, since
  `?` is already an operator), and a qualified name is counted by its
  components (`Foo\Bar\Baz` is `Foo`, `Bar`, `Baz` around two `\`
  operators — the same reading PHP's own `::` and `->` already get).
  The `primitive_type` wrapper keeps the operand and its keyword child
  is suppressed under it, because the grammar emits no child token for
  `callable`, `iterable`, `mixed`, `void`, `false` or `true` and
  dropping the wrapper would score those six types zero; the
  suppression is parent-scoped, so the `array` heading an `array(…)`
  literal still counts. **Metric values shift**: PHP files report lower
  `halstead` `N2` / `n2` — 23 → 15 and 14 → 9 on the issue's two
  reproducers — and therefore lower volume, difficulty, effort and
  bugs, and a higher `mi` (maintainability index). Refresh affected
  PHP baselines. Follows #1259, which fixed the `$variable` half of the
  same wrapper/leaf shape.

- `nexits` now counts the abrupt-exit builtins of Ruby, Perl, Tcl and
  iRules (#1270). Ruby `raise` / `exit` / `exit!`, Perl `die` / `exit`,
  Tcl `error` / `throw` / `exit` and iRules `error` leave a function
  exactly the way Python's `raise`, Go's `panic` and Lua's `error` do,
  but none of them has a dedicated grammar node, and only the latter
  three had a callee-text arm — so a Ruby guard clause that raised
  scored `1` where the byte-equivalent Python scored `3`. Each is
  matched at the same seam its siblings already used: a `call` whose
  method identifier spells the builtin and whose receiver is absent or
  the explicit `Kernel` constant (Ruby), a `call_expression_with_bareword`
  naming the builtin bare or through `CORE::` (Perl), the leading word
  of a generic command (Tcl/iRules). Calls with any other receiver
  (`obj.raise`, `$obj->die`), other package-qualified callees
  (`Carp::croak`), and the same words in argument position (`puts error`)
  are not counted. **Metric values shift**: any Ruby, Perl, Tcl or iRules
  function using these builtins reports a higher `nexits` sum, average
  and max than in 2.1.0, which can newly breach an `nexits` threshold —
  refresh affected baselines. Tcl 8.6's `throw` is deliberately absent
  from the iRules set (TMOS runs a Tcl 8.4-derived interpreter with no
  such builtin), and a bare argument-less Ruby `raise` stays uncounted
  because it parses as a plain identifier, indistinguishable from a
  variable read.

- **(breaking)** The Python bindings' `analyze_batch` / `analyze_paths` dropped a
  result slot for any file the read gate declines to parse — three
  bytes or fewer, a UTF-16 BOM, or a leading window that is not valid
  UTF-8 — even under `skip_generated=False`, which both entry points
  documented as guaranteeing one element per input. That gate is
  unconditional, so a batch containing one such file returned a shorter
  list and the `zip(inputs, results)` pattern the docstrings endorse
  silently attributed every later result to the wrong path: a
  data-corruption class defect with no error and no `AnalysisFailure` to
  observe. With `skip_generated=False` such a file now holds its
  position as a `None` element — the same value single-file `analyze`
  returns for it, and one `to_sarif` already skips — so the documented
  `zip` is finally safe (#1238). The `skip_generated=True` default is
  unchanged: a skipped file, generated or unreadable, still yields no
  element. The typed surface widens to match: `analyze_batch` and
  `analyze_paths` are now annotated
  `list[FuncSpaceDict | AnalysisFailure | None]` in `_native.pyi`, so a
  `mypy --strict` consumer indexing a slot without a `None` check is
  told to add one rather than discovering it at runtime. An **untyped**
  `skip_generated=False` consumer sees the change at runtime instead: a
  loop that indexed every slot used to run to completion on such a batch
  (silently mis-paired) and now raises `TypeError: 'NoneType' object is
  not subscriptable` at the placeholder, and `len(results)` grows.
  That is the intended direction — loud and local beats silent and
  downstream — but it is a behaviour break, and `analyze_paths` shares
  it, so a directory walk under `skip_generated=False` gains one element
  per discovered file the gate declined. Widening a return union is not
  an additive change under [STABILITY.md](./STABILITY.md); the exception
  and its reasoning are recorded there under *Python bindings →
  Typing*.
- Ruby regex literals and Perl bare match literals fabricated division
  operators: the delimiter tokens under the literal wrapper were
  classified through the generic `/` operator arm, so `x = /abc/`
  reported a division with none in the source. Both are now
  parent-guarded to `Unknown` under `regex` / `pattern_matcher`, the
  same compound-leaf shape #1256 applied to Elixir sigils. Ruby's
  `%r{…}` spellings alias onto the same token and are covered; a
  standalone `a / b` division still counts. Ruby and Perl Halstead
  operator counts drop accordingly (#1312). The remaining eight
  languages with this defect are tracked in #1314.
- Literal delimiters fabricated operators in eight more languages, the
  rest of the class #1256 and #1312 fixed. A JavaScript, TypeScript,
  TSX or MozJS regex spelled both delimiters `/`; a Groovy slashy
  string spelled its closer `/`; a C++ or MozC++ raw string spelled its
  `R"(` opener `(`; and a Tcl or iRules braced *word* spelled its `{`
  the way a script block does. Each reported an operation absent from
  the source, and each moved with the author's choice of delimiter. All
  are now parent-guarded to `Unknown` under the literal wrapper, so a
  real division, call or block still counts (#1314).
- JavaScript-family regex literals contributed **no operand** either —
  `Regex` was in neither the operator nor the operand arm, so `/abc/g`
  reached the Halstead vocabulary from neither side. It now counts as
  one operand, matching Ruby's `regex` and Elixir's sigils. `n2`/`N2`
  rise for JS-family code containing regex literals (#1314).
- Perl's five pattern wrappers all scored zero, and are now split by
  what they are: `/abc/`, `m/abc/` and `qr/abc/` are pattern *values*
  and count as one operand each, while `s///` and `tr///` (with its
  `y///` synonym) are operations applied to a target and count as
  operators, rendered in `bca ops` as `s///` and `tr///`. Perl
  `n1`/`N1`/`n2`/`N2` all move for code using patterns (#1314).
- PHP string-interpolation openers fabricated a block operator. `{` is
  both the compound-statement brace and the complex-interpolation
  opener, so `"{$x}"` inflated the same `{}` vocabulary entry a real
  block uses — the worst case of the five interpolating languages, and
  the only one where the two share a token. Guarded in all four
  positions the grammar puts it (`encapsed_string`, `heredoc_body`,
  `shell_command_expression`, and the deprecated `"${x}"` form's
  `dynamic_variable_name`); a real block still counts (#1314).
- Every `bca` subcommand man page omitted the CLI's global options
  (`-w`/`--warnings`, `--report-skipped`), and `bca-vcs-commit.1` /
  `bca-vcs-trend.1` additionally omitted the whole vcs history-tuning
  family. `xtask` rendered pages from a never-built `clap::Command`, and
  clap propagates global args into subcommands only during
  `Command::build()`. The drift gate could not catch it: the committed
  pages faithfully matched the wrong generator output. The regenerated
  pages gain the missing options and keep their existing
  `bca-metrics`-style synopsis spelling (#1248). The same pass removes
  two dangling cross-references: `man/bca.1` pointed at `bca-help(1)`
  and `man/bca-vcs.1` at `bca-vcs-help(1)`, pages the renderer listed
  in SUBCOMMANDS but deliberately never wrote. clap's auto-inserted
  `help` subcommand is now suppressed in the rendering tree, so it is
  neither listed nor referenced; `bca help <cmd>` is unaffected at
  runtime.
- C++ function-pointer data members (`int (*fp)(int);`) were counted as
  methods and skipped as attributes — both backwards. The
  `function_declarator` arm is now gated on the declarator child not
  being an indirection inside parentheses, and the attribute counter
  recurses through `function_declarator` / `parenthesized_declarator`
  so the field is reachable. Parenthesised method names
  (`void (f)();`) and `Foo* operator->();` still count as methods
  (#1300).
- C++ conversion operators declared without a body (`operator float();`
  and `template<typename T> operator T();`) were counted as neither a
  method nor an attribute — their declarator is an `operator_cast`, not
  a `function_declarator`, so both `npm` and `npa` skipped them. They
  now count as methods, in both the Cpp and Mozcpp grammars (#1298).
- In TypeScript and TSX, a `: string` type annotation counted as both a
  Halstead operator (the `predefined_type` wrapper) and an operand (its
  inner anonymous `"string"` keyword token, added by #313 for parity
  with `Checker::is_string`) — one source token, two tallies, while
  `: number` / `: boolean` counted once. The keyword now counts exactly
  once, as the text-keyed `string` operator, symmetric with every other
  predefined type; Halstead length and vocabulary drop accordingly for
  annotation-dense TS/TSX code. `Checker::is_string` was narrowed in
  the same direction, so `bca find --type string` / `bca count --type
  string` report string literals and templates only, no longer the
  `: string` annotation keyword; string *literals* whose contents spell
  `"string"` are unaffected (#1261).
- A Ruby stabby lambda (`->(z) { … }` / `->(z) do … end`) opened two
  nested anonymous Function spaces — one for the `Lambda` node and a
  phantom zero-metric one for the `Block` / `DoBlock` that is the
  lambda's own body. #465 had fixed the closure *count* half; the
  space-promotion walk still promoted the body block a second time.
  The `Block | DoBlock` promotion is now gated on the same
  parent-is-not-`Lambda` predicate `is_closure` uses, so each stabby
  lambda opens exactly one space; keyword forms (`lambda { }`,
  `proc { }`) and iterator blocks are unchanged. Serialized space
  trees for Ruby files with stabby lambdas lose the phantom entries,
  which also removes their zero contributions from `function_spaces`
  averages and per-file minimums (#1257).
- Ruby cognitive complexity charged a stabby lambda's body twice for
  lambda nesting: both the `Lambda` wrapper and its own body `Block` /
  `DoBlock` incremented the surcharge, so `f = ->(a) { if a then 1
  end }` scored 3 where the equivalent keyword form `f = lambda { |a|
  if a then 1 end }` scored 2. The body block is now excluded via the
  same shared stabby-lambda-body predicate as the closure count and
  the space tree (the sweep #1257's rationale mandated), and both
  forms score 2. Cognitive values drop by 1 per nesting-sensitive
  construct inside each stabby lambda; keyword lambdas, `proc`, and
  iterator blocks are unchanged.
- Elixir sigil delimiter tokens (`/`, `(`, `{`, `[`, `<`, `>`, `|`)
  counted as ordinary Halstead operators, so the delimiter choice
  changed `n1`/`N1` and `~r/abc/` fabricated two division operators.
  The delimiters are now suppressed when their parent is the sigil
  node (which is already the operand); `~` remains the single
  per-sigil operator, and standalone `/`, `<`, `|`, … in ordinary
  expressions — including inside a sigil's interpolation — still
  count. The ABC condition count had the same delimiter sensitivity
  through `<` / `>` (`x = ~s<hi>` scored 2 conditions, `x = ~s(hi)`
  zero) and now applies the same parent-is-sigil guard (#1256).
- Elixir's bare `_ ->` catch-all arm (`case` / `receive` / `rescue`)
  and any unguarded `true ->` directly under a `cond` (shape-based,
  whatever the arm's position — matching Rust's position-blind
  bare-`_` rule) counted toward standard cyclomatic complexity, while
  every sibling language excludes its default arm (Rust `_ =>`,
  Python `case _:`, Kotlin `else ->`, C-family `default:`, …). Both
  are now excluded; guarded wildcards (`_ when …`), named discards
  (`_x ->`), `true ->` under a `case`, and a multi-clause `fn`'s
  trailing `_ ->` (its free base path is the #776 head-clause skip,
  so its 2nd+ clauses all count, in parity with the identical `case`)
  still count (#1272).
- A Tcl `try { … } on error { … }` construct contributed zero to
  cyclomatic and cognitive complexity. Each `on` / `trap` handler now
  counts +1 in both metrics (`finally` stays free), matching `catch`
  and every exception-bearing sibling language, and the iRules `Try`
  sibling counts the same way — its `on_handler` / `trap_handler`
  nodes previously opened spurious anonymous function spaces,
  inflating `nom`, instead of counting as decisions (#1266).
- A Tcl `for` loop contributed zero to cyclomatic and cognitive
  complexity because the grammar has no `for` rule — the loop parses
  as a generic `command` node. It is now recognised by command name
  at the same out-of-band slot as `switch` (#467) and counts like
  `while` / `foreach`, nesting increments included (#1264).
- Every variable a Tcl script assigned was absent from Halstead
  operands: the parser emits the anonymous `id` kind in both of its
  positions, and the getter excluded it wholesale on the false
  premise that it only appears inside `$var` substitutions (whose
  wrapper is already the operand). The exclusion is now scoped to
  the substitution-leaf position, so `set` targets count in
  `n2`/`N2`, matching the iRules parent guard (#1294).
- The `enums` code generator minted collision-breaking names
  (`Foo` → `Foo2`) without registering them, so a minted suffix could
  silently duplicate a node kind whose own sanitized name is literally
  `base` + digits. `tree-sitter-php` is one aliased rule away from
  exactly that — it already emits `cast_type_token1` through
  `cast_type_token12` — and the generator would have exited `0`, leaving
  the duplicate to surface as a rustc duplicate-variant error in the
  parent crate during a grammar bump, where it reads as upstream
  breakage. Minted names are now probed against, and registered in, the
  taken-name set. No shipped artifact changes: no current grammar
  reaches the collision, so every generated file is byte-identical
  (#1237).
- `bca check --baseline` rendered a malformed, double-signed
  `[regr +-29%]` tag for every `mi.*` regression. `Baseline::classify`
  has been direction-aware since #827 — for the lower-is-worse `mi.*`
  family a *drop* below the recorded value is the regression — but the
  tag formatter still computed `(value - recorded) / recorded` and
  prefixed a literal `+`, so the percentage carried its own minus sign.
  Anything grepping the documented `[regr +N%]` shape mis-parsed it. The
  magnitude is now measured in the metric's own direction, keeping one
  tag shape across every metric; the higher-is-worse rendering is
  unchanged (#1242).
- The VCS bot-author filter was case-sensitive despite
  `DEFAULT_BOT_PATTERN`'s doc promising the opposite since the option
  shipped. `BotFilter::new` compiled with a plain `Regex`, and
  `push_if_human` matches the raw signature bytes before any
  lowercasing, so `Dependabot[bot]` was counted as a human author while
  its lowercase twin was excluded — diluting ownership and raising file
  risk on the same repository depending only on how a bot capitalised
  itself. A user pattern behaves as written now too:
  `--bot-pattern renovate` matches `Renovate Bot`. Prefix `(?-i)` to
  restore exact matching. `CACHE_SCHEMA_VERSION` moves 1 → 2, since the
  cache fingerprint hashes only the pattern string and an event log
  walked under the old matcher would otherwise replay with its excluded
  set intact; the bump costs one cold walk (#1265).
- The VCS window-boundary computation was non-saturating in two of its
  six sites — `build_cached`, the *default* path for `bca vcs`, and
  `BlameWalk`'s constructor. Both operands are attacker-adjacent: the
  reference time is an `--as-of` value or a committer timestamp read out
  of an object header, and the window length is user-supplied up to
  roughly `i64::MAX` through `--long-window`. A plain `-` therefore
  panicked in a debug build and, in release, wrapped to a boundary in
  the far *future*, silently reversing the pure-hit check, the candidate
  filter, `prune_and_dedup`, and the `walk_long_boundary` persisted for
  future runs. All six sites now route through one
  `vcs::options::window_boundary` helper that documents and pins the
  saturation invariant once. `days_between` carried the same hazard
  under a `.max(0)` clamp that a wrapped (positive) delta defeats, and
  saturates too (#1271).
- A Bash arithmetic ternary — the only ternary form Bash has —
  contributed nothing to cyclomatic or cognitive complexity, so
  `local m=$(( a > b ? a : b ))` reported cyclomatic 1 (base only) and
  cognitive 0, where the same construct scores 2 / 1 in every sibling
  that has a ternary. It is now a decision point in both cyclomatic
  tiers and a nesting construct for cognitive, matching the C-family
  `ConditionalExpression`. Both arithmetic contexts are covered — the
  `$(( … ))` expansion and the bare `(( … ))` statement. Bash
  `cyclomatic`, `cognitive` and the `mi` family derived from them shift
  for files using arithmetic ternaries (#1268).
- Go ABC scored zero assignments for a `var` declaration with an
  initializer. `var x = 5` and `x := 5` are the same binding spelled two
  ways, but only the latter is a `short_var_declaration`; the former is a
  `var_spec`, which no arm matched. An initialized `var_spec` — typed or
  not, standalone or inside a grouped `var ( … )` block — is now one
  assignment, matching what Rust, Java, C, C++, C#, JS, Lua and PHP
  already count for the same construct (each measured). `const` stays
  excluded, as does an uninitialized `var z int`. Go `abc` values rise
  by one per initialized `var` spec (#1278).
- Constructor delegation scored zero ABC branches in Java, C# and
  Kotlin. `super(…)` / `this(…)` parses as `explicit_constructor_invocation`
  in Java, `: base(…)` / `: this(…)` as `constructor_initializer` in C#,
  and `: super(…)` as `constructor_delegation_call` in Kotlin — none of
  which is the call kind each language's branch counter matched, so a
  delegation contributed nothing where Groovy scored one for identical
  source. Each is now one branch, per Fitzpatrick's "one per function
  call". Calls in a delegation's argument list are separate nodes and
  still count on their own, so `super(f())` is two branches, not three.
  Java, C# and Kotlin `abc` values rise by one per delegating
  constructor (#1279).
- Ruby ABC counted the `<` of a superclass clause as a comparison
  condition, so every subclass declaration scored a phantom condition —
  `class Foo < Bar` with a single assignment inside reported
  `conditions = 1` for a file with no conditional in it. `LT` / `GT` are
  now counted only under a `binary` parent, the positive polarity Rust,
  Go, C, C++, Java, Groovy, C#, Kotlin and the JS family already use.
  The same gate stops counting the `<` that names an operator method
  (`def <(other)`). Real comparisons, `<=>`, the `<<` shovel and heredoc
  openers are unaffected. Ruby `abc` values drop by one per subclass
  declaration and per operator-method definition (#1280). The gate now
  covers the sibling comparison and equality tokens too — `==`, `!=`,
  `===`, `<=`, `>=`, `<=>`, `=~`, `!~` are each equally definable as an
  operator method, so `def ==(other)` scored the same phantom condition
  `def <(other)` did.
- Bash ABC counted every I/O redirection as a condition. `>` and `<`
  spell a redirect as well as a comparison, and the grammar parents the
  redirect under `file_redirect`, so `echo hi > out.txt` reported
  `conditions = 1` for a line with no test in it — the Bash instance of
  the #1280 polarity, missed by that issue's cross-language sweep. Both
  tokens now require a `binary_expression` parent; comparisons inside
  `[[ … ]]` and `(( … ))` are unaffected. Bash `abc` values drop by one
  per redirection (#1280).
- Bash ABC scored no condition for the arithmetic ternary, so
  `local m=$(( a > b ? a : b ))` reported 1 where the equivalent C
  `int m = a > b ? a : b;` reports 2. #1268 brought Bash's only ternary
  form into cyclomatic and cognitive; ABC now counts it too, matching
  the C-family `ConditionalExpression`. Bash `abc` values rise by one
  per arithmetic ternary (#1268).
- `fix_includes` returned its diagnostics in a different order on every
  run for identical input, because both producers push while iterating a
  `HashMap` — `files` in `build_include_graph`, `nodes` in
  `record_indirect_includes`. `bca preproc` prints that `Vec` straight to
  stderr, so the same source tree emitted the same warnings in a
  different order each time. Measured at 40 distinct orders across 40
  runs of one 8-file input, against a single distinct *set* — the
  content was already deterministic, only the sequence was not. The
  sequence is now sorted, which needed `Ord` on the public
  `PreprocDiagnostic` (additive; see `STABILITY.md`). The inner half of
  this had already been fixed: `IncludeCycle` sorts its own member list
  "so the emitted diagnostic is deterministic across runs", and only the
  outer sequence was left. Found while establishing whether
  `fix_includes` was safe to fuzz (#1288).
- The `LANG::C` arm of the preprocessor macro-replacement pass had no
  test that could observe it. `parse_then_metrics_c_with_preproc_…` was
  added by #721 to give that arm "direct coverage", but its only
  assertion is a `cyclomatic_sum`, and `DBG ? x : 0` and `$$$ ? x : 0`
  have the same complexity — so it passed unchanged with the whole
  `LANG::C | LANG::Cpp | LANG::Mozcpp` arm of `get_fake_code` disabled,
  which is how this was measured. It now asserts the rewritten bytes.
  The `Cpp` sibling has the same blind spot but is separately covered by
  `cpp_ast_source_reflects_preproc_expansion`; the `C` arm had nothing.
  Found while verifying that the new `preproc_macro` fuzz target is not
  vacuous (#1154).

- **Halstead, PHP:** variable references no longer count twice. `$x` parses
  as a `variable_name` wrapping a `name` leaf and both were classified as
  operands, so `N2` roughly doubled on variable-dense PHP and `n2` carried a
  sigil-less twin per variable; variable-variable syntax compounded it (`$$a`
  scored 3, `$$$b` scored 4). `Name` is now suppressed under a
  `variable_name` or `dynamic_variable_name` parent, and those wrappers under
  a `dynamic_variable_name` parent, so every reference counts once at any
  nesting depth. Volume, difficulty, effort and MI change for every PHP file
  (#1259).

- **Halstead, Elixir and C#:** `true` / `false` / `nil` / `null` literals no
  longer count twice in `N2`. The grammar wraps the keyword leaf in a named
  literal node and `get_op_type` classified both as operands; because
  operands are keyed by source text, `n2` hid the duplication while `N2` —
  and the length, volume, difficulty and effort derived from it — inflated by
  one per literal occurrence. Elixir drops the leaves outright; C# parent-
  guards them, because `operator true` emits a bare leaf with no
  `boolean_literal` wrapper (#1253).

- **ABC, Java and Groovy:** generic type syntax is no longer counted as
  conditions. A generic declaration (`class Gen<T>`, `<T> void m()`, Groovy's
  `def <U> U m(U x)`) scored two conditions per bracket pair, and a wildcard
  bound (`List<? extends T>`) scored one as a phantom ternary. Both tokens
  are now gated on their parent node kind, matching the polarity C, C++,
  Objective-C, mozcpp, Rust and Go already use (#1274).

- **npm, C++ and Mozcpp:** a templated member function with an inline body
  was not counted. The `template_declaration` guard resolves through a shared
  helper that hunted a `function_declarator`, but a templated member *with a
  body* parses as `template_declaration > function_definition` and has no
  such node at that level — so the method scored zero while `nom` opened a
  function space for it and `wmc` weighted its cyclomatic, leaving three
  metrics disagreeing about one class. The helper now accepts a
  `function_definition` child outright rather than recursing into it, which
  also fixes a templated conversion operator (whose declarator is an
  `operator_cast`, so no `function_declarator` exists at any depth). C++
  `npm` values rise on classes with templated inline-bodied members (#1258).

- `bca count -t call` and `bca find call` reported 0 on all C, C++ and Mozcpp
  source. `Checker::is_call` matched only the unsuffixed `CallExpression`
  kind_id, which is the grammar's always-aliased `preproc_call_expression`
  and never reaches `kind_id()`; every real call carries the aliased variant.
  No metric values are affected (#1254).

- **web:** a misused `author_hash_key` on `POST /v1/vcs` and `/v1/vcs/trend`
  reported `error_kind: "vcs_internal_error"`, the token reserved for backend
  faults, so a client branching on the token saw its own mistake as a server
  failure. It now carries `vcs_invalid_author_hash_key`. The status was
  already correct (`400`) (#1245).

- `bca metrics --output <FILE>` and `bca ops --output <FILE>` wrote the
  aggregate document's per-file elements in worker-completion order, so two
  runs over an unchanged tree produced differently-ordered files at
  `--jobs > 1`. The elements are now sorted by emitted path in every format
  (JSON, YAML, TOML, CBOR, CSV), making the artifact diffable and usable as a
  cache key. Multi-seed runs change too: the walk sorts per seed and
  concatenates, so `bca metrics -p b -p a --output x.json` previously emitted
  `b`'s files first and now emits one globally sorted document (#1244).

- `bca check`'s remediation block printed a baseline-refresh command in the
  pre-#597 flag order (`bca --paths … check …`), which exits 1 with a clap
  usage error. The command now names the `check` subcommand first and mirrors
  every flag that decides what `--write-baseline` records — including
  `--threshold`, `--no-config`, `--check-exclude` and `--check-exclude-from`,
  without which the suggested refresh wrote a different baseline than the
  gate measured, or failed outright (#1243).

- The man-page drift gate now fails on a *newly generated* page.
  `git diff --exit-code -- man/` reports tracked content only, so the page
  `cargo xtask` writes for a brand-new subcommand passed both the CI
  `manpage` job and `make manpages-check` green and silently never shipped.
  The check moved out of the two hand-mirrored shell blocks into
  `utils/check-manpage-drift.py`, which both sites now call, and covers
  modified, deleted, and added pages (#1249).

- `cargo xtask` deleted the man page it had just written when a command was
  renamed case-only, on macOS/APFS and Windows/NTFS, while still exiting 0.
  `render_man_page`'s collision guard folded ASCII case; `sweep_orphans`
  compared byte-for-byte. Because case-insensitive filesystems are also
  case-preserving, the write for `BCA.1` landed in the existing `bca.1`
  directory entry and the case-sensitive sweep then unlinked it as an orphan.
  The relation now has a single definition that both sites call, and the
  sweep classifies three ways rather than two: byte-equal keeps, no match
  removes, and a case-only match is refused with an error naming every
  conflicting spelling (#1250).

- The pre-tag `cargo publish --dry-run` for `big-code-analysis` was skipped
  on every release, not only the first. It was gated on a crates.io sparse
  index probe for a leaf version that the Lockstep policy guarantees is the
  version being released — and therefore never published yet — so the branch
  that runs the dry-run was unreachable, while `release.yml` and
  `RELEASING.md` both told a maintainer it became a hard gate from the second
  tag onwards. `big-code-analysis-cli` and `big-code-analysis-web` had no
  such check at all: both pin `big-code-analysis = "=<version>"`, so neither
  could have been dry-run either. Replaced with `make check-publish-metadata`
  (`utils/check-publish-metadata.py`), a registry-independent gate over every
  publishable crate's crates.io-required metadata, `[package].include`
  whitelist, and packaged size, wired into `release-check`, `lint`,
  `pre-commit`, `ci`, and the release workflow's preflight. It reads resolved
  fields from `cargo metadata` so `[workspace.package]` inheritance is
  honoured, and resolves `include.workspace = true` separately because
  `cargo metadata` does not emit `include`. Its `cargo package --list` runs
  `--locked`, carrying over the flag from the `cargo publish --dry-run
  --locked` it replaced: without it a stale `Cargo.lock` is silently
  re-resolved and rewritten and the gate still reports a pass, so a tag
  could be cut without the committed lockfile ever being verified (#1224).
- `make release-check` now probes `cargo-deny` and `cargo-about` before
  running them, naming the missing tool and its exact install command instead
  of surfacing cargo's generic `no such command` partway through the gate.
  Both are listed by `make check-tools` and documented in `RELEASING.md`. The
  `cargo-about` hint spells `--features cli`: the binary sits behind a
  non-default feature, so a bare `cargo install cargo-about` compiles the
  library, installs no binary, reports the miss as a *warning*, and exits 0
  (#1226).
- `make release-check` rejects uncommitted changes in the vendored grammar
  leaves before its slow stages, with the commit → gate → tag ordering as the
  remedy rather than cargo's misleading `--allow-dirty` hint — passing that
  flag would dry-run content differing from the tag, defeating the gate. The
  check is scoped to what `cargo publish --dry-run` actually rejects (tracked
  changes inside the five leaf directories), measured rather than assumed, so
  it cannot fail on trees cargo would accept. `RELEASING.md`'s pre-release
  checklist now carries the gate and that ordering explicitly, and notes the
  push is separable (#1225).
- The CLI and web crates no longer terminate on a library parse error.
  All fifteen `.expect(FEATURES_PINNED)` call sites — eight in `bca`'s
  dispatch helpers, seven in `bca-web`'s handlers, plus the constant
  itself in each crate — now propagate onto the error channel each
  caller already had: an `io::Error` of kind `InvalidData` that the
  concurrent runner reports per file and continues past, and the
  existing sanitized `500` that logs the cause server-side. The pinned
  `all-languages` feature does make `MetricsError::LanguageDisabled`
  unreachable, but `MetricsError` is `#[non_exhaustive]` and documents
  that variants may be added in a *minor* release, so the `expect` was a
  panic scheduled against a routine dependency bump rather than an
  invariant. No behaviour changes today: the only reachable outcome is
  still success (#1152).

### Changed

- Groovy's qualified-type operand classification is now pinned by a
  regression test (#1352). #1263 removed both `QualifiedName` and
  `QualifiedType` from `GroovyCode::get_op_type`'s operand arm, but only
  the `QualifiedName` half was covered; re-adding the emitted alias
  `QualifiedType2` failed no test, leaving the "complete the alias list"
  reading — which is the double count #1263 removed — unobstructed. No
  metric values change.

- `gix` 0.86 → 0.87.1, with the rest of the gitoxide family advancing
  in step (36 lockfile entries). Forced rather than routine: every
  published version of `bisync`, a transitive dependency of
  `gix-protocol` 0.64, was yanked on 2026-08-24, so `cargo deny`'s
  `advisories` check failed on any lockfile still holding it and no
  `cargo update -p bisync` could satisfy the `^0.3.0` requirement.
  `gix-protocol` 0.65 dropped the crate. One source change: `gix-date`
  now takes its "now" reference as a `jiff::Zoned`, so `--as-of`
  parsing passes `gix::date::Zoned::now()` instead of `SystemTime`.
  No behaviour change and no public-API change — no `gix` type appears
  in a public signature. The six excluded crates lock no `gix` and are
  untouched.

- The pdf.js corpus test now covers all 384 files: the 118-entry
  exclude list frozen in the mozjs-default era (#84) was retired
  (#1282), since every entry parses without ERROR nodes under the
  post-#507 upstream `tree-sitter-javascript` grammar. The issue's own
  probe had reported 3 residual failures, but those were `rg ERROR`
  matching source text (`MAX_ERROR`, a regex literal) rather than
  parse-error nodes. The 118 orphaned mozjs-era snapshots were
  refreshed to current metric output, and the 5 DeepSpeech orphan
  snapshots (files still excluded under #86) were deleted, so on-disk
  snapshot counts match the asserted counts for every corpus. The
  corpus harness now asserts that inverse direction — a `.snap` with no
  corresponding corpus file fails the test naming the orphan — so a
  future exclude or corpus change cannot strand snapshots silently.
- The `tree-sitter` runtime is `=0.26.12`, up one upstream patch
  release, pinned in lockstep across the root manifest, `enums`, and
  the five vendored `bca-tree-sitter-*` crates. `tree_sitter` is
  re-exported from the library root, so the resolved version is visible
  to consumers. Nothing in the release reaches this workspace's
  behaviour: `include/tree_sitter/api.h` and the Rust bindings are
  byte-identical to `0.26.11`, so `TREE_SITTER_LANGUAGE_VERSION` stays
  at 15 and the vendored `parser.c` sources are unaffected, as is the
  `Send + Sync` argument the PyO3 bindings rest on. The three C fixes
  are an error-recovery restart when the parser is already in
  `ERROR_STATE`, a `has_later_named_siblings` correction in the tree
  cursor, and query-anchor semantics for skipped quantifiers — the
  last inert here, since this workspace uses no tree-sitter query API.
  No metric value moves.

- Ruby's and Elixir's `#{` interpolation opener no longer counts as a
  Halstead operator. Unlike PHP's `{`, which aliases the
  compound-statement brace and was fabricating a block (see Fixed),
  `#{` is a token of its own and nothing was miscounted — this is a
  deliberate change of rule, so that the six interpolating languages
  agree. Kotlin, C# and Groovy already declined to count theirs; an
  interpolation opener is spelling rather than an operation, and the
  interpolated expression's own operators are counted either way. Ruby
  and Elixir spell the marker with the same token, so leaving either
  counted would have made the two disagree on one construct. Halstead
  operator counts drop for every literal that interpolates: Ruby
  strings, symbols, regexes, heredocs and subshells, and Elixir
  strings, charlists and sigils (#1314).
- The `enums` generator's `sanitize_string` / `get_token_names` lost their
  `escape: bool` parameter. No production caller passed `true`: #862
  established that the JSON generator, the last one, was double-escaping
  by mistake. Removing the flag deletes the dead double-backslash branch,
  its test, and the `JSON_TOKEN_ESCAPE` constant that existed only to
  document why the answer must be `false` — making the #862 bug
  unrepresentable rather than one flipped boolean away. Internal build
  tool only; generated output is byte-identical (#1241).
- The workspace-excluded `enums` crate declares the workspace lint posture
  (`clippy::pedantic`, `missing_docs`) in its own manifest.
  `[workspace.lints]` reaches members only, so `make enums-check` had been
  gating the crate at `-D warnings` against the compiler defaults while
  reading as a full lint gate. Clearing the table cost 38 findings, not the
  23 pedantic ones alone — `missing_docs` accounted for the other 15 — and
  none was silenced with a blanket `allow`.
  `utils/check-excluded-manifests.py` gained a third invariant so this cannot
  recur: every workspace-excluded crate must declare its own `[lints]` table
  or be named in the gate's exempt set. It is an exempt list rather than a
  required list, so the *next* excluded crate has to make the decision
  explicitly instead of inheriting the silence. The five vendored
  `tree-sitter-*` grammar crates are exempt — their Rust is generated binding
  boilerplate a regeneration replaces wholesale (#1228).
- `make py-fmt`, `py-fmt-check` and `py-lint` resolve
  `big-code-analysis-py/.venv/bin/ruff` before PATH, the way `py-typecheck`
  already resolved mypy and pyright, so the local gate runs the
  `uv.lock`-resolved ruff rather than whichever unpinned copy `mise.toml`,
  the `Dockerfile`, or a bare `pipx install ruff` left on PATH. Those three
  provisioning paths stay unpinned deliberately: an exact version in any of
  them would be a fifth ungated copy to keep in lockstep, which is the
  failure this change exists to prevent (#1230).
- `clippy::arithmetic_side_effects` is enforced on the `loc` metric
  module, and the span arithmetic there is now explicitly saturating.
  `Loc` is the one metric computing on tree-sitter row coordinates, and
  #1051 was a `usize` underflow of exactly that shape — a Rust doc
  comment at EOF drove `end - 1` below zero from an input as small as
  `/// x`, panicking in debug and wrapping to `usize::MAX` in release.
  Validated by replaying the lint against the pre-#1051 tree, where it
  flags both reported panic sites. Metric values are unchanged: a
  saturating operation is identical to the plain one unless it would
  have overflowed, and none does (#1152).
- `clippy::indexing_slicing` is enforced on `src/c_macro.rs`, the C/C++
  macro-masking byte lexer, with nine per-function carve-outs each
  naming the bound that makes its indexing safe. Validated by replaying
  it against the pre-#126 tree, where it flags the `&DOLLARS[..]` slices
  that panicked on macro identifiers longer than 2048 bytes. The one
  slice whose bound is established in a *different* function —
  `step_raw_string`'s delimiter comparison, carried through
  `LexState::RawString` — is hardened with `get` rather than allowed
  (#1152).
- `clippy::unwrap_used` is enforced on production code across every
  crate, as `#![cfg_attr(not(test), warn(...))]` at each of the eight
  lib/bin roots rather than a `[workspace.lints]` entry: a Cargo lint
  applies to every target of its package, and the ban is a production
  rule — this workspace has **0** production `unwrap()` calls against
  1,023 legitimate ones in test targets. `cfg(test)` is set for
  integration-test crates as well as the unit-test target, so the gate
  needs no per-file carve-out and carries zero `#[allow]`s. Adopting it
  costs nothing today and fails CI on the first production `unwrap()`
  added. `clippy::expect_used` is deliberately **not** enabled — all 37
  production `expect` sites already name their invariant in the message,
  the form `AGENTS.md` sanctions. The workspace-excluded `enums` codegen
  crate carries the same gate: it is CI-linted by `make enums-check` but
  invisible to `cargo clippy --workspace`, so its 7 production
  `unwrap()` calls were outside the original count. They now propagate
  onto the `io::Result` each generator already returned, except the Go
  generator's `max()` width, which becomes `unwrap_or(0)`. None was a
  reachable crash: every one rests on an invariant as solid as the 37
  `expect` sites left alone. The difference is that an `unwrap()` states
  no invariant, which is the whole basis for gating it (#1227).
- The Python bindings' ruff config states its rule set absolutely
  (`select`) instead of relative to ruff's defaults (`extend-select`),
  and the dev-extra bound moves to `ruff>=0.13,<0.17` with `uv.lock`
  and the `requirements/` exports resolving 0.16.2. ruff 0.16.0 grew its
  default rule set from 59 rules to 413, which under `extend-select`
  took this config from 265 enabled rules to 501 and pulled in 28
  families it never selected — `PLC` / `PLE` / `PLR` / `PLW` among
  them, silently overriding the deliberate omission of Pylint's design
  rules that the config comment states. The same release *dropped* 18
  opinionated `E` / `F` rules from the defaults, which a
  defaults-relative config would have lost just as quietly. Under
  `select` the count is 265 on 0.15.22 (unchanged, so nothing was lost
  by dropping the implicit defaults) and 268 on 0.16.2, the three
  additions being new `RUF` rules in an already-selected family.
  `ruff check` passes on 0.15.22 and 0.16.2 with no `noqa`, no
  suppressions, and no source changes; the three diagnostics 0.16
  reported (`DTZ001`, `BLE001`, `PYI044`) were all in families this
  config never asked for. The `ruff-pre-commit` `rev:` in
  `.pre-commit-config.yaml` moves to `v0.16.2` to match what `uv.lock`
  resolves (it had drifted to `v0.15.14` against a locked 0.15.22).
  The floor moves off `0.6` in the same breath because it was already
  fiction: `UP038` left ruff's stable set in 0.13, and every release
  from 0.6.0 through 0.12.12 flags the same five
  `isinstance(x, (A, B))` sites here — under the old `extend-select`
  config identically, so this states what was always true rather than
  changing anything (#1222).

### Security

- Cleared the OpenSSF Scorecard Vulnerabilities alert for
  RUSTSEC-2026-0258 (`h2` queues empty HTTP/2 DATA frames without
  limit). The flagged crate was `h2 0.3.27`, reached only through
  `actix-web`'s default `http2` feature, and no patched 0.3 release
  exists: `actix-http` 3 is pinned to `h2 0.3` and the fix ships only
  in `h2 0.4.16`. `big-code-analysis-web` now builds `actix-web`
  without `http2` — the daemon binds plaintext, and actix negotiates
  HTTP/2 only over TLS ALPN, so the feature was never reachable — which
  drops `h2 0.3` from `Cargo.lock` altogether.

## [2.1.0] - 2026-08-06

A feature and correctness release on the `2.x` line, and the first to
carry a deliberate exception to the stability contract. `FuncSpace`,
`Ops`, `AstNode`, `wire::FuncSpace` and `wire::Ops` gained an explicit
`Drop` impl (#1056): a source-level break (`E0509` on a by-value field
move, fixed at each call site with a `.clone()` or a borrow) landed
under a minor because the compiler-generated drop glue recursed once
per nesting level and aborted the process on a deep tree — reachable
remotely through `bca-web`'s 4 MiB body cap. Its **(breaking)** entry
under *Changed* below carries the full rationale. Everything else in
this release is additive or a fix.

### Added

- `bca check --print-effective-config` reports which exclude globs are
  manifest-anchored (#1194). After #1164 a glob's meaning depends on its
  origin — a `--check-exclude` pattern resolves against the caller's
  working directory, a `bca.toml` one against the manifest's directory —
  and a single flattened array cannot express that. `manifest_exclude`,
  `manifest_check_exclude`, `manifest_exclude_from` and
  `manifest_check_exclude_from` name the manifest-origin subset
  alongside the resolved lists, which stay where they were so the TOML
  form keeps round-tripping through `--config`. The anchor is the
  reported `manifest` file's directory. Each key is omitted when the
  manifest contributed nothing; the `*_from` pair is present only when
  the manifest's file is the one actually in effect, since a CLI
  `--exclude-from` *replaces* rather than unions with it.
- Kotlin property accessors (`get()` / `set()`) and `init { … }`, Java
  and Groovy `static { … }`, and JavaScript class static blocks now open
  a function space of their own (#1184). Each carries executable code but
  was referenced nowhere outside the generated language enum, so its
  control flow was charged to the enclosing class and `bca check` could
  never flag one however complex it got. They are reported under
  synthesised names — `<get>`, `<set>`, `<init>`, `<static-init>` —
  following the existing `<anonymous>` convention. **They are
  deliberately absent from `nom.functions`, `nargs` and `bca functions`**:
  none is a callable named at a call site, and counting an accessor as a
  method would make `npm` bill the same property once as an attribute and
  again as a method. See the NOM section of the metrics guide.

- `bca check --explain-threshold <metric>=<limit>`: preview what a
  candidate threshold would cost at **both** tiers without editing
  `bca.toml` or running a gate (#1169). Reports hard-tier offenders, the
  resolved soft limit and its offenders, how many of each already match
  a `--baseline` entry — so the new-entry count a reviewer actually
  weighs is on screen — and names a cluster when the candidate lands on
  top of an existing population. Repeatable, one candidate per metric;
  honours `exclude_tests`, `[check] exclude`, suppression markers,
  `[thresholds.lang.<slug>]` overrides and the baseline exactly as the
  run it predicts, and always exits 0 on success (1 on a tool error,
  such as a candidate naming a metric this build does not gate). This
  closes the gap that
  `--threshold` limits are absolute and never scaled, which made the
  one-command way to trial a candidate limit the one way that could not
  show its soft-tier cost.
- `make rustfmt-bail` (`utils/check-rustfmt-bail.py` plus
  `.rustfmt-bail-baseline.txt`), a gate that blocks new match arms
  rustfmt silently refuses to format (#1136). A comment inside a match
  *pattern* makes rustfmt emit the enclosing match verbatim while
  `cargo fmt --check` still exits 0, so those matches sat outside the
  formatting gate entirely — 36 of them, concentrated in exactly the
  per-language modules where this project's most common change shape, a
  bulk edit mirrored across sibling modules, lands. The gate mirrors the
  snapshot-anchor pattern: per-file counts, fail on increase, silent on
  decrease, `--update` to ratchet, wired into `make lint` and therefore
  `make pre-commit` / `make ci`. It reports the two distinct causes it
  cannot tell apart — a hoistable in-pattern comment, and a
  `macro_rules!` body rustfmt cannot parse, which is permanent — so the
  baseline does not send the next reader hunting for a comment that
  does not exist.
- `make worktree-setup`: one idempotent bootstrap for a fresh clone or
  `git worktree` (#1171). Checks out the integration corpora under
  `tests/repositories/` and the Python-bindings venv, classifying each
  submodule first so it is a ~100 ms no-op once the tree is set up. It
  escalates to `git submodule update --force` for the interrupted-
  checkout state that a plain re-run cannot repair — a plain re-run is a
  silent no-op there, because the recorded SHA already matches HEAD —
  and refuses to force a submodule that also carries local
  modifications.
- Per-language threshold overrides in `bca.toml` (#1141). A
  `[thresholds.lang.<slug>]` table layers over the global `[thresholds]`
  per metric, keyed by the same language slugs `--language` accepts, so
  a polyglot repository can apply the per-language recommendations
  #1140 published instead of picking one number and baselining the
  difference. An unknown slug is a hard error with a did-you-mean hint;
  a file whose language is not detected falls through to the global
  table. The soft tier is derived from each language's *resolved* hard
  limit rather than the global one — otherwise a loosened language's
  soft threshold would sit below its hard threshold and exit code 5
  would become silently reachable for every function between them — and
  a soft limit looser than its hard limit is now rejected outright.
  `--print-effective-config` renders one fully resolved table per
  overridden language. Additive manifest surface; not a `STABILITY.md`
  event.

- `big_code_analysis::vcs::BlameSession`, a per-thread handle obtained
  from the new `PerFunctionBlame::session` (#1117). It carries the
  thread-local repository handle, its object cache, the parsed
  `.mailmap`, and the resolved-commit memo, so a caller blaming many
  files in one repository pays each once instead of once per file;
  `PerFunctionBlame::per_function` keeps its one-shot semantics by
  building and discarding a session. Additive — see `STABILITY.md`.

- `ConcurrentRunner::without_path_verification`, which skips the
  per-path `is_file()` check during dispatch (#1114). `FilesData::paths`
  is documented as a terminal file list, so the check is a safety net for
  a caller that hands in something else; a caller whose own traversal
  already read each entry's kind — the `bca` CLI walk — was paying one
  redundant `stat` per file. Additive: the default is unchanged.

- Benchmark harness for the metric walk, in the new workspace member
  `big-code-analysis-bench` (#1068). `cargo bench -p
  big-code-analysis-bench --bench scaling` (or `make bench-scaling`)
  measures eighteen probes at three doubling nesting depths and fits
  `time ~ depth^k`, failing when a probe's exponent leaves its declared
  complexity class; `--bench metric_walk` (`make bench-walk`) runs
  criterion benchmarks per metric over a deterministic, self-reporting
  slice of the corpus submodules. The wall-clock assertions in
  `cognitive_deep_nesting_is_tractable` and
  `tokens_deep_nesting_is_tractable` moved into the gate and the
  `BCA_ASSERT_SCALING` escape hatch is gone; both tests keep their
  value assertions and are renamed
  `cognitive_nesting_is_inherited_at_depth` and
  `tokens_count_holds_at_depth`. The harness is out-of-band by design —
  `.github/workflows/benchmark.yml` runs it quarterly, not per-PR.
  Documented in
  [`docs/development/benchmarking.md`](docs/development/benchmarking.md).
  The first run recorded three walks as quadratic in nesting depth
  (`Checker::is_else_if`, `Node::count_specific_ancestors` from `loc`,
  and `elixir_is_inside_quote_block` from `nom`), all through
  `Node::parent`, which `tree_sitter` resolves by descending from the
  root; all three are fixed in this release (#1084) and their probes
  now sit at the harness's linear bound.
- Japanese localization of the documentation. The mdBook is now
  translated through the gettext workflow from `mdbook-i18n-helpers`
  (`big-code-analysis-book/po/ja.po`; untranslated or stale entries
  fall back to English) and deployed at
  [`/ja/`](https://dekobon.github.io/big-code-analysis/ja/) alongside
  the English site, with `README.ja.md` as a hand-maintained sibling
  of `README.md`. Fragment-linked headings carry explicit `{#anchor}`
  ids so intra-book links survive heading translation. New Makefile
  targets `book-pot`, `book-po-update`, and `book-ja` drive the
  refresh workflow, documented in
  [`docs/development/translations.md`](docs/development/translations.md).
- Per-PR coverage for the release/wheel smoke harnesses, closing the
  drift gap that let three stale assertions block the `v2.0.0` cut
  (#995). The integer-valued-metric JSON serialization (#530) is now
  pinned by a CLI integration test
  (`cli_metrics_json_serializes_integer_metrics_as_integers`) that
  asserts `cyclomatic.sum` serializes as a JSON integer (`is_u64()`),
  which the existing `as_f64()`-coercing round-trip tests could not
  catch. The previously-inline library and CLI wheel smokes were
  extracted into checked-in, lint-gated scripts under
  [`scripts/smoke/`](scripts/smoke/) (mypy `--strict` / ruff for the
  Python script, shellcheck for the shell script), referenced by both
  wheel workflows and runnable locally via `make smoke`. A new
  path-filtered [`smoke-dryrun.yml`](.github/workflows/smoke-dryrun.yml)
  workflow runs those scripts against a cheap dev build on any PR that
  touches the release/wheel plumbing, so a future metric rename or
  serialization change reds a PR check instead of a release.
- `wire::MAX_SPACE_SERIALIZE_DEPTH` (128) and `MAX_AST_SERIALIZE_DEPTH`
  (512): the nesting depths past which `FuncSpace` / `Ops` and `AstNode`
  refuse to serialize (#1056). Both are set far clear of real source:
  across the 14 450-file corpus under `tests/repositories` (TensorFlow,
  DeepSpeech, serde, …) the deepest AST is 188 levels and the deepest
  space nesting is 10. The space limit is also more permissive than the
  read side, where a document caps out near 61 levels — `serde_json`'s
  own 128-level `Deserializer` limit charges two levels per space.

### Changed

- Dependencies advanced past the semver-major line Dependabot is
  configured to ignore: `gix` 0.83 → 0.86, `sha2` 0.10.9 → 0.11.0,
  `hmac` 0.12.1 → 0.13.0, `num-derive` 0.4 → 0.5, `clap_mangen` 0.2 →
  0.3, and `jsonschema` 0.46 → 0.49 (dev only), alongside a full
  lockfile refresh in the root workspace and in each of the six
  excluded crates. No behaviour change and no public-API change — no
  `gix` or RustCrypto type appears in a public signature, so
  `STABILITY.md` is unaffected. Two consequences are worth recording.
  `sha2` and `hmac` now sit on the RustCrypto `digest` 0.11 trait
  family, which `actix-http` already pulled in via `sha1` 0.11. Only
  the trait plumbing moved, so every emitted digest is byte-identical:
  the Code Climate fingerprints were already pinned to literal values,
  and the author-identity digests are pinned for the first time by the
  entry below. And `clap_mangen` 0.3 renders a required option after
  the optional ones in the SYNOPSIS, which moves `<-t|--type>` in
  `man/bca-count.1` and `man/bca-find.1`.

- `AuthorId::hashed` and `AuthorHashKey::apply` are now pinned to
  absolute digest values, derived independently from Python's
  `hashlib` / `hmac` so the assertions check conformance to SHA-256 and
  RFC 2104 rather than agreement with our own implementation. Every
  prior assertion on these two was relational — comparing one digest
  against another produced by the same build — and so held whatever the
  hash library emitted. That is the wrong shape for a *stored* value:
  `src/vcs/cache.rs` writes unkeyed digests to disk and honours them
  across version boundaries (`CACHE_SCHEMA_VERSION` tracks the on-disk
  format, not the hash implementation), and `AuthorId::hashed`
  documents the emitted digests as stable cross-report pseudonyms.
  Perturbing either pre-image fails the two new tests and nothing
  else, which is what the gap looked like from the inside.

- Severity prefixes moved off the message producers and onto the layer
  that presents them (#609, #1199). `PreprocDiagnostic`'s `Display` now
  renders the **bare message, with no severity prefix at all**: `bca
  preproc` prints it through the CLI's `warn` helper instead of a bare
  `eprintln!`, so `warning:` is written in one place rather than baked
  into five variants. Embedders that captured these and printed them
  verbatim must add their own prefix. (Previously the five variants
  disagreed among themselves: `SelfInclusion`, `IncludeCycle` and
  `NotPreprocessed` capitalised `Warning:` while the two non-UTF-8
  variants did not, and neither spelling matched any other CLI
  diagnostic, since `warn()` has printed lowercase `warning:` since
  #609.) Per `STABILITY.md`, `Display` impls are stable but their exact
  wording is not, so this is not a breaking change. Two further
  user-visible consequences, both intended: the `IncludeCycle` block no
  longer ends in a newline, so it is no longer followed by a blank
  line; and the CSV writer's non-UTF-8 path warning now reads
  `warning: skipping non-UTF-8 path in CSV output: …`, having moved
  onto the shared helper that already words the other five formats'
  (dropping its capitalised prefix and its lone use of "source path").
  The remaining capitalised warnings in the library — the code-climate
  empty-path skip and the walker's non-regular-file skip — are
  lowercase for the same reason, as are the walk seam's two
  explicit-path notices, which shed the redundant `bca: warning:`
  double prefix #609 removed elsewhere. `make check-diagnostic-prefix`
  (`utils/check-diagnostic-prefix.py`, wired into `make lint`,
  `make pre-commit`, `make ci` and pre-commit) blocks a further site
  from reappearing.

- This project's own `nargs` limit converges from 7 to the shipped
  default of 5 (#1183). Repository configuration only — no library or CLI
  behaviour changes. The convergence was declined twice before, both
  times correctly: #1143 measured it against a hard-tier count that
  missed the soft tier, and #1183 found the offenders were mostly
  artifacts of the gate summing closure parameters into the enclosing
  function. #1196 removed that, and with it the reason to stay at 7 —
  which had become a limit catching nothing in the current tree.

- **`bca check --threshold nargs=N` now gates each callable on its own
  parameter list** rather than on `nargs.total()`, which summed a
  function's parameters with every nested closure's (#1196). This changes
  gate outcomes on existing configurations — read it before upgrading a
  pinned CI.

  A three-parameter function containing a two-parameter sort comparator
  was reported at 5, and the remediation the number implied — fewer
  parameters — was not the one that would clear it. Measured on this
  repository, of the 76 functions a limit of 5 would have newly gated,
  only 17 had six or more parameters of their own; one had a single
  parameter plus five contributed by closures in its body. Refreshing
  this project's own baseline under the new rule retired 45 of its 61
  recorded `nargs` entries.

  Every comparable tool measures the same quantity the gate now does —
  RuboCop `Metrics/ParameterLists`, ESLint `max-params`, Clippy
  `too_many_arguments`, lizard, SonarQube S107, Pylint `R0913` — and two
  of those are the anchors the shipped default of 5 is derived from, so
  the default and the gate were previously calibrated against different
  quantities.

  Nothing escapes the narrower rule. Where a closure opens its own space
  (Rust, the JavaScript family, C#, Go, PHP, Perl, Ruby, Lua, Elixir) it
  is gated on its own offender row. Where a lambda opens none (Python,
  Java, Kotlin, C++) its arguments still fold into the enclosing
  function, and the offender row now shows the split —
  `nargs = 8 (1 own + 7 lambda)` — so the reader can tell whether the
  lever is the signature or the lambda.

  Unchanged: the serialized `function_args` / `closure_args` / `total`
  keys, which remain subtree sums. Only the gate's reading of them moved.
  If you have a `nargs` limit tuned against the old behaviour, expect
  fewer offenders and consider whether the limit is now looser than you
  intended.

- This repository's own `bca.toml` gates `cognitive` at 15, the shipped
  default, instead of the pre-#1140 folklore value of 25 (#1143). This
  is self-scan configuration only — no public API, no metric
  computation, nothing a consumer sees. The old limit was inert: the
  measured maximum here is 20, so 25 could never fire. Re-deriving the
  statistic #1140 used against this tree gives p97.5 = 15 exactly, so
  the shipped default and a local re-derivation agree. Of the 18
  offenders convergence surfaced, 10 were genuinely simplified and 8
  suppressed with a reason.

  The rest of the ledger, stated plainly because it is a cost and not a
  benefit: **14 further entries were added to `.bca-baseline.toml`**,
  all sitting *at* 15. `bca check --explain-threshold cognitive=15`
  reports them as a cluster — "14 of 14 soft-band offenders sit at
  exactly 15, the candidate limit itself … none of them can clear it
  without real work" — which is the same shape `AGENTS.md` warns about
  under *Price a candidate limit at both tiers*. The "0 new offenders"
  reading is circular: it is 0 *because* those 14 are baselined. What
  distinguishes this from the `nargs` case that was rejected is that
  the population extends past the limit (`cognitive` runs to 20, so the
  hard tier gains 18 real offenders) rather than stopping at it, and
  that a baseline entry at the limit keeps a growth alarm a suppression
  marker would discard. `nargs` stays at 7; `nargs` 6 → 5 is tracked
  separately in #1183.
- **(behaviour change)** `bca check` writes its offender rows to
  **stdout** instead of stderr (#1167). The rows are the command's
  product, so `bca check | wc -l`, `| head`, `| rg -c` and
  `bca check 2>/dev/null` now reach them; previously all four reported
  an empty offender list, which reads as "this tree is clean" rather
  than as an error. Everything that is commentary about the run stays on
  stderr: the `--- summary ---` footer, the `--- next steps ---`
  remediation block, GitHub Actions annotations, and the
  `bca: skipped N …` / `bca: filtered N …` / `warning:` / `error:`
  diagnostics. One exception — `--report-format` without `--output`
  gives the aggregated SARIF / Checkstyle / Code Climate document
  stdout, so the human rows fall back to stderr rather than corrupting
  a payload that parses today; `--output <file>` moves the document off
  stdout and the rows return to it. Exit codes (including
  `--exit-codes=tiered`), the `--summary-file` digest, and the
  aggregated document are unchanged. *Migration:* a pipeline reading the
  rows through `2>&1` needs no change; one that captured them with
  `2>file` should now use `>file`.
- An unrecognized or non-suppressible metric name inside
  `bca: suppress(...)` is now reported and skipped rather than voiding
  the entire marker, so `suppress(cognitive, exit)` still silences
  `cognitive` (#1168, reversing the contract pinned by #896). Skipping
  can only narrow a marker's coverage, so a typo still cannot widen
  scope — whereas voiding left the author believing an exemption was
  active when it was not.
- **Baseline schema v6.** `.bca-baseline.toml` records `start_line` only
  for an entry whose `(path, qualified, metric)` identity is shared with
  another — the sole case matching consults it (#1170). Elsewhere the
  field re-rendered on every unrelated edit above a baselined function,
  churning diffs, hiding real value changes in review, and conflicting
  on every merge between branches. Entry order keeps its line-number
  tiebreak: `start_line` moves from third to last in the sort key, so it
  now decides only between entries sharing one identity. v2–v5 baselines
  read unchanged. A baseline written by a *newer* schema now reports the
  version mismatch by name instead of a bare serde field error; the
  reverse direction cannot be fixed from here, so a v6 file handed to an
  already-released pre-v6 `bca` still surfaces the raw error and must be
  regenerated with `--write-baseline`. An entry that pins no line drops
  it from every rendering: `bca exemptions` omits the `:line` suffix
  (text), renders `-` in the Line column (markdown), and omits the
  `line` key (JSON); `bca diff-baseline --format json` omits
  `start_line`.
- `.bca-baseline.toml` is marked `-merge` in `.gitattributes` (#1170).
  The file is generated wholesale, so a textual merge of two branches
  produces hunks that are wrong on *both* sides; git now leaves it
  conflicted as a whole and the resolution is to regenerate with
  `make self-scan-write-baseline-headroom`. `-merge` rather than a
  `merge=ours` driver, which would need per-clone `git config` and
  silently falls back to a normal merge where unconfigured.
- `make pre-commit` and `make ci` now end with a single
  machine-readable verdict line — `BCA_GATE: pass (gate=pre-commit)` or
  `BCA_GATE: fail (gate=pre-commit, exit=2, stage=_pc-fmt)` — replacing
  the success-only `Pre-commit checks passed` / `CI checks passed`
  (#1172). Grep it anchored (`^BCA_GATE:`); absence of the line is a
  third state (crash, kill, interrupt), not a pass. `stage=` is a
  comma-separated list in make's report order, because `-j` stops
  scheduling but lets running jobs finish and fail. Both gates' exit
  statuses are unchanged.
- The 24 tests that depend on the integration corpora now fail with a
  diagnostic naming the cause and the remedy — including that by-hand
  recovery needs `--force` — instead of `bca`'s generic "path does not
  exist" or a corpus-count mismatch that conflated an absent corpus with
  a drifted one (#1171).
- **Metric values move.** A ternary's condition and its two branch
  operands now each count as a Fitzpatrick Rule 9 unary condition in
  `abc.conditions`, matching what Java, Groovy, and C# already did
  (#1102). `a ? !b : !c` scored 1 — the `?` alone — and now scores 4.
  Affects C, C++, Mozcpp, Objective-C, JavaScript, TypeScript, TSX,
  Mozjs, PHP, and Perl. Ruby and Python are not covered by this pass
  but caught up later in this same release (#1161), as did Tcl and
  iRules (#1180), so the cross-language comparison ships even.

- **Metric values move.** `nargs` counts formal parameters for Elixir
  (#1142) and for Perl subroutine signatures (#1147), both of which
  reported 0 unconditionally. Elixir's `def`/`defp`/`defmacro` are
  `Call` nodes whose parameter list sits two `arguments` levels down, so
  the shared `parameters`-field heuristic found nothing; Perl's
  signature is an unnamed `function_signature` child. A `def` inside
  `quote do … end` still contributes nothing (#310), an `@_`-style Perl
  sub still reads 0 correctly, and Bash still reports 0 (the shell has
  no formal parameter list). Anonymous Perl subs read 0 pending an
  upstream grammar fix.

- **Metric values move.** Python resets structural nesting at a `def`
  boundary, so a function defined inside a conditional is scored against
  its own depth rather than the enclosing function's (#1149). Python was
  the only family with a syntactic function-definition node that did
  not, charging an inherited-conditional surcharge no sibling language
  charges; a `def` two conditionals deep now scores 2 where it scored 4.

- Documented Python's per-enclosing-`lambda` surcharge on boolean
  operators in the book's *Cognitive Complexity → Per-language
  deviations* list (#1150). No behaviour change.

- Documented an upstream `tree-sitter-c` limitation on the book's
  *Supported Languages* page (#1209). A pre-ANSI (K&R) function
  definition whose return type wraps the declarator — `int *f(a) int a;
  { … }`, and likewise `char **`, `struct S *` or a `static` pointer
  return — opens no function space under `C` or `Objective-C`, so it is
  absent from `nom.functions`, `nargs` and `bca functions` while the
  orphaned body's decisions are charged to the file's unit space. The
  parse produces no `ERROR` node, so nothing downstream can detect it.
  `C/C++` supports no K&R form at all. No behaviour change; the paired
  fixture in `tests/grammars/c_grammar_metrics.rs` is a drift marker, so
  a grammar bump that fixes the parse fails the test rather than
  shifting metrics silently.

- `bca ops` opens the same function spaces as `bca metrics`, through the
  same source-aware promote-and-classify predicate (#1130). The two
  walks each carried their own copy of the decision and the `ops` copy
  was byte-less, so every Elixir input came back as a bare file-level
  space while `bca metrics` returned the full module/function tree.
  `tests/parity/ops_metrics_space_parity.rs` pins the agreement per
  language. `bca functions` and `bca find --type function` carried the
  same byte-less predicate and were fixed in the same release (#1162,
  below).

- Every walking subcommand exits `1` when the traversal could not read
  an entry — typically a directory the process cannot list (#1131). A
  whole subtree drops out of the resolved set before any file is
  selected, so the per-file read tally stayed zero and the run reported
  success over a tree it had not read; `bca check` was the worst case,
  being indistinguishable from a clean gate. `bca diff --since` reports
  it as `UnwalkableInputs` and `bca vcs` gates its ranking the same way.
  An ignore file still prunes such a directory before the walker
  descends; `--exclude` does not, being a post-walk filter.

- A path named directly on the command line still overrides the walker's
  `--exclude` / `--exclude-from` / `.bcaignore` / manifest `exclude`
  deny-set, but now says so on stderr, naming the glob it overrode
  (#1146). Silent for a seed no language claims, so a
  `git diff --name-only | bca … --paths-from -` pipeline does not warn
  about lockfiles and Markdown. `[check] exclude` is unchanged: it is
  gate scope, survives an explicit path, and is where a
  "never gate this" entry belongs. An absolute explicit path now anchors
  against the CWD before the `[check] exclude` globs are applied, so a
  `./`-prefixed glob matches every spelling of the same file.

- `bca strip-comments` terminates non-UTF-8 output on stdout with a
  newline, matching the UTF-8 branch, and flushes both (#1132).

- `ConcurrentRunner::new`'s `num_jobs` is now the consumer-thread count
  rather than a budget shared with a dedicated producer thread, which
  spawned `max(2, num_jobs) - 1` consumers and left one slot idle
  (#1114). Dispatch happens on the calling thread instead. The signature
  is unchanged; a caller passing `n` now gets `n` consumers rather than
  `n - 1`. `ConcurrentErrors::Producer` is consequently never
  constructed — retained so a downstream `match` still compiles, and
  scheduled for removal in the next major.
- `bca`'s per-file output order for a directory walk is now sorted
  rather than readdir order (#1114). It was never specified, and the
  parallel walker would otherwise vary it run to run; sorting also makes
  it independent of the filesystem and the machine. Any consumer that
  pinned the previous order sees a one-time reshuffle.
- A file that disappears between the walk and its analysis is now a tool
  error (exit 1) rather than a warning-and-exit-0 (#1114). The CLI opts
  out of the runner's redundant `is_file()` re-check, so such a path is
  no longer silently skipped during dispatch; it fails at the read and
  is counted by the same `read_failures` guard that already refuses to
  report a result derived from a partially analysed input set (#1098).
  The previous silent skip was the inconsistency.
- Retuned the `[thresholds]` table `bca init` scaffolds, deriving each
  limit from published thresholds plus a 20-language corpus measurement
  (#1140). `cognitive` 25 to 15 (SonarSource's own default for the
  metric), `nargs` 7 to 5 (RuboCop's value; 7 fired on under 1% of
  functions and so could not catch anything), `abc` 50 to 40, and file
  size split into a `loc.ploc` working limit of 600 with `loc.sloc`
  demoted to a 1200-line bloat backstop (#1138). `cyclomatic`,
  `nexits`, `halstead.effort`, `nom`, and `wmc` are unchanged. Existing
  `bca.toml` files are unaffected; this changes what a fresh `bca init`
  writes. The derivation, a per-language override table, and
  per-use-case profiles are in the book's new *Choosing thresholds*
  recipe.
- `make test` now runs the suite through `cargo-nextest` when it is
  available, matching CI, and falls back to `cargo test` otherwise
  (#1120). nextest schedules every binary's tests into one global pool
  rather than finishing each test binary before starting the next. Set
  `NEXTEST=` to force the fallback, or point it at a specific binary.
  nextest's default profile disables fail-fast so a local run still
  reports the whole failure set, as `cargo test` did.
- Corpus snapshot tests now assert an exact per-corpus file count, and
  separately that each resolved file reached its snapshot assertion
  (#1123). A traversal or glob change that silently analyzed fewer files
  previously still passed while verifying less than it claimed. A
  deliberate corpus bump must update the expected count alongside the
  snapshots.
- **(breaking)** `FuncSpace`, `Ops`, `AstNode`, `wire::FuncSpace`, and
  `wire::Ops` now implement `Drop` (#1056), so fields can no longer be
  moved out of one by value: `let m = space.metrics;` becomes
  `let m = space.metrics.clone();` or `let m = &space.metrics;`
  (`E0509`). The compiler-generated `Drop` glue recursed once per
  nesting level and aborted the process on a deep tree — reachable
  through `bca-web`'s 4 MiB body cap — and an explicit `Drop` that
  hoists descendants into a flat work list is the only way to break
  that chain. [`STABILITY.md`](./STABILITY.md) reserves source-level
  shape breaks for a major bump; this one is landed under a minor as a
  deliberate, documented exception, because the alternative was leaving
  a reachable remote process abort open until `3.0`. The mechanical fix
  at each call site is a `.clone()` or a borrow; 13 sites inside this
  repository needed it.
- Repository layout, no shipped-library change: the twelve helper
  scripts that used to sit in the repository root moved into `utils/`,
  joining `check-tools.sh` and `deploy-book-to-gh-pages.sh`. This
  covers every gate run by `make pre-commit` / `make ci`
  (`check-versions.py`, `check-snapshot-anchors.py`,
  `check-manpage-assets.py`, `check-grammar-marker-sync.py`,
  `check-enums-codegen-drift.sh`, `check-grammar-crate.py`), the
  scripts coupled to them (`check-grammars-crates.sh` and each gate's
  `*-test.py` self-tests), and `verify-name-only-churn.py`. Each script
  now resolves the repository root as `parents[1]` of its own location
  rather than `parent`, so it still runs correctly from any cwd, and
  the two self-tests that stage a copy of their gate into a tempdir
  stage it under `<tmpdir>/utils/` so the tempdir keeps standing in for
  the repository root. Callers were updated in lockstep: the
  `Makefile`, `.pre-commit-config.yaml` (both the `entry:` commands and
  the `^`-anchored `files:` triggers), `.github/workflows/ci.yml`, and
  `.taskcluster.yml` now invoke them as `utils/<name>`. Contributors
  invoking a gate by hand need the new prefix, e.g.
  `./utils/check-snapshot-anchors.py --update`.
- Internal, no behaviour change: the crate's shared `#[cfg(test)]`
  helpers moved out of `src/tools.rs` into a new test-only
  `src/test_support.rs`, retiring that file's `loc.sloc` baseline entry
  (#1066); `python_comprehension_clause_nesting` takes the `Nesting`
  struct rather than three positional `usize` parameters, completing
  the threading started in #1062 (#1070); `increase_nesting` does the
  same at its 43 call sites across the 19 per-language cognitive
  modules and the shared `js_cognitive!` macro (23 language impls),
  which now hold the `Nesting` struct end to end instead of
  destructuring it into three same-typed locals and rebuilding it, with
  the `conditional + function_depth + lambda` sum folded into a new
  `Nesting::total()` (#1086); the two statements that make up the
  function-boundary rule moved behind a shared `enter_function_boundary`
  helper, replacing eighteen longhand copies and leaving Elixir and the
  `js_cognitive!` macro visibly opted out at their call sites (#1103);
  and `node_text`'s safety documentation no longer describes a UTF-8
  char-boundary panic that cannot occur for a `&[u8]` parameter, with
  the same-parse precondition now stated on the `Getter` trait (#1059). `bca.toml`'s
  `exclude_tests` comment, which claimed the option does not lower
  `loc.sloc`, was corrected — #722 made it do exactly that (#1066).

### Performance

- `finalize` no longer re-derives a parent space's Halstead `Stats` and
  MI after every child merges into it (#1106). The per-child pass was
  three map traversals over the parent's accumulated vocabulary for a
  result the parent's own finalize overwrites — `O(children x
  vocabulary)`, quadratic in a file's function count. Only the WMC third
  is load-bearing there (`wmc::Stats::merge` dispatches on the parent's
  recorded `space_kind`), so only it survives in the pop arm. Metric
  values are unchanged. On the widest corpus file (1,808 top-level
  spaces) this is ~10% of the walk; `Limits::default` caps files at
  64 KiB, so the corpus average does not move.

- The per-metric unit-test modules compute only the metric family they
  assert plus its declared dependencies, instead of all thirteen
  (#1127). Single-threaded per-run minima of the all-features lib test
  binary: CPU 4.64 s to 4.20 s, and the 2,317-test `metrics::` tranche
  alone 1.16 s to 0.95 s. Values are unchanged, pinned by a new
  `metric_selection_parity` test asserting a restricted walk reproduces
  the full walk's per-space values for every metric in the selection's
  resolved closure.

- The workspace's 68 integration test files are now 12 directory test
  targets, and `[profile.dev]` sets `debug = "line-tables-only"`
  (#1124). Test binaries drop from 13.03 GB to 1.74 GB, `target/debug`
  from 18.8 GB to 4.9 GB, and a relink after a one-line `src/lib.rs`
  edit from ~90 to ~28 CPU-seconds. No test bodies changed; the
  before/after `cargo nextest list` sets were compared to prove nothing
  was dropped.

- The VCS per-function perf fixture builds 50 commits rather than 200,
  with its wall-clock budget re-derived from 30 s to 8 s at the same
  57x headroom (#1125). Cuts 300 `git` spawns and roughly halves the
  `vcs_per_function` binary. Its work-product assertion was tightened
  from "some function has history" to exact per-function commit counts,
  so a shrunk fixture cannot pass while covering less.

- CLI integration fixtures are served from one shared, content-addressed
  directory instead of being rewritten per test (#1126).

- `bca check` computes only the metric families its resolved thresholds
  read, instead of the whole suite (#1113). Over
  `tests/repositories/DeepSpeech` (12.7k files), median user CPU of five
  runs: a one- or two-metric gate falls from 28.6–29.2 s to 22.4–23.3 s
  (1.24–1.28×). A gate naming nine families — this repository's own
  `bca.toml` — is unchanged, since it already selects nearly
  everything; the ~22 s parse-and-walk floor bounds the saving.
- `bca diff --since` builds both sides' metric sets in memory rather
  than writing one JSON document per source file to a temp tree and
  immediately re-walking, re-reading and re-parsing it (#1116). Each
  tree is reduced to its metric values by a collector running alongside
  the walk, over a bounded channel, so the trees are dropped as they
  arrive rather than all held at once. On `tests/repositories/DeepSpeech`
  (12,732 files), median of three: wall 9.42 s → 7.62 s, system time
  3.59 s → 2.34 s. Output is byte-identical.

  **Peak memory rises**: 437 MB → 599 MB (+37%) on that tree. The
  `MetricSet` for a side is now accumulated during its walk instead of
  in a separate pass afterwards, so the two overlap — that overlap is
  what buys the speed. Draining after the walk instead of during it
  would take the same tree to 831 MB, which is what the bounded channel
  and the concurrent collector exist to avoid. Size CI containers
  accordingly for very large trees.
- The CLI's directory walk runs on `ignore`'s parallel walker instead of
  its single-threaded iterator, and the worker pool no longer reserves a
  slot for a producer thread that finished almost immediately (#1114).
  Walking DeepSpeech in isolation falls from 100.8 ms to 33.6 ms (3.0×);
  a full `check` over it improves ~1.12×. The resolved file list is now
  sorted, so per-file output order is deterministic and independent of
  readdir order — previously it followed the filesystem's own ordering.
- The walk's five result channels are plain `crossbeam` senders rather
  than `Mutex<std::sync::mpsc::Sender<_>>`, so workers no longer take a
  lock per file (#1119). No measurable throughput change at `--jobs 32`
  or `--jobs 64` on a 16-core host; the lock was taken once per file,
  never per record. The change removes a global serialization point and
  four unreachable poisoned-lock branches.
- The debug-build ancestor-chain check no longer re-derives every
  parent, so an unoptimised metric walk is linear like the shipped one
  (#1122). `Ancestors::checked` verified `chain.last() == node.parent()`
  per node on all five walks that thread a chain, and `Node::parent`
  costs `O(depth)` — which made every `cargo test` walk `O(nodes ×
  depth)` and hit the deep-nesting regression tests hardest. The exact
  assertion moved behind `--cfg chain_audit` (`make chain-audit`, plus a
  `chain-audit` CI lane); a plain debug build keeps an `O(1)`
  consequence of the same invariant, which catches a `push` moved ahead
  of the per-node computes and a dropped `truncate` but not a chain
  short by exactly one. The library test suite falls from ~5.0 s to
  ~1.7 s, `cognitive_nesting_is_inherited_at_depth` from ~1.6 s to
  ~0.02 s. No shipped behaviour changes.
- Traversals that enumerate every node's children reuse one
  `TreeCursor` instead of building and freeing one per node, through
  the new internal `Node::children_with` (#1112). The six per-node
  consumers are `Node::preorder`, the suppression-marker DFS, the two
  `Search` walks behind `bca find` and the function-space name lookup,
  `bca dump`'s tree renderer, and Python's instance-attribute scan in
  `metrics::npa::python` — the last being 92 % of the Python metric
  walk's child scans. Over 400 Python corpus
  files that is 414,620 cursor allocations down to 33,328 (−92 %) and
  −2.4 % walk time. Other languages reach `children` on 3-6 % of nodes
  (16 % for C#), where the effect is under 1 %. The Python bindings'
  mirror of that walk — `Node.walk()` and `Node.descendants_by_kind()` —
  hoists a cursor the same way. Every one of the six is pinned by the
  `child_scan_cursors` counter, so reverting one is a test failure
  rather than a silent allocation per node. Metric values are
  unchanged.
- Every file destination and terminal dump writes through an
  explicitly-flushed 64 KiB buffer, replacing the raw `File` and
  `LineWriter` handles the incremental serializers wrote through one
  structural token at a time (#1115). A 165-file `metrics --format json
  --output-dir` run falls from 4,757,028 `write(2)` calls to 303 (1.55 s
  → 0.13 s); the 12 MB `--output` aggregate from 4,757,194 to 197
  (3.36 s → 0.20 s); the `metrics` text tree from 1,524,444 to 165;
  `check --output-format sarif` from 5,622 to 16. The terminal dumps
  emit in bounded chunks rather than one whole-document buffer, so peak
  resident memory for a deeply nested file is 21 MB rather than 545 MB.
  Output is byte-identical in every format.
- The Rust `exclude_tests` prune no longer resolves siblings from the
  node to find the `#[…]` run before an item (#1100). That cost
  `O(attributes × depth)`, because `tree_sitter` resolves a parent by
  descending from the root. The run is now read forward from the parent
  the walker already carries, under a budget that grows with depth;
  a parent too wide for that budget keeps the backward walk, so the
  shallow-and-wide shape is unaffected. A Rust shape with an attributed
  item at each of 4,000 nesting levels drops from 2.05 s to 8.1 ms
  (fitted exponent 2.00 → 1.21).
- `Loc` stores its per-space physical- and comment-line sets as a
  word-array bitset instead of a hash set, making the space-stack merge
  a word-wise OR (#1109). This fixes a quadratic in function-space
  nesting depth — a new `loc/nested-fn-rows` scaling probe fits 2.11
  before and 1.10 after — removes ~7.5M hash probes over the corpus
  repositories, and cuts the retained set payload roughly 29×. LOC
  values are unchanged.
- The C/C++ indirect-include closure is computed once per include-graph
  node in reverse topological order rather than by a fresh DFS per file,
  and `Parser::new` borrows a file's visible macro names out of
  `PreprocResults` instead of deep-cloning them (#1107). Measured over a
  10,918-file tree: `record_indirect_includes` 126.6 ms → 83.0 ms, and
  2.37M fewer allocations per metrics pass, with identical output.
- `Ops` serializes through a borrowed projection instead of cloning an
  owned `wire::Ops` first (#1110). `serde_json::to_string` on a
  hundred-level space nest is 5.4× faster, and a tree past the
  serialization depth limit is refused without cloning it at all
  (109 ms → 0.007 ms at 2,000 levels). Per-space vocabularies are sorted
  before their `String`s are rendered, making `Ast::ops` ~22% faster on
  vocabulary-heavy input. Output is byte-identical in every format.
- `HalsteadMaps::operators` hashes its `kind_id` keys with the crate's
  integer hasher instead of SipHash-1-3, closing the gap left by #1069
  (#1108). Output is bit-identical. The text-keyed `primitive_operators`
  and `operands` maps deliberately stay on SipHash: their keys come from
  the analysed source, so hash-flooding resistance is load-bearing.
- `guess_language` evaluates its extension → modeline → shebang
  precedence lazily, so a file with a recognised extension no longer
  runs the Emacs/Vim modeline regex scan, and an already-lowercase
  extension is borrowed rather than reallocated (#1111). Detection
  results are unchanged.
- `Tree::new` reuses one `tree_sitter::Parser` per thread instead of
  constructing one per file, saving ~2.5% of parse time on trees of
  small files (#1118). Internal only — no public API change.
- `#[cfg(...)]` predicate classification is now linear in the attribute
  body rather than `O(len²)` on deeply nested predicates such as
  `all(all(all(…test…)))` (#1105). Every operand previously rescanned the
  whole remaining tail probing for a top-level comma; commas are now
  bucketed by paren depth in one forward pass and each region queries its
  own bucket. This path is reached from every Rust attribute under
  `--exclude-tests` — which this repository's own `bca.toml` enables — so
  a machine-generated or adversarial source file was a denial-of-service
  vector. Classification behaviour is unchanged, verified against the
  previous implementation over millions of generated predicates. The
  depth-50 000 regression test drops from ~73 s to ~0.04 s.
- Dependencies now build at `opt-level = 1` under the dev and test
  profiles (#1121). The tree-sitter runtime and every grammar are C/C++
  libraries compiled by `cc-rs`, which forwards Cargo's `OPT_LEVEL`, so
  they were previously parsing at roughly half speed in test builds.
  Workspace members are unaffected and stay fully debuggable; the cost is
  a one-time dependency rebuild and a slower cold build. Deliberately 1
  rather than 2, which measured slower on the unit suite.
- Corpus snapshot tests size their worker pool from
  `available_parallelism()` instead of a hardcoded 4 jobs, which had left
  three consumer threads analyzing DeepSpeech's 1042 files (#1123).
- The ancestor chain now reaches the per-language metric bodies, retiring
  the last per-node `Node::parent` calls in the walk (#1096).
  `Getter::get_op_type` (and its `_with_code` variant), `Abc::compute`,
  `Npm::compute`, `Npa::compute`, `Cyclomatic::compute` /
  `compute_with_options`, and `Checker::is_useful_comment` gained an
  `Ancestors` parameter; the ABC condition walkers take the slot's
  parent from the caller that descended from it; and the
  comment-removal walk behind `bca remove-comments` maintains a chain of
  its own. All are `pub(crate)`, so the published API is unchanged, and
  metric values are unchanged for every language. Python's `Cyclomatic`
  `else` arm went the same way: it climbed two links through
  `Node::parent_grandparent_match`, which no search for `.parent()`
  finds at the call site. Four new probes and three new controls guard
  the classes: `halstead/nested-not` fits `time ~ depth^k` at **1.99**
  before and **0.99** after, `abc/nested-if` at **2.00** and **1.14**,
  `cyclomatic/nested-ternary` at **2.06** and **1.27**, and
  `loc/nested-quote` at **2.00** and **1.03**. At depth 4000 those four
  drop from ~478 ms to ~0.57 ms, from ~808 ms to ~3.3 ms, from ~1.13 s
  to ~3.4 ms, and from ~9.6 s to ~9.2 ms; their `halstead/nested-paren`,
  `abc/nested-block`, and `cyclomatic/nested-and` shape controls hold at
  0.97-1.31 either side. Per #1088's lesson, the primitives were checked
  too: the ABC walkers' `Node::previous_sibling` carried the same
  `O(depth)` (`ts_node__prev_sibling` opens with `ts_node_parent`) and
  now scans the known parent's children instead, through the new
  `Node::previous_sibling_under`.
- The ancestor chain now reaches the predicates #1084 left climbing, and
  the `ops`, `bca function`, and suppression-marker walks maintain one of
  their own (#1088). The JS-family `Checker::is_func` / `is_closure`
  name-binding walk, Ruby's `Checker::is_closure` block-versus-lambda
  test, Elixir's `Getter::get_func_space_name`, and the
  suppression scan each resolved ancestors with `Node::parent`, which
  `tree_sitter` answers by descending from the root. `Checker::is_func`,
  `is_closure`, `Getter::get_func_name`, `get_func_space_name`, and
  `NArgs::compute` gained an `Ancestors` parameter to carry it; all are
  `pub(crate)`, so the published API is unchanged.
- Elixir's `Npm` / `Npa::compute` no longer run the class-space
  classifier at all (#1088). Both opened with
  `is_func_space_with_code`, which cost a source-text keyword scan per
  node and, for `def`-shaped calls, an ancestor walk asking whether the
  call sat inside a `quote` template. That walk's answer was always
  discarded: the `defmodule` keyword check immediately below admits
  exactly the nodes the classifier would have, and rejects every node
  the walk was consulted for. Deleting the call removes the work rather
  than making it cheaper. Counts are unchanged, pinned by two new tests
  over a `defmodule` nested inside a `quote`.
- `Node::wraps_any` — reached from `is_child` and `has_sibling`, and so
  from every language's checkers and getters — scans a node's children
  with a cursor instead of chaining `next_sibling()` (#1088). The chain
  #217 introduced was premised on a sibling step being `O(1)`; it is
  not, because `ts_node_next_sibling` resolves the parent first, so the
  scan cost `O(children × depth)`. This was the dominant term behind the
  JS closure classifier: the new `nom/nested-arrow` probe fits
  `time ~ depth^k` at **1.97** before and **1.03** after, and at depth
  4000 `nom` over nested arrow functions drops from ~17.6 s to ~6.3 ms,
  while its `nom/nested-declared-function` shape control — the same
  nesting written with `function` declarations, which need no ancestor
  walk — holds at 1.08 either side. Ordinary input benefits too: a
  metric walk over the 384-file `pdf.js` corpus drops from ~443 ms to
  ~370 ms. Metric values are unchanged for every language.
- The `ops` walk builds the file-level vocabulary once instead of once
  per closing space. `finalize` rebuilt the innermost still-open space's
  operator and operand lists on every call — that is, every time the
  walk left a function — and every result but the last was immediately
  overwritten, so a file with F function spaces rebuilt the root's whole
  vocabulary F times. Only the root needs the trailing rebuild, and it
  now happens once, in `ops_inner`, after the walk drains. On
  `hlo_instruction.cc` (4 057 lines, ~180 spaces) `bca ops -O json`
  drops from ~39 ms to ~27 ms per run — faster than before the #1091
  sort was added, which the redundant rebuilds would otherwise have run
  F times over. Output is byte-identical.

- The metric walk carries its ancestor chain down the traversal instead
  of rediscovering it with `Node::parent`, which `tree_sitter` resolves
  by descending from the root (#1084). `Checker::is_else_if` (13
  languages), `Loc`'s declaration gate (8 languages), Python's
  `cognitive` boolean-operator walk, and Elixir's `quote`-template
  lookup in `Nom` each cost `O(depth)` per node and were therefore
  quadratic in nesting depth. The three depth-scaling probes covering
  them fit `time ~ depth^k` at 1.97 / 1.95 / 2.01 before and
  1.14 / 1.12 / 1.02 after, and moved from the harness's quadratic
  bound to its linear one; at depth 1000, `nom` on nested Elixir
  `quote` blocks drops from ~260 ms to ~2 ms and `loc` on nested C
  declarations from ~62 ms to ~1 ms. Metric values are unchanged for
  every language.
- `Loc`'s per-line sets and the cognitive nesting map no longer pay for
  SipHash and incremental rehashing (#1069). The line-number sets and
  the node-id keyed nesting map are keyed by integers this crate
  produces itself, so hash-flooding resistance buys nothing; both now
  use the crate's fast integer hasher, and the nesting map is sized up
  front from the subtree's node count. `corpus/walk/loc` measured 7%
  faster and the depth-1000 cognitive shape 12% faster on an
  interleaved paired benchmark; output is bit-identical.

### Fixed

- C, C++, Mozcpp and Objective-C functions whose declarator is obscured
  by an unexpanded function-like macro now report their own arity rather
  than the macro's (#1213). `RUN_STATS_METHOD(allocate)(JNIEnv *env,
  jclass clazz)` — the JNI shim idiom — nests one `function_declarator`
  directly inside another, and since #1200 `nargs` read the innermost,
  so the macro's `(allocate)` was the answer and the function's own
  arguments were discarded. TensorFlow's four `run_stats_jni.cc` shims
  all reported 1 while declaring 2, 3, 4 and 3; `bca check --threshold
  nargs=1` found one violation in that file and now finds five. The
  multi-argument spelling moved the other way, `void MACRO(a, b)(int x)`
  having reported the macro's 2 rather than the function's 1.

  Neither language permits a function to return a function type (C11
  6.7.6.3p1, C++ `[dcl.fct]`), so the direct nesting is not a declarator
  chain and the rule is structural rather than a guess about macros: a
  legitimate function returning a function pointer,
  `int (*fp(int a, int b))(int c)`, interposes a
  `parenthesized_declarator` and does not move, nor does C++
  `operator()` — the one construct whose source text resembles the
  shape, at 1,546 function spaces across the corpora, none of which
  nests: the grammar emits a single `operator_name` with the parameter
  list as its sibling.

  The space keeps the **macro's** name, so the 44 names #1208 recovered
  are unaffected: after `##` pasting the real symbol is not in the
  source at all, and the macro is the token a reader greps for. Arity
  now comes off the outer declarator and the name off the invocation it
  wraps, which retires #1208's same-*node* pairing in favour of one
  *function*, one walk.

  46 function spaces change across the corpora, all in files with no
  snapshot coverage. 27 are macro shims, fixed. The other 19 are
  `TF_ASSIGN_OR_RETURN(…); if (…)` statements that tree-sitter recovers
  into this same shape, where the outer "parameter list" is the `if`
  condition; recovery trees have never been inside the walk's contract
  and those numbers were not arities before the change either.

- C, C++, Mozcpp and Objective-C function spaces whose declared name
  sits under an extra declarator layer now carry that name instead of
  `null` (#1208). Two spellings were affected: a function returning a
  function pointer, `int (*fp(int a, int b))(int c)`, and the
  macro-obscured declarator `RUN_STATS_METHOD(allocate)(JNIEnv *env)`
  that JNI shims use. Both put a `function_declarator` in the slot
  `get_func_space_name` expected an identifier in, so the name resolved
  to nothing. The four getters now take the name from the same
  declarator walk `nargs` has taken the arity from since #1200, so the
  two answers about one function can no longer disagree. (#1213, above,
  then moved the macro spelling's arity to the outer declarator while
  leaving its name where it is, so for that one shape the two answers
  come off two nodes of the same walk rather than one.)

  Three surfaces change with it: `name` in the metric output, `bca
  functions`, which had been rendering these as a red `error:` line, and
  the `bca check` / `.bca-baseline.toml` offender key, which had been
  the line-dependent `<anon@L…>`. A C-family baseline holding such an
  entry needs one refresh, after which the key is stable across line
  drift like any other named function.

  Six spaces move the other way, all of them inside `ERROR`-recovery
  subtrees, where no declarator rule holds and any strategy's answer is
  arbitrary: two lose a name they had (one of which had been reporting
  an `if` statement's callee as a function name) and four are renamed.
  One of the renames is a regression to weigh when refreshing a
  baseline: an annotation macro carrying an argument —
  `T *f() TF_LOCKS_EXCLUDED(mu_)`, the TensorFlow / Abseil idiom — now
  names the space after the macro, so two members of one class sharing
  an annotation share one offender key. Measured over `DeepSpeech` and
  `pdf.js` (14,269 files): 46 spaces named, 2 un-named, 4 renamed, for a
  net 44 fewer nameless spaces.

- C, C++, Mozcpp and Objective-C functions whose return type is a
  pointer or a reference now report their real arity instead of 0
  (#1200). C declarator syntax nests outward from the declared name, so
  `FILE *f(int a, int b, int c)` puts the parameter list on a
  `function_declarator` wrapped by the `pointer_declarator` the return
  type contributed; `nargs` read the wrapper, found no parameters and
  reported nothing. `int **`, `int &`, `Foo &&`, `static int *` and a
  member function returning a reference were all affected. A function
  returning a *function pointer* — `int (*fp(int a, int b))(int c)` —
  is fixed in the same walk: it had been reporting `(int c)`, the
  return type's list, rather than its own.

  A C++11 `[[…]]` attribute on a function no longer hides its
  parameters either. `attributed_declarator` is the one declarator rule
  that puts its declarator *first*, so
  `int f(int a, int b) [[deprecated]]` had always reported 0. The GNU
  `__attribute__((…))` spelling was never affected — every one of the
  four grammars absorbs it into the `function_declarator` instead of
  wrapping it.

  A C++ conversion operator is explicitly *excluded* from the walk:
  `operator int (*)(int x)` takes no arguments however many its target
  type has.

  **Metric drift.** Serialized `nargs` rises wherever such a function
  appears — 3,336 recorded values across 276 files of the `DeepSpeech`
  corpus, and nothing falls. Since #1196 made the gate read a
  callable's own parameter count, these functions were invisible to
  `bca check --threshold nargs=N` and can now trip it.

- C's `(void)` marker is no longer counted as a parameter. `int f(void)`
  declares nothing, but the grammar emits a real `parameter_declaration`
  for the `void` and every `nargs` filter counted it, so `f(void)` and
  `f(int)` both reported 1. Fixed for C, C++, Mozcpp and Objective-C,
  in both the function and the closure channel — an Objective-C block
  literal `^(void){ … }` counted the marker too, because its arm
  matched parameter kinds positively instead of routing through the
  shared `count_args` helper, and so never consulted the hook (#1218).
  The distinction needs the source bytes rather than the tree — an
  unnamed parameter is the same shape and really is one argument — so
  `Checker` gained an `is_empty_param_marker` hook that defaults to
  `false` and reads them.

  **Metric drift.** 24 recorded values fall to 0 in the `DeepSpeech`
  corpus, all in `pywrapfst.cc`, its only `(void)` definitions. The
  block-literal half moves nothing recorded: `^(void)` appears in two
  corpus files, both `.mm`, which route to C++ — where blocks are not a
  construct.

- A comment written inside a parameter list is no longer counted as a
  parameter (#1201). tree-sitter attaches such a comment as a direct
  child of the parameter-*list* node rather than inside the parameter it
  documents, and every `nargs` filter listed punctuation only, so
  `int h(int a /* one */, int b /* two */)` reported 4 and the C++ idiom
  for a deliberately unused parameter, `void f(int /*unused*/)`,
  reported 2. Fixed for C, C++, Mozcpp, Objective-C, JavaScript, MozJS,
  TypeScript, TSX, Python, Rust, Java, C#, PHP, Ruby, Groovy, Elixir and
  Kotlin lambdas in one shared predicate. Go, Lua, Tcl, iRules,
  Objective-C methods and blocks, Kotlin functions and Groovy closures
  already reported the right count here and needed no fix; Perl had
  carried the exclusion privately since its signature support landed.
  Objective-C blocks were correct only incidentally — their arm listed
  the parameter kinds it wanted rather than excluding comments, and
  nothing asserted it — so #1218 routed them through the shared
  predicate and added the fixture.

  **Metric drift.** Serialized `nargs` falls wherever a signature
  carries a comment — across the `DeepSpeech` corpus, 854 recorded
  values, including kenlm's `DontBhiksha::DontBhiksha` (7 → 4) and
  `ReadBackoff` (3 → 2). A signature that previously tripped
  `bca check --threshold nargs=N` on its comments alone now passes.

- **Serialized shape.** `npm` and `npa` emission is now decided by the
  space's kind alone, for every language (#1203). #1197 declared the rule
  — containers and the file `unit` root carry the block, a function space
  never does — but enforced it only for the ten languages it routed
  through a shared predicate. The other seven kept enabling from their
  own grammar node kinds and disagreed with it in both directions:

  - A Go or Rust `struct` declared inside a function body put the block
    on that **function** space. Across the `serde` corpus that was 39
    function spaces in 10 files, most of them `#[test]` functions
    declaring a local `struct`.
  - A C++ `namespace`, and any file root whose only container sat inside
    a function, carried **no** block. In the last case the counts were
    serialized nowhere at all — they reached the root's `_sum` fields,
    which nothing emitted — so merely suppressing the function-space
    block would have deleted them from output rather than relocating
    them.

  The space kind is now the only input, recorded once per space by the
  walker, so there is no per-language surface left to deviate on.
  Practically: every container space and every file root of a language
  with class-shaped constructs carries both blocks, and no function space
  does. In this repository's integration corpora that adds a block to
  1,214 file roots and 1,332 C++ namespaces, and removes one from 39 Rust
  function spaces.

  **No metric value changed.** The counts always rolled up through every
  enclosing space regardless of which one serialized them; this moves
  which space reports them. Thresholds are unaffected — `bca check` reads
  `metrics.npm` directly through `MetricScope`, which never consulted the
  emission gate. `STABILITY.md` already places which space carries which
  block outside the shape contract.

  Languages with no class-shaped construct at all — Bash, C, Lua, Perl,
  Tcl, iRules — still emit neither block, rather than gaining an all-zero
  one on every file root. Go remains the one language whose `npm` / `npa`
  appear only on the root, because its space tree has no container kind;
  since `bca check` gates both on container spaces, no `npm` or `npa`
  limit can fire on Go source. Both are documented in the metrics guide.

  One consequence reaches a front-end. The Python `to_sarif` binding
  walks serialized JSON and skips a metric whose key is absent, so it
  silently dropped every `npm` / `npa` offender on a C++ `namespace` —
  a kind `MetricScope::Container` admits and the CLI has always gated,
  reading the struct rather than the JSON. The two front-ends now agree,
  and SARIF output may gain namespace-scoped findings it was missing.

- **Serialized shape.** `npm` and `npa` no longer emit an all-zero block
  on function spaces (#1197). They enabled themselves from
  `Checker::is_func_space`, which answers "does this node open a space",
  not "is this a scope that owns methods and attributes". C#, JavaScript,
  MozJS, TypeScript, TSX, PHP and Ruby therefore carried a block on every
  ordinary method, and #1184 extended that to Kotlin `get()` / `set()` /
  `init { … }`, Java and Groovy `static { … }` and the JS-family
  `class_static_block` — which is the inconsistency the issue reports: a
  `<get>` space carrying OOP metrics while the `m` beside it did not.
  Kotlin, Java, Groovy, JavaScript, MozJS, TypeScript, TSX, C#, PHP and
  Ruby are affected; Python, Rust, C, C++, Mozcpp, Go, Objective-C and
  Elixir gate on their own node kinds and do not move.

  The rule is now `SpaceKind::is_member_scope`, which `wmc` already
  followed and which the three metrics share as a single definition:
  containers and the whole-file `unit` root carry the block, a function
  space never does. **The file-root roll-up is retained** — an earlier
  draft of this fix narrowed to containers alone, which would have
  deleted the whole-file `class_npm_sum` (7,530 fields across the
  integration corpus, 400 of them non-zero) and left `npm` / `npa`
  disagreeing with `wmc` about the same root.

  Consumers reading `metrics.npm` off a *function* space now find the key
  absent. Across the integration corpus that removes 6,974 blocks, of
  which 6,969 were entirely zero. The other five belong to a function
  that lexically contains a class — a PHP `new class { … }` or a
  JavaScript class inside a callback — where the block was that nested
  class's roll-up; it remains available on the class's own space and in
  the file-root total, and `wmc` has always omitted it in the same
  position. No metric value changed anywhere.

  The CSV projection is a fixed-column format and is unaffected: it
  writes the `npm.*` / `npa.*` columns on every row regardless of space
  kind, carrying the real accessor values.

- **Metric drift.** ABC `conditions` moves wherever a comment sits inside
  a ternary (#1181). Slots are now addressed by grammar field rather than
  by neighbouring token or fixed index, which fixes two opposite errors
  from one cause: C, C++, Objective-C, Mozcpp, PHP, Perl, JavaScript,
  TypeScript, TSX and MozJS **over**-counted (`a ? /*n*/ (b) : c` scored
  3 against `a ? (b) : c`'s 2), while Java, C# and Groovy **under**-counted
  (`a ? /*n*/ !b : c` scored 2 against `a ? !b : c`'s 3).
- **Metric drift.** ABC `conditions` counts Ruby's and Perl's `not`
  keyword like `!` (#1182). `if not b` scored 0 against `if !b`'s 1, and a
  `not` ternary scored 2 against the `!` form's 4. Lua and Elixir were
  already correct.
- **Metric drift.** Tcl and iRules gained the Phase 2B slot routing every
  other language already had (#1180): `if {$a}` and `while {$a}` move 0 →
  1, `if`/`elseif`/`else` 2 → 4, and `expr {$a ? !$b : !$c}` 1 → 4,
  matching the value the other languages report for the same expression.
  The argument and `return` slots remain unrouted.
- **Metric drift.** A lambda written without its optional parentheses
  reports its parameter in Java and C# (#1185). `x -> x + 1` scored
  `nargs` 0 where `(x) -> x + 1` scored 1; the parameters are billed to
  `closure_args` as before.
- **Metric drift.** JavaScript-family generator functions are classified
  as functions rather than closures (#1186), so `nom`'s function/closure
  split, `nargs`' `fn_args`/`closure_args` split and cognitive nesting all
  move for `function*`. `bca functions` and `bca find --type function`
  now report a named generator, which they previously omitted.
- **Metric drift.** An immediately-invoked function expression is a
  closure whether or not its result is bound (#1188). `nom` and `nargs`
  previously classified `(function(){…})()` and
  `const v = (function(){…})()` differently. A class field initialiser and
  a non-identifier-keyed object property are now classified the same way
  whether written as a function expression or an arrow.
- **Metric drift.** Cognitive complexity resets the lambda surcharge at
  every function boundary, not only in the JavaScript family (#1187). A
  function *declared inside* a closure scored 3 where the same body
  outside one scored 2, in Rust, Java, C++, PHP and C#. Separately,
  `(function(){ function g(){…} })()` and `(() => { function g(){…} })()`
  now charge `g` the same function depth.
- **Metric drift.** The file-level unit's line span is anchored at line 1
  (#1195). A whitespace-only file reported `0..0`, and any file opening
  with blank lines reported a span that omitted them — `"\n\n\nfn a(){}\n"`
  gave `4..4` of a 4-line file. An empty file still reports `0..0`, having
  no lines.
- **Metric drift.** Kotlin `init { … }` complexity now contributes to its
  class's WMC, which follows from the new function space (#1184).
- A `bca.toml` `exclude` glob keeps applying under a directory seed
  (#1189). `bca metrics -p sub` moved the walk root, and manifest globs —
  written against the manifest's directory — silently stopped matching.
  The rule is now stated once and shared by the walker and the `bca check`
  gate.
- `utils/check-snapshot-anchors.py` lexes char literals, byte-raw strings
  and the whole of `src/metrics/` (#1192). A `b'"'` opened a string span
  that hid every later snapshot call, and the scan was non-recursive so
  the 126 files under the per-language subdirectories were never checked.
  Latent — no live count changed.

- `utils/check-diagnostic-prefix.py` decides string and comment state
  with a lexer rather than a per-line regex (#1219). Three inputs made
  it read a raw-string open where there was none — a plain string whose
  closing quote follows `r` (`"dir/r"`), and an unterminated `r"` in a
  trailing `//` or a `/* … */` comment — after which every line to the
  next quote was skipped and any severity literal in between was a
  false clean. Neither cheap fix works: no lookbehind can express the
  first, since what distinguishes it is that the quote *closes* a
  literal, and stripping comments by regex would truncate `"http://x"`
  mid-literal and open a phantom span of its own. The walk is ported
  from `check-snapshot-anchors.py`, which needed the identical machine;
  both sides now name the other. One deliberate widening: a severity
  quoted inside any comment is skipped, where before only a whole-line
  comment was. Latent — no live count changed, verified by diffing both
  scanners over all 559 tracked Rust files.

- The book and [`STABILITY.md`](./STABILITY.md) scope the
  object-oriented emission rule to `npm` and `npa` (#1220). Both said
  all three blocks follow the space's kind with no grammar deviating in
  either direction; that holds for `npm` / `npa`, which are gated
  centrally by kind, and not for `wmc`, which is decided per language.
  Go emits no `wmc` block on any space including the file root while its
  `npa` / `npm` do appear there, and a `namespace` space — a C++ or
  Mozcpp `namespace`, or a Ruby `module` — carries `npm` / `npa` but no
  `wmc` because its member functions are free functions rather than
  methods of a class. Both narrowings are now asserted in
  `container_scope_tests.rs`, which previously tracked only `npm` and
  `npa` — nothing pinned `wmc`'s scope, which is how one rule came to
  describe three blocks.

- **Metric values move.** A Java record's compact constructor
  (`record R(int a) { R { … } }`) now opens its own function space
  instead of charging its body to the enclosing class (#1160).
  `cognitive`, `cyclomatic` and `nexits` move from the record's class
  space onto the constructor's; `nom`, `npm.class_methods` /
  `class_npm_sum` and `wmc` each count it; and `nargs` reports the
  record's component count, so the compact and canonical spellings of
  one constructor score identically. `bca check` can flag a compact
  constructor for the first time — previously it could never be flagged
  however complex it got.
- **Metric values move.** The JS-family cognitive function-boundary
  rule now applies to `method_definition` and to `function_expression`
  nodes the `Checker` calls functions, not to `function_declaration`
  alone, and the function-depth `stops` list carries the same kinds
  (#1159). A method or bound function expression defined inside
  conditionals no longer inherits the enclosing nesting, and a
  `function` declared inside a method now takes the depth surcharge.
  JavaScript / TypeScript / TSX / MozJS values move in **both**
  directions — 544 corpus values up and 14 down, the increases
  dominated by declarations inside IIFE module wrappers, which now take
  the surcharge they always should have.
- **Metric values move.** ABC now counts the condition and both branch
  operands of a Ruby ternary, and the condition of a Python conditional
  expression, bringing both level with Java and the C family (#1161).
  Ruby's `a ? !b : !c` went from 1 to 4; Python's `a if c() else b`
  from 1 to 2. Tcl and iRules got the same coverage through their
  broader Phase 2B slot routing, which also ships in this release
  (#1180). Bash's ABC is keyword-driven by design rather than
  expression-driven, so its arithmetic ternary is not a gap — the
  deviation table now says so, since "scores 0" and "not applicable"
  read the same to a reader.
- A space's `end_line` is keyed on the node's end column rather than on
  its `SpaceKind` (#1163). A Perl function that is the last item in a
  file reported a span one row past its parent unit and past EOF, which
  is not a representable tree and is the shape that produced the
  `usize` underflow in #1051 — `bca check` offender lines, the SARIF
  `region` and any editor integration slice source by these spans. The
  `SpaceKind::Unit` arm's missing `+ 1` was never a statement about
  units; it was a statement about nodes that end at column 0, which the
  root always does. `bca functions` computed the same quantity a third
  way with no unit case at all, and is fixed with them; sources with no
  trailing newline reported a unit span one row short in *every*
  language, observable only through the verbatim library API.
- `bca functions`, `bca find --type function` and `bca count --type
  function` reported nothing at all for Elixir sources (#1162). Elixir's
  `def` / `defp` / `defmacro` are not distinct grammar productions but
  `Call` nodes whose target identifier spells the keyword, so a
  byte-less predicate cannot see them; both seams now read the source
  bytes, as `bca metrics` and `bca ops` already did. The `Ast::functions`
  and `Ast::find` library seams and the web `/function` endpoint were
  affected identically. A cross-language parity test now pins that every
  named `Function`-kind space in the `metrics()` tree appears in
  `functions()`, which is the invariant that would have caught this seam
  and #1130's together.
- `[check] exclude` globs from a `bca.toml` are resolved
  against the manifest's directory rather than the caller's working
  directory (#1164), so an exemption written for the project root holds
  when `bca check <file>` is invoked from a subdirectory. `[check]
  exclude` is the one exclude surface documented as surviving an
  explicit path — #1146 steered the agent-feedback hooks and this
  repository's own dev-tooling exemptions at it for exactly that
  reason — and the guarantee silently did not hold whenever the
  caller's cwd was not the manifest root, which for a per-file editor
  hook is unpredictable. The `--exclude` override warning added in
  #1146 was anchored by the same helper and went silent under the same
  conditions; it is fixed with them. CLI `--check-exclude` / `--exclude`
  globs stay relative to the working directory, because that is where
  the user typed them. A manifest `exclude` glob under a *directory*
  walk keeps its previous walk-root anchoring, which differs from the
  manifest root only when the walk does not start there; that remaining
  gap predates this change and is tracked in #1189.

  **Migration — only if your `bca.toml` sets `paths` to something other
  than `["."]`.** `[check] exclude` globs were previously matched
  against each file's path relative to the *walk root*; they are now
  matched relative to the manifest's directory. Where `paths = ["."]`
  those are the same directory and nothing moves — which is the common
  case, and why this is a fix rather than a break. Where they differ,
  both directions are live, and the second is the one to check for:

  ```toml
  paths = ["sub"]
  [check]
  exclude = ["vendor/**"]      # walk-root-relative: exempted before, reports now
  exclude = ["sub/vendor/**"]  # manifest-relative: matched nothing before, exempts now
  ```

  The first direction fails loudly — an offender you had exempted
  starts being reported. The second is silent and is the dangerous one:
  a glob that never matched anything, and that nobody would have
  noticed was inert, now exempts real violations. Run
  `bca check --print-effective-config` and confirm the resolved
  `check_exclude` list still means what you intended.
- A threshold written with the bare `bca diff --metric` alias (`sloc`,
  `ploc`, `lloc`, `cloc`, `blank`) now *overrides* the same metric's
  dotted spelling instead of adding a second, independent threshold
  (#1165). Aliases are resolved where each layer is parsed — the
  manifest and `--config` `[thresholds]` table, `[thresholds.soft]`,
  `[thresholds.lang.<slug>]`, and `--threshold` — so the layers merge by
  metric rather than by spelling, one `(function, metric)` pair emits
  one offender line, and `--print-effective-config` prints the limit
  that actually fires; its output now round-trips through `--config` to
  an identical gate result. A single table that sets one metric under
  both spellings is rejected rather than silently keeping whichever key
  sorts last.
- `bca check --tier=soft=RATIO` (and its `--headroom` alias, and a
  `"<ratio>x"` string in `[thresholds.soft]`) scaled the lower-is-worse
  `mi.*` family the wrong way (#1166). A limit there is a *floor*, so
  multiplying it by the ratio lowered it: `[thresholds] "mi.original" =
  20` with `--tier=soft=0.5` resolved to a soft floor of 10, below the
  hard floor it was meant to warn ahead of. The early-warning band could
  never fire first, making the soft tier a silent no-op for the whole
  family. The ratio now tightens each limit in its own direction — 20
  with `soft=0.9` resolves to 22.2223, rounded up so the band never
  resolves below the exact quotient. The `[thresholds.soft]`
  soft-looser-than-hard check, previously restricted to higher-is-worse
  metrics because of this defect, now applies to `mi.*` too.
- A suppression marker carrying a rationale on the same line
  (`// bca: suppress(nargs) — threaded context`) is no longer rejected
  as malformed and silently inert (#1168). Anything after the metric
  list is free text, with no separator required: the parentheses are the
  positive signal that the comment is a marker. `AGENTS.md` and the book
  prescribed writing the rationale there, which is the spelling that
  voided the marker. A *bare* verb (`// bca: suppress`, no list) still
  takes no trailing text and warns when it carries any — no separator
  set can distinguish a rationale from prose *about* the marker, since
  `-`, `:`, `//`, `#` and the dashes are exactly what someone writing
  `// bca: suppress - we removed this marker, see #123` reaches for, and
  reading that as a marker silences every metric on its function with no
  diagnostic at all. The warning now names the way out: list the metrics
  you mean, or move the reason to the line above.
- Corrected the inverted doc comment on `python_apply_boolean_operator`,
  which described its ancestor walk as counting control constructs and
  stopping at lambdas when `count_specific_ancestors`'s
  `(ancestors, check, stop)` order makes it do the reverse (#1090). Adds
  a test discriminating the previously-untested `ExpressionList` stop
  arm through both routes that reach one under a lambda — a parenthesised
  `yield` and an f-string interpolation. No metric values change.

- `make bench-scaling` now measures two axes (#1133). `Probe` carries an
  `Axis` (`Depth` or `Width`), and the new `nom/wide-attributed-fn`
  probe sweeps one parent's child count so a walk that is linear in
  nesting depth but quadratic in a parent's child count fails the gate —
  the class #1100's rejected fix belonged to, which every existing probe
  passed. Falsified against that fix: exponent 0.97 clean, 1.99 with it
  reinstated, while all depth probes stayed green.

- `bca vcs`, `bca vcs commit`, and `bca vcs trend` exit `0` again when
  their consumer closes the pipe (`bca vcs … | head`). The
  `write_text` flush below made the resulting `EPIPE` visible, and those
  emitters `die`d on every I/O error, so a routine pipeline became
  `error: writing vcs output: Broken pipe` and exit `1` while `dump`,
  `metrics`, and `ops` piped into the same consumer exited `0`. The
  `BrokenPipe` exemption the rest of the CLI applies is now shared by
  the `vcs` family; a genuine write failure still exits `1`.

- `bca vcs`, `bca vcs commit`, and `bca vcs trend` exit `1` when their
  report cannot be written to stdout. All three emit compact JSON, and
  `std::io::Stdout` is a `LineWriter` over a 1 KiB buffer: a document
  containing no newline and shorter than that was accepted into the
  buffer and only written by the exit-time cleanup flush, whose error is
  discarded — so a full disk or a closed `>` target produced exit `0`
  with no output at all. #1132 fixed the walk's stdout paths and missed
  these three, because every other `vcs` format (`yaml`, `toml`,
  `markdown`, `html`, `csv`, the default table) contains newlines and
  was already surfacing the failure. `path_io::write_stdout_parts_or_die`
  carried the same missing flush; no shipped subcommand can reach it
  with a newline-free document, so that half was latent.
- Every crate the root manifest `exclude`s — the five vendored
  `bca-tree-sitter-*` grammars and `enums` — now roots its own
  workspace (#1145). `exclude` denies membership without terminating
  cargo's upward search for a workspace root, so inside a git worktree
  under `.claude/worktrees/` that search escaped the worktree and
  resolved against the main checkout, where the crate's path is neither
  a member nor excluded; `cargo metadata` errored and took `cargo fmt
  --all` and every `make pre-commit` stage chained behind it with it.
  `.claude/worktrees` is excluded from the root workspace for the
  mirror-image reason.
- The `tree-sitter` runtime is `=0.26.11`, up from the `=0.26.9` that
  `v2.0.0` shipped — two upstream patch releases, taken via Dependabot
  and pinned in lockstep across the root manifest and every excluded
  crate. `tree_sitter` is re-exported from the library root, so the
  resolved version is visible to consumers; per `STABILITY.md`, a
  runtime bump rides a minor release. No grammar content moved: the
  vendored `parser.c` sources and every external grammar pin are
  byte-identical to `v2.0.0`.
- The vendored grammar manifests pin their tree-sitter dependencies with
  `=X.Y.Z` requirements rather than caret ranges (#1151):
  `tree-sitter-cpp` in `bca-tree-sitter-mozcpp` and
  `tree-sitter-javascript` in `bca-tree-sitter-mozjs`. Both are
  build-dependencies of published crates, so the loose requirement let a
  downstream consumer resolve a different grammar than this workspace
  builds against, and let a plain `cargo update` move one silently.
  `tree-sitter-language` deliberately stays caret-ranged — it is the
  ecosystem's shared `LanguageFn` shim, not a grammar, and `=`-pinning
  it makes both this workspace and downstream consumers unresolvable.
- A new gate, `utils/check-excluded-manifests.py`, holds both of the
  above (wired into `make lint`, `make pre-commit`, `make ci`, the
  pre-commit hooks, and the `lint` CI job). It parses manifests with
  `tomllib` and checks the root manifest's `[workspace.dependencies]`
  block alongside each excluded crate's own tables.
- `utils/check-grammar-marker-sync.py` compares the vendored grammar
  marker against its baseline with the requirement operator stripped, so
  `=0.23.4`, `= 0.23.4` and `0.23.4` all name the same upstream version.
  The literal comparison reported drift for #1151's pin tightening,
  which touched no generated byte.
- `make book-pot` writes `messages.pot` to the book's `po/` directory
  again, and now refuses to run against an unsupported mdBook. The
  target passed a relative `-d po`, which mdBook 0.4 resolved against
  the book root but 0.5 resolves against the working directory, so
  under 0.5 the pot silently landed in `./po/` at the repository root
  and `make book-po-update` then failed on a missing file. The
  destination is now absolute. A version guard was added alongside it:
  mdbook-i18n-helpers 0.3.x pairs only with mdBook 0.4.x, and the
  mismatched pair that does *not* error — helpers 0.4.x — extracts
  fenced code blocks one entry per line instead of one per block, so
  the following `msgmerge` marks every code-block entry fuzzy and
  rewrites `po/ja.po` against msgids the pinned toolchain never
  produces. `docs/development/translations.md` now pins both halves of
  the toolchain and describes both failure modes.
- Comment-only rows are no longer counted as physical lines of code in
  Tcl, iRules (#1135), and Perl (#1137). Both defects let a token reach
  the `_` catch-all that ends `stats.ploc.lines.insert(start)`: in the
  Tcl family it was the row terminator, which those two grammars alone
  surface as a token child of the root and whose start row is the row it
  *terminates*; in Perl it was the `#` *inside* the `comments` node,
  which additionally reclassified the row from comment-only to
  code-and-comment. A realistic fourteen-row Tcl file with six comment
  rows reported `ploc 13` instead of `7`. The Tcl family also counted
  whitespace-only rows — trailing whitespace on an otherwise blank
  line — as code, so `blank` moves there too; a wholly empty row was
  unaffected either way. **PLOC, `ploc_average`, `ploc_min` /
  `ploc_max`, and (for the Tcl family) `blank` move for Tcl, iRules,
  and Perl sources**; no other language is affected.
  `a_comment_row_is_never_counted_as_code` now sweeps every language
  and comment spelling, comment-before-code and comment-after-code, so
  a third instance of this shape fails a test rather than shipping.
- Documented the one input class where a trailing newline does change a
  LOC value (#1087). #1067 established that a trailing newline is a
  formatting detail no LOC sub-metric may depend on, but whitespace-only
  source violates that: most grammars collapse tree-sitter's root to a
  zero-width node at end-of-input, so `b"  "` reports `sloc 1` and
  `b"  \n"` reports `sloc 0`. This is upstream grammar behaviour and is
  now stated as an explicit carve-out rather than left as an unspoken
  exception. Measuring it across the tree (it had only been checked for
  Rust) found the split is 20 grammars collapsing and 5 — Elixir, Tcl,
  iRules, `preproc`, `ccomment` — keeping the span; both halves are
  pinned per language, so a grammar bump that moves a language across
  fails a test rather than silently changing a metric. No behaviour
  change. See
  [`developers/loc.md`](big-code-analysis-book/src/developers/loc.md).
- `bca` no longer exits `0` when an input file cannot be read (#1098).
  #1060 fixed this for `check` only; `metrics`, `ops`, `report`,
  `functions`, `find`, `count`, `dump`, `exemptions`, `preproc`,
  `strip-comments`, and `diff --since` all exited `0` after printing
  `error processing <path>: …` to stderr. The guard now lives in the
  shared walk layer, so any read failure is a tool error (exit `1`) for
  every walking subcommand. `diff --since` reports which side failed,
  and a partial aggregate, report, tally, or preproc document is no
  longer emitted — output already streamed during the walk is kept.
  **This can turn a previously-green CI job red**: a run that tolerated
  unreadable files now fails. Use `--exclude` to skip them deliberately.
- Every walking subcommand exits `1` when an output document could not
  be written — an unwritable `--output-dir`, a full disk — mirroring the
  unreadable-input contract above (#1115). Previously the per-file error
  went to stderr and the process reported success, leaving a truncated
  document behind. A broken pipe still exits `0`.
- `bca dump` and `bca find` no longer interleave one file's `== path ==`
  banner with another worker's tree under `--jobs N` (#1115). The stdout
  lock is held across banner and tree.
- `bca check`'s unreadable-input summary reads `error: N input files
  could not be read …` rather than `error: bca: N input files could not
  be read …`; the `bca:` prefix was duplicated by `error:` (#1098).
- `bca check` no longer exits `0` when input files could not be read
  (#1060). The counter backing the "no input files matched" guard was
  bumped before the read was attempted, so a tree whose every file was
  unreadable (permission denied, a broken symlink, a container
  volume-mount mismatch) reported `error processing <path>: …` on
  stderr and then exited `0` — the worst failure mode a CI gate has.
  The counter now moves only for files that were actually read, and
  read failures are tallied separately: any input file that failed to
  read exits `1` (tool error, distinct from `2` = gate breach) with a
  summary line, because a partially analysed gate is not a passing
  gate. Like the pre-existing empty-input guard, the check runs before
  the gate is evaluated and is not suppressed by `--no-fail`, which
  suppresses threshold failures rather than broken input. `bca init`
  scaffolds its baseline through the same walk, so it inherits the
  guard and refuses to pin a baseline that would silently under-record
  the debt in a file it could not read. Both guards' messages are now
  prefixed `bca:` rather than `bca check:`, which misattributed an
  `init` failure to a subcommand the user never ran. Other subcommands
  are unchanged for now; extending the same contract past `check` is
  tracked separately.

  `read_file_with_eol` is fixed on the same issue: its "≤ 3 bytes is
  not worth parsing" shortcut returned `Ok(None)` from a bare `stat`,
  and `stat` succeeds on a file the process cannot open — so a tiny
  unreadable file was indistinguishable from an empty one and the
  permission error the function documents never surfaced. `bca check`
  on a 3-byte unreadable file therefore exited 0 with no diagnostic at
  all, not even the per-file `error processing` line. The shortcut now
  confirms readability by opening the file first; the open is skipped
  for anything that is not a regular file, so the function still never
  blocks on a FIFO. Behaviour is unchanged for readable files of any
  size.

- `Ops::operators` and `Ops::operands` are now sorted in
  byte-lexicographic order, so `bca ops` produces identical bytes for
  identical input (#1091). Both vectors were collected from `HashMap`
  keys, and `RandomState` reseeds per map instance, so the listings
  were reordered on every run — and even between two parses within one
  process. That made `bca ops` output impossible to diff between runs,
  check into a repository, or use as a cache key, in the tree renderer
  and in every serialized format alike. The fields were documented as
  "arbitrary order", so pinning them down is additive for callers; no
  metric value moves, since Halstead's `n1` / `n2` are set
  cardinalities. Sorting costs `O(n log n)` per space over the space's
  vocabulary and is paid only on the `ops` seam — the metric walk does
  not run it.

- The `dump` AST walk no longer rebuilds its indentation prefix per
  node, and no longer resolves a node's parent per node (#1054). Each
  queued node carried an owned copy of its ancestors' box-drawing
  prefix — a string that grows ~3 bytes per nesting level — so a
  wide-and-deep tree held O(depth²) resident bytes and copied O(depth)
  per node; separately, the flush-left check called
  `tree_sitter::Node::parent`, which resolves by descending from the
  root, once per node. The walk now keeps one shared prefix buffer that
  is extended on descent and truncated on the next visit, and carries
  each node's connector glyph on the work stack so only the node the
  walk starts from needs a parent lookup. On the issue's fixture
  (`int main(){return ((((…1…))));}`) `bca dump` goes from 1.18 s to
  0.05 s at nesting depth 4000 with peak RSS falling from 66 MB to
  13 MB; on a wide-and-deep JavaScript fixture (1500 nested functions,
  two siblings each) it goes from 4.00 s to 0.05 s. The rendered text
  is byte-identical — verified across 300 files spanning the corpus
  submodules — and the *emitted* size stays O(nodes × depth), which is
  inherent to a tree drawing where every line carries its own
  indentation. The mirrored `metrics` and `ops` text dumps
  (`dump_metrics`, `dump_ops`) carried the same per-entry owned prefix
  and got the same shared-buffer treatment; their measured cost is
  unchanged on the fixtures tried — a nested-closure chain keeps only
  one stack entry alive at a time, so the quadratic term needs a tree
  that is wide *and* deep — but the O(depth) copy per rendered line is
  gone and the three walks no longer differ in shape.

- `loc.sloc` no longer drops the final line of source that is not
  newline-terminated (#1067). `Sloc` derived its row count from an
  "is this the unit span?" flag; the unit branch was correct only
  because a trailing newline pushes tree-sitter's root node onto a
  phantom extra row, so a one-line unterminated file reported
  `sloc == 0`, `mi.original` / `mi.sei` / `mi.visual_studio`
  short-circuited to `0.0` through `mi::inputs_are_empty`, and
  `cloc + ploc > sloc` for input such as `b"fn f(){}\n/// x"`. The row
  count now comes from the span's end column, which is correct in both
  directions.
  **Metric drift, all languages:** callers passing bytes with no
  trailing newline now see `sloc` (and `blank`, `sloc_max`, the
  `*_average` values, and all three MI formulas) increase by one
  line's worth; whitespace-only unterminated files move from `sloc 0`
  to `sloc 1` / `blank 1`. This reaches every entry point that does
  not normalise its input: the Rust `Source` / `Ast::parse` API and
  the Python `Ast.parse(code, language)` staticmethod, which passes
  its bytes through verbatim. Entry points that read through
  `read_file_with_eol` / `normalize_eol` — the CLI, the web server's
  metrics endpoints, `analyze()`, and `Ast.from_path` — append a
  trailing newline and are unaffected on this axis.
  **Metric drift, per-function spans:** the same rule corrects the
  opposite error wherever a grammar ends a func-space node at column 0
  of a row it does not occupy — a span that used to be credited one
  row too many. `tree-sitter-perl` does this to the last `sub` of a
  file, whose `function_definition` swallows the newline after the
  closing brace; that sub's `sloc` could exceed the whole file's, and
  now drops by one along with the file's `sloc_max` / `blank_max`
  where it was the maximum. `tree-sitter-bash` does the same to some
  `function_definition`s (one file in the in-tree corpus:
  `parse_valgrind_suppressions.sh`, whose function drops from `sloc 9`
  to `8` and moves `mi.sei` from 37.4 to 49.1). This drift does reach
  the CLI, web server, and Python bindings.
- `wire::FuncSpace::from` and `wire::Ops::from` no longer recurse
  (#1056). Both projected a nested tree with
  `spaces.iter().map(Self::from).collect()`, one stack frame per nesting
  level at roughly 2.3 KB each, which aborted the process at ~900 levels
  on a default 2 MiB thread. They now walk an explicit work stack and
  convert a 1 000 000-level chain on a 512 KiB thread.
- Serializing a `FuncSpace`, `Ops`, or `AstNode` deeper than its limit
  now fails with an ordinary serializer error naming the type and the
  limit, instead of overflowing the stack (#1056). `serde` cannot emit a
  tree without one native frame per level — `serialize_field` must run
  the child's `Serialize` to completion before returning — so the depth
  is bounded rather than de-recursed, mirroring the 128-level recursion
  limit `serde_json`'s `Deserializer` already applies to the same
  documents.
- The `cognitive` metric's nesting lookup is no longer quadratic in
  nesting depth (#1062). It recovered each node's inherited nesting via
  `node.parent()`, which is `O(depth)` — tree-sitter stores no parent
  pointer — making the lookup `O(nodes × depth)`. The walker now hands
  each node its inherited nesting directly, so the lookup is `O(1)`.
  Cognitive values are unchanged. On shapes that exercise only this path,
  whole-file analysis is now linear: nested parentheses at depths
  8000 / 16000 / 32000 / 64000 take 13 / 23 / 45 / 88 ms, so a 128 KB
  file completes in under a tenth of a second.

  The walker also no longer pre-seeds that map with the root. Nothing
  read the seed — the lookup already falls back to a default — but it was
  the map's only entry whenever `cognitive` is deselected, so a
  metric-subset run (`--metrics loc`, `bca check` with a threshold
  subset) allocated a hash table per file for one unread entry. The two
  grammars whose cognitive impl is a no-op, `preproc` and `ccomment`, now
  build no map at all rather than one entry per AST node.

  `cognitive`'s remaining `Node::parent` sites are gone with it.
  `increment_function_depth` asked every function node whether a
  function encloses it by climbing with `node.parent()`, which kept the
  metric `O(depth²)` on nested definitions across its 19 call sites (22
  languages, counting the four the JS-family macro expands to). It now
  reads the ancestor chain the walker hands down (the #1084
  mechanism, deferred out of that change), and a new
  `cognitive/nested-fn` depth-scaling probe covers it: `time ~ depth^k`
  fits 2.04 against the climb and 1.21 against the chain, and at depth
  4000 the walk drops from ~150 ms to ~16 ms. The two remaining
  per-node climbs inside the metric — Kotlin's `when`-default check and
  Ruby's `case`-default check, one per `else` node — read the same
  chain now. Cognitive values are unchanged; the arithmetic is pinned at
  depth 1000 by `cognitive_function_depth_is_inherited_at_depth` and
  across languages by
  `function_depth_surcharge_holds_across_languages`.

  `Node::parent` climbs remain elsewhere in the crate — the five
  `Ancestors::unknown()` call sites, the Halstead `get_op_type`
  getters, and per-node lookups in several `loc`, `npa` / `npm`, and
  `checker` arms — all outside `cognitive` and outside the probed
  walks, and tracked in #1088. Operators analysing untrusted input
  should still bound request concurrency and input size.

- Deeply nested source no longer costs quadratic time in the `tokens`
  metric (#1052). `Tokens` decided whether a leaf sat inside a comment by
  walking that leaf's ancestor chain — and `Node::parent` is itself
  `O(depth)`, so the metric ran in `O(leaves × depth²)`. A 2 KB file of
  nested parentheses took ~19 s and a 4 KB one over two minutes, with
  parsing itself staying flat, which made it an unauthenticated CPU
  exhaustion vector against `bca-web` and a way to stall `bca check` in
  CI. The walker now propagates comment membership down the traversal in
  `O(1)` per node, so `tokens` is linear: measured at nesting depths
  1000 / 2000 / 4000, the metric now costs 4 / 5 / 6 ms, and the same
  files analyse end-to-end in 74 ms / 285 ms / 1.1 s. Token counts are
  unchanged — comment-internal leaves (Rust doc-comment markers and
  content) are still excluded, now by an inherited flag rather than a
  rediscovered one.

  Nesting-heavy input is **faster but still superlinear overall**, so
  untrusted deeply-nested source is not yet safe to analyse unbounded.
  `Node::parent` is `O(depth)` and is used per-node in several other
  places: `cognitive`'s nesting-map lookup dominates the nested-paren
  shape measured above, while `Loc`'s `count_specific_ancestors`
  (C-family, Java, C#, Go, Groovy, Objective-C) and Elixir's
  `is_inside_quote_block` are superlinear on other shapes — the latter
  behind no metric selection, so it cannot be deselected. Tracked in
  #1062.
- A Rust doc comment ending at EOF without a trailing newline no longer
  crashes or miscounts (#1051). `Loc` discounts the row that a
  `DocComment`'s scanner consumes along with its newline, but at EOF
  there is no newline left to consume, so the node ends on its own start
  row and the row was discounted anyway. The symptom split by where the
  comment sat:
  - **On the first row** (`/// x` as the whole file): the analyzer
    panicked — in debug at the subtraction, in release as a hash-table
    capacity overflow while inserting comment rows.
  - **On any later row** (`fn f() {}\n/// x`): release builds did **not**
    crash. They silently reported `cloc` one too low, which also shifted
    `blank` and the Maintainability Index's comment percentage.

  **This changes metric values.** Any Rust file whose last line is a doc
  comment with no trailing newline now reports one more `cloc` (and one
  fewer `blank`) than a `2.0.0` release build did; two such comments
  shift by two. Re-check `.bca-baseline.toml` entries and thresholds for
  affected files.

  Reachable from `analyze` / `Source` (the documented library entry
  point) and from the Python `Ast.parse(...).metrics()` fast path, which
  — unlike `analyze_source` and `Ast.from_path` — does not normalize
  line endings. The `bca` CLI and every `bca-web` endpoint that computes
  metrics normalize their input and were unaffected.
- docs.rs now publishes the **complete** API reference. The published
  build previously used default features only, silently dropping the
  entire feature-gated `vcs` module (change-history metrics, #328) from
  the reference. A `[package.metadata.docs.rs]` section
  (`all-features = true`, `--cfg docsrs`) restores it and enables
  per-item "Available on crate feature …" badges via `doc(cfg)`. A new
  `make doc-check-docsrs` target reproduces the docs.rs build locally on
  nightly (`--cfg docsrs`), so the published rendering can be verified
  before a release rather than discovered broken after publish.

### Security

- Closed a remotely-triggerable process abort in the recursive
  `Serialize` and `Drop` paths (#1056). `bca metrics -O json` on ~1 000
  nested functions (11 KB of source) overflowed the thread stack, and a
  stack overflow is a `SIGABRT`, not a catchable panic: `bca-web`'s
  `spawn_blocking` wrapper turns a panic into one failed request, but an
  abort takes the whole process down with every request in flight. Three
  recursions were involved, all now bounded — see the **Fixed** and
  **Changed** entries below. Reachable payloads were small: ~11 KB of
  nested `fn`s for the serialization overflow, ~80 KB of nested
  parentheses for the `/ast` one, both far inside the 4 MiB body cap.

  Issues #700 / #709 had converted every AST *traversal* to an explicit
  work stack; this was the same hazard in the recursive *types*, which
  those tests did not reach.

- Narrowed a remotely-triggerable CPU-exhaustion vector: a few kilobytes
  of deeply nested source could pin a core for minutes against the
  unauthenticated `bca-web` endpoints, whose parse deadline frees the
  client but cannot cancel the blocking task. Two of the quadratic paths
  are gone — the `tokens` ancestor walk (#1052) and every one of
  `cognitive`'s parent lookups (#1062) — as are the three `Node::parent`
  predicates the benchmark harness measured as quadratic (#1084); see
  those entries under **Fixed**. Every walk the depth-scaling gate
  probes now fits an exponent near 1.0.

  **This is not closed.** The climbs tracked in #1088 are unprobed and
  still resolve a parent by descending from the root, among them the
  JS/TS `is_func` / `is_closure` walk, Elixir's `Npa` / `Npm` /
  `get_func_space_name` / suppression-marker lookups, the Halstead
  `get_op_type` getters, and per-node `Node::parent` calls in several
  `loc` and `checker` arms. Operators analysing untrusted input must
  still bound request concurrency and input size.
- Cleared the two RUSTSEC advisories behind the OpenSSF Scorecard
  Vulnerabilities alert: `anyhow` `1.0.102` → `1.0.103` (unsound
  `Error::downcast_mut()`, RUSTSEC-2026-0190) and `memmap2` `0.9.10`
  → `0.9.11` (unchecked pointer offset in the `advise_range` /
  `flush_range` family, RUSTSEC-2026-0186). Both are transitive
  dependencies (via `wit-parser` and `gix` respectively); neither
  affected API is called directly by this workspace.
- Removed `test_ext`, a 4.3 MB compiled debug binary accidentally
  committed at the repository root (flagged by the OpenSSF Scorecard
  Binary-Artifacts check). Nothing referenced it.
- CI Python tooling is now hash-pinned (OpenSSF Scorecard
  Pinned-Dependencies). Workflows install from
  `big-code-analysis-py/requirements/{dev,examples}.txt` — exports of
  `uv.lock` regenerated by `make py-relock` — with `pip install
  --require-hashes`, replacing the floor-range `pip install`s; this
  also closes the long-tracked "CI does not consume `uv.lock`" gap.
  The wheel smoke jobs install the just-built wheel by explicit
  `dist/*.whl` path with `--no-deps` (the build jobs already verify
  exactly one wheel per artifact), and the unpinned
  `pip install --upgrade pip` steps are gone. `pytest-cov` joins the
  `dev` extra and `maturin` the `examples` extra so the exports cover
  exactly what each CI job needs.

- The grammar-regeneration scripts now install npm dependencies
  hash-verified (OpenSSF Scorecard Pinned-Dependencies, code-scanning
  alerts #759–#761, issue #1012). The four internal grammar crates
  (`tree-sitter-ccomment`, `tree-sitter-preproc`, `tree-sitter-mozcpp`,
  `tree-sitter-mozjs`) now commit their `package-lock.json` (previously
  gitignored — the lockfiles were never actually in git) and
  `generate-grammars/generate-grammar.sh` installs with
  `npm ci --include=dev`, which fails loudly on a missing or drifted
  lockfile. `generate-mozcpp.sh` uses `npm ci` inside the upstream
  `tree-sitter-cpp` checkout (upstream commits a lockfile at the pinned
  revision) and replaces the `npm install --no-save tree-sitter-c@0.23.1`
  override with a registry tarball fetched by exact version and verified
  against a recorded sha512 before extraction — no npm version
  resolution at all, and the package's install scripts are never run.
  `generate-mozjs.sh` gains the `set -euo pipefail` fail-loud guard its
  mozcpp sibling already had, so an aborted regen can no longer fall
  through to cleanup and report success. Both regens were verified
  byte-reproducible from a clean checkout.

- Cleared three RUSTSEC advisories flagged by the `cargo-deny` gate.
  `crossbeam-epoch` (a shipped transitive dependency via `crossbeam`)
  moves `0.9.18` → `0.9.20` for the invalid-pointer-dereference in its
  `fmt::Pointer` impl (RUSTSEC-2026-0204). The dev-only `quick-xml`
  test dependency moves `0.39` → `0.41` for the quadratic
  duplicate-attribute check (RUSTSEC-2026-0194) and the unbounded
  namespace-declaration allocation in `NsReader` (RUSTSEC-2026-0195);
  the XML-validation test helpers migrate from the now-deprecated
  `Attribute::unescape_value()` to `normalized_value(XmlVersion::Implicit1_0)`,
  which is its exact behavioral equivalent.

## [2.0.0] - 2026-06-29

The project's first major version bump since `1.0`, and the first
stable release on the `2.x` line. It collects every breaking change
staged across the `1.x` cycle behind a single major boundary. The
detailed **(breaking)** entries are listed under the headings below;
the migration notes here summarise the surface and consolidate the
serialized key map and the metric-value re-baseline that the
contract promised the `2.0.0` entry would carry. (The `2.0.0-rc1`
release candidate, tagged 2026-06-19, carried the same change set
ahead of this stable cut.)

### Migration from 1.x

The breaking changes fall into a few groups, each detailed under its
own **(breaking)** entry below:

- **Library `Stats` accessors and metric naming** — Halstead,
  `NArgs`, and MI accessors were renamed to a uniform wire
  vocabulary, the `exit` metric module was renamed to `nexits`, and
  the `Metric::NArgs` / `Metric::Exit` variants became
  `Metric::Nargs` / `Metric::Nexits`.
- **Serialized output shape** — metric keys were normalised
  (#510, #511), integer-valued metrics now serialize as integers and
  their accessors return `u64` (#530), non-finite floats serialize as
  a uniform `null` (#531), and several wire keys were renamed (see the
  key map below).
- **CLI grammar** — flags renamed (`--language-type` → `--language`,
  `--num-jobs` → `--jobs`, `--warning` → `--warnings`), exit codes
  restructured (argv errors exit 1; 2–5 reserved for metric gates),
  and several argument-parsing behaviours tightened.
- **REST schema** — uniform `{error, error_kind, id}` error and
  `{id, language}` analysis envelopes, stricter unknown-field
  rejection, a nested per-file `vcs` object, and removal of the
  unprefixed route aliases.
- **Default grammars** — `.js` / `.jsx` now parse through upstream
  `tree-sitter-javascript` (the Mozilla fork is demoted to the opt-in
  `mozjs`, owning only `.jsm`), `.cpp` / `.h` through upstream
  `tree-sitter-cpp` (Mozilla fork demoted to opt-in `mozcpp`), `.c`
  through a new `LANG::C`, and `.m` through a new `LANG::Objc`.
- **Python bindings** — the typed surface tightened and
  `analyze_batch`'s `skip_generated` default flipped from `False` to
  `True`.

#### Metric-value re-baseline

`2.0.0` is a one-time metric-value re-baseline boundary. Values
shifted across the `1.x` cycle from metric-definition fixes
(divide-by-zero guards across the suite, the per-function
`cyclomatic` average) and, at `2.0`, from the default-grammar flips:
`.c` files now parse through `tree-sitter-c`, `.m` through
`tree-sitter-objc`, and the Mozilla C++ overlay was swapped for
upstream `tree-sitter-cpp` — each moves the affected files' numbers.
The integration snapshots were re-baselined in lockstep. Consumers
comparing across the `1.x` → `2.0` boundary should treat it as a
single re-baseline rather than reconciling the union of every
patch-level drift; pin an exact version and store it alongside your
results if you need bit-for-bit reproducibility.

#### Serialized key & accessor renames

Library `Stats` accessor renames — **serialized output keys are
unchanged** (these are Rust method names only):

| Metric            | Old accessor                          | New accessor                                    |
| ----------------- | ------------------------------------- | ----------------------------------------------- |
| Halstead          | `u_operators`                         | `unique_operators`                              |
| Halstead          | `operators`                           | `total_operators`                               |
| Halstead          | `u_operands`                          | `unique_operands`                               |
| Halstead          | `operands`                            | `total_operands`                                |
| NArgs             | `fn_args` (+ `_sum`/`_average`/`_min`/`_max`) | `function_args` (+ `_sum`/`_average`/`_min`/`_max`) |
| NArgs             | `nargs_total` / `nargs_average`       | `total` / `average`                             |
| MI                | `mi_original` / `mi_sei` / `mi_visual_studio` | `original` / `sei` / `visual_studio`    |
| Nexits (was `exit`) | `exit` (+ `_sum`/`_average`/`_min`/`_max`)  | `nexits` (+ `_sum`/`_average`/`_min`/`_max`) |

Module / variant renames: the `exit` metric module became `nexits`
(`crate::exit` → `crate::nexits`), `Metric::Exit` → `Metric::Nexits`,
`Metric::NArgs` → `Metric::Nargs`. The retired `"exit"` metric parse
alias no longer resolves — only `"nexits"` parses.

Serialized **wire-key** renames (JSON / YAML / TOML / CBOR, and the
matching CSV columns):

| Block    | Old key                                      | New key                                  |
| -------- | -------------------------------------------- | ---------------------------------------- |
| `npm`    | `classes` / `interfaces`                     | `class_npm_sum` / `interface_npm_sum`    |
| `npa`    | `classes` / `interfaces`                     | `class_npa_sum` / `interface_npa_sum`    |
| `wmc`    | `classes` / `interfaces`                     | `class_wmc_sum` / `interface_wmc_sum`    |
| `tokens` | `tokens_average` / `tokens_min` / `tokens_max` | `average` / `min` / `max`              |

The bare-sum `tokens` leaf is kept, and the terminal dump's
tokens-sum label changed `sum` → `tokens`. The truthful sibling keys
on `npm` / `npa` / `wmc` (`class_methods`, `total`, `coa`, `cda`, …)
are unchanged.

Type / shape: integer-valued metrics (every count, sum, and min/max,
plus Halstead `length` / `vocabulary` and all WMC values) now
serialize as integers and their `Stats` accessors return `u64`
instead of `f64`; ratios, averages, ABC `magnitude`, the derived
Halstead scores, and MI stay `f64`. No value changes — only the type.

### Added

- Python `Node` is now a closer py-tree-sitter drop-in: a `type` property
  aliases `kind` (the py-tree-sitter spelling — `kind` stays the canonical
  bca name), and `text` is now a property rather than a method, matching
  py-tree-sitter's `node.text`. Together these erase the two most-common
  mechanical edits when porting a py-tree-sitter walker. The `text`
  method→property shape change is part of the still-unreleased #728 surface,
  so it lands free before 2.0. Covered by the `make py-stubtest` gate (#975).
- Public `metric_catalog::MetricScope` enum (`File` / `Function` /
  `Container`) with `MetricScope::admits(SpaceKind)`, a
  `metric_catalog::scope(id)` lookup, a `scope` field on the
  `#[non_exhaustive]` `MetricInfo`, and `SpaceKind::from_serialized` —
  the single source of truth for which space kind each threshold metric
  gates (#969), shared by the CLI gate and the Python `to_sarif` binding
  so they cannot drift.
- Lazy `Node` traversal handle for Python (`big_code_analysis.Node`) over
  the tree retained by `Ast`, so a caller walks the AST py-tree-sitter-style
  — `kind` (with the py-tree-sitter-compatible `type` alias), byte offsets,
  points, `children`, `child_by_field_name`, the `text` property, a lazy
  pre-order `walk()`, `descendants_by_kind()` — **without**
  materialising the tree into dicts the way `dump()` does (#728). Reach one
  through the new `Ast.root_node` property or `Ast.find(filters)`; the node
  keeps its `Ast` alive, so it stays valid after every other reference to
  the parse is dropped, and is safe to share across a `ThreadPoolExecutor`.
  Kinds are the **raw** grammar kinds (not the `Alterator`-curated kinds
  `dump()` emits — they intentionally disagree on altered nodes such as
  string literals), and each node exposes its location in every vocabulary:
  `start_byte`/`end_byte`, 0-based `start_point`/`end_point` (py-tree-sitter
  parity), and 1-based `start_line`/`end_line` plus a `span` dict matching
  `dump()`. Covered by the `make py-stubtest` gate.
- `Node::preorder()` (a pre-order iterator) and
  `Node::descendants_by_kind(kinds)` on the Rust `Node` surface — the
  Rust counterparts the Python `walk()` / `descendants_by_kind()` mirror,
  so Rust callers gain the same ergonomic traversal helpers (#728).
- Python `Ast` parse-once handle (`big_code_analysis.Ast`) binds the Rust
  `Ast` seam, so a Python caller parses a source **once** and draws both
  metrics and the AST from the same parse instead of parsing twice — once
  in py-tree-sitter, once in `analyze()` (#727). `Ast.parse(code,
  language)` and `Ast.from_path(path)` construct the handle; `.metrics()`
  (byte-for-byte `analyze_source`), `.dump()` (the `bca dump` / `/ast` node
  tree), `.functions()`, `.ops()`, `.count()`, `.strip_comments()`, and
  `.suppressions()` all reuse the one parse. `from_path` is *no-magic*: it
  reads through the same text reader as `analyze` (so metrics match) but
  does not skip generated files and never silently returns nothing. New
  `AstNodeDict` / `SpanDict` / `FunctionSpanDict` / `OpsDict` /
  `SuppressionMarkerDict` TypedDicts; all covered by the `make py-stubtest`
  gate.
- `Ast::from_path` on the Rust surface (the file-backed counterpart to
  `Ast::parse`): reads + language-detects + parses one file, returning a
  new `FromPathError` for each distinct failure (I/O, non-UTF-8 path,
  empty/binary/non-text file, unknown language, disabled-language build)
  (#727).
- `language_grammar_version(language)` (Python) and `LANG::grammar_version`
  (Rust) return the pinned tree-sitter grammar crate version backing a
  language (e.g. `"0.25.1"` for `bash`) — the exact upstream version for
  crates.io grammars, the fork crate version for the vendored forks
  (#727).
- Byte offsets in the AST dump span: every dump node's `span` now carries
  `start_byte` / `end_byte` (0-based, half-open) alongside the existing
  1-based line/column pairs, across the library, CLI `dump`, web `/ast`,
  and the Python `dump()` (#727). Structural consumers can slice the
  original source for any node — including internal nodes whose `value`
  the dump omits — without re-deriving offsets from lines and columns.
  (See the **(breaking)** `Span` note under Changed for the Rust
  struct-shape impact.)
- `MetricSet::resolved()` returns the set closed under
  `Metric::dependencies` (idempotent), the set-in/set-out counterpart of
  `from_slice_with_deps` (#743).
- `defang_formula` is now public (re-exported from the crate root) so the
  CLI's VCS-report CSV writer can share the lib's CWE-1236 spreadsheet
  formula-injection mitigation rather than duplicating it (#794).
- `AuthorId::has_identity()` reports whether a VCS author carries any
  usable name or email key (#817).

- Per-space *own* value for the four subtree-aggregate metrics in the
  serialized wire shape: `cyclomatic.value`, `cyclomatic.modified.value`,
  `cognitive.value`, and `abc.value` (#958). Each space already carried
  its subtree aggregate (`sum` / `magnitude`); the new `value` field adds
  the per-space scalar — the value the CLI thresholds against, excluding
  nested function/closure spaces. SemVer-additive: it appears in every
  output format (JSON / YAML / TOML / CBOR) and in the Python
  `CyclomaticDict` / `CyclomaticModifiedDict` / `CognitiveDict` /
  `AbcDict` TypedDicts.

- Opt-in keyed author-identity hashing for `--emit-author-details`
  (#956). A secret key — `--author-hash-key <KEY>` (or the
  `BCA_AUTHOR_HASH_KEY` environment variable, preferred so the secret
  stays off the process list), the REST `author_hash_key` field, and the
  Python `vcs.Options(author_hash_key=…)` — hardens the emitted author
  digests into an `HMAC-SHA256(key, SHA-256(email))`, defeating the
  email-enumeration and precomputed-table attacks a bare SHA-256
  pseudonym is vulnerable to (the Gravatar weakness; see #811). The key
  hardens only the *emitted* digests (it requires `--emit-author-details`)
  and is applied at finalization, so default output is unchanged and the
  persistent-cache replay invariant (#334) holds: the cache stores the
  unkeyed inner digest and a cached walk re-finalizes under any key
  without a re-walk. New library surface: `vcs::AuthorHashKey`, the
  additive `Options::author_hash_key` field, and `AuthorId::emit_hashed`.

- `LANG::Objc` (slug `objc`) and the `objc` Cargo feature: dedicated
  Objective-C support backed by upstream `tree-sitter-objc` `=3.0.2`,
  owning the `.m` extension and the `objc` / `objective-c` emacs modes
  (all moved off `LANG::Cpp`). Objective-C is a strict superset of C, so
  `.m` files now parse correctly instead of ERROR-cascading every
  `@interface` / `@implementation` / message send through the C++
  grammar. Objective-C++ (`.mm`) deliberately stays on `LANG::Cpp`: the
  Objective-C grammar cannot parse the C++ half of a `.mm` file, and C++
  is the larger surface, so the C++ grammar degrades more gracefully
  there — the same asymmetric trade-off `.h` uses (a known limitation;
  metrics for the Objective-C portions of `.mm` files are approximate).
  Real impls ship for all metrics: cyclomatic, cognitive, exit,
  Halstead, LoC, nom, and nargs (#724), plus `abc` (message sends count
  as calls; `@try` / `@catch` as conditions) and the OO metrics `npa`
  (`@property` and `@public` instance variables), `npm` (`@interface` /
  `@protocol` declarations and `@implementation` definitions), and `wmc`
  (per-method cyclomatic rolled into the `@implementation` class) (#737).
  The retired internal `fake::get_true` Objective-C slug overlay (#540)
  is gone — `.m` reports `"objc"` and `.mm` reports `"cpp"` natively.
  `all-languages` now includes `objc` (#724, part of #718).

- New book recipe, *Feeding metrics to an agentic coding tool*
  (`recipes/agent-feedback.md`): wires the existing `bca check` surface
  into an agent's after-edit feedback loop with copy-pasteable sections
  for Claude Code (`PostToolUse` hook with stderr/`additionalContext`
  injection) and opencode (a `tool.execute.after` plugin that throws to
  signal). Ships a verbatim anti-gaming guidance block, the exact
  in-source suppression syntax (canonical `nexits`, never `exit`), and
  the task-boundary-vs-per-edit and Goodhart caveats. Documentation
  only — no binary changes; contrasts itself with the proposed
  `bca lsp` (#384) (#733).

- `LANG::C` (slug `c`) and the `c` Cargo feature: a dedicated C
  language backed by upstream `tree-sitter-c` `=0.24.2`, owning the
  `.c` extension and the `c` emacs mode (both moved off `LANG::Cpp`).
  `.h` deliberately stays on `Cpp` — a C++ header through the C grammar
  ERROR-cascades, while a C header through the C++ grammar only trips on
  C++-keyword identifiers. C code that uses C++ keywords (`new`,
  `class`, `delete`, `template`) as identifiers now parses cleanly
  instead of ERROR-cascading through the C++ grammar. C has no classes,
  so `npm` / `npa` / `wmc` are no-ops; the `.c` re-routing shifts metric
  values on C files (integration snapshots re-baselined). `all-languages`
  now includes `c` (#721, part of #718).

- `LANG::Mozcpp` (slug `mozcpp`) and the opt-in `mozcpp` Cargo feature:
  the Mozilla/Gecko C++ dialect, backed by the vendored
  `bca-tree-sitter-mozcpp` fork (upstream `tree-sitter-cpp` plus the
  `MOZ_*` / `QM_TRY_*` / alone-macro overlay). It owns no file
  extensions — select it explicitly with `--language mozcpp`, a
  manifest, or the API, mirroring `mozjs` for `.jsm` since #507. The
  `all-languages` feature now includes `mozcpp` (#720, part of #718).
  `Mozcpp` is a first-class C++ dialect everywhere `Cpp` is: it shares
  the preprocessor macro-replacement pass, the comment-stripping
  redirect, and the `bca-web` `GET /v1/languages` listing (where it
  appears with an empty `extensions` array, for parity with the Python
  `supported_languages()` surface).

- `exclude_tests` `bca.toml` manifest key: opt a project's `bca check` /
  `bca metrics` into Rust test-subtree pruning declaratively, mirroring
  the `--exclude-tests` flag (CLI wins; the presence-only flag means the
  key can only turn pruning on). Rust-only; purely additive — absent key
  preserves today's behaviour (#717).
- `metric_catalog::lower_is_worse(id)`: a `#[must_use]` helper answering
  whether a metric's unhealthy direction is downward (the `mi.*` family),
  single-sourcing the direction predicate the CLI threshold gate, the
  Code Climate severity inversion, and the Python SARIF binding all share
  (#698).
- `write_csv_aggregate` (re-exported from the crate root): writes several
  metric trees into one CSV document under a single shared header row.
  Backs `bca metrics/ops --output <FILE> --format csv` (#669), which
  previously repeated the header before every file's rows.
- A `mypy stubtest` gate (`make py-stubtest`) verifies the hand-written
  PyO3 type stub
  `big-code-analysis-py/python/big_code_analysis/_native.pyi` against the
  compiled extension — diffing names, signatures, and **defaults** — so a
  stub default can no longer silently drift from the
  `#[pyo3(signature = …)]` runtime the way #583 did (the usage-only
  `make py-typecheck` mypy/pyright passes cannot catch that class of
  drift). Wired into `make pre-commit` and `make ci` (chained after
  `py-test`, sharing its `maturin develop` build) and skipped with a clear
  "not found" message when the venv / maturin / stubtest are absent. A
  minimal, commented allowlist
  (`big-code-analysis-py/stubtest-allowlist.txt`) covers the deliberate
  facade differences (the `vcs` submodule, runtime `__all__`) (#673).
- The HTML report's table of contents now nests each language's h3 hotspot
  subsections under its h2 entry in a collapsible `<details>` list (#685),
  reusing the per-language-unique ids the report already mints, so a reader
  can jump straight to one hotspot table instead of landing at the top of a
  ten-screen section. A global cross-language Actionable Summary roll-up is
  rendered near the TOC so a multi-language report gives one top-of-page
  signal (#678).
- Legend entries and HTML column headers now link to the hosted metric
  reference (`metrics.html#<anchor>`) via one shared docs-base-URL constant
  and a per-metric anchor map (#675), so a one-line legend entry can hand
  the reader the full chapter. A test asserts every legend header maps to an
  anchor that actually exists in the book's `metrics.md`, so a renamed
  heading fails CI rather than shipping a dead link. The Markdown legend,
  HTML legend, HTML headers, and VCS legend all share the one constant.
- Added a provenance footer to the Markdown and HTML AST reports (#680):
  `Generated by bca <version> on <date> over <paths> — top <N> per table,
  suppression markers <honored|ignored>.` The date honors `SOURCE_DATE_EPOCH`
  for reproducible builds; the suppression line is load-bearing, since
  hotspot-table membership depends on it.
- The HTML report now carries a `<meta name="viewport">` tag and wraps every
  table in an `overflow-x:auto` scroll container (#686), so it renders at
  device width on mobile and a wide table (the 21-column VCS table) scrolls
  instead of clipping its right-most columns.
- **(Python)** `big_code_analysis.language_for_extension(ext)` — a
  filesystem-free extension → language lookup that accepts both `"py"` and
  `".py"` (case-insensitive), returns `None` for an unknown extension, and
  never reads a file or raises (#682). Paired with a new
  `language_for_file(path, *, read=False)` option that resolves by
  extension alone — answering for paths that do not exist yet (archive
  listings, git trees, candidate filtering) — so the README/example dance
  of inverting the per-language extension table by hand collapses to one
  call.
- **(Python)** `big_code_analysis.analyze_paths(*paths, include=None,
  exclude=None, …)` — a directory-walk entry point that reuses the CLI's
  gitignore-aware walker (include/exclude globs, generated-file filter,
  language inference) and returns the `analyze_batch` shape with the same
  never-raise semantics (per-file failures become `AnalysisFailure`
  elements) (#658). Each positional seed may be a file or a directory; it
  forwards the `analyze` kwargs (including `vcs` / `vcs_per_function`) so a
  data-science consumer can point it at a repository root instead of
  writing their own walker.
- **(Python)** `vcs` / `vcs_per_function` boolean kwargs on `analyze_batch`,
  mirroring single-file `analyze` (#670). The batch builds **one** shared
  history index / blame engine per containing repository — keyed by the
  discovered work-tree root (`vcs::workdir_root`), so files in different
  subdirectories of one checkout (`src/a.rs`, `tests/b.rs`) share a single
  index rather than rebuilding it per directory — and reuses it across that
  repo's files (amortising the walk the comprehension form repeats per
  file); a VCS failure leaves the AST metrics intact and never becomes an
  `AnalysisFailure`. Keeps the "migrating
  `[analyze(p) for p in paths]` to `analyze_batch(paths)` is
  behaviour-preserving" claim true even when the comprehension used `vcs=`.
- **(Python)** Typed `TypedDict`s for the change-history report shapes
  (#664): `VcsReportDict` (from `vcs.rank`), `VcsTrendDict` (from
  `vcs.trend`), `JitCommitReportDict` (from `vcs.commit`), and
  `JitDiffReportDict` (from `vcs.score_diff`) replace the former
  `dict[str, Any]` returns. The report / trend envelope structs are
  single-sourced in `big_code_analysis::wire` (the same drift-gated
  generator the analysis-result dicts use), so the Python types cannot
  diverge from the JSON the CLI emits.
- `big_code_analysis::vcs::workdir_root(path)` — discover the canonicalised
  work-tree root of the repository enclosing a file or directory, or `None`
  when it is outside any repository (or the repository is bare). Lets a
  front end coalesce a batch of files onto the repository each belongs to;
  the Python `analyze_batch(vcs=True)` cache uses it so files in different
  subdirectories of one checkout share a single history index (#670).
  Additive, `vcs-git`-gated.
- `bca metrics --metrics <name,…>` restricts computation to a subset of
  metrics via the public `MetricsOptions::with_only` (dependencies
  auto-resolved). Accepts comma-separated and/or repeated values using
  the same canonical ids as `check --threshold` / `diff --metric`
  (dotted and bare `loc` sub-metric spellings included); an unknown name
  errors (exit 1) with a did-you-mean. Default (flag absent) computes
  every metric (additive) (#691).
- `bca diff` / `bca diff-baseline` gain an opt-in `--exit-code` flag:
  exit with the metric-gate code (`2`) when the diff, after the active
  `--metric` / `--min-change` (or `--*-only` section) filtering, is
  non-empty; exit `0` when empty. Default behavior is unchanged (always
  `0` on success); a tool error still exits `1`. `git diff
  --exit-code`-style boolean for grammar-bump CI (#692).
- Metric legend in reports: Markdown gains a `### Legend` footnote and
  HTML a visible collapsible legend, with per-column definitions hoisted
  onto the shared column specs so the HTML tooltips and both legends
  draw from one source; also fixes the bus-factor "Files" / "Bus
  factor" HTML tooltips (#611, refs #610).
- HTML report navigation: slug-based heading anchors, a
  table-of-contents `<nav>`, and `aria-sort` initial-sort indication on
  each pre-ranked table column (#622).
- `--color auto|always|never` global CLI flag with tty detection and
  `NO_COLOR` support; piped text dumps (`metrics`/`ops` default tree,
  `dump`, `find`, `functions`) no longer emit ANSI escapes. Library
  gains the additive `ColorMode` enum and `dump_*_with_color` /
  `dump_function_spans_with_color` variants (#605).
- `vcs::Error::is_client_input()`: exhaustive client-input vs
  environment classification shared by the web 400/500 mapping and the
  Python exception taxonomy (additive) (#641).
- `GET /v1` route index endpoint generated from a single route table
  (the unprefixed `/` alias serves it with deprecation headers), and
  the REST book chapter now documents the full `/vcs` family (#643).
- Deprecation/Sunset/`Link: rel="successor-version"` headers on the
  unprefixed legacy web route aliases; removal of the aliases remains
  scheduled for the 2.0 cut (#637, refs #517).
- Python: typed VCS exception taxonomy — `VcsError(ValueError)` with
  `NotARepositoryError`, `InvalidRevisionError`, `InvalidDiffError`,
  and `VcsEnvironmentError`; existing `except ValueError` handlers
  keep working (#624).
- Python: generated `TypedDict` stubs for the analysis result shapes
  (`FuncSpaceDict` and nested metric dicts), rendered from the Rust
  wire shapes with a byte-compare drift gate; `analyze` /
  `analyze_source` / batch returns are now statically typed (#623).
- Python: VCS kwargs accept native types — `cache_dir` takes
  `os.PathLike`, `as_of` takes `datetime`, `file_types` takes a
  sequence of extensions — alongside the existing string forms (#619).

- Change-history (VCS) metrics: a new, language-agnostic metric family
  derived from git history rather than the AST (#328). A single history
  walk produces per-file signals over two windows (default 12mo / 90d) —
  distinct commits, line churn, distinct authors, top-author ownership
  share, burst, bug-fix / security-fix / revert commit counts, file age
  and last-modified days — combined into an ordinal, formula-versioned
  composite `risk_score` (`--risk-formula weighted|percentile`), plus a
  `hotspot_score` (complexity × recent churn) when AST metrics are
  computed alongside. Built on [`gix`](https://github.com/GitoxideLabs/gitoxide)
  behind the hierarchical `vcs = ["vcs-git"]` Cargo feature; the generic
  `vcs` module is backend-neutral so future backends (#335) reuse it.
  Surfaces:
  - Library: `big_code_analysis::vcs::{build_history_index, Options,
    Stats, HistoryIndex, …}`, `wire::Vcs`, and
    `CodeMetrics::vcs: Option<vcs::Stats>` (all behind `vcs-git`).
  - CLI: a new `bca vcs` subcommand. `--format` accepts a rendered
    report page (`markdown` / `html`, a self-contained sortable table
    styled like `bca report html`), the structured formats (`json` /
    `yaml` / `toml` / `cbor` / `csv`), or a default ranked table.
    `bca vcs --output <file>` writes a single whole-repo document (not
    the per-file directory `metrics` / `ops` emit). Plus `bca metrics
    --vcs` to attach a `vcs` block to each file's metrics, and `bca
    report markdown|html --vcs` to append a "Change-history risk"
    section to the aggregated quality report (#573). The Markdown and
    HTML renderers share one column spec so they cannot drift. The HTML
    report (both `bca vcs --format html` and the `bca report html --vcs`
    section) severity-heats the `risk_score` cell on a green→yellow→red
    gradient (#577); because `risk_score` is ordinal, the band is derived
    from each row's relative rank within the displayed set (five equal
    quantile bands), not from absolute thresholds, with WCAG-AA-contrast
    light and dark-mode palettes. Markdown output stays plain text.
    `bca vcs` errors clearly outside a git working tree; `--include` /
    `--exclude` / `--paths` are reused.
  - Web: a new `POST /vcs` endpoint taking a server-side `repo_path`.
  - Python: `vcs_metrics(repo_path, …)` and an opt-in `vcs=True` on
    `analyze()`.

  *Design note:* the issue proposed a `Metric::Vcs` enum variant +
  `--metrics vcs`; VCS is file-level, has no per-function threshold, and
  is not suppressible, so it is exposed via a dedicated `--vcs` flag
  rather than overloading the per-function `Metric` bitfield.
- Change-entropy and co-change graph-entropy VCS signals (#330). The
  single history walk now also emits four per-file fields:
  `change_entropy_long` / `change_entropy_recent` (Hassan 2009 History
  Complexity Metric — how scattered a file's changes are across commits;
  file-level Pearson 0.54 with defects on Apache projects) and
  `cochange_entropy_long` / `cochange_entropy_recent` (arXiv 2504.18511,
  2025 — how widely a file's changes ripple to co-changing partners,
  computed from a sparse co-change graph built during the walk). Both are
  Shannon entropies in bits; a `0.0` is *computed* (the file only ever
  changed alone), not "missing". Bulk-import commits wider than 1000 files
  are excluded from the co-change graph to bound its O(width²) growth.
  These fold into the composite score as a **`risk_score_version` bump to
  `2`** (the recent-window pair enters both the weighted and percentile
  formulas); the new formula is documented in `src/vcs/score.rs` and the
  mdBook VCS chapter. Because the serialized field set grew, the
  output-shape stamp **`vcs_schema_version` also bumps to `2`**. Surfaced
  on every VCS front end (library `Stats` / `wire::Vcs`, `bca vcs` CSV /
  Markdown / HTML, `POST /vcs`, and the Python bindings). Additive
  field-set change — no existing field moved.
- Per-function change-history metrics via `git blame` (#329). `bca
  metrics --vcs-per-function` (which implies `--vcs`) attaches a `vcs`
  block to every nested function / method / class space in addition to
  the file-level block, by blaming each file once and bucketing the
  surviving lines into the AST function spans. Each function's block
  reuses the same fields and ordinal `risk_score` as the file block,
  plus a per-function `hotspot_score`. The per-function numbers are a
  *current-blame snapshot* and deliberately differ in meaning from the
  file-level walk: `churn` counts surviving lines last touched in the
  window (not historical added+deleted), and ownership is by touching
  commit. Surfaces:
  - Library: `big_code_analysis::vcs::{PerFunctionBlame, LineSpan}` and a
    new `vcs::Error::Blame` variant (all behind `vcs-git`); nested
    `CodeMetrics::vcs` is now populated for function spaces, not only the
    file space.
  - CLI: `bca metrics --vcs-per-function`.
  - Enables the `blame` feature on the pinned `gix` dependency.

  *Design note:* the issue proposed a `--metrics vcs:per-function`
  sub-selector; consistent with the `--vcs` flag chosen for #328, this
  ships as a dedicated `--vcs-per-function` flag instead. Documented
  limitations cover renames, function splits, deletion+recreation, and a
  narrow `gix-blame` robustness bug on pathologically repetitive files
  (real source is unaffected; an unblameable file degrades gracefully to
  the file-level block only). Python `analyze()` / web parity for the
  per-function selector is tracked as a follow-up.
- Just-in-time (commit-level) VCS risk scoring (#331). `bca vcs jit
  <commit>` scores a single commit for defect-induction risk at
  check-in — the unit a CI gate reviews — rather than ranking files at
  HEAD. It is a static, rule-based scorer (no trained model, so nothing
  drifts as a project ages), with feature groups and signs taken from the
  just-in-time defect-prediction literature (Kamei et al., IEEE TSE 2013;
  open replications Commit Guru, FSE 2015 and McIntosh & Kamei, IEEE TSE
  2018): **size** (lines added/deleted, files, hunks), **diffusion**
  (subsystems, directories, within-commit change entropy), **history**
  (the touched files' priors — prior changes, distinct authors, bug- and
  security-fix counts, and the #328 composite `risk_score`, measured from
  history *before* the commit), **experience** (the author's prior commit
  count, which *lowers* the score — the one protective Kamei signal), and
  **purpose** (fix / security-fix / revert classification). The output is
  a stable JSON document with per-group feature contributions and an
  ordinal, formula-versioned composite `score`; `--fail-over <SCORE>`
  exits `2` (the `check` metric-gate convention) for CI use. Merge commits
  are scored against their first parent and flagged; root commits and new
  files carry zero priors by construction. Surfaces:
  - Library: `big_code_analysis::vcs::{score_commit, JitReport,
    JitFeatures, JitContributions, JitCommit, JIT_SCORE_VERSION,
    JIT_SCHEMA_VERSION, …}` (behind `vcs-git`); reuses the #328 history
    walk for the file priors and a separate cheap author-only walk for
    experience.
  - CLI: `bca vcs jit <commit> [-O json|yaml|toml|cbor] [--fail-over
    <SCORE>]`, reusing the parent `bca vcs` window / `--ref` / bot /
    merge / rename flags. The bare `bca vcs` ranking path is unchanged.

  *Scope note:* scoring an arbitrary `--diff <file>` (no commit, so no
  author / parent / file-history context — only size and diffusion would
  be computable) and web / Python parity are deferred to follow-ups;
  ML-based JIT and server-side hooks are out of scope per the issue.
- Directory- and repo-level **bus factor** (truck factor) VCS aggregate
  (#332). When a front end opts in, the single history walk now also
  emits a top-level `vcs_aggregate.bus_factor` object alongside the
  per-file `vcs` data: the minimum number of developers whose departure
  would orphan more than a configurable fraction (default `0.5`, per
  Avelino) of a directory's files. Authorship is scored with the Avelino
  *Degree-of-Authorship* heuristic (Avelino et al., ICPC 2016 — `3.293 +
  1.098·FA + 0.164·DL − 0.321·ln(1+AC)`, normalised, with the paper's
  `0.75` author threshold), and the truck factor is the greedy
  most-files-first removal. Reported for the whole repository (`repo`)
  and for each top-level directory and its immediate subdirectories
  (`by_directory`); under `--emit-author-details` each group also lists
  the SHA-256-hashed key developers in removal order. Files with no
  in-window authorship (and bot identities, already filtered) are
  excluded from the denominator. Surfaces:
  - Library: `big_code_analysis::vcs::{BusFactor, GroupBusFactor,
    DirectoryBusFactor, VcsAggregate, BUS_FACTOR_SCHEMA_VERSION}`, a new
    `HistoryIndex::bus_factor()` accessor + `with_bus_factor` builder,
    `Options::{compute_bus_factor, bus_factor_threshold}`, a
    `vcs::options::validate_bus_factor_threshold` helper, and a new
    `vcs::Error::InvalidBusFactorThreshold` variant (all behind
    `vcs-git`). The generic `bus_factor` module is backend-neutral.
  - CLI: `bca vcs` and `bca report --vcs` emit `vcs_aggregate` in every
    structured format and render it in the table / Markdown / HTML
    pages; `--bus-factor-threshold <F>` (in `(0, 1)`) tunes the coverage
    fraction.
  - Web: `POST /vcs` gains a `bus_factor_threshold` field and returns
    `vcs_aggregate`.
  - Python: `vcs_metrics(…, bus_factor_threshold=…)` returns
    `vcs_aggregate` in the result dict.

  Opt-in by design (`compute_bus_factor`, off by default): it retains
  per-file authorship beyond the per-file `Stats`, so the repeated
  JIT-prior and per-file-injection walks neither compute nor pay for it.
  Additive — no existing field moved, and `vcs_schema_version` is
  unchanged (the aggregate carries its own `BUS_FACTOR_SCHEMA_VERSION`).
- Historical metric **trend** (#333). Samples the change-history metrics
  at several points in time so a consumer sees whether a file's risk is
  improving or degrading over the project's life, not only its risk *now*
  — the actionable question for technical-debt programs (the Kamei JIT
  survey notes single-snapshot models lose predictive power as a project
  ages; a trend is the more durable framing). `points` evenly-spaced
  samples (inclusive of both endpoints) cover a `span`, ending at `as_of`
  (or wall-clock now); each sample **re-anchors at the mainline tip that
  existed at or before that moment** (resolved from one first-parent walk)
  rather than windowing today's `HEAD` tree, so it is a faithful
  historical snapshot — a file not yet born at an older point is `null`
  there. The output is a versioned (`trend_schema_version`) time series:
  `as_of_points` (oldest-first) plus a per-file array aligned to it, and a
  most-improved / most-regressed `deltas` summary by `risk_score`. The
  point count is bounded (2–120) to cap the per-point walks on deep
  histories. Surfaces:
  - Library: `big_code_analysis::vcs::{build_trend, Trend, TrendDelta,
    TrendDeltas, TREND_SCHEMA_VERSION}`, the `wire::{VcsTrend,
    VcsTrendPoint, VcsTrendDelta, VcsTrendDeltas}` projection, and a new
    `vcs::Error::InvalidTrend` variant (all behind `vcs-git`). The generic
    `trend` module is backend-neutral.
  - CLI: `bca vcs trend [--points N] [--span DURATION] [--top-deltas N]
    [-O json|yaml|cbor]`, reusing the parent `bca vcs` window / `--ref` /
    bot / merge / rename / `--as-of` / `--top` flags. (TOML is excluded —
    an absent point serializes as `null`, which TOML cannot represent.)
  - Web: a new `POST /vcs/trend` endpoint taking the `/vcs` fields plus
    `points` / `span` / `top_deltas`.
  - Python: `vcs_trend(repo_path, points=…, span=…, …)`.

  *Rename limitation:* renames are followed *within* each sample's walk,
  but a file renamed *between* two samples appears as two separate path
  series (old name, then new); cross-sample rename stitching is deferred.
- Persistent change-history **cache** keyed by `HEAD` SHA and repository
  identity (#334). Re-running a VCS analysis on an unchanged tree now
  replays a cached, pre-finalize event log instead of re-walking history;
  when `HEAD` has advanced it walks only the new commits and splices them
  onto the cached tail. The cache is a pure optimization — a hit is
  bit-identical to a fresh walk, and re-windowing tracks the current
  reference time rather than freezing at cache-write time. A force-push
  (the cached head is no longer an ancestor) falls back to a full walk;
  an entry is ignored when the cache format, `vcs_schema_version`,
  `risk_score_version`, or the walk-option fingerprint (windows, traversal
  mode, merge / rename / bot toggles, `--as-of`) differs, so a window
  change forces a fresh walk. Writes are atomic (temp file + rename), and
  a missing or corrupt entry is silently recomputed, never fatal. Author
  identities are stored only as their irreversible SHA-256 digests —
  never plaintext — so the cache is not a side channel for raw emails.
  Surfaces:
  - Library: `big_code_analysis::vcs::{build_history_index_cached,
    CacheConfig, CACHE_SCHEMA_VERSION}` and the `vcs::cache` module
    (behind `vcs-git`); `AuthorId::from_digest` reconstructs a hashed
    identity for replay. The generic event-log replay (`vcs::replay`) is
    now the single fold shared by the live walk and a cache hit, so the
    two cannot diverge.
  - CLI: `bca vcs --no-cache` / `--clear-cache` / `--cache-dir <DIR>`.
    The cache defaults to `$XDG_CACHE_HOME/big-code-analysis/vcs` (or the
    platform equivalent) and is enabled by default; `bca metrics --vcs`
    and `bca report --vcs` reuse it transparently.
  - Web: `POST /vcs` gains optional `no_cache` / `cache_dir` fields.
  - Python: `vcs_metrics(…, no_cache=False, cache_dir=None)`.
- File-type scoping for the change-history ranking (#576). `bca vcs` now
  ranks **only files bca has metrics for by default** instead of every
  tracked text file, so high-churn non-source files (`CHANGELOG.md`,
  `Cargo.lock`, CI config) no longer dominate the risk ranking and the
  standalone ranking agrees with the AST hotspot tables (`bca report
  --vcs`). A new `--file-types <SCOPE>` flag selects the scope: `metrics`
  (the default — resolved by the same extension predicate the metrics
  walk uses, so it stays in lockstep as languages are added/removed),
  `all` (the previous whole-tree behaviour), or a comma-separated
  extension allow-list (`rs,py,toml`; leading dots optional,
  case-insensitive). The filter is extension-only (no blob content is
  read) and ANDs with `--paths` / `--include` / `--exclude`. Because the
  whole VCS surface is still unreleased, the default flip is not a
  stability break. The scope is applied at file enumeration (an
  out-of-scope file is never seeded), so it does not affect the cached
  event log — a cache written under one scope replays correctly under
  another. Surfaces:
  - Library: `big_code_analysis::vcs::{FileTypeScope, Options::file_types}`
    and a new `vcs::Error::InvalidFileTypeScope` variant (behind
    `vcs-git`).
  - CLI: `bca vcs --file-types <metrics|all|EXT,…>` plus a `bca.toml`
    `[vcs] file_types` key (the CLI flag replaces the manifest value).
  - Web: `POST /vcs` gains an optional `file_types` field.
  - Python: `vcs_metrics(…, file_types=None)` and `vcs_trend(…,
    file_types=None)`.
- `Ast::strip_comments()`, `Ast::functions()`, `Ast::dump(cfg)`,
  `Ast::count(filters)`, `Ast::find(filters)`, and `Ast::suppressions()`
  complete the parse-once `Ast` seam: comment removal, function-span
  detection, AST-node dumping, node counting/finding, and suppression
  scanning now have explicit-name, re-parse-free counterparts alongside
  `Ast::metrics` / `Ast::ops`. `Ast` is now the single entry point for
  every analysis operation. Output is identical to the existing
  parser-generic free fns and the `action`/`Callback` dispatch (which
  become redundant and are retired in the 2.0 surface reshape,
  #566/#570) (#567, #571).
- `ParseMetricError::input()` and `ParseLangError::input()` accessors
  return the rejected input string (previously `Display`-only) (#536).
- `bca strip-comments` gains `--output`/`-o` to write a single file's
  stripped source to a path (stdout when omitted); mutually exclusive
  with `--in-place`, and a multi-file input is rejected rather than
  clobbering one path (#539).
- `bca-web`: `GET /v1/version` and `GET /v1/languages` introspection
  endpoints (with unprefixed `/version` and `/languages` aliases),
  mirroring the Python `__version__` / `supported_languages()` /
  `language_extensions()` surface (#541).
- `bca-web --num-jobs` now accepts `<N|auto>` and defaults to a
  cgroup-quota- / cpuset-aware `auto`, matching the `bca` CLI. The
  clap-agnostic `NumJobs` worker-count selector (`FromStr` + `resolve()`)
  is now public library API, re-exported from `big_code_analysis`; its
  `FromStr::Err` is the named `ParseNumJobsError` (`Zero` /
  `NotAPositiveInteger`, each exposing the rejected `input()`,
  `Display + Error`), matching the `ParseMetricError` / `ParseLangError`
  convention (#560).
- Derived `PartialEq` on the compute-side per-metric `Stats` types (abc,
  cognitive, cyclomatic, exit, halstead, loc, mi, nargs, nom, npa, npm,
  tokens, wmc) and on `CodeMetrics` / `FuncSpace` / `Metrics`, so callers
  can compare analyses without round-tripping through `to_wire()` (`Eq`
  omitted due to float fields); derived `Hash` on `SpaceKind` and
  `metric_catalog::Direction`; derived `Hash` + `PartialOrd` / `Ord` on
  `Severity` (ordered scale: `Error > Warning`, following declaration
  order) (#552).
- `ConcurrentErrors` now implements `Display` + `std::error::Error`, so it
  composes with `?` into `Box<dyn Error>` / `anyhow` and participates in a
  `source()` chain (#553).
- Documented the workspace-wide `bca` exit-code convention (0 success,
  1 tool error, 2 `check` gate, 3-5 `check --strict-exit-codes`) in
  top-level `bca --help` and the book, pinned by exit-code tests (#561).
- `STABILITY.md` now locks the output-format contracts: `CSV_HEADER`
  column order, the SARIF 2.1.0 schema version + canonical URI, the
  code-climate field set and fingerprint algorithm, the AST JSON shape
  (one-way `Serialize`-only), and the round-trip vs one-way format split;
  `wire::CyclomaticModified` is named in the serialized-shape enumeration
  (#559).
- Library: `big_code_analysis::VERSION` constant exposing the crate
  version (#541).
- Python: `Lang` and `MetricName` `StrEnum`s (generated from the live
  `LANG` / `Metric::NAMES` tables, so `Lang.CPP == "cpp"` and values
  round-trip with the CLI/JSON slugs); `analyze_batch` gains
  `exclude_tests` / `allow_lossy_path` / `skip_generated` keyword
  arguments (#542).
- The serialized metric output is now **readable back**: a new public
  `big_code_analysis::wire` module provides plain `Serialize`/`Deserialize`
  structs (`wire::FuncSpace`, `wire::CodeMetrics`, `wire::Ops`,
  `wire::FunctionSpan`, and one per metric) mirroring the exact JSON / YAML
  / TOML / CBOR shape. The compute types' `Serialize` impls now delegate to
  these structs (the single definition of the wire shape — output is
  byte-identical), and the public types gain `to_wire()`
  (`FuncSpace`/`CodeMetrics`/`Ops`/`FunctionSpan`). Read a tree back with, e.g.,
  `serde_json::from_str::<wire::FuncSpace>(&json)`. Non-finite floats map
  `null`↔`NaN` (the deserialize side of #531); integer-valued fields are `u64`
  (#530); `wire::CodeMetrics` elides unselected metrics and exposes
  `selected()` to rebuild the `MetricSet` from present keys. `SpaceKind`,
  `SuppressionScope`, and `Metric` now also derive `Deserialize`, and the
  crate enables serde_json's `float_roundtrip` feature so float values
  round-trip bit-exactly through JSON. Additive — no serialized output or
  existing accessor changes
  ([#532](https://github.com/dekobon/big-code-analysis/issues/532), keystone of
  the #510/#530/#531 serialization-schema cluster, part of
  [#505](https://github.com/dekobon/big-code-analysis/issues/505)).

- `bca diff` and `bca diff-baseline` now accept `--output`/`-o <PATH>` (writing
  to the file when given, stdout when omitted) and `--strip-prefix <PREFIX>`
  (trimming the prefix from displayed file paths in the TTY and Markdown
  per-file tables; a no-op for `--format json`), for parity with `report` and
  `exemptions` (#544).
- Round-trip smoke-test coverage for the TOML, YAML, and CBOR per-file output
  formats in `big-code-analysis-cli`, parsing each format back and asserting
  structural keys and integer-valued numeric fidelity against JSON (#543).

- `Ast::ops()` — the `Source`-based counterpart of `get_ops`. Returns the
  operator/operand `Ops` tree for a parsed `Ast`, carrying the
  `Source::name` (`Option<String>`) end-to-end instead of deriving the
  top-level `Ops::name` from a filesystem path via lossy UTF-8 conversion.
  This closes the last public seam that keyed function identity off a lossy
  path: a `None` source name now yields a `None` top-level `Ops::name`
  (which `get_ops` cannot express), and `Ops::name_was_lossy` is never set
  on this path. Mirrors `Ast::metrics`
  ([#509](https://github.com/dekobon/big-code-analysis/issues/509),
  part of [#505](https://github.com/dekobon/big-code-analysis/issues/505)).

- `LANG` now derives `Hash` and implements `Display` (its `name()`
  string) and `FromStr` (parsing that canonical name; case-sensitive, error
  type `ParseLangError`). After the #507 JavaScript-grammar split the only
  variants still sharing a display name are `Tsx` / `Typescript` (both
  `typescript`), which parse back to the first-declared variant (`Tsx`);
  every other name — including `javascript` and `mozjs` — round-trips
  exactly. `Serialize` / `Ord` are deferred to the 2.0 bump
  ([#508](https://github.com/dekobon/big-code-analysis/issues/508)).

- The `bca` CLI is now pip-installable. `pip install big-code-analysis-cli`
  drops the compiled `bca` binary onto your `PATH` (no Rust toolchain
  required), the way `pip install ruff` installs the `ruff` command. The
  PyPI **distribution name is `big-code-analysis-cli`** while the installed
  **command stays `bca`** — distinct from the importable library bindings
  published as `big-code-analysis`. A new
  [`python-cli-wheels.yml`](.github/workflows/python-cli-wheels.yml)
  workflow builds `-b bin` wheels for Linux (`manylinux_2_28` `x86_64` /
  `aarch64`), macOS (`x86_64` / `arm64`), and Windows (`x86_64`),
  smoke-tests each, and publishes to PyPI via Trusted Publishing in
  lockstep with the workspace version. Each wheel carries the full
  `all-languages` grammar set and bundles the per-binary
  `THIRD-PARTY-LICENSES-bca.md` + `LICENSE` (in `.dist-info/licenses/`)
  and the `bca` man pages. (#408)
- `bca report markdown|html` now honors in-source suppression markers
  (`bca: suppress`, `bca: suppress-file`, `#lizard forgives`) **by
  default**, omitting a function from a metric's hotspot table when that
  metric is suppressed for it — matching `bca check` and the SARIF
  emitter (the report previously listed raw values and re-surfaced every
  silenced function). Suppression is per-metric and folds the file's
  `suppress-file` scope into each function's own scope. `bca report
  --no-suppress` (or `[report] no_suppress = true` in `bca.toml`) opts
  into the raw audit view that lists every offender. The Actionable
  Summary roll-up is the sole figure that intentionally keeps counting
  raw measurements (a whole-codebase health indicator); every per-metric
  hotspot caption — including the cyclomatic Average/Max/CC>10 note —
  reflects the suppression-filtered set and is identical across the
  Markdown and HTML reports. `SuppressionScope::merge` is now `pub`
  (additive) so report consumers can fold scopes
  ([#501](https://github.com/dekobon/big-code-analysis/issues/501)).
- `bca check --report-suppressed`: surface the debt the gate tolerates in
  the code-scan document instead of dropping it. Offenders silenced by an
  in-source `bca: suppress` marker or covered by the baseline stay out of
  the gate (exit code and human stream unchanged) but are emitted into the
  `--output-format sarif` document carrying a SARIF `suppressions` entry
  (`kind: "inSource"` for markers, `"external"` for the baseline). Only the
  SARIF format represents suppression; the flag is mutually exclusive with
  `--no-suppress` and `--write-baseline`. Note: GitHub code scanning does
  not honor the SARIF `suppressions` property natively — it ingests such
  results as *open* alerts — so this flag targets downstream tooling that
  reads suppressions (e.g. the `advanced-security/dismiss-alerts` action).
  The repo's own Pages workflow does not pass it; its Code Scanning upload
  carries active offenders only.
- Library: new `write_sarif_with_suppressed(active, in_source, baseline,
  writer)` writer that emits SARIF `suppressions` entries for suppressed
  offenders. `write_sarif` is unchanged (the active-only special case),
  so existing output is byte-for-byte identical.
- `bca diff`: compare two `bca metrics -O json` runs (single JSON files
  or directory trees), bucketing per-file metric deltas by metric in
  `tty` / `markdown` / `json` form, with `--min-change` and `--metric`
  filters. Replaces the external `json-minimal-tests` +
  `split-minimal-tests.py` grammar-bump diff chain (the latter is
  retired). Informational — always exits 0 on success
  ([#487](https://github.com/dekobon/big-code-analysis/issues/487)).
  A `bca diff --since <ref> [<new>]` mode analyzes the tree at a git ref
  (materialized into an auto-cleaning temp dir) for the "before" side
  and diffs it against the working tree or an explicit `<new>`,
  honouring the same `--paths` / `--include` / `--exclude` selection;
  it hard-errors (exit 1) on unresolvable refs or a non-git checkout
  ([#492](https://github.com/dekobon/big-code-analysis/issues/492)).
- `bca check` baseline files now record **tier/headroom provenance**
  (format v5): a `[provenance]` table stamps the tier (and headroom for
  the scaled soft tier) the baseline was written at. `bca check` warns
  when the current run is *stricter* than the baseline was written
  against (the silent-desync the baseline-refresh discipline guards),
  staying silent for the safe hard-reads-soft and equal cases; v2–v4
  baselines read unchanged with provenance treated as absent
  ([#486](https://github.com/dekobon/big-code-analysis/issues/486)).
- `bca check --write-baseline` now accepts an optional path. A bare
  `--write-baseline` (no value) writes to the `baseline` key from the
  auto-discovered `bca.toml` manifest — the same file `bca check` reads
  — so the baseline filename lives in exactly one place. Passing an
  explicit `--write-baseline <path>` still works; the bare form errors
  (exit 1) when no manifest `baseline` is set rather than guessing a
  filename. The repo's own `make self-scan-write-baseline[-headroom]`
  recipes drop their hard-coded path
  ([#496](https://github.com/dekobon/big-code-analysis/issues/496)).
- Support for **F5 iRules** source files (`.irule`, `.irules`), a Tcl
  scripting dialect, via the
  [`tree-sitter-irules`](https://crates.io/crates/tree-sitter-irules)
  grammar. Adds the `Irules` `LANG` variant and the `IrulesCode` /
  `IrulesParser` re-exports, gated behind a new `irules` Cargo feature
  (enabled by `all-languages`). `when EVENT { … }` event handlers and
  `proc` definitions are treated as function spaces, so per-handler
  metrics are reported (the grammar's `on` / `trap` handler nodes are
  handled defensively but parse as ordinary commands in practice). Real
  implementations are provided for ABC,
  cognitive, cyclomatic (including the dedicated `switch`/`switch_arm`
  node and the `and`/`or` keyword operators), exit, Halstead, LoC, and
  NArgs; the remaining metrics use the shared defaults. Additive new
  language — minor bump per [STABILITY.md](./STABILITY.md).
- Cyclomatic complexity's counting of Rust's `?` operator (the
  `try_expression` grammar node) is now configurable
  ([#409](https://github.com/dekobon/big-code-analysis/issues/409)).
  `MetricsOptions` gains a `count_cyclomatic_try` field (default
  `true`) and a `with_count_cyclomatic_try` builder; the CLI gains a
  global `--no-cyclomatic-try` flag and a `cyclomatic_count_try`
  `bca.toml` key (the flag ORs on top — it can force opt-out but not
  force counting back on). Setting it false treats `?` as linear error
  propagation rather than a branch, on both standard and modified
  cyclomatic. The repo's own `make self-scan` gate sets it via the
  `bca.toml` manifest. **The default is unchanged**: `?` keeps
  counting `+1`, matching upstream rust-code-analysis, so every
  published metric value and existing snapshot is byte-identical (no
  value change on the default path). Rust-only — no other language
  emits the node, so the toggle is inert elsewhere. Additive per
  `STABILITY.md`: `MetricsOptions` is `#[non_exhaustive]`, so the new
  field and builder do not break downstream callers; a global default
  flip is deferred to a deliberate `2.0` decision.
- `metric_catalog::MetricInfo` gains a `skip_at_unit: bool` field
  recording whether a metric's serialized JSON headline at the
  file-level `unit` space is an aggregate over descendant spaces that
  does not match the CLI threshold accessor's per-space scalar (`true`
  for `cognitive`, `cyclomatic`, `cyclomatic.modified`, and `abc`).
  This is the single source of truth the CLI `EXTRACTORS` table and the
  Python `to_sarif` binding's `METRIC_FIELDS` table now both derive
  from, so a metric added to one front-end but not the other — or a
  `skip_at_unit` flag that disagrees — is a build/test failure instead
  of silent SARIF divergence
  ([#442](https://github.com/dekobon/big-code-analysis/issues/442)).
  Additive: `MetricInfo` is `#[non_exhaustive]`, so the new field does
  not break downstream readers.

- `bca exemptions` audits everything the `bca check` gate skips in one
  report ([#386](https://github.com/dekobon/big-code-analysis/issues/386)):
  in-source suppression markers (`bca: suppress`, `#lizard forgives`,
  …), `[check.exclude]` globs, and `.bca-baseline.toml` entries. Each
  marker is listed with its file, line, target (function/file), metric
  scope, dialect, and surrounding function, so reviewers can see every
  silencer in the tree — not just the offenders they happen to hide.
  `--format tty|markdown|json` selects the output style (`json` nests
  the three tiers under a single `suppressions` envelope; an omitted
  section is `null`, a requested-but-empty one is `[]`); the combinable
  `--only-markers` / `--only-excludes` / `--only-baseline` flags narrow
  the report for PR-bot use. The walk honours `[walker.exclude]`, and
  the baseline (`bca.toml` top-level `baseline`) and `[check.exclude]`
  inputs default to the same sources `bca check` reads. Read-only and
  informational: it
  always exits 0 on success (1 on a tool error such as a missing
  `--baseline`), never gating. The new `big-code-analysis` library
  re-exports `SuppressionMarker`, `SuppressionTarget`,
  `SuppressionDialect`, and the `SuppressionScan` callback that back it.
- `bca check --strict-exit-codes` opts into tiered exit codes that
  split the violation case (previously a single exit `2`) by severity
  ([#385](https://github.com/dekobon/big-code-analysis/issues/385)):
  `2` new offenders only, `3` regressions only, `4` both, `5` a
  `--tier=soft` violation that also breaches the hard limit. CI can
  now branch on severity without parsing the `[new]` / `[regr +N%]`
  stderr tags. The default 0/1/2 contract is unchanged — the tiered
  mode is opt-in via the flag or `[check] exit_codes = "tiered"` in
  `bca.toml` (the flag ORs on top: a bare flag cannot represent
  "off"). Every fail-state stays non-zero, so existing
  `exit != 0 → fail` tooling is unaffected; only consumers that test
  `$? -eq 2` explicitly need to widen to `2`-`5`. `--no-fail` still
  forces exit `0`. `--print-effective-config` reports the resolved
  `exit_codes` style.
- `bca diff-baseline old.toml new.toml` emits a structured diff
  between two `.bca-baseline.toml` files — `added`, `removed`,
  `worsened`, `improved` — replacing the in-the-head TOML diff
  parsing the baselines recipe used to walk reviewers through
  ([#382](https://github.com/dekobon/big-code-analysis/issues/382)).
  Entries pair on their `(path, qualified, metric)` identity (line
  drift is tolerated, mirroring the on-disk matcher), so only genuine
  value changes surface. `--format tty|markdown|json` selects the
  output style (`markdown` fences each section for a sticky PR
  comment; `json` emits the full structured diff). The combinable
  `--added-only` / `--removed-only` / `--worsened-only` /
  `--improved-only` flags narrow the rendered sections for PR-bot use.
  Both files are read through the same loader `bca check` uses, so any
  supported legacy version (v2/v3) is migrated on read and an
  unsupported version is a clear error rather than a silent
  no-match. The command always exits 0 on success — the diff is
  informational, not a gate.
- `bca check` gains a glob-level gate exemption via a `[check]
  exclude` list in `bca.toml` and the `--check-exclude` /
  `--check-exclude-from` flags
  ([#378](https://github.com/dekobon/big-code-analysis/issues/378)).
  Matching files are still walked, parsed, metric'd, and shown by
  `bca report` — only `bca check` drops their violations before
  emitting offenders and before `--write-baseline` records anything,
  so structural exemptions (test fixtures, generated code,
  macro-dispatch modules) stay out of `.bca-baseline.toml`.
  `--check-exclude` is repeatable; `--check-exclude-from` reads a
  `.gitignore`-style file (convention `.bcacheckignore`); the two
  union with each other, while an explicit `--check-exclude` replaces
  the manifest `[check] exclude` list (CLI-wins, like every other
  manifest key). Globs match the walked path exactly like `--exclude`.
  Precedence, most-specific first: in-source
  `bca: suppress` markers, then `[check.exclude]` globs, then the
  baseline. `--print-effective-config` reports the resolved
  `check_exclude` globs.
- `bca check` gains a native two-tier threshold model via a
  `[thresholds.soft]` table and a `--tier <hard|soft>` flag
  ([#375](https://github.com/dekobon/big-code-analysis/issues/375)).
  The default `hard` tier compares against `[thresholds]` verbatim.
  `--tier=soft` is the early-warning tier: it merges
  `[thresholds.soft]` overrides on top of `[thresholds]` (per metric,
  either an absolute limit like `cognitive = 18` or a
  scale-relative `"0.9x"` string that multiplies the hard limit);
  metrics absent from the soft table inherit their hard limit (no
  soft band). When no soft table is configured, `--tier=soft` falls
  back to scaling every limit by `--headroom` (default `0.95`). Both
  the manifest `bca.toml` and `--config` files accept the soft
  sub-table, and both tiers ratchet through the same `--baseline`.
  `--print-effective-config` now reports the resolved `tier`. As a
  consequence, **`--headroom` is now a soft-tier dial**: it takes
  effect only under `--tier=soft` (ignored with a note at the hard
  tier), and an explicit `[thresholds.soft]` table takes precedence
  over `--headroom` (which is then ignored with a warning).
- `bca check` baselines now match on the **qualified symbol** rather
  than the exact start line
  ([#377](https://github.com/dekobon/big-code-analysis/issues/377)).
  Each entry keys on `(path, qualified_symbol, metric)` — e.g.
  `MyStruct::do_thing` — so editing code above a named function no
  longer re-keys it as a `[new]` offender (the most common source of
  baseline churn). A configurable `start_line` tolerance
  (`--baseline-line-tolerance <LINES>`, default 50, or
  `baseline_line_tolerance` in `bca.toml`) disambiguates a symbol
  shared by several functions. A new `--baseline-fuzzy-match` flag
  (`baseline_fuzzy_match` in `bca.toml`) enables a rename-tolerant
  body-hash fallback: a function renamed but otherwise unchanged stays
  covered, because the normalised body digest (which elides the
  function's own name and ignores indentation/blank-line churn) still
  matches. The offender line and JSON `function` field now show the
  qualified symbol. The baseline schema bumps to **v4** (`function`
  field renamed to `qualified`, optional `body_hash` added); v2/v3
  baselines are still read and degrade to bare-name + tolerance
  matching until refreshed with `--write-baseline`. See
  `STABILITY.md` for the migration. Additive, minor bump.
- `bca.toml` manifest — auto-discovered at (or above) the working
  directory, consolidating the flags every local-gate recipe used to
  thread through each invocation
  ([#374](https://github.com/dekobon/big-code-analysis/issues/374)).
  Top-level keys `paths`, `exclude_from`, `num_jobs`, `include`,
  `exclude`, `baseline`, and `headroom`, plus an inline `[thresholds]`
  table, map to the corresponding flags. Explicit CLI flags always win
  over manifest keys; `--config <file>` merges on top of the manifest
  `[thresholds]` table (resolution order: manifest `[thresholds]` →
  `--config` → tier resolution → `--threshold` overrides). Relative
  manifest paths resolve against the manifest's directory. A new global
  `--no-config` flag skips discovery for fully-explicit invocations
  (`bca init` also ignores any existing manifest, since it scaffolds
  config rather than consuming it).
  Unrecognized keys (forthcoming `[check]`, `exit_codes`) are ignored
  with a one-line warning so projects can pre-adopt schema additions. `bca check --print-effective-config` gains
  a `manifest` provenance line. Additive, minor bump.
- `bca check --headroom <ratio>` — scales every threshold from
  `--config` (or `bca.toml`'s `[thresholds]`) by a ratio in `(0, 1]`
  before the offender comparison, implementing the soft-tier
  early-warning gate natively
  ([#373](https://github.com/dekobon/big-code-analysis/issues/373)).
  `0.95` (the default knob in the local-gates recipe) fires on
  functions that have reached 95% of any limit; `1.0` is a no-op
  parity run with the hard gate; out-of-range values exit 1.
  `--headroom` is a **soft-tier dial**: it takes effect only under
  `--tier=soft` (see #375; ignored with a note at the default hard
  tier). Explicit `--threshold name=value` overrides are absolute and
  are applied after scaling (resolution order: config → tier
  resolution → `--threshold`). Stacks with `--write-baseline` (the
  baseline then captures offenders at the scaled limits) and is
  surfaced by `--print-effective-config`. Replaces the
  `utils/bca-self-scan-headroom.py` helper, which is removed;
  `make self-scan-headroom` and the local-gates book recipe now
  invoke `--tier=soft --headroom` directly. Additive, minor bump.
- `metric_catalog` module — a single canonical registry of metric
  metadata ([#397](https://github.com/dekobon/big-code-analysis/issues/397)).
  Public items: `metric_catalog::{MetricInfo, MetricFamily, MetricRow,
  Direction, METRICS, FAMILIES}`. `METRICS` is the canonical list of
  offender sub-metric ids (`halstead.volume`, `mi.original`, …) with
  their long-form SARIF / Code Climate sentences and
  higher-/lower-is-worse `Direction`; `FAMILIES` is the view rendered
  by `bca list-metrics`. The library's offender formatters and the
  CLI's threshold engine now read this one source instead of three
  hand-maintained tables that had silently drifted (ten rule-
  description keys once matched no real offender id for two model
  versions). A cross-crate parity test pins the threshold extractor
  ids to `METRICS`, so a new metric can no longer ship with a half-
  updated catalog. SARIF, Code Climate, and `bca list-metrics` output
  are unchanged. Additive, minor bump.
- Python bindings dev environment: `make py-bootstrap`, `make py-sync`
  (alias), `make py-relock`, and `make py-clean` Makefile targets.
  Bootstrap provisions `big-code-analysis-py/.venv` from the
  checked-in `uv.lock` via `uv sync --locked --extra dev`; relock
  regenerates `uv.lock` after a `pyproject.toml` edit; py-clean
  removes `.venv`, the editable-install compiled extension, per-tool
  caches (`.pytest_cache`, `.mypy_cache`, `.ruff_cache`), and
  `__pycache__` trees. Requires `uv` to be installed locally; see
  CONTRIBUTING.md for the install one-liners.
- `make distclean` target — chains `py-clean` and `cargo clean` for a
  full-wipe before a from-scratch bootstrap. `make clean` continues
  to do `cargo clean` only.
- `grammar-marker-sync` static lint (`check-grammar-marker-sync.py`,
  baseline at `.grammar-marker-baseline.toml`) blocking the failure
  mode from
  [#400](https://github.com/dekobon/big-code-analysis/issues/400):
  bumping the notification-only `tree-sitter-javascript` /
  `tree-sitter-cpp` marker in `tree-sitter-{mozjs,mozcpp}/Cargo.toml`
  without re-running the matching
  `./generate-grammars/generate-*.sh` script ships a marker that
  lies about the bundled `src/parser.c` version. The gate compares
  the live marker against the baseline and fails on drift in either
  direction (marker bumped without regen, or regen without baseline
  refresh — `--update` after a verified regen). Wired into `make
  lint`, `make pre-commit`, `make ci`, the `.pre-commit-config.yaml`
  system hook, and a defensive explicit `lint` job step in
  `.github/workflows/ci.yml`. Verified against the
  `tree-sitter-javascript` 0.23.1 → 0.25.0 marker bump (#1207) that
  motivated #400: regen against the live 0.25.0 marker confirmed
  no source diff under
  `tree-sitter-mozjs/src/{parser.c,scanner.c,grammar.json,node-types.json}`
  with `tree-sitter` CLI 0.26.9.
- `enums-codegen-drift` static lint
  (`check-enums-codegen-drift.sh`) blocking the failure mode from
  [#405](https://github.com/dekobon/big-code-analysis/issues/405):
  running any `recreate-grammars.sh` invocation silently
  regenerated `src/c_langs_macros/{c_macros,c_specials}.rs` to a
  pre-optimization form (linear `.contains()` lookup + missing
  sorted-invariant tests), undoing months of hand-improved work.
  The `enums/templates/c_macros.rs` template now emits the
  `binary_search`-based lookup plus the `*_is_sorted`,
  `*_lookup`, and `*_lookup_boundaries` test modules; the gate
  runs the codegen into a tempdir and diffs against the
  checked-in files so any future divergence fails CI. Wired into
  `make lint`, `make pre-commit`, `make ci`, the
  `.pre-commit-config.yaml` system hook, and a defensive
  explicit `lint`-job step in `.github/workflows/ci.yml`.

- `check-manpage-assets` static lint
  (`check-manpage-assets.py`) blocking the failure mode from
  [#444](https://github.com/dekobon/big-code-analysis/issues/444):
  a `bca` subcommand man page that drops out of the
  hand-maintained deb/rpm asset lists ships a package without its
  page. The gate globs `man/bca-*.1`, partitions `bca-web.1` to
  `big-code-analysis-web` and every other page to
  `big-code-analysis-cli`, and asserts each page appears in BOTH
  the `[package.metadata.deb].assets` and
  `[package.metadata.generate-rpm].assets` tables of its owning
  crate's `Cargo.toml`, failing loud with the offending
  filename(s). Wired into `make lint`, `make pre-commit`, `make
  ci`, the `.pre-commit-config.yaml` system hook, and a defensive
  explicit `lint`-job step in `.github/workflows/ci.yml`
  ([#446](https://github.com/dekobon/big-code-analysis/issues/446)).
  The guard is now bidirectional
  ([#447](https://github.com/dekobon/big-code-analysis/issues/447)):
  beyond asserting every page is listed in its owner, it also fails on
  a page listed in the *wrong* crate's asset tables (cross-contamination)
  and on a stale asset entry whose `bca-*.1` source no longer exists
  under `man/`, both scoped to `bca-*.1` basenames so binaries,
  completions, the top-level `bca.1`, and licences are not swept in.

- `bca check` actionable failure output (umbrella
  [#356](https://github.com/dekobon/big-code-analysis/issues/356)
  now complete):
  - `--since <ref>` / `--changed-only` diff-aware mode: the
    summary footer surfaces "Files in this range:" (offenders in
    files touched between the diff base and `HEAD`) before the
    legacy offender list; `--changed-only` drops out-of-range
    rows entirely for terser PR-gate output. Auto-detects the
    diff base from `BCA_DIFF_BASE`, `GITHUB_BASE_REF`, or
    `GITHUB_EVENT_BEFORE` in that precedence. Pass
    `-c core.quotePath=false` to git so non-ASCII filenames
    survive the canonicalize roundtrip. Fixes
    [#359](https://github.com/dekobon/big-code-analysis/issues/359).
  - `--github-annotations` (auto-enabled when
    `$GITHUB_ACTIONS == "true"`) emits `::error file=…,line=…,
    title=…::msg` workflow commands so the GHA UI renders
    inline annotations on the file-diff view. Capped at 10 per
    metric with an overflow rollup line so a 400-violation run
    cannot exhaust GitHub's 10-error-per-step UI quota. Fixes
    [#360](https://github.com/dekobon/big-code-analysis/issues/360).
  - `$GITHUB_STEP_SUMMARY` markdown digest (or
    `--summary-file <path>`) — per-file rollup, per-metric
    breakdown, top-10 offenders by ratio. Bracketed by
    HTML-comment markers so a retried step replaces (not stacks)
    the previous block. Fixes
    [#361](https://github.com/dekobon/big-code-analysis/issues/361).
  - Trailing `--- next steps ---` remediation block on stderr
    (and inside the step-summary digest) names the artifact,
    prints a copy-paste-safe `--write-baseline` refresh
    invocation that mirrors the gate's resolved path filters,
    and links to the Baselines recipe. Suppress with
    `--no-remediation`. Fixes
    [#362](https://github.com/dekobon/big-code-analysis/issues/362).
- `bca check --output-format code-climate` (new) emits GitLab Code
  Climate JSON directly into the MR Code Quality widget, replacing
  the previous third-party Checkstyle→Code-Climate converter recipe.
  Severity bands map metric-vs-threshold ratios onto GitLab's five
  levels (`minor` ≤1.5×, `major` ≤2×, `critical` ≤4×, `blocker`
  >4×), inverted for the `mi.*` family where lower is worse.
  Fingerprints hash `path \0 function \0 metric` (deliberately
  excluding line and value) so cosmetic line-drift edits still
  collapse into the same widget entry. Fixes
  [#354](https://github.com/dekobon/big-code-analysis/issues/354).
- `enums/tests/dispatch.rs` (new) pins every `Lang` variant to its
  expected backing tree-sitter grammar crate via per-variant
  integration tests for `get_language` and `get_language_name`,
  catching the Cpp→mozcpp class of drift bug (fixed in
  [#344](https://github.com/dekobon/big-code-analysis/issues/344))
  at `cargo test` time rather than first-dispatch panic. The new
  test suite runs under `make enums-check` (extended in this
  release) so pre-commit and CI gate on it. Fixes
  [#350](https://github.com/dekobon/big-code-analysis/issues/350).
- `bca init`: new subcommand scaffolds the canonical pre-#374
  adoption files in one shot — `bca-thresholds.toml` (with the
  full header comment), `.bcaignore` (with commented default
  patterns), and an initial `.bca-baseline.toml` derived from a
  write-baseline pass. Flags: `--dir <DIR>`, `--force`,
  `--no-baseline`. Interactive prompts and `--emit
  make|just|pre-commit|github-actions` skeletons are deferred to
  follow-up. Fixes
  [#379](https://github.com/dekobon/big-code-analysis/issues/379).
- `bca check --print-effective-config[=FORMAT]` serializes the
  resolved threshold / check configuration after merging
  `--config` TOML + `--threshold` CLI overrides, then exits 0
  without walking the codebase. Default format is TOML; `=json`
  selects JSON. Mutually exclusive with `--write-baseline`. The
  printed view is round-trippable through `--config`. Future
  layers (#373 headroom, #374 `bca.toml`, #375
  `[thresholds.soft]`, #385 tiered exit codes) plug into the same
  printer without changing its CLI surface. Fixes
  [#380](https://github.com/dekobon/big-code-analysis/issues/380).
- `bca check` / config now suggests the closest known metric name
  when a `--threshold` flag or `[thresholds]` TOML key is
  misspelled. Uses Levenshtein with a `min(2, max(len)/3)` cutoff
  plus a shared-prefix rescue for truncations (covers
  `cyclic` → `cyclomatic` and `halstead.efort` →
  `halstead.effort`). Up to three ties listed; unrelated input
  still falls back to the prior "unknown metric" error. Fixes
  [#381](https://github.com/dekobon/big-code-analysis/issues/381).
- Python `analyze()` gains a keyword-only `vcs_per_function=True` flag
  that mirrors the CLI's `bca metrics --vcs-per-function`: it blames the
  file once and attaches a per-function `vcs` block (byte-identical in
  shape to the CLI's) to every nested function/method/class space in the
  returned JSON tree. Independent of the file-level `vcs=` opt-in; degrades
  gracefully outside a git repository (#578).
- `bca vcs jit --diff <file>` (and `--diff -` for stdin) scores an
  arbitrary `git diff`-style unified diff. A bare diff carries no author,
  parent, or file history, so only the size and diffusion feature groups
  are computable; the result is a distinct partial report (`source:
  "diff"`, `partial_score`) whose history/experience/purpose groups are
  *absent* (not zero) and whose score is **not comparable** to a commit
  score. New library surface `vcs::score_diff`. Just-in-time scoring is
  now also exposed via the REST `POST /vcs/jit` endpoint and the Python
  `vcs_jit(repo_path, commit=…, diff=…)` binding, both reusing
  `score_commit` / `score_diff`. Commit-mode JIT output is unchanged
  (#580).
- `bca-web` gains an opt-in `--cors <ORIGINS>` flag (off by default) so
  browser tooling can call the API cross-origin without a proxy. The
  argument is an explicit comma-separated allow-list; a listed origin is
  echoed back in `Access-Control-Allow-Origin`, an unlisted origin gets
  no header, and a literal `*` opts into a wide-open policy. Layered on
  the existing RFC 9110 OPTIONS→204 + `Allow` handling via a new
  `CorsPolicy` enum and `from_fn` middleware (the preflight sources its
  methods from the resource's own `Allow` header), wrapped under
  `Condition` so the default request path carries no extra layer.
  `Access-Control-Allow-Credentials` is never emitted (#694).
- The REST API now honors the `Accept` header on every structured
  analysis endpoint (`/v1/ast`, `/v1/comment` JSON, `/v1/function`,
  `/v1/metrics`, `/v1/vcs`, `/v1/vcs/trend`, `/v1/vcs/jit`), reusing the
  same `serde_yaml` / `ciborium` serializers the CLI drives so a value is
  byte-identical over both surfaces. JSON stays the default (absent
  `Accept`, `*/*`, `application/*`, `application/json`); `application/yaml`
  and `application/cbor` get that format with the matching `Content-Type`,
  q-weights are honored, and any other concrete type answers `406 Not
  Acceptable` through the uniform `{error, error_kind, id}` envelope. TOML
  and CSV are excluded (#657).
- Python VCS documentation: a new `python/vcs.md` book chapter covering
  the namespaced change-history surface (`vcs.rank` / `vcs.trend` /
  `vcs.commit` / `vcs.score_diff` and the shared `vcs.Options`), the
  widened option kwargs (#619), the `as_of` reproducible-snapshot
  semantics (#648), and the GIL-release `ThreadPoolExecutor` note (#620);
  `python/errors.md` gains the typed VCS exception taxonomy (#624) and the
  stale flat references in the CLI `vcs.md` are refreshed to the post-#612
  namespaced names (#649).

### Changed

- VCS JIT scoring diffs each touched blob once (computing added/deleted
  counts and hunk count from a single `Diff::compute`) instead of twice,
  with bit-identical results (#815).

- The HTML and Markdown report's headline **Average MI** is now the
  **SLOC-weighted mean of the *unclamped* Visual Studio MI** and is
  relabelled `Average MI (SLOC-weighted)`. Previously it averaged the
  *clamped* `mi_visual_studio` (floored at 0) over the file count, so a
  catastrophically unmaintainable file (true MI ≈ −400) and a marginally
  bad one (≈ −5) both contributed 0 and were indistinguishable, and a
  five-line file counted as much as a five-thousand-line one. The new
  headline mirrors the MI hotspot ranking (which already sorts on the
  unclamped value, #627): large files dominate and the figure can go
  negative for an unmaintainable codebase. The per-language overview's
  `Avg MI` column changes the same way and gains its own tooltip. The
  per-file `MI` hotspot column is unchanged (still the clamped Visual
  Studio value). Report output is not contract-locked, so this is not a
  SemVer break, but published headline numbers move (#725, follow-up to
  #627).
- **(breaking)** Lib: the AST `Span` struct (`big_code_analysis::Span`)
  gains `start_byte` / `end_byte` fields (0-based, half-open byte offsets
  into the parsed source) and is now `#[non_exhaustive]`. Construct it via
  the new `Span::new(...)` constructor; struct-literal construction
  (`Span { start_line, .. }`) and exhaustive destructuring from outside the
  crate no longer compile. The serialized wire shape only *adds* the two
  byte fields (both `#[serde(default)]`, so pre-existing line/col-only span
  JSON still deserializes), so `/ast` and dump consumers are unaffected
  beyond the additive fields; only Rust callers that built or destructured
  `Span` by literal are affected. Deferred to the **2.0** milestone (#727).
- **(breaking)** `LANG::Cpp` (slug `cpp`) is now backed by the upstream
  community `tree-sitter-cpp` grammar instead of the Mozilla fork. The
  fork moved to the new opt-in `LANG::Mozcpp` (see Added). The `cpp`
  Cargo feature's dependency set changed accordingly
  (`bca-tree-sitter-mozcpp` → `tree-sitter-cpp`); a `--no-default-features`
  consumer that enabled `cpp` for the Mozilla dialect must now also enable
  `mozcpp`. Default (`all-languages`) builds analyze the same `.c` / `.h` /
  `.cpp` / … extensions as before. Generic C++ metric values shift
  slightly where the Gecko overlay diverged from upstream (≈0.6% of files
  in the measurement corpus, #719); the integration snapshots were
  re-baselined in the same change. Deferred to the **2.0** milestone
  (#720, part of #718).
- **(Python)** `analyze(..., vcs=True)` and
  `analyze(..., vcs_per_function=True)` now release the GIL across their
  per-file history walk / blame-engine open via `Python::detach`, the same
  off-GIL treatment the `vcs.rank`/`trend`/`commit` entry points and the
  batch path already had — completing the GIL release across the VCS
  surface (#620). The walk touches no Python objects, so the cheap
  JSON-injection step stays under the re-acquired GIL; results and
  signatures are unchanged, so a `ThreadPoolExecutor` over several
  `analyze(vcs=True)` calls now parallelises instead of serialising on the
  walk.
- Web/Lib: the `id` field on every JSON request payload (`/v1/ast`,
  `/v1/comment`, `/v1/function`, `/v1/metrics`, `/v1/vcs`,
  `/v1/vcs/trend`, `/v1/vcs/jit`) and the `comment` / `span` fields on
  `/v1/ast` are now optional (`#[serde(default)]`). Omitting `id`
  defaults to an empty string (the "no correlation id" sentinel echoed
  back unchanged); omitting `comment` / `span` defaults to `false`. This
  ends the JSON-vs-query-variant inconsistency where the query form
  already defaulted these fields while the JSON form returned a `400
  missing field`. Strictly request-side loosening: previously-valid
  payloads are unaffected and no new keys are accepted (#645).
- **(breaking)** Lib: Halstead `Stats` accessors renamed to the wire
  vocabulary — `u_operators` → `unique_operators`, `operators` →
  `total_operators`, `u_operands` → `unique_operands`, `operands` →
  `total_operands`. JSON/YAML/TOML/CBOR output keys are unchanged.
  Deferred to the 2.0.0 release (#588).
- **(breaking)** Lib: the `exit` metric module is renamed to `nexits`
  (`src/metrics/exit.rs` → `nexits.rs`; the crate-internal
  `crate::exit` path is now `crate::nexits`), its `Stats` accessors
  `exit`/`exit_sum`/`exit_average`/`exit_min`/`exit_max` become
  `nexits`/`nexits_sum`/`nexits_average`/`nexits_min`/`nexits_max`,
  and the retired `"exit"` parse alias for `Metric::Nexits` no longer
  resolves (only `"nexits"` parses). Output keys are unchanged.
  Deferred to the 2.0.0 release (#588).
- **(breaking)** Lib: NArgs `Stats` accessors renamed —
  `fn_args`/`fn_args_sum`/`fn_args_average`/`fn_args_min`/`fn_args_max`
  → `function_args`/`function_args_sum`/`function_args_average`/
  `function_args_min`/`function_args_max`, and `nargs_total`/
  `nargs_average` → `total`/`average`. Wire keys are unchanged.
  Deferred to the 2.0.0 release (#588).
- **(breaking)** Lib: MI `Stats` accessors renamed —
  `mi_original`/`mi_sei`/`mi_visual_studio` →
  `original`/`sei`/`visual_studio`. Output keys are unchanged.
  Deferred to the 2.0.0 release (#588).
- **(breaking)** Lib: the `Metric::NArgs` enum variant is renamed to
  `Metric::Nargs`; its lowercase `"nargs"` serde/`Display`/`FromStr`
  spelling is unchanged. Deferred to the 2.0.0 release (#588).
- **(breaking)** Output: the sum-carrying `classes` / `interfaces`
  wire keys on `npm` / `npa` / `wmc` are renamed to mirror their
  accessors — `npm.{class_npm_sum,interface_npm_sum}`,
  `npa.{class_npa_sum,interface_npa_sum}`,
  `wmc.{class_wmc_sum,interface_wmc_sum}` — across JSON/YAML/TOML/CBOR
  and the CSV columns. The truthful sibling keys (`class_methods`,
  `total`, `coa`, `cda`, …) are unchanged. Deferred to the 2.0.0
  release (#589).
- **(breaking)** Output: the JSON `tokens` block's `tokens_average` /
  `tokens_min` / `tokens_max` leaves are renamed `average` / `min` /
  `max`, matching the CSV columns; the bare-sum `tokens` leaf is kept.
  The terminal dump's tokens sum label changes `sum` → `tokens` to
  match. Deferred to the 2.0.0 release (#590).
- **(breaking)** CLI: the default text metric dump is now driven from
  the serialized (`wire::CodeMetrics`) shape, so every metric block
  renders its full, uniform field set (e.g. `loc` now shows the
  averages and min/max, `nexits` renders as a `sum`/`average`/`min`/
  `max` aggregate) instead of a hand-picked per-metric subset. Float
  values render rounded to two decimals in the text view only; JSON
  keeps full precision. Deferred to the 2.0.0 release (#674).
- **(breaking)** Output: the per-file change-history (VCS) block is now
  a **nested `vcs` object** under each ranked file (and each
  `/vcs/trend` point) for `bca vcs`, `POST /vcs`, `vcs_metrics()`, and
  `vcs_trend()`, replacing the former flattened-beside-`path` layout;
  CSV stays flat with dotted columns. Deferred to the 2.0.0 release
  (#684).
- **(breaking)** Output: the per-row VCS block is always-slim — the
  four constant stamps `vcs_schema_version`, `risk_score_version`,
  `long_window_days`, and `recent_window_days` are carried exactly
  once on the enclosing `/vcs` and `/vcs/trend` envelope (and no
  longer duplicated per file row or per trend point). `POST /vcs`
  gains the two version stamps at the response top level. Deferred to
  the 2.0.0 release (#635).
- **(breaking)** CLI: argv/usage/value-parse errors now exit 1 instead
  of clap's 2, reserving exit codes 2–5 for the `check` and
  `vcs jit --fail-above`-style metric gates; `--help` / `--version`
  still exit 0 (#594).
- **(breaking)** CLI: renamed `--language-type` to `--language`
  (hidden alias kept one cycle). The flag accepts a language name
  (`rust`) or extension (`rs`); an unknown value is now a hard error
  listing valid languages instead of silently disabling analysis
  (#595).
- **(breaking)** CLI: walk commands default `--paths` to `.` when no
  CLI/manifest seed is given; a nonexistent explicit path now fails
  with exit 1 instead of warning and exiting 0; a zero-file walk
  prints a stderr notice (#596).
- **(breaking)** CLI: `-I/--include` and `-X/--exclude` take exactly
  one glob per occurrence and are repeatable; the greedy
  space-separated multi-value spelling no longer parses (#601).
- **(breaking)** CLI: `--top` unified on `usize` with `0` meaning
  "all rows" across `vcs`, `report`, and `vcs trend`
  (`report --top 0` was previously a usage error) (#602).
- **(breaking)** CLI: renamed `--num-jobs` to `--jobs` and `--warning`
  to `--warnings` (hidden aliases kept one cycle), and the default
  tree output is now selectable explicitly as `--format text` on
  `metrics`/`ops` (#604).
- **(breaking)** Manifest: the check-only keys `baseline`,
  `baseline_line_tolerance`, `baseline_fuzzy_match`, and `headroom`
  moved under `[check]` in `bca.toml`; the top-level spelling warns
  for one release cycle and goes away at 2.0 (#599).
- **(breaking)** Wire: version stamps are now uniformly
  domain-prefixed — bus factor emits `bus_factor_schema_version`
  (schema 2, was bare `schema_version`) and JIT reports emit
  `risk_score` / `partial_risk_score` (schema 3, was bare
  `score` / `partial_score`) (#591).
- **(breaking)** Web: `/vcs/jit` rejects a payload combining `diff`
  with any commit-mode field (400 naming the conflict) instead of
  silently ignoring `repo_path`/`commit` (#632).
- **(breaking)** Web: an unsupported language now answers
  `422 Unprocessable Entity` with the machine token
  `unsupported_language` instead of 404; 404 is reserved for unknown
  routes (#634).
- **(breaking)** Web: the `/comment` JSON response returns the
  stripped source as a string instead of an array of byte numbers
  (#629).
- `bca vcs jit` / `vcs trend` accept the history-tuning flags
  (`--long-window`, `--as-of`, …) in the subcommand position;
  `--ref` combined with `vcs jit` is now a usage error instead of
  being silently ignored (#598).
- Report headings and the Languages line show human-readable language
  names (C++, C#, TSX, …); slugs are unchanged in structured output
  and CSS classes (#613).
- The report's three differently-filtered cyclomatic statistics are
  captioned (CC note excludes suppressed functions; the Actionable
  Summary names its raw basis and suppressed count; a fully-suppressed
  hotspot table leaves a "table omitted" note) (#616).
- Halstead Effort and Functions-With-Many-Parameters hotspot tables
  gained the Line column in both report formats (#628).
- Rendered VCS report polish: plain-English bus-factor wording,
  thousands separators on count cells, gap-free heading levels, and a
  provenance line with the ordinal-only Risk caveat (#618).
- Python: `language_for_file` returns `Lang | None` (a `StrEnum`, so
  string comparisons keep working) and `language_extensions` accepts
  `str | Lang` (#625).
- Python: pyproject metadata polish before first publish — Beta
  status, `Typing :: Typed`, Python 3.14 classifier, Documentation
  and Changelog URLs (#626).
- The repository's own `suppress-file` markers migrated from the
  legacy `exit` spelling to the canonical `nexits` (the parser alias
  for `exit` is unchanged) (#593).
- **(breaking, deferred to 2.0)** Retired the `action` / `Callback`
  dispatch and the path-positional analysis surface, leaving
  [`Ast`] (with `analyze` for the one-shot case) as the single public
  analysis seam (#566, #570). Removed: the `Callback` trait and its
  per-action tag/`Cfg` types (`Dump`/`DumpCfg`, `CommentRm`/`CommentRmCfg`,
  `Function`/`FunctionCfg`, `Find`/`FindCfg`, `CountCfg`,
  `NodeTypeFilters`, `OpsCode`/`OpsCfg`, `Metrics`/`MetricsCfg`,
  `SuppressionScan`, `AstCallback`); the `action` dispatcher; the
  parser-generic free functions `metrics` / `metrics_with_options`
  (in `spaces`) and `operands_and_operators` (in `ops`); and the
  path-positional shims `get_function_spaces`,
  `get_function_spaces_with_options`, `metrics_from_tree`, and
  `get_ops`. The internal parser machinery is demoted from `pub` to
  `pub(crate)` and dropped from the crate root and prelude:
  `Parser`, `ParserTrait`, `Filter`, `LanguageInfo`, `Alterator`,
  `Getter`, `Checker`, the per-metric compute traits
  (`Cyclomatic`/`Cognitive`/`Halstead`/`Loc`/`Nom`/`Mi`/`NArgs`/`Exit`/
  `Wmc`/`Abc`/`Npm`/`Npa`/`Tokens`), the per-language `<Lang>Parser`
  aliases and `<Lang>Code` tags, `PreprocParser`, and the
  `rm_comments` / `function` / `count` / `find` / `suppression_markers`
  walk cores. Callers migrate to `Ast` (`parse`, `from_tree_sitter`,
  `metrics`, `ops`, `strip_comments`, `functions`, `dump`, `count`,
  `find`, `suppressions`, `root_node`) or `analyze`. No metric values
  change — this is a pure removal/visibility change. The deletions land
  staged on `main` and take effect at the `2.0` major bump.
- `bca` now analyzes each file through the explicit-name `analyze` /
  `Ast::ops` seams instead of the deprecated path-positional shims
  (`get_function_spaces_with_options`, `get_ops`). Behaviour is
  unchanged for UTF-8 paths; for a non-UTF-8 path the emitted top-level
  name is now empty rather than a lossy-mangled (`U+FFFD`) rendering of
  the path bytes. Part of the `Ast`-seam unification (#566/#568); the
  shims themselves are removed in the 2.0 surface reshape (#570).
- **(breaking, deferred to 2.0)** Unified the two parallel metric enums:
  suppression now reuses the `Metric` enum and `MetricKind` is removed from
  the public API. `Metric` gains canonical-spelling serde (`nargs` /
  `nexits`, not `n_args`) and declaration-order `Ord`; the suppressed-scope
  serialization uses canonical names (`nexits`, not `exit`) and the
  `nexits→exit` alias bridge is gone; `tokens` is non-suppressible
  (rejected with a clear error). Suppression parsing now surfaces the
  offending token via `ParseMetricError` instead of `Err = ()`, closing
  #554 (#555, #554).
- **(breaking, deferred to 2.0)** `Node`'s inner `tree_sitter::Node` is no
  longer a `pub` tuple field; reach it via the new
  `Node::as_tree_sitter(&self) -> tree_sitter::Node<'a>` accessor
  (value-not-stable, mirroring `Ast::as_tree_sitter`) (#556).
- **(breaking, deferred to 2.0)** Marked the remaining open public enums
  `#[non_exhaustive]` (`Severity`, `SpaceKind`, `SuppressionDialect`);
  documented the deliberately-closed suppression enums (`SuppressionPolicy`,
  `SuppressionScope`, `SuppressionTarget`) (#551).
- **(breaking)** Marked every per-metric compute-side `Stats` struct
  (`abc`, `cognitive`, `cyclomatic`, `halstead`, `loc`, `mi`, `nargs`,
  `nexits`, `nom`, `npa`, `npm`, `wmc`, `tokens`) `#[non_exhaustive]`.
  Their fields were already private and read through accessors, so the
  marker is observationally invisible to existing callers; it makes the
  "no external struct-literal construction, no exhaustive match"
  guarantee explicit and keeps a future field addition additive within
  `2.x` rather than a shape break deferred to `3.0`.
- **(breaking, deferred to 2.0)** `ConcurrentErrors` is now
  `#[non_exhaustive]` and its `Sender` / `Thread` variants carry a boxed
  `std::error::Error + Send + Sync` source instead of a `String` (so
  `source()` chains); `Producer` / `Receiver` remain message-only (their
  cause is a thread-panic payload, not an `Error`) (#553).
- **(breaking, deferred to 2.0)** The `/comment` endpoint now returns `200`
  with a uniform empty payload across both content types for the "no
  comments" outcome — JSON returns `{code: []}` and octet-stream returns
  `200` with an empty body, replacing the former octet-stream `204 No
  Content` (#558).
- CBOR output (`bca metrics --format cbor`) now serializes via
  `ciborium` instead of the unmaintained `serde_cbor`
  (RUSTSEC-2021-0127). Output remains valid CBOR; no public API or
  CLI change.
- **(breaking)** Serialized AST node output (`AstNode`, REST `/ast`,
  `AstCallback`) now uses snake_case keys `type` / `value` / `span` /
  `field_name` / `children` (was `Type` / `TextValue` / `Span` /
  `FieldName` / `Children`); `TextValue` is renamed to `value`. `Span`
  changes from a bare `(usize, usize, usize, usize)` tuple to a named
  object `{start_row, start_col, end_row, end_col}` (still `Option`,
  `null` for root / span-disabled nodes); field order and 1-based
  row/column values are unchanged. Deferred to the next major bump
  (#535).
- **(breaking)** `Metric::Exit` renamed to `Metric::Nexits`; its
  `Display` is now `"nexits"` and `Metric::NAMES` lists `nexits`,
  matching the `nargs`/`nom`/`npa`/`npm` "number-of" family. The CLI
  accepts `nexits` canonically with `exit` kept as a hidden parse alias
  for one cycle. The serialized field and JSON key were already `nexits`,
  so output is unchanged. Deferred to the next major bump (#536).
- **(breaking)** Removed the never-produced `MetricsError::NonUtf8Path`
  and `MetricsError::ParseHasErrors` variants (the enum stays
  `#[non_exhaustive]`, so a future strict mode can re-add them).
  `EmptyRoot` is retained — it is constructed at live forward-compat
  guards. Deferred to the next major bump (#536).
- **(breaking)** `FunctionSpan.name` is now `Option<String>` and the
  `error: bool` field was removed; an unresolved name is `None`
  (serialized `null`), matching `FuncSpace`/`Ops`. The wire DTO and the
  REST `/function` JSON shape are updated accordingly. Deferred to the
  next major bump (#536).
- **(breaking)** `CountCfg` and `FindCfg` no longer expose
  `Arc<Mutex<Count>>` / `Arc<[String]>` in their public fields.
  `CountCfg.stats` is now an opaque `CountCollector`
  (`CountCollector::new()`, `into_count()`); `CountCfg.filters` and
  `FindCfg.filters` are now an opaque `NodeTypeFilters`
  (`NodeTypeFilters::new(&[String])` / `From<Vec<String>>`, borrowed
  `as_slice()`). Both newtypes are re-exported from the crate root.
  Deferred to the next major bump (#537).
- **(breaking)** `bca exemptions`: section filters renamed to the
  `--<section>-only` idiom (`--markers-only` / `--excludes-only` /
  `--baseline-only`), matching `diff-baseline`. The old `--only-*`
  spellings remain as hidden aliases for one release cycle. Deferred to
  the next major bump (#538).
- **(breaking)** CLI excludes now merge with the manifest. `--exclude` /
  `--check-exclude` (and their `*-from` files) UNION with the `bca.toml`
  `exclude` / `[check] exclude` lists instead of replacing them, so a
  command-line filter can no longer silently un-exclude a directory the
  project config skipped. Positive scope keys (`paths`, `include`) still
  replace on a CLI value; `--no-config` still bypasses the manifest.
  Deferred to the next major bump (#539).
- **(breaking)** `LANG::name`/`Display`/`FromStr` now use one canonical
  lowercase slug per language; the pretty `c/c++` / `c#` display forms
  are dropped and `Tsx` reports `tsx`. The serialized `language` value
  (CLI JSON, web `/metrics`, Python) changes accordingly and is now
  always a valid `FromStr` lookup token. Deferred to the next major bump
  (#540).
- **(breaking)** `bca-web`: all error responses (including
  octet-stream/plain endpoints and the 415/405/404 fallbacks) now return
  a uniform JSON body `{"error", "id"}` with the correct status,
  replacing the former bare `text/plain` bodies. Deferred to the next
  major bump (#541).
- **(breaking)** `bca-web`: `/v1/function` and `/v1/comment` responses
  now include `id` and the detected `language` (canonical slug),
  matching the `/v1/metrics` envelope. Deferred to the next major bump
  (#541).
- **(breaking)** `bca-web`: the `unit` query flag on `/v1/metrics` now
  uses normal boolean semantics (`true`/`false`/`1`/`0`,
  case-insensitive); other values (including `yes`/`on`) return HTTP 400.
  Deferred to the next major bump (#541).
- **(breaking)** Python: `analyze_batch`'s `skip_generated` default
  flips to `True`, aligning with single-file `analyze`;
  `supported_languages()` now returns `list[Lang]` and `METRIC_NAMES` a
  `tuple[MetricName, ...]` (values remain string-compatible). Deferred to
  the next major bump (#542).
- **(breaking)** Tidied internal-plumbing visibility. `Cursor`
  (`src/node.rs`) is narrowed from `pub` to `pub(crate)` and dropped from the
  `lib.rs` re-exports: every one of its methods was already `pub(crate)`, so the
  re-exported type could be named but never used. `Callback` and `LanguageInfo`
  gain `#[doc(hidden)]` to match `ParserTrait` (`Callback::call` is bound on the
  hidden `ParserTrait`, and `LanguageInfo` is reachable from documented API only
  through the hidden `Parser`), so the bound and the trait now have coherent
  visibility; they remain `pub` for the `action::<T>` dispatcher and the
  in-crate / `bca-web` `impl Callback` blocks, so only their rustdoc presence
  changes. `Node` stays `pub` — the doc-hidden `ParserTrait::root` returns it,
  and it carries a genuine public method (`has_error`); `Ast::as_tree_sitter` is
  the preferred higher-level raw-tree seam. Removing `Cursor` from the public
  surface is SemVer-breaking; **deferred to the `2.0.0` release** (the
  release-prep commit moves this entry into the `2.0.0` section). The
  `#[doc(hidden)]` additions are not themselves SemVer-breaking.
  ([#534](https://github.com/dekobon/big-code-analysis/issues/534), part of
  [#505](https://github.com/dekobon/big-code-analysis/issues/505))

- **(breaking)** The builder types `Source`, `MetricsOptions`, and
  `MetricsCfg` no longer expose `pub` fields — they are narrowed to
  `pub(crate)`. These types are already documented as "construct via `new` +
  `with_*` setters" and carry `#[non_exhaustive]`; the `pub` fields only froze
  the internal representation (e.g. `Source::code: &[u8]`, `Source::name:
  String`) as API for no benefit. Construction is unchanged
  (`Source::new(...).with_*(...)`, `MetricsOptions::default().with_*(...)`,
  `MetricsCfg::new(...).with_options(...)`); only direct field reads break, and
  the builders cover every supported use. No accessors were added — no consumer
  needs to read the config back. SemVer-breaking for code that read the fields
  directly; **deferred to the `2.0.0` release** (the release-prep commit moves
  this entry into the `2.0.0` section).
  ([#533](https://github.com/dekobon/big-code-analysis/issues/533), part of
  [#505](https://github.com/dekobon/big-code-analysis/issues/505))

- **(breaking)** Non-finite float metric values (`NaN`/`±Infinity`) now
  serialize as a null uniformly across every structured format, enforced once
  at the serialize boundary via an internal `NonFinite` float wrapper rather
  than relying on each accessor staying finite. A non-finite value renders as a
  native `null` in JSON, YAML, and CBOR, and as an omitted key in TOML (which
  has no null literal). This replaces the previous per-format divergence — JSON
  silently emitted `null`, TOML `nan`, YAML `.nan`, and CBOR the raw IEEE-754
  bits — so YAML/TOML/CBOR consumers of a non-finite field see a changed shape;
  JSON is unchanged. The structured serializers also explicitly commit to
  **full `f64` precision**, documented in [STABILITY.md](./STABILITY.md) as not
  byte-stable across versions/platforms (the human-readable `bca check` warning
  path keeps its own six-decimal rounding, intentionally distinct from machine
  output). Finite values — every value the guarded metric accessors produce
  today (#428, #438, the Halstead/MI `log`/division guards) — serialize
  byte-identically to before, so this is a structural backstop with no
  observable change for current metrics. SemVer-breaking shape change to the
  serialized output, **deferred to the `2.0.0` release** (the release-prep
  commit moves this entry into the `2.0.0` section).
  ([#531](https://github.com/dekobon/big-code-analysis/issues/531), part of
  [#505](https://github.com/dekobon/big-code-analysis/issues/505))

- **(breaking)** Integer-valued metrics now serialize as integers instead of
  floats, and their public `Stats` accessors return `u64` instead of `f64`.
  Affected: every count, sum, and min/max (cyclomatic, cognitive, exit, nargs,
  nom, tokens, loc lines, ABC assignments/branches/conditions, npa/npm
  attribute/method counts), Halstead `length`/`vocabulary` and the four
  operator/operand counts, and all three WMC values. Ratios, averages, ABC
  `magnitude`, the derived Halstead scores (`volume`, `difficulty`, `level`,
  `effort`, `time`, `bugs`, `purity_ratio`, `estimated_program_length`), and the
  MI scores remain `f64`. JSON/TOML/YAML now emit `"sloc": 5` rather than
  `"sloc": 5.0`, CBOR encodes these fields as compact integers rather than
  float64, and CSV output is unchanged (it already rendered integral values
  without a trailing `.0`). No metric *value* changes — only its type and
  representation. This is a SemVer-breaking shape change to the serialized
  output and the library accessor signatures; it is **deferred to the `2.0.0`
  release** (the release-prep commit moves this entry into the `2.0.0` section).
  ([#530](https://github.com/dekobon/big-code-analysis/issues/530), part of
  [#505](https://github.com/dekobon/big-code-analysis/issues/505))

- Internal refactor of the crate-private `Checker` classification trait
  (a `pub(crate)` extension point, not part of the public API or the
  [STABILITY.md](./STABILITY.md) shape contract) so that adding a language
  no longer means copy-pasting `-> false` stubs. The ten predicates
  (`is_comment`, `is_useful_comment`, `is_func_space`, `is_func`,
  `is_closure`, `is_call`, `is_non_arg`, `is_string`, `is_else_if`,
  `is_primitive`) now carry `-> false` defaults, so a language implements
  only the categories its grammar expresses (~150 boilerplate lines removed
  across the 22 impls). `is_primitive` now takes `&Node` instead of a bare
  `u16`, matching every other predicate and removing the "two same-typed
  primitives" footgun, and `Node::count_specific_ancestors` is bound on
  `Checker` rather than the full `ParserTrait`. No public-API or
  metric-output change — this is internal plumbing only and the serialized
  metrics are byte-identical
  ([#520](https://github.com/dekobon/big-code-analysis/issues/520),
  part of [#505](https://github.com/dekobon/big-code-analysis/issues/505)).
- The `bca` line-range flags are now **scoped to the `dump` and `find`
  subcommands** instead of being `global`, and gain descriptive long
  names: `--line-start` / `--line-end` (canonical) with `--ls` / `--le`
  kept as hidden, deprecated aliases for one release cycle. Previously
  the flags were advertised on every subcommand's help even though only
  `dump`/`find` consumed them, and passing e.g. `bca metrics --ls 5`
  was silently ignored; that invocation — and the pre-existing
  flag-*before*-subcommand form `bca --ls 5 dump` — now errors. The new
  form puts the flag after the subcommand: `bca dump --line-start 5
  --line-end 10`. The order change and the eventual removal of the
  `--ls`/`--le` aliases are **(breaking)** and deferred to the next
  major bump
  ([#518](https://github.com/dekobon/big-code-analysis/issues/518),
  part of [#505](https://github.com/dekobon/big-code-analysis/issues/505)).
- `bca-web` REST routes are now versioned under a `/v1` prefix
  (`/v1/ast`, `/v1/comment`, `/v1/metrics`, `/v1/function`, `/v1/ping`).
  The original unprefixed paths remain available as **deprecated
  aliases** for one release cycle and resolve to the same handlers, so
  existing clients keep working; new clients should adopt the `/v1`
  paths. The known-endpoint set is no longer mirrored in a
  hand-maintained `GUARDED_POST_PATHS` constant — each resource carries
  its own `default_service`, so a request that reaches a known endpoint
  but matches no route is answered with a diagnostic `415`/`405` by the
  resource itself (a new endpoint can never silently regress to a
  bodyless `404`), and a genuinely unknown URL falls through to the
  app-level `404`. A side effect: `POST /ping` now returns `405` (was a
  bodyless `404`). Additionally, errors are no longer signalled inside a
  `200` body: the metrics endpoint's `spaces` field is now a
  non-optional `FuncSpace` (a successful response is byte-identical to
  before), and metric-computation / AST-construction failures now return
  `500 Internal Server Error` with an error body rather than `200` with
  `spaces`/`root` = `null`
  ([#517](https://github.com/dekobon/big-code-analysis/issues/517),
  part of [#505](https://github.com/dekobon/big-code-analysis/issues/505)).

- `bca-web` now logs server-side events via `tracing` instead of
  unstructured `eprintln!`: parse failures at `error!` and parse timeouts
  at `warn!`, each with a structured `payload_id` field taken from the
  request payload's `id`. It also wires `tracing-actix-web`'s
  `TracingLogger` middleware for per-request spans (one access-log line
  per completed request, with its own `request_id` UUID, method, route,
  status, and latency). Log level and output are controlled by the
  `RUST_LOG` environment variable (default `info`). HTTP responses are
  byte-for-byte unchanged — this is server-side observability only
  ([#516](https://github.com/dekobon/big-code-analysis/issues/516),
  part of [#505](https://github.com/dekobon/big-code-analysis/issues/505)).

- Unified output-format selection across every `bca` subcommand
  ([#513](https://github.com/dekobon/big-code-analysis/issues/513),
  part of [#505](https://github.com/dekobon/big-code-analysis/issues/505)).
  `--format` (short `-O`) is now the canonical spelling everywhere:
  - `metrics` / `ops` / `check` gain the long `--format` spelling;
    their previous `--output-format` is kept as a hidden, deprecated
    alias.
  - `report` gains a `--format` / `-O` flag and now **defaults to
    `markdown`** when no format is given (previously a missing
    positional was an error). The bare positional form
    (`bca report markdown`) is kept working as a hidden, deprecated
    alias; the `--format` flag wins when both are supplied.
  - `diff` / `diff-baseline` / `exemptions` gain the `-O` short for
    their existing `--format` flag.
  - These additions are backward-compatible. Removal of the deprecated
    `--output-format` alias and the bare `report` positional is
    **(breaking)** and deferred to the next major bump.
- Unified the "average over a count" divisor convention and its
  divide-by-zero guard across the metric suite, and **re-baselined the
  cyclomatic averages** as part of the `2.0` re-baseline
  ([#512](https://github.com/dekobon/big-code-analysis/issues/512),
  part of [#505](https://github.com/dekobon/big-code-analysis/issues/505)).
  - A single shared `average(sum, count)` helper now applies the `.max(1)`
    divisor guard (added for
    [#428](https://github.com/dekobon/big-code-analysis/issues/428)) for
    every metric average instead of repeating it per call site. This
    removes the former reliance on a counter that merely defaulted to `1`
    for `cyclomatic`, `nom`, and the previously-unguarded per-space
    averages (`loc`, `abc`, `tokens`). Behaviour-preserving for every
    metric except `cyclomatic` (below): the guarded divisor is identical
    whenever the count is already non-zero.
  - **Metric values change for `cyclomatic.average` and
    `cyclomatic.modified.average` only.** They are now **per function**:
    the divisor is the number of function/closure *spaces* in the subtree
    — the per-function convention `cognitive` / `exit` / `nargs` use —
    rather than the previous per-space count (which also divided by
    classes, structs, and the file unit and so reported a smaller
    average). Files with classes/structs/units see a larger average.
    `cyclomatic.sum` / `min` / `max` and every other metric — including
    the Maintainability Index and WMC, which consume the cyclomatic
    *sum* — are unchanged. (The divisor counts the spaces that each carry
    a cyclomatic value, so it matches `cognitive`'s function/closure count
    wherever every closure opens its own space; a closure form that opens
    no space, such as a Python `lambda`, is counted by `cognitive` but not
    as a separate cyclomatic divisor unit.)
  - The divisor is sourced from the space kind during finalization, not
    from the `Nom` metric, so a `cyclomatic`-only metric selection still
    divides per function without pulling a `nom` block into the output.
  - `nom`'s own averages stay **per space** (it is the count metric;
    a per-function divisor would be circular).

- `get_ops`, `metrics_from_tree`, and the doc-hidden
  `operands_and_operators` are now `#[deprecated]` in favour of the
  explicit-name `Ast` seams (`Ast::ops`, `Ast::from_tree_sitter`), which
  carry `name: Option<String>` from `Source` end-to-end. The shims keep
  their previous lossy-path behaviour (the lossy UTF-8 conversion now lives
  only in the deprecated path-positional shims; the shared walk core takes
  an explicit name), so existing callers see no behaviour or output change.
  This completes the `Source`/`Ast` migration begun for the metrics family
  in [#254](https://github.com/dekobon/big-code-analysis/issues/254);
  removal is deferred to the `2.0.0` bump
  ([#509](https://github.com/dekobon/big-code-analysis/issues/509),
  part of [#505](https://github.com/dekobon/big-code-analysis/issues/505)).

- **(breaking)** Normalized the public language-dispatch surface
  (deferred to the `2.0.0` bump;
  [#507](https://github.com/dekobon/big-code-analysis/issues/507)):
  - Dropped the Java-style `get_` prefix from every language getter, per
    the Rust C-GETTER guideline: `LANG::get_name` → `name`,
    `get_tree_sitter_language` → `tree_sitter_language`, `get_extensions`
    → `extensions`; `LanguageInfo::get_lang` / `get_lang_name` → `lang` /
    `lang_name`; `ParserTrait::get_language` / `get_root` / `get_code` /
    `get_filters` → `language` / `root` / `code` / `filters`;
    `Parser::get_ts_tree` → `ts_tree`.
  - The dispatchers `action`, `get_function_spaces`,
    `get_function_spaces_with_options`, `metrics_from_tree`, and `get_ops`
    now take `lang: LANG` by value instead of `&LANG` (`LANG` is a `Copy`
    1-byte enum, so the reference was pointless indirection). Call sites
    pass `LANG::Rust`, not `&LANG::Rust`.
  - Rename + signature only; no serialized output or metric values change.
- **(breaking)** The default JavaScript grammar is now the upstream
  `tree-sitter-javascript`, not the vendored Mozilla `tree-sitter-mozjs`
  fork (the project is no longer Mozilla-driven;
  [#507](https://github.com/dekobon/big-code-analysis/issues/507)):
  - `LANG::Javascript` (upstream grammar) is the default for `.js`, `.mjs`,
    `.cjs`, and `.jsx`, and is declared first in the language list. `.cjs`
    (CommonJS) is **newly recognized** — it was previously unmapped.
  - `LANG::Mozjs` (the Mozilla/SpiderMonkey fork) is now opt-in: it owns
    only the `.jsm` (Firefox module) extension and its display name changed
    from `javascript` to `mozjs`, so `.jsm` files report
    `"language": "mozjs"`. Select the fork explicitly via `LANG::Mozjs`.
  - The two grammars are metric-equivalent on real-world JavaScript (the
    fork only adds SpiderMonkey-specific node types absent from ordinary
    code), so **no metric values change** for `.js` / `.jsx` / `.mjs`
    files and no snapshots were re-baselined — verified against the full
    integration corpus (385 `.js` snapshots) plus an independent sample.
  - Builds that enable the `mozjs` feature but **not** `javascript` no
    longer analyze `.js` files (they resolve to the now-disabled
    `Javascript` variant and return `LanguageDisabled`); default
    `all-languages` builds are unaffected.
- **(breaking)** Normalized the serialized metric output keys for a
  coherent 2.0 data contract (deferred to the `2.0.0` bump;
  [#510](https://github.com/dekobon/big-code-analysis/issues/510),
  [#511](https://github.com/dekobon/big-code-analysis/issues/511)).
  Affects the JSON / YAML / TOML / CBOR / CSV output and the `bca dump`
  metric tree:
  - `halstead`: `n1`/`N1`/`n2`/`N2` → `unique_operators` /
    `total_operators` / `unique_operands` / `total_operands` (the
    case-only-distinct keys collided for case-insensitive CSV/env
    consumers).
  - `mi`: leaves drop the redundant `mi_` prefix — `mi_original` /
    `mi_sei` / `mi_visual_studio` → `original` / `sei` /
    `visual_studio` (now equal to the `mi.*` threshold ids).
  - `nargs`: `total_functions` / `total_closures` → `function_args` /
    `closure_args`; `average_functions` / `average_closures` →
    `function_args_average` / `closure_args_average`; the
    `functions_*` / `closures_*` min/max keys gain the `_args` infix.
    Removes the `total_functions` sum-vs-count name collision and the
    adjective-order disagreement with `nom`.
  - `npa` / `npm`: the `classes_average` / `interfaces_average` /
    `average` keys carried CDA/COA accessibility *ratios*, not
    averages, and are renamed `class_cda` / `interface_cda` / `cda`
    (npa) and `class_coa` / `interface_coa` / `coa` (npm).
  - `abc.magnitude` is documented as a derived roll-up with no
    min/max/average projection (it is not accumulated per space).
  - Metric *values* are unaffected — this is a key-shape change only.
    (The separate per-function divisor re-baseline, #512, is deferred
    to its own change so it can be made self-contained rather than
    coupling `cyclomatic` to `nom`.)
- `guess_language` now returns `(Option<LANG>, &'static str)` instead of
  `(Option<LANG>, &'a str)` with an unbound output lifetime, making the
  honest type explicit and removing a latent-unsoundness trap (every return
  path was already `&'static`). Source-compatible for normal callers
  (return-lifetime widening is covariant)
  ([#506](https://github.com/dekobon/big-code-analysis/issues/506)).
- perf(node): `has_sibling` no longer heap-allocates a `TreeCursor` per
  call — it reuses the allocation-free sibling walk introduced in #217,
  eliminating the missed allocation on the JS/TS arrow-function
  closure-classification hot path
  ([#521](https://github.com/dekobon/big-code-analysis/issues/521)).
- perf(spaces): the AST walker now computes a node's space kind lazily —
  only when the node is promoted to a function space or the `Loc` metric is
  selected — avoiding a wasted per-node source-text scan (notably Elixir's
  per-`Call` keyword scan) when the result would go unused. No metric
  values change
  ([#522](https://github.com/dekobon/big-code-analysis/issues/522)).
- refactor(node): `Node::children()` drives termination off the cursor
  alone (struct iterator `Children`), eliminating latent duplicate-node
  padding if `child_count()` and the cursor sibling walk ever desync;
  `ExactSizeIterator` retained, no metric-value or public-API change
  ([#523](https://github.com/dekobon/big-code-analysis/issues/523)).
- build(deps): exact-pin `tree-sitter-kotlin-ng` to `=1.1.0` to match every
  sibling grammar, and guard the root vs `enums/` external grammar-pin
  lockstep via `check-versions.py` so future drift fails fast in
  pre-commit / CI (resolved version unchanged)
  ([#524](https://github.com/dekobon/big-code-analysis/issues/524)).
- build(deps): drop the `num` meta-crate from the library's direct
  dependencies (its sole use, `num::FromPrimitive::from_u16`, now goes
  through the already-present `num-traits` re-export) and hoist `csv` /
  `tempfile` into `[workspace.dependencies]`; no behavioral change
  ([#525](https://github.com/dekobon/big-code-analysis/issues/525)).

- Python bindings: `lang_to_name` now delegates to `LANG::get_name()`
  for all but three lookup-token overrides (`Cpp` → `"cpp"`, `Csharp` →
  `"csharp"`, `Tsx` → `"tsx"`), collapsing a 22-arm hand-maintained
  table that duplicated the upstream CLI display names. The Python-facing
  `language` identifiers are byte-identical for every variant; this only
  removes drift risk between the facade and the CLI display names
  ([#500](https://github.com/dekobon/big-code-analysis/issues/500)).
- **(breaking)** `FilesData` and `ConcurrentRunner` are reshaped into a
  terminal file-set processor: `FilesData` drops its `include` /
  `exclude` `GlobSet` fields (now just `FilesData { paths }`),
  `ConcurrentRunner::run` returns `Result<(), ConcurrentErrors>` instead
  of `Result<HashMap<String, Vec<PathBuf>>, ConcurrentErrors>`, and the
  `set_proc_dir_paths` / `set_proc_path` builder methods are removed.
  The library previously re-walked and re-filtered the file list the
  CLI had already resolved and anchored (#489), causing a redundant
  per-file `stat` and dead, path-form-sensitive globsets. The library
  is now a pure concurrent processor of an already-resolved file list;
  the CLI's anchored, gitignore-aware `expand_seed_paths` is the single
  walk and filtering seam. This is a source-level break **deferred to
  the next major (`2.0`)** bump; the release-prep commit moves this
  entry into the `2.0.0` section
  ([#495](https://github.com/dekobon/big-code-analysis/issues/495)).
- The project's own self-scan gate now reads all configuration (paths,
  exclude_from, baseline, thresholds, and the cyclomatic-`?` policy)
  from a single consolidated `bca.toml` manifest; the standalone
  `bca-thresholds.toml` and the redundant `BCA_COUNT_CYCLOMATIC_TRY`
  Makefile plumbing are retired, so `bca check` reproduces the gate
  with no flag threading
  ([#483](https://github.com/dekobon/big-code-analysis/issues/483)).
- `bca init` now scaffolds a consolidated `bca.toml` manifest
  (auto-discovered zero-config) instead of the retired
  `bca-thresholds.toml` three-file split; `.bcaignore` and
  `.bca-baseline.toml` are still written
  ([#484](https://github.com/dekobon/big-code-analysis/issues/484)).
- CI: removed the hand-translated `.bcaignore` mirror regex from the
  `bca-self-scan` / `bca-self-scan-headroom` pre-commit hooks;
  `.bcaignore` is now the single source for the self-scan deny-set
  ([#485](https://github.com/dekobon/big-code-analysis/issues/485)).
- Cognitive complexity now applies the SonarSource §B2 jump-statement
  rule uniformly across languages: an *unstructured* jump (labeled
  `break`/`continue`, `goto`) adds +1 while a plain unlabeled
  `break`/`continue` adds +0. Previously this was inconsistent in both
  directions. The JS family (JavaScript/TypeScript/TSX/mozjs) now
  counts labeled `break LABEL` / `continue LABEL` (+1, gated on the
  `statement_identifier` label child); PHP now counts `goto label;`
  (+1). Conversely, Ruby no longer counts plain `break`/`next` (Ruby
  has no labeled loops, so these are always unlabeled → +0; `redo` and
  `retry` remain +1 as genuinely unstructured jumps), and Lua no longer
  counts plain `break` (Lua has no labeled break → +0; `goto label`
  remains +1). PHP's numeric `break N;` / `continue N;` stays +0 — it
  is a structured loop-level exit whose enclosing loops are already
  counted via nesting. This raises published cognitive (and the derived
  MI) values for JS/TS/PHP code using labeled jumps or `goto`, and
  lowers them for Ruby/Lua code using plain `break`/`next`, so cognitive
  scores are now comparable across languages.
  Fixes [#435](https://github.com/dekobon/big-code-analysis/issues/435).
- Cyclomatic complexity now counts the safe-navigation operator as a
  decision point for Kotlin (`?.`, `QMARKDOT`) and PHP (`?->`,
  `QMARKDASHGT`), matching the existing JS/TS/C# treatment of `?.`
  ([#281](https://github.com/dekobon/big-code-analysis/issues/281)).
  Each occurrence adds +1 to both standard and modified cyclomatic
  (a chain `a?.b?.c` adds +2). Matching the operator *token* — rather
  than the wrapper node — counts each operator exactly once across PHP's
  `nullsafe_member_access_expression` and
  `nullsafe_member_call_expression` forms, and across Kotlin's
  `navigation_expression`. This raises published cyclomatic (and the
  derived MI) values for Kotlin/PHP code that uses safe navigation, so
  metrics are now comparable across these languages.
  Fixes [#436](https://github.com/dekobon/big-code-analysis/issues/436).
- `bca init` now scaffolds `bca-thresholds.toml` with `loc.sloc = 800`
  (was `300`). File-level SLOC counts inline `#[cfg(test)]` tests,
  comments, and blank lines, so the old limit sat below the median
  source file and flagged ordinary well-documented modules rather than
  genuinely oversized ones; `800` better reflects a healthy Rust file
  ceiling (inline tests inflate file SLOC 2-3x). The scaffold tracks the
  project's own gate, now pinned by a drift test so the two cannot
  silently diverge. `init` still refuses to overwrite an existing
  `bca-thresholds.toml`, so only newly-scaffolded files are affected.
- Python's hidden `block` / `lambda` kind-id aliases are now normalized
  behind a single `python_is_block` helper, and `is_closure` accepts the
  currently-unseen `Lambda2` alias, with drift-guard tests mirroring the
  `Php::String3` / `Java::MultilineStringLiteral` guards. Defensive
  refactor; no metric output changes. Fixes
  [#419](https://github.com/dekobon/big-code-analysis/issues/419).
- Completed the #419 Python lambda-alias normalization in the cognitive
  metric: the three `impl Cognitive for PythonCode` lambda sites (the two
  boolean-operator ancestor-scope walks and the lambda-nesting dispatch
  arm) now recognize the `Lambda2` (197) hidden alias, not just `Lambda`
  (196). Added a single `cognitive::python_is_lambda` chokepoint reused by
  those sites and by `is_closure` (mirroring `python_is_block`), so the
  closure and cognitive lambda detection can no longer desync. Defensive
  refactor; `Lambda2` is unemitted by the current grammar pin, so there is
  no metric output change. Fixes
  [#422](https://github.com/dekobon/big-code-analysis/issues/422).
- The per-language Halstead string-interpolation operand skip (a literal
  is one operand unless it wraps interpolation, in which case the wrapper
  yields `Unknown` and the inner expressions are counted) is unified
  behind a `Getter::string_operand_type` default plus a `Node::wraps_any`
  primitive, retiring the two bespoke Tcl/PHP helpers and nine duplicated
  sites. No metric values change. Fixes
  [#420](https://github.com/dekobon/big-code-analysis/issues/420).
- The AST-dump renderer (`bca dump`) is refactored internally: the
  monolithic `dump_tree_helper` (cyclomatic 32, nexits 20, nargs 8)
  is split into a state struct plus single-purpose helpers
  (`branch_glyphs`, `line_in_range`, `paint`, `write_node_line` /
  `_header` / `_location` / `_snippet`, `dump_children`), each well
  under the per-function thresholds. **Output is byte-for-byte
  identical** — no public API, CLI, or dump-format change; a new
  byte-exact regression test (`dump_output_matches_expected_tree`)
  plus unit tests for the extracted predicates pin the behavior.
  Note: most of the original cyclomatic/nexits score was Rust's `?`
  operator (each counts as a `TryExpression` decision point), not
  genuine branching — see
  [#401](https://github.com/dekobon/big-code-analysis/issues/401).
- `tree-sitter-mozjs` is regenerated against its declared
  `tree-sitter-javascript` `0.25.0` base grammar (with `tree-sitter`
  CLI `0.26.9`), and its floating `tree-sitter-cli` `^0.25.3`
  devDependency is pinned to `0.26.9`. Investigation for
  [#407](https://github.com/dekobon/big-code-analysis/issues/407)
  found the bundled mozjs parser was **stale at JS `0.23.1`**: the
  `0.23.1` → `0.25.0` marker bump (#1207) shipped without the
  matching regen, and #400 then pinned the grammar-marker-sync
  baseline at `0.25.0` on the incorrect belief that the regen was a
  no-op. The real `0.25.0` regen is **not** a no-op — it adds the
  `using` / `await using` explicit-resource-management declaration
  (`using_declaration`), so the generated `Mozjs` node-kind enum in
  `language_mozjs.rs` gains `Using` and `UsingDeclaration` variants
  (the pre-existing `switch_default` node is renumbered, not added).
  The bump
  is **metric-neutral for the existing fixture corpus** (no fixture
  uses a `using` declaration and the parse-table renumbering does not
  change parse results for existing constructs), so no snapshot or
  `big-code-analysis-output` integration movement. The new capability
  is pinned by a drift-marker test
  (`checker::tests::mozjs_parses_using_declaration`) that fails
  against the pre-#407 parser. The grammar-marker-sync baseline stays
  at `0.25.0` (now honestly matching the bundled sources) with its
  history comment corrected.
- `tree-sitter-ccomment` and `tree-sitter-preproc` regeneration is
  now reproducible: their floating `tree-sitter-cli` `^0.25.3`
  devDependency is pinned to `0.26.9` and the bundled `src/parser.c`
  is regenerated with that CLI (previously stamped `0.25.3`),
  mirroring the #406 mozcpp fix. Both are leaf grammars (no upstream
  base grammar to pin) and the regen is **metric-neutral**:
  `grammar.json` and `node-types.json` are byte-identical, only the
  `parser.c` version-stamp comment changes, `parser.h` drops the
  redundant `TSLanguageMetadata` forward declaration, and
  `src/tree_sitter/array.h` advances to the 0.26.9 strict-aliasing
  runtime template. Resolves the sibling reproducibility gap tracked
  in [#407](https://github.com/dekobon/big-code-analysis/issues/407).
- `tree-sitter-mozcpp/src/tree_sitter/array.h` is advanced to the
  0.26.9 strict-aliasing runtime template, closing a #406 gap: that
  regen updated `parser.c` (version stamp) and `parser.h` (forward
  declaration) to 0.26.9 form but left `array.h` at the pre-0.26
  layout, leaving the crate internally inconsistent and a bare regen
  non-reproducible. The header is byte-identical to the
  ccomment/preproc/mozjs runtime template. Metric-neutral (a runtime
  memory helper; does not affect parsing).
- `tree-sitter-tcl` regeneration is documented in its `Cargo.toml`:
  it ships no `grammar.js`/`package.json`, so there is no local
  `tree-sitter generate` path and no floating `tree-sitter-cli` axis
  to pin. The committed `src/` is the source of truth; updating it
  means re-vendoring from upstream (a deliberate grammar-version
  change, not a reproducibility regen). No code change — confirms the
  open question in [#407](https://github.com/dekobon/big-code-analysis/issues/407).
- Internal refactor of the CLI HTML report renderer
  (`big-code-analysis-cli/src/html_report.rs`) to relieve the
  self-scan parameter-count pressure on `emit_hotspot`,
  `generate_html_report`, and `write_language_section`
  ([#402](https://github.com/dekobon/big-code-analysis/issues/402)).
  The nine hotspot sections are now described by `const HotspotSpec`
  tables of `fn`-pointer column projectors; `emit_hotspot` drops from
  ten parameters (four of them closures) to five, and the two large
  orchestrators are flattened into named per-block helpers
  (`group_by_language`, `GlobalTotals`, `write_overview_table`,
  `partition_by_kind`, `CyclomaticStats`, `ActionableCounts`, …).
  Output is byte-for-byte identical — the `html_report_two_lang`
  snapshot and every `assert_html_well_formed` test are unchanged.
  No public API or CLI behaviour change.
- `tree-sitter-mozcpp` grammar regeneration is now reproducible and
  the bundled `src/parser.c` is regenerated with `tree-sitter` CLI
  `0.26.9`, matching the workspace's `tree-sitter = "=0.26.9"`
  runtime (the bundled parser was previously generated with CLI
  `0.25.3`). Two floating toolchain pins that made the regen
  non-deterministic are now exact: `tree-sitter-cli` in
  `tree-sitter-mozcpp/package.json` (`^0.25.3` → `0.26.9`) and the
  upstream `tree-sitter-c` base grammar that `tree-sitter-cpp`
  extends, pinned to `=0.23.1` in `generate-grammars/generate-mozcpp.sh`.
  Investigation for [#406](https://github.com/dekobon/big-code-analysis/issues/406)
  confirmed the regen is **metric-neutral**: `grammar.json` and
  `node-types.json` are byte-identical to the previous output and the
  parse table in `parser.c` is unchanged (only the version-stamp
  comment differs), so no metric values shift and the
  `big-code-analysis-output` integration snapshots are unchanged. The
  `tree-sitter-cpp = "0.23.4"` grammar marker (and the
  grammar-marker-sync baseline from #400) are unchanged — this is a
  codegen-toolchain alignment, not a grammar-version bump. Also
  hardens the grammar-regeneration scripts: `generate-mozcpp.sh` and
  the shared `generate-grammar.sh` (which runs the actual
  `tree-sitter generate`) now run under `set -euo pipefail`, so a
  failed download / install / fetch / generate aborts the run
  instead of falling through to `cargo test` against the unchanged
  parser and reporting a no-op regen as success. Fixes a latent bug
  in both `generate-mozcpp.sh` and `generate-mozjs.sh` where the
  crates.io download used a bare `wget` that now receives HTTP 403
  (crates.io requires a User-Agent), and documents that the
  npm-distributed tree-sitter CLI 0.26.9 needs glibc ≥ 2.39 (build it
  with `cargo install tree-sitter-cli --version 0.26.9` on older
  hosts).
- `big-code-analysis-py/uv.lock` is now tracked in git (was
  `.gitignore`d as a "per-developer cache"). It pins the dev set
  (ruff/mypy/pyright/maturin/pytest) for every contributor using
  `make py-bootstrap`. Alternative install paths (`mise install`,
  `pipx install`, `pip install -e .[dev]`) remain functional but
  bypass the lockfile — see the "Python bindings" section of
  CONTRIBUTING.md for the policy and rebase-conflict resolution.
  CI workflows are unchanged in this release and still pip-install
  pyproject floors; converging them onto `uv sync --frozen` is a
  follow-up.
- `make py-test`'s pre-build cleanup now also removes
  `big-code-analysis-py/python/big_code_analysis/_native*.so` before
  invoking `maturin develop`. Contributors who switch between abi3
  and per-version maturin build modes can otherwise end up with two
  compiled extensions in the editable install dir, where Python's
  loader prefers the more-specific cpython-tagged filename over the
  fresh abi3 build — producing `ImportError` for any symbol added
  since the older build.

- ABC metric (Phase 3 docs): the
  [book chapter on ABC](https://dekobon.github.io/big-code-analysis/metrics.html#abc)
  gains a *Counting rules* section that reproduces Fitzpatrick's
  Figure 2 / 3 / 4 rule tables (split into Assignments, Branches,
  and Conditions sub-tables, each row attributed to the figure
  that introduces it), a per-language deviation table (the
  `try` / `catch`-less languages, `default`-fallthrough
  exclusions for C++ / Go / Python / Rust, Ruby's `rescue`
  substitution, Tcl Phase-2 deferral, the Phase-2B `if` /
  `while` / `return` / argument-list slot coverage), a worked
  `if (am >= 0 && am <= 0xF)` example walking through each
  token's contribution, and an explicit comparison with
  RuboCop's `Metrics/AbcSize` (counts `and` / `or` directly),
  StepicOrg/abcmeter, and eoinnoble/python-abc. The module-level
  doc on `crate::metrics::abc::Stats` is expanded to quote
  Fitzpatrick's Rule 7 / Rule 9 worked example
  (`if (x || y) printf("test failure\n");` → two unary
  conditions) and to link to the new book chapter. Fixes
  [#404](https://github.com/dekobon/big-code-analysis/issues/404).
- ABC metric: the unary-conditional walker that previously
  applied to Java, Groovy, and C# now also runs for Rust, Go,
  JavaScript, TypeScript, TSX, Mozjs, PHP, C++, Python, Perl,
  Lua, and Tcl. On every `&&` / `||` token (and per-language
  equivalents — Python's `and` / `or`, Lua's `and` / `or`,
  Perl's `&&` / `||` / `//` / `and` / `or` / `xor`, Tcl's
  `&&` / `||`) the walker iterates the immediate operands of
  the parent `binary_expression` and counts each terminal-bool
  operand (identifier, boolean literal, call, field/member
  access, subscript, etc.) once, plus each `!`-wrapped /
  `not`-wrapped operand whose inner expression is a terminal.
  This is Fitzpatrick's Rule 7 (Figure 2) / Rule 9 (Figure 3 /
  Figure 4): "Add one to the condition count for each unary
  conditional expression." So `if (a && b) {}` now reports 2
  conditions across every language with the walker, matching
  the worked example in Listing 2 ("there are two unary
  conditions since both x and y are tested as conditional
  expressions"). Library users will see *higher* C-counts on
  functions that mix logical operators (the inverse of the
  Phase-1 drop in #395); ABC magnitudes shift upward
  accordingly. Phase 2 of [#395]
  (https://github.com/dekobon/big-code-analysis/issues/395).
  Fixes
  [#403](https://github.com/dekobon/big-code-analysis/issues/403).
- ABC metric (Phase 2B alias / coverage close-out, findings.md):
  closes three latent under-counts from the Phase-2 walker land
  surfaced by a follow-up audit. (a) Go gains a `for`-condition
  dispatcher arm: `for true {}` / `for !ready {}` now count one
  Fitzpatrick condition; for-loops with `init; cond; post` and
  `range` clauses fall through harmlessly. (b) Lua's terminal-
  bool set adds `Number`: `if 1 then end` and `return a and 2`
  now count their numeric-truthy operands once each, matching
  the existing inline comment that already promised numbers
  were terminal-bool. (c) The per-language `*_bool_terminal_kinds!()`
  macros now include every aliased `kind_id` the runtime grammar
  emits (lesson #2): Go `Identifier2/3`, C++ `QualifiedIdentifier`
  / `QualifiedIdentifier2/3/4`, PHP `Name2`, `MemberAccessExpression2/3`,
  `NullsafeMemberAccessExpression` / `NullsafeMemberAccessExpression2`,
  `SubscriptExpression2/3`, JS/Mozjs/TS/Tsx `MemberExpression2/3`
  (plus 4 for TS / Tsx) and `CallExpression2` (plus 3/4 for TS /
  Tsx), JS / Mozjs / Tsx `Identifier2`, TS / Tsx `SubscriptExpression2`.
  Pre-fix, idiomatic shapes like `if (n::flag) {}` in C++,
  `if (o.x) {}` in JS, `$obj?->prop` chains in PHP, and bare-
  identifier `for` conditions in Go silently reported zero
  conditions because the runtime kind_id (e.g., `MemberExpression2 =
  208` for JS `obj.x`) did not match the primary-only macro. The
  JS-family shared macro is split into four per-language macros
  to accommodate TypeScript's missing `Identifier2`. Integration-
  snapshot submodule refreshed accordingly. Closes the alias-leak
  gap noted in the Phase 2B review.
- ABC metric (Phase 2B follow-up): `if`/`while`/`do-while`,
  `return`, and argument-list slots also route through the
  per-language unary-conditional walker. `if (true) {}` (and
  `if true {}` for grammars without paren wrap — Rust, Go,
  Python, Lua) now counts 1 condition. `m(!a, !b)` counts 2
  (one per `!`-wrapped argument). `return !x` counts 1 (a
  bare `return x` continues to report zero, matching
  Fitzpatrick's "bare identifier in return slot is not a unary
  conditional" policy). 11 of the 12 Phase-2A languages (Rust,
  Go, JS, TS, TSX, Mozjs, PHP, C++, Python, Perl, Lua) ship
  Phase 2B arms — Tcl is deferred pending a per-grammar audit
  of its expression / command shape. Additional regression
  tests `<lang>_if_boolean_literal_condition`,
  `<lang>_methods_arguments_with_conditions`, and
  `<lang>_return_with_conditions` cover the new slots per
  language. Continues
  [#403](https://github.com/dekobon/big-code-analysis/issues/403).
- ABC metric: `&&` / `||` (and per-language equivalents — Python's
  `and` / `or`, Lua's `and` / `or`, Tcl's `&&` / `||`, Perl's
  `&&` / `||` / `//` / `and` / `or` / `xor`) are no longer counted
  as conditions on their own. Fitzpatrick's Rule 5 (Figures 2-4
  in the 1997 paper) lists only the comparison operators (`==`,
  `!=`, `<=`, `>=`, `<`, `>`) and his worked Listing 2 annotates
  `(am >= 0 && am <= 0xF) ? '/' : 'C'` as `accc` — three
  conditions for `>=`, `<=`, `?`, zero for `&&`. The C++, Python,
  Perl, Lua, and Tcl per-language `Abc` impls dropped the
  short-circuit arms; Java, Groovy, and C# already routed
  `&&` / `||` through their unary-conditional walker (which
  counts operands, not the operator, per Rule 7 / 9) and were
  unaffected. Library users will see lower C-counts on functions
  that mix logical operators; ABC magnitudes shift downward
  accordingly. The unary-conditional walker is being extended to
  the remaining languages (Rust, Go, JS/TS, PHP, C++, Python,
  Perl, Lua, Tcl) under
  [#403](https://github.com/dekobon/big-code-analysis/issues/403)
  so `if (a && b)` ultimately reports 2 conditions per the paper.
  Fixes
  [#395](https://github.com/dekobon/big-code-analysis/issues/395).
- Markdown linting / formatting now uses
  [`rumdl`](https://github.com/rvben/rumdl) (Rust-native) instead of
  `markdownlint-cli2` (Node.js). `mise.toml` pins `rumdl = "0.2.2"`,
  the project rule customisations migrate to `.rumdl.toml` (the old
  `.markdownlint-cli2.jsonc` is removed), `make markdown-fmt` /
  `make markdown-lint` invoke `rumdl check --fix` / `rumdl check`,
  and the CI `lint` job installs the upstream release tarball with a
  pinned SHA256 (dropping the previous `actions/setup-node` step).
  Contributors who track tooling via `mise install` get the new
  binary automatically; otherwise install via `mise install rumdl`
  or `cargo install rumdl`.
- `bca --num-jobs` now defaults to the OS-reported effective CPU
  count via `available_parallelism()` (cgroup-, cpuset-, and
  quota-aware on Linux; OS CPU count on macOS/Windows) instead of
  `1`. `--num-jobs auto` is accepted as an explicit synonym for
  the default; `--num-jobs 0` is rejected with an actionable
  message; `--num-jobs 1` still forces serial mode for debugging.
  The Makefile / book / package.json skeletons drop their
  per-recipe `$(nproc)` / `BCA_NUM_JOBS` threading. Fixes
  [#383](https://github.com/dekobon/big-code-analysis/issues/383).
- Cognitive complexity: removed the dead `UnaryExpression →
  not_operator()` arms across 13 language impls (Python, Rust,
  Cpp, the js_cognitive macro covering Mozjs/Javascript/Typescript/
  Tsx, Java, Groovy, Csharp, Perl, Kotlin, Go, Tcl, Lua, PHP,
  Elixir, Ruby). In pre-order traversal the reset fired after the
  enclosing `BinaryExpression` was already scored, so chained-NOT
  (`!a && !b && !c`) silently scored zero contribution; NOT-
  wrapping (`a && !(b && c)`) had a small effect that the patch
  collapses to a single boolean sequence, aligning with
  SonarSource rule B1's intent that only operator-type switches
  start a new sequence. `BoolSequence::not_operator()` is removed.
  Fixes
  [#392](https://github.com/dekobon/big-code-analysis/issues/392).
- `bca check` baseline path keys are now canonicalised relative to
  the baseline file's own directory (the *anchor*). `--paths .`,
  `--paths src/`, and `--paths "$PWD"` produce byte-identical
  baselines and `--baseline` runs match across forms — switching
  `--paths` style no longer surfaces every existing offender as a
  spurious `[new]`. The on-disk schema is bumped to `version = 3`;
  v2 baselines load with a one-time stderr deprecation hint and an
  in-memory migration (best-effort for ASCII-clean paths;
  pre-encoded non-ASCII paths may need a `--write-baseline`
  refresh). Removes the four "path-style stickiness" callouts in
  the book and the 8-line header warning in `bca-thresholds.toml`.
  Fixes
  [#376](https://github.com/dekobon/big-code-analysis/issues/376).
- ABC metric: refactored the Java / Groovy / C# `impl Abc::compute`
  bodies into four mutually-exclusive category helpers
  (`*_count_token_assignment`, `*_count_token_branch`,
  `*_count_token_condition`, `*_walk_for_conditions`) plus
  walk-ternary / walk-for-statement / inspect-child helpers per
  language. Each `compute` is now a 4-statement short-circuit chain
  at cyclomatic 4 (down from 48 / 38 / 36). Behaviour is preserved
  bit-for-bit (228 abc unit tests pass, zero submodule snapshot
  drift). Also renamed the pre-existing C# helper
  `inspect_csharp_child` → `csharp_inspect_child` for naming parity
  with the new `java_inspect_child` / `groovy_inspect_child`. Fixes
  [#369](https://github.com/dekobon/big-code-analysis/issues/369).
- Stability documentation re-baselined for the `1.x` line.
  `STABILITY.md`, the matching book page, the top-level README, the
  `CHANGELOG.md` caveat, `RELEASING.md` semver guidance, and the
  agentic instructions in `AGENTS.md` were rewritten to spell out
  the `1.x` contract: shape stability across patch and minor
  bumps, breaking shape changes reserved for the next major bump,
  and the metric-value drift carve-out preserved with per-release
  changelog notes. Notable policy decisions captured in this
  refresh: the `metrics` / `metrics_with_options` deprecation
  window (originally tagged for removal after one minor release
  when introduced at `0.0.26`) is extended to the full `1.x`
  lifetime — downstream code that took the original wording at
  face value has already migrated, and the shim stays in place
  until `2.0`. No source changes.
- `dump_spans` (internal) now writes into an injected
  `WriteColor` writer instead of constructing `StandardStream`
  internally, so its last-prefix-marking branch is observable in
  tests. The three regression tests added in
  [#343](https://github.com/dekobon/big-code-analysis/issues/343)
  now assert on rendered output instead of `is_ok()`. Fixes
  [#352](https://github.com/dekobon/big-code-analysis/issues/352).
- `make enums-check` now also runs `cargo test` on the
  workspace-excluded `enums` crate so the new dispatch tests
  (and any future tests) execute in pre-commit and CI rather than
  being compile-only.
- Internal CLI refactor: consolidated three pre-existing inline
  duplicates onto the shared helpers added in #544. Path-prefix
  stripping in `exemptions.rs` / `markdown_report.rs` now calls
  `format_util::strip_path_prefix`, and the `--output` validate/write
  logic in `report` / `exemptions` now calls `validate_output_path` /
  `write_output_or_stdout`. No behavior change — error wording and
  stdout fallback are byte-identical (#545).
- Hardened the two `OnceLock` static initializers in `checker.rs`
  (the cbindgen `<div rustbindgen` AhoCorasick automaton and the
  Python `coding:` regex) to use documented `.expect(...)` instead of
  bare `.unwrap()` on infallible constant-pattern compilation,
  completing the #343 non-test `unwrap` sweep. No behavior change
  (#549).
- `vcs::Options` and its sibling `vcs::CacheConfig` are now
  `#[non_exhaustive]`; external crates construct them via
  `Options::default()` / `CacheConfig::default()` plus field assignment
  (struct-literal and `..Default::default()` are forbidden cross-crate).
  Future additive VCS option fields are no longer a breaking change for
  external constructors. Landable now because the VCS surface is
  unreleased — not a break for any existing user. Part of #505's 2.0
  preparation (#584).

- **(breaking within report scope)** Unified the AST report's hotspot
  section titles onto one sentence-case template,
  `<Concept> hotspots (<top N|lowest N|all> by <column>)` (#677). The nine
  titles previously mixed Title Case, sentence case, and an all-caps
  internal `(NEXITS)` ID; the metric IDs now live only in the legend. The
  truncation clause appears on every table and reflects the actual `--top`
  state (`top 20 by CC`, or `all, by CC` for `--top 0`). HTML section
  anchors derive from a *stable* `<Concept> hotspots` slug
  (`#rust-cyclomatic-complexity-hotspots`), so deep links no longer shift
  with `--top`; the cross-format section-membership test and the rich
  Markdown/HTML snapshots moved with the titles. Wire shape untouched.
- **(breaking within report scope)** Relabelled the WMC hotspot from
  "Classes/impls/traits" to "Types" (#687): the global-header row, the
  section title, and the column header all read `Type`/`Types`, since
  `is_class_like` matches six kinds (class, struct, trait, impl, interface,
  namespace), not three. The full kind list is enumerated once in the
  legend's WMC entry. The WMC wire accessor (`class_wmc_sum`) is unchanged
  and stays distinct from this presentation label.
- Moved the per-language Actionable Summary to the top of each language
  section — directly after `### Summary`, before any hotspot table (#678),
  so a reader who stops after one or two tables still sees the
  highest-altitude counts. Dropped the index-splice mechanism in favour of
  explicit emission order. Section anchors keep their ids; only their
  position moves.
- Emit the Markdown report legend at `##` (was `###`) so it gets its own
  TOC entry instead of nesting under the last language's section, and
  render the HTML legend `<details open>` (was collapsed) so it survives
  print, mobile, and screen readers — the surfaces the legend exists to
  serve (#679). Added legend entries (and an HTML tooltip) for the
  global-header PLOC / Comments / Comment-ratio stats.
- Render Halstead Effort and Volume in the report tables as rounded
  integers with thousands separators (`8,845`, not `8844.75701441285`),
  matching the neighbouring SLOC/Tokens columns (#668). JSON/CSV/wire keep
  full f64 precision.
- Raised the exit-points (NEXITS) hotspot floor from `nexits > 0` to
  `> 2` (#689). A single `return` is the baseline, not a hotspot, so the
  old floor degenerated into a table of noise on a healthy codebase; a
  codebase whose worst function clears only the baseline now omits the
  section entirely.
- Relabelled the VCS report's `(total)` column headers to `(long)`
  (Churn / Commits / Authors / Change entropy / Co-change entropy), so they
  grep straight back to the `*_long` wire keys, and enriched each
  window-relevant tooltip with its window duration (default 365d long /
  90d recent) and backing key (#592). The plain table's `COMMITS r/l` /
  `CHURN r/l` headers spell out to `rec/long`. `(total)` was doubly wrong:
  it matched no key and misrepresented a one-year window as all-history.
  Rendered text only — CSV/wire already use `_long`/`_recent`.
- **(Python, breaking, deferred to 2.0)** Renamed the `AnalysisError`
  pyclass to `AnalysisFailure` (#614). It is a value type **returned** (not
  raised) by `analyze_batch` — the `…Error` suffix that PEP 8 reserves for
  exceptions misled readers into `except bca.AnalysisError:`, a `TypeError`
  at the `except` site since the class does not inherit `BaseException`.
  The shape, fields, and returned-not-raised semantics are unchanged; only
  the name moves. The package is not yet on PyPI, so the rename is cheap
  now but pinned by the 2.0 contract.
- **(Python, breaking, deferred to 2.0)** Namespaced the change-history
  surface into a `big_code_analysis.vcs` submodule (#612). The flat
  `vcs_metrics` / `vcs_trend` / `vcs_jit` functions become `vcs.rank` /
  `vcs.trend` / `vcs.commit` (names mirroring the `bca vcs` CLI
  subcommands), with the 15-shared parameters collapsed onto a single
  `vcs.Options` object all three accept — killing the duplicated
  17-kwarg signatures (and the drift #583 patched once already). The GIL
  is released across the history walks (folds in #620), so a
  `ThreadPoolExecutor` over several repositories parallelises them.
- **(Python, breaking, deferred to 2.0)** Split the dual-mode `vcs_jit`
  into `vcs.commit` (commit mode) and `vcs.score_diff(diff)` (diff mode)
  (#667). `vcs.commit` no longer accepts a `diff` kwarg — the footgun where
  `diff` silently discarded a named `commit` and returned a non-comparable
  `partial_risk_score` is gone structurally, matching the CLI/web's #632
  reject-the-mix behaviour. Each function now returns one well-defined
  shape.
- Structured the `bca-web` error body with a machine-readable
  `error_kind` token (#631). Every error response is now
  `{error, error_kind, id}`: `error` keeps its human role but carries the
  *specific* cause, and `error_kind` is a new closed-vocabulary
  `snake_case` token (e.g. `invalid_window`, `unknown_field`,
  `unsupported_language`) that clients branch on without string-matching
  the prose. This replaces the single kitchen-sink `/vcs` 400 message —
  a bad window now answers the specific `vcs_invalid_window` cause, not a
  sentence listing every possible parameter, and the message no longer
  names `/vcs` when the request hit `/vcs/jit` or `/vcs/trend`. The
  `VCS_BAD_REQUEST` constant is removed. Additive over the prior
  `{error, id}` shape (`error_kind` is purely new; `error` was always a
  free-form string), so this is not itself a break; the token vocabulary
  is pinned in STABILITY.md.
- **(breaking, deferred to 2.0)** Rejected unknown fields on every
  `bca-web` request body and query string with a `400` naming the
  offending key (#633). A typo'd field (e.g. `long_widnow`) previously
  `200`'d, silently computing with defaults the client did not ask for;
  it now `400`s with `error_kind: "unknown_field"` and the key named in
  the `error`. Every request struct gains `#[serde(deny_unknown_fields)]`,
  and the `/vcs/trend` payload is un-flattened (its shared `/vcs` knobs
  are inlined so `deny_unknown_fields` applies; the JSON shape is
  unchanged). Clients probing for feature support use the `GET /v1` route
  index (#643). Payloads with extra/typo'd fields that succeeded before
  now fail.
- **(breaking, deferred to 2.0)** Aligned the `bca-web` `/vcs`-family
  defaults with the CLI (#636). `top` now defaults to 50, `top_deltas`
  to 10, and `points` to 12 (formerly hard-required, so an omitted
  `points` now succeeds instead of `400`ing) — matching `bca vcs --top`
  / `--top-deltas` / `--points`. An explicit `top: 0` / `top_deltas: 0`
  still returns all (the #602 `0 = all` escape). This fixes the
  unbounded "all files" default on the most-exposed surface (a
  serializer self-DoS on a monorepo) and makes the same logical
  invocation return the same-sized result regardless of surface.
  Payloads omitting these fields get a smaller result.
- **(breaking, deferred to 2.0)** Added the `language` key to the
  `/v1/ast` response envelope (#654), bringing the published
  `AstResponse` library type to `{id, language, root}` and matching
  `/comment`, `/function`, and `/metrics`. AST node kinds are
  grammar-specific, so an `/ast` consumer can now confirm which grammar
  produced them. Additive on the wire, but a shape change to the
  published `AstResponse` type.
- **(breaking, deferred to 2.0)** Renamed the `bca-web` `/metrics`
  envelope and aligned the span vocabulary (#638). The single root
  metric space moves from the misleading plural `spaces` key to `root`
  (its own nested `spaces` list still holds the children); the boolean
  `unit` request flag (body field and query parameter) becomes the
  self-describing `scope` enum — `"full"` (default) or `"file"`; and
  `/ast`'s span keys `start_row` / `end_row` are renamed to
  `start_line` / `end_line`, matching `/function` and `/metrics` (1-based
  everywhere). This shape-changes the published `Span` library type.
  With #633's unknown-field rejection, a stale `unit` key now `400`s as
  an unknown field.
- **(breaking, deferred to 2.0)** Removed the unprefixed `bca-web` route
  aliases (#637 / #517). The original unprefixed paths (`/metrics`,
  `/comment`, `/function`, `/ast`, `/ping`, `/version`, `/languages`,
  the bare `/` index, and the `/vcs*` routes) now `404` like any other
  unknown URL; all routes are served under the `/v1` prefix. The interim
  `Deprecation` / `Sunset` / `Link` signalling headers (shipped in 1.x)
  are gone with the aliases. Clients must use the `/v1` prefix.
- **(breaking, deferred to 2.0)** Simplified `bca check`'s threshold-tier
  model (#688). The standalone `--headroom <RATIO>` flag is retired; the
  soft tier now carries its own scale ratio via the value-taking
  `--tier <hard|soft|soft=RATIO>` (default `hard`; a bare `--tier`
  means `soft`; `soft` alone uses the 0.95 default ratio). This folds
  the former four-mechanism precedence model (config / `[thresholds.soft]`
  / headroom / `--threshold`) down to tier-carries-ratio plus absolute
  `--threshold` overrides, and removes the three runtime precedence
  notes whose self-contradictory help text the issue flagged. The
  `[check] headroom` manifest key is unchanged and folds into the soft
  tier's ratio. `--headroom <R>` survives as a hidden one-cycle
  deprecated alias for `--tier=soft=<R>` (warns; removed next major) —
  note that it now *promotes* a hard run to the soft tier rather than
  being ignored at the hard tier.
- **(breaking, deferred to 2.0)** Aligned `bca.toml` manifest key names
  with their CLI flags (#666). The `num_jobs` key is renamed to `jobs`
  (matching `--jobs`); `--no-cyclomatic-try` (presence flag) becomes the
  value-taking `--cyclomatic-count-try <bool>` (matching the positive
  `cyclomatic_count_try` key); and `--strict-exit-codes` becomes the
  value-taking `--exit-codes <default|tiered>` (matching the
  `[check] exit_codes` key). All three new flags are full overrides — a
  CLI value beats the manifest in *both* directions, not just opt-in.
  One-cycle deprecated aliases (warn-but-honor): the `num_jobs` manifest
  key, the `--no-cyclomatic-try` flag (= `--cyclomatic-count-try=false`),
  and the `--strict-exit-codes` flag (= `--exit-codes=tiered`). The false
  "Every key mirrors a CLI flag" doc claim is corrected.
- **(breaking, deferred to 2.0)** Gave the auto-enabled CI behaviours of
  `bca check` never-forms and gave manifest booleans two-way CLI
  overrides (#683). `--github-annotations` becomes tri-state
  `<auto|always|never>` (mirroring `--color`; `auto` detects
  `$GITHUB_ACTIONS`, `never` suppresses even inside a step, a bare flag
  still means `always`); `--summary-file` now accepts `auto` / `never`
  keywords alongside a path (`never` suppresses the step-summary append
  even when `$GITHUB_STEP_SUMMARY` is set); and `--baseline-fuzzy-match`
  / `--no-suppress` become value-taking (`<bool>`, bare = `true`) so a
  CLI `false` overrides the matching manifest key without dropping the
  whole config via `--no-config`.
- **(breaking)** CLI flags are now scoped to the subcommands that consume
  them, with sectioned `--help` output (#597). Every flag that used to be
  `global = true` — `--paths`/`-p`, `--include`/`-I`, `--exclude`/`-X`,
  `--language`/`-l`, `--jobs`/`-j` (alias `--num-jobs`), `--no-ignore`,
  `--exclude-tests`, `--no-cyclomatic-try`, `--no-config`,
  `--preproc-data`, `--color`, `--no-skip-generated`, `--paths-from`,
  `--exclude-from` — now lives in a per-subcommand group
  (Input selection / Walker tuning / Preprocessor / Output) and must be
  passed **after** the subcommand: `bca metrics --paths src`, not
  `bca --paths src metrics`. Only `-w`/`--warnings` and `--report-skipped`
  stay universal. A flag passed to a subcommand that never consumed it is
  now a hard usage error (exit 1) instead of a silent no-op — e.g.
  `bca vcs commit --exclude-tests` and `bca list-metrics --paths` now
  error, and `bca list-metrics --help` no longer advertises walker flags.
  `vcs commit` / `vcs trend` take no walk or preproc flags. Migration:
  move each affected flag to after the subcommand token. Deferred to
  2.0.0 (#597).
- **(breaking)** Input paths are now accepted as a trailing `[PATHS]...`
  positional on the walking subcommands (#651): `bca metrics src/` works,
  matching tokei / cloc / scc / rg. The positional is unioned with any
  `--paths`/`-p` values (both remain valid). `bca find` and `bca count`
  move their node kinds off the `<NODES>...` positional onto a repeatable
  `-t`/`--type` flag to free the positional slot for paths —
  `bca find function_item` becomes `bca find -t function_item [PATHS]...`,
  and at least one `-t` is required. Script authors relying on positional
  `<NODES>...` for `find`/`count` must add `-t`. Deferred to 2.0.0 (#651).
- `bca dump` and `bca find` text output now print a `== <path> ==` banner
  before each file's tree, so a multi-file dump is attributable despite
  the parallel walk interleaving output (non-breaking; human debug
  output) (#690).
- **(breaking)** `bca dump` now requires an explicit path: bare `bca
  dump` errors (exit 1) instead of dumping the whole current directory —
  the documented exception to #596's default-`.` walk, since a
  whole-tree AST dump has no plausible use. Deferred to 2.0.0 (#690).
- **(breaking)** One human-output-format vocabulary across the CLI
  (#659): the diff-family human value `tty` is renamed to `text`
  (`bca diff` / `diff-baseline` / `exemptions --format text`), `bca vcs`
  gains a selectable `text` value that renders its human ranked table
  (previously the unnamed default), and `bca check`'s CI-dialect selector
  is renamed `--format`/`-O` → `--report-format` to separate "which CI
  report dialect" from data serialization. The renamed values/flags keep
  one-cycle hidden aliases (`tty`, and `--format`/`-O`/`--output-format`
  on `check`); `-O`/`--format` stay canonical for the structured format
  on the data-serialization subcommands. Deferred to 2.0.0.
- **(breaking)** `bca vcs jit` is renamed to `bca vcs commit` and its
  gate flag `--fail-over <SCORE>` to `--fail-above <SCORE>`; the gate
  output prefix is now `vcs commit: …`. The old `jit` subcommand spelling
  and `--fail-over` flag keep working as hidden aliases for one release
  cycle, then are removed in the next major. "Just-in-time (JIT)" stays
  in the long help and book as the defect-prediction-literature
  cross-reference. Deferred to 2.0.0 (#603).
- **(breaking)** `bca metrics`/`ops`: `--output <FILE>` now means a
  single aggregate file everywhere (a top-level array of the per-file
  documents; TOML wraps it under a `files` key, CSV concatenates each
  file's rows), matching every other subcommand. The per-file directory
  tree that `--output` used to imply moved to a new `--output-dir <DIR>`;
  `--output out.json` used to create a *directory* named `out.json`.
  Passing both `--output` and `--output-dir` is a usage error (exit 1).
  Migration: scripts that relied on `metrics -o <dir>` / `ops -o <dir>`
  for the per-file tree must switch to `--output-dir <dir>`. Deferred to
  2.0.0 (#669).
- **(breaking)** `bca metrics`/`ops`: an *explicitly-named* file whose
  language is unrecognized now warns on stderr unconditionally (no longer
  gated behind `-w`) and exits 1 when the run produced no analyzable
  output — mirroring the nonexistent-explicit-path rule (#596). A mixed
  run that analyzed at least one file still exits 0 with the warning;
  `--language` forces a parser for files whose extension lies.
  Directory-walk skips for unrecognized languages stay silently gated
  behind `-w`. Previously such an explicit file was skipped silently with
  exit 0. Deferred to 2.0.0 (#663).
- The `mi.*` threshold gate is now direction-aware (lower-is-worse):
  because maintainability index is healthier when *higher*, an `mi.*`
  value *below* the limit is the violation, while every other metric still
  breaches above. `Violation::ratio` inverts to `limit/value` so a lower
  MI ranks as the worse breach, offender messages read "falls below
  limit" (Checkstyle / Clang / MSVC / SARIF / Code Climate), and Code
  Climate's declared severity is now a floor. The same direction is now
  applied in the Python `to_sarif` binding, which previously used
  `value > limit` for every metric — flagging a healthy (high) MI and
  ignoring an unhealthy (low) one. Behavior change to threshold/SARIF
  output across the CLI and Python (#698).
- The CLI's deprecated flag/subcommand spellings (`--num-jobs`,
  `--warning`, `--output-format`, …) now emit a one-time migration warning
  via a single `deprecations` inventory scanned from the raw argv at the
  parse chokepoint — clap normalizes each alias to its canonical id before
  `ArgMatches` is built, so the silent clap-`alias` flags previously gave
  no signal. Warnings are always-on, independent of `-w` (#646).
- Report renderers now source their advisory cutoffs (Actionable Summary,
  CC-note bands, Many-Parameters filter) from the manifest `[thresholds]`
  table when present (falling back to built-in defaults) and emit a
  provenance line naming the source; counting is single-sourced via
  `AdvisoryThresholds::count_over` so the Markdown and HTML formats cannot
  drift (#630). The Markdown VCS table is now a curated subset (Rank, File,
  Risk, recent Commits/Churn/Authors, Ownership, Bug fixes, Hotspot) with a
  pointer to `--format csv|json` for the full record, while HTML stays
  complete and sortable (#621). An MI note (variant, GOOD/MODERATE/LOW
  bands, 0-100 clamping caveat) is added under the MI table in both formats
  and the MI hotspot tie-breaks on the unclamped `mi_original` so the
  ranking stays informative when displayed values all clamp to 0.0 (#627).
  The bus-factor "Files" tooltip is pinned distinct from the AST "files
  analysed" tooltip (#693). Presentation only; no structured-output
  contract change.
- Web: two structural forcing functions in the route/payload discipline —
  a `get_resource(path, handler)` registration helper that no GET/HEAD
  introspection route can bypass (so a future `web::get()` route cannot
  silently 405 HEAD), and a single named `COMMIT_MODE_FIELDS` list driving
  the `/vcs/jit` diff-vs-commit conflict check (so a new commit-mode field
  cannot silently bypass it). No change to status codes, the error
  envelope, or the REST contract (#647).

### Fixed

- Ruby `case … in` pattern matching now follows the project-wide
  match/switch default-arm policy in both cyclomatic and ABC. The bare
  wildcard arm `in _` (no guard) no longer adds a standard cyclomatic
  decision — it is the `case_match` default arm, matching Rust's bare-`_`
  `MatchArm` and Python's `case _:` filters. Conversely, non-wildcard
  `in` arms and guarded wildcard arms (`in _ if g`) now count as ABC
  conditions, which they previously did not, restoring parity with
  Python `case_clause` handling. `case_match` remains a modified-only
  container decision (#977).
- `bca check`: threshold limits now apply only to the space kind each
  metric actually measures, so a metric's whole-file or whole-`impl`
  aggregate no longer fires as if it were a per-function limit (#969).
  The subtree-summed and per-function metrics (`cognitive`, `cyclomatic`,
  `cyclomatic.modified`, `halstead.*`, `mi.*`, `abc`, `nargs`, `nexits`,
  `tokens`) gate individual functions; the object-oriented size metrics
  (`nom`, `wmc`, `npm`, `npa`) gate container spaces (class / struct /
  trait / impl / namespace / interface); and the `loc.*` size family
  gates the file root. Previously every limit was checked against every
  space, so a clean file or a multi-method `impl` tripped a per-function
  limit purely from the summed total — which forced ~120 whole-file
  `bca: suppress-file` markers across this repo that in turn blinded the
  gate to genuine per-function regressions. This restores per-function
  coverage without suppression. The scope is an intrinsic, per-metric
  default; there is no new manifest or CLI syntax. Not a SemVer break —
  the CLI grammar is unchanged and the file/container aggregate firing
  was never a documented per-function limit; if you relied on it as a
  crude whole-file budget, set an explicit `loc.*` threshold instead. The
  scope is owned by the shared `metric_catalog` (the new public
  `MetricScope` enum, alongside the existing lower-is-worse direction), so
  the Python `to_sarif` binding applies the identical gate and emits the
  same offenders as `bca check` — e.g. `wmc` now emits per class, not at
  the file unit, in both.
- Web (`POST /vcs/trend`): the trend endpoint no longer advertises the
  `no_cache` / `cache_dir` cache controls it could never honor. Trend does
  not use the persistent change-history cache (each sampled point re-anchors
  at a distinct historical tip with its own `as_of`, which the cache
  fingerprints separately and never evicts), so the two fields are removed
  from the trend payload and now answer `400` (`unknown_field`) instead of
  being silently accepted and ignored. The CLI mirrors this: `--no-cache` /
  `--clear-cache` / `--cache-dir` combined with `bca vcs trend` or
  `bca vcs commit` (which also do not cache) is now a usage error rather
  than a silent no-op (#961).
- Python `to_sarif`: findings are now emitted for the four
  subtree-aggregate metrics (`cyclomatic`, `cyclomatic.modified`,
  `cognitive`, `abc`) at *interior* spaces — a function owning nested
  closures, or a container — whose own value breaches the limit, matching
  `bca check -O sarif` exactly. The binding now reads the new per-space
  `value` wire field instead of the leaf-only subtree aggregate, closing
  the residual under-emission that #855's leaf-only fix left open (#958).
- Report (CLI): `escape_name` no longer doubles a literal backslash inside
  a Markdown table code span, so a backslash-bearing identifier (e.g. a PHP
  fully-qualified name `Foo\Bar`) renders with a single backslash instead of
  `Foo\\Bar` (#846).
- Report (CLI): the Markdown provenance footer now wraps user-supplied seed
  paths in a backtick code span so Markdown-active characters (`_`, `*`,
  `` ` ``) no longer leak into the italic footnote's emphasis, matching the
  HTML footer's escaping (#848).
- Report (CLI): advisory cutoff labels and the populations they count now
  describe the same boundary — fractional manifest `cyclomatic` / `cognitive`
  / `halstead.bugs` thresholds are rounded at resolution to the precision the
  label prints, mirroring the existing `loc.sloc` / `nargs` rounding (#845).
- Report (CLI): the Actionable Summary's suppressed-row breakdown now honors
  the resolved advisory `nargs` cutoff for the Many-parameters metric, so its
  count matches what the Many-parameters hotspot table actually hides under a
  non-default manifest `nargs` threshold; both the Markdown and HTML renderers
  are affected (#844).
- CLI metric-direction handling for the lower-is-worse `mi.*`
  (Maintainability Index) family is now consistent across the baseline
  surfaces. `bca diff-baseline` buckets an MI *drop* as **Worsened** and
  an MI *rise* as **Improved** (previously inverted, so the summary
  counts and `--worsened-only`/`--improved-only` filters selected the
  wrong rows) (#825); `bca check --baseline` classifies an MI drop below
  the recorded value as a regression instead of silently dropping it as
  `Covered` (#827); and `bca check --tier=soft --strict-exit-codes`
  escalates an MI value below its hard floor to a hard breach (exit 5)
  rather than under-reporting it as exit 2/3 (#837). All three now reuse
  the central `metric_catalog::lower_is_worse` predicate via a shared
  `breaches_limit` helper so the gate and the outcome classifier cannot
  drift on direction.
- `bca vcs commit --fail-above` now rejects a non-finite (`nan`, `inf`,
  `-inf`) or negative threshold at parse time (exit 1) instead of
  accepting it: a `nan`/`inf` threshold silently disabled the CI gate
  (`score >= NaN` is always `false`) and a negative one tripped on every
  commit. The flag now uses the same finite-non-negative validation as
  the `check` threshold parser (#850).
- A VCS-injection failure on one file in `analyze_batch` /
  `analyze_paths` no longer aborts the whole batch. The injection step
  (re-parse / reserialize of the self-produced metrics JSON) now degrades
  to the un-attached JSON — leaving that file's AST metrics intact —
  instead of propagating an `Err` that raised to Python and discarded
  every result computed so far, matching the documented graceful-
  degradation contract and the sibling `json_string_to_py` handling
  (#851).
- `big_code_analysis.analyze_paths` no longer silently drops a
  nonexistent (or dangling-symlink) path seed. It now classifies seeds
  with `symlink_metadata` (not `Path::is_file`) and surfaces a missing
  seed as an `AnalysisFailure` element (`error_kind="IoError"`,
  `error="path does not exist"`), restoring CLI parity with `bca
  metrics <paths>`'s hard error on a missing `--paths` seed (#596) while
  preserving the never-raise posture of the result vector (#858).
- The `pipeline_db.py` example's `_persist` no longer crashes when an
  existing sqlite db is reused with a wider metric column set. It now
  reconciles the live schema via `PRAGMA table_info` +
  `ALTER TABLE ... ADD COLUMN` before inserting, honouring the example's
  "survives the addition of new metrics" promise on db reuse (a library
  upgrade adding a metric, or a prior run with a narrower `metrics=[...]`
  selection) instead of raising
  `OperationalError: table metrics has no column named <X>` (#890).
- The Python binding's `to_sarif` no longer over-emits findings for the
  four aggregate-shaped metrics (`cyclomatic`, `cyclomatic.modified`,
  `cognitive`, `abc`) at interior container spaces. These metrics read
  the subtree-summed JSON field (`*.sum`, `abc.magnitude`), which equals
  the CLI's per-space accessor only at a leaf space; the binding now
  emits them only at leaf spaces (no descendant function/closure
  spaces), restoring byte-equivalence with `bca check -O sarif` for a
  file with nested functions or methods inside classes. Previously a
  class was flagged for the sum of its methods' complexity — a finding
  the CLI never produces (#855).
- `bca` CLI: the `vcs jit` -> `vcs commit` deprecation warning is no
  longer dropped when a `global = true` or either-position flag precedes
  the `jit` subcommand token (`bca vcs -w jit`, `bca vcs --long-window
  6mo jit`); the argv scan now finds the subcommand past any leading
  flags and their values instead of inspecting only the token right after
  `vcs` (#834).
- `bca` CLI: the deprecated-subcommand scan now stops at the `--`
  end-of-options marker (matching the flag scan), so path values literally
  named `vcs` and `jit` after `--` no longer trigger a spurious
  `` `jit` is deprecated `` warning (#836).
- `bca diff --since <ref>` no longer reports `export-ignore`'d source
  files as spuriously "added", nor perturbs `export-subst`'d file
  metrics. The before-side tree is now materialized with `git ls-tree`
  and `git cat-file` (which never consult gitattributes) instead of
  `git archive` (which silently honoured `export-ignore` /
  `export-subst`), so the before-side reflects the source exactly as
  committed at `<ref>` (#838).
- `bca find` (`dispatch_find`) no longer escalates a future-fallible
  `Ast::find` error to a worker-thread panic: it now degrades to no
  output for that file, matching the `bca metrics` / `bca ops`
  dispatchers. Behaviour is unchanged today (`Ast::find` is currently
  infallible); this removes a latent multi-file crash for the
  contracted strict-parsing mode (#839).
- The generated `From<u16> for Tcl` out-of-range fallback now resolves to
  the tree-sitter ERROR sentinel (`Tcl::Error2`, display `"ERROR"`)
  instead of the Tcl `error` *command keyword* (`Tcl::Error`, display
  `"error"`). The `enums/` Rust template hardcoded `unwrap_or(Self::Error)`
  and never consumed the renamed sentinel name, so Tcl — the lone grammar
  whose keyword set camel-cases to `Error` — silently misclassified an
  unknown/error node as a valid `error` token; every other language
  module already pointed at its ERROR sentinel. The template now renders
  the generator-computed sentinel name (#954).
- The `enums/data/mac.py` codegen helper is no longer append-only: it now
  rewrites `c_macros.txt` to exactly `sorted(macros)` each run, so a name
  removed from its `macs` template is pruned from the data file rather
  than lingering forever. The prior diff-and-union contract could only
  grow the artifact, letting it drift from its own generator (#892).
- `make py-stubtest` now also diffs the Python `vcs` submodule
  (`rank` / `trend` / `commit` / `score_diff` / the `Options`
  constructor) against its `vcs.pyi` stub by adding a second
  `big_code_analysis.vcs` module argument to the stubtest run. The
  submodule was previously allowlisted out wholesale, so PyO3
  signature/default drift in the change-history surface escaped the
  gate — reopening the #583 stub-drift failure mode for that surface.
  `vcs.pyi` now spells the PyO3-introspected shapes (a `@final`
  `Options` with a `__new__` constructor, and `commit`'s
  computed-default `...`) so the gate passes on correct stubs and
  fails on a planted default mismatch; the allowlist keeps only the
  `_native.vcs` submodule-attribute entry (its signatures are now
  checked via the facade) and runtime `__all__` (#854).
- Cross-language metric consistency: several per-language metrics were
  brought into line with their siblings. ABC now counts numeric-truthy
  operands in Python/JS/TS boolean slots (`if 5:`, `while (5)`, `x && 5`)
  (#772) and counts a Kotlin bare `if`/`while`/`do-while` predicate as
  one condition (#773); cognitive complexity resets nesting and applies
  the function-depth surcharge at nested PHP function/method boundaries
  (#775); cyclomatic no longer counts the head clause of a single-clause
  Elixir anonymous function (#776); JS-family LLOC no longer counts
  `statement_block` brace groupings as logical lines (#777); multi-line
  string interior rows now count as PLOC (not blank) across all languages
  with multi-line strings, matching Python (#778); `nexits` now counts Go
  `panic` and Lua `error`/`os.exit` as abrupt exits (#779); `npa` no
  longer counts C# interface fields with explicit `private`/`protected`
  as public (#780) nor PHP enum cases as attributes (#781); and `npm` no
  longer counts a C# property's narrowed (`private`/`protected`) accessor
  as a public method (#783). These shift the affected languages' metric
  values; integration snapshots are re-baselined.
- `loc.sloc` under `--exclude-tests` now drops the lines of `#[test]`
  functions nested in retained `impl`/`trait`/closure spaces, not only
  top-level `#[cfg(test)] mod`, matching `loc.ploc` and the MI SLOC term
  (#741).
- `MetricsOptions::with_metric_set` now resolves the supplied
  `MetricSet`'s dependency closure before storing it, so a derived metric
  (`Mi`/`Wmc`) selected via a hand-built set no longer computes from
  zero-valued prerequisites (#743).
- The `nargs` text (`Display`) output now reports `function_args` /
  `closure_args` as the cross-space sum, matching the JSON/YAML/TOML/CBOR
  serializers (#782).
- Markdown VCS bus-factor directory cells are now GFM-escaped, so a `|`
  in a directory path no longer corrupts the table (#739). `bca preproc`
  recovers its worker accumulator without panicking on a poisoned mutex
  or un-joined `Arc` (#740).
- Output edge cases: Checkstyle clamps a `Some(0)` column to `column="1"`
  (#784); SARIF neutralizes a scheme-ambiguous colon in a relative path's
  first segment with a `./` prefix (#798); and the human-readable number
  formatter renders a tiny negative that rounds to zero as `0`, not `-0`
  (#800).
- `read_file_with_eol` now skips UTF-16 BE/LE BOM files (`Ok(None)`)
  instead of stripping the BOM and parsing the interleaved-NUL body as
  garbage, and propagates a genuine probe-read I/O error as `Err` instead
  of swallowing it as `Ok(None)` (#803, #804).
- Comment removal preserves the source file's existing line-ending
  convention: stripping comments from a CRLF file no longer emits LF in
  the removed-comment region (#767).
- `Ast::ops` no longer invents a synthetic `<anonymous>` name for unnamed
  `Unit` spaces (their `Ops::name` is `None`), and now wraps an
  ERROR-root parse in a synthetic `Unit` space instead of returning
  `Err(EmptyRoot)`, matching `metrics()` (#755, #789).
- `CountCollector::into_count` trips a `debug_assert!` when called while
  the collector is still shared (a worker failed to join) instead of
  silently returning a non-final snapshot (#757).
- A `cfg(not(X), …)`-led top-level comma list ending in `)` no longer
  swallows a trailing `test` operand, so such items are correctly
  classified test-only under `--exclude-tests` (#763).
- The C/C++ macro-masking prepass treats a C++14/C23 digit-separator `'`
  (`1'000`, `0xDEAD'BEEF`) as numeric rather than a char-literal opener,
  so macros after such a literal are still masked (#765).
- The predefined-macro and special-token tables no longer list the
  nonexistent `UINT*_MIN` macros (#760) or the non-type `char64_t` /
  `charptr_t` specials (#762).
- Bash `heredoc_body` and Perl `heredoc_body_statement` now flatten in
  the AST dump, matching `Checker::is_string` (#761).
- VCS correctness: rollback revert-detection is now subject-anchored so a
  body-prose "rollback" no longer flags a commit as a revert (#806); the
  security-fix classifier requires a qualifier for `injection`/`overflow`
  and drops the bare ambiguous terms (#808); a persistent history-cache
  entry written from a shallow clone is no longer reused after the repo
  is deepened (and vice versa) (#810); `Co-authored-by:` is only honoured
  in a commit's final trailer block, so a body-quoted co-author is no
  longer counted (#812); authors with no name and no email are dropped
  from the participant set instead of collapsing into one phantom
  identity (#817); and a rename-only file with a space in its new path is
  no longer truncated, taking the path from the authoritative `rename to`
  line (#813).
- VCS arithmetic is now saturating/checked at the blame line-number,
  window-cutoff, days-rounding, and cross-author edit-sum sites,
  restoring the subsystem's no-panic invariant on extreme/degenerate
  inputs (#742, #809, #814, #820, #821).
- `Node::child_by_field_name` returns `Node<'a>` (the tree lifetime)
  instead of a borrow-scoped `Node<'_>`, so callers can hold a child past
  the parent borrow (#786).
- Documentation/comment accuracy: the crate-level Supported Languages
  rustdoc now lists Objective-C and three corrected slugs, guarded by a
  drift test (#769); and numerous stale or misleading comments/docs were
  corrected across the CSV/dump/parser/output modules, the suppression
  hint (now derived from `Metric::suppressible()`), the VCS jit/score/
  trend/wire/error/identity modules, and the book (#764, #766, #771,
  #774, #788, #790, #791, #792, #793, #795, #796, #797, #799, #801, #802,
  #805, #807, #816, #818, #819, #822, #823). The `tokens` metric gained
  smoke tests for C/Objc/Elixir/Ruby/iRules and a strengthened C++
  attribution test (#785, #787).
- `read_file_with_eol` now validates its 64-byte UTF-8 probe at the
  byte level with `std::str::from_utf8` instead of the previous
  `from_utf8_lossy` + unconditional `pop` + `U+FFFD` scan (#746, #758).
  The old heuristic dropped the probe's final character unconditionally,
  which both hid a genuinely invalid trailing byte (wrongly accepting
  non-UTF-8 input) and rejected files that legitimately contained the
  `U+FFFD` replacement scalar within the probe window. A trailing
  incomplete multibyte sequence is now tolerated only when the file
  continues past the probe (so the split character is completed by later
  bytes); when the probe is the whole file, an incomplete tail is
  treated as genuine corruption. The public `Ok(None)` /
  `Ok(Some(..))` contract is unchanged.

- `--exclude-tests` (and `MetricsOptions::exclude_tests`) now prunes
  unit- and function-level `loc.sloc` in step with the other loc
  sub-metrics (#722). `sloc` is the lone loc metric computed by span
  subtraction rather than node-by-node accumulation, so skipping a
  test subtree left `sloc` pinned at the full-file extent while
  `ploc`/`cloc`/`lloc` correctly dropped — an internally inconsistent
  loc block, a `blank` count inflated by the elided lines, and a
  `mi.*` whose `16.2·ln(SLOC)` term never benefited from test
  exclusion. The walker now records each pruned subtree's row span on
  its enclosing space and subtracts it from `sloc()`. The Markdown and
  HTML report Total SLOC roll-ups, comment ratio, and Average-MI inputs
  (which read `loc.sloc` directly) are corrected as a consequence.
  The default (`exclude_tests` off) path is byte-for-byte unchanged.
- Documentation: the [Suppression markers](commands/suppression.md)
  metric-identifier list no longer advertises the retired `exit`
  spelling. `exit` was retired in favour of the canonical `nexits` in
  #555, but the book still listed `exit` and claimed
  `bca: suppress(exit)` silenced a `nexits` violation — following it
  produced an unknown-identifier warning that voided the whole marker.
  The list now reads `nexits` and calls out that `exit` is rejected
  (#733).
- Bare-relative glob patterns now match the same files as their `./`-prefixed
  equivalents: `--exclude 'dir/**'` is identical to `--exclude './dir/**'`
  (#726). The walker matches discovered files against a `./`-anchored
  walk-root path, but patterns were compiled verbatim, so a bare `dir/**`
  silently matched nothing while `./dir/**` worked — affecting `--include`,
  `--exclude`, `--exclude-from`, `.bcaignore`, and the `[check.exclude]`
  gate-exemption set. A leading `./` is now stripped symmetrically from both
  the pattern and the match path. `**/`-prefixed, `*`, and absolute patterns
  were unaffected and remain unchanged. The Python bindings' `analyze_paths`
  walker had the mirror-image bug — it matched bare seed-relative paths, so
  `./dir/**` silently matched nothing there — and now accepts both spellings
  identically as well.
- A file named directly via `--paths` (or `--paths-from` / manifest `paths`)
  is now always analyzed regardless of the project's exclude deny-set
  (`.bcaignore`, `--exclude`, manifest `exclude`), matching the
  `ripgrep`/`fd`/`git` convention that an explicitly-named path overrides
  ignore rules (#726). Previously an exclude that matched the named file
  silently produced "0 files matched"; this was masked for `./`-anchored
  project patterns by the glob bug above, and surfaced once that was fixed.
  Directory-walk excludes are unchanged, and an `--include` allow-list still
  narrows which explicitly-named files are analyzed — matched against the
  seed's CWD-relative form, so `--include 'src/**'` now accepts
  `--paths "$PWD/src/f.rs"` exactly like `--paths src/f.rs` (the emitted
  `name` keeps the as-spelled form). The Python bindings' `analyze_paths`
  applies the same rule: a file seed bypasses `exclude`, and `include`
  still narrows it by basename.
- CLI `--help` text sweep (#608): relocated 80+ issue-tracker references
  (`#539`, `issue #328`, `pre-#182`, …) out of the user-facing clap
  doc-comments into adjacent `//` maintainer comments; the `bca check
  --config` TOML example now renders as a readable indented block instead
  of a one-line smashed Markdown fence (`verbatim_doc_comment`); replaced
  internal walker/seed jargon with user terms; and added
  `value_name = "FORMAT"` so the format flag reads `<FORMAT>` (not the
  leaked `<OUTPUT_FORMAT>` field name) on `metrics`, `ops`, and `check
  --report-format`. Also corrected the shared `--format` default-`text`
  help to name the `ops` operator/operand tree, stated the `vcs trend
  --points` cap (120) explicitly, and pointed `exemptions`/`check`
  baseline help at the canonical `[check] baseline` manifest key.
- Per-language Halstead operator/operand classification inconsistencies
  (#695). Several languages now agree with the C-family majority and with
  the standard Halstead convention, so their `n1`/`n2`/`N1`/`N2` — and the
  derived volume / difficulty / effort / MI — shift:
  - **Balanced-delimiter counting unified to opener-only.** Lua, Bash,
    Tcl, iRules, PHP, Ruby, and Elixir classified the *closing* `)`/`]`/`}`
    as a separate operator while the 18-language majority folds each
    balanced pair to a single glyph (`()`/`[]`/`{}`) and counts only the
    opener. A balanced `(...)` now counts as one operator, not two,
    removing the n1/N1 inflation in those seven languages.
  - **Bash `$var` no longer double-counts.** `$foo` / `$?` / `$1` parse as
    a `simple_expansion` wrapping a `variable_name` /
    `special_variable_name` leaf; both the wrapper and the inner leaf were
    operands, so every bare variable reference counted twice. The inner
    leaf is now suppressed (the guard Tcl and iRules already apply),
    lowering n2/N2 for variable-heavy Bash.
  - **JS `get`/`set` accessor keywords are now operators**, matching the
    C# getter's `Get`/`Set`/`Init`/`Add`/`Remove` accessor arm; previously
    the same accessor keyword was an operand in JS and an operator in C#.
  - **Python concatenated docstrings are now suppressed.** A
    `"""doc""" "more"` docstring is wrapped in a `concatenated_string`, so
    the single-literal docstring guard never fired and each fragment
    counted as an operand; all fragments of a docstring are now skipped
    (non-docstring concatenated literals are still counted).
  - `Getter::get_func_space_name` and friends now slice node spans with a
    bounds- and UTF-8-checked helper (`code.get(..)`), degrading a stale
    cross-parse span to an unnamed space instead of risking a panic on
    incremental reparse / VCS per-function re-slice.
- The optional-value check/walk flags introduced this cycle
  (`--cyclomatic-count-try`, `--tier`, `--exit-codes`,
  `--github-annotations`, `--no-suppress`, `--baseline-fuzzy-match`) no
  longer swallow a following positional `[PATHS]` argument (#651/#669).
  They now require `=` to take a value (`--tier=soft`,
  `--github-annotations=never`); the bare form still selects each flag's
  default-missing value (`--tier` → `soft`). Previously
  `bca metrics --cyclomatic-count-try src` errored with "invalid value
  'src'" because the path was parsed as the flag's value.
- **(breaking)** `bca metrics` / `bca ops --output <path>` without a
  structured `--format` now errors (exit 1) instead of silently writing
  nothing on exit 0 — the default `text` format streams to stdout and
  writes no files, so an explicit `--output` under it was a silent no-op
  (the failure mode the #600 `check` fix called out). Mirrors #600.
  Invocations that previously exited 0 writing nothing now exit nonzero
  (#661).
- `bca diff --metric <name>` now validates names at parse time: an
  unknown name (e.g. a typo `cylomatic`) exits 1 with the known-names
  list and a did-you-mean, instead of silently matching nothing and
  reporting a clean diff (exit 0). Reuses the `check --threshold`
  did-you-mean machinery and accepts the #514 dotted / `loc`-sub-metric
  aliases. Technically breaking — spellings that previously matched
  nothing now error, but those invocations were already silently broken
  (#662).
- `bca check` remediation footer no longer claims a `bca-reports` CI
  artifact on local runs; outside GitHub Actions it now suggests
  `bca report` for the detailed view (#676).
- Actionable Summary suppression caption now shows a per-metric
  breakdown (`halstead: N, cognitive: M`) of the rows each hotspot
  table actually hides, instead of an any-marker-any-metric
  per-function count that read as a near-total silencing of the
  codebase (#672). Report bytes only.
- Fully-suppressed hotspot sections now emit their heading (`###` /
  `<h3>` + id) with the omission note as the body, so heading
  structure and deep-link anchors stay stable across suppression
  states (#681). Report bytes only.
- Markdown report global header and per-language summary now render as
  two-column tables instead of run-on paragraphs in spec-conformant
  GFM (wikis, mdBook, repo-rendered `.md`) (#671). Report bytes only.
- VCS trend tip selection tie-breaks same-second commits toward the
  newest commit; previously `max_by_key` selected the oldest (e.g. an
  empty init commit), collapsing the trend to empty/wrong snapshots on
  repositories whose commits share one commit-time second (#650).
- Per-file walker errors are now Display-formatted
  (`error processing <path>: <err>`) and `BrokenPipe` is swallowed
  silently (`| head` / `| less`); previously a Rust `Os { … }` Debug
  struct leaked. Stderr diagnostics only (#665).
- Web 405 responses now carry an RFC 9110 `Allow` header, and
  `OPTIONS` is answered with `204` + `Allow` so clients can discover
  supported methods (#655).
- Per-file output (`bca metrics/ops -o <dir>`) is written under the
  walk-root-relative path again when `--paths` is spelled through a
  symlinked directory. `reanchor_seed` compared the as-spelled seed
  against the canonical process CWD, so a symlinked seed (the default on
  macOS, where a `TempDir` lives under `/var/folders` and `/tmp` is a
  symlink into `/private`) failed to strip and every output file nested
  under its full absolute path instead of the expected flat / relative
  layout. The seed is now canonicalized before the strip.
- **(breaking)** `bca check --output <path>` without `--format` infers
  the format from the file extension (`.sarif` → SARIF, `.xml` →
  Checkstyle) or exits 1 with a usage error, instead of silently
  writing nothing (#600).
- In-memory sources are EOL-normalized like CLI file reads: the web
  `/metrics` and `/function` endpoints and Python's `analyze_source` /
  `analyze` now produce identical metrics for CRLF / lone-CR /
  unterminated input; `/ast` and `/comment` stay byte-faithful by
  design. New additive library helper `normalize_eol` (#640).
- Web error responses honor the documented `{error, id}` JSON body on
  every path: extractor failures, body-size 413s (both JSON and
  octet-stream), pool-saturation 503, timeout 504, and internal 500
  no longer emit actix's plaintext bodies (#639).
- HEAD requests to the GET endpoints (`/ping`, `/version`,
  `/languages`, the route index) answer like GET instead of 405
  (#644).
- Commit-mode `bca vcs jit` / `/vcs/jit` reports carry the
  `source: "commit"` discriminator the docs promised (diff mode
  already had `source: "diff"`); JIT schema bumped accordingly
  (#642).
- `bca report --vcs` fills the change-history Hotspot column
  (complexity × recent churn) from the AST metrics computed in the
  same run, and both renderers omit the column when no row has a
  score (plain `bca vcs`) (#615).
- `bca vcs` window-parse errors name the failing flag and echo the
  offending input with a format hint instead of quoting an empty
  string (#607).
- HTML report SLOC tooltip described PLOC; it now states SLOC counts
  total physical lines including blanks and comments, and the Tokens
  tooltip no longer says "in the unit" (refs #610).
- `exemptions --check-exclude` help now describes the actual union
  (merge) semantics instead of claiming the CLI value replaces the
  manifest list (#606).
- Python error-message polish: unknown-language errors name the bad
  input and list the supported languages, and `OSError`s no longer
  duplicate the errno text (#617).
- The unified-diff parser behind `bca vcs jit --diff` (#580) now decodes
  git's C-style path quoting, so a diff touching a file with a non-ASCII
  name (`"a/na\303\257ve.txt"` under the default `core.quotePath=true`)
  keys its diffusion subsystem/directory on the real `naïve.txt` instead
  of the literal quoted, octal-escaped string. The `diff --git` header
  parse also recovers a new-side path containing a space — a binary file
  with a spaced name carries no `+++ b/<path>` line to self-correct — via
  the symmetric `a/X b/X` shape rather than truncating at the first space.
  Line/file counts and the partial score were already correct; only the
  diffusion grouping was affected (#586).
- The Python `vcs_metrics` type stub omitted the `bus_factor_threshold`
  keyword that the runtime `#[pyfunction]` accepts, so `mypy` / `pyright`
  rejected `vcs_metrics(repo, bus_factor_threshold=0.6)` even though it
  worked at runtime. The stub now lists it, matching the runtime
  signature order (#583).
- Per-function `git blame` (`bca metrics --vcs-per-function`) now retries
  each ODB object lookup that can hit the transient `gix-odb` object-lookup
  miss tracked in #579 — the whole-file blame and the post-blame commit
  resolution — up to three times, before falling back to the existing
  graceful degradation (skip that one file's per-function blocks, keep its
  file-level block). The underlying defect is a lock-free pack-refresh race
  one layer below `gix-blame` (gitoxide discussion #1412) that surfaces
  non-deterministically on pathologically repetitive files under the
  concurrent worker-pool ODB access whole-repo analysis performs; a retry
  re-reads the index snapshot, turning a momentary miss into a hit. Not
  fixed upstream as of `gix` 0.84 / `gix-blame` 0.14; real source does not
  trigger it (#579).
- ABC: the Fitzpatrick unary-condition walker is now wired for Kotlin,
  Ruby, and Elixir, so bare `&&` / `||` chain operands count toward
  `abc.conditions` / `magnitude` (Elixir ABC now agrees with cyclomatic).
  Re-baselines ABC conditions for these languages; existing per-language
  outputs containing logical-operator chains report higher condition
  counts (#557).
- The two Halstead labels in the human `dump` output (`estimated program
  length`, `purity ratio`) now use the underscore keys
  (`estimated_program_length`, `purity_ratio`) that match the JSON/CSV key
  scheme used by every other field (#562).
- `<halstead::Stats as Display>` now emits the same two labels with
  underscore keys (`estimated_program_length`, `purity_ratio`) instead of
  the space-separated forms, matching the JSON/CSV scheme and the post-#562
  `dump` output — the second human-readable surface #562 left out of scope
  (#563).
- Windows CI: the Python bindings' `enums_module_matches_checked_in`
  drift gate no longer fails on Windows runners. The generated
  `_enums.py` is byte-compared against `render_enums_module()` output
  (which emits `\n`), but a `core.autocrlf=true` Windows checkout
  rewrote the file to CRLF, breaking the comparison. The file is now
  pinned to LF (`*.py text eol=lf`) in `.gitattributes`, keeping the
  byte-exact gate valid on every platform
  ([#550](https://github.com/dekobon/big-code-analysis/issues/550)).
- PHP cognitive complexity no longer double-counts the two-word `else if`
  form. PHP parses `else if` as an `else_clause` wrapping a nested
  `if_statement`, and the nested `IfStatement` fell through the cognitive
  `IfStatement` arm — which, unlike every sibling language, lacked the
  `if !Self::is_else_if(node)` guard — firing `increase_nesting` on top of
  the wrapping clause's branch extension and inflating nesting for later
  arms super-linearly. The two-word form now scores identically to the
  one-word `elseif` keyword (e.g. an `if … else if … else` chain reports
  cognitive sum `3`, matching the SonarSource reference and the equivalent
  C++/JS). PHP's `is_else_if` now recognizes both the dedicated
  `else_if_clause` node and the `else_clause → if_statement` shape. Metric
  values for PHP functions using two-word `else if` decrease; cyclomatic
  and ABC are unaffected
  ([#529](https://github.com/dekobon/big-code-analysis/issues/529)).
- Groovy cognitive complexity now applies a lambda-nesting surcharge to
  control flow inside a closure (`list.each { … }`, `def c = { … }`),
  matching Java's `LambdaExpression` and every other closure-bearing
  language. Previously a closure body scored strictly lower than the
  byte-equivalent construct elsewhere, violating cross-language parity;
  e.g. `list.each { if (a) { while (b) {} } }` now reports cognitive sum
  `5.0`, the same as the equivalent Java lambda. Metric values for Groovy
  closures with nested control flow increase
  ([#519](https://github.com/dekobon/big-code-analysis/issues/519)).
- Bumped the `dekobon-tree-sitter-groovy` grammar pin to `=0.2.2`, which
  fixes an upstream parse defect
  ([tree-sitter-groovy#20](https://github.com/dekobon/tree-sitter-groovy/issues/20)):
  a top-level method with an explicit return type whose body contained a
  `;` (e.g. a C-style `for`) previously misparsed into an identifier,
  call, and standalone closure — it was not recognised as a function, so
  its cyclomatic / nargs / nom / cognitive metrics were all wrong. Such
  methods now parse correctly. The grammar's anonymous-token node IDs
  shifted (a `;` terminator was added), so the generated `Groovy` enum
  in `src/languages/language_groovy.rs` was regenerated to stay aligned
  ([#519](https://github.com/dekobon/big-code-analysis/issues/519)).
- `bca-web` routes now accept `Content-Type` variants such as
  `application/json; charset=utf-8`, `APPLICATION/JSON`, and parameterised
  `application/octet-stream` by matching the parsed media-type essence
  instead of an exact header string; a content-type mismatch on a known
  endpoint now returns a diagnostic `415` instead of a bodyless `404`
  ([#515](https://github.com/dekobon/big-code-analysis/issues/515)).
- `bca check --threshold` and `bca diff --metric` now accept each other's
  metric-name spelling (`sloc` ↔ `loc.sloc`, `halstead.volume` ↔
  `halstead`), normalising internally via a shared catalog-derived alias
  map; ambiguous bare family heads (`halstead`, `mi`) are rejected with a
  "did you mean" hint
  ([#514](https://github.com/dekobon/big-code-analysis/issues/514)).
- docs: corrected the Supported Languages chapter and deb/rpm package
  descriptions to match the actual `LANG` variants — removed phantom `C` /
  `Mozcpp` entries, labelled the internal `Ccomment` / `Preproc` helpers,
  and added the previously-omitted Elixir, Groovy, iRules, Ruby, and Mozjs
  ([#526](https://github.com/dekobon/big-code-analysis/issues/526)).

- `bca report markdown|html`: the "Maintainability Index (lowest files)"
  table no longer hides the worst files. `mi_visual_studio()` clamps a
  negative MI formula to `0.0`, but the table filtered on
  `mi_visual_studio > 0.0`, so a large, unmaintainable file scoring `0.0`
  collided with the empty-file `0.0` and vanished from the very table meant
  to surface it. The filter now mirrors `mi::Stats::inputs_are_empty`
  (`halstead_volume > 0 && sloc > 0`): clamped-to-zero worst files appear,
  while genuinely empty files stay excluded.
- `bca report html`: the cyclomatic summary note (`Average CC | Max |
  CC > 10 | CC > 20`) now reflects the suppression-filtered table it
  captions, matching the Markdown report. It previously counted raw
  measurements, so under the default suppression policy the note could
  disagree with its own table (e.g. report `CC > 10: 1` above a table
  showing none) and diverge from the Markdown rendering of the same data.
  The raw, suppression-independent CC count remains in the Actionable
  Summary roll-up.
- Library `dump_root` (the metric tree dump): the `nom` and `nargs` blocks
  printed each space's *immediate* `functions`/`closures` counts next to
  the subtree-aggregate `total`, so the parts did not sum to the total at
  any parent space (e.g. a Rust file whose functions all live inside
  `impl`/`mod` printed `functions: 0, total: N`) and disagreed with the
  JSON serializer. Both blocks now print the subtree-aggregate `*_sum`
  counts, matching JSON, `Display`, and the sibling `cyclomatic`/`tokens`
  blocks.
- `bca diff --since <ref> <subdir>` no longer reports every file as both
  added and removed. The before side (a `git archive` of `<ref>`) is
  rooted at the repo top, but the positional was used as the after-side
  *root*, so a subtree positional keyed the two sides differently and
  nothing paired. The `--since` positional is now a relative *scope*
  (equivalent to `--paths`) applied to both sides, which stay rooted at
  their tree top; an absolute positional is rejected like an absolute
  `--paths`
  ([#497](https://github.com/dekobon/big-code-analysis/issues/497)).
- `[check.exclude]` / `--check-exclude` now exempts offenders from
  `--paths-from` seeds, not only `--paths`. `apply_check_exclude` was
  handed only `--paths`, so a violation from a `--paths-from`-sourced
  absolute seed was matched against the deny-set unanchored and a
  walk-root-anchored pattern silently failed to exempt it (a
  false-positive gate failure, or a polluted `--write-baseline`). It now
  anchors against the full reanchored seed set
  ([#497](https://github.com/dekobon/big-code-analysis/issues/497)).
- `bca preproc` again resolves cross-file `#include` directives. #489
  moved the walk to the CLI and handed the library a flat file list, so
  every path took the library walker's single-file branch and the
  basename-grouping callback (`proc_dir_paths`) that feeds
  `fix_includes` never fired — leaving each file's `indirect_includes`
  containing only itself and every cross-file include unresolved. The
  basename grouping now runs in the CLI over the resolved file list the
  workers analyzed, so includes resolve again. Guarded by a new
  multi-file integration test
  ([#495](https://github.com/dekobon/big-code-analysis/issues/495)).
- `--exclude-from` (`.bcaignore`) glob matching is now anchored to the
  walk root, so an absolute walk root — a `bca.toml` `paths = ["."]`
  resolved to an absolute path, or an explicit `--paths "$PWD"` —
  excludes the same files as `--paths .`. Previously the `./`-anchored
  ignore patterns silently failed to match an absolute walk root,
  breaking the documented zero-config manifest gate
  ([#488](https://github.com/dekobon/big-code-analysis/issues/488)).
  Matching is now anchored to each file's walk root rather than the
  process CWD, so manifest-driven excludes also apply when `bca` runs
  from a subdirectory below the manifest directory
  ([#489](https://github.com/dekobon/big-code-analysis/issues/489)).
  The same walk-root anchoring now applies to the `[check.exclude]`
  gate-exemption globs, which previously matched the emitted (possibly
  absolute) violation path and so silently exempted nothing under an
  absolute / above-CWD walk root
  ([#493](https://github.com/dekobon/big-code-analysis/issues/493)).
- The `cyclomatic_count_try` `bca.toml` key is no longer flagged with a
  spurious "ignoring unrecognized key" warning. The key was added to the
  typed manifest view in [#409](https://github.com/dekobon/big-code-analysis/issues/409)
  but omitted from the `KNOWN_KEYS` allowlist that drives the
  unrecognized-key advisory, so a manifest setting it was silently
  honored while bca printed a warning claiming it was ignored. The
  allowlist now includes the key.
- Ruby stabby lambdas (`->(z) { … }`) are no longer double-counted in
  the `nom` closures metric. tree-sitter-ruby parses a stabby lambda as
  a `Lambda` node that contains its body `Block`/`DoBlock`, so the lambda
  was counted once and its body block again; the inner block of a
  `Lambda` is now skipped while keyword `lambda { }` / `proc { }` forms
  still count once
  ([#465](https://github.com/dekobon/big-code-analysis/issues/465)).
- C/C++ Halstead now counts `sized_type_specifier` modifiers
  (`unsigned` / `signed` / `long` / `short`) as operators. They are bare
  keyword tokens rather than `primitive_type` children, so they were
  classified `Unknown` and dropped — deflating operator counts and
  derived volume/difficulty/effort/MI for sized integer types
  ([#466](https://github.com/dekobon/big-code-analysis/issues/466)).
- Tcl `switch` now contributes to cognitive and cyclomatic complexity
  (and derived MI). A `switch` is a generic `command` node with no
  dedicated kind, so both metric dispatchers skipped it; each
  non-`default` arm is now a cyclomatic decision point and the switch is
  a `+1`+nesting cognitive structure, matching the cross-language switch
  convention ([#467](https://github.com/dekobon/big-code-analysis/issues/467)).
- Go interface method signatures now respect the lexical export rule in
  `npm`: `interface_npm` counts only methods whose name begins with an
  uppercase Unicode letter, instead of treating every signature as public
  ([#471](https://github.com/dekobon/big-code-analysis/issues/471)).
- C# expression-bodied properties (`int W => _w;`) now count as one
  method in `nom`/`wmc`, matching the `npm` `.max(1)` count. With no
  accessor to defer to, the property previously opened no function space
  and contributed `0`; bodied and auto-property counts are unchanged
  ([#472](https://github.com/dekobon/big-code-analysis/issues/472)).
- PHP ABC condition counting no longer over-counts the `default` arm of
  a `switch` (`DefaultStatement`) or `match` (`MatchDefaultExpression`),
  matching PHP's cyclomatic decision count. This completes the
  cross-language "ABC conditions == cyclomatic decisions" invariant after
  the #469 C-family and #456 Kotlin/C# fixes
  ([#473](https://github.com/dekobon/big-code-analysis/issues/473)).
- Kotlin cognitive complexity no longer over-counts labeled non-jump
  expressions ([#450](https://github.com/dekobon/big-code-analysis/issues/450)).
  The labeled-jump arm added by #450 was unconditional, but
  tree-sitter-kotlin-ng models *any* labeled expression as
  `labeled_expression`, so a labeled non-jump such as `lbl@ run { … }`
  wrongly scored `+1`. The arm now increments only when the
  `labeled_expression`'s `label` child is the fused jump keyword
  `break@` / `continue@`; labeled `break@outer` / `continue@outer`
  still score `+1` (§B2) and bare `break`/`continue` remain `+0`.

- Java `nargs` no longer counts an explicit receiver parameter as a
  formal parameter ([#470](https://github.com/dekobon/big-code-analysis/issues/470)).
  Java's explicit receiver (`void m(S this, int a)`, JLS 8.4.1) parses
  as a `receiver_parameter` node — distinct from a real
  `formal_parameter` — and binds `this`, not a value, so it is not an
  argument. `JavaCode::is_non_arg` now excludes `Java::ReceiverParameter`,
  matching the Rust `self` receiver fix ([#457](https://github.com/dekobon/big-code-analysis/issues/457)),
  Go's `receiver` field, and C++'s implicit `this`. A method written
  `void m(S this, int a)` now reports `nargs = 1` instead of `2`; a
  receiver-only `void m(S this)` reports `0`. Methods without an
  explicit receiver are unchanged. Kotlin is unaffected (its extension
  receiver sits outside `function_value_parameters`).

- The `default` arm of a C-family `switch` no longer over-counts ABC
  conditions ([#469](https://github.com/dekobon/big-code-analysis/issues/469)).
  Java, C#, Groovy, JavaScript, TypeScript, TSX, and Mozjs counted the
  `default` arm as an ABC condition while cyclomatic — which counts only
  the `Case` arms — did not, inflating the ABC C-component (and
  magnitude) by one for every switch carrying a `default`. Both the
  classic statement form (`default:`) and the arrow form (`default ->`)
  emit the same `Default` token, so both forms over-counted. The
  `Default` token is now excluded from the condition tally in every
  affected language, mirroring the cyclomatic gate and the Kotlin
  `when`-else / C# switch-expression discard fixes from #456. C and C++
  already excluded it. This is a metric-value change: ABC conditions and
  magnitude drop by one per affected switch.

- The cyclomatic cross-language parity tests now anchor each family's
  absolute normalised CCN, not just mutual cross-language agreement
  ([#468](https://github.com/dekobon/big-code-analysis/issues/468)).
  Seven of the eight families (`switch_with_default`,
  `switch_without_default`, `if_else_if_else_chain`, `single_if_no_else`,
  `two_arm_switch_with_wildcard`, `do_while_loop`, `range_for_loop`)
  asserted only `assert_parity` (all languages agree), so a regression
  shifting every language's count by the same amount would pass green at
  the wrong number (lessons #6 / #23). Each family now also asserts the
  hand-derived expected magnitude via `assert_normalised`, mirroring the
  anchor already present in `safe_navigation_chain_parity`. Test-only —
  no production behaviour or metric value changes.

- C# indexers are no longer triple-counted in `nom` and `wmc`
  ([#464](https://github.com/dekobon/big-code-analysis/issues/464)).
  A bodied indexer (`this[int i] { get => …; set => …; }`) opened a
  function space for the `indexer_declaration` node *and* for each
  `get`/`set` accessor, counting one indexer as three functions in
  `nom` and folding an extra entry into `wmc`. The `indexer_declaration`
  node now opens its own space only for the accessor-less
  expression-bodied form (`this[int i] => _d[i];`), deferring to its
  accessor children otherwise — matching how properties are counted and
  the `npm` reference (`csharp_count_member`), which counts an indexer
  as its accessor count with a `.max(1)` fallback for the
  expression-bodied form.
- Anonymous class/object bodies now open their own `Class` func-space,
  so their members are attributed to the anonymous type rather than the
  enclosing function
  ([#463](https://github.com/dekobon/big-code-analysis/issues/463)).
  Kotlin `object : T { ... }` (`object_literal`) was absent from
  `KotlinCode::is_func_space` and the Kotlin `get_space_kind` arm — the
  direct sibling of the companion-object fix (#431) — so its methods and
  properties folded into the enclosing function. Java anonymous classes
  (`new Runnable() { ... }`) had the same gap: the fix opens a `Class`
  space for an `object_creation_expression` that carries a `class_body`
  child (a plain `new Foo()` and a lambda — a distinct
  `lambda_expression` — correctly open no class space), bringing Java to
  parity with PHP `AnonymousClass` and C# anonymous forms. Both forms now
  resolve to `<anonymous>` via the default `get_func_space_name`. Groovy
  shares the `object_creation_expression` shape but the pinned grammar
  models an anonymous body as a separate `closure`, not a `class_body`,
  so Groovy gets no `Class` space (its members already land in the
  closure's `Function` space rather than the enclosing method) — an
  upstream-grammar limitation documented in `get_space_kind`.
- `wmc` (Weighted Methods per Class) no longer double-counts the
  complexity of a class nested inside a method (anonymous classes,
  Kotlin object literals, Java/Groovy local inner classes, Rust local
  items)
  ([#463](https://github.com/dekobon/big-code-analysis/issues/463)).
  A method's contribution to its enclosing class's WMC previously used
  its cumulative cyclomatic, which folded in the complexity of any class
  nested inside it — complexity those nested classes already count as
  their own WMC scope. The aggregator now subtracts each nested
  class/interface's cyclomatic from the method's contribution, so every
  method's complexity is counted exactly once, in its own class. This
  corrects per-class and file-level `wmc` for code with class-in-method
  nesting across all object-oriented languages (the fix lives in the
  shared `Wmc::merge`); `nom`, `npm`, `npa`, and `cyclomatic` totals are
  unaffected (the new anonymous class space adds a base `+1` to the
  cyclomatic sum, exactly like a named class).
- Python `lloc` now counts `match` statements (PEP 634) and `type`
  alias statements (PEP 695)
  ([#462](https://github.com/dekobon/big-code-analysis/issues/462)).
  Both parse cleanly and exist in the language enum, but
  `match_statement` and `type_alias_statement` were absent from the
  `Loc` logical-lines arm, so they fell through to the default branch
  and contributed zero — inconsistent with `if`/`for`/`while`/`try`
  and with C/C++ `switch`. A `match` now adds one logical line like
  `if`/`try` (its `case_clause` children stay uncounted, mirroring
  how `elif_clause`/`else_clause`/`except_clause` are omitted — the
  statements inside each case still count via their own arms), and a
  `type` alias counts like an assignment. Any Python `lloc` snapshot
  whose fixture uses these constructs shifts accordingly.
- `cloc` no longer over-counts multiple block comments sharing a
  single code line
  ([#461](https://github.com/dekobon/big-code-analysis/issues/461)).
  `add_cloc_lines` previously incremented `code_comment_lines` once
  per comment node, so a one-line construct such as
  `int f(int /*a*/, int /*b*/) { return 1; }` reported `cloc = 2` for
  a single physical line — violating `cloc <= sloc` and pushing the MI
  `comments_percentage` (`cloc / sloc · 100`) above 100%, which
  distorted the unclamped SEI term `50·sin(√(2.4·CM))` by tens of MI
  points. Comment-bearing code lines are now de-duplicated per
  physical line (mirroring `ploc`), and `comments_percentage` is
  defensively clamped to `[0, 100]` in `mi_sei`. Affects every
  block-comment language (the helper is shared); multi-line block
  comments still count one comment line per spanned line. Corpus
  `cloc` / `mi_sei` snapshots shift accordingly.
- Rust `npm` / `npa` no longer count `pub(self)` / `pub(in self)`
  items as public
  ([#460](https://github.com/dekobon/big-code-analysis/issues/460)).
  Both forms restrict visibility to the current module — semantically
  private, like no modifier — but `rust_item_is_public` returned true
  for any `visibility_modifier`, so `pub(self)` methods, struct fields,
  and associated consts were over-counted. The helper now treats a
  `visibility_modifier` whose restriction keyword is `self` (a direct
  `Zelf` child, distinguishing `pub(self)` / `pub(in self)` from
  `pub(crate)` / `pub(super)` / `pub(in <other path>)`) as private.
  The same check is applied to the tuple-struct positional-field path.

- TypeScript / TSX `npa` now counts `readonly`-only constructor
  parameter properties as attributes
  ([#459](https://github.com/dekobon/big-code-analysis/issues/459)).
  A `constructor(readonly b: number)` declares a public readonly
  instance field, but the parameter-property arm of `ts_npa_compute!`
  fired only when the `required_parameter` carried an
  `accessibility_modifier`. `readonly` is a distinct keyword child, so
  `readonly`-only parameter properties were dropped — inconsistent with
  `readonly` class fields, which already count. The arm now also fires
  on a `readonly` child; such a property is public (it has no
  visibility modifier) so it counts toward both `na` and `npa`.
  `private readonly` carries both children but is counted exactly once
  and stays non-public.
- Go `npm` / `npa` now respect Go's lexical export rule instead of
  marking every method and field public
  ([#458](https://github.com/dekobon/big-code-analysis/issues/458)).
  The public counts (`npm.classes`, `npa.classes`) now increment only
  for members whose name begins with an uppercase Unicode letter
  (`unicode.IsUpper`, checked via `char::is_uppercase` so non-ASCII
  exports such as `Ärger` are recognised); unexported (lowercase) and
  blank (`_`) members are private. As part of the same fix, `npa` now
  tallies per declared field *name* rather than per `FieldDeclaration`
  node, so a grouped declaration `A, b int` contributes two attributes
  (one public, one private) instead of one, and an embedded field
  (`io.Reader`, `*Foo`) takes its visibility from the embedded type's
  base name. This makes Go's COA / CDA accessibility ratios reflect
  real visibility rather than always reporting `1.0`. Go-only — no
  other language uses capitalization for visibility, and the `nm` /
  `na` totals are unchanged in count semantics aside from the
  grouped-field correction.

- Rust `nargs` no longer counts the `self` receiver as a formal
  parameter ([#457](https://github.com/dekobon/big-code-analysis/issues/457)).
  `RustCode::is_non_arg` now excludes the `self_parameter` node, so
  `self`, `&self`, and `&mut self` contribute nothing to the argument
  count — matching how Go excludes its `receiver` field and C++
  excludes the implicit `this`. A method `fn m(&mut self, x: i32)`
  now reports `nargs = 1` instead of `2`. A *typed* receiver
  (`self: Box<Self>`, `self: Rc<Self>`, `self: Pin<&mut Self>`)
  parses as an ordinary `parameter` node rather than `self_parameter`,
  but its binding is still the `self` keyword; it is now also excluded,
  so `fn c(self: Box<Self>, x, y)` reports `nargs = 2`, matching the
  bare-receiver case and Go/C++ receiver parity. This lowers `nargs`
  sum / average / min / max for Rust fixtures with methods.
- ABC condition counting for switch/`when`-expression arms now tracks
  the cyclomatic decision count in Kotlin and C#
  ([#456](https://github.com/dekobon/big-code-analysis/issues/456)).
  Kotlin counted a `when`'s `else ->` fallback arm as a condition
  (`when (x) { 1 -> …; 2 -> …; else -> … }` reported `conditions = 3`
  instead of `2`); the `else` arm is now excluded via the same
  `kotlin_when_entry_is_else` gate cyclomatic already uses. C# scored
  zero conditions for `switch` *expression* arms
  (`x switch { 1 => …, _ => … }` reported `conditions = 0`) because
  such arms carry no `case` / `default` token; each non-discard arm now
  contributes one condition via the shared
  `csharp_switch_expression_arm_is_bare_discard` gate, while the bare
  `_ =>` discard arm is excluded as the `default:` analogue. C#
  switch-expression and Java arrow-`switch` ABC condition counts now
  agree on equivalent code.
- ABC assignment counting for Kotlin no longer drops standalone
  assignments that follow a `val` declaration
  ([#455](https://github.com/dekobon/big-code-analysis/issues/455)).
  The previous design pushed a `Const` sentinel on `val` and cleared
  the declaration stack only on an explicit `SEMI` token, but
  tree-sitter-kotlin emits no `SEMI` (even for explicit semicolons),
  so the sentinel leaked and suppressed every later `=` in the same
  function — `val x = …; a = 1; b = 2` reported `A = 0` instead of `2`.
  The `=` is now classified structurally from its parent node
  (`property_declaration` / `class_parameter` carrying a `val`
  keyword) rather than from a persistent stack, so only the immutable
  initialiser is exempt. Groovy was audited and is not affected: its
  `def` declarations push only a mutable sentinel and the grammar does
  not produce a reachable `final` variable-declaration node, so no
  `Const` sentinel can leak.
- Halstead now classifies interpolated-string operands correctly for
  Groovy GStrings and the Kotlin short `$name` template form
  ([#454](https://github.com/dekobon/big-code-analysis/issues/454)).
  Groovy had no interpolation guard at all, so a GString counted both
  the wrapping literal and its inner expression, double-counting the
  inner identifier in `n2`/`N2`; `StringLiteral` now routes through the
  shared interpolation skip with the `gstring_brace_interpolation` and
  `gstring_dollar_interpolation` child kinds. Kotlin's guard only fired
  for the long `${expr}` form (a structured `interpolation` node); the
  short `"Hi $name"` form has no such node — the kotlin-ng grammar emits
  the variable as bare `string_content` tokens — so the opaque wrapper
  was counted and the inner variable dropped. A new source-aware
  classification hook (`Getter::get_op_type_with_code`) recovers the
  clean-identifier short-form variable as the operand and suppresses the
  wrapper, matching the long form. Mid-segment short forms the grammar
  does not tokenise cleanly (`"$x is "`, `"$obj."`) and a `$` not
  followed by an identifier (`"price: $5"`) stay literal text.
  Metric-value change for affected Groovy/Kotlin sources; derived MI
  shifts accordingly.
- Halstead no longer double-counts a TypeScript/TSX `void` return (or
  parameter) type as two operators
  ([#453](https://github.com/dekobon/big-code-analysis/issues/453)).
  The TS/TSX grammars parse `: void` as a `predefined_type` wrapper
  around an inner `void` token; `is_primitive` routed the wrapper into
  the lexeme-keyed `primitive_operators` map as `"void"`, while the
  inner token was independently classified as the standalone `void`
  expression operator (`void 0`), so one source `void` counted as two
  distinct operators (`n1`/`N1` inflated by one). The wrapper is now
  suppressed when its sole child is a `void` token, letting the inner
  token carry the single operator and keeping the count consistent with
  expression-`void`. Other predefined types (`: string`, `: number`,
  `: boolean`) and C#'s `void` (a `PredefinedType` with no inner token)
  are unaffected. Metric-value change for affected TS/TSX sources;
  derived MI shifts accordingly.
- Cognitive complexity now applies the SonarSource §B2 jump rule
  correctly for Perl and Kotlin
  ([#450](https://github.com/dekobon/big-code-analysis/issues/450)).
  Perl `goto LABEL;` no longer double-counts (`goto_expression` wraps
  the anonymous `goto` keyword token, which the walker also visits, so
  matching both scored `+2`); it is now `+1`. Perl labeled
  `last`/`next`/`redo LABEL` now add `+1` each — the prior guard
  gated on the loop-*definition* `label` node instead of the
  `identifier` jump target, so labeled jumps silently scored `+0`.
  Kotlin labeled `break@outer`/`continue@outer` (modelled as
  `labeled_expression`, with no dedicated jump kind in
  tree-sitter-kotlin-ng) now add `+1`; bare `break`/`continue` and
  `return@label` stay `+0`. Brings Perl and Kotlin to Java/JS/Go
  parity. Metric-value change for affected sources; derived MI shifts
  accordingly.
- Cognitive complexity no longer double-counts the `else` arm of a
  Ruby `case`/`when`
  ([#451](https://github.com/dekobon/big-code-analysis/issues/451)).
  The `case` node already pays nesting (`+1`), but the shared
  `else` arm added another `+1`, scoring `2` for
  `case x; when 1 then 1; else 0; end` versus `1` without the
  `else`. The `else` of a `case`/`when` is the default arm of a
  switch-like construct and now adds `+0`, matching Kotlin
  `when`-`else`, Java/C#/PHP switch-`default`, and the SonarSource
  spec. The `else` of an `if`/`elsif` chain and the no-exception
  `else` of `begin`/`rescue` (mirroring Python `try`/`except`/`else`)
  still add `+1`. Metric-value change for affected Ruby sources;
  derived MI shifts accordingly.
- Cyclomatic complexity now counts safe-navigation operators for Ruby
  (`&.`) and Groovy (`?.` / `??.`)
  ([#452](https://github.com/dekobon/big-code-analysis/issues/452)).
  These short-circuit operators were already counted as decision
  points for Kotlin (`?.`), PHP (`?->`), JS/TS (`?.`), and C#
  (conditional access), but Ruby and Groovy silently scored `+0`,
  making cyclomatic non-comparable across languages. Each operator
  now adds `+1` to both standard and modified CCN per link, so
  `a&.b&.c` / `a?.b?.c` scores `+2` like the Kotlin/PHP analogues.
  The stale comment blaming amaanq's grammar for Groovy ERROR nodes
  is corrected to describe the pinned dekobon grammar. Metric-value
  change for affected Ruby/Groovy sources; derived MI shifts
  accordingly.
- `bca count` no longer aborts the whole worker pool when one worker
  panics: `Count::call` now recovers the poisoned shared `stats`
  mutex (via `into_inner()` + `clear_poison()`) instead of cascading
  a `.lock().unwrap()` panic across every subsequent worker and the
  CLI's final `into_inner()`
  ([#445](https://github.com/dekobon/big-code-analysis/issues/445)).
  Mirrors the #425 degrade-on-poison pattern; the aggregation is two
  monotonically-incremented counters, so a panicked peer leaves at
  worst a slightly-low tally, never an unsafe state.

- The Java Exit (`nexits`) metric now counts the switch-expression
  `yield` statement as an exit point, matching the existing Groovy and
  C# behaviour
  ([#434](https://github.com/dekobon/big-code-analysis/issues/434)).
  Java 14+ switch expressions use `yield value;` to hand a value back,
  which is an explicit exit; the Java arm previously matched only
  `return` / `throw`, under-counting exits for any method using a
  switch expression and diverging from its sibling languages.

- Kotlin `companion object { ... }` now opens its own `Class`
  func-space, consistent with named `object` declarations
  ([#431](https://github.com/dekobon/big-code-analysis/issues/431)).
  `companion_object` is a distinct grammar node that was absent from
  both `KotlinCode::is_func_space` and the Kotlin `get_space_kind`
  arm, so companion members were silently attributed to the enclosing
  class — inconsistent with a named `object Foo { ... }`, which opens
  its own space. The grammar models a companion's body as a
  `class_body` of methods and properties exactly like a named object,
  so it is now mapped to `SpaceKind::Class`. Its space name comes from
  the optional `name` field (the declared name for `companion object
  Named`, `<anonymous>` for the nameless form). Per-space metric
  attribution (cognitive, Halstead, `wmc`, `nom`, `npa`, `npm`) for
  Kotlin code using companion objects is now correct; file-level
  aggregates are unchanged (no member is lost or double-counted).
- Groovy `Checker::is_call` no longer counts constructor invocations
  (`new Foo()`, `object_creation_expression`) as call sites, aligning
  it with the Java-family convention
  ([#430](https://github.com/dekobon/big-code-analysis/issues/430)).
  Java's `is_call` is `MethodInvocation` only and C#'s is
  `InvocationExpression` only; Groovy diverged by also matching
  `ObjectCreationExpression`, so the CLI `call` filter / `--ops`
  feature counted `new Foo()` as a call in Groovy but not in Java or
  C#. `is_call` now matches `MethodInvocation | CommandChain` only
  (`command_chain` is Groovy's juxtaposed command-call form and stays
  a genuine call). ABC counting is unaffected: Groovy's ABC `branches`
  still counts constructors via its separate `New` arm. This changes
  only the `call`/`--ops` CLI counts for Groovy sources that construct
  objects; it does not touch any value in the `Metrics` struct or any
  snapshot.
- C# `enum` declarations now report `kind: "class"` instead of
  `kind: "unknown"` in the per-space output, matching the existing
  Java, PHP, and Groovy treatment of enum declarations
  ([#429](https://github.com/dekobon/big-code-analysis/issues/429)).
  A C# `EnumDeclaration` opens a `FuncSpace` (it is listed in
  `is_func_space`), but `CsharpCode::get_space_kind` had no matching
  arm, so the space fell through to `SpaceKind::Unknown`.
- The per-function averages for `Cognitive`, `Exit`, and `NArgs` no
  longer emit `inf`/`NaN` (serialized as `null`)
  ([#428](https://github.com/dekobon/big-code-analysis/issues/428)).
  These three averages divide by a function/closure count sourced from
  `Nom`, but `Metric::dependencies` did not declare `Nom` as a
  dependency of any of them — so selecting one through the public
  `MetricsOptions::with_only` API without also selecting `Nom` left the
  divisor at the `Stats` default (0), producing `inf`/`NaN`. Two
  defenses now apply: (1) `Metric::dependencies` declares `Nom` for
  `Cognitive`, `Exit`, and `NArgs`, so `with_only` pulls in the
  function counter automatically (matching how `Wmc` already declares
  `Nom`); and (2) `cognitive_average` and `exit_average` guard their
  divisor with `.max(1)`, matching the existing `nargs_average`, so a
  zero divisor degrades to `sum / 1` instead of `inf`/`NaN`. As a
  consequence, spaces that genuinely contain zero functions now report
  `average: 0` rather than `null` (consistent with `NArgs`, which has
  always used the guard). Both changes are additive runtime-behaviour
  fixes; `Metric::dependencies` keeps its `&'static [Metric]` shape.
- `Loc` per-function min/max (`sloc_min` / `sloc_max`, `ploc_*`,
  `lloc_*`, `cloc_*`, `blank_*`) now reflect nested function spaces
  instead of only top-level spaces
  ([#437](https://github.com/dekobon/big-code-analysis/issues/437)).
  The `merge` step folded each child space's *aggregate* value
  (`other.sloc()`) into the parent min/max rather than the child's own
  `min`/`max`, and a guarded `compute_minmax` skipped a container's own
  span once a child had folded — so a file containing classes with
  methods (or nested functions) reported min/max over the outermost
  spaces only, dropping the smallest/largest leaf. `merge` now folds
  `other.X_min`/`other.X_max` and `compute_minmax` folds the space's own
  value unconditionally, matching every sibling metric (cyclomatic,
  cognitive, exit, nargs, nom, tokens, abc): containers participate in
  min/max, and nested leaves propagate to the root. The `blank()`
  value is additionally clamped at `0.0` — its
  `sloc - ploc - only_comment_lines` subtraction could go negative and
  an `as usize` cast at the merge site saturated that to `0`, corrupting
  `blank_min`; the serialized `blank` can no longer be negative. This
  changes LOC min/max for every language whose files contain nested
  spaces.
- The `Npa` (`class_cda` / `interface_cda` / `total_cda`) and `Npm`
  (`class_coa` / `interface_coa` / `total_coa`) accessibility-ratio
  accessors no longer return `NaN` (serialized as JSON `null`) for an
  attribute-less / method-less class or interface
  ([#438](https://github.com/dekobon/big-code-analysis/issues/438)).
  The ratios divided the public-member count by the total-member count
  with no zero-guard, so `class Foo {}` produced `0.0 / 0.0 = NaN`. A
  shared `accessibility_ratio` helper now returns `0.0` when the divisor
  is zero — an empty type exposes no public surface, so `0.0` is the
  defined value, consistent with the codebase's existing empty-divisor
  convention (`mi.comments_percentage`). The convention is applied
  uniformly across every class / interface / total CDA and COA accessor.
  Metric-value drift: previously-`null` `classes_average` /
  `interfaces_average` / `average` fields now read `0.0` for empty
  types.

- The preprocessor no longer panics when an `#include` `string_literal`
  is shorter than its two surrounding quotes
  ([#432](https://github.com/dekobon/big-code-analysis/issues/432)).
  `preprocess` stripped the quotes by slicing `code[start + 1..end - 1]`
  before validating the node length, so a malformed/error-recovered span
  (e.g. a truncated `#include "` with no closing quote) built a reversed
  range and panicked. The quote-stripping is now isolated in a
  `strip_include_quotes` helper that rejects any span unable to hold both
  quote bytes (returning `None` and skipping the include) before slicing.

- Python `to_sarif` no longer emits a file-scope `abc` finding the CLI
  never produces
  ([#441](https://github.com/dekobon/big-code-analysis/issues/441)).
  The `abc` threshold was flagged `skip_at_unit: false`, but the JSON
  `abc.magnitude` headline is serialized from `magnitude_sum()` (the
  aggregate across descendant spaces) while the CLI's threshold
  accessor is the per-space `m.abc.magnitude()` — the same
  sum-vs-per-space divergence as `cyclomatic` / `cyclomatic.modified`
  / `cognitive`, which are already skipped at the unit space. `abc` is
  now `skip_at_unit: true`, restoring byte-for-byte parity with `bca
  check -O sarif`. The unit test that enumerated the skip set was
  reworked to document the JSON-aggregate-vs-CLI-accessor property
  (and why `nexits`, whose JSON path also ends in `sum`, is correctly
  not skipped) so it guards the invariant rather than enshrining the
  bug.
- `bca-web --num-jobs 0` no longer panics the server at startup
  ([#427](https://github.com/dekobon/big-code-analysis/issues/427)).
  The flag was an unconstrained `Option<usize>`, so `0` flowed through
  to a zero-permit `Semaphore` (blocking every parse forever) and to
  actix-server's `ServerBuilder::workers`, which asserts the worker
  count is non-zero and aborted the process. `--num-jobs` is now a
  `clap` range-validated `Option<u32>` (`range(1..)`) that rejects `0`
  at parse time with a clear validation error; the flag-omitted path
  still defaults to `available_parallelism()` (always >= 1).
- **Security (DoS):** the web server's `application/octet-stream`
  endpoints (`/comment`, `/metrics`, `/function`) no longer accept an
  unbounded request body
  ([#426](https://github.com/dekobon/big-code-analysis/issues/426)).
  These handlers read the body with the raw `web::Payload` extractor,
  which ignores the `web::PayloadConfig` size limit the routes carried,
  so the body was buffered with no cap — an attacker could stream an
  arbitrarily large body and exhaust server memory before admission
  control ever ran. `get_code` now enforces the configured
  `max_body_size` (4 MiB) incrementally, rejecting with `413 Payload
  Too Large` as soon as the running total would exceed the limit rather
  than buffering the whole body first. The dead per-route
  `PayloadConfig` is removed.
- The `bca-diff-baseline(1)` man page is now shipped in the `.deb` and
  `.rpm` packages
  ([#444](https://github.com/dekobon/big-code-analysis/issues/444)).
  The page was generated by `cargo xtask` when `bca diff-baseline` was
  added but never listed in either packaging asset array in
  `big-code-analysis-cli/Cargo.toml`, so the packages omitted it.
- The C/C++ preprocessor worker dispatcher no longer panics on a
  poisoned results mutex
  ([#425](https://github.com/dekobon/big-code-analysis/issues/425)).
  `dispatch_preproc` acquired the shared `preproc_lock` with
  `.expect("mutex not poisoned")`, so a single panicking preproc
  worker cascaded a panic across every remaining worker and aborted
  the pool. It now degrades like the `check` and `report` worker
  dispatchers — logging a warning and returning `Ok(())` for that
  file — so one worker fault no longer escalates to a full-pool abort.
- `bca check --github-annotations` overflow rollup no longer re-breaches
  the GitHub Actions 10-error/step quota. The emitter wrote one
  uncapped `::error::` rollup line per overflowing metric, so a run that
  tripped many distinct metrics could emit `10×k + k` error annotations
  — exceeding the very cap the per-metric limit exists to enforce. The
  overflow is now consolidated into a single `::notice::` line that
  carries the total hidden count and the per-metric breakdown; a notice
  lives outside the error band and one line cannot multiply with the
  metric count, so the total `::error::` annotations stay bounded
  regardless of how many metrics overflow.
  Fixes [#440](https://github.com/dekobon/big-code-analysis/issues/440).
- `bca check --github-annotations` no longer undercounts violations
  with non-UTF-8 paths in the per-metric overflow rollup. The emitter
  incremented the cap counter *before* skipping a non-UTF-8 path, so a
  skipped violation consumed a cap slot, emitted no annotation, and was
  never rolled into the "N more … not shown" line — making the
  user-visible total (shown + overflow) fall short and displacing a
  later real annotation into overflow. The path is now resolved before
  the counter is touched, so a skip leaves both tallies untouched.
  Fixes [#424](https://github.com/dekobon/big-code-analysis/issues/424).
- `bca` per-file `--output` writers no longer collapse a `..`
  (`ParentDir`) input segment to a no-op `.`, which let distinct inputs
  (`../sibling/x.rs` vs `sibling/x.rs`) clobber the same output file.
  `..` now maps to a collision-free `%2E%2E` marker and any literal `%`
  in a component is escaped to `%25`, so the input → output filename
  mapping is injective. Fixes
  [#423](https://github.com/dekobon/big-code-analysis/issues/423).
- Python NArgs no longer counts the PEP 570 positional-only `/` and
  PEP 3102 keyword-only `*` parameter separators as arguments. The
  grammar emits them as `positional_separator` / `keyword_separator`
  children of the `parameters` list; both are now excluded. Fixes
  [#414](https://github.com/dekobon/big-code-analysis/issues/414).
- Python LOC now counts the interior rows of a multi-line, non-docstring
  string literal (assigned to a variable or passed as an argument) as
  `ploc` instead of `blank`. Fixes
  [#415](https://github.com/dekobon/big-code-analysis/issues/415).
- Python NPA: foreign-object writes (`db.x = …`) no longer count as
  class attributes; tuple/list-unpacking (`self.a, self.b = …`) and
  multi-target class-level assignments (`a = b = 3`, `p, q = 1, 2`)
  are now counted per bound name; and a class-level default and an
  instance write of the same name are deduplicated. Fixes
  [#412](https://github.com/dekobon/big-code-analysis/issues/412).
- Python Halstead operator classification: `await` is no longer
  double-counted, `lambda` / `match` / `case` / `nonlocal` are now
  counted, and `not in` / `is not` count as one operator each (MI,
  which consumes Halstead volume, shifts accordingly). Fixes
  [#413](https://github.com/dekobon/big-code-analysis/issues/413).
- Python cognitive complexity no longer charges +1 for a `finally`
  clause; structured cleanup adds 0 per the SonarSource spec, so
  try/except/finally now scores the same as try/except. Fixes
  [#416](https://github.com/dekobon/big-code-analysis/issues/416).
- Python cyclomatic complexity no longer counts `with` / `async with`
  as a decision point; it is unconditional resource management,
  matching the C-family `using` sibling and standard McCabe. Fixes
  [#418](https://github.com/dekobon/big-code-analysis/issues/418).
- Python cognitive complexity now counts comprehension and
  generator-expression `for` / `if` clauses, matching cyclomatic
  complexity and the equivalent explicit loop+condition. Fixes
  [#417](https://github.com/dekobon/big-code-analysis/issues/417).
- Python cognitive complexity no longer under-counts comprehensions
  nested in another comprehension's element expression (e.g.
  `[[y for y in x if y] for x in xs if x]`); they now match the
  equivalent explicit nested loop+filter form at every depth (refines
  [#417](https://github.com/dekobon/big-code-analysis/issues/417)).
  Fixes [#421](https://github.com/dekobon/big-code-analysis/issues/421).
- Cognitive complexity now counts Rust `loop {}` as a nesting
  construct. The arm in `src/metrics/cognitive.rs` matched
  `ForExpression | WhileExpression | MatchExpression` but omitted
  `LoopExpression`; every `loop {}` block silently scored zero
  structural / nesting contribution while structurally identical
  `for` / `while` loops scored correctly. Cyclomatic was already
  correct. Fixes
  [#389](https://github.com/dekobon/big-code-analysis/issues/389).
- Halstead now classifies Rust `FieldIdentifier` and
  `TypeIdentifier` as operands. `p.x + p.y` previously counted
  `p` but dropped `x`/`y`; `Vec<i32>` dropped `Vec`. C++ and Go
  already classified both; Rust now matches. Fixes
  [#390](https://github.com/dekobon/big-code-analysis/issues/390).
- The alterator now flattens Rust `RawStringLiteral` like
  `StringLiteral` / `CharLiteral`. AST dump output for `r#"…"#`
  no longer renders structured children (start delimiter,
  content, end delimiter) — it produces a single flat text node
  to match `"…"`. `Checker::is_string` and `Getter::get_op_type`
  already treated both as equivalent. Fixes
  [#391](https://github.com/dekobon/big-code-analysis/issues/391).
- The alterator now flattens C++ `RawStringLiteral` the same way
  (peer of #391). Fixes
  [#398](https://github.com/dekobon/big-code-analysis/issues/398).
- Halstead now classifies Rust `COLONCOLON` (`::`) and 14
  declaration / visibility keywords (`Const`, `Static`, `Enum`,
  `Struct`, `Trait`, `Impl`, `Use`, `Mod`, `Pub`, `Type`,
  `Union`, `Where`, `Extern`, `Dyn`) as operators, matching
  C++ / Java / C# / Kotlin parity. Path-heavy and declaration-
  heavy Rust code previously had deflated n1 / N1 / volume /
  effort estimates. Fixes
  [#394](https://github.com/dekobon/big-code-analysis/issues/394).
- Cognitive complexity now counts `&&` operators inside Rust
  2024 let-chains. `if let Some(x) = a && let Some(y) = b && x >
  y` previously contributed zero boolean-sequence cost because
  the `BinaryExpression`-only dispatch missed `&&` tokens
  directly under `LetChain` / `LetChain2`. Cyclomatic already
  counted them. Fixes
  [#396](https://github.com/dekobon/big-code-analysis/issues/396).
- ABC: C# / Java / Groovy condition walkers now count
  `MemberAccessExpression`, `AwaitExpression`, `CastExpression`,
  `IsPatternExpression`, `ElementAccessExpression` (and the
  per-language equivalents) as conditions. Idioms like
  `if (cfg.Enabled)`, `if (await CheckAsync())`, `if ((bool)v)`,
  `if (x is not null)`, `if (flags[0])` no longer silently score
  zero. Fixes
  [#372](https://github.com/dekobon/big-code-analysis/issues/372).
- ABC: Rust `LetDeclaration` and C++ `InitDeclarator` carrying an
  explicit `=` initializer now count toward the A-count. The
  literal Fitzpatrick reading is restored, matching JavaScript's
  treatment of `let x = expr;` (while `const x = expr;` /
  declarations without initializer still contribute zero). Fixes
  [#393](https://github.com/dekobon/big-code-analysis/issues/393).
- `bca check` integration tests no longer leak TempDir fixture
  paths into the GHA runner's `$GITHUB_STEP_SUMMARY`. The shared
  `bca_command()` test builder scrubs `GITHUB_STEP_SUMMARY` and
  `GITHUB_ACTIONS` from the child environment, so a failing
  `test` job stops appearing as a "threshold gate" failure in
  the runner's UI. Fixes
  [#388](https://github.com/dekobon/big-code-analysis/issues/388).
- Vendored tree-sitter scanner builds (`tree-sitter-ccomment`,
  `tree-sitter-preproc`, `tree-sitter-mozcpp`) suppress
  `-Wsign-compare` warnings emitted when scanner sources compare
  `lexer->lookahead` (int32_t) against `wchar_t` containers. The
  suppression lives in each crate's `build.rs` (not the C/C++
  source) so it survives grammar regeneration. Fixes
  [#399](https://github.com/dekobon/big-code-analysis/issues/399).
- Checkstyle output (`bca metrics --output-format checkstyle`) now
  substitutes plane-end Unicode non-characters (`U+FFFE`, `U+FFFF`,
  and every supplementary-plane counterpart `U+nFFFE` / `U+nFFFF`
  for plane `n` in `1..=16`) with `?` in attribute values, matching
  the existing C0-control handling. The BMP pair is forbidden by
  XML 1.0 §2.2's `Char` production; the 32 supplementary-plane
  non-characters are permitted by the spec but rejected by strict
  libxml2-based consumers (Jenkins, SonarQube). Fixes
  [#349](https://github.com/dekobon/big-code-analysis/issues/349).
- `release.yml` `Compute SHA256SUMS` step now records bare basenames
  via `find -printf '%f\0'` instead of `./`-prefixed paths, so the
  downstream `post-publish verify` job matches without `./` prefix
  gymnastics. Fixes
  [#351](https://github.com/dekobon/big-code-analysis/issues/351).
- `release.yml` `run:` blocks now use `set -euo pipefail` instead of
  `set -eu`, so a producer failure mid-pipe no longer silently masks
  as success. The canonical risk was the SHA256SUMS producer
  (`find … | sort -z | xargs -0 sha256sum > SHA256SUMS`), where a
  pre-`xargs` failure would have left an empty checksum file that the
  signing step would then attest. The `llvm-objcopy --version | head`
  call and the four `curl … | grep -q` crates.io index lookups are
  rewritten to capture-then-match so `pipefail` cannot flip them to
  the wrong branch on producer SIGPIPE. Fixes
  [#363](https://github.com/dekobon/big-code-analysis/issues/363).
- `release.yml` `Flatten into release/` step now fails loudly on
  basename collisions instead of silently dropping the second writer.
  The previous `find … -exec cp -n {} release/ \;` relied on the
  unenforced invariant that every matrix artefact has a distinct
  basename; a future overlapping platform suffix or job-matrix typo
  would have shipped a subset of the release with no CI signal
  (the downstream `post-publish verify` job would only notice when a
  `gh release download` came back empty for the missing triple).
  The step now reads `find … -print0` into a NUL-delimited while
  loop, checks for an existing target, and emits
  `::error::flatten collision: …` before `exit 1`. The step also
  asserts that `incoming/` exists and that at least one artefact was
  copied — process substitution does not propagate find's exit
  status and `pipefail` does not cover `< <(…)` redirection, so
  without explicit guards an upstream artifact-download regression
  would silently produce an empty release/. Fixes
  [#364](https://github.com/dekobon/big-code-analysis/issues/364).
- `release.yml` crates.io publish skip-checks (preflight dry-run plus
  the four publish steps) now substring-match the sparse-index body
  via bash `[[ "$BODY" == *"$NEEDLE"* ]]` instead of `printf '%s'
  "$BODY" | grep -q` / `echo "$BODY" | grep -q`. Glob matching has no
  pipe (so no SIGPIPE risk under pipefail when grep -q exits early on
  a large body) and no regex semantics (dots in semver versions never
  match arbitrary characters). Code-review follow-up to #363.
- ABC metric: the dekobon tree-sitter-groovy condition classifier
  in `groovy_count_condition`, `groovy_inspect_container`, and
  `groovy_count_unary_conditions` now matches the `BooleanLiteral`
  wrapper node (kind_id 270) instead of the leaf `True` / `False`
  keyword tokens. The Groovy grammar mirrors the C# grammar's
  wrapping convention for boolean literals, so the same class of
  silent miscount affected Groovy `if (true)` / `while (false)` /
  ternary / `!true` / `&&` + literal-operand chains. Companion to
  the C# fix below; surfaced by a parallel-language audit of the
  C# fix.
- ABC metric: the tree-sitter-c-sharp condition classifier in
  `csharp_count_condition`, `csharp_inspect_container`, and
  `csharp_count_unary_conditions` now matches the `BooleanLiteral`
  wrapper node instead of the leaf `True` / `False` keyword tokens.
  The grammar wraps a bare `true` / `false` used as the condition of
  `if` / `while` / `do-while` / `?:` in a `boolean_literal` named
  node; the leaf tokens never appear at that position, so every
  literal-condition statement (`if (true)`, `while (false)`, `!true`,
  `!false`, etc.) silently scored zero conditions. The for-loop
  helper `csharp_walk_for_statement` was already correct, which
  isolated the asymmetry to the if / while / do / ternary / unary
  paths. The inline
  `csharp_declarations_with_conditions` snapshot rises from 2 to 4
  conditions because `!true` / `!false` now correctly contribute;
  the three C# integration fixtures (`anonymous.cs`, `generics.cs`,
  `strings.cs`) contained no literal-condition statements that
  this fix changes — their counts were already correct after the
  #370 walker fix landed in submodule commit `630ba5be`.
  Fixes [#371](https://github.com/dekobon/big-code-analysis/issues/371).
- ABC metric: the tree-sitter-c-sharp condition walker now targets
  the correct child index — child(2) for `if` / `while` and child(4)
  for `do-while`, where the actual condition expression lives. The
  pre-fix dispatch pointed at child(1) / child(3), which are the
  literal `(` / `while` token children, so every bare-identifier or
  `!`-prefixed unary C# condition silently scored zero. Comparison
  operators continued to be counted via the GT / LT / EQEQ token
  arms (which is why no existing test caught the bug). A new
  `csharp_count_condition` helper mirrors the existing
  `groovy_count_condition` shape and is now reused by
  `csharp_walk_conditional` to keep the ternary classifier in sync.
  Integration snapshots for C# files containing bare-identifier,
  unary, or method-call conditions (`anonymous.cs`, `generics.cs`,
  `strings.cs`) shift upward to reflect the corrected counts. Fixes
  [#370](https://github.com/dekobon/big-code-analysis/issues/370).
- `release.yml` `Compute SHA256SUMS` step now passes `-r`
  (`--no-run-if-empty`) to `xargs` so empty input produces an empty
  `SHA256SUMS` instead of GNU xargs's phantom empty-string hash
  under filename `-`. A defense-in-depth `[ -s SHA256SUMS ]` assert
  fails the step loudly with an `::error::` annotation when the
  find pipeline produced no artefacts. Fixes
  [#365](https://github.com/dekobon/big-code-analysis/issues/365).
- `release.yml` `Compute tap/bucket SHA-256s` step now assigns each
  `get` / `getzip` result to a local variable before emitting it to
  `$GITHUB_OUTPUT`. `echo "X=$(get …)"` previously swallowed
  command-substitution failures under `set -e` because `echo`'s
  exit status (always 0) was the controlling one — a missing
  tarball would silently render `X=` and publish empty SHA-256
  placeholders into the Homebrew formula / Scoop manifest. Fixes
  [#366](https://github.com/dekobon/big-code-analysis/issues/366).
- `release.yml` `Generate CycloneDX SBOMs` step now uses a
  `cp_no_clobber` helper that asserts `! [ -e "$dst" ]` before
  copying. The three SBOM `cp` invocations had no collision guard,
  so a future build artefact or SBOM rename sharing a basename
  would have silently overwritten a flattened release file —
  exactly the failure mode the flatten guard from #364 was added to
  prevent. Fixes
  [#367](https://github.com/dekobon/big-code-analysis/issues/367).
- `release.yml` `Sign SHA256SUMS with minisign` step now applies a
  symmetric `[[ -z "${MINISIGN_PASSWORD:-}" ]]` guard mirroring the
  existing `MINISIGN_SECRET` check. An unset password previously
  reached `printf | minisign` and surfaced as a cryptic
  `Wrong password for that key` from minisign rather than the
  actionable `::error::MINISIGN_PASSWORD not configured` annotation
  pattern used elsewhere. Fixes
  [#368](https://github.com/dekobon/big-code-analysis/issues/368).
- Metric dispatch: doc comment above
  `src/spaces.rs::compute_per_node` (the per-node `compute` chain
  that dispatches **every** metric, not just ABC) no longer claims
  the metric-gating bit test is "AND-and-compare on a u16" — the
  `MetricSet` storage was widened from `u16` to `u32` in #339, and
  the explanatory parenthetical now references the `MetricSet`
  bitfield generically so the rationale survives any future
  widening. Fixes
  [#348](https://github.com/dekobon/big-code-analysis/issues/348).

- The markdown report (`bca --output-format markdown`) no longer
  corrupts File-column values that contain a backslash
  ([#439](https://github.com/dekobon/big-code-analysis/issues/439)).
  Its cell escaper escaped `|` and newlines but not `\`, so a GFM
  table cell treated `\` as an escape introducer — `src\main.rs`
  rendered as `srcmain.rs` and `a\|b` broke out of the cell.
  `escape_cell` now delegates to the `check` report's
  `escape_gfm_cell`, which escapes `\` before `|`, so the two
  GFM-table emitters share one escaping policy.
- Bash LOC `blank` was undercounted by one for sources where the
  grammar anchors a zero-width code leaf to a standalone comment's row
  (e.g. a `$(...)` command substitution containing only a comment).
  The `Loc` leaf arm now reclassifies that row from comment-only to
  code-comment via `check_comment_ends_on_code_line`, mirroring Elixir
  and every other language impl; the row is no longer double-credited
  to both `ploc` and the comment-only set. Affects metric *values* for
  Bash sources with that shape (#547).
- Per-language ABC / Cognitive / NOM computation divergences (#696).
  Ruby now counts the bare predicate of `if`/`unless`/`while`/`until` as
  one ABC condition (restoring conditions ≥ cyclomatic decisions); Bash
  counts `if`/`elif`/`while` and each non-wildcard `case` arm; Kotlin
  counts the `try` keyword alongside `catch`. C++/Java/C#/Groovy now reset
  Cognitive nesting and bump function-depth at nested function boundaries
  (C# also for local functions), matching the other 9 families. `Nom`
  takes the source bytes and dispatches via `is_func_with_code`, so
  Elixir's `def`/`defp`/`defmacro` count as functions instead of leaving
  `functions_sum` at 0. Affects metric *values* in the listed languages.
- `is_comment` misclassification: the JS-family predicate matched only
  `comment`, missing `html_comment` (Annex-B `<!-- -->`), and Groovy
  missed `groovydoc_comment`. This skewed every consumer — `loc` counted
  those comments as PLOC instead of CLOC (and so `mi` comments_percentage),
  `tokens` counted the comment leaves, and `bca find comment` /
  strip-comments ignored them. `HtmlComment` is added to all 4 JS arms,
  `GroovydocComment` to Groovy (#697).
- AST-dump string flattening now agrees with `Checker::is_string`: the
  alterator kept the structured children of interpolating string literals
  that `is_string` already treats as a single string, producing
  per-language dump asymmetries. JS-family `TemplateString`, Cpp
  `ConcatenatedString`, and Python/Java/Kotlin string-like literals are now
  flattened; Go `RuneLiteral` is deliberately left out (a rune is a
  character, not a string). Dump-only change — no metric values move (#699).
- **(breaking)** C/C++ preprocessor: `fix_includes` now returns a
  `Vec<PreprocDiagnostic>` (a new public enum) instead of writing
  include-resolution warnings to stderr via `eprintln!`, so an embedder
  (`bca-web`) can capture or discard them; the CLI prints them to stderr.
  Also: `#undef X` no longer records `X` as *defined* (directives are now
  replayed in source order, so `#undef` removes a preceding `#define`'s
  macro), and an ambiguous `#include` now resolves to a single
  deterministic target (the lexicographically smallest tied path) instead
  of fanning out one edge per candidate and leaking macros. Corrects the
  macro set feeding `c_macro::replace` (#705).
- VCS `--as-of` anchoring: the plain ranking walk re-based only the window
  arithmetic, leaving the walk anchored at HEAD, so commits in the future
  of `as_of` were still counted. `build()` (and the cached `as_of`
  entry point) now re-anchor at the mainline tip at-or-before `as_of`
  (reusing trend's `tip_at_or_before`); an `as_of` before the first commit
  yields an empty snapshot rather than an error. The Python and CLI cache
  tests were updated to exercise the cache with a default HEAD-anchored
  walk, since a historical `as_of` now bypasses the HEAD-keyed persistent
  cache (#648).
- VCS bit-identity / determinism (restoring #334): co-change entropy now
  sums edge weights in `FileId`-sorted order so the non-associative float
  fold cannot diverge by ULPs between the live walk and a cache replay;
  `resolve_alias` gains cycle detection so a rename-back cycle terminates
  deterministically; the bus-factor degenerate tie-break breaks on the
  hashed id and drops empty-contribution files; and
  `stats::Accumulator::record` switches to `saturating_add` (#701).
- VCS risk-score / JIT / scope correctness: `FileTypeScope` rejects
  interior-dot entries (`d.ts`, `tar.gz`) that `Path::extension()` can
  never match instead of silently ranking none; the JIT experience walk
  routes through `ParticipantResolver` (author + bot-filtered co-authors)
  so co-authored history counts and a filtered bot no longer inflates
  experience; `file_risk_max` is clamped finite and non-negative before the
  JIT total so an inf prior cannot violate the ordinal invariant; and the
  `Vcs` wire f64 fields carry the #531 `non_finite` (de)serialization so a
  future NaN emits null rather than erroring (#702).
- Terminal dumps no longer risk an uncatchable stack-overflow abort: all
  three dumpers (AST, ops, metrics) walk with an explicit work stack
  instead of recursion, so a pathologically deep tree cannot overflow the
  thread stack at dump time. `dump_ops` also renders the mid-child glyph
  for the operators connector since operands always follow it (#700).
- **(breaking for stderr scrapers)** Every true stderr diagnostic is now
  funneled through three severity helpers — `die()` (`error:`, was
  capitalized `Error:`), `warn()` (`warning:`), and `note()` (`note:`) —
  matching clap's lowercase rustc/cargo/git prefix family. Capitalized
  `Warning:` literals and the redundant `bca: warning:` double prefix are
  gone; status/result lines keep their `bca <cmd>:` prefix. Message bodies
  are unchanged — only the prefix moved (#609).
- CLI seed-expansion, path-handling, and diff extraction robustness:
  overlapping seeds de-duplicate a file so it is analyzed once; a per-entry
  walk error skips-with-warning instead of aborting the run; seeds are
  classified via `symlink_metadata` (symmetric with the walk's
  `follow_links(false)`); `--paths-from` tolerates non-UTF-8 path lines.
  `bca diff` replaces the system-`tar` shell-out with the in-process `tar`
  crate (removing the BSD-vs-GNU empty-archive gap), recognizes the
  SHA-256 (64-zero) null-ref sentinel, and rejects `--since` selectors that
  escape the tree via `..`. `--print-effective-config` now includes the
  gate-affecting `report_suppressed` field, and `CyclomaticStats::max` is
  guarded against `NaN` on an empty entry set (#704).
- The per-function VCS hotspot now scores each function by its own
  `cyclomatic()` complexity, not the subtree `cyclomatic_sum()` rollup, so
  a nested function's complexity is no longer re-counted at every enclosing
  level (the file-level inject keeps `cyclomatic_sum()`). `bca vcs commit` /
  `vcs trend` `--output` now create missing parent directories, matching
  the sibling single-file output paths (#709). `get_emacs_mode` /
  `get_vim_mode` now scan a fixed window of real (non-empty) lines at each
  end so a mode-line on the 5th line or after trailing blanks is no longer
  missed; `Count::Display` guards `total == 0` to report `0.00%` instead of
  `NaN%`; and `cfg_predicate_marks_test` walks an explicit work stack
  instead of recursing, so pathological `all(all(all(…)))` input cannot
  overflow the stack (#709).
- Web: `/vcs/jit` no longer scores wholly-non-diff garbage as a confident
  `partial_risk_score: 0.0` — a non-empty, non-whitespace `diff` that
  parses to zero touched files is rejected with `400 vcs_invalid_diff`,
  while an empty/whitespace-only diff still scores a valid 0.0 (#652). A
  nonexistent `repo_path` now answers a consistent `400 vcs_not_a_repository`
  (it previously returned 500 via gix's environment-level error) across
  `/vcs`, `/vcs/trend`, and `/vcs/jit` commit mode (#653). `bca-web` now
  exits non-zero on a server-run failure, signals a dropped non-UTF-8
  repo-relative path via `tracing::warn` instead of vanishing silently, and
  documents the `parse_timeout_secs = 0` coupling (#707).
- Python bindings CLI-parity: `analyze_path` routes the on-disk read
  through the CLI walker's `read_file_with_eol`, inheriting its
  pre-dispatch skips (≤3-byte cutoff, BOM stripping, non-UTF-8 binary head)
  and EOL normalisation (each skip returns `None`); a directory named
  directly surfaces a hard `IsADirectoryError` rather than a silent skip.
  SARIF `logicalLocations` now emit the CLI's qualified `Container::method`
  symbol and collapse anonymous spaces to `<anon@L{line}>` (#706).
- Build/CI helper scripts and enums codegen hardening (tooling only, no
  shipped-library change): `check-grammar-crate.py` runs subprocesses with
  `check=True` and prints help / exits 2 on a missing subcommand;
  `check-versions.py` matches an internal-dep `version` pin split across
  lines of a multi-line inline table; `check-snapshot-anchors.py` skips
  snapshots inside `/* … */` block comments; `check-manpage-assets.py`
  fails loud on an unrecognized layout; `check-grammar-marker-sync.py`'s
  `--update` consumes backslash escapes; and `enums/data/{mac,special}.py`
  stop generating non-existent names and resolve their data file relative
  to the script (#708).

### Security

- The VCS-report CSV writer (`bca vcs --format csv`) now defangs the
  free-text `path` column against spreadsheet formula injection
  (CWE-1236), closing the second emission path that bypassed the #703 fix
  in the standard CSV output (#794).
- The `--emit-author-details` author hash is now documented honestly as a
  stable pseudonym (it avoids emitting plaintext emails and deters casual
  disclosure) rather than "irreversible": an unsalted SHA-256 of a
  low-entropy, enumerable email is recoverable against a candidate email
  set (the Gravatar weakness). Opt-in keyed/salted hardening is tracked
  as a follow-up (#811).

- CSV output now defangs spreadsheet formula injection (CWE-1236): a
  `path` or `space_name` cell beginning with `=` `+` `-` `@` tab or CR is
  prefixed with `'` so a spreadsheet app treats it as literal text rather
  than executing it as a formula. Companion fix: the Code Climate
  `fingerprint` no longer collides when two distinct findings share the
  same `path\0function\0metric` triple in one file — an ordinal is folded
  into the hash so each finding gets a distinct fingerprint, while the
  first occurrence keeps its historical (byte-identical) value so the
  frozen GitLab-dedup contract holds (#703).

## [1.1.0] - 2026-05-25

### Added

- `bca --exclude-from <FILE>` global flag reads `--exclude` glob
  patterns from a file (one per line, `.gitignore`-style: blank lines
  and `#`-prefixed comments are skipped). Patterns union with any
  inline `--exclude <GLOB>` flags into a single deny-set. The
  convention is a `.bcaignore` at the repo root so workflow,
  recipe, and local baseline-bootstrap can share one source of truth.
  Use `-` for stdin. Fixes
  [#355](https://github.com/dekobon/big-code-analysis/issues/355).

- **Python bindings shipped** — close-out of
  [#103](https://github.com/dekobon/big-code-analysis/issues/103)
  (umbrella) via phase 9/9 in
  [#273](https://github.com/dekobon/big-code-analysis/issues/273).
  `big_code_analysis` is now installable from PyPI via
  `pip install big-code-analysis`, exposes the same metric pipeline
  as the `bca` CLI, and ships abi3 manylinux wheels for Linux
  x86_64 and aarch64 on CPython 3.12+. The public surface is:
  `analyze` / `analyze_source` / `analyze_batch` (never-raise
  per-file errors via `AnalysisError`), `flatten_spaces` (scalar
  rows for DataFrames / sqlite), `to_sarif` (SARIF 2.1.0 ready for
  GitHub Code Scanning), `language_for_file`,
  `supported_languages`, `language_extensions`, and the `metrics=`
  kwarg threading through every entry point with `METRIC_NAMES` as
  the validated list. Output dicts match `bca metrics
  --output-format json` byte-for-byte (verified by the
  `cli_parity.py` example and the parametrized parity tests in
  `tests/test_smoke.py`). The `examples/` directory ships
  `cli_parity.py` (CLI parity smoke test, wired into
  `make py-test`), `pipeline_db.py` (directory walk →
  `analyze_batch` → `flatten_spaces` → sqlite top-N, with a
  deliberately broken file to demonstrate the never-raise
  contract), `sarif_upload.py` (SARIF emission ready for
  `github/codeql-action/upload-sarif@v3`), and
  `jupyter_quickstart.ipynb` (pandas DataFrame + matplotlib
  cyclomatic-per-function plot, executed end-to-end in CI via a
  new `python-examples-nbconvert` job). Type-checked under
  `mypy --strict` and `pyright`; PEP 561 `py.typed` ships in the
  built wheel. The granular per-phase implementation history lives
  in the sub-issues
  ([#265](https://github.com/dekobon/big-code-analysis/issues/265)–[#272](https://github.com/dekobon/big-code-analysis/issues/272))
  and on the commits referenced from them; this umbrella entry is
  the single end-user-facing announcement.

- Public `Ast` type for parse-once, compute-many-times analysis. Build
  one with `Ast::parse(Source)` (re-parses bytes, mirrors `analyze`)
  or `Ast::from_tree_sitter(lang, tree, code, name)` (adopts a
  caller-built `tree_sitter::Tree`, the `Source`-flavored counterpart
  of `metrics_from_tree` with no lossy path-to-name conversion). Then
  call `Ast::metrics(options)` repeatedly against the same parse —
  with different `MetricsOptions::with_only` selections, interleaved
  with a custom tree-sitter walk via `Ast::as_tree_sitter`, or cached
  across configuration changes in an analysis pipeline. `analyze` and
  `metrics_from_tree` are now thin wrappers around the same seam, so
  the per-language dispatch table lives in exactly one place. See
  [`library/parse-once.md`](big-code-analysis-book/src/library/parse-once.md)
  and [`library/ast-traversal.md`](big-code-analysis-book/src/library/ast-traversal.md)
  for working with the held `tree_sitter::Tree` directly
  ([#264](https://github.com/dekobon/big-code-analysis/issues/264)).
- `bca check --baseline <path>` and `--write-baseline <path>` flags
  for ratcheting thresholds on an existing codebase without raising
  limits. The baseline is a sorted TOML file keyed on `(path,
  function, start_line, metric)` that records today's offender set;
  a baselined function whose value has not worsened is filtered
  from threshold checks, but regressions (`current > baseline.value`)
  and new offenders still fail. Composes with in-source suppression
  markers — `--write-baseline` excludes already-suppressed functions
  by default, and `--no-suppress --write-baseline` records every
  violation for CI-auditor flows. See
  [`commands/check.md`](big-code-analysis-book/src/commands/check.md)
  and the [Baselines recipe](big-code-analysis-book/src/recipes/baselines.md)
  for the full adoption flow
  ([#99](https://github.com/dekobon/big-code-analysis/issues/99)).
- Per-language Cargo features (default: `all-languages`) so library
  consumers can compile only the grammars they need. Each supported
  language now has its own feature (`rust`, `typescript`, `python`,
  `cpp`, …) that gates the matching `tree-sitter-*` grammar crate
  in the dependency graph. The default feature set keeps the
  library's historical "every grammar compiled in" behaviour
  (`bca` and `bca-web` both pin `features = ["all-languages"]`
  explicitly); downstream library consumers can opt into a narrower
  set with `default-features = false, features = ["rust", "typescript", …]`.
  The `LANG` enum keeps every variant defined regardless of the
  active feature set; selecting a variant whose feature is off
  produces `Err(MetricsError::LanguageDisabled(LANG))` from every
  dispatch entry point. A new `LANG::is_enabled` predicate lets
  callers query the compiled-in set without going through a
  dispatcher
  ([#252](https://github.com/dekobon/big-code-analysis/issues/252)).
- New `big_code_analysis::prelude` module exposing the recommended
  entry points for the 90% case: `analyze`, `metrics_from_tree`,
  `Source`, `MetricsOptions`, `MetricsError`, `Metric`, `LANG`,
  `FuncSpace`, `CodeMetrics`, `SpaceKind`. Callers can now write
  `use big_code_analysis::prelude::*;` instead of long
  per-import lists; everything outside the prelude is still
  reachable by its fully-qualified name from the crate root
  ([#255](https://github.com/dekobon/big-code-analysis/issues/255)).
- `MetricsOptions::with_only(&[Metric])` for selective metric
  computation. Pass a slice of [`Metric`] values to restrict the
  walker to those metrics; everything outside the set is skipped at
  the per-node level (no `T::Halstead::compute`, no
  `T::Cognitive::compute`, etc.) and elided from `CodeMetrics`
  serialization output. Derived metrics auto-resolve their
  dependencies — `with_only(&[Metric::Mi])` silently adds
  `Loc + Cyclomatic + Halstead`, and `with_only(&[Metric::Wmc])`
  adds `Cyclomatic + Nom`. The `Metric` enum is `#[non_exhaustive]`
  and the backing bitfield (`MetricSet`) is exposed alongside it so
  callers can introspect which metrics were computed via the new
  `CodeMetrics::selected()` accessor. Defaults are unchanged:
  `MetricsOptions::default()` selects every metric, matching the
  pre-#257 behaviour byte-for-byte
  ([#257](https://github.com/dekobon/big-code-analysis/issues/257)).
- New library entry point `analyze(Source, MetricsOptions) ->
  Result<FuncSpace, MetricsError>` in `src/spaces.rs`. `Source<'a>`
  is `#[non_exhaustive]` and carries the language, source bytes,
  optional caller-supplied display name (`Source::name`), optional
  C++-preprocessor path (`Source::preproc_path`), and optional
  `PreprocResults`. Construct via `Source::new(lang, code)` plus
  the `with_name` / `with_preproc_path` / `with_preproc` setters.
  This is the recommended entry point for in-memory analysis —
  callers no longer need to fabricate a `&Path` to identify a
  buffer
  ([#254](https://github.com/dekobon/big-code-analysis/issues/254)).
- Parse seam for callers who already drive `tree-sitter`. New
  `Parser::from_tree(tree, code)` accepts a pre-built
  `tree_sitter::Tree` plus the matching source bytes, skipping the
  bundled parse. A non-generic `metrics_from_tree(lang, tree,
  source, path, pr, options) -> Result<FuncSpace, MetricsError>`
  dispatches on `&LANG` for the common case. The `tree_sitter`
  crate is re-exported as `big_code_analysis::tree_sitter` so
  consumers can build trees against the exact version the metric
  walker was compiled against without taking a sibling
  dependency; `LANG::get_tree_sitter_language` returns the
  matching grammar. Both seam entry points accept `tree_sitter::Tree`
  directly, so the internal `Tree` wrapper stays crate-private.
  The re-exported `tree_sitter` API and the
  `LANG::get_tree_sitter_language` return type follow the
  underlying grammar pin and are documented as value-not-stable
  in `STABILITY.md`. The `library/reuse-tree` book chapter is
  upgraded from a stub to a working example
  ([#251](https://github.com/dekobon/big-code-analysis/issues/251)).
- Top-level `STABILITY.md` documenting the versioning contract for
  the `0.x` line: which types and entry points are shape-stable,
  why no value stability is offered until `1.0`, the escape hatches
  (`Node.0`, the still-direct `tree-sitter` dependency,
  `#[doc(hidden)]` items), and the MSRV policy
  (`rust-version = "1.94"` workspace-wide). Linked from the README
  under a new "Using as a library" section
  ([#258](https://github.com/dekobon/big-code-analysis/issues/258)).
- In-source suppression markers for metric threshold checks. Comments
  matching `bca: suppress`, `bca: suppress(metric, ...)`,
  `bca: suppress-file`, `bca: suppress-file(metric, ...)`,
  `#lizard forgives`, or `#lizard forgive global` silence offending
  `bca check` violations without editing source. A new `--no-suppress`
  flag forces all markers to be ignored for CI auditors. `FuncSpace`
  gains a `suppressed: SuppressionScope` field (elided from JSON when
  empty so existing snapshots are unchanged). New public types:
  `MetricKind`, `SuppressionScope`, and `SuppressionPolicy`. Documented
  in the new *Suppression markers* book chapter
  ([#98](https://github.com/dekobon/big-code-analysis/issues/98),
  [#263](https://github.com/dekobon/big-code-analysis/issues/263)).
- **(breaking)** `AstNode` JSON output now carries a `FieldName`
  key holding the tree-sitter grammar field through which each
  node was reached (`left`, `right`, `name`, `parameters`,
  `body`, ...). Consumers can distinguish structurally
  equivalent children without grammar-specific positional
  knowledge. The `Alterator` trait's `get_ast_node` /
  `get_default` / `alterate` methods gain a `field_name:
  Option<&'static str>` parameter; downstream `impl Alterator
  for X` consumers must update signatures. `AstNode::new` keeps
  its existing signature (defaulting `field_name` to `None`)
  and a new `AstNode::with_field_name` constructor accepts the
  field. `AstNode` also gains a public `field_name` field,
  which breaks callers using positional struct construction
  ([#244](https://github.com/dekobon/big-code-analysis/issues/244)).
- Support for Groovy source files (`.groovy`, `.gradle`, `.gvy`,
  `.gy`, `.gsh`), wired up to `tree-sitter-groovy = "=0.1.2"`
  (amaanq). All twelve metric traits get real impls mirroring Java's
  with Groovy-specific extensions for closures, `juxt_function_call`,
  GStrings, the `def` keyword, and the explicit `**` / `..` / `in`
  operator tokens. Several Groovy-specific operators (Elvis `?:`,
  safe-navigation `?.`, spread `*.` / `*:`, spaceship `<=>`, regex
  `=~` / `==~`, identity `===` / `!==`, exclusive ranges `..<`,
  trait declarations) are not yet supported by the upstream grammar
  and are tracked as follow-up issues.
- Python `Abc` impl now counts unary `not` as one condition,
  closing the parity gap with Java / C# / Kotlin. `if not flag:`
  reports `conditions = 1` (was `0`); `not (x > 0)` reports `2`
  (`NotOperator` + `ComparisonOperator`), matching Java's
  `!(x > 0)`
  ([#214](https://github.com/dekobon/big-code-analysis/issues/214)).
- New `tcl_no_string_lloc` test pins that a multi-line Tcl
  double-quoted string literal contributes only one lloc (the
  surrounding `set` command), not one lloc per body line — mirrors
  the existing Lua / Elixir / PHP heredoc-shape coverage
  ([#210](https://github.com/dekobon/big-code-analysis/issues/210)).
- New `cpp_lambda_in_function_lloc` test covers C++11 lambda LLOC
  counting, the one language missing from #195's wave-9 nested-
  function / closure / lambda coverage
  ([#213](https://github.com/dekobon/big-code-analysis/issues/213)).
- Append `("elixir", "elixir")` to `LANGUAGE_PALETTE` in the HTML report
  with matching light- and dark-mode `section.lang-elixir{…}` CSS
  rules. Elixir-only reports now render in a distinct purple instead of
  the neutral "other" grey
  ([#187](https://github.com/dekobon/big-code-analysis/issues/187)).
- Real Ruby implementations of `Abc`, `Npa`, `Npm`, and `Wmc` metrics
  (previously default no-ops). Removes Ruby from the four
  `implement_metric_trait!` default registrations in `src/macros.rs`;
  registers concrete impls mirroring the PHP / Java / C# patterns
  including visibility-flag tracking (`private` / `public` /
  `protected`), `attr_*` macro detection, singleton methods, and
  inheritance. 52 new per-metric Ruby tests reach parity with the
  Java/C#/PHP sibling counts (Abc=14, Npa=13, Npm=13, Wmc=12)
  ([#190](https://github.com/dekobon/big-code-analysis/issues/190)).
- `Npa::compute` and `Npm::compute` now take the source bytes as a
  second parameter — `fn compute<'a>(node: &Node<'a>, code: &'a [u8],
  stats: &mut Stats)` — mirroring `Cyclomatic::compute` and
  `Exit::compute`. Languages whose visibility markers are bare
  `Identifier` text (Ruby `private` / `public` / `protected`) can now
  read the source bytes to classify them. Existing per-language impls
  that do not need the bytes discard them with `_`. The `Checker`
  supertrait is `pub(crate)`, so downstream crates cannot observe this
  change ([#190](https://github.com/dekobon/big-code-analysis/issues/190)).
- Audit and document the `implement_metric_trait!` default-impl matrix
  in `src/macros.rs`. 44 (language, metric) cells classified — 21
  genuine defaults (the language has no construct the metric measures)
  and 23 placeholders (real impls owed). 29 placeholder smoke tests
  added so a future implementation that lands without updating tests
  trips the gate. Follow-up issues filed per language (#201 Python,
  #202 JS/Mozjs, #203 Rust, #204 C++, #205 Go, #206 Elixir, #208
  Perl/Lua/Tcl)
  ([#188](https://github.com/dekobon/big-code-analysis/issues/188)).
- `Abc`, `Npa`, `Npm`, `Wmc`, `Mi` metric implementations for
  **Python**, **Rust**, **C++**, **JavaScript**, and **Mozjs**
  (previously default no-op, scored as 0/0/0). Python and JavaScript
  helpers operate via the `Npa::compute` / `Npm::compute` trait
  signatures, so prototype assignments and Python name-mangling
  visibility are documented limitations. Rust maps `Impl→Class` and
  `Trait→Interface` for Wmc aggregation; C++ tracks per-class
  visibility (public/private/protected, with class-default-private
  and struct-default-public semantics). 200+ new anchored tests
  ([#201](https://github.com/dekobon/big-code-analysis/issues/201),
  [#202](https://github.com/dekobon/big-code-analysis/issues/202),
  [#203](https://github.com/dekobon/big-code-analysis/issues/203),
  [#204](https://github.com/dekobon/big-code-analysis/issues/204)).
- `Abc`, `Npa`, `Npm` metric implementations for **Go**, plus `Mi`
  via the default cascade. `Wmc` deliberately left at zero with a
  documented regression test — Go's flat space model exposes
  `SpaceKind::Function` for both `MethodDeclaration` and free
  `FunctionDeclaration`, so per-receiver grouping isn't possible
  without space-model surgery
  ([#205](https://github.com/dekobon/big-code-analysis/issues/205)).
- `Cognitive`, `Abc` metric implementations for **Elixir** (the
  highest-impact gap from the audit — Elixir is heavily branchy and
  previously scored 0 on cognitive complexity), plus `Mi` via the
  default cascade. Recursion and `Enum.reduce` are intentionally
  omitted with documented zero-pin tests
  ([#206](https://github.com/dekobon/big-code-analysis/issues/206)).
- `Abc` metric implementations for **Perl**, **Lua**, and **Tcl**
  ([#208](https://github.com/dekobon/big-code-analysis/issues/208)).
- 18 lesson-9 synthetic-Unit regression tests in `src/spaces.rs`
  covering every supported language (Python, JS, TS, TSX, Mozjs,
  Java, Kotlin, Go, Rust, C#, Bash, Lua, Tcl, Perl, PHP, Elixir,
  Preproc, Ccomment, Ruby); only Lua exercises the synthetic-Unit
  promotion path today, the rest pin the current
  translation-unit-root contract as future-proofing
  ([#193](https://github.com/dekobon/big-code-analysis/issues/193)).
- 20 nested-function/closure/lambda LLOC tests across Python, Java,
  C#, JavaScript, Kotlin, Go, PHP, Lua, Tcl, Perl, Elixir
  ([#195](https://github.com/dekobon/big-code-analysis/issues/195)).
- Three new lesson-11 cross-language parity tests
  (`cognitive_cross_language_parity`, `exit_cross_language_parity`,
  `nargs_cross_language_parity`) covering 2-arm wildcard switches,
  loops with early exit, and 3-parameter functions; the original
  `cyclomatic_if_elseif_else_chain_cross_language` was the only
  one previously implemented
  ([#196](https://github.com/dekobon/big-code-analysis/issues/196)).
- PHP heredoc (`<<<EOT … EOT;`) and nowdoc (`<<<'EOT' … EOT;`) LOC
  regression tests
  ([#194](https://github.com/dekobon/big-code-analysis/issues/194)).
- `--exclude-tests` CLI flag (and `MetricsCfg::options.exclude_tests`
  library option) elides Rust `#[test]` / `#[cfg(test)]` /
  `#[cfg(all(test, ...))]` / `#[cfg(any(test, ...))]` and common
  test-framework attributes (`#[tokio::test]`, `#[async_std::test]`,
  `#[rstest]`, `#[test_log::test]`, `#[wasm_bindgen_test]`,
  `#[test_case]`) from metric computation, plus `mod` items carrying
  outer `#[cfg(test)]` or inner `#![cfg(test)]` attributes. The skip
  is implemented as a new `Checker::should_skip_subtree(node, code)`
  trait method (default `false`, backward-compatible — only
  `RustCode` overrides; non-Rust languages are unaffected) and runs
  upstream of every per-metric `compute` call so Halstead, Cyclomatic,
  Cognitive, LOC, NOM, WMC, ABC, NPA, NPM, Exit, NArgs, and the
  derived Maintainability Index all benefit from the same gate.
  Default is OFF (tests still counted) to preserve existing numeric
  output for downstream library consumers; the issue author's
  recommendation to flip the default is left for a follow-up.
  `MetricsOptions` and `MetricsCfg` are both `#[non_exhaustive]` so
  future option fields don't struct-literal-break downstream
  callers; construct via `MetricsOptions::default()
  .with_exclude_tests(true)` and `MetricsCfg::new(path)
  .with_options(...)` (issue [#185](https://github.com/dekobon/big-code-analysis/issues/185))
  ([#182](https://github.com/dekobon/big-code-analysis/issues/182)).
- Support for Ruby source files (`.rb`, `.rake`, `.gemspec`) via
  [`tree-sitter-ruby`](https://crates.io/crates/tree-sitter-ruby)
  `=0.23.1`. Real implementations for `Cyclomatic` (if/unless/while/
  until/for/case-when/rescue/conditional/`&&`/`||`/`and`/`or`),
  `Exit` (counting `return` statements only — `yield` does not exit
  the method), `Halstead` (full keyword-token, punctuation, and
  operator/operand classification with interpolation-aware string
  literals), `Loc` (named statement nodes, postfix modifiers, and
  class/module/method declarations), and `Cognitive`
  (`is_else_if` keyed on the dedicated `elsif` clause so chains stay
  below nested-`if` cost). `Abc`, `Mi`, `NArgs`, `Nom`, `Npa`, `Npm`,
  and `Wmc` use default impls; `Tokens` is wired through.
- Support for Elixir source files (`.ex`, `.exs`) via
  [`tree-sitter-elixir`](https://crates.io/crates/tree-sitter-elixir)
  `=0.3.5`. Real implementations for `Halstead`, `Loc`, `Cyclomatic`,
  and `Exit`; remaining metric traits use default impls. Branching
  constructs (`if`/`unless`/`for`/`while`/`with`/`case`/`cond`/`try`)
  surface as `Call` nodes with text-keyed targets and are identified
  via source-byte inspection (#179); short-circuit booleans (`&&`,
  `||`, `and`, `or`) and per-arm `stab_clause`s round out cyclomatic
  detection.
- Full binary-release pipeline (`.github/workflows/release.yml`) plus
  packaging skeletons under `packaging/`. Tagging `vX.Y.Z` on `main`
  runs preflight (tag/CHANGELOG/version-parity gates), builds release
  binaries for 8 platforms (x86_64/aarch64 across linux-gnu,
  linux-musl, freebsd, darwin, windows-msvc), assembles archives
  containing both `bca` and `bca-web` alongside `README.md`,
  `LICENSE`, `CHANGELOG.md`, and per-binary
  `THIRD-PARTY-LICENSES-bca.md` / `THIRD-PARTY-LICENSES-bca-web.md`
  (the two binaries have non-overlapping direct deps — clap/ignore
  vs actix-web/tokio/futures — so a single shared notices file would
  under-attribute one side), builds
  two `.deb`/`.rpm`/`.apk`/FreeBSD-pkg artefacts per arch (one each
  for the CLI and web crates), smoke-installs every package across
  Ubuntu 22.04/24.04, Debian 12, Rocky 9, Fedora, Amazon 2023,
  Alpine 3.20, FreeBSD 14, macOS, and Windows, then signs +
  attests + uploads them. CycloneDX SBOMs and SHA256SUMS are
  minisign-signed and SLSA-build-provenance-attested. A
  `publish-crates` job (Trusted Publishing via OIDC, order
  `big-code-analysis` → `-cli` → `-web`) and the Homebrew tap /
  Scoop bucket pushes are gated by repo vars
  (`ENABLE_CRATES_PUBLISH`, `ENABLE_HOMEBREW_TAP`,
  `ENABLE_SCOOP_BUCKET`) so the binary pipeline can ship today
  while the vendored-grammar publish strategy is still deferred
  (see [#149](https://github.com/dekobon/big-code-analysis/issues/149)).
  `Makefile` gains `release-check`, `verify-changelog`,
  `pkg-deb-local`, `pkg-rpm-local` targets to surface preflight
  drift before tagging
  ([#155](https://github.com/dekobon/big-code-analysis/issues/155)).
- `#[must_use]` on 157 public accessor methods flagged by
  `clippy::must_use_candidate` — the per-metric getter families
  under `src/metrics/` (loc, abc, halstead, npa, npm, nom, nargs,
  cyclomatic, wmc, exit, cognitive, tokens, mi) plus the
  `Alterator`, `ParserTrait`, `OffenderRecord`, `Severity`, `Node`,
  `Ast`, and preproc / tools public entry points. Callers that
  ignored the return value will now see a compiler warning
  ([#158](https://github.com/dekobon/big-code-analysis/issues/158)).
- Minimal `.markdownlint-cli2.jsonc` enabling `MD024 siblings_only`
  so Keep-a-Changelog repeated `### Added` / `### Changed` headers
  across version sections don't trip the no-duplicate-heading rule.
  Extended in this release with `MD013` (line_length 120,
  tables/code_blocks false) and an `ignores` list covering `target/**`,
  `node_modules/**`, `.claude/**`, `tests/repositories/**`, and
  `big-code-analysis-book/book/**`
  ([#151](https://github.com/dekobon/big-code-analysis/issues/151)).
- Contributor-facing and release-process documentation: `CONTRIBUTING.md`,
  `SECURITY.md`, `RELEASING.md`, and `.github/ISSUE_TEMPLATE/` (bug
  report and feature request)
  ([#156](https://github.com/dekobon/big-code-analysis/issues/156)).
- Supply-chain hygiene configuration at the repo root: `deny.toml`
  (cargo-deny: yanked-as-deny, license allow-list including MPL-2.0,
  wildcards-as-deny, unknown-registry/git-as-deny), `about.toml` and
  `about.hbs` (cargo-about template covering the 8 release targets),
  and a `minisign.pub` placeholder the release preflight grep-matches
  to fail fast on un-rotated keys
  ([#151](https://github.com/dekobon/big-code-analysis/issues/151)).
- Per-PR GitHub Actions pipeline (`.github/workflows/ci.yml`): `fmt`,
  `clippy`, `docs`, `test` (3-OS matrix), `msrv` (1.94 build-only),
  `feature-matrix`, `deny`, `license-audit`, `lint`, and an
  `if: always()` aggregator `ci` job intended as the single required
  status check for branch protection. All third-party actions are
  pinned to commit SHAs. The standalone `snapshot-anchors.yml`
  workflow is removed; `check-snapshot-anchors.py` now runs inside
  the new `lint` job
  ([#152](https://github.com/dekobon/big-code-analysis/issues/152)).
- Explicit `cargo check` gate (under `RUSTFLAGS="-D warnings"`) for the
  workspace-excluded `enums` codegen crate, wired into the `make
  pre-commit` / `make ci` parallel DAG, the `make lint` aggregate, the
  `.github/workflows/ci.yml` `lint` job, and the `.pre-commit-config.yaml`
  hook set. The crate stays out of the workspace (so per-PR clippy
  isn't run on codegen-only code) but its lint surface no longer
  drifts silently — the gate would have caught the `unused_imports`
  warning that motivated #162
  ([#164](https://github.com/dekobon/big-code-analysis/issues/164)).
- CodeQL scanning workflow (`.github/workflows/codeql.yml`) covering
  Rust, Python, and GitHub Actions on push to `main`, PRs to `main`,
  and a weekly Monday 06:23 UTC cron. All `uses:` are pinned to commit
  SHAs and job permissions follow least-privilege
  ([#153](https://github.com/dekobon/big-code-analysis/issues/153)).
- Top-level `LICENSE` file containing the verbatim MPL-2.0 text, so
  the references in `about.hbs` (cargo-about output) and
  `CONTRIBUTING.md` resolve and downstream consumers can find the
  license at the conventional path. `Cargo.toml`'s
  `license = "MPL-2.0"` SPDX declaration is unchanged
  ([#163](https://github.com/dekobon/big-code-analysis/issues/163)).
- Real `Abc`, `Npa`, `Npm`, `Wmc` implementations for Kotlin
  (`KotlinCode`). The four metrics now report non-zero values for
  Kotlin classes / interfaces / `object` singletons / `data class`
  / nested+inner classes / companion-object members. Java is the
  parity reference; deliberate divergences are documented in-code
  (data-class compiler-synthesized members excluded; companion-object
  members folded into the enclosing class; extension functions and
  top-level `val`/`var`/`fun` excluded from class metrics;
  primary-constructor parameter properties count as class
  attributes; `init` blocks not methods). Adds 73 new Kotlin tests
  with anchored snapshots
  ([#168](https://github.com/dekobon/big-code-analysis/issues/168)).
- Real `Abc`, `Npa`, `Npm`, `Wmc` implementations for TypeScript
  (`TypescriptCode`) and TSX (`TsxCode`), sharing one compute body
  per metric via `ts_<metric>_compute!` macros. Both languages now
  score class / interface / abstract-class / generic-class /
  parameter-property / accessor / arrow-field / overload shapes.
  Documented decisions: default-public visibility; constructor
  parameter properties count as attributes; interface `property_
  signature` → npa and `method_signature` / `abstract_method_
  signature` / `construct_signature` → npm (Java parity); method-
  overload signatures are skipped (only the implementation counts);
  arrow-function class fields count as methods, not attributes;
  getters/setters each count once. Adds 99 new TS/TSX tests
  ([#169](https://github.com/dekobon/big-code-analysis/issues/169)).
- Generated Unix manpages (`man/bca.1`, `man/bca-web.1`, and one
  `man/bca-<sub>.1` per `bca` subcommand: check, count, dump, find,
  functions, list-metrics, metrics, ops, preproc, report,
  strip-comments). Produced from the live clap derive schemas by a
  new `xtask` workspace crate that depends on `clap_mangen`. The
  pages are committed to `man/` so CI can drift-check them; the new
  `manpage` job in `.github/workflows/ci.yml` runs `cargo xtask`
  and `git diff --exit-code -- man/` on every PR. Release workflow
  stages the pages into per-OS tarballs and the DEB / RPM / Alpine
  apk / FreeBSD pkg / Homebrew formula assets so `man bca` works
  after install on every shipping channel. `Cli` and `Opts` were
  lifted from each binary's `main.rs` into the corresponding crate
  `lib.rs` so `clap_mangen` can link against them. `cargo install`
  from crates.io does not currently ship manpages (the workspace-
  root `man/` directory is outside individual crate tarballs) —
  noted as a follow-up
  ([#171](https://github.com/dekobon/big-code-analysis/issues/171)).
- 13 new C/C++ cognitive complexity tests (`c_*` in
  `src/metrics/cognitive.rs`) covering ternary, try/catch, range-
  based and nested loops, recursion, multi-label `goto`, C++11
  lambdas, switch fall-through and nesting, and macro-expanded
  control flow. The exercise locked in three documented gaps in
  the C/C++ cognitive impl — `ConditionalExpression` (now tracked
  by #172), `ForRangeLoop` (now tracked by #173), and recursion
  (a static-analysis limitation documented at the top of the
  file). FIXMEs in the new tests point at the fix issues
  ([#167](https://github.com/dekobon/big-code-analysis/issues/167)).
- ~28 new C/C++ tests across `cyclomatic`, `exit`, `halstead`,
  `nargs`, `nom`, `tokens` bringing each metric near its peer-
  language high-mark. Pinned the C-family behaviour for `goto`
  (not in cyclomatic), `throw` (not in C++ `exit`), implicit
  `this` (not counted by `nargs`), template parameter packs
  (collapse to one runtime arg), lambdas-inside-functions (closures,
  not methods), and the `&` vs `&&` Halstead separation
  ([#170](https://github.com/dekobon/big-code-analysis/issues/170)).

### Changed

- **README badges replaced.** Dropped the four upstream
  `mozilla/rust-code-analysis` badges that no longer worked for this
  fork: the Mozilla TaskCluster job (URL pinned at `/master/`; the
  fork's default branch is `main` and no TaskCluster job is wired up),
  the codecov badge (rendered `coverage: unknown` since no CI job
  uploads coverage), and the Mozilla Matrix chat badge (link
  resolved to the upstream `#rust-code-analysis:mozilla.org` room,
  not a room this fork owns). Replaced them with the badge set the
  fork can actually back: crates.io version, MSRV (sourced
  dynamically from `Cargo.toml`'s `rust-version`), CI workflow
  status, CodeQL status, docs.rs, and license. No coverage badge is
  re-added — CI does not currently upload coverage; revisit only if
  a `cargo-llvm-cov` job lands
  ([#148](https://github.com/dekobon/big-code-analysis/issues/148)).
- Removed four dead per-route `.app_data(web::Json::<T>)` calls from
  `big-code-analysis-web`'s `HttpServer` builder. Each was a bare
  tuple-struct constructor function-item, not a `JsonConfig`, so the
  `Json<T>` extractor never looked them up. The global
  `JsonConfig::default().limit(max_size)` registered one line earlier
  is what actually bounds JSON payload size; the per-route lines were
  misleading no-ops from PR #883's actix-web 4.1 bump. Added a 413
  Payload-Too-Large regression test against a small global limit
  ([#336](https://github.com/dekobon/big-code-analysis/issues/336)).
- `xtask`'s `render_man_page` now returns
  `io::Error::new(io::ErrorKind::AlreadyExists, ...)` when two
  rendered pages would write to the same `{name}.1` filename. The
  previous blind `fs::write` plus `expected.push(filename)` would
  silently overwrite the first page and mask the collision from the
  orphan sweep, so a hypothetical future `bca web` subcommand
  colliding with the `bca-web` top-level binary would ship only one
  of the two pages. The collision check is ASCII-case-insensitive
  so case-insensitive filesystems (APFS on macOS, NTFS on Windows)
  treat `Bca.1` and `bca.1` as colliding even though Rust string
  equality does not
  ([#337](https://github.com/dekobon/big-code-analysis/issues/337)).
- `enums/templates/foo.rs` has been deleted. The file was an
  unreferenced early scaffold (no `#[template(path = ...)]` pointed
  at it) containing syntactically invalid Rust — semicolons instead
  of commas as enum-variant separators, plus missing `Eq`/`Clone`/
  `FromPrimitive` derives that real production templates require —
  and would have misled anyone using it as a copy-paste starting
  point
  ([#342](https://github.com/dekobon/big-code-analysis/issues/342)).
- Four `unwrap()` sites in non-test code have been replaced with
  either `expect("invariant")` carrying the proof in the panic text
  or restructured `match`/`while let` patterns that make the
  impossibility lexical: `dump_spans` in `src/function.rs` now uses
  `into_iter().enumerate()` with an `i == last_idx` flag; `consumer`
  in `src/concurrent_files.rs` uses `while let Ok(Some(job)) =
  receiver.recv()`; `get_regex` and `get_paths_dist` in
  `src/tools.rs` carry `.expect("constant regex must compile")` and
  `.expect("ancestor verified by starts_with above")` respectively.
  Regression tests added for the `dump_spans` n=0/1/many paths and
  the `consumer` poison-pill loop
  ([#343](https://github.com/dekobon/big-code-analysis/issues/343)).
- `enums::mk_langs!` and `enums::mk_get_language!` no longer accept
  the unused grammar-crate ident as a tuple second element. The
  macro expansion never referenced `$name`, so the
  `(Cpp, tree_sitter_cpp)` declaration was decorative — and
  misleading, since `get_language(&Lang::Cpp)` actually resolves to
  `tree_sitter_mozcpp::LANGUAGE`. Variants that resolve against a
  non-obvious grammar (`Cpp` → mozcpp, `Mozjs` → mozjs, the vendored
  `bca-tree-sitter-*` forks, the `LANGUAGE_TYPESCRIPT` / `LANGUAGE_PHP`
  per-language consts) now carry an inline `// -> <crate>` comment
  pinned to each entry
  ([#344](https://github.com/dekobon/big-code-analysis/issues/344)).
- **Vendored grammar forks renamed to `bca-tree-sitter-*`** to unblock
  first-time crates.io publication of `big-code-analysis`. The five
  in-tree path-dep crates — `tree-sitter-ccomment`, `tree-sitter-mozcpp`,
  `tree-sitter-mozjs`, `tree-sitter-preproc`, `tree-sitter-tcl` —
  now publish as `bca-tree-sitter-ccomment`, `bca-tree-sitter-mozcpp`,
  `bca-tree-sitter-mozjs`, `bca-tree-sitter-preproc`, and
  `bca-tree-sitter-tcl` (all at `0.1.0`). The Rust crate name produced
  by each leaf manifest is preserved as `tree_sitter_<lang>` via
  `[lib] name = ...`, and the workspace alias in `Cargo.toml` /
  `enums/Cargo.toml` uses Cargo's `package = ...` aliasing so every
  call site keeps importing `tree_sitter_ccomment::LANGUAGE` etc.
  unchanged. Resolves the "vendored-grammar publish strategy" item
  in [#149](https://github.com/dekobon/big-code-analysis/issues/149)
  via the "rename forks" option (rationale and rejected
  alternatives are recorded on the issue thread). MIT `LICENSE`
  files now ship in every leaf tarball — added a `LICENSE` to
  `tree-sitter-tcl` (copied from
  `tree-sitter-grammars/tree-sitter-tcl` at `v1.1.0`) and appended a
  `Modifications copyright (c) 2025-2026 Elijah Zupancic` line to
  the four Mozilla-derived `LICENSE` files.

- `release.yml` `publish-crates` job now publishes the five
  `bca-tree-sitter-*` leaf crates ahead of `big-code-analysis`, with
  per-leaf sparse-index existence checks so re-runs of the same tag
  stay idempotent.

- `big-code-analysis-py/pyproject.toml`: `[tool.maturin].features`
  now includes `pyo3/abi3-py312` alongside `pyo3/extension-module`.
  Both `maturin develop` and the CI wheel build go through the
  limited (stable) Python ABI, so the local-dev binary matches the
  released wheel byte-for-byte at the C-API surface. A future
  contributor who introduces a non-abi3 PyO3 dependency now trips a
  local build failure long before the change reaches CI.

- Python bindings (`big-code-analysis-py`) are now lint-, format-,
  and type-checked under `ruff`, `mypy --strict`, and `pyright` in
  `make pre-commit` / `make ci`. Phase 6/9 of
  [#103](https://github.com/dekobon/big-code-analysis/issues/103);
  fixes [#270](https://github.com/dekobon/big-code-analysis/issues/270).
  Linux / macOS / Windows × Python 3.12 / 3.13 CI matrix gated by a
  `dorny/paths-filter` job so Rust-only PRs are unaffected. Pre-commit
  hooks pin `ruff-check` + `ruff-format` externally and run `mypy
  --strict` via a `local` `language: system` hook (pre-commit's
  isolated venv cannot see the `maturin develop`'d extension).
- **(breaking, bindings-py)** `bca.language_for_file(path)` now reads
  the file and resolves through `big_code_analysis::guess_language`
  — the same detection pipeline `bca.analyze` uses — so an
  extension-less script with a `#!/usr/bin/env python` shebang
  resolves to `"python"` instead of `None`. Closes the API
  asymmetry with `analyze` introduced in
  [#314](https://github.com/dekobon/big-code-analysis/issues/314).
  The prior "Never raises" contract is dropped: I/O failures
  surface as `OSError` (dispatching to `FileNotFoundError`,
  `PermissionError`, …) with `errno` / `filename` populated — same
  typed-exception taxonomy `analyze` uses. Callers needing the
  prior extension-only, never-raising semantics for a cheap path
  inspection can wrap the call in `try / except OSError`; the
  underlying `LANG::get_extensions` table is unchanged
  ([#318](https://github.com/dekobon/big-code-analysis/issues/318)).
- Consolidate the four JS-family `Getter::get_op_type` impls
  (JavaScript, MozJS, TypeScript, TSX) behind a single
  `impl_js_family_get_op_type!` macro that takes per-language operator
  and operand `extras` lists. Mirrors the existing
  `impl_cyclomatic_js_family!` / `impl_js_family_is_string!` patterns.
  Pure refactor: Halstead operator/operand classification is
  byte-identical. Adds a four-way parity regression test for
  optional-chain member access. Reviewer cross-walk of the
  consolidated table surfaced a pre-existing TypeScript
  `Checker::is_string` / `Getter::get_op_type` disagreement on
  `String2` (the `string` type-keyword alias), tracked under
  [#313](https://github.com/dekobon/big-code-analysis/issues/313)
  for follow-up ([#299](https://github.com/dekobon/big-code-analysis/issues/299)).
- Consolidate `impl Cyclomatic for JavaCode` and `impl Cyclomatic for
  GroovyCode` behind a new `impl_cyclomatic_java_like!` macro that
  takes a list of extra decision kinds (`[]` for Java, `[Assert]` for
  Groovy). Mirrors the existing `impl_npm_java_like!` /
  `impl_npa_java_like!` patterns. Adds a Java/Groovy parity regression
  test plus a Groovy-only `Assert`-arm assertion, both with
  `cyclomatic_max` and `cyclomatic_modified_max` coverage so
  one-counter regressions can't slip past
  ([#300](https://github.com/dekobon/big-code-analysis/issues/300)).
- Introduce `impl_simple_is_string!($lang, $variants...)` and apply it
  to 17 single-or-flat-variant `Checker::is_string` impls (Preproc,
  Ccomment, Cpp, Python, Java, Csharp, Rust, Go, Kotlin, Perl, Lua,
  Bash, Tcl, Php, Elixir, Ruby, Groovy). The JS family keeps its
  dedicated `impl_js_family_is_string!` because of its
  `String + String2 + TemplateString + per-variant String3` shape.
  Adds per-variant positive coverage for every consolidated language
  plus negative coverage for all 17, with drift-marker assertions
  pinning the hidden grammar supertypes (`Java::MultilineStringLiteral`,
  `Groovy::StringLiteral2`, `Php::String3`) so a future grammar
  revision that promotes them surfaces in CI
  ([#301](https://github.com/dekobon/big-code-analysis/issues/301)).
- `tests/suppression_test.rs::deeply_nested_function_suppression_does_not_overflow_stack`
  rewritten in JavaScript (100 nested `function f<i>() { … }`)
  and unignored. The previous fixture used a 1000-level Python `def`
  pyramid whose ~1M whitespace bytes of indent took ~229 s to parse
  under tree-sitter-python's effectively O(N²) layout cost, so the
  test was marked `#[ignore]` and never ran on the default gate —
  meaning the iterative-suppression regression guard added by #292
  was effectively unprotected. The JavaScript fixture parses in
  ~0.8 s while preserving the deeply-nested integration path
  (parse → walk → suppression attachment), and an added
  `space.suppressed.is_empty()` assertion catches a regression
  where a function-scoped marker bubbles up to file scope
  ([#308](https://github.com/dekobon/big-code-analysis/issues/308)).
- Elixir `Wmc` / `Npm` / `Npa` now classify `def` / `defp` /
  `defmacro` / `defmacrop` calls inside `defmodule` blocks as
  methods and `defstruct` argument lists as attribute fields,
  instead of pinning each metric to zero on ordinary Elixir module
  code. The trait surface gains source-aware predicates
  (`Checker::is_func_space_with_code`, `Checker::is_func_with_code`,
  `Getter::get_space_kind_with_code`) with default-forwarding impls
  so non-Elixir languages need no override, and the walker threads
  the source bytes through to let the Elixir `Checker` disambiguate
  macro-shaped `Call` nodes by their target identifier text. `def`
  and `defmacro` are public (count in `class_nm_sum`); `defp` and
  `defmacrop` are private (counted in `class_wmc_sum` but not
  `class_nm_sum`, matching Java's npm semantics); a user-defined
  macro called `custom_def` is **not** misclassified as a method
  because the dispatch matches the literal target lexeme.
  Snapshot averages / min / max shifted across 16 Elixir snapshot
  files as the new Function / Class spaces changed the denominator
  (sums and decision-point counts are unchanged), and 10
  cyclomatic Elixir tests had their totals bumped by +2 from the
  `Stats::default()` entry seeds on the new spaces
  ([#275](https://github.com/dekobon/big-code-analysis/issues/275)).
- C# bare-discard switch-arm detection in `src/metrics/cyclomatic.rs`
  now dispatches through a private `PatternKind` enum + `classify_pattern`
  helper instead of five interleaved mutable booleans. Behavior is
  preserved (existing #282 regression tests still pass); two new tests
  cover typed-discard (`int _ =>`) and guarded var-underscore
  (`var _ when g =>`) paths
  ([#303](https://github.com/dekobon/big-code-analysis/issues/303)).
- `apply_suppression` (`src/spaces.rs`) now matches the file-scope
  target on `SpaceKind::Unit` explicitly instead of taking
  `state_stack.first_mut()`. The function-scope arm already used an
  explicit `SpaceKind::Function` predicate; this aligns the two arms
  so a future regression that leaves a non-Unit frame at index 0
  silently drops the file marker rather than attaching it to an
  arbitrary frame. New tests pin both the positive case (Unit root
  accepts the marker) and the defensive case (no Unit frame anywhere
  on the stack is a silent no-op)
  ([#306](https://github.com/dekobon/big-code-analysis/issues/306)).
- Extracted the `cfg(...)` predicate parser from `src/checker.rs`
  (~217 lines of string-level parsing plus five `cfg_*` helpers) into
  a dedicated `src/cfg_predicate.rs` module with a single
  `pub(crate) fn attribute_marks_test` entry point. Helpers and the
  regression tests added by #278 move with the parser. Aligns with
  the existing `c_macro.rs` / `preproc.rs` / `suppression.rs` pattern
  of top-level helper modules; pure extraction, zero behavior change
  ([#304](https://github.com/dekobon/big-code-analysis/issues/304)).
- Replaced the `FunctionDefinition4` source-grep regression test in
  `src/spaces.rs` (which read `src/checker.rs` and `src/getter.rs`
  from disk and string-matched their bodies) with documenting
  comments at the four C++ predicate call sites. The production
  `matches!` patterns already enumerate every `Cpp::FunctionDefinition`
  alias by name and are themselves the structural contract; the grep
  test was brittle to cosmetic edits and could pass vacuously
  ([#302](https://github.com/dekobon/big-code-analysis/issues/302)).
- Tightened the `Npm` and `Npa` Java/Groovy annotation-type tests to
  use `check_func_space` so each one additionally asserts that the
  `AnnotationTypeDeclaration` opens a `SpaceKind::Interface`
  FuncSpace named `Marker`, mirroring the sibling `Wmc` tightening
  in commit `ba2a8e3`. Factored the six annotation-type assertion
  blocks across `npm.rs` / `npa.rs` / `wmc.rs` into a single
  `tools::assert_child_space_kind(...)` test helper
  ([#307](https://github.com/dekobon/big-code-analysis/issues/307)).
- Tightened the `Npm` and `Npa` plain interface / class / trait
  tests with the same `check_func_space` + `assert_child_space_kind`
  pattern from #307. Each non-zero `interface_*_sum` assertion in
  `src/metrics/npa.rs` and `src/metrics/npm.rs` is now paired with
  a structural check that the corresponding declaration opens a
  `SpaceKind::Interface` (or `Class` / `Trait` for sibling spaces),
  so dropping `InterfaceDeclaration` / `TraitDeclaration` from a
  language's `is_func_space` no longer leaves the body-walker totals
  passing vacuously against the file-level Unit space. The Go test
  retains its pre-existing `check_metrics` form because
  `GoCode::is_func_space` does not promote `interface_type` to a
  FuncSpace at all — its `interface_*_sum` totals come from
  AST-level body walking, not the FuncSpace tree, and so are
  outside the failure mode this issue guards against
  ([#311](https://github.com/dekobon/big-code-analysis/issues/311)).
- **(library API, breaking)** `LANG::get_tree_sitter_language`
  returns `Result<tree_sitter::Language, MetricsError>` instead of
  `tree_sitter::Language` directly. Feature-gated builds need a
  way to report "this variant's grammar isn't compiled in" and
  panicking would violate the no-panic rule on disabled-language
  paths; the new signature surfaces the disabled state as
  `Err(MetricsError::LanguageDisabled(LANG))`. Callers that
  previously wrote `.set_language(&LANG::Rust.get_tree_sitter_language())`
  need to add `.expect("rust feature enabled")` (or propagate the
  error). This method is part of the value-not-stable surface (see
  STABILITY.md); the matching `action::<T>` shim was widened from
  `T::Res` to `Result<T::Res, MetricsError>` for the same reason
  ([#252](https://github.com/dekobon/big-code-analysis/issues/252)).
- **(library API)** `src/lib.rs` re-exports are now explicit:
  every previous `pub use module::*` glob has been replaced with a
  named `pub use module::{X, Y, Z}` list. Helpers that were only
  ever called from inside the crate but accidentally became part
  of the published surface via those globs are now `pub(crate)`.
  The known curated public types (`analyze`, `Source`,
  `MetricsOptions`, `MetricsError`, `Metric`, `MetricSet`, `LANG`,
  `FuncSpace`, `CodeMetrics`, `SpaceKind`, `Node`, `Cursor`,
  the per-language `<Lang>Code` / `<Lang>Parser` tags, the
  `metrics` / `output` sub-modules, the `tree_sitter` re-export,
  and the deprecated path-positional shims) keep their crate-root
  paths so the CLI, web crate, integration tests, and the book
  examples continue to compile unchanged. The published API as
  rendered by `cargo doc` is now noticeably smaller
  ([#255](https://github.com/dekobon/big-code-analysis/issues/255)).
- `ParserTrait`, the per-metric compute traits (`Cognitive`,
  `Cyclomatic`, `Halstead`, `Loc`, `Mi`, `Nom`, `NArgs`, `Exit`,
  `Abc`, `Npa`, `Npm`, `Tokens`, `Wmc`), and the supporting
  `Checker` / `Getter` / `Alterator` traits are now
  `#[doc(hidden)]`. `Parser<T>` and `Filter` are also `#[doc(hidden)]`.
  The generic `ParserTrait`-bound shims (`metrics`,
  `metrics_with_options`, `operands_and_operators`, `find`, `count`,
  `function`, `rm_comments`) keep their signatures (they remain
  callable from the CLI / web crates) but are likewise
  `#[doc(hidden)]` so they no longer appear in the curated rustdoc
  surface. `metrics` and `metrics_with_options` additionally carry
  `#[deprecated]` in favour of `analyze` (see #253 / #254). The non-generic
  `analyze` / `metrics_from_tree` / `get_function_spaces*` /
  `get_ops` entry points are now the documented surface for
  language-dispatched analysis. `Callback` and `action::<T>`
  remain documented and unchanged; their fate is tied to the REST
  API shape and will be re-evaluated separately
  ([#256](https://github.com/dekobon/big-code-analysis/issues/256)).
- **(breaking)** Removed `FuncSpace::name_was_lossy`. The new
  `analyze` entry point makes the top-level name an explicit
  caller-supplied `Option<String>` (via `Source::name`), so the
  lossy-conversion workaround disappears. The deprecated path-
  positional shims (`metrics`, `metrics_with_options`,
  `get_function_spaces`, `get_function_spaces_with_options`) still
  derive `FuncSpace::name` from `path` via lossy UTF-8 conversion
  for backwards compatibility but no longer surface a `name_was_lossy`
  bit. Downstream consumers that read `name_was_lossy` from
  serialized output must drop that field; consumers that need a
  stable identifier should pass `Source::name` directly via the
  new `analyze` entry point
  ([#254](https://github.com/dekobon/big-code-analysis/issues/254)).
- The path-positional entry points (`metrics`, `metrics_with_options`,
  `get_function_spaces`, `get_function_spaces_with_options`) are
  now `#[deprecated(since = "0.0.26", …)]` in favour of
  `analyze(Source, MetricsOptions)`. They remain functional for one
  minor release. The CLI and web crate still call the deprecated
  shims internally (they always have a `&Path` in hand); library
  consumers should migrate to `analyze`
  ([#254](https://github.com/dekobon/big-code-analysis/issues/254)).
- **(breaking)** Library entry points now return
  `Result<FuncSpace, MetricsError>` (and `Result<Ops, MetricsError>` /
  `Result<Vec<Node>, MetricsError>` for the sibling APIs) instead of
  `Option<…>`. Affected: `metrics`, `metrics_with_options`,
  `get_function_spaces`, `get_function_spaces_with_options`,
  `operands_and_operators`, `get_ops`, and `find`. The new
  `MetricsError` enum (`#[non_exhaustive]`, implements
  `std::error::Error` + `Display`) distinguishes empty-input
  (`EmptyRoot`), disabled-language (`LanguageDisabled(LANG)`),
  non-UTF-8 paths (`NonUtf8Path`), and strict-mode parse errors
  (`ParseHasErrors`); only `EmptyRoot` is produced today, the rest
  are reserved for the matching follow-up issues (#252, #254, and a
  future strict-parse toggle). The CLI and web crates adapt; the
  REST `WebMetricsResponse.spaces` schema is intentionally
  unchanged and keeps `Option<FuncSpace>` (parallels the
  `AstResponse.root` decision)
  ([#253](https://github.com/dekobon/big-code-analysis/issues/253)).
- Bumped `jsonschema` from `0.46.4` to `0.46.5` (patch: percent-
  encoded characters in `$ref` URI fragments are now decoded when
  stored as `schema_path`)
  ([#237](https://github.com/dekobon/big-code-analysis/issues/237)).
- Bumped seven GitHub Actions to their latest pinned versions:
  `actions/checkout` v4.3.1 → v6.0.2 (mutation-test.yml),
  `EmbarkStudios/cargo-deny-action` v2.0.17 → v2.0.18,
  `taiki-e/install-action` v2.62.x → v2.78.2,
  `actions/setup-python` v5.6.0 → v6.2.0,
  `actions/setup-node` v5.0.0 → v6.4.0,
  `github/codeql-action` v4.35.2 → v4.35.5,
  `actions/upload-artifact` v4.6.2 → v7.0.1
  (mutation-test.yml). Also corrected a stale `# v2.62.23`
  comment in release.yml that sat next to the v2.78.2 SHA
  ([#238](https://github.com/dekobon/big-code-analysis/issues/238)).
- **(breaking)** Offender-record output formats (Checkstyle, SARIF,
  clang/GCC warning lines, MSVC warning lines) moved from `bca metrics
  --output-format <fmt>` to `bca check --output-format <fmt>` with a
  new `--output <path>` option. `bca metrics` keeps the per-file
  serializations (`json` / `yaml` / `toml` / `cbor` / `csv`). Legacy
  invocations now exit with a migration hint pointing at the new
  command; the empty-document placeholder behaviour is removed. The
  CLI version bumps to `0.1.0` and the book chapters for `metrics`,
  `report`, and `check` are updated to be internally consistent about
  which command owns which output kind
  ([#235](https://github.com/dekobon/big-code-analysis/issues/235)).
- Python `case_clause` bare-`_`-plus-guard classifier is now shared
  between `Cyclomatic for PythonCode` and `Abc for PythonCode` via
  a single `python_case_clause_counts` helper in
  `src/metrics/npa.rs`. No behaviour change; pure code-quality
  refactor ([#223](https://github.com/dekobon/big-code-analysis/issues/223)).
- **(breaking)** `Abc::compute` and `Cognitive::compute` now take the
  source bytes as a third parameter — `fn compute<'a>(node: &Node<'a>,
  code: &'a [u8], stats: &mut Stats)` — mirroring `Cyclomatic::compute`
  and `Exit::compute`. Languages whose control-flow constructs surface
  as untyped `Call` nodes (Elixir most notably) can identify them by
  inspecting the call target's text. Per-language impls that do not
  need the bytes discard them with `_`
  ([#206](https://github.com/dekobon/big-code-analysis/issues/206)).
- **(breaking)** `Cyclomatic::compute` now takes the source bytes as
  a third parameter — `fn compute<'a>(node: &Node<'a>, code: &'a [u8],
  stats: &mut Stats)` — mirroring `Exit::compute`. Languages whose
  branching constructs surface as untyped `Call` nodes (Elixir most
  notably) can identify them by inspecting the call target's text.
  Per-language impls that do not need the bytes discard them with
  `_`. The Elixir impl now distinguishes `if`/`unless`/`for`/`while`/
  `with`/`case`/`cond`/`try` Calls: single-branch keyword Calls
  contribute to both standard and modified CCN, while multi-arm
  container Calls (`case`/`cond`/`with`/`try`) contribute to modified
  only — per-arm `stab_clause`s carry standard CCN, mirroring the
  C-family case/switch treatment
  ([#179](https://github.com/dekobon/big-code-analysis/issues/179)).
- Workspace-wide pedantic clippy + `missing_docs` lint posture is now
  enforced. `[workspace.lints.rust]` adds `missing_docs = "warn"` and
  `[workspace.lints.clippy]` adds `pedantic = "warn"` with explicit
  carve-outs (`module_name_repetitions`, `missing_errors_doc`,
  `too_many_lines`, `similar_names`,
  `doc_markdown`, `needless_pass_by_value`, `struct_field_names`,
  `if_not_else`, `unused_self`, `match_wildcard_for_single_variants`,
  `struct_excessive_bools`, `ref_option`, each justified inline). All
  three shipping crates inherit via `[lints] workspace = true`.
  `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` and the default-features variant both exit clean
  ([#158](https://github.com/dekobon/big-code-analysis/issues/158)).
- Downgraded ~254 `#[inline(always)]` attributes to `#[inline]`
  across language modules, metric modules, and the `enums/`
  template, removing the `clippy::inline_always` warnings and
  letting LLVM decide on inlining. Mechanical batch alongside
  fixes for `clippy::semicolon_if_nothing_returned`,
  `clippy::redundant_else`, `clippy::redundant_closure`,
  `clippy::items_after_statements`,
  `clippy::unnecessary_debug_formatting` (path `{:?}` →
  `path.display()` in `eprintln!` warning logs),
  `clippy::unnested_or_patterns`, `clippy::implicit_clone`,
  `clippy::manual_string_new`, `clippy::needless_raw_string_hashes`,
  and `clippy::uninlined_format_args`. Public API unchanged
  ([#158](https://github.com/dekobon/big-code-analysis/issues/158)).
- Cargo workspace now uses `resolver = "3"` and inherits shared
  package metadata (`version`, `edition`, `rust-version`, `license`,
  `authors`) via `[workspace.package]` so the three shipping crates
  have a single source of truth. Per-crate `repository` URLs are
  preserved so each crate's crates.io page still links to its own
  subdirectory ([#150](https://github.com/dekobon/big-code-analysis/issues/150)).
- MSRV is now declared as `1.94` in `[workspace.package]`
  ([#150](https://github.com/dekobon/big-code-analysis/issues/150)).
- `[profile.release]` drops `strip = "debuginfo"` and sets
  `debug = "line-tables-only"` so release packaging can split
  symbols into separate `.dbg` artefacts and panic backtraces still
  carry line numbers. The same change applies to `enums/`'s
  independent release profile
  ([#150](https://github.com/dekobon/big-code-analysis/issues/150)).
- The 5 vendored grammars (`tree-sitter-ccomment`, `tree-sitter-mozcpp`,
  `tree-sitter-mozjs`, `tree-sitter-preproc`, `tree-sitter-tcl`) and
  the `enums` codegen helper are now marked `publish = false` and
  excluded from the workspace member list, leaving exactly three
  publishable packages (`big-code-analysis`, `big-code-analysis-cli`,
  `big-code-analysis-web`)
  ([#150](https://github.com/dekobon/big-code-analysis/issues/150)).
- The 18 shared `tree-sitter*` version pins (13 external, 5 vendored
  path-deps) are now consolidated in `[workspace.dependencies]` in the
  root `Cargo.toml`; the root crate inherits them via
  `.workspace = true`. `enums/Cargo.toml` is `[workspace].exclude`d and
  cannot inherit, so it keeps literal pins with a lockstep-update
  comment in both manifests
  ([#159](https://github.com/dekobon/big-code-analysis/issues/159)).
- Promoted the workspace-excluded `enums` crate's CI gate from
  `cargo check` to `cargo clippy --all-targets --locked -- -D warnings`,
  fixing three pre-existing `clippy::manual_is_ascii_check` sites in
  `enums/src/common.rs` (replaced range-based ASCII checks with
  `c.is_ascii_lowercase()` / `is_ascii_uppercase()` / `is_ascii_digit()`).
  The gate now enforces the same lint floor as the workspace
  ([#166](https://github.com/dekobon/big-code-analysis/issues/166)).
- `kotlin_loc_no_zero_blank` test (`src/metrics/loc.rs`) rewritten to
  actually exercise its advertised contract: the input now interleaves
  a blank line between trailing-comment code so the test asserts
  `blank() == 1.0` rather than `blank() == 0.0`. The original
  no-blank-input coverage is preserved under
  `kotlin_loc_blank_zero_sanity`
  ([#200](https://github.com/dekobon/big-code-analysis/issues/200)).
- Rewrote `.github/dependabot.yml`: added a `github-actions` ecosystem
  entry (grouped, weekly, `ci:` commit prefix) so SHA-pinned action
  bumps auto-update; standardised cargo entries on `deps:` prefix and
  added `version-update:semver-major` ignore rules so MSRV-bumping
  deps no longer auto-merge; trimmed `open-pull-requests-limit` from
  99 to 5 for the five vendored grammar directories and `/enums`
  (kept 99 for `/`); added a previously-missing cargo entry for
  `/tree-sitter-tcl`
  ([#154](https://github.com/dekobon/big-code-analysis/issues/154)).
- `Node::is_child(id)` avoids the per-call `TreeCursor` heap
  allocation by walking via `child(0)` + `next_sibling()` instead
  of `children(&mut self.0.walk())`. Behaviour-preserving; total
  cost stays O(n). Hot on the JS/TS/TSX/Mozjs template-literal
  arms in `src/getter.rs`
  ([#217](https://github.com/dekobon/big-code-analysis/issues/217)).
- Lesson-9 partial-input tests split into two suites for honesty:
  16 `*_top_level_space_is_unit_contract` tests pin the public API
  contract, and `lua_partial_input_yields_synthetic_unit_wrapper`
  and `cpp_error_root_yields_unit_top_level_space` are the only two
  that today actually exercise the synthetic-Unit wrapper in
  `metrics()`. The naming was previously uniform and implied all
  18 tests exercised the wrapper
  ([#220](https://github.com/dekobon/big-code-analysis/issues/220)).

### Fixed

- `extract_summaries_inner` in the CLI's markdown-report walker now
  walks the `FuncSpace` tree iteratively (explicit `Vec<&FuncSpace>`
  stack with children pushed in reverse for source order). The prior
  recursive form reintroduced the unbounded-recursion DoS that #292
  closed elsewhere — an adversarially deep AST (chained lambdas,
  generated parser fixtures) would overflow the worker thread's
  default 2 MiB stack during `bca report` execution. Mirrors the
  iterative pattern in `ThresholdSet::evaluate_with_policy`; see
  lesson 13
  ([#338](https://github.com/dekobon/big-code-analysis/issues/338)).
- `MetricSet`'s internal bitfield is now `u32`. `Metric::bit()` shifted
  `1 << (self as u32)` but the storage was `u16`, so adding a 17th
  variant (the enum is `#[non_exhaustive]` precisely so that can
  happen) would panic in debug or silently wrap in release, corrupting
  every `MetricSet` operation. Widened storage and bit-width to `u32`
  for 32 metrics of headroom, with three regression tests pinning the
  invariants: every variant maps to a distinct non-zero bit, every
  variant round-trips through `MetricSet::all().contains()`, and the
  variant count stays within the storage width
  ([#339](https://github.com/dekobon/big-code-analysis/issues/339)).
- `XmlAttr` in the Checkstyle XML output now escapes TAB, LF, and CR
  as numeric character references (`&#x9;`, `&#xA;`, `&#xD;`). The
  previous implementation passed these bytes through literally on the
  belief they would survive — but XML 1.0 §3.3.3 mandates that any
  whitespace character inside an attribute value (other than via a
  numeric character reference) is normalized to a single space on
  read. POSIX paths with embedded newlines and any future multi-line
  message template would silently lose their whitespace structure on
  every conforming consumer (Jenkins, SonarQube, IDE plugins).
  Round-trip test added that re-parses the emitted XML with
  `quick-xml` and asserts the bytes survive
  ([#340](https://github.com/dekobon/big-code-analysis/issues/340)).
- `to_sarif` in the Python bindings now silently skips `None`
  iterable entries alongside its existing `AnalysisError` skip
  semantics, **and** accepts a scalar `None` (yielding a
  well-formed empty SARIF run). The natural compositions
  `bca.to_sarif([bca.analyze(p) for p in paths])` and
  `bca.to_sarif(bca.analyze(p))` previously raised `TypeError`
  whenever any input file was classified as generated — the
  documented return of `analyze()` for those files is `None`. Both
  `None` and `AnalysisError` now represent "no record emitted for
  this file". The TypeError message for genuinely-unsupported items
  now lists `None` as an accepted shape
  ([#341](https://github.com/dekobon/big-code-analysis/issues/341)).
- `sanitize_identifier` in the `enums` crate now matches both the
  canonical `\u{FEFF}` BOM (the shape tree-sitter actually emits
  from `node_kind_for_id` after UTF-8 decoding) and the
  three-codepoint `\u{00EF}\u{00BB}\u{00BF}` mojibake form the
  previous `"ï»¿"` literal decoded to. A future grammar that
  surfaces a BOM token now gets a stable `BOM` identifier instead of
  falling through to the `Anon<N>` fallback. New `tests` module
  anchors both BOM shapes plus the reserved-keyword table
  ([#345](https://github.com/dekobon/big-code-analysis/issues/345)).
- TypeScript and TSX Halstead now classify the `string` type-keyword
  alias as an operand, matching `Checker::is_string`. The tree-sitter
  TS / TSX grammars expose the `string` keyword used in type
  annotations (`: string`) as an anonymous alias of `String` —
  `Typescript::String2` (kind_id 135) in TS and `Tsx::String3`
  (kind_id 141) in TSX. `Checker::is_string` matched both (#283),
  but `Getter::get_op_type` for `TypescriptCode` and `TsxCode`
  dropped them to `Unknown`, so every `: string` annotation was
  silently undercounted by one Halstead operand. `String2` is now
  in `operand_extras` for TypeScript and `String3` is now in
  `operand_extras` for TSX, restoring per-language parity with the
  JS / MozJS / TSX (for `String2`) classifications and closing the
  Checker/Getter agreement gap. Cross-language regression covered
  by `ts_family_string2_string3_type_keyword_parity_313` in
  `src/metrics/halstead.rs`
  ([#313](https://github.com/dekobon/big-code-analysis/issues/313)).
- The Rust `cfg(...)` slow-path whitespace collapser in
  `cfg_predicate::attribute_marks_test` now decodes UTF-8 correctly.
  The previous implementation rebuilt the compact string with
  `bytes().filter(...).map(char::from).collect()`, treating each
  byte as a Latin-1 codepoint and mangling any multi-byte UTF-8
  sequence (e.g. `é` / `0xC3 0xA9` became `Ã©`). The fix iterates
  over `chars()` so multi-byte sequences survive intact. Latent
  today — `matches_test` only recognises ASCII identifiers, so no
  current cfg rule could observe the mangling — but the pattern was
  wrong by construction and would have surprised any future rule
  that keyed off a non-ASCII identifier
  ([#312](https://github.com/dekobon/big-code-analysis/issues/312)).
- Elixir `Wmc` and `Npm` now agree on the methods of a class. A
  `def` / `defp` / `defmacro` / `defmacrop` nested inside a
  `quote do … end` template is no longer promoted to a Function
  space — that syntax tree is a code template emitted on macro
  expansion, not a real method of any enclosing `defmodule`. Before
  this fix, `Wmc` walked the entire Function-space subtree under a
  Class and counted the quoted `def`s, while `Npm` filtered by
  direct children of the module's `do_block` and excluded them. A
  new `Checker::promotes_to_func_space_with_code` predicate
  centralises the func-space decision (default impl forwards to
  `is_func_with_code || is_func_space_with_code`); Elixir overrides
  it to consult `elixir_is_inside_quote_block` once per `Call`
  node, replacing what was previously three independent
  `elixir_call_keyword` lookups per Call in the walker
  ([#310](https://github.com/dekobon/big-code-analysis/issues/310)).
- `bca check --baseline` now produces injective baseline keys for
  Windows non-UTF-8 paths. The Windows branch of `normalize_path`
  previously fell back to `to_string_lossy()`, which substitutes
  U+FFFD for invalid UTF-16 surrogates and could collide two
  distinct paths onto one baseline entry. The fix walks the WTF-16
  sequence with `OsStrExt::encode_wide`, decodes valid scalar
  values as UTF-8 (sharing the per-byte percent-encoder with the
  Unix branch), and emits `%uHHHH` (a marker disjoint from `%XX`)
  for unpaired surrogates so distinct invalid-surrogate inputs map
  to distinct keys. A `cfg(not(any(unix, windows)))` fallback
  preserves the U+FFFD prefix anti-collision marker for wasm-like
  targets, and the encoder is covered by always-on unit tests
  plus a `#[cfg(windows)]` integration test
  ([#305](https://github.com/dekobon/big-code-analysis/issues/305)).
- `bca check --baseline` no longer collides a UTF-8 path containing
  the literal text `%FF` with a non-UTF-8 path containing the byte
  `0xFF`. The UTF-8 fast path in `normalize_path` previously emitted
  `%` verbatim while the non-UTF-8 branch percent-encoded it,
  producing the same key (`foo%FF.rs`) for both inputs. The encoder
  is now total: every byte that is not in the unreserved set —
  including `%` — is escaped, so the UTF-8 input becomes `foo%25FF.rs`
  and remains disjoint from the non-UTF-8 key. **(breaking)** The
  baseline schema is bumped to `version = 2`; v1 baselines containing
  non-ASCII or `%`-bearing paths must be regenerated with
  `bca check --write-baseline` (the version mismatch surfaces the
  existing "regenerate" hint instead of silently failing to match)
  ([#298](https://github.com/dekobon/big-code-analysis/issues/298)).
- `Halstead` (C#) keys predefined type keywords (`int`, `string`,
  `bool`, `object`, …) by source text instead of collapsing every
  keyword onto a single `n1` slot. The fix flips
  `CsharpCode::is_primitive` to return true for
  `Csharp::PredefinedType` so the finalization path stores the node
  under its lexeme, mirroring how C++ `PrimitiveType` is keyed.
  `n1`, vocabulary, volume, and downstream MI now reflect the real
  number of distinct type keywords in C# source
  ([#286](https://github.com/dekobon/big-code-analysis/issues/286)).
- `Halstead` (Perl) recognises heredoc literals
  (`Perl::HeredocBodyStatement`) as both string-filter targets and
  operand sources. Inert heredocs contribute one operand; heredocs
  carrying `$var` / `@var` interpolation drop to `Unknown` so the
  inner substitution attributes exclusively (no double-count)
  ([#287](https://github.com/dekobon/big-code-analysis/issues/287)).
- `Halstead` (Tcl) guards `Tcl::QuotedWord` against double-counting
  embedded `$var` / `[cmd]` substitutions. Inert
  `"hello world"` strings still count as one operand; strings
  containing `VariableSubstitution` or `CommandSubstitution`
  classify as `Unknown` so the inner substitution carries the
  count. Matches the existing PHP / Bash / C# / Kotlin / Elixir /
  Ruby / Python interpolation guards
  ([#277](https://github.com/dekobon/big-code-analysis/issues/277)).
- PHP string-like node handling is now consistent across the
  checker, alterator, and Halstead getter. `Php::String2` and
  `Php::String3` (the anonymous "string" type-keyword alias and
  the hidden supertype) are recognised by `is_string` and
  `alterate`, and `ShellCommandExpression` (backtick command
  literals) now contributes a Halstead operand — gated by
  `php_string_has_interpolation` so interpolated backticks do not
  double-count
  ([#288](https://github.com/dekobon/big-code-analysis/issues/288)).
- `Abc` (C#) now counts unary and single-token `for`-loop
  conditions (`for (; ready ;)`, `for (; Ok() ;)`,
  `for (; true ;)`) via an explicit `ForStatement` arm that mirrors
  the existing Java logic. Empty conditions still contribute zero;
  comparison conditions retain their existing operator-arm
  contribution
  ([#279](https://github.com/dekobon/big-code-analysis/issues/279)).
- C++ now classifies `Cpp::FunctionDefinition4` as a function
  space. `is_func_space`, `get_func_space_name`, and
  `get_space_kind` all handle the fourth aliased
  `function_definition` kind identically to the other three, so
  C++ functions emitted through that alias keep their
  function-space identity instead of falling through to
  `SpaceKind::Unknown`
  ([#285](https://github.com/dekobon/big-code-analysis/issues/285)).
- Java and Groovy `enum`, `record`, and `@interface` declarations
  are now recognised as class-like spaces, so `Npa`, `Npm`, and
  `Wmc` walk their bodies and produce non-zero counts on common
  declaration forms. Enum / record bodies map to
  `SpaceKind::Class`; annotation-type bodies map to
  `SpaceKind::Interface` (annotation elements are abstract methods
  at the bytecode level)
  ([#280](https://github.com/dekobon/big-code-analysis/issues/280)).
- Optional chaining (`?.`) is now normalised across the JS family.
  TypeScript and TSX Halstead used to count both
  `OptionalChain` (the wrapping kind) and `QMARKDOT` (the bare
  token); the wrapper is now dropped so each textual `?.`
  contributes exactly one operator. JS-family cyclomatic now adds
  +1 per `?.` short-circuit (`OptionalChain` for JS/MozJS,
  `QMARKDOT` for TS/TSX) so the construct is treated as a
  decision point like `&&` / `||` / `??`
  ([#281](https://github.com/dekobon/big-code-analysis/issues/281)).
- Cyclomatic no longer over-counts wildcard switch arms in C# or
  Kotlin. C# `SwitchExpressionArm` with a bare `_` discard pattern
  (or `var _` declaration pattern) is skipped; guarded discards
  (`_ when g => …`) still count via the `WhenClause`. Kotlin
  `WhenEntry` is detected as the `else` arm via the absence of the
  `condition` field and skipped
  ([#282](https://github.com/dekobon/big-code-analysis/issues/282)).
- `Checker::is_string` (JavaScript / MozJS / TypeScript / TSX) now
  includes the anonymous `String2` (and TSX `String3`) aliases that
  the generated language enums map to `"string"`. The public
  `bca find string` / `count string` filters were previously
  silently dropping string literals on these alias kinds
  ([#283](https://github.com/dekobon/big-code-analysis/issues/283)).
- `Checker::is_else_if` (Python) detects `else: if …` chains
  wrapped in `else_clause`, matching the C++/JS/TS/TSX/Rust
  pattern. The `elif_clause` shape was already handled
  structurally by the cognitive metric via
  `increment_branch_extension`, so the predicate stayed false for
  that case by design; this is now documented inline. A regression
  test pins `if / elif / elif / else` cognitive at the documented
  flat-chain score so future refactors cannot silently re-nest the
  chain
  ([#274](https://github.com/dekobon/big-code-analysis/issues/274)).
- Cyclomatic for C++ `do { … } while (…)` / `for (auto x : …)` and
  Java/Groovy `do { … } while (…)` / `for (Foo x : …)` is now
  pinned by regression tests against the C-family keyword-token
  semantics (`While` / `For` already fire +1 via the trailing or
  leading keyword inside `DoStatement` / `ForRangeLoop` /
  `EnhancedForStatement`). The match-arm doc comments now spell
  out the contract so a future contributor cannot misread the
  keyword-token approach as a missing statement-node arm and
  introduce a double-count
  ([#284](https://github.com/dekobon/big-code-analysis/issues/284)).
- `rust_attribute_marks_test` now recognises the `test` predicate
  anywhere inside a `cfg(...)` attribute, not just as the first
  argument of `cfg(all(...))` / `cfg(any(...))`. Forms like
  `#[cfg(all(unix, test))]` and `#[cfg(any(feature = "x", test))]`
  are now elided when `MetricsOptions::exclude_tests()` is set; the
  walker refuses to descend into `not(...)` so `cfg(not(test))`
  and `cfg(all(unix, not(test)))` correctly remain production
  code, and `cfg(feature = "test")` (a feature literally named
  `"test"`) is not treated as a test predicate
  ([#278](https://github.com/dekobon/big-code-analysis/issues/278)).
- The C/C++ macro-masking prepass now tracks lexical context, so
  identifiers inside string literals (`"DBG"`), char literals
  (`'D'`), single-line comments (`// DBG`), multi-line comments
  (`/* DBG */`), and raw string literals (`R"delim(DBG)delim"`)
  are no longer rewritten. The synthetic parse buffer now matches
  real preprocessor semantics — macro masking only affects
  identifier occurrences a real C/C++ preprocessor could expand
  ([#290](https://github.com/dekobon/big-code-analysis/issues/290)).
- C/C++ `#include` resolution now preserves caller-relative `..`
  segments. `guess_file` joins the include path against the
  including file's parent before lexical normalisation, then
  matches candidates against the fully resolved relative target
  before falling back to basename / same-directory / distance
  heuristics. `#include "../foo.h"` no longer collapses to the
  basename and can no longer pick a sibling header with the same
  name in a different directory
  ([#297](https://github.com/dekobon/big-code-analysis/issues/297)).
- `bca` per-file output and baseline identity keys preserve
  non-UTF-8 path components instead of dropping them lossily.
  Output filenames carry the raw byte sequence as `OsString`, so
  two distinct non-UTF-8 paths produce two distinct output files.
  Baseline keys percent-encode non-UTF-8 bytes (Unix) so the
  TOML-stable key is injective across distinct paths; UTF-8 paths
  retain the prior byte-identical key
  ([#295](https://github.com/dekobon/big-code-analysis/issues/295)).
- `bca-web` plain-endpoint tests now exercise the same
  `application/octet-stream` `guard::Header` that the production
  `/comment`, `/metrics`, and `/function` routes are installed with
  in `run()`. The previous tests mounted bare handlers without the
  guard and sent `text/plain` requests — succeeding on a routing
  shape that would not exist in deployment. New
  `*_rejects_text_plain` cases lock in the guard contract by
  asserting a 404 when the content type does not match. No
  production routing change; this is a test-fidelity fix
  ([#294](https://github.com/dekobon/big-code-analysis/issues/294)).
- `bca-web` now re-checks the orphaned-task cap after acquiring a
  semaphore permit, closing a race where a burst of queued requests
  could all pass the pre-admission check while the orphan counter
  was still low, then drain the semaphore one at a time and each
  spawn another `spawn_blocking` task — growing the orphan pool
  past `BCA_MAX_ORPHANED_TASKS` and defeating the configured cap.
  The fast-path check is retained as a cheap rejection before the
  semaphore wait, but the post-admission re-check is now the hard
  gate. Counter updates use `Acquire`/`Release` ordering so admitted
  requests observe orphan counts published by any prior orphaning
  task ([#291](https://github.com/dekobon/big-code-analysis/issues/291)).
- In-source suppression markers (`bca: suppress`, `bca: suppress(metric,
  ...)`, and the `#lizard forgives` compat form) now attach to the
  syntactically enclosing function rather than to whichever function's
  line range covered the comment's source line. The previous resolver
  matched on `start_line..=end_line` and picked the first hit by source
  order, which silently attached a marker to the wrong sibling whenever
  two single-line functions shared a row (e.g.
  `int a(){...} int b(){/*bca: suppress*/...}` attached to `a`). The
  walker now applies markers inline against the active state stack at
  the comment node so the topmost `SpaceKind::Function` frame — the
  only function the grammar nested the comment inside — wins. A
  user-visible side effect: a marker on the closing-brace line but
  *outside* the function body (a sibling of `function_definition`, not
  a child of it) no longer attaches; previously the line-range match
  would have caught it
  ([#289](https://github.com/dekobon/big-code-analysis/issues/289)).
- Suppression attachment is now O(stack depth) per marker on the
  iterative walker stack instead of recursing once per nested
  `FuncSpace` on the Rust call stack. The pre-fix
  `attach_function_suppression` helper overflowed the default 8 MiB
  thread stack on inputs with ~1000-deep nested functions; the
  iterative replacement scales to arbitrary nesting
  ([#292](https://github.com/dekobon/big-code-analysis/issues/292)).
- `bca find <NODE>` and `bca count <NODE_TYPE>` now match node kinds
  exactly. Unknown filters that were not a hardcoded keyword
  (`all`/`call`/`comment`/`error`/`string`/`function`) or numeric
  `kind_id` previously fell through to `node.kind().contains(&f)`,
  so a filter like `expression` collapsed `binary_expression`,
  `parenthesized_expression`, `expression_statement`, etc. into one
  bucket — contradicting the CLI documentation, which describes both
  verbs as searching for *a specific node type*
  ([#293](https://github.com/dekobon/big-code-analysis/issues/293)).
- `Cyclomatic` now counts the compound short-circuit assignment
  operators `&&=` and `||=` in JavaScript / TypeScript / TSX /
  Mozjs, matching the existing `??=` handling and the cognitive
  parity from #236. Each compound short-circuit assignment is a
  distinct control-flow decision and must contribute uniformly.
  C# is unaffected (its grammar exposes only `??=`)
  ([#248](https://github.com/dekobon/big-code-analysis/issues/248)).
- `Cognitive` and `Cyclomatic` now count Perl's compound
  short-circuit assignments `&&=`, `||=`, and `//=` as boolean-
  sequence increments / decision edges. The Perl grammar exposes
  these as direct operator tokens inside `binary_expression`,
  unlike the JS family's `augmented_assignment_expression`; the
  predicates that already handle `&&`/`||`/`//` were extended in
  place
  ([#249](https://github.com/dekobon/big-code-analysis/issues/249)).
- `Cognitive` now counts the compound short-circuit assignment
  operators (`&&=`, `||=`, `??=`) in JavaScript / TypeScript /
  TSX / Mozjs and `??=` in C# / PHP. Pre-existing gap: cognitive
  inspected only `BinaryExpression` children, missing the
  `augmented_assignment_expression` container these operators sit
  in. Mirrors the cyclomatic fix from #231
  ([#236](https://github.com/dekobon/big-code-analysis/issues/236)).
- Kotlin's Elvis operator `?:` is now counted as a boolean-sequence
  operator in `Cognitive` (Sonar B1) and as a short-circuit
  decision in `Cyclomatic`, mirroring the JS `??` treatment from
  #226 / #230
  ([#239](https://github.com/dekobon/big-code-analysis/issues/239)).
- Python `Cognitive` ExceptClause now applies the correct nesting
  penalty for `except` clauses nested inside control-flow
  constructs (`if`, `for`, `while`, lambdas). The arm was using
  the stale `stats.nesting` because it bypassed the shared
  `increase_nesting` helper that every other language's
  catch/rescue path uses
  ([#242](https://github.com/dekobon/big-code-analysis/issues/242)).
- `Exit for RustCode` no longer adds a spurious `+1` for every
  Rust function with an explicit return type. The visit of the
  function's own `function_item` node was incrementing
  `stats.exit` inside the function's own state, double-counting
  any function with both an explicit return statement and a
  return type. Aligned with peer-language behaviour: only
  explicit `return` and `?` (TryExpression) count
  ([#243](https://github.com/dekobon/big-code-analysis/issues/243)).
- `mi_sei` now treats `comments_percentage` as a percentage in
  `[0, 100]` as the SEI formula `50·sin(√(2.4·CM))` requires.
  Previously stored as a ratio in `[0, 1]`, the argument to the
  `sqrt` was 100× too small and `MI_SEI` was wildly incorrect for
  any file with comments. The storage site was rescaled (private
  field; no public JSON schema change). All `mi_sei` values for
  files with non-zero comments will shift
  ([#241](https://github.com/dekobon/big-code-analysis/issues/241)).
- **(breaking — CLI internals)** `Violation::path` in
  `big-code-analysis-cli` is now `PathBuf` instead of `String`,
  and `ThresholdSet::evaluate` takes `&Path` instead of `&str`.
  The threshold pipeline previously dropped non-UTF-8 path bytes
  via `Path::to_str()` with a skip-and-warn fallback, so non-UTF-8
  source files could not surface in offender output at all. The
  bytes now round-trip through `Violation` and
  `violation_to_offender` end-to-end (lossy only at the
  human-facing `Display` boundary, via `Path::display()`)
  ([#240](https://github.com/dekobon/big-code-analysis/issues/240)).
- Dead `!matches!(list_kind, ArgumentList | …)` post-conditions
  in `java_count_unary_conditions` / `csharp_count_unary_conditions`
  removed. The preceding `matches!(list_kind, BinaryExpression)`
  already pinned `list_kind` to a single variant; the negated
  match was unreachable. Pure code-quality cleanup
  ([#245](https://github.com/dekobon/big-code-analysis/issues/245)).
- `Cognitive` now counts the nullish-coalescing operator `??` as a
  boolean-sequence operator (Sonar B1) in JavaScript, TypeScript,
  TSX, Mozjs, C#, and PHP. The `compute_booleans` two-operator helper
  is replaced at these call sites by the slice-friendly
  `compute_booleans_with`, mirroring Ruby / Perl / Elixir. Kotlin
  keeps the `&&` / `||` pair (no `??`). Closes the parity gap left by
  #226 on the cyclomatic side
  ([#230](https://github.com/dekobon/big-code-analysis/issues/230)).
- LOC `_min` getters (`sloc_min`, `ploc_min`, `lloc_min`, `cloc_min`,
  `blank_min`) now collapse the `usize::MAX` sentinel to `0.0`
  instead of leaking `1.8446744e19` from a raw `Stats::default()`
  that bypasses the metric pipeline. Mirrors the guard pattern
  already documented on `tokens::Stats::tokens_min` and applied to
  six other metrics in #227
  ([#233](https://github.com/dekobon/big-code-analysis/issues/233)).
- `NExit` now counts `yield` as an exit edge in Python, JavaScript,
  TypeScript, TSX, and Mozjs, matching the long-standing C# / PHP
  behaviour. Generator suspension hands control back to the caller —
  the function does leave its frame, just resumably — so it belongs
  alongside `return` / `throw` / `raise` in the exit-point count.
  Follow-up to #228, which closed the throw/raise parity gap and
  scoped `yield` out as a separate design call
  ([#232](https://github.com/dekobon/big-code-analysis/issues/232)).
- Python cyclomatic complexity no longer over-counts plain `if/else` by
  one. Root cause: the `has_ancestors` helper in `src/node.rs` did not
  actually verify both predicates against the expected ancestor chain;
  it returned true whenever the immediate parent matched the second
  predicate. The helper has been renamed to `parent_grandparent_match`
  and now strictly checks both. Python `try/except/else` is now
  counted alongside `for/else` and `while/else`
  ([#229](https://github.com/dekobon/big-code-analysis/issues/229)).
- Cyclomatic complexity now counts the nullish coalescing operator
  (`??`, token `QMARKQMARK`) as a short-circuit decision in
  JavaScript, TypeScript, TSX, and Mozjs, matching the existing C#
  and PHP treatment. `a ?? b` adds one decision edge to the CFG (does
  not evaluate `b` if `a` is non-null). The `impl_cyclomatic_c_family!`
  macro now takes the short-circuit operator list as a parameter so
  per-language differences (C++ has no `??`) stay explicit
  ([#226](https://github.com/dekobon/big-code-analysis/issues/226)).
- Cyclomatic complexity now counts the compound nullish-coalescing
  assignment operator (`??=`, token `QMARKQMARKEQ`) as a short-circuit
  decision in JavaScript, TypeScript, TSX, Mozjs, C#, and PHP. `a ??= b`
  is semantically `a = a ?? b` — it evaluates and assigns `b` only when
  `a` is null/undefined, the same one-decision-edge contribution as
  `??`. Sibling assignment forms `&&=` and `||=` remain uncounted and
  are tracked as a follow-up
  ([#231](https://github.com/dekobon/big-code-analysis/issues/231)).
- Cognitive complexity now counts the ternary `?:` operator with
  `+1 + nesting` for Java, C#, and PHP, matching `cyclomatic.rs`, the
  C++ fix from #172, and SonarSource Cognitive Complexity §2. Adds
  `TernaryExpression` (Java) and `ConditionalExpression` (C#, PHP) to
  each language's `increase_nesting` arm
  ([#224](https://github.com/dekobon/big-code-analysis/issues/224)).
- Cognitive complexity now counts labeled `break`/`continue` for
  Java and all forms of `goto` (`label`, `case`, `default`) for C#,
  mirroring the Rust/Go/C++/Perl/Lua handling per SonarSource
  Cognitive Complexity §B2. C#'s grammar does not allow labeled
  `break`/`continue` so only `goto_statement` is added there
  ([#225](https://github.com/dekobon/big-code-analysis/issues/225)).
- `throw`/`raise` now contribute to `NExit` in Python, JavaScript,
  TypeScript, TSX, Mozjs, Java, and C++, aligning with the existing
  C#/Kotlin/PHP/Elixir behaviour. `throw`/`raise` is a function exit
  by definition — control leaves the function and the stack unwinds.
  Fixtures containing throws see their `nexits` sum/min/max/average
  increase accordingly; no other metrics or structural fields change
  ([#228](https://github.com/dekobon/big-code-analysis/issues/228)).
- The `cognitive`, `cyclomatic`, `nom`, `nargs`, `exit`, and `abc`
  metric `_min` getters now collapse the `usize::MAX` / `f64::MAX`
  sentinel that `Stats::default()` plants to `0.0`, so a never-observed
  space serializes to a meaningful number rather than `1.8446744e19`
  (for `usize` sentinels) or `1.7976931e308` (for `f64` sentinels).
  Mirrors the existing guards in `tokens::Stats::tokens_min` and the
  three LOC variants
  ([#227](https://github.com/dekobon/big-code-analysis/issues/227)).
- Python `match`/`case` (PEP 634, 3.10+) now contributes decision
  points to both cyclomatic and cognitive complexity, matching Rust /
  C-family / Java / JS / TS / C# / PHP / Kotlin / Go / Bash. A 2-arm
  match with a wildcard previously reported `cyclomatic_max == 1` /
  `cognitive_max == 0`; it now reports `2` and `1`. Bare `case _:`
  (no guard) is filtered, mirroring Rust's `MatchArm` rule
  ([#212](https://github.com/dekobon/big-code-analysis/issues/212)).
- Bash 2-arm `case … esac` with a `*)` catch-all arm reported
  `cyclomatic_max == 3`; the bare `*)` is Bash's analogue of the
  C-family `default:` and is now excluded from the standard count,
  matching every other switch-bearing language. Multi-value patterns
  (`a|*)`) are NOT bare and still contribute a decision
  ([#211](https://github.com/dekobon/big-code-analysis/issues/211)).
- Python `Npa` impl now deduplicates `self.x = …` bindings by
  attribute identifier text. The defensive re-init pattern
  (`__init__` + `reset` both binding `self.value`) and conditional
  initialisation (`if flag: self.x = 1; else: self.x = 2`) count
  the attribute exactly once instead of inflating by one per
  re-bind. Uses the source bytes widened into the trait by #219
  ([#215](https://github.com/dekobon/big-code-analysis/issues/215)).
- Map `elixir` and `iex` shebang interpreters to `LANG::Elixir` so
  extensionless Elixir scripts (`#!/usr/bin/env elixir`) are correctly
  identified by `guess_language`
  ([#186](https://github.com/dekobon/big-code-analysis/issues/186)).
- Guard Python `String` and Kotlin `StringLiteral` /
  `MultilineStringLiteral` Halstead op-type with `is_child(Interpolation)`
  so f-strings (`f"Hi {name}!"`) and string templates (`"Hi $name!"` /
  `"${expr}"`) no longer double-count interpolated operands, matching the
  pattern already in place for Bash (#180), C# (#183), Elixir, PHP, and
  Ruby ([#191](https://github.com/dekobon/big-code-analysis/issues/191)).
- Correct nine sibling `*_no_zero_blank` tests in `src/metrics/loc.rs`
  (Elixir, Mozjs, Tcl, Bash, TypeScript, TSX, PHP, Perl, Lua) — they
  previously used no-blank input and asserted `blank == 0`, exactly
  inverting the contract their name advertised. Each now interleaves
  blank lines with code carrying trailing comments to exercise the
  `blank = sloc - (ploc ∪ cloc lines)` union math; Elixir, Lua, and
  Perl were also split into a renamed `*_blank_zero_sanity` test plus
  a real positive-case test
  ([#189](https://github.com/dekobon/big-code-analysis/issues/189)).
- C++20 spaceship operator `<=>` (`Cpp::LTEQGT`) now classified as
  Halstead operator; previously fell through to `Unknown` and was
  excluded from `n1`/`N1`
  ([#197](https://github.com/dekobon/big-code-analysis/issues/197)).
- C++ Halstead operator set now includes `-=` (`DASHEQ`), `.*`
  (`DOTSTAR`), and `->*` (`DASHGTSTAR`); previously these three
  fell through to `Unknown`
  ([#198](https://github.com/dekobon/big-code-analysis/issues/198)).
- Perl `string_double_quoted` / `string_qq_quoted` / `backtick_quoted`
  / `command_qx_quoted` literals no longer double-count their inner
  scalar/array/hash variables when an `interpolation` child is
  present; the wrapping string is now classified as `Unknown` only
  in that case, while plain (non-interpolated) Perl strings still
  count as one operand
  ([#199](https://github.com/dekobon/big-code-analysis/issues/199)).
- JavaScript / TypeScript / TSX / Mozjs template literals
  (`` `…` ``) are now Halstead operands; previously they fell
  through to `Unknown` (plain backtick strings contributed zero,
  interpolated literals dropped the wrapper entirely)
  ([#192](https://github.com/dekobon/big-code-analysis/issues/192)).

- Bash `variable_name` and `special_variable_name` are now classified
  as Halstead operands in every parse-table context. tree-sitter-bash
  emits these node kinds under three aliased `kind_id`s (`VariableName`
  / `VariableName2` / `VariableName3`) and two for special variables
  (`SpecialVariableName` / `SpecialVariableName2`); the original
  `impl Getter for BashCode::get_op_type` matched only the unsuffixed
  variant, so assignment LHS identifiers like `name` in `name=value`
  and the `name` child of `$name` simple expansions were silently
  unclassified. All five variants are now matched, restoring the
  intended operand contribution; `bash_operators_and_operands` is
  anchored with integer assertions and its snapshot refreshed to
  match. Same lesson-2 bug class as #40 / #36 / #50 / #44 / #94 / #119.
- Halstead operand counts for interpolated Elixir strings/sigils and
  Bash `$var`/`${…}`/`$(…)`/`$((…))`-bearing strings no longer
  double-count the inner identifiers. Elixir `String` / `Charlist` /
  `Sigil` and Bash `String` / `RawString` / `AnsiCString` /
  `TranslatedString` are still classified as one operand when they
  have no interpolation child, but skip classification when an
  `interpolation` (Elixir) or `simple_expansion` / `expansion` /
  `command_substitution` / `arithmetic_expansion` (Bash) child is
  present — so the inner expression contributes once instead of the
  wrapping literal contributing in addition to it. `N2`, `n2`,
  volume, and all derived metrics for code that uses interpolated
  strings idiomatically are now correspondingly lower
  ([#180](https://github.com/dekobon/big-code-analysis/issues/180)).
- Halstead operand counts for C# `$"..."` interpolated strings no
  longer double-count the inner identifiers.
  `CsharpCode::get_op_type` now routes `InterpolatedStringExpression`
  through a conditional check (mirroring the Elixir/Bash precedents
  from #180): when the literal carries any `Interpolation` child the
  inner expressions already contribute their identifiers as operands
  and the wrapper is classified as `Unknown`; when it does not (a
  static `$"hello"` with no `{...}` substitution), the wrapper still
  counts as one operand, matching the plain-string equivalent.
  `is_string` (for the LOC comment/code classifier) is unchanged. C#
  `linq.cs` / `strings.cs` integration snapshots refresh with lower
  `n2` / `N2` / volume / effort and slightly higher MI
  ([#183](https://github.com/dekobon/big-code-analysis/issues/183)).
- Halstead operand counts for PHP `"…$var…"` / `"…{$expr}…"`
  double-quoted (`EncapsedString`) and `<<<EOT … EOT;` interpolating
  heredoc literals no longer double-count the inner identifiers.
  `PhpCode::get_op_type` now routes `EncapsedString` and `Heredoc`
  through a conditional check (mirroring #180 / #183): when the
  literal carries a `$var` (`variable_name`), `${name}`
  (`dynamic_variable_name`), or `{$expr}` (a direct `{` brace child,
  or — for heredoc — any of the above inside `heredoc_body`)
  interpolation child, the inner expressions already contribute their
  identifiers as operands and the wrapper is classified as `Unknown`;
  when it does not (a plain `"hello world"` or a heredoc whose body
  is `string_content` only), the wrapper still counts as one
  operand, matching the single-quoted `String` / `Nowdoc` equivalent.
  `is_string` (for the LOC comment/code classifier) is unchanged. PHP
  `classes.php` / `control_flow.php` / `embedded.php` / `strings.php` /
  `traits_enums.php` integration snapshots refresh with lower
  `n2` / `N2` / volume and slightly higher MI
  ([#184](https://github.com/dekobon/big-code-analysis/issues/184)).
- Makefile `EXCLUDE_DIRS` no longer glob-expands the `tree-sitter-*`
  entry into absolute paths at recipe-execution time, which had
  silently neutered `make markdown-lint`, `make shellcheck`,
  `make sh-fmt`, and `make sh-fmt-check` (each piped to `xargs -r`
  against empty input and exited 0). The glob is now quoted in
  `EXCLUDE_DIRS` and the `find`-fallback path strips the quoting so
  vendored grammar trees stay excluded in both code paths
  ([#160](https://github.com/dekobon/big-code-analysis/issues/160)).
- Cleared 96 pre-existing `markdownlint-cli2` findings now that the
  markdown-lint gate actually runs. Source edits in 10 files
  (top-level README, the two crate-level READMEs, mdBook command and
  developer chapters, `docs/file-detection.md`) reflow long prose,
  demote stray H1s to H2 where appropriate, and add accessibility
  attributes to inline `<img>` badges. The remaining flagged files
  (AGENTS.md, CLAUDE.md, and book index pages) had their findings
  absorbed by widening `.markdownlint-cli2.jsonc`: MD033 now allows
  a narrow list of inline-HTML elements (`a`, `img`, `br`, `details`,
  `summary`) for legitimately GitHub-rendered constructs, and MD060
  (table-column-style) is disabled globally for content-driven tables
  ([#161](https://github.com/dekobon/big-code-analysis/issues/161)).
- Removed unused `pub use crate::macros::*;` re-export in
  `enums/src/lib.rs`. The line could not re-export the
  `macro_rules!` definitions in `enums/src/macros.rs` (macros use a
  separate name namespace and none carried `#[macro_export]`), so the
  re-export was dead. `#[macro_use] mod macros;` continues to make
  the macros visible within the crate
  ([#162](https://github.com/dekobon/big-code-analysis/issues/162)).
- Fixed shellcheck findings (SC2164 missing `|| exit` on pushd/popd,
  SC1083 literal `}` in path, SC2086 unquoted variable expansion,
  SC2006 legacy backticks) in
  `generate-grammars/{generate-grammar,generate-mozcpp,generate-mozjs}.sh`
  and applied `shfmt` formatting to `check-grammars-crates.sh` and
  `utils/check-tools.sh`. All findings were pre-existing and were
  silently masked by the Makefile `EXCLUDE_DIRS` glob bug fixed in
  [#160](https://github.com/dekobon/big-code-analysis/issues/160)
  ([#165](https://github.com/dekobon/big-code-analysis/issues/165)).
- C++ range-based `for (x : v)` loops are now scored by cognitive
  complexity. `CppCode::compute` in `src/metrics/cognitive.rs`
  previously matched only the classic `ForStatement`; the C++11
  `for_range_loop` node was missing from the dispatch, so range-fors
  cost `0` and nested range-fors did not compound. The match arm now
  includes `ForRangeLoop` alongside `ForStatement`, so range-fors
  add `1 + nesting` like every other loop. Flipped the lock-in test
  `c_range_based_for` to assert `+1`, added `c_nested_range_based_for`
  for the compounding case, and refreshed 99 DeepSpeech integration
  snapshots in the `big-code-analysis-output` submodule
  ([#173](https://github.com/dekobon/big-code-analysis/issues/173)).
- Java enhanced-for `for (T x : c)` loops are now scored by cognitive
  complexity. `JavaCode::compute` in `src/metrics/cognitive.rs`
  previously matched only the classic `ForStatement`; the
  `enhanced_for_statement` node was missing from the dispatch, so
  enhanced-fors cost `0` and nested enhanced-fors did not compound.
  The match arm now includes `EnhancedForStatement` alongside
  `ForStatement`, so enhanced-fors add `1 + nesting` like every
  other loop. Cross-language audit also locked in regression tests
  for JS / Mozjs / TypeScript / TSX `for...of`, which the upstream
  grammars fold into the same `for_in_statement` node as `for...in`
  and were therefore already scored correctly
  ([#178](https://github.com/dekobon/big-code-analysis/issues/178)).

## [1.0.0] - 2026-05-10

> **Fork-anchor note.** Forked from Mozilla's
> [`rust-code-analysis`](https://github.com/mozilla/rust-code-analysis)
> at commit `007ee15` on 2026-04-26 and renamed to `big-code-analysis`.
> This entry consolidates all changes through the first
> public release; there were no intermediate tagged releases between
> the fork point and `1.0.0`.

### Added

#### New languages

- **Bash** — full Checker / Getter / Alterator and metric implementations.
- **C#** — full implementation with Java-parity test coverage, including
  shebang-free detection and aliased-`kind_id` variant handling.
- **Lua** — full implementation.
- **Perl** — full implementation with metrics.
- **PHP** — full implementation with per-metric test matrix at Java parity
  and integration-suite wiring into the `big-code-analysis-output` submodule.
- **Tcl** — full implementation.
- **Kotlin / Go** — promoted from default `implement_metric_trait!` stubs
  to real per-language metric implementations. Kotlin gained Checker,
  Getter, and all seven metric traits; Go gained a real Cognitive
  complexity implementation. Both languages parsed pre-fork but emitted
  default/no-op metric values.

#### New metrics and metric variants

- Per-function **Tokens** metric with markdown-report column wiring.
- **Modified cyclomatic complexity** exposed alongside the standard count
  for languages that distinguish bare-wildcard / fall-through arms.

#### CLI (`bca`)

- New `check` subcommand with a threshold engine for CI gates
  (per-metric ceilings, exit-code-driven).
- CLI restructured into **subcommand verbs** **(breaking)** — e.g.
  `bca metrics`, `bca check`, `bca find`, `bca count`. Old top-level
  flag invocations no longer work; see the migration notes in
  `big-code-analysis-book/`.
- `--list-metrics` command to enumerate every metric the binary supports.
- `-O markdown` aggregated hotspot report with `--top` and
  `--strip-prefix` flags, padded for plain-text readability.
- `-O html` aggregated hotspot report (separate from the per-file HTML
  output): hover tooltips on aggregate headers, per-language section
  tinting with a stable palette.
- Gitignore-aware path traversal and `--paths-from <file>` for piping
  pre-computed file lists.
- Mutually exclusive action flags enforced via clap `ArgGroup` so
  conflicting modes fail at parse time.
- Auto-skip files marked as generated (e.g. `@generated` headers).
- Shebang-based language detection for extensionless scripts.

#### Output formats

- **CSV** output.
- **Checkstyle XML** output (with reusable `OffenderRecord` stub).
- **SARIF 2.1.0** output for GitHub Code Scanning ingestion.
- **Clang/GCC** and **MSVC** warning-line output formats for editor /
  CI integration.
- **Self-contained HTML** per-file report.

#### REST API (`bca-web`)

- Synchronous parsing offloaded to the blocking thread pool so the
  async runtime stays responsive under load.
- Bounded tracking of orphaned blocking tasks; new requests are
  rejected with a clear status when the threshold is exceeded.
- HTTP 500 responses now sanitise internal error details before
  emission.

#### Tooling and CI

- Makefile-based developer and CI gate (`make pre-commit`,
  `make ci`); install targets built with `target-cpu=native`.
- Workspace builds the CLI and web crates by default
  (no opt-in feature flag required).
- Per-PR snapshot-anchor lint
  (`check-snapshot-anchors.py` + `.github/workflows/snapshot-anchors.yml`)
  enforced via baseline file `.snapshot-anchor-baseline.txt`.
- Scheduled `cargo-mutants` job over `src/metrics/`,
  `src/checker.rs`, and `src/getter.rs` (quarterly cron;
  auto-files GitHub issues on escapes).
- CI lint blocking *new* bare insta snapshots in `src/metrics/`.

#### Documentation

- mdBook documentation tree at `big-code-analysis-book/` with
  Recipes section, file/language detection workflow, per-output-format
  chapters, and a developer guide for adding new languages.
- `add-lang` skill under `.claude/skills/` codifying the end-to-end
  workflow for wiring a new tree-sitter language.
- Lessons learned 9–14 added to
  `docs/development/lessons_learned.md`.

### Changed

- **Project renamed** from `rust-code-analysis` to `big-code-analysis`
  (fork anchor `007ee15`).
- **Binaries renamed** **(breaking)** to `bca` (formerly
  `rust-code-analysis-cli`) and `bca-web` (formerly
  `rust-code-analysis-web`). Distribution package names follow.
- Default branch renamed from `master` to `main`.
- Integration-snapshot submodule renamed from `tests/repositories/rca-output`
  to `tests/repositories/big-code-analysis-output`
  (remote: `dekobon/big-code-analysis-output`).
- `tree-sitter` bumped to `0.26.8` (with `Node::child(u32)` signature
  adaptation in our wrapper).
- CLI `Format` enum replaced with clap `ValueEnum` derivation —
  `-O` / `--format` accepts the same values, but error messages and
  shell completions are now generated from the type.
- Output writers consolidated under a single dispatch path; HTML
  per-function metrics format folded into the unified writer set.
- `FindCfg` / `CountCfg` filter lists now stored as `Arc<[String]>`
  **(breaking, library-level)** for cheaper cloning; downstream
  callers constructing these structs by hand must wrap their
  `Vec<String>` accordingly. `bca`'s CLI internals also moved to
  `Arc<[String]>` for find/count filters.
- `FuncSpace` and `Ops` now carry a `name_was_lossy` flag so callers
  can detect when a non-UTF-8 path component was lossily converted
  for display.
- Internal cleanup: numerous `refactor:`, `chore:`, and `style:`
  commits across the workspace tightened visibility, removed dead
  code, consolidated test helpers, modernised Rust syntax, and
  bumped internal-only dependencies (e.g. `askama` 0.15 → 0.16 in
  the `enums/` codegen helper crate, which is excluded from the
  default workspace). See
  `git log 007ee15..HEAD --grep '^refactor\|^chore\|^style\|^build(deps)'`
  for the full list.

### Fixed

#### Metrics

- **Cognitive**: handle unary negation in Kotlin and Go boolean
  sequences; exclude `else` arms of Kotlin `when` expressions from
  cognitive complexity; correct sibling boolean-sequence detection;
  generalise depth-stop tracking with correct per-language
  boundaries; implement `is_else_if` for Java and C# to fix
  `else if` over-counting.
- **Cyclomatic**: skip bare wildcard `_ =>` arms in Rust standard
  CCN; remove the spurious `CaseStatement` container increment
  in Bash standard CCN.
- **Nargs**: count bare-identifier arrow-function parameters in
  JS/TS; correct Java and Kotlin argument counting.
- **Nom**: add missing comma separators in the `Stats` `Display`
  implementation.
- **Loc**: wrap parses with a synthetic `Unit` space when the
  grammar's root node is not `Unit` (e.g. languages whose root is
  `program` / `module`).
- **C#**: match all aliased `kind_id` variants so that aliased
  syntax doesn't silently fall through.

#### Output

- `dump_metrics` now uses `cognitive_sum` / `cyclomatic_sum`
  (was previously emitting per-function values where sums were
  expected).
- Eliminated panic paths in the alterator + output pipeline; added
  regression tests.
- Flattened the `String2` variant in JS / TS / TSX alterators so
  template-literal substrings serialise consistently.

#### Web (`bca-web`)

- Comment-stripping handler swaps the C++ grammar to `Ccomment`
  (matches the CLI's behaviour for plain-text comment removal).
- Explicit `serde` `derive` feature flag enabled (was relying on
  transitive activation).

#### Robustness

- Normalise CR and CRLF line endings before parsing
  (previously, lone-CR and CRLF inputs could drift line counts).
- Walk the **parent's** children in `Node::has_sibling`
  (was walking the wrong node and missing siblings).
- Spaces: handle non-UTF-8 paths via lossy conversion when
  computing the top-level space name.
- CLI: trim whitespace from `--paths-from` lines before
  `PathBuf` construction; warn instead of silently dropping when
  non-UTF-8 path components appear in `handle_path`.
- C macro lookup: switched to `binary_search` with short-circuit
  `||` for hit-path branch prediction; dropped the static
  `DOLLARS` buffer to avoid a panic on long identifiers.

#### Build / scripts / documentation

- `ops.rs` — removed stray `println!` debug output.
- `loc.rs` — fixed `cloc_min` / `cloc_max` doc comments that
  said `Ploc` instead of `Cloc`.
- `WebCommentResponse.code` doc comment corrected.
- `enums/` build script — regenerate language enums after
  grammar version bumps.
- `split-minimal-tests.py` — use a raw f-string so regex
  metacharacters in metric names aren't misinterpreted; escape
  `metric_name` before regex interpolation.
- Cargo `repository` URLs updated to reference the `main` branch.

### Removed

- HTML per-function metrics output format
  (`refactor(output): remove HTML metrics output format`,
  commit `eb57500`). HTML output remains available via the new
  self-contained per-file HTML report (commit `7af09d1`) and the
  aggregated hotspot HTML report (commit `5eb41fd`); migrate
  depending on whether you want per-file or cross-file output.

### Security

- **`bca-web`** error sanitisation: HTTP 500 responses no longer
  leak internal error details (`fix(web): sanitize internal error
  details from HTTP 500 responses`, commit `99a2691`).
- **`bca-web`** orphan-task tracking: bounded tracking of orphaned
  blocking tasks rejects new requests when a configurable threshold
  is exceeded, mitigating slow-loris-style resource exhaustion of
  the blocking thread pool (`fix(web): track orphaned blocking
  tasks and reject when threshold exceeded`, commit `94c8141`).

<!-- Release-cutter: when a new vX.Y.Z tag is created, retarget the
[Unreleased] link to `vX.Y.Z...HEAD` and add a `[X.Y.Z]:` line
pointing at `<prev-tag>...vX.Y.Z`. -->

[Unreleased]: https://github.com/dekobon/big-code-analysis/compare/v2.1.0...HEAD
[2.1.0]: https://github.com/dekobon/big-code-analysis/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/dekobon/big-code-analysis/compare/v1.1.0...v2.0.0
[1.1.0]: https://github.com/dekobon/big-code-analysis/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/dekobon/big-code-analysis/compare/007ee15...v1.0.0
