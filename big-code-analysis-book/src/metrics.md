# Supported Code Metrics

This chapter is a guided tour of every metric that **big-code-analysis**
computes. Each section starts from the original research paper, walks
through the algorithm, and explains both the way the metric was
*originally* meant to be used and the ways the industry has actually
ended up using it years later. If you are new to software metrics, read
the sections in order — the later metrics (Maintainability Index in
particular) are explicitly built on top of the earlier ones (Halstead,
Cyclomatic, LOC).

A few framing notes before we start:

- **A metric is a measurement, not a verdict.** Every number on this
  page summarises a structural property of source code. None of them
  measures correctness, productivity, or developer skill. The most
  important question for any metric is always "compared with what?" —
  the same module, a month ago; this module versus its siblings; this
  codebase versus an industry baseline. Absolute thresholds are
  rough heuristics at best.
- **Most metrics here are computed at three scopes**: per *function /
  method*, per *class or unit-like space*, and per *file*. The
  underlying tree-sitter parser produces a tree of "spaces" (functions,
  closures, classes, namespaces, …) and every metric is rolled up
  through that tree.
- **Object-oriented metrics only fire on object-oriented constructs.**
  WMC, NPA, and NPM report `0` on a Rust file that has no `impl`
  blocks or on a Python module without classes; that is the correct
  answer, not a bug.

## Index

| Metric | Measures | First defined by |
|--------|----------|------------------|
| [ABC](#abc) | Size as `<Assignments, Branches, Conditions>` | Fitzpatrick, 1997 |
| [Cognitive Complexity](#cognitive-complexity) | How hard a function is to *read* | Campbell / SonarSource, 2017 |
| [Cyclomatic Complexity (CC)](#cyclomatic-complexity-cc) | Independent paths through a function | McCabe, 1976 |
| [Halstead](#halstead) | Vocabulary-based size, difficulty, effort, bugs | Halstead, 1977 |
| [Lines of Code (SLOC, PLOC, LLOC, CLOC, BLANK)](#lines-of-code) | Raw, physical, logical, comment, and blank line counts | Conte, Dunsmore & Shen, 1986 |
| [Maintainability Index (MI)](#maintainability-index-mi) | Composite maintainability score | Oman & Hagemeister, 1992; Coleman *et al.*, 1994 |
| [NArgs](#nargs) | Number of arguments per function | folk metric |
| [NExits](#nexits) | Number of exit points per function | structured-programming literature |
| [NOM](#nom) | Number of methods and closures | Lorenz & Kidd, 1994 |
| [NPA](#npa) | Number of public attributes | Lorenz & Kidd, 1994 |
| [NPM](#npm) | Number of public methods | Lorenz & Kidd, 1994 |
| [Tokens](#tokens) | Tree-sitter leaf-token count (size proxy) | Lizard tool, Terry Yin |
| [WMC](#wmc) | Sum of cyclomatic complexity across a class's methods | Chidamber & Kemerer, 1994 |

## ABC {#abc}

The **ABC** metric measures the size of a piece of code as a
three-dimensional vector. Each component counts one kind of operation:

- **A**ssignments — anything that stores a value into a variable,
  including compound assignments (`+=`, `++`) and explicit
  initialisation.
- **B**ranches — function and method *calls*. Despite the name, this
  is not the count of conditional jumps; it is the number of points
  where control branches out to other code.
- **C**onditions — boolean tests: comparison operators (`==`, `!=`,
  `<=`, `>=`, `<`, `>`), ternary operators (`?`), and the fixed
  keyword set (`else`, `case`, `try`, `catch`). The `default` /
  wildcard arm is **not** counted in any language (see the
  per-language deviations below). The
  short-circuit logical operators `&&` and `||` are **not**
  counted on their own — instead, each non-comparison operand of
  a `&&` / `||` chain contributes one condition via Fitzpatrick's
  "unary conditional expression" rule. The next subsection walks
  through the rules, the per-language deviations, and worked
  examples.

The metric was introduced by Jerry Fitzpatrick in the 1997 C++ Report
article *Applying the ABC metric to C, C++ and Java*. The current
canonical specification, including the rules for what counts as an
*A*, *B*, or *C* in modern languages, is maintained on Fitzpatrick's
[Software Renovation](https://www.softwarerenovation.com/Articles.aspx)
site.

### Counting rules

Fitzpatrick's paper enumerates the rules in three figures — Figure
2 (C), Figure 3 (C++, which extends Figure 2), and Figure 4 (Java).
Big-code-analysis implements those rule sets directly per language;
the table below summarises what counts in each component, with
each row attributed to the figure that introduces it.

#### Assignments

| Rule | Counted as `A` | First defined in |
|------|----------------|------------------|
| Plain assignment (`=`) | one per occurrence | Figure 2 (C) |
| Compound assignment (`+=`, `-=`, `*=`, `/=`, `%=`, `<<=`, `>>=`, `&=`, `\|=`, `^=`) | one per occurrence | Figure 2 (C) / Figure 4 (Java) |
| Java unsigned-right-shift-assign (`>>>=`) | one per occurrence | Figure 4 (Java) |
| Pre- or post-increment / decrement (`++`, `--`) | one per occurrence | Figure 2 (C) |
| Initializing constructor invocation | one per occurrence | Figure 3 (C++) |

#### Branches

| Rule | Counted as `B` | First defined in |
|------|----------------|------------------|
| Function or method call | one per call site | Figure 2 (C) / Figure 4 (Java) |
| `new` operator | one per occurrence | Figure 3 (C++) / Figure 4 (Java) |
| `delete` operator | one per occurrence | Figure 3 (C++) |
| `goto label`, `break label`, `continue label` | one per occurrence | Figure 2 (C) / Figure 3 (C++) / Figure 4 (Java, labeled `break` / `continue` only — Java has no `goto`) |

#### Conditions

| Rule | Counted as `C` | First defined in |
|------|----------------|------------------|
| Comparison operator (`==`, `!=`, `<=`, `>=`, `<`, `>`) | one per occurrence | Figure 2, Rule 5 |
| Ternary `? :` | one per occurrence | Figure 2, Rule 5 |
| `else`, `case` | one per occurrence | Figure 2, Rule 5 |
| Preprocessor `#else`, `#elif` | one per occurrence | Figure 2, Rule 5 |
| `try`, `catch` | one per occurrence | Figure 3 (C++) / Figure 4 (Java) |
| Unary conditional expression | one per non-comparison operand of `&&` / `\|\|` (and per `!`-wrapped or bare-truthy condition in `if` / `while` / argument / `return` slots) | Figure 3, Rule 7 / Figure 4, Rule 9 |

The short-circuit logical operators (`&&`, `||`, and per-language
equivalents — Ruby `and` / `or`, Python `and` / `or`, Perl `and` /
`or` / `xor`, Lua `and` / `or`, Tcl `&&` / `||`, iRules `&&` / `||` /
`and` / `or`) do **not**
contribute a condition on their own. Each non-comparison operand
contributes one instead, via the unary-conditional rule. The
paper makes this explicit twice:

1. **Listing 2** annotates `(am >= 0 && am <= 0xF) ? '/' : 'C'` as
   `accc` — one assignment plus three conditions, where the three
   conditions are the two comparisons (`>=`, `<=`) and the
   ternary (`?`). The `&&` itself contributes zero.
2. **Rule 7 / Rule 9** instead counts each operand: for
   `if (x || y) printf("test failure\n");` the paper writes "there
   are two unary conditions since both `x` and `y` are tested as
   conditional expressions". The `||` again contributes zero; `x`
   and `y` each contribute one.

#### Per-language deviations

Per-language `impl Abc` blocks narrow the paper rule set where the
language has no equivalent construct, or where strict literal
application would over-count.

| Language | Deviation | Reason |
|----------|-----------|--------|
| C, Go, Rust | `try` / `catch` omitted | No `try`/`catch` keyword in the grammar; error-handling uses `errno` / `Result` / `Result`-like sums. |
| Ruby | `Rescue` substitutes for `catch` | Ruby's exception-handling keyword is `rescue`; the AST node `Rescue` plays the role of Java's `catch`. |
| All languages | `default` / `_` wildcard arm excluded from the condition set | Fitzpatrick's Figure 2 lists `default`, but it falls through unconditionally — counting it would inflate `C` on every `switch` / `match` regardless of body. big-code-analysis omits it for every language (the Rust `_ =>` and Java `default:` arms included). |
| Tcl | Chain-operand, bare-truthy and ternary slots wired; argument and `return` slots are not | Each operand of a `&&` / `\|\|` chain inside `expr {…}` counts as one condition, so `if {$a && $b}` reports two. `if` / `elseif` / `while` route their `expr {…}` predicate, so a bare-truthy `if {$a}` reports one and `if {!$a}` likewise — matching C's `if (a)` (#1180). A predicate written as a command substitution is a truthy test of that command's result, so `if {[somecmd]}` also reports one, and the redundant `if {[expr {$a \|\| $b}]}` idiom reports three (the chain's two operands plus the substitution). The argument and `return` slots remain unrouted: a negation reached only through a standalone `expr {…}` command still reports zero. |
| iRules | Chain-operand, bare-truthy and ternary slots wired; argument and `return` slots are not | Each operand of a `&&` / `\|\|` / `and` / `or` chain counts as one condition (Rule 9), so `if {!$a && !$b}` reports two. iRules also recognises the word-form string-match comparators (`contains`, `starts_with`, `ends_with`, `equals`, `matches`, …) that Tcl lacks (Tcl's `eq` / `ne` / `in` / `ni` are shared). Bare-truthy and ternary slot routing matches its Tcl sibling exactly (#1180) — see that row for the shared detail, including the argument and `return` slots that remain unrouted. |
| All Phase 2 languages (Java, Groovy, C#, Rust, Go, JavaScript, TypeScript, TSX, Mozjs, PHP, C, C++, Objective-C, Mozcpp, Python, Perl, Lua) | `if (true) {}`, `m(!a, !b)`, `return !x` count their operand(s) | Phase 2B routes `if` / `while` / `do-while` / argument-list / `return` slots through the same walker, so the rule applies uniformly across decision-bearing positions. A bare `return x` continues to report zero — Fitzpatrick treats an identifier in a return slot as a value, not a unary conditional. |
| Ternary slots: Java, Groovy, C#, C, C++, Objective-C, Mozcpp, JavaScript, TypeScript, TSX, Mozjs, PHP, Perl, Ruby, Python, Tcl, iRules | `a ? !b : !c` counts its condition and both branch operands | The same walker also runs over a ternary's three operand slots, so `a ? !b : !c` reports 4 (the `?` plus three unary conditions) rather than 1. Tcl and iRules reach the same 4 without grammar fields: their `ternary_expr` exposes none, and `_expr` inlines `( … )`, so the slots are located relative to the `?` and `:` tokens instead of by index (#1180). Python arrives at the same total by a different route: `not` operands are counted by the `NotOperator` rule wherever they appear, so only the condition slot is routed through the walker and `(not b) if a else (not c)` likewise reports 4. Languages with no ternary (Rust, Go, Kotlin, Lua, Elixir) are unaffected. |
| Ruby | Bare-predicate `if` / `unless` / `while` / `until` (block and modifier forms) count one condition | Idiomatic Ruby favours bare predicates (`if flag`, `x if flag`); counting the condition slot keeps ABC conditions at or above Ruby's cyclomatic decision count (the alignment enforced across the other languages). A comparison (`if a == b`) or `&&` / `\|\|` chain in the predicate is counted by its own operator / walker arm and is not double-counted. |
| Bash | `if` / `elif` / `while` and each non-wildcard `case` arm count one condition | A Bash predicate is a *command*, so the branch keyword itself — not an embedded boolean expression — is the condition signal. Each matches a Bash cyclomatic decision; the bare `*)` case arm (the analogue of `default:`) is excluded, mirroring the cyclomatic standard count. The arithmetic ternary `$(( a ? b : c ))` therefore contributes nothing: it carries no branch keyword, so it falls outside the rule set rather than through a gap in it. |
| Kotlin | `try` counts a condition alongside `catch` | Fitzpatrick counts both keywords, and Java / C# / C++ / Groovy already count both; Kotlin previously counted only the catch block. |
| Java, Groovy, C#, TypeScript, TSX | A `?` used as type syntax is not a ternary | In each of these grammars the ternary `?` and the type-syntax `?` are the *same* anonymous token, so the ternary rule above is gated on the token's parent. Java and Groovy exclude the wildcard bound `List<? extends T>` (#1274); C# excludes the nullable type `int? x` and the constraint `where T : class?`; TypeScript and TSX exclude optional parameters, properties, methods, class fields and tuple elements, and conditional types (`T extends U ? X : Y` — resolved by the type checker and erased before runtime, so no more a branch than the `<` / `>` already excluded) (#1275). Safe navigation is untouched: C#'s `a?.b` shares the same token and still counts, while the other languages spell theirs as a distinct one. |

#### Worked example

Consider this C function:

```c
char digit_or_C(int am) {
    char c;
    if (am >= 0 && am <= 0xF) {
        c = '/';
    } else {
        c = 'C';
    }
    return c;
}
```

Walking the function body:

| Token / construct | Component | Why |
|-------------------|-----------|-----|
| `am >= 0` | C += 1 | Comparison (Rule 5, `>=`) |
| `am <= 0xF` | C += 1 | Comparison (Rule 5, `<=`) |
| `&&` | — | Logical operator — does not contribute on its own. |
| `if`/`else` | C += 1 | `else` keyword (Rule 5) |
| `c = '/'` | A += 1 | Assignment (Rule 1) |
| `c = 'C'` | A += 1 | Assignment (Rule 1) |

Total: `<A,B,C> = <2, 0, 3>`, magnitude `√13 ≈ 3.61`.

If the same body is rewritten with a unary conditional —

```c
if (am_in_range || force_letter) {
    c = 'C';
}
```

the walker counts `am_in_range` and `force_letter` once each
(Rule 7 / 9 unary conditional). The `||` operator itself
contributes zero. This matches Fitzpatrick's Rule 7 / 9 worked
example in the paper: `if (x || y) printf("test failure\n");`
"there are two unary conditions since both x and y are tested as
conditional expressions."

#### Comparison with other ABC tools

The project follows Fitzpatrick's original paper for `&&` / `||`:
the operator does not count; each non-comparison operand counts
once as a unary conditional. This deviates from
[RuboCop's `Metrics/AbcSize`](https://docs.rubocop.org/rubocop/cops_metrics.html#metricsabcsize)
(which counts `and` / `or` directly) and matches
[`StepicOrg/abcmeter`](https://github.com/StepicOrg/abcmeter) and
[`eoinnoble/python-abc`](https://github.com/eoinnoble/python-abc).
When comparing ABC numbers across tools, the operator-counting
choice is the single biggest source of disagreement on the same
source.

### Algorithm

The implementation walks every leaf node of the syntax tree exactly
once. For every node it asks the language's per-language `Abc` trait
implementation three yes/no questions: *is this an assignment? a
branch? a condition?* — and increments the matching counter. The
four headline values are:

- the three components themselves, `assignments`, `branches`,
  `conditions`;
- the **magnitude** `|<A,B,C>| = √(A² + B² + C²)`, which is the way
  Fitzpatrick recommends summarising the vector as a single number.

The full serialised output (`src/metrics/abc.rs`) emits these four
together with `value` (the per-space magnitude the CLI thresholds
against, which equals `magnitude` at a leaf space), the per-component
averages (`assignments_average`, `branches_average`,
`conditions_average`), and per-component `*_min` / `*_max` at the file
scope, for fourteen fields total. The metric is specialised per
language in `src/languages/language_*.rs`.

### How to read it

ABC is a *size* metric, not a complexity metric — a long, dull
function with no decisions still scores high if it does a lot of
assignments. Fitzpatrick's original recommendation was to use the
magnitude as a relative ruler: rank a file's functions by ABC
magnitude and look at the top decile.

In practice ABC ended up being most widely adopted by the Ruby
community, where the [`rubocop` linter](https://rubocop.org/) and the
[`flog` tool](https://github.com/seattlerb/flog) both default to
threshold-based warnings. A Ruby method with an ABC magnitude over
about 17 is conventionally a refactoring candidate; over 30 is
considered hard to maintain. Those thresholds are language-specific —
expect higher values in C++ and Java, which use explicit getter/setter
assignments more aggressively.

## Cognitive Complexity {#cognitive-complexity}

**Cognitive Complexity** was introduced by G. Ann Campbell at
SonarSource in the 2017 white paper *Cognitive Complexity — A new way
of measuring understandability* and the follow-up IEEE TechDebt 2018
paper [*Cognitive Complexity — An Overview and
Evaluation*](https://ieeexplore.ieee.org/document/8595102/). The
white paper itself is available as
[`CognitiveComplexity.pdf`](https://www.sonarsource.com/docs/CognitiveComplexity.pdf)
on the SonarSource site.

The metric was designed as a deliberate replacement for Cyclomatic
Complexity in code-quality tooling. The argument Campbell makes is
that cyclomatic complexity measures how hard code is to *test*, not
how hard it is to *understand*: a 1024-arm `switch` statement scores
the same as a deeply nested chain of `if`s that perform identical
logic, yet a human reader has a much harder time following the
nested code.

### Algorithm

Cognitive Complexity starts at zero and applies three rules as it
walks the tree:

1. **Ignore "shorthand" control flow.** Constructs that simply route
   to a single block — a top-level `if` with no nesting, an `else`
   without conditions of its own, the head of a `for`, a `?:` ternary
   — add a baseline `+1` each, but they do not punish you for the
   pattern.
2. **Penalise breaks in linear flow.** Every `if`, `else if`, `else`,
   `switch`, `try`/`catch`, loop, jump (`goto`, `break label`,
   `continue label`), and recursive call adds at least `+1`.
3. **Punish nesting.** Every time control flow appears *inside* an
   already-nested block, the metric adds an extra `+1` *per level of
   nesting*. An `if` inside a `for` inside an outer `if` inside a
   method scores `1 + 2 + 3 = 6`, where a flat sequence of the same
   three constructs would have scored `1 + 1 + 1 = 3`.

Sequences of identical boolean operators (`a && b && c`) score `+1`
for the whole run, on the grounds that a chain of `&&`s is no harder
to read than a single `&&`. Switching operators (`a && b || c`) is
where the cognitive load jumps, so the second operator earns its own
`+1`.

big-code-analysis exports the per-function structural score along
with the file-wide `sum`, `min`, `max`, and a per-function `average`.
The implementation is in `src/metrics/cognitive.rs`.

### How to read it

A Cognitive Complexity of `0` means the function is purely linear; no
branches, no loops. SonarSource's tooling defaults to flagging
functions above `15` as "too complex" and Campbell's recommendation
in the white paper is that a function should rarely exceed about
`25`. Unlike Cyclomatic Complexity, the metric scales smoothly:
deeply nested code with the same number of decisions scores
significantly higher than flat code with the same decisions.

The emergent use case is **refactoring guidance during code review**:
because the metric penalises nesting specifically, it tends to flag
exactly the kind of function that benefits from an early-return or
"extract method" refactor. SonarLint's IDE plugins (IntelliJ, VS
Code, Visual Studio, Eclipse) all surface it as the headline
complexity number on hover, and the metric has since been picked up
by several language servers and code-review platforms outside the
Sonar ecosystem.

### Per-language deviations

- **Elixir** does not score recursion or jump statements. Elixir
  control flow (`if` / `unless` / `cond` / `case` / `with`) is built
  from macro-shaped `Call` nodes rather than dedicated grammar
  productions, and the language has no `break` / `continue` /
  `goto`; the implementation therefore scores only the
  nesting-bearing constructs it can identify and omits the
  recursion (B3) and unstructured-jump (B2) increments that the
  SonarSource specification adds for languages that expose those
  shapes syntactically.
- For every language with a syntactic function-definition node, a
  nested function — a local function, or a method on a local /
  inner class — **resets the nesting counter to zero** at its
  boundary and adds a function-depth surcharge, so control flow
  inside it is scored against the nested function's own depth rather
  than the enclosing function's nesting. Byte-equivalent constructs
  therefore score identically across languages.
- A lambda or closure (`x -> …`, `|x| …`, `lambda x: …`, a Ruby
  block, an Objective-C block) is not a function boundary in that
  sense. It adds a surcharge *on top of* the enclosing nesting
  instead of replacing it, so a decision inside a lambda written
  inside an `if` is charged for both.
- That surcharge stops at the next function boundary. A function
  *declared inside* a closure body opens a fresh lexical scope, so it
  does not inherit the closure's surcharge — `fn g` inside a `|| { … }`
  scores what it would score outside one. Until #1187 only the
  JavaScript family applied this, so the same body scored differently
  in Rust, Java, C++, PHP and C# depending on whether something two
  levels up happened to be a closure. Python cannot express the shape
  at all: a `def` is a statement and a lambda body is a single
  expression.
- **Python** charges a boolean operator an extra `+1` for each
  enclosing `lambda`, on top of the `+1` the boolean sequence itself
  earns. No other language does this. Only the outermost operator
  inside a given lambda body pays the surcharge, and the walk that
  counts those lambdas stops at the nearest enclosing
  `expression_list`. (It also stops at `if`, `for` and `while`, but a
  lambda body is a single expression, so no lambda ever sits above one
  of those statements and those three arms never change a count.) The
  sequence increments themselves still follow the operator-switch rule
  above, so a mixed chain inside one lambda pays two of them plus a
  single surcharge. Measured:

  | Python source | `cognitive.sum` |
  | --- | --- |
  | `g = a and b` | 1 |
  | `g = lambda y: y and a` | 2 |
  | `g = lambda y: y and a and b` | 2 |
  | `g = lambda y: y and a or b` | 3 |
  | `g = lambda x: (lambda y: y and a)` | 3 |
  | `k = lambda q: (yield a and b, c)` | 1 |
  | `def f(a): g = lambda x: x` | 0 |

  The last two rows are the boundaries. The parenthesised `yield` puts
  an `expression_list` between the operator and the `lambda`, ending
  the walk before it reaches one, so only the fundamental `+1` is left;
  and a lambda containing no boolean operator costs nothing by itself.

  Campbell gives a boolean sequence a fundamental increment and no
  nesting increment, so this is an addition to the specification rather
  than an implementation of it. Issue #1150 reviewed the rule and kept
  it deliberately. Python's boolean-operator cost is not comparable
  with another language's score for the same code.

## Cyclomatic Complexity (CC) {#cyclomatic-complexity-cc}

The original software complexity metric, introduced by Thomas J.
McCabe in 1976 in [*A Complexity
Measure*](https://www.literateprogramming.com/mccabe.pdf) (IEEE
Transactions on Software Engineering, SE-2(4), pages 308–320).

McCabe's idea was to apply graph theory to the *control-flow graph*
of a function. If you draw every basic block as a node and every jump
between blocks as an edge, the cyclomatic number of that graph is

```text
M = E − N + 2P
```

where `E` is the number of edges, `N` the number of nodes, and `P`
the number of connected components. Crucially, `M` is also exactly
the number of **linearly independent paths** through the function —
in other words, the minimum number of test cases needed to cover
every branch at least once.

### Algorithm

big-code-analysis does not literally build a control-flow graph.
Instead it uses the equivalent, much cheaper, formulation McCabe
proved in the 1976 paper for structured programs:

> *Cyclomatic Complexity = 1 + (number of decision points)*

A "decision point" is any node where control can branch:

- `if`, `else if`, ternary `?:`
- `case` / `when` arms in `switch` / `match` / `select`
- `while`, `do … while`, every variant of `for`
- exception-handler `catch` clauses
- short-circuit boolean operators `&&` and `||`

The per-language `Cyclomatic` trait, in `src/metrics/cyclomatic.rs`,
asks each tree-sitter node "are you a decision?" and increments the
counter. The metric is rolled up per function and per file; per-class
aggregation across method bodies is provided separately by
[WMC](#wmc) below.

### Modified cyclomatic

big-code-analysis also reports a **modified** variant that collapses
all `case` / `match` / `when` arms inside a *single* switch
statement into one decision point, regardless of how many arms it
has. This tends to undercount big dispatch tables in a way that
often matches developer intuition better than the strict McCabe
definition — a 30-arm `enum` dispatch reads as one decision, not
thirty. (The convention itself is not original to this project: it
echoes the long-standing `-m` mode from Terry Yin's
[lizard](https://github.com/terryyin/lizard) tool, which is where
many readers will first have seen it.) Both numbers are exported
side by side; pick one and be consistent.

### Counting Rust's `?` operator

By default Rust's `?` operator (the `try_expression` grammar node)
adds `+1` to both standard and modified cyclomatic, matching upstream
rust-code-analysis: `?` is an early-return branch. When cyclomatic is
used as a maintainability *gate*, this can over-penalize linear-but-
fallible code that threads a handful of `?` through a happy path. You
can opt out so `?` is treated as linear error propagation:

- Library: `MetricsOptions::default().with_count_cyclomatic_try(false)`.
- CLI: `--cyclomatic-count-try=false` (or the deprecated
  `--no-cyclomatic-try` alias), or `cyclomatic_count_try = false` in
  `bca.toml` (the CLI value overrides the key in either direction).
- A repo gate: set `cyclomatic_count_try = false` in the
  auto-discovered `bca.toml` (this project's own `make self-scan` does
  exactly this — no flag or env var). Toggling the policy shifts
  cyclomatic values, so regenerate `.bca-baseline.toml` in the same
  change.

The default is unchanged — `?` keeps counting — so published metric
values are preserved. The toggle is Rust-only; no other language emits
`try_expression`.

### How to read it

McCabe's original recommendation, repeated in the 1976 paper and
preserved by [NIST's *Structured Testing*
report](https://www.nist.gov/publications/structured-testing-testing-methodology-using-cyclomatic-complexity-metric) (Special
Publication 500-235, 1996), is to treat `10` as the upper bound for a
single function: above that, the number of test cases needed for
branch coverage grows uncomfortably large.

The emergent uses of cyclomatic complexity have been:

1. **Defect prediction.** Complexity correlates well — though
   imperfectly — with the *probability* of a function containing a
   bug, and most static-analysis tools flag high-CC functions as risky.
2. **Test-coverage planning.** CC is the lower bound on the number
   of test cases needed to cover every branch, so test teams use it
   directly to budget effort.
3. **Refactor triage.** Cyclomatic Complexity is the headline
   "complexity" number in almost every code-quality dashboard,
   often as a tie-breaker between two functions that look similar
   in length.

Be aware of the metric's well-known blind spot: it treats every
decision as equal weight. A 30-arm `switch` over an enum and a
function with two nested `if`s each containing nested `if`s both
score around 30, even though they are very different reading
experiences. Cognitive Complexity (above) was designed to fix exactly
that.

## Halstead {#halstead}

The **Halstead suite** is the oldest size-and-effort metric family on
this page. Maurice H. Halstead introduced it in his 1977 book
[*Elements of Software
Science*](https://openlibrary.org/books/OL4535482M/Elements_of_software_science)
(Elsevier, ISBN 0-444-00205-7); the
Wikipedia page on [Halstead complexity
measures](https://en.wikipedia.org/wiki/Halstead_complexity_measures)
summarises the formulas. Halstead's project was strikingly ambitious:
he wanted a quantitative, empirical *science of software* in the same
way that physics is the empirical science of matter.

### The four base counts

Halstead reduces a program to its tokens, then partitions them into
two categories:

- **Operators** — anything that *does* something: keywords (`if`,
  `return`, `while`), arithmetic and logical operators, assignment,
  function-call syntax, punctuation that controls flow.
- **Operands** — anything that *is* something: identifiers and
  literals.

From these you derive four base counts:

| JSON key | Symbol | Meaning |
|----------|--------|---------|
| `unique_operators` | n1 | number of **distinct** operators |
| `unique_operands` | n2 | number of **distinct** operands |
| `total_operators` | N1 | **total** count of operator occurrences |
| `total_operands` | N2 | **total** count of operand occurrences |

The serialized output uses the descriptive **JSON key** column; the
derived-metric formulas below use Halstead's classic `n1`/`N1`/`n2`/`N2`
notation for the same four counts.

big-code-analysis records these four numbers in
`src/metrics/halstead.rs` per function and per file. The per-language
trait classifies tokens as operator vs. operand on a token-by-token
basis; the rules deliberately exclude pure layout punctuation like
parentheses and statement separators, which is why the Halstead
totals are *not* the same as the Tokens count.

Two classification rules are worth knowing because they are choices
rather than consequences, and because several grammars spell the
tokens involved the same way they spell real operators:

- **A literal's own delimiters are not operators.** A JavaScript
  regex, a Groovy slashy string, a C++ raw string, a Tcl braced word
  and a Perl or Ruby pattern each contribute *one operand* — the
  literal — and no operator for the punctuation around it. Otherwise
  the score would move with the author's choice of delimiter, which
  says nothing about the code. One known exception, tracked in
  [#1318](https://github.com/dekobon/big-code-analysis/issues/1318):
  the Tcl grammar parses a braced literal as a *script* everywhere
  except the value slot of the handful of commands it special-cases,
  so `lappend x {a b}` still reports a `{}` operator and bills the
  words inside the braces rather than the literal.
- **A string-interpolation opener is not an operator.** `"{$x}"` in
  PHP, `"#{x}"` in Ruby and Elixir, `"${x}"` in Kotlin and Groovy and
  `$"{x}"` in C# all count the interpolated expression's own operators
  and nothing for the opener itself.

### Derived metrics

Halstead then derives a small zoo of formulas. big-code-analysis
reports all of the standard ones, plus three less-common derivations
(`estimated_program_length`, `purity_ratio`, `level`) that are part
of the original suite:

```text
vocabulary               n  = n1 + n2
length                   N  = N1 + N2
estimated_program_length N̂  = n1·log2(n1) + n2·log2(n2)
purity_ratio                = N̂ / N
volume                   V  = N · log2(n)                          (bits)
difficulty               D  = (n1 / 2) · (N2 / n2)
level                    L  = 1 / D
effort                   E  = D · V          (elementary mental discriminations)
time                     T  = E / 18                               (seconds)
bugs                     B  = E^(2/3) / 3000 (estimated delivered defects)
```

The numeric constants come from Halstead's empirical fits against a
heterogeneous corpus of CDC-era programs including FORTRAN, PL/I, and
Algol-family languages. The `T = E / 18` "Stroud number" is separate
— it comes from psychology: Halstead borrowed John Stroud's estimate
that the human mind makes about 18 elementary discriminations per
second.

### How to read it

Halstead's *original* intent was to predict three things about a
program before it was even written: how big it would be in bits,
how long it would take to implement, and how many bugs to expect in
deployment. The empirical evidence for the volume and length
predictions is reasonable; the time and bugs predictions are more
controversial and have been criticised at length, notably in the
Purdue technical report [*Software Science Revisited*](https://docs.lib.purdue.edu/cgi/viewcontent.cgi?article=1302&context=cstech).

In modern practice the Halstead numbers are used for three things:

1. As inputs into composite metrics — most importantly the
   Maintainability Index (next section), which depends on Halstead
   *volume*.
2. As a **language-independent size proxy**: volume in bits scales
   smoothly across languages in a way that LOC does not.
3. For **comparative effort budgeting**: when two refactoring
   candidates have similar cyclomatic complexity, the one with the
   higher Halstead difficulty is the one more likely to introduce
   regressions.

## Lines of Code {#lines-of-code}

This section covers the five LOC variants — SLOC, PLOC, LLOC, CLOC,
and BLANK. "Counting lines" sounds trivial until you have to define exactly
what counts. The five variants below are the de-facto standard
breakdown, going back to Samuel Conte, Hubert Dunsmore and Vincent
Shen's 1986 textbook [*Software Engineering Metrics and
Models*](https://books.google.com/books/about/Software_Engineering_Metrics_and_Models.html?id=PKlQAAAAMAAJ)
(Benjamin/Cummings, ISBN 0-8053-2162-4), which codified the
distinction between physical and logical lines. The Wikipedia entry on
[source lines of
code](https://en.wikipedia.org/wiki/Source_lines_of_code) is a
readable summary of that physical-versus-logical distinction.

| Variant | Counts |
|---------|--------|
| **SLOC** | Source Lines Of Code — every line in the file, comments, blanks, and code alike |
| **PLOC** | Physical Lines Of Code — non-blank, non-comment-only lines |
| **LLOC** | Logical Lines Of Code — statement-bearing lines (definitions, assignments, declarations) |
| **CLOC** | Comment Lines Of Code — lines that contain a comment (with or without code on the same line) |
| **BLANK** | Blank lines — whitespace-only lines |

### Algorithm

big-code-analysis derives all five counts from a single pass over the
tree-sitter syntax tree (see `src/metrics/loc.rs`). Comments and
strings are identified by their AST node type rather than by lexical
scanning, so multi-line strings, raw strings, doc comments, and
string interpolations are all handled correctly. The per-language
`Loc` trait specifies which node kinds count as a "statement" for
LLOC; this is the subtle one, because what counts as a statement is
language-defined.

The five counts satisfy a couple of useful identities:

```text
SLOC = PLOC + BLANK + (lines that are comment-only)
CLOC ≥ (lines that are comment-only)        # CLOC also counts mixed code+comment lines
```

### How to read it

- **SLOC** is what most people mean colloquially by "lines of code".
  It is the canonical size proxy, but is sensitive to formatting and
  not portable across language conventions.
- **PLOC** strips away the visual noise. It is the size measure used
  inside the Maintainability Index formula below.
- **LLOC** is the most reliable *statement* count. It is the right
  measure if you are budgeting test cases per statement, or comparing
  the density of a Python file against a Java file.
- **CLOC**, combined with PLOC, gives you a *comment density* —
  `CLOC / PLOC` is a useful rough proxy for how much of the file is
  documentation versus implementation.
- **BLANK** is mostly diagnostic: a file with very low BLANK
  proportion is often hard to read.

The emergent uses of LOC variants go well beyond raw size. They are
the most common input into cost-estimation models (COCOMO and COCOMO
II both use KSLOC — thousands of source lines — as their base unit),
they feed effort prediction in product-portfolio dashboards, and they
are used as a normalising denominator for almost every other metric:
*defects per KSLOC*, *churn per KSLOC*, *test cases per KSLOC*. The
weakness — LOC is easy to game and a 10× difference in coding style
can produce a 2× difference in LOC — is the reason this chapter has
so many other metrics in it.

## Maintainability Index (MI) {#maintainability-index-mi}

The **Maintainability Index** is a composite metric that rolls
several of the metrics above into a single 0-to-100ish number meant
to be read as "how maintainable is this code?". It was proposed by
Paul Oman and Jack Hagemeister in their 1992 ICSM paper *Metrics for
assessing a software system's maintainability* and refined by Don
Coleman, Dan Ash, Bruce Lowther, and Paul Oman in the 1994 IEEE
Computer paper [*Using metrics to evaluate software system
maintainability*](https://www.ecs.csun.edu/~rlingard/comp589/ColemanPaper.pdf)
(IEEE Computer 27(8), pages 44-49). Their methodology was empirical:
they collected expert maintainability ratings on a handful of
production Hewlett-Packard systems, computed forty candidate metrics
on each, and let regression analysis pick the best linear
combination. The combination that survived used Halstead volume,
cyclomatic complexity, lines of code, and comment density.

big-code-analysis reports the three formulas that have stuck in
practice:

The three values nest under the `mi` object as the keys `original`,
`sei`, and `visual_studio` (the dotted threshold names `mi.original`,
`mi.sei`, `mi.visual_studio`):

```text
mi.original      = 171 − 5.2·ln(HV) − 0.23·CC − 16.2·ln(SLOC)
mi.sei           = 171 − 5.2·log2(HV) − 0.23·CC − 16.2·log2(SLOC) + 50·sin(√(2.4·comment_percentage))
mi.visual_studio = max(0, mi.original · 100 / 171)
```

- `mi.original` is the Coleman–Oman formula. It can be negative for
  pathological files.
- `mi.sei` is the Software Engineering Institute's refinement, which
  adds a comment-density term — the `sin(√(...))` shape was chosen so
  that *some* comments help, but adding more after a point does not.
  `comment_percentage` is the comment-line share expressed as a
  percentage in `[0, 100]` (not a ratio in `[0, 1]`); the code feeds
  this percentage straight into the SEI term (see `src/metrics/mi.rs`
  and issue #241).
- `mi.visual_studio` is the linear rescaling Microsoft chose for
  Visual Studio, where the score is clamped to `[0, 100]` and shown
  to developers traffic-light style: green ≥ 20, yellow ≥ 10, red
  below.

The historical context, and a sharp critique of the metric, is
collected on Arie van Deursen's blog post [*Think Twice Before Using
the Maintainability
Index*](https://avandeursen.com/2014/08/29/think-twice-before-using-the-maintainability-index/).

### Algorithm

The implementation is purely arithmetic — `src/metrics/mi.rs`
consumes the already-computed `Halstead`, `Cyclomatic`, and `LOC`
metrics and applies the three formulas. Because the formulas use the
natural log of Halstead volume and SLOC, MI is undefined for empty
files; big-code-analysis returns `0.0` for any file with zero SLOC or
zero Halstead volume.

### How to read it

MI was *originally* designed as a portfolio-level score: "how much
maintenance pain should we expect from this codebase over the next
year?". It is fairly stable across releases of a healthy system and
tends to drop measurably before a system enters the "legacy"
quadrant.

The emergent use case is the **Visual Studio traffic-light rendering**:
every C# developer who has hovered a method in the IDE has seen the
green / yellow / red icon, and the underlying number is `mi.visual_studio`.
This made MI by far the most user-facing software metric for an
entire generation of .NET developers, which is also why it is the
metric that has attracted the most criticism. Treat it as a smoke
detector, not a thermostat: a sudden drop is a useful signal, but
the absolute number is noisy.

## NArgs {#nargs}

**NArgs** counts the number of arguments declared by a function,
method, or closure. The metric does not have a famous origin paper —
it is folk wisdom dating to at least Kernighan and Plauger's [*The
Elements of Programming
Style*](https://en.wikipedia.org/wiki/The_Elements_of_Programming_Style)
(1974) and prominently re-stated in Robert C. Martin's [*Clean
Code*](https://www.pearson.com/en-us/subject-catalog/p/clean-code-a-handbook-of-agile-software-craftsmanship/P200000009044/9780136083252)
(2008), which suggests three arguments as a soft ceiling.

big-code-analysis splits the count by callable kind: every aggregate
is reported separately for *functions* and *closures* so a Rust file
heavy on `|…| …` closures and a Java file with only methods produce
comparable numbers. The serialised output
(`src/metrics/nargs.rs`) is `function_args`, `closure_args`,
`function_args_average`, `closure_args_average`, `total`, `average`,
`function_args_min`, `function_args_max`, `closure_args_min`,
`closure_args_max`.
The implementation handles default arguments, variadic arguments,
keyword-only arguments, and destructured parameters consistently per
language. Comments written inside the parameter list are not
parameters, including the C++ idiom that puts one where an unused
parameter's name would go — `void f(int /*unused*/)` is one argument.

### Macro-obscured C-family declarators {#nargs-macro-declarators}

C, C++, Mozcpp and Objective-C are parsed without running the
preprocessor, so a function-like macro standing where the declared name
belongs is still there in the tree. The idiom is the JNI shim:

```c
#define RUN_STATS_METHOD(name) JNICALL Java_org_tensorflow_RunStats_##name

JNIEXPORT jlong RUN_STATS_METHOD(allocate)(JNIEnv *env, jclass clazz) { … }
```

Read as written, `RUN_STATS_METHOD(allocate)` is the declarator and
`(JNIEnv *env, jclass clazz)` belongs to the return type — one argument.
big-code-analysis reads it the other way and reports **two**, because
neither language lets a function return a function type (C11 6.7.6.3p1,
C++ `[dcl.fct]`): a declarator nested directly inside another one cannot
be a declarator chain, so the outer list is the function's own. Every
legitimate function-returning-a-function-pointer form —
`int (*fp(int a, int b))(int c)` — puts parentheses in between and is
unaffected, as is C++ `operator()`.

The space is named for the **macro**, not the function. There is no
other candidate: the real symbol is assembled by `##` token pasting and
never appears in the source. So several shims in one file share a name,
which `bca check` shows as several rows with the same label at different
lines, and which `.bca-baseline.toml` disambiguates by body hash the way
it does an overload set.

### What the threshold gate measures {#nargs-gate}

`bca check --threshold nargs=N` gates each callable on **its own**
parameter list, which is what every comparable tool measures — RuboCop
`Metrics/ParameterLists`, ESLint `max-params`, Clippy
`too_many_arguments`, lizard, SonarQube S107 and Pylint `R0913` all
count one callable at a time.

Note that this is *not* the serialized `total`. The `function_args` and
`closure_args` keys above are subtree sums: a function that declares two
parameters and contains a three-parameter nested function reports
`function_args: 5`. Before
[#1196](https://github.com/dekobon/big-code-analysis/issues/1196) the
gate read that sum, so a three-parameter function with a two-parameter
sort comparator was flagged at 5 — and the remediation its number
implied, fewer parameters, was not the one that would clear it.

Nothing escapes the narrower rule. In the twelve grammars whose closures
open their own space — Rust, JavaScript, TypeScript, TSX, MozJS, C#, Go,
PHP, Perl, Ruby, Lua and Elixir — a closure is gated on its own offender
row, which is also where its fix belongs. In Python, Java, Kotlin and
C++ a lambda opens no space, so its arguments can only be attributed to
the enclosing function; there the offender row shows the split:

```text
small: nargs = 8 (1 own + 7 lambda) (limit 5)
```

so you can tell at a glance whether the lever is the signature or the
lambda.

### Languages where it reads 0 {#nargs-language-gaps}

A metric that silently reports 0 reads as "no offenders" rather than
"not measured", so it is worth knowing where the count is inert:

- **Bash** — correct and permanent. The shell has no formal parameter
  list; arguments arrive as `$1`, `$2`, and so on. It is the only
  language where every function reads 0.
- **Perl subs without a signature** — correct. Signatures
  (`sub add($x, $y)`) are counted; a sub that reads its arguments from
  `@_` declares no formal parameters to count.
- **Perl anonymous subs** — a gap, and an upstream one: the grammar
  parses an anonymous sub's signature inside an error node, so
  `my $f = sub ($x) {…}` reads 0 even though it has a signature.

A `nargs` limit is therefore inert on a Bash codebase and sparse on
Perl written before `use v5.36`. Gate it per language rather than
repository-wide; see
[Choosing thresholds](recipes/thresholds.md#language-gaps).

### How to read it

A function with many arguments is hard to call correctly and even
harder to test exhaustively — the test matrix grows roughly
exponentially. The classic refactoring advice is the *introduce
parameter object* pattern: when a function takes more than four
related arguments, group them into a record / struct / dataclass.

The emergent use is as a **review-blocking lint rule**: most modern
linters (`pylint`'s `R0913`, ESLint's `max-params`, Checkstyle's
`ParameterNumber`) flag functions with more than a configurable
threshold. NArgs is also a useful component of API-design dashboards:
public APIs whose average NArgs has crept upward over time tend to be
ones that have accreted "just one more parameter" feature flags.

## NExits {#nexits}

**NExits** counts the number of distinct exit points from a
function — every explicit `return`, every `throw` / `raise`, and
(in Rust) every `?` early-return. The implicit fall-through return at
the end of a function is **not** counted; only explicit exits are
(see issue #243).

The metric goes back to the structured-programming literature of the
1970s, where Edsger Dijkstra and others argued that functions should
have **a single entry and a single exit point** (the "SESE" rule).
Modern thinking is much more nuanced — see Steve McConnell's
[*Code Complete*](https://www.microsoftpressstore.com/store/code-complete-9780735619678),
2nd edition (Microsoft Press, 2004), which
explicitly recommends *early returns* as a clarity-improving pattern
when they reduce nesting.

big-code-analysis walks each function's syntax tree, identifies the
language-specific exit nodes (see the per-language `Exit` trait in
`src/metrics/nexits.rs`), and reports per-function counts plus
file-level `sum`, `average`, `min`, and `max`. The serialised
field name is `nexits`, matching the prose acronym used here.

### How to read it

Strict SESE coding standards ([DO-178C](https://en.wikipedia.org/wiki/DO-178C)
for avionics, MISRA C for embedded automotive — see [MISRA's official
site](https://misra.org.uk/)) still require an NExits of 1 per
function, because multiple exit points complicate certified
control-flow analysis. Outside those domains, an NExits of `2-4` is
usually a *good* sign — it almost always means the function uses
guard clauses to handle preconditions and then proceeds in a flat
body.

A *very* high NExits — say above 8 — is the warning sign. It usually
means the function should have been split into several smaller
functions, with each "successful branch" becoming its own helper.

## NOM {#nom}

**NOM** stands for *Number Of Methods* and counts every function,
method, and closure defined inside a given scope (file, class, or
namespace). For object-oriented codebases it is one of the first
metrics introduced by Mark Lorenz and Jeff Kidd in their 1994 book
[*Object-Oriented Software
Metrics*](https://books.google.com/books/about/Object_oriented_Software_Metrics.html?id=lsJnQgAACAAJ)
(Prentice Hall, ISBN 0-13-179292-X), where it is treated as the
primary class-size indicator.

big-code-analysis reports the count split by callable kind in
`src/metrics/nom.rs`. The serialised fields are `functions`,
`closures`, `functions_average`, `closures_average`, `total`,
`average` (overall average across containing spaces), and per-kind
`functions_min`, `functions_max`, `closures_min`, `closures_max`.

The split lets you ask different questions of the same code: a Rust
crate with many closures and few functions is typical of
iterator-heavy code; a Python module with many functions and few
closures is typical of script-style code.

Some constructs that carry executable code are deliberately counted by
neither field: Kotlin property accessors (`get()` / `set()`) and `init`
blocks, Java and Groovy `static { … }` initialisers, and JavaScript
class static blocks. Each opens a function *space* — so it has its own
complexity scores, `bca check` can flag it, and it contributes to WMC —
but none is a callable you name at a call site, and counting an accessor
as a method would make NPM bill the same property once as an attribute
and again as a method, skewing the NPA/NPM ratio the OOP metrics exist
to report. A Kotlin file of nothing but property accessors therefore
reports `nom.functions == 0` while still reporting their complexity
(#1184).

### How to read it

NOM is the input to several other metrics — WMC sums *cyclomatic*
complexity across the same set of methods that NOM counts, and NPM
filters that same set down to public methods. As a standalone
metric, the Lorenz–Kidd recommendation is `≤ 20` methods per class.
The emergent use is as a *God-class detector*: a class with NOM in
the dozens is almost always doing too much, and is a strong
candidate for "extract collaborator" refactoring as documented in
Martin Fowler's [*Refactoring* catalogue
entry on Large Class](https://refactoring.com/catalog/extractClass.html).

## NPA {#npa}

**NPA** counts the **number of public attributes** (a.k.a. fields,
properties, instance variables) declared by a class or interface. It
is part of the metric family introduced by Lorenz and Kidd in
*Object-Oriented Software Metrics* (1994) and was later folded into
the MOOD ("Metrics for Object-Oriented Design") suite proposed by
[Brito e Abreu and Carapuça
(1994)](https://www.researchgate.net/publication/267412803_Object-Oriented_Software_Engineering_Measuring_and_Controlling_the_Development_Process).

big-code-analysis splits the count by definition-site kind:
*classes* (concrete types with state) and *interfaces* (abstract
contracts). The serialised output (`src/metrics/npa.rs`) is
`class_npa_sum` (sum of NPA across all classes), `interface_npa_sum`
(sum across interfaces), `class_attributes` (sum of *all* attributes — public or
not — across classes), `interface_attributes`, `class_cda`
(class density of public attributes — an accessibility *ratio*, not an
average), `interface_cda`, `total`, `total_attributes`, and `cda`. The
per-language `Npa` trait
decides what counts as "public" (Java `public`, C# `public`, Rust
`pub`, Python's "no leading underscore" convention, …) and what
counts as "attribute" rather than "method".

### Which spaces carry NPA, NPM and WMC {#oop-emission-scope}

NPA and NPM are emitted on **container spaces** — `class`, `struct`,
`trait`, `impl`, `namespace`, `interface` — and on the whole-file `unit`
root, which carries the roll-up across every container in the file. WMC
follows the same rule wherever it is computed at all, which is narrower
in two ways set out below. A **function space does not carry any of
them**: a
method owns no methods or attributes of its own, so the block would be
all zeros. Before big-code-analysis 2.1.0, NPA and NPM did emit that
all-zero block on function spaces in C#, JavaScript, MozJS, TypeScript,
TSX, PHP and Ruby, and on the Kotlin, Java, Groovy and JS-family
accessor / `init` / `static` spaces. In the same release Go, Rust,
Python, C++, Objective-C and Elixir went the other way: they decided
from their own grammar node kinds, so a `struct` declared inside a
function put the block on that **function** space, while a `namespace`
or a file root with no container at file scope carried none. Both
deviations are gone — the space's kind is now the only input, for every
language.

That rule governs the *block*, not the numbers behind it: the counts
roll up through every enclosing space regardless. So a type declared
inside a function body is reported by the nearest enclosing container,
or by the file root when there is none.

Four things read differently. A Go file's NPA and NPM live on the `unit`
root and nowhere else, because Go is the one language that emits them
without having a container kind in its space tree — `type … struct` and
`type … interface` open no space of their own. (Bash, C, Lua, Perl,
Tcl and iRules have no container kind either, but they emit neither
block anywhere, so the question does not arise.)

Neither WMC narrowing moves NPA or NPM, which is why the rule above
still holds as stated for those two. The first is language-level: **Go
emits no `wmc` block at all**, on any space including the `unit` root,
because its flat space model cannot attribute a method to a receiver
class — so a Go file carries `npa` and `npm` at the root and `wmc`
nowhere. The second is space-kind-level: a **`namespace` space carries
`npa` and `npm` but no `wmc`**, because a namespace's member functions
are free functions rather than methods of a class, so there is no
per-class complexity to weight. That covers every construct mapping to
`SpaceKind::Namespace` — a C++ or Mozcpp `namespace`, and a Ruby
`module` — not just the C++ spelling. Objective-C has no namespace
construct of its own, so the case does not arise there. The class
*inside* the namespace carries all three, and so does the file root.

Finally, the CSV projection
is a fixed-column format: it writes the `npa.*` / `npm.*` columns on
**every** row regardless of space kind, carrying the real accessor
values rather than eliding them.

Thresholds are narrower still — `bca check` gates `npa` / `npm` on
container spaces only, never the file root (see
[Threshold scope](commands/check.md#threshold-scope)). Taken with the
paragraph above, that has a consequence worth stating outright: since a
Go file's NPA and NPM are only ever reported at the root, **no `npa` or
`npm` threshold can fire on Go source**.

### How to read it

NPA is a *direct* measure of encapsulation. Every public attribute
is a piece of internal state that callers can read or write without
going through a method, which means it is a piece of internal state
the class cannot validate or evolve without breaking callers. The
canonical guidance — first explicitly stated in Bertrand Meyer's
[*Object-Oriented Software
Construction*](https://en.wikipedia.org/wiki/Object-Oriented_Software_Construction)
(Prentice Hall, 1988) and known as the *Uniform Access Principle* — is
to keep NPA at or near
zero and to expose state through public methods instead.

The emergent use is **API-stability auditing**: a public library
class whose NPA grows over time accumulates breaking-change
liability faster than its public-method surface.

## NPM {#npm}

**NPM** counts the **number of public methods** declared by a class
or interface. It is the method-side companion to NPA and was again
codified by Lorenz and Kidd (1994).

As with NPA, big-code-analysis splits NPM by definition-site kind
(classes vs. interfaces). The serialised output
(`src/metrics/npm.rs`) is `class_npm_sum` (sum of NPM across classes),
`interface_npm_sum`, `class_methods` (sum of *all* methods — public or
not — across classes), `interface_methods`, `class_coa`,
`interface_coa` (operation-accessibility *ratios*, not averages),
`total`, `total_methods`, and `coa`. It follows the same emission rule
as NPA ([above](#oop-emission-scope)).
The language-specific `Npm` trait decides what counts as public —
for example, Rust's `pub`, Python's leading-underscore convention,
C++'s `public:` section — and folds together regular methods,
constructors, and operator overloads as appropriate.

NPM is also one of the inputs into [Mark Hitz and Behzad
Montazeri's *Class Interface Size*
metric](https://link.springer.com/chapter/10.1007/978-94-011-5006-1_19),
and into Chidamber and Kemerer's *Response For a Class* (RFC).

### How to read it

NPM is the **public interface size**. A class with NPM in the dozens
is a class with too large an API contract: every public method is
something callers can come to depend on, and every change to it is a
breaking change. The Lorenz–Kidd guidance is `≤ 20` public methods
per class, with anything over `40` being considered a strong
refactoring candidate. The same rule applies particularly forcefully
to *interfaces* in Java and C#, where the contract really is the
shape clients pin against.

The emergent use is as a **public-API change tracker** for
libraries: monitoring NPM at the package level catches accidental
expansion of a library's surface area in the same way that NPA
catches accidental exposure of internal fields.

## Tokens {#tokens}

**Tokens** is a per-function and per-file count of the *tree-sitter
leaf tokens* — identifiers, literals, keywords, punctuation —
excluding any token whose AST ancestor is a comment node. It is a
modern, lexer-driven size proxy intended as a more
formatting-resilient alternative to LOC. (The same idea is well
known from Terry Yin's [`lizard`](https://github.com/terryyin/lizard)
command-line tool, which is where many readers will first have seen
a token-count metric.)

The implementation lives in `src/metrics/tokens.rs`. Because Tokens
counts *every* leaf, including punctuation that Halstead
deliberately skips, the value will *not* equal Halstead `N1 + N2`,
and because it counts tokens rather than lines it is **not**
equivalent to any LOC variant. Whitespace-only reformatting does not
change Tokens; renaming a variable does not change the count;
removing a comment does not change Tokens. Edits that change the
*tokens themselves* — adding an `if`, adding optional braces around
a single-statement block, or inserting/removing semicolons in a
language where they are optional — do change the count.

### How to read it

Tokens is the most **formatting-resilient size proxy** in the suite.
It is the right size measure to use when you are normalising another
metric across languages or across teams with different style
conventions — `bugs per KSLOC` is sensitive to formatting, while
`bugs per 1000 tokens` is much less so.

The emergent use is as the **defect-density denominator of choice**
in cross-language research: a 1000-line Java file and a 1000-line
Lisp file contain very different amounts of code, but a
1000-*token* slice of each contains roughly the same amount of
information. This makes Tokens particularly useful for
machine-learning code-quality models that train across many
languages.

## WMC {#wmc}

**WMC** — *Weighted Methods per Class* — is the first metric in
the [Chidamber and Kemerer suite](https://www.eso.org/~tcsmgr/oowg-forum/TechMeetings/Articles/OOMetrics.pdf),
introduced in their 1994 IEEE Transactions on Software Engineering
paper *A Metrics Suite for Object Oriented Design* (volume 20,
issue 6, pages 476-493). The CK suite — WMC, DIT, NOC, CBO, RFC,
LCOM — is the single most-cited collection of OO metrics in the
academic literature; big-code-analysis currently implements WMC and
the simpler size metrics (NOM, NPA, NPM), with the inheritance- and
coupling-based ones tracked for future work.

WMC is the **sum of the cyclomatic complexity of every method
defined in a class**. The original paper deliberately left the
"weighting" abstract — Chidamber and Kemerer wrote that "if all
method complexities are considered to be unity, then WMC = n, the
number of methods" — but the empirical follow-up literature has
almost universally settled on cyclomatic complexity as the weight,
and that is what big-code-analysis uses.

### Algorithm

For each class or interface found by the per-language parser,
big-code-analysis sums the standard cyclomatic complexity of every
method body inside it (`src/metrics/wmc.rs`). The file-level
serialised output is three fields: `class_wmc_sum` (sum of WMC across
all classes in the file), `interface_wmc_sum` (sum across interfaces),
and `total` (the two combined). No min/max/average aggregation is
emitted at the file scope — to rank individual classes by WMC, use
the report subcommand, which surfaces a *Type hotspots (top N by
WMC)* section (see [Commands → Report](./commands/report.md)).

### How to read it

Chidamber and Kemerer offered three hypotheses about WMC, all of
which have been validated repeatedly since:

1. **Higher WMC predicts higher maintenance effort.** A class whose
   methods are individually complex will resist comprehension.
2. **Higher WMC reduces reuse.** Classes that do many complicated
   things are hard to drop into a new context.
3. **Higher WMC suggests broader application-specific behaviour.**
   Such classes tend to be "main loop"-style coordinators rather
   than reusable building blocks.

The emergent use is **God-class detection**: combined with NOM,
WMC is one of the clearest signals that a class needs to be split.
A class with high NOM but low WMC is a passive data holder
(probably fine). A class with low NOM and high WMC has a few
gargantuan methods (split the methods, not the class). A class with
*both* high NOM and high WMC is the classic God class.

---

## Where to go next

- The [Supported Languages](./languages.md) chapter lists every
  supported language and grammar. Metric coverage varies by
  language because some metric definitions (`NPA`, `NPM`, `WMC`)
  only make sense in languages with classes.
- The [Supported Change-history (VCS) Metrics](./metrics-vcs.md)
  chapter covers the complementary family derived from version-control
  history — commit frequency, churn, ownership, and the composite risk
  and hotspot scores — rather than from the source AST.
- The [Commands → Metrics](./commands/metrics.md) page documents
  how to invoke `bca metrics` to produce the JSON / YAML / TOML /
  CBOR output for any of these numbers.
- The [Recipes](./recipes/quality-reports.md) chapter shows
  end-to-end examples of producing quality reports from these
  metrics, including pipelining them into dashboards.
