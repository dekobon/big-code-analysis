# Supported Metrics

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
  through that tree. See the [Supported Languages](./languages.md)
  chapter for which scopes apply to which languages.
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

## ABC

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
  keyword set (`else`, `case`, `default`, `try`, `catch`). The
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
| Compound assignment (`+=`, `-=`, `*=`, `/=`, `%=`, `<<=`, `>>=`, `>>>=`, `&=`, `\|=`, `^=`) | one per occurrence | Figure 2 (C) / Figure 4 (Java) |
| Pre- or post-increment / decrement (`++`, `--`) | one per occurrence | Figure 2 (C) |
| Initializing constructor invocation | one per occurrence | Figure 3 (C++) |

#### Branches

| Rule | Counted as `B` | First defined in |
|------|----------------|------------------|
| Function or method call | one per call site | Figure 2 (C) / Figure 4 (Java) |
| `new` operator | one per occurrence | Figure 3 (C++) / Figure 4 (Java) |
| `delete` operator | one per occurrence | Figure 3 (C++) |
| `goto label`, `break label`, `continue label` | one per occurrence | Figure 2 (C) / Figure 3 (C++) |

#### Conditions

| Rule | Counted as `C` | First defined in |
|------|----------------|------------------|
| Comparison operator (`==`, `!=`, `<=`, `>=`, `<`, `>`) | one per occurrence | Figure 2, Rule 5 |
| Ternary `? :` | one per occurrence | Figure 2, Rule 5 |
| `else`, `case`, `default` | one per occurrence | Figure 2, Rule 5 |
| Preprocessor `#else`, `#elif` | one per occurrence | Figure 2, Rule 5 |
| `try`, `catch` | one per occurrence | Figure 3, Rule 7 / Figure 4, Rule 9 |
| Unary conditional expression | one per non-comparison operand of `&&` / `\|\|` (and per `!`-wrapped or bare-truthy condition in `if` / `while` / argument / `return` slots) | Figure 3, Rule 7 / Figure 4, Rule 9 |

The short-circuit logical operators (`&&`, `||`, and per-language
equivalents — Ruby `and` / `or`, Python `and` / `or`, Perl `and` /
`or` / `xor`, Lua `and` / `or`, Tcl `&&` / `||`) do **not**
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
| C++, Go, Python, Rust | `default` excluded from the condition set | Falls through unconditionally to the default arm — counting it inflates `C` on every `switch` / `match` regardless of body. Aligns with the Rust `_ =>` and existing Java `default:` precedent. |
| Tcl | Unary-conditional walker not yet wired | Phase 2 walker is deferred pending an audit of Tcl's `expr {…}` / command-substitution grammar. `if {$a && $b}` reports zero conditions today; a follow-up will close this. |
| All Phase 2 languages (Java, Groovy, C#, Rust, Go, JavaScript, TypeScript, TSX, Mozjs, PHP, C++, Python, Perl, Lua) | `if (true) {}`, `m(!a, !b)`, `return !x` count their operand(s) | Phase 2B routes `if` / `while` / `do-while` / argument-list / `return` / ternary slots through the same walker, so the rule applies uniformly across decision-bearing positions. A bare `return x` continues to report zero — Fitzpatrick treats an identifier in a return slot as a value, not a unary conditional. |

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
contributes zero. Combined with the surrounding `else` (if any)
and the `c = 'C'` assignment, this matches the worked count in
Listing 2 of the paper.

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
together with the per-component averages (`assignments_average`,
`branches_average`, `conditions_average`) and per-component
`*_min` / `*_max` at the file scope, for thirteen fields total. The
metric is specialised per language in `src/languages/language_*.rs`.

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

## Cognitive Complexity

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

## Cyclomatic Complexity (CC)

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

## Halstead

The **Halstead suite** is the oldest size-and-effort metric family on
this page. Maurice H. Halstead introduced it in his 1977 book
*Elements of Software Science* (Elsevier, ISBN 0-444-00205-7); the
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

| Symbol | Meaning |
|--------|---------|
| `n1` | number of **distinct** operators |
| `n2` | number of **distinct** operands |
| `N1` | **total** count of operator occurrences |
| `N2` | **total** count of operand occurrences |

big-code-analysis records these four numbers in
`src/metrics/halstead.rs` per function and per file. The per-language
trait classifies tokens as operator vs. operand on a token-by-token
basis; the rules deliberately exclude pure layout punctuation like
parentheses and statement separators, which is why the Halstead
totals are *not* the same as the Tokens count.

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

## Lines of Code

This section covers the five LOC variants — SLOC, PLOC, LLOC, CLOC,
and BLANK. "Counting lines" sounds trivial until you have to define exactly
what counts. The five variants below are the de-facto standard
breakdown, going back to Samuel Conte, Hubert Dunsmore and Vincent
Shen's 1986 textbook *Software Engineering Metrics and Models*
(Benjamin/Cummings, ISBN 0-8053-2162-4), which codified the
distinction between physical and logical lines. The OpenStaticAnalyzer
project maintains a [readable summary of the modern
definitions](https://github.com/sed-inf-u-szeged/OpenStaticAnalyzer/blob/master/doc/usersguide/md/SourceCodeMetricsRef.md).

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

## Maintainability Index (MI)

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

```text
mi_original      = 171 − 5.2·ln(HV) − 0.23·CC − 16.2·ln(SLOC)
mi_sei           = 171 − 5.2·log2(HV) − 0.23·CC − 16.2·log2(SLOC) + 50·sin(√(2.4·comment_ratio))
mi_visual_studio = max(0, mi_original · 100 / 171)
```

- `mi_original` is the Coleman–Oman formula. It can be negative for
  pathological files.
- `mi_sei` is the Software Engineering Institute's refinement, which
  adds a comment-density term — the `sin(√(...))` shape was chosen so
  that *some* comments help, but adding more after a point does not.
- `mi_visual_studio` is the linear rescaling Microsoft chose for
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
green / yellow / red icon, and the underlying number is `mi_visual_studio`.
This made MI by far the most user-facing software metric for an
entire generation of .NET developers, which is also why it is the
metric that has attracted the most criticism. Treat it as a smoke
detector, not a thermostat: a sudden drop is a useful signal, but
the absolute number is noisy.

## NArgs

**NArgs** counts the number of arguments declared by a function,
method, or closure. The metric does not have a famous origin paper —
it is folk wisdom dating to at least Kernighan and Plauger's *The
Elements of Programming Style* (1974) and prominently re-stated in
Robert C. Martin's *Clean Code* (2008), which suggests three
arguments as a soft ceiling.

big-code-analysis splits the count by callable kind: every aggregate
is reported separately for *functions* and *closures* so a Rust file
heavy on `|…| …` closures and a Java file with only methods produce
comparable numbers. The serialised output
(`src/metrics/nargs.rs`) is `total_functions`, `total_closures`,
`average_functions`, `average_closures`, `total`, `average`,
`functions_min`, `functions_max`, `closures_min`, `closures_max`.
The implementation handles default arguments, variadic arguments,
keyword-only arguments, and destructured parameters consistently per
language.

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

## NExits

**NExits** counts the number of distinct exit points from a
function — every `return`, every `throw` / `raise`, and the implicit
fall-through return at the end of a void function.

The metric goes back to the structured-programming literature of the
1970s, where Edsger Dijkstra and others argued that functions should
have **a single entry and a single exit point** (the "SESE" rule).
Modern thinking is much more nuanced — see Steve McConnell's
*Code Complete*, 2nd edition (Microsoft Press, 2004), which
explicitly recommends *early returns* as a clarity-improving pattern
when they reduce nesting.

big-code-analysis walks each function's syntax tree, identifies the
language-specific exit nodes (see the per-language `Exit` trait in
`src/metrics/exit.rs`), and reports per-function counts plus
file-level `sum`, `average`, `min`, and `max`. The serialised
field name is `nexits`, matching the prose acronym used here.

### How to read it

Strict SESE coding standards (DO-178C for avionics, MISRA C for
embedded automotive — see [MISRA's official
site](https://misra.org.uk/)) still require an NExits of 1 per
function, because multiple exit points complicate certified
control-flow analysis. Outside those domains, an NExits of `2-4` is
usually a *good* sign — it almost always means the function uses
guard clauses to handle preconditions and then proceeds in a flat
body.

A *very* high NExits — say above 8 — is the warning sign. It usually
means the function should have been split into several smaller
functions, with each "successful branch" becoming its own helper.

## NOM

**NOM** stands for *Number Of Methods* and counts every function,
method, and closure defined inside a given scope (file, class, or
namespace). For object-oriented codebases it is one of the first
metrics introduced by Mark Lorenz and Jeff Kidd in their 1994 book
*Object-Oriented Software Metrics* (Prentice Hall, ISBN
0-13-179292-X), where it is treated as the primary class-size
indicator.

big-code-analysis reports the count split by callable kind in
`src/metrics/nom.rs`. The serialised fields are `functions`,
`closures`, `functions_average`, `closures_average`, `total`,
`average` (overall average across containing spaces), and per-kind
`functions_min`, `functions_max`, `closures_min`, `closures_max`.

The split lets you ask different questions of the same code: a Rust
crate with many closures and few functions is typical of
iterator-heavy code; a Python module with many functions and few
closures is typical of script-style code.

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

## NPA

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
`classes` (sum of NPA across all classes), `interfaces` (sum across
interfaces), `class_attributes` (sum of *all* attributes — public or
not — across classes), `interface_attributes`, `classes_average`
(class density of public attributes), `interfaces_average`, `total`,
`total_attributes`, and `average`. The per-language `Npa` trait
decides what counts as "public" (Java `public`, C# `public`, Rust
`pub`, Python's "no leading underscore" convention, …) and what
counts as "attribute" rather than "method".

### How to read it

NPA is a *direct* measure of encapsulation. Every public attribute
is a piece of internal state that callers can read or write without
going through a method, which means it is a piece of internal state
the class cannot validate or evolve without breaking callers. The
canonical guidance — first explicitly stated in Bertrand Meyer's
*Object-Oriented Software Construction* (Prentice Hall, 1988) and
known as the *Uniform Access Principle* — is to keep NPA at or near
zero and to expose state through public methods instead.

The emergent use is **API-stability auditing**: a public library
class whose NPA grows over time accumulates breaking-change
liability faster than its public-method surface.

## NPM

**NPM** counts the **number of public methods** declared by a class
or interface. It is the method-side companion to NPA and was again
codified by Lorenz and Kidd (1994).

As with NPA, big-code-analysis splits NPM by definition-site kind
(classes vs. interfaces). The serialised output
(`src/metrics/npm.rs`) is `classes` (sum of NPM across classes),
`interfaces`, `class_methods` (sum of *all* methods — public or
not — across classes), `interface_methods`, `classes_average`,
`interfaces_average`, `total`, `total_methods`, and `average`.
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

## Tokens

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

## WMC

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
serialised output is three fields: `classes` (sum of WMC across
all classes in the file), `interfaces` (sum across interfaces),
and `total` (the two combined). No min/max/average aggregation is
emitted at the file scope — to rank individual classes by WMC, use
the report subcommand, which surfaces a *WMC hotspots* section
(see [Commands → Report](./commands/report.md)).

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

- The [Supported Languages](./languages.md) chapter lists which
  metrics fire for which languages — language coverage varies
  because some metric definitions (`NPA`, `NPM`, `WMC`) only make
  sense in languages with classes.
- The [Commands → Metrics](./commands/metrics.md) page documents
  how to invoke `bca metrics` to produce the JSON / YAML / TOML /
  CBOR output for any of these numbers.
- The [Recipes](./recipes/quality-reports.md) chapter shows
  end-to-end examples of producing quality reports from these
  metrics, including pipelining them into dashboards.
