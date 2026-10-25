# Supported Code Metrics

This chapter describes every metric that **big-code-analysis**
computes. Each section starts from the original research paper, walks
through the algorithm, and explains both how the metric was
*originally* meant to be used and how the industry uses it today. If
you are new to software metrics, read the sections in order: the later
metrics (the Maintainability Index in particular) are built on the
earlier ones (Halstead, Cyclomatic, LOC).

Three framing notes:

- **A metric is a measurement, not a verdict.** Every number on this
  page summarises a structural property of source code. None of them
  measures correctness, productivity, or developer skill. The first
  question for any metric is "compared with what?": the same module a
  month ago, this module versus its siblings, this codebase versus an
  industry baseline. Absolute thresholds are rough heuristics.
- **Most metrics here are computed at three scopes**: per *function /
  method*, per *class or unit-like space*, and per *file*. The
  underlying [tree-sitter](https://tree-sitter.github.io/tree-sitter/)
  parser produces a tree of "spaces" (functions, closures, classes,
  namespaces, and so on) and every metric is rolled up through that
  tree.
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
| [Lines of Code (SLOC, PLOC, LLOC, CLOC, BLANK, DLOC)](#lines-of-code) | Raw, physical, logical, comment, blank, and documentation line counts | Conte, Dunsmore & Shen, 1986 |
| [Maintainability Index (MI)](#maintainability-index-mi) | Composite maintainability score | Oman & Hagemeister, 1992; Coleman *et al.*, 1994 |
| [NArgs](#nargs) | Number of arguments per function | folk metric |
| [NExits](#nexits) | Number of exit points per function | structured-programming literature |
| [NOM](#nom) | Number of methods and closures | Lorenz & Kidd, 1994 |
| [NPA](#npa) | Number of public attributes | Lorenz & Kidd, 1994 |
| [NPM](#npm) | Number of public methods | Lorenz & Kidd, 1994 |
| [Tokens](#tokens) | Tree-sitter leaf-token count (size proxy) | [Lizard](https://github.com/terryyin/lizard) tool, Terry Yin |
| [WMC](#wmc) | Sum of cyclomatic complexity across a class's methods | Chidamber & Kemerer, 1994 |

## ABC {#abc}

The **ABC** metric measures the size of a piece of code as a
three-dimensional vector. Each component counts one kind of operation:

- **A**ssignments: anything that stores a value into a variable,
  including compound assignments (`+=`, `++`) and explicit
  initialisation.
- **B**ranches: function and method *calls*. Despite the name, this
  is not the count of conditional jumps; it is the number of points
  where control branches out to other code.
- **C**onditions: boolean tests. These are comparison operators
  (`==`, `!=`, `<=`, `>=`, `<`, `>`), ternary operators (`?`), and the
  fixed keyword set (`else`, `case`, `try`, `catch`). The `default` /
  wildcard arm is **not** counted in any language (see the
  per-language deviations below). The short-circuit logical operators
  `&&` and `||` are **not** counted on their own; instead, each
  non-comparison operand of a `&&` / `||` chain contributes one
  condition via Fitzpatrick's "unary conditional expression" rule.
  The next subsection walks through the rules, the per-language
  deviations, and worked examples.

Jerry Fitzpatrick introduced the metric in the 1997 C++ Report
article *Applying the ABC metric to C, C++ and Java*. The current
canonical specification, including the rules for what counts as an
*A*, *B*, or *C* in modern languages, is maintained on Fitzpatrick's
[Software Renovation](https://www.softwarerenovation.com/Articles.aspx)
site.

### Counting rules

Fitzpatrick's paper enumerates the rules in three figures: Figure
2 (C), Figure 3 (C++, which extends Figure 2), and Figure 4 (Java).
The tables below summarise the paper's rule set, with each row
attributed to the figure that introduces it. Big-code-analysis
implements them per language, with the exceptions listed under
[Per-language deviations](#abc-per-language-deviations) and these
paper rows that no language implements today: the `delete` operator,
the `goto` / labeled `break` / labeled `continue` jumps, the
preprocessor `#else` / `#elif`, and the C++ initializing constructor
invocation (`S s(1);`). Each scores zero.

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
| `goto label`, `break label`, `continue label` | one per occurrence | Figure 2 (C) / Figure 3 (C++) / Figure 4 (Java, labeled `break` / `continue` only, since Java has no `goto`) |

#### Conditions

| Rule | Counted as `C` | First defined in |
|------|----------------|------------------|
| Comparison operator (`==`, `!=`, `<=`, `>=`, `<`, `>`) | one per occurrence | Figure 2, Rule 5 |
| Ternary `? :` | one per occurrence | Figure 2, Rule 5 |
| `else`, `case` | one per occurrence | Figure 2, Rule 5 |
| Preprocessor `#else`, `#elif` | one per occurrence | Figure 2, Rule 5 |
| `try`, `catch` | one per occurrence | Figure 3 (C++) / Figure 4 (Java) |
| Unary conditional expression | one per `&&` / `\|\|` operand and per `if` / `while` / ternary / guard predicate that no other rule already counts (and per `!`-wrapped argument or `return` value) | Figure 3, Rule 7 / Figure 4, Rule 9 |

The short-circuit logical operators (`&&`, `||`, and the per-language
equivalents: Ruby `and` / `or`, Python `and` / `or`, Perl `and` /
`or`, Lua `and` / `or`, Tcl `&&` / `||`, iRules `&&` / `||` /
`and` / `or`) do **not** contribute a condition on their own. Each
non-comparison operand contributes one instead, via the
unary-conditional rule. Perl and PHP `xor` evaluate both operands, so
like `^` they are a value: the slot holding one pays a single
condition. The paper makes this explicit twice:

1. **Listing 2** annotates `(am >= 0 && am <= 0xF) ? '/' : 'C'` as
   `accc`: one assignment plus three conditions, where the three
   conditions are the two comparisons (`>=`, `<=`) and the
   ternary (`?`). The `&&` itself contributes zero.
2. **Rule 7 / Rule 9** instead counts each operand: for
   `if (x || y) printf("test failure\n");` the paper writes "there
   are two unary conditions since both `x` and `y` are tested as
   conditional expressions". The `||` again contributes zero; `x`
   and `y` each contribute one.

#### Per-language deviations {#abc-per-language-deviations}

Per-language `impl Abc` blocks narrow the paper rule set where the
language has no equivalent construct, or where strict literal
application would over-count.

| Language | Deviation | Reason |
|----------|-----------|--------|
| C, Go, Rust | `try` / `catch` omitted | No `try`/`catch` keyword in the grammar; error-handling uses `errno` / `Result` / `Result`-like sums. |
| Ruby | `Rescue` substitutes for `catch` | Ruby's exception-handling keyword is `rescue`; the AST node `Rescue` plays the role of Java's `catch`. |
| All languages | `default` / `_` wildcard arm excluded from the condition set | Fitzpatrick's Figure 2 lists `default`, but it falls through unconditionally, so counting it would inflate `C` on every `switch` / `match` regardless of body. big-code-analysis omits it for every language (the Rust `_ =>` and Java `default:` arms included). |
| Tcl | Chain-operand, bare-truthy and ternary slots wired; argument and `return` slots are not | Each operand of a `&&` / `\|\|` chain inside `expr {…}` counts as one condition, so `if {$a && $b}` reports two. `if` / `elseif` / `while` route their `expr {…}` predicate, so a bare-truthy `if {$a}` reports one and `if {!$a}` likewise, matching C's `if (a)` (#1180). A predicate written as a command substitution is a truthy test of that command's result, so `if {[somecmd]}` also reports one, and the redundant `if {[expr {$a \|\| $b}]}` idiom reports three (the chain's two operands plus the substitution). The argument and `return` slots remain unrouted: a negation reached only through a standalone `expr {…}` command still reports zero. |
| iRules | Chain-operand, bare-truthy and ternary slots wired; argument and `return` slots are not | Each operand of a `&&` / `\|\|` / `and` / `or` chain counts as one condition (Rule 9), so `if {!$a && !$b}` reports two. iRules also recognises the word-form string-match comparators (`contains`, `starts_with`, `ends_with`, `equals`, `matches`, and so on) that Tcl lacks (Tcl's `eq` / `ne` / `in` / `ni` are shared). Bare-truthy and ternary slot routing matches its Tcl sibling exactly (#1180); see that row for the shared detail, including the argument and `return` slots that remain unrouted. |
| Java, Groovy, C#, Rust, Go, JavaScript, TypeScript, TSX, Mozjs, PHP, C, C++, Objective-C, Mozcpp, Python, Perl, Lua | `if (true) {}`, `m(!a, !b)`, `return !x` count their operand(s) | These implementations pass `if`, `while`, `do-while`, argument-list and `return` expressions through the same condition walker, so the rule applies uniformly across decision-bearing positions. A bare `return x` continues to report zero: Fitzpatrick treats an identifier in a return slot as a value, not a unary conditional. C# wraps each argument in an `argument` node, so the walker reads the value after any named-argument label or `ref` / `out` / `in` keyword: `M(!a, !b)` and `M(name: !a, !b)` report two, as do the base calls `: base(!a, !b)` and `class S(bool a, bool b) : Base(!a, !b)`. Before #1537 it read the wrapper instead, and every C# argument reported zero. An indexer argument (`o[!a]`) is not a call argument and reports zero in C#, as Java's array index does. |
| Ternary slots: Java, Groovy, C#, C, C++, Objective-C, Mozcpp, JavaScript, TypeScript, TSX, Mozjs, PHP, Perl, Ruby, Python, Tcl, iRules | `a ? !b : !c` counts its condition and both branch operands | The same walker also runs over a ternary's three operand slots, so `a ? !b : !c` reports 4 (the `?` plus three unary conditions) rather than 1. Tcl and iRules reach the same 4 without grammar fields: their `ternary_expr` exposes none, and `_expr` inlines `( … )`, so the slots are located relative to the `?` and `:` tokens instead of by index (#1180). Python arrives at the same total by a different route: `not` operands are counted by the `NotOperator` rule wherever they appear, so only the condition slot is routed through the walker and `(not b) if a else (not c)` likewise reports 4. Languages with no ternary (Rust, Go, Kotlin, Lua, Elixir) are unaffected. |
| `for` header condition slot: Java, Groovy, C#, C, C++, Objective-C, Mozcpp, JavaScript, TypeScript, TSX, Mozjs, PHP, Go, Perl | `for (; a; )` counts one condition; `for (;;)` counts none | The loop header's condition is a decision-bearing slot like an `if` predicate, so `for (; a; )`, `for (; !a; )` and `for (; (a); )` each report one. A comparison-shaped condition (`i < n`) is already counted by its own operator arm and is not double-counted. Every one of these grammars except Go exposes a `condition` field on `for_statement` (Perl's sits on its C-style `for_statement_1`), so the slot is read by field name rather than by child index; Go's field sits on the nested `for_clause`, and its outer header slot is located structurally (first named child that is neither the body nor a comment). An **empty** condition counts zero in every language: Fitzpatrick's rules count conditional operators and unary conditions that are present, and an omitted test is not a decision. Java and Groovy previously scored `for (;;)` as one vacuously-true condition and no longer do. Tcl, iRules and Bash are command-dispatched grammars whose loop headers carry no boolean-expression slot to route (grammar-dispatch §9) (#1276). |
| Ruby | Bare-predicate `if` / `unless` / `while` / `until` (block and modifier forms) count one condition | Idiomatic Ruby favours bare predicates (`if flag`, `x if flag`); counting the condition slot keeps ABC conditions at or above Ruby's cyclomatic decision count (the alignment enforced across the other languages). A comparison (`if a == b`) or `&&` / `\|\|` chain in the predicate is counted by its own operator / walker arm and is not double-counted. Each `&&` / `\|\|` / `and` / `or` operand is scored by the same rule (once, unless its own arm already counts it), so `a && self`, `a && Foo::Bar` and `a && -b` score 2 like `a && b`, wherever the chain is written (#1529). |
| Python, JavaScript, TypeScript, TSX, Mozjs, C, C++, Objective-C, Mozcpp, Java, C#, Rust, PHP, Perl, Lua, Groovy, Kotlin, Tcl, iRules | A boolean slot pays one condition unless something inside its predicate already counts it | Every `if` / `while` / `do` / `for` condition, ternary condition, guard and `&&` / `\|\|` operand scores the way Ruby's has since #1520: one condition, unless the predicate (seen through parentheses, a negation, a cast or a plain assignment) is a comparison, chain, ternary or type test that its own rule counts. Before #1526 these slots paid only for a predicate from a fixed list of operand kinds, so `if (-x)`, `if (x + 1)`, `if (this)`, `if (*p)`, `if (flags & k)`, `if (y = x)` and `if (sizeof x)` each scored 0 against a cyclomatic decision of 1, as did Java's, C#'s and Rust's `if (a & b)` and a boolean `switch` / `match` expression. The same rule stops a wrapped comparison paying twice: `if ((int)(x > 1))`, Python's `if (y := x > 1):` and Python's `if not (x > 0):` score 1, not 2, as Java's `if (!(x > 0))` always has. Python's `elif`, PHP's `elseif`, Lua's `elseif` and Perl's `elsif` now score like Ruby's `elsif` and Java's `else if`: one for the `else` plus the predicate slot, so `elif b` no longer scores one below `elif x > 0`. Kotlin joined in #1533, so the slot pays for a `when` it holds, as it does for Java's `switch`, C#'s `switch` expression and Rust's `match`: `if (when (a) { else -> true })` scores 1 (it scored 0) and `if (when (x) { 1 -> true; else -> false })` 2 (it scored 1), its entry paying 1 and the slot 1. Go still pays a slot through an operand list (#1526). |
| Bash | `if` / `elif` / `while` and each non-wildcard `case` arm count one condition | A Bash predicate is a *command*, so the branch keyword itself, not an embedded boolean expression, is the condition signal. Each matches a Bash cyclomatic decision; the bare `*)` case arm (the analogue of `default:`) is excluded, mirroring the cyclomatic standard count. The arithmetic ternary `$(( a ? b : c ))` counts one, as the C-family ternary does (#1268). Each comparison inside a test pays one more: `==`, `!=`, the `-eq` / `-z` / `-f` family, and `=`, which compares in `[ … ]` / `[[ … ]]` and assigns in `(( … ))`. The `[ … ]` connectives `-a` / `-o` join two tests as `&&` / `\|\|` do, and pay nothing, so `[ X -a Y ]`, `[ X ] && [ Y ]` and `[[ X && Y ]]` score alike in every decision metric; a bare `test X -a Y` is all plain words to the grammar and scores none of them (#1536). |
| Kotlin | `try` counts a condition alongside `catch` | Fitzpatrick counts both keywords, and Java / C# / C++ / Groovy already count both; Kotlin previously counted only the catch block. |
| C#, Java, Rust, Python, Ruby, Elixir | A pattern-match guard is a condition slot, scoring one however it is spelled | A guard is a branch: the pattern can match while the guard fails. The guard's expression is scored exactly as an `if` condition is (one for a call, type test, attribute or bare identifier, one via the operator arm for a comparison), so every spelling agrees, and a compound guard (`when a > 1 && b < 2`) keeps its sub-structure rather than collapsing to one. Before this, ABC scored whatever operator happened to sit inside, so `when x > 2` counted one and the equivalent `when IsEven(x)` counted none. The spellings, per language: C# `when_clause` on a switch arm or `case` section and `catch_filter_clause` on a `catch` (#1422); Java 21's `guard` on a pattern-switch label, in both the arrow and colon forms; Rust's `match_pattern` guard; Python's `case … if g:`; Ruby's `if_guard` / `unless_guard` on a `case … in` arm; Elixir's `when` operator on a `stab_clause` head or a `def` / `defguard` head (#1454). The same change made the guard a **cyclomatic** decision wherever it was not already one (Java, Ruby and Elixir) and excluded Elixir's typespec `when` (`@spec f(a) :: a when a: integer`), which spells the same token as a guard but is type syntax. Groovy and Kotlin are absent because neither pinned grammar has a guard production at all: Kotlin 2.1 guard syntax does not parse, so there is nothing to classify (#1454). |
| Kotlin | A subject-less `when` arm scores through the predicate slot; a subject-ful arm scores per entry | A subject-less arm's condition (`when { x > 5 -> … }`) is an ordinary boolean expression, compiled as an `if` predicate, so the comparison inside it is already counted by the operator arms and a blanket per-entry increment double-counted it. A subject-ful arm (`when (x) { in 1..2 -> … }`) lists a pattern rather than an independent boolean expression, so the implicit `subject == pattern` is a decision nothing in the source spells and the entry itself pays for it (#1421). |
| Ruby | A subject-less `case` scores each `when` pattern as an `\|\|` operand; a subject-ful `case` scores per clause | A subject-less clause (`case; when x > 5`) lists ordinary boolean expressions, evaluated like an `if` predicate, and several patterns (`when a, b`) are an implicit `\|\|`. Each pattern therefore scores once unless its own operator already counted it, so `when x > 5` scores 1 like `if x > 5`, and `when a, b` scores 2 like `if a \|\| b`. Before this, a blanket per-clause increment counted a comparison twice. A subject-ful clause (`case x; when 1`) matches each pattern with an implicit `pattern === x` that nothing in the source spells, so the clause pays once (#1453). |
| Go | A tagless `switch` scores each case expression as an `\|\|` operand; a tagged `switch` scores per case | A tagless switch (`switch { case x > 5: }`, with or without an init statement) is `switch true`: each case expression is an ordinary boolean scored by the same slot an `if` predicate uses, and a list (`case a, b:`) is an implicit `\|\|`. So `case x > 5:` scores 1 like `if x > 5`, and `case a, b:` scores 2 like `if a \|\| b`. Before this, a blanket per-case increment counted a comparison twice. A tagged case (`switch x { case 1: }`) compares with an implicit `x == 1` that nothing in the source spells, so the case pays once; type-switch and `select` cases are unchanged (#1523). |
| Elixir | An `if` / `unless` predicate and each `cond` clause are condition slots; every other clause construct scores per clause; an `if` / `unless` `else` scores one | An `if` predicate scores one condition unless an operator inside it already counts it, as Ruby's does: `if x > 5` and `if b` each score 1, and `if a && b` scores 2, its two operands. `cond` is an `if` / `else if` chain written as clauses, so each clause's condition is scored the same way, and `cond do x > 5 -> …; b -> …; true -> … end` scores 2, like Go's tagless `switch` twin. The unguarded `true ->` catch-all is `cond`'s default arm and scores nothing, as in cyclomatic. Before this, the keyword call added one condition on top of its predicate, so `if x > 5` scored 2 and `cond` scored once however many clauses it had (#1527). The same change scores every `&&` / `\|\|` operand and guard that no operator counts, so `a && &f/1` now scores 2, like `a && b`. The clauses of a `case`, of a `with`'s `else`, of a `receive`, of a `try`'s `rescue` / `catch` / `else`, and of a multi-clause `fn` after its head each score one, through the same gate cyclomatic uses: a bare `_ ->` catch-all outside a `fn` scores nothing, like Go's `default:` and Rust's `_ =>`. A typespec's function type (`@spec f((any -> any)) :: list`) parses as a clause too and scores nothing in either metric; cyclomatic counted it before. The keyword forms (`case(x, do: (1 -> :a; …))`, `cond(do: (…))`, `with`'s `else:`) hold their clauses in the same parenthesised block a typespec does, so they are recognised by the `do:` / `else:` / `rescue:` / `catch:` key that holds the block, and score exactly as the `do … end` forms. A clause whose pattern an operator already counts does not pay again, so `rescue e in RuntimeError ->` scores 1, its `in`. So a two-clause `case` scores 2, like Ruby's subject-ful `case` and Rust's `match`; it scored 1 once per construct. Each `pattern <- expr` clause of a `with` scores one as well, because it either matches or falls through to the `else`, so `with {:ok, a} <- g(x) do a end` scores 1, like Rust's `if let Ok(a) = g(x)`. A `when` guard on that pattern is a guard slot like any other. A `for` generator's `<-` is not a clause and scores nothing. A `for … reduce:` block's accumulator clauses score like a multi-clause `fn`'s: the first is free, so a single `acc -> …` clause scores nothing (#1535). The `else` of an `if` / `unless`, in the block or the `else:` keyword form, is Fitzpatrick's Rule 5 `else` and scores one, as Ruby's, Java's, Python's and Go's do (#1531). A bracketed keyword list in last position (`if(c, [do: 1, else: 2])`) is the same argument as the bare one and scores the same, as does a quoted key (`"else": 2`); a list anywhere else, or one with a non-pair item, holds no sections. A `Kernel.&&` / `Kernel.\|\|` remote call scores its operands as `a && b` / `a \|\| b` does, the piped value of `a \|> Kernel.\|\|(b)` included (#1574), and so do `Kernel.and` / `Kernel.or` (#1582). A `receive` has one free arm, as a `case` does: its bare `_ ->` when the `do` section holds one, otherwise its `after` clause (the timeout, the path taken when no message clause matched in time), which then scores nothing; beside a bare `_ ->` the timeout is a path of its own and scores one (#1581). |
| C++, Objective-C, Mozcpp, Rust, Go, Java, Groovy, C#, Kotlin, TypeScript, TSX, JavaScript, Mozjs, Lua, Perl, Ruby, Bash, Elixir | A `<` or `>` that is not a comparison is not a condition | Every one of these grammars spells at least one non-comparison construct with the same bare `<` / `>` token a comparison uses, so the comparison rule is gated on the token's parent. What that excludes, per family: template and generic brackets in C++, Objective-C, Mozcpp, Rust, Go, Java, Groovy, C#, Kotlin, TypeScript and TSX (#1274); JSX tag delimiters in TSX, JavaScript and Mozjs (TypeScript carries the same gate through the shared macro, but the `.ts` dialect of tree-sitter-typescript has no JSX production, so it has nothing to exclude there); Lua 5.4 variable attributes (`local x <const> = 1`); a C# comparison-operator overload's declared name (`operator <`); Kotlin's qualified super call (`super<A>.g()`) (#1297); Perl's filehandle and lexical-handle readlines (`<FH>`, `<$fh>`, but not `<STDIN>`, which the grammar lexes as one token); Ruby's superclass clause (`class Foo < Bar`) and comparison-operator method names (`def <(other)`), and Bash I/O redirection (`cmd > out`) (#1280); Elixir's sigil delimiters (`~s<hi>`) (#1256). C# additionally excludes the operator of a relational pattern (`x is > 0`, and the `> 5 =>` arm of a switch expression): the arm or `if` condition slot that owns the pattern already scores the decision, so counting the operator too charged a relational arm twice what the constant arm `5 => 1` scores (#1383). Its `>=` / `<=` spelling is excluded by the same rule through a separate token. The declared name of an operator overload is excluded on every spelling too: C# overloads six comparison operators and gives each a distinct token, so `operator <=`, `operator >=`, `operator ==` and `operator !=` join `operator <` and `operator >`, which alone were excluded before #1420. C++ and Mozcpp do the same for their seven, `operator<=>` included, and also exclude the operator a comparison *fold* spells twice: `(0 == ... == a)` counts once, like `(a == ...)` (#1448). An `&&` / `\|\|` fold applies its operator once as well, and scores what one application of it scores: `(true && ... && a)` what `true && a` does, and `(... && a)`, whose unwritten side is one more operand, what `a && a` does (#1533). Their ISO alternative tokens `not_eq`, `not`, `and` and `or` count exactly as `!=`, `!`, `&&` and `\|\|` do (#1470). The exclusion is on the operator alone, not on the test that encloses it: since #1461 the `is` test itself is a condition wherever it is written, so `bool b = x is > 0;` and `return x is > 0;` each score 1, level with the `x > 0` they are sugar for, and the same 1 a `when n is > 5` guard has scored since #1422 made the guard a condition slot. Counting the pattern's operator as well would make a relational arm worth twice the constant arm `5 => 1`. PHP, Python, Tcl and iRules emit a bare `<` / `>` from no non-comparison production; C carries the same gate as its C-family siblings although, having no templates, it has nothing to exclude. The gate is a claim about the grammar's productions, not about every parse: where a grammar resolves a generic *call* into nested `binary_expression` nodes, as tree-sitter-kotlin-ng does for `id<Int>(a)`, no polarity can exclude it (#1394). The exclusion is ABC's alone: Halstead counts vocabulary rather than decisions, so `bca ops` still bills every one of these brackets as an operator (a type-enclosing pair as one `<>` since #1559 and #1567), and the two metrics are meant to disagree here (#1395); see [the Halstead delimiter rule](#halstead). |
| Java, Groovy, C#, TypeScript, TSX | A `?` used as type syntax is not a ternary | In each of these grammars the ternary `?` and the type-syntax `?` are the *same* anonymous token, so the ternary rule above is gated on the token's parent. Java and Groovy exclude the wildcard bound `List<? extends T>` (#1274); C# excludes the nullable type `int? x` and the constraint `where T : class?`; TypeScript and TSX exclude optional parameters, properties, methods, class fields and tuple elements, and conditional types (`T extends U ? X : Y`, which the type checker resolves and erases before runtime, so it is no more a branch than the `<` / `>` already excluded) (#1275). Safe navigation is untouched: C#'s `a?.b` shares the same token and still counts, while the other languages spell theirs as a distinct one. |
| C#, Java, Groovy, Kotlin, Ruby, Elixir, Perl | A relational operator scores by use; a value-bearing operand scores in a boolean slot | Five of these grammars spell at least one relational construct as its own production rather than as a binary expression with an operator token: C#'s `x is int` and `x is null`, Java's and Groovy's `x instanceof T`, Groovy's `a in l`, Kotlin's `a is T` and `a in 1..2`, and Ruby's one-line `a in Integer`. Having no token to count, each was reached only through the language's terminal-operand set, which the walker consults inside an `if` / `while` / ternary / `&&`-operand slot and nowhere else, so `var b = x == 1;` scored 1 while `var b = x is int;` scored 0. Fitzpatrick's Rule 5 counts a relational operator wherever it appears, so each now counts wherever it appears and has left the operand set; being in both would score it twice. Groovy's spaceship `<=>` counts on the same rule, as it already did in Ruby, PHP, C++ and Mozcpp, although its result is an integer rather than a boolean: what Rule 5 measures is the comparison, not its type. Elixir reached the same 0 by the other route: its membership and type tests (`a in [1, 2]`, `rescue e in RuntimeError`) *do* carry an operator token, and simply had no arm matching it. They count by use on the same rule, gated on the token's parent so that an operator merely *named* (`&in/2`) stays excluded alongside `<` and `>`, and, since #1531, alongside `==`, `===`, `!=`, `!==`, `<=` and `>=`, so `&==/2` and `Kernel.==(a, b)` score nothing. Perl's bare pattern match (`my $r = /^#/;`, or `m{^#}`) matches the implicit `$_` with no operator token at all, and scores by use on the same rule, level with `$x =~ /^#/` (#1467). Two patterns are excluded: the one bound by `=~` / `!~`, whose operator already counts, and the one `split` takes first (`split /,/, $s`), which is a delimiter rather than a match. A substitution or transliteration (`s///`, `tr///`, `y///`) edits its string and yields a count, so it is no test by use in either spelling: bare on `$_` or bound as `$x =~ s/a/b/`, it scores nothing outside a slot, and inside one the slot pays for it once (#1540). The converse still holds for a construct whose *value* fills the slot: a cast, a Go type assertion, a Rust `matches!` / `cfg!` macro and a Python walrus are all operands and count only where a slot reads them as a predicate (#1461). |
| Perl | A comparison's right operand is not a condition, although the grammar parses it as one | tree-sitter-perl 1.1.2 lets a chaining comparison (`<`, `>`, `<=`, `>=`, `lt`, `gt`, `le`, `ge`, `==`, `!=`, `eq`, `ne`) take everything of lower precedence to its right as its operand, so `$a > $b ? 1 : 2` parses as `$a > ($b ? 1 : 2)` and `$a > $b && $c` as `$a > ($b && $c)`. The ternary's condition slot and the chain's first operand then held `$b`, which scored as a condition of its own: one above the JavaScript twin for each misparsed comparison. The walker recognises the shape and leaves that operand to the comparison, so both spellings score what the parenthesised `($a > $b) ? 1 : 2` and `($a > $b) && $c` do. An explicit `$a > ($b ? 1 : 2)` is unaffected. `<=>`, `cmp`, `=~` and `!~` do not chain and parse correctly. The workaround goes when the grammar is fixed upstream (#1273). |
| Perl | An auto-quoted hash key is a string, not a call | Perl quotes a bareword before `=>` and alone in a hash subscript, a leading `-` included, so `(name => 1)`, `new(name => $x)`, `(-name => 1)` and `$h{-name}` hold the strings `"name"` and `"-name"`. tree-sitter-perl 1.1.2 parses each word as a call, which scored a branch, and each `-name` as a negation, which billed a Halstead `-` operator and the operand `name`. Each now scores as its quoted twin `('name' => 1)` does, and Halstead bills the key as one operand spelled as written: `name`, `-name`. A package-qualified word (`Foo::Bar => 1`), which `=>` does not quote, a word with arguments (`name() => 1`) and a bareword value (`(a => foo)`) keep their call. `bca find --type call` reports the same calls ABC counts (#1580). Nor does `nexits` count a `die` or `exit` key as an exit (#1585). The workaround goes when the grammar is fixed upstream (#1571, #1572). |

#### Where a constructor call lands {#abc-constructor-attribution}

Fitzpatrick's branch rule is "function invocation **or object
creation**", so object construction and constructor delegation count as
branches even though they are not call sites. They do not appear in
`bca find --type call`, which reports invocations only; see
[Semantic filters](commands/nodes.md#semantic-filters).

Which *space* a constructor call is charged to depends on how it is
spelled, and C# 12 and Kotlin both let you spell it two ways:

```csharp
class Sub(int x) : Base(x) { }            // charged to the class space
class Classic : Base {
    public Classic(int x) : base(x) { }   // charged to the constructor
}
```

A primary constructor has no body. Its superclass call sits in the class
header (C#'s `base_list`, Kotlin's `delegation_specifiers`), outside
every member, so no function space encloses it and the branch belongs to
the class. The classic spelling puts the same call inside a constructor
declaration, which does open a function space.

File-level `abc.branches` is identical either way, so nothing shifts at
the aggregate. Per-function figures do: `branches_max`,
`branches_average`, and a per-space `bca check --threshold abc=N` all see
the primary-constructor form on a class row and never on a function row.
A codebase migrating to primary constructors will therefore see its
per-function ABC distribution move without any behaviour change. That is
the intended reading: there is no function to attribute the call to.

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
| `&&` | (none) | Logical operator; does not contribute on its own. |
| `if`/`else` | C += 1 | `else` keyword (Rule 5) |
| `c = '/'` | A += 1 | Assignment (Rule 1) |
| `c = 'C'` | A += 1 | Assignment (Rule 1) |

Total: `<A,B,C> = <2, 0, 3>`, magnitude `√13 ≈ 3.61`.

If the same body is rewritten with a unary conditional:

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
Operator-counting choices cause ABC results to differ across tools on
the same source.

### Algorithm

The metric walk passes syntax-tree nodes to the language's `Abc`
implementation. It counts assignments, calls and conditions,
inspecting child expressions where a construct requires it. The four
headline values are:

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
language in `src/metrics/abc/<lang>.rs`, matching on the node kinds
generated into `big-code-analysis-ast/src/languages/language_*.rs`.

### How to read it

ABC is a *size* metric, not a complexity metric: a long, dull
function with no decisions still scores high if it does many
assignments. Fitzpatrick's original recommendation was to use the
magnitude as a relative ruler: rank a file's functions by ABC
magnitude and look at the top decile.

In practice the Ruby community adopted ABC most widely; the
[`rubocop` linter](https://rubocop.org/) and the
[`flog` tool](https://github.com/seattlerb/flog) both default to
threshold-based warnings. A Ruby method with an ABC magnitude over
about 17 is conventionally a refactoring candidate; over 30 is
considered hard to maintain. Those thresholds are language-specific.
Expect higher values in C++ and Java, which use explicit getter/setter
assignments more often.

## Cognitive Complexity {#cognitive-complexity}

G. Ann Campbell at [SonarSource](https://www.sonarsource.com/)
introduced **Cognitive Complexity** in the 2017 white paper
[*Cognitive Complexity: A new way of measuring
understandability*](https://www.sonarsource.com/docs/CognitiveComplexity.pdf)
and the follow-up IEEE TechDebt 2018 paper [*Cognitive Complexity: An
Overview and Evaluation*](https://ieeexplore.ieee.org/document/8595102/).

The metric was designed as a replacement for Cyclomatic Complexity in
code-quality tooling. Campbell's argument is that cyclomatic
complexity measures how hard code is to *test*, not how hard it is to
*understand*: a 1024-arm `switch` statement scores the same as a
deeply nested chain of `if`s that perform identical logic, yet a human
reader finds the nested code much harder to follow.

### Algorithm

Cognitive Complexity starts at zero and applies three rules as it
walks the tree:

1. **Ignore "shorthand" control flow.** Constructs that route to a
   single block (a top-level `if` with no nesting, an `else` without
   conditions of its own, the head of a `for`, a `?:` ternary) add a
   baseline `+1` each, but they do not punish you for the pattern.
2. **Penalise breaks in linear flow.** Conditionals, switches, loops,
   `catch` clauses and labeled jumps add increments under the rules
   below. A `try` block alone adds nothing.
   big-code-analysis does not score the recursive-call increment in
   any language: it performs no call resolution, so a recursive call
   costs nothing (see the per-language deviations below).
3. **Punish nesting.** Every time control flow appears *inside* an
   already-nested block, the metric adds an extra `+1` *per level of
   nesting*. An `if` inside a `for` inside an outer `if` inside a
   method scores `1 + 2 + 3 = 6`, where a flat sequence of the same
   three constructs would have scored `1 + 1 + 1 = 3`.

Sequences of identical boolean operators (`a && b && c`) score `+1`
for the whole run, on the grounds that a chain of `&&`s is no harder
to read than a single `&&`. Switching operators (`a && b || c`) is
where the cognitive load jumps, so the second operator earns its own
`+1`. "Identical" means the same logical operation, not the same
token: where a language spells conjunction and disjunction both ways
(C++, Ruby, PHP, Perl, Elixir, iRules), `a && b and c` is one
sequence. Precedence still decides what nests where, so Ruby's
`a || b and c` is `(a || b) and c` and scores two. `??` and Perl's
`//` are operations of their own; `xor`, like `^`, is no sequence.

big-code-analysis exports the per-space structural score as `value`
along with the subtree `sum`, `min`, `max`, and a per-function
`average`. The implementation is in `src/metrics/cognitive.rs`.

### How to read it

A Cognitive Complexity of `0` means the function is purely linear: no
branches, no loops. SonarSource's tooling defaults to flagging
functions above `15` as "too complex", and Campbell's recommendation
in the white paper is that a function should rarely exceed about
`25`. Unlike Cyclomatic Complexity, the metric scales smoothly:
deeply nested code scores well above flat code with the same number
of decisions.

The common use is **refactoring guidance during code review**:
because the metric penalises nesting specifically, it flags the kind
of function that benefits from an early-return or "extract method"
refactor. [SonarLint](https://www.sonarsource.com/products/sonarlint/)'s
IDE plugins (IntelliJ, VS Code, Visual Studio, Eclipse) all surface it
as the headline complexity number on hover, and several language
servers and code-review platforms outside the Sonar ecosystem have
since adopted it.

### Per-language deviations

- **No language** scores the recursion increment (B3): the metric
  walk resolves no calls, so a function calling itself costs nothing
  beyond its explicit control flow.
- **Elixir** does not score jump statements either. Elixir control
  flow (`if` / `unless` / `cond` / `case` / `with` / `receive`) is
  built from macro-shaped `Call` nodes rather than dedicated grammar
  productions, and the language has no `break` / `continue` / `goto`.
  The implementation therefore scores only the nesting-bearing
  constructs it can identify and omits the unstructured-jump (B2)
  increments that the SonarSource specification adds for languages
  that expose those shapes syntactically. A `with` expression's `else`
  clauses act like `case` arms. The `with` expression already
  contributes the nesting increment, so its `else` adds nothing, just
  as Ruby's `case`/`else` does. An `if` / `unless` / `try` `else`
  still adds +1. The keyword forms (`else:`, `rescue:`, `catch:`, bare or
  bracketed) score as their `do … end` blocks do, and
  `Kernel.||(a, b)` is a boolean sequence like `a || b` (#1574), as
  `Kernel.or(a, b)` is like `a or b` (#1582). A `receive` is a `case`
  over the mailbox and pays +nesting as one; its `after` timeout adds
  nothing, and a `receive` holding only an `after` is a timed wait
  that scores nothing (#1581).
- For every language with a syntactic function-definition node, a
  nested function (a local function, or a method on a local or inner
  class) **resets the nesting counter to zero** at its boundary and
  adds a function-depth surcharge, so control flow inside it is
  scored against the nested function's own depth rather than the
  enclosing function's nesting. Byte-equivalent constructs therefore
  score identically across languages.
- A lambda or closure (`x -> …`, `|x| …`, `lambda x: …`, a Ruby
  block, an Objective-C block) is not a function boundary in that
  sense. It adds a surcharge *on top of* the enclosing nesting
  instead of replacing it, so a decision inside a lambda written
  inside an `if` is charged for both.
- That surcharge stops at the next function boundary. A function
  *declared inside* a closure body opens a fresh lexical scope, so it
  does not inherit the closure's surcharge: `fn g` inside a `|| { … }`
  scores what it would score outside one. Until #1187 only the
  JavaScript family applied this, so the same body scored differently
  in Rust, Java, C++, PHP and C# depending on whether something two
  levels up happened to be a closure. Python cannot express the shape
  at all: a `def` is a statement and a lambda body is a single
  expression.
- **Rust**'s `let`-`else` (`let Some(a) = y else { continue };`)
  postdates the specification. It is scored as the `if let` it
  replaces: `+1` plus the current nesting, with everything under it,
  the `else` block included, one level deeper. Its `else` token does
  not pay again. A `for` holding it scores 3, the same as one holding
  `if let Some(a) = y { … }`. Before #1548 the `let`-`else` paid a flat
  `+1` through its `else` token whatever its depth, so that `for`
  scored 2.
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

Cyclomatic Complexity is the original software complexity metric.
Thomas J. McCabe introduced it in 1976 in [*A Complexity
Measure*](https://www.literateprogramming.com/mccabe.pdf) (IEEE
Transactions on Software Engineering, SE-2(4), pages 308–320).

McCabe's idea was to apply graph theory to the *control-flow graph*
of a function. If you draw every basic block as a node and every jump
between blocks as an edge, the cyclomatic number of that graph is

```text
M = E − N + 2P
```

where `E` is the number of edges, `N` the number of nodes, and `P`
the number of connected components. `M` counts **linearly independent
control-flow paths**. Basis-path testing uses this count; branch
coverage alone can require fewer tests.

### Algorithm

big-code-analysis does not build a control-flow graph. Instead it
uses the equivalent, much cheaper formulation McCabe proved in the
1976 paper for structured programs:

> *Cyclomatic Complexity = 1 + (number of decision points)*

A "decision point" is any node where control can branch:

- `if`, `else if`, ternary `?:`
- `case` / `when` arms in `switch` / `match` / `select`
- `while`, `do … while`, every variant of `for`
- exception-handler `catch` clauses
- short-circuit boolean operators `&&` and `||`, including their
  word spellings (`and` / `or` in C++, Python, Ruby, Lua, Perl, PHP,
  Elixir and iRules)

A `&&` or `||` counts only where it applies the operator. C++ also
spells the token in rvalue references (`int&& x`), the ref-qualifier
(`void f() &&`), overload names (`operator&&`) and `requires`-clause
constraints, and none of those is a decision (#1525). Nothing inside a
`requires` clause or a `requires` expression is a decision, written
with parentheses or without (#1533); cognitive complexity and ABC
conditions skip it too, though a call in it is still an ABC branch.
Any other compile-time expression still counts, as `if constexpr`
does: `static_assert(A && B)`, and a concept body
`concept C = A && B`.

The per-language `Cyclomatic` trait, in `src/metrics/cyclomatic.rs`,
asks each tree-sitter node "are you a decision?" and increments the
counter. The metric is rolled up per function and per file; per-class
aggregation across method bodies is provided separately by
[WMC](#wmc) below.

### Modified cyclomatic

big-code-analysis also reports a **modified** variant that collapses
all `case` / `match` / `when` arms inside a *single* switch
statement into one decision point, regardless of how many arms it
has. This undercounts big dispatch tables in a way that often matches
developer intuition better than the strict McCabe definition: a
30-arm `enum` dispatch reads as one decision, not thirty. The
convention is not original to this project; it echoes the `-m` mode
of Terry Yin's [lizard](https://github.com/terryyin/lizard) tool,
where many readers will first have seen it. Both numbers are exported
side by side; pick one and be consistent.

Only alternatives collapse. A sequence of tests is still one decision
per test in both variants. Elixir's `with` is the case where the two
meet: each `pattern <- expr` clause is a separate test, scored once in
both variants, as Rust's `if let` and `?` are. The clauses of its
`else` are alternatives, so modified scores them once, and only when
one of them is a decision: `else _ -> 0` selects nothing, like a plain
`else`, and is free in both variants (#1535). Rust's `let`-`else`
(`let Ok(a) = g(x) else { return 0 };`) is one such test too. It binds
or diverges, so it scores one in both variants, as its `if let` twin
does. Before #1542 it scored nothing.

Elixir's `receive` is a `case` over the mailbox, and modified scores
its clauses once, as it does a `case`'s. Like a `case` it has one free
arm: its bare `_ ->` when the `do` section holds one, otherwise its
`after` clause (the timeout, the path taken when no clause matched in
time). So `receive do {:a, y} -> y after 10 -> :t end` scores one
decision in standard, as the two-clause `case` does, and
`receive do {:a, y} -> y; _ -> 0 after 10 -> :t end` two: beside the
catch-all the timeout is a path of its own. A `receive` holding only an
`after` is a timed wait and scores nothing at all (#1581).

### Counting Rust's `?` operator

By default Rust's `?` operator (the `try_expression` grammar node)
adds `+1` to both standard and modified cyclomatic, matching upstream
[rust-code-analysis](https://github.com/mozilla/rust-code-analysis):
`?` is an early-return branch. When cyclomatic is
used as a maintainability *gate*, this can over-penalise linear but
fallible code that threads a handful of `?` through a happy path. You
can opt out so `?` is treated as linear error propagation:

- Library: `MetricsOptions::default().with_count_cyclomatic_try(false)`.
- CLI: `--cyclomatic-count-try=false` (or the deprecated
  `--no-cyclomatic-try` alias), or `cyclomatic_count_try = false` in
  `bca.toml` (the CLI value overrides the key in either direction).
- A repo gate: set `cyclomatic_count_try = false` in the
  auto-discovered `bca.toml`. This project's own `make self-scan` does
  exactly this, with no flag or env var. Toggling the policy shifts
  cyclomatic values, so regenerate `.bca-baseline.toml` in the same
  change.

The default is unchanged (`?` keeps counting), so published metric
values are preserved. The toggle is Rust-only; no other language emits
`try_expression`.

### How to read it

McCabe's original recommendation, repeated in the 1976 paper and
preserved by [NIST's *Structured Testing*
report](https://www.nist.gov/publications/structured-testing-testing-methodology-using-cyclomatic-complexity-metric)
(Special Publication 500-235, 1996), is to treat `10` as the upper
bound for a single function: above that, the number of test cases
needed for branch coverage grows uncomfortably large.

Cyclomatic complexity is used for three things:

1. **Defect prediction.** Complexity correlates, imperfectly, with
   the *probability* of a function containing a bug, and most
   static-analysis tools flag high-CC functions as risky.
2. **Test planning.** CC measures the size of a basis-path test set,
   not the minimum test count for branch coverage.
3. **Refactor triage.** Cyclomatic Complexity is the headline
   "complexity" number in almost every code-quality dashboard,
   often as a tie-breaker between two functions that look similar
   in length.

The metric's known blind spot is that it treats every decision as
equal weight. A 30-arm `switch` over an enum and a function with two
nested `if`s each containing nested `if`s both score around 30, even
though they read very differently. Cognitive Complexity (above) was
designed to fix exactly that.

## Halstead {#halstead}

The **Halstead suite** is the oldest size-and-effort metric family on
this page. Maurice H. Halstead introduced it in his 1977 book
[*Elements of Software
Science*](https://openlibrary.org/books/OL4535482M/Elements_of_software_science)
(Elsevier, ISBN 0-444-00205-7); the
Wikipedia page on [Halstead complexity
measures](https://en.wikipedia.org/wiki/Halstead_complexity_measures)
summarises the formulas. Halstead's aim was a quantitative, empirical
*science of software*, in the same way that physics is the empirical
science of matter.

### The four base counts

Halstead reduces a program to its tokens, then partitions them into
two categories:

- **Operators**, anything that *does* something: keywords (`if`,
  `return`, `while`), arithmetic and logical operators, assignment,
  function-call syntax, punctuation that controls flow.
- **Operands**, anything that *is* something: identifiers and
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
basis. Punctuation counts as an operator: a bracket pair is one
operator, billed once at its opener and listed by `bca ops` as `()`,
`[]` or `{}`, and `,`, `;`, `.` and `:` count once per occurrence.
Closing brackets, comments and a literal's own delimiters count as
nothing, which is why the Halstead totals are *not* the same as the
Tokens count.

The following rules define which syntax contributes to Halstead's
vocabulary. Context matters because grammars reuse punctuation for
operators, literal delimiters and other syntax:

- **A literal's own delimiters are not operators.** A JavaScript
  regex, a Groovy slashy string, a C++ raw string, a Tcl braced word
  and a Perl or Ruby pattern each contribute *one operand*, the
  literal, and no operator for the punctuation around it. Otherwise
  the score would move with the author's choice of delimiter, which
  says nothing about the code. The same holds for punctuation the
  grammar spells *inside* a literal: Perl's bare `/[a]#/` bills no `[]`
  or `#`, matching `m/[a]#/`, whose text the grammar does not split
  into tokens (#1549). Tcl and iRules need one extra step to
  honour this, because their grammars spell a braced literal and a
  braced script body the same way. `{a b}` is a block or a quoted
  value depending on the command it is passed to, so the classifier
  reads that command's name. `eval {…}`, `uplevel`, `after`, `time`,
  an `on` or `trap` handler clause written outside a `try`, and Tcl's
  `for` and `switch` take scripts and keep their `{}` operator, as
  does every construct the grammar models with a node of its own
  (`proc`, `if`, `while`, `foreach`, `catch`, `try`, `namespace`, an
  iRules `when` handler). Every other command (`lappend`, `puts`,
  `list`, and any user-defined proc) is taken to receive a value, so
  its braces score no operator.

  Recognising the command is only half of it, because most of those
  signatures mix the two roles. A braced word in a slot the command's
  documented syntax reserves for a *value* scores no operator either,
  however script-like the command around it: `after`'s delay
  (`after {100} {puts hi}` scores the one `{}` its script earns, the
  same as `after 100 {puts hi}`), `time`'s iteration count, a braced
  `proc` name (`proc {my proc} {} {}`), a defaulted `proc` parameter
  (`proc p {a {b {x y}}}`), `namespace export`'s pattern list,
  `namespace ensemble create -map`'s dictionary, and a `trap` or `on`
  handler's error code and variable list. The slots come from the
  [Tcl 8.6 manual pages](https://www.tcl-lang.org/man/tcl8.6/). The
  two `proc` cases are positions the grammar names, so the classifier
  reads them off the tree; the rest are recorded as per-command
  signatures in
  `big-code-analysis-ast/src/lang_helpers/tcl_family.rs`. Where a
  command's argument list does not match the signature (a top-level
  `try … trap … finally` parses its whole tail as one `trap` command,
  five arguments rather than three), no position in it is read, and
  every brace stays a block.

  This decides the *operator* only. The words inside a braced argument
  are counted either way, because an unrecognised command is as likely
  to have been handed real code (an `oo::class create C {…}` body, a
  `tcltest` `-body {…}`) as a list. So a script passed to a command
  outside the list (a Tk `-command {…}` callback, a `dict for` body)
  gives up one `{}` occurrence per block and nothing else, with one
  visible consequence: a space whose *only* blocks are of that kind
  has no operator left, and because Halstead's difficulty multiplies
  by the operator count, its `effort` reads `0.0` rather than slightly
  low. A `proc` keeps its own `proc` keyword and body brace, so this
  reaches top-level script fragments, not functions.

  The consequence for the operand column is an asymmetry, and it is
  the contract rather than an open defect: a braced value scores one
  operand where the grammar names the command (`set x {a b}` → the
  operand `{a b}`) and one per word where only the command name would
  (`lappend x {a b}` → the operands `a` and `b`). Both ways of
  removing it were weighed and neither is available. Deciding from the
  *contents* (no substitution, no nested block, therefore a list) was
  built and measured: on a file holding an `oo::class` body, a
  `tcltest -body` and an `apply` lambda it took n2 from 25 to 15 and
  N2 from 32 to 15, each of those three collapsing into a single
  operand. And listing the value-taking commands instead needs an
  *open* set where the script-taking one is closed, since every user
  proc belongs to it, so the list would be incomplete by construction
  and would make three behaviours where there are two.
- **A string-interpolation opener is not an operator.** `"{$x}"` in
  PHP, `"#{x}"` in Ruby and Elixir, `"${x}"` in Kotlin and Groovy,
  `$"{x}"` in C#, `f"{x}"` in Python and Java 21's `STR."\{x}"` all
  count the interpolated expression's own operators and nothing for the
  opener itself, and the wrapping literal is not an operand either.
  Before #1703, Java counted the wrapping string template as an
  operand as well as the interpolated `x`. The `:` that
  opens a Python format spec (`f"{x:>10}"`) counts nothing either,
  because the spec after it is string content (#1486). Nor does the
  self-documenting `=` of `f"{x=}"`, which prints the expression's
  text and assigns nothing (#1551). C#'s format `:` and alignment `,`
  (`$"{x,8:F2}"`) count nothing for the same reason (#1573), but the
  alignment is an expression and keeps its own vocabulary: `8` is an
  operand, and `{x,-w}` counts `-` and `w`. A Python literal width
  (`f"{x:>10}"`) is format-spec text and counts nothing.
- **A delimiter around *syntax* is an operator, whatever it delimits.**
  The rule above is about a literal's own punctuation, and it stops
  there. Halstead counts the vocabulary a program is written in and has
  no notion of a decision. Halstead uses the same `<` vocabulary entry
  for comparisons, JSX tag delimiters, Lua `<const>` attributes, C#
  `operator <` declarations and Perl `<FH>` readlines, and `bca ops`
  reports every one of them. This is the
  deliberate counterpart to
  [the ABC deviation](#abc-per-language-deviations) that excludes the same
  tokens from `conditions`: ABC asks whether a branch is taken, and
  these are not branches; Halstead asks what alphabet the source uses,
  and these are part of it. Expect the two metrics to disagree here.
  The same reading bills Python's `(`, `[`, `{`, `;` and block,
  slice, dict and annotation `:`, and Rust's annotation, field, bound
  and label `:`, which is a separate operator from `::`. Both languages
  left these out until #1486, so their Halstead scores rose with it.
- **A declaration keyword is an operator.** Python's `class` is billed
  like `def` since #1550, as Ruby, Kotlin, C# and PHP bill their
  `class`. Java, Groovy, the JavaScript family, C, C++ and Objective-C
  followed in #1552 with their type-declaration keywords: `class`,
  `struct`, `union`, `enum`, `interface`, `record`, `trait`,
  `@interface`, `@implementation` and `@protocol`. The `class` tail of
  Java's `C.class` is an operand, not this keyword (see the class
  literal below). Groovy's `var` bills as its `def` does since #1704,
  once per local declaration however many declarators follow it. A
  `var` used as a name stays an operand (`def var = 3`, `[var: 1]`),
  or bills nothing where the grammar cannot parse the statement
  (`var = 4`), and the `var` of a field (`var field = 1`) or of
  `final var`, which the grammar reads as a type name, still bills as
  a type-name operand (#1706). Kotlin's `interface` followed in #1556.
  #1557 added C++'s `namespace`, `template`, `typename`, `typedef` and
  `using` (C and Objective-C bill `typedef` too), so
  `template <class T>` and `template <typename T>` each bill their
  keyword and score alike. It also added TypeScript's `type`,
  `namespace`, `module`, `declare` and `global`. #1561 made
  TypeScript's type-level operator keywords
  operators, as their value-level siblings `as`, `typeof` and `extends`
  already were: `satisfies`, `keyof`, `infer`, and the `is` and
  `asserts` of a type predicate. #1564 made Kotlin's `typealias`,
  `init`, `where`, `by`, `get` and `set` operators, as C# bills its
  `get`, `set`, `init` and `where`. The `@get:` and `@set:` annotation
  targets bill nothing, like `@field:`. The backing `field` in an
  accessor is an operand, and the words used as names (`val by = 1`,
  `x.get(i)`) stay operands.
- **A declaration modifier is an operator, keyed by its keyword and
  never its wrapper node.** C#, Kotlin, Java, Groovy, TypeScript and
  TSX, C, C++, Mozcpp and Objective-C bill every modifier once, as the
  keyword and never as the node that wraps it: Kotlin's and Java's
  `modifiers`, TypeScript's `accessibility_modifier` and
  `override_modifier`, and the C family's `storage_class_specifier`,
  `type_qualifier` and `virtual_specifier`. So the `const` of
  `void f() const` and of `int * const p` each bill once. C# already
  did; the others followed in #1558 (Kotlin), #1563 (TypeScript), #1565
  (Java and Groovy), #1578 (C, C++ and Mozcpp) and #1579
  (Objective-C). The exceptions:
  - The same word used as a name stays an operand (`val open = 1`,
    `let readonly = 1`, `obj.static`, `var sealed = 1`,
    `int override = 1;`). Where Kotlin's grammar cannot parse the name
    (`open = !open` at the start of a statement, `val suspend = 1`, a
    parameter named `vararg`), the word bills nothing.
  - The `static` of `import static` and `requires static` bills
    nothing, as `import` does not: Java and Groovy bill no directive
    keyword (`package`, `import`, `module`, `requires`, `exports`),
    and the word a directive qualifies follows it. C# is the outlier
    here: it bills `using`, and so `using static` in full. Each
    reading is consistent with the language's own directive rule, and
    #1600 kept both rather than move one.
  - C++'s `auto` is a placeholder type and stays unbilled. Attributes,
    the MS calling conventions, the Gecko `MOZ_*` macros and the Apple
    SDK macros (`NS_INLINE`, `FOUNDATION_EXPORT`, `IBOutlet`) bill
    nothing.

  The words each language bills:

  - Kotlin: the `enum` of `enum class`, `data`, `sealed`, the
    visibility keywords, `open`, `override`, `suspend`, `reified`,
    `out` and the rest.
  - TypeScript and TSX: the member modifiers `public`, `private`,
    `protected`, `override`, `readonly`, `abstract`, `accessor` and
    `static`. JavaScript and Mozjs bill `static` too. Their grammars
    lex `static get` followed by a newline as one `static get` token,
    which bills as one operator, so that spelling scores one entry
    where `static get y()` on one line bills `static` and `get`.
  - Java and Groovy: the visibility keywords, `static`, `native`,
    `strictfp`, `volatile`, `sealed` and `non-sealed`, where before
    #1565 they billed only `abstract`, `final`, `transient`,
    `synchronized` and `default`; plus the `permits` clause keyword
    beside `extends` and `implements`.
  - C, C++ and Mozcpp: every storage class, function specifier and
    type qualifier: `static`, `extern`, `register`, `inline`,
    `thread_local`, `const`, `volatile`, `restrict`, `_Atomic`,
    `constexpr`, `alignas` and their vendor spellings (`__inline`,
    `__forceinline`, `__restrict__`, `__thread`, `__volatile__`,
    `__extension__`, …), plus C++'s `virtual`, `mutable`, `constinit`,
    `consteval`, `explicit`, `friend`, the access labels, `override`,
    `final` and `noexcept`. C bills its storage class `auto`.
    `extern "C"` bills `extern` and keeps `"C"` an operand.
  - Objective-C: C's list, plus the `@public`, `@private`, `@protected`
    and `@package` ivar labels, the `@optional` and `@required`
    protocol sections, the ownership qualifiers (`__weak`, `__strong`,
    `__block`, …), the nullability qualifiers (`nullable`,
    `_Nullable`, `__nullable`, …), `__kindof`, the `__covariant` and
    `__contravariant` variance, and the `in`, `out`, `inout`, `bycopy`,
    `byref` and `oneway` method-type qualifiers. A property attribute
    (`nonatomic`, `copy`, `getter`) bills as its word; a `getter=` or
    `setter=` value stays an operand.
- **A type name is an operand.** #1557 made every TypeScript type name
  an operand (`x: Foo`, `interface I`, `Map<K, V>`), as C#, Rust and
  the C family already did. Java followed in #1560: `Foo f`,
  `implements I`, `List<Foo>` and each segment of `java.util.List`
  bill their names as operands. The inferred type `var` does not, as
  C# bills nothing for its own. Java's primitive types are operators
  everywhere. Groovy, whose grammar parses a primitive type as a name,
  bills it the same way since #1568: `int`, `boolean`, `void` and the
  other primitives are operators keyed by their spelling, as a
  declared type and as a class literal's receiver. Groovy reserves
  these words, and they bill as operators; in the positions Groovy
  nonetheless allows them as names (`x.int`, `[int: 1]`), they stay
  operands. Java's `boolean` is listed as `boolean` rather than its
  node kind `boolean_type`, so both languages report it alike.
- **A class literal bills its parts.** Since #1566 a Java class
  literal bills its parts rather than one operand spelled by its whole
  text, since
  [JLS §15.8.2](https://docs.oracle.com/javase/specs/jls/se21/html/jls-15.html#jls-15.8.2)
  defines it as an expression rather than a literal: in
  `java.util.List.class` and `Foo[].class` each name and the `class`
  tail are operands and each `.` and `[]` an operator, as Groovy's
  `Foo.class` and Kotlin's and PHP's `Foo::class` bill theirs. The
  `int` of `int.class` and the `void` of `void.class` stay operators,
  as Java bills them everywhere.
- **Angle brackets that enclose types are one bracket pair.** The `<`
  and `>` around type arguments and type parameters count as one `<>`
  operator, billed at the `<` as `()` is at the `(`, and the `>` counts
  nothing (#1559). That covers `Vec<u8>`, `fn f<T>`, a turbofish
  `h::<u8>()`, `for<'a>`, `use<T>` and a qualified path's
  `<T as Trait>::` or `<T>::` in Rust; C++'s
  `template <class T>` and `std::vector<int>`; and the type-argument
  and type-parameter lists of Java, Groovy, C#, Kotlin, TypeScript and
  TSX, plus C#'s `delegate*<int, void>` pointer type and Kotlin's
  `super<A>` (#1567). Objective-C has four generic lists:
  `NSArray<NSString *>`, `id<NSCopying>`, the protocol list in
  `@interface A : NSObject <NSCopying>`, and a type passed as a call
  argument, `f(NSArray<NSString *>)`. A comparison's `<` and `>` and a
  shift's `<<` and `>>` stay separate operators, and a C++ `>>` that
  closes two template lists parses as two `>`, so it bills nothing.
  The rule follows the parse: where tree-sitter-cpp reads comparisons
  as a template argument list, as in `x = a < b || c > (d)`, their
  brackets bill as `<>` too, since only name lookup tells that from a
  template call spelled the same. The comparisons Objective-C and C#
  misread as a call argument's generic list, `g(a<b, c>d)` and
  `F(a < b, c > d)`, keep `<` and `>`. The exception is C#'s `>=`
  spelling, `F(a < b, c >= d)`, which the grammar parses as an
  assignment and which still bills `<>` and `=`. Go's generics use
  square brackets and already billed `[]`. Angle brackets around
  anything other than types still bill `<` and `>`: the JSX, Lua and
  Perl constructs above. So does a Kotlin generic call with arguments,
  `id<Int>(a)`, which tree-sitter-kotlin-ng parses as two comparisons
  (#1394), and a generic inside a Rust macro call,
  `vec![Vec::<u8>::new()]`, whose token tree tree-sitter-rust leaves
  unparsed; a `>>` closing two lists there bills as the shift.
  Before #1559 each bracket billed under the comparison's own entry,
  so `Vec<u8>` and `a < b` shared one. Every generic now costs
  one operator occurrence less, so `N1` falls in any code that uses
  generics. `n1` rises by one where a comparison `<` or `>` remains
  beside the new `<>`, so `halstead.effort` falls in most files and
  rises in the rest; on the C++ and Rust corpora it fell in 87% of the
  files that moved, and MI rose in most of them.
- **An alternative spelling is an operator of its own.** C++ spells
  `&&` equally as `and`, `!=` as `not_eq` and so on through all eleven
  [alternative tokens](https://en.cppreference.com/w/cpp/language/operator_alternative).
  Each counts once in `total_operators`, exactly like its symbol, but
  `a && b and c` has two entries in `unique_operators` where
  `a && b && c` has one. Operators are distinct by token, and `bca ops`
  lists `and` and `&&` separately, so the metric matches what it
  reports (#1522).
- **A self-reference is an operand.** `this`, `self`, `$this`,
  `super` and `base` name the receiver of a member access, so
  `this.x` counts the way `p.x` does: one `.` operator and two
  operands. Java, Groovy, C#, Kotlin (labelled `this@Outer` included),
  Python, Rust and PHP all agree. The keyword is an operator only where
  it *declares* something, as the `operator` keyword of an overload
  does: C#'s indexer `public int this[int i]`, and the wildcard bound
  `? super T` in Java and Groovy (#1380).
- **A keyword's role follows its text, not its node kind.** C#'s
  grammar parses the bare `this`, `ref`, `out`, `in`, `scoped` and
  `readonly` of a parameter, and the `static` / `async` of a lambda,
  as a `modifier` node, a different kind from the one the same
  keyword has anywhere else. Each keeps the role its text has
  elsewhere: an extension method's `this Foo f` is an operand, the
  `ref` in `M(ref int a)` is the same operator as the `ref` in the
  call `M(ref x)`, and `static (int x) => x` bills the same `static`
  as `static void M()`. A repeated operator keyword adds to
  `total_operators` but not to `unique_operators` (#1418, #1482).
- **Perl's `__END__` and `__DATA__` end the program.** Neither marker
  nor the data section after it is an operator or an operand, so two
  files that differ only in their data section score alike. The
  grammar holds the marker and every byte after it in the same
  `special_literal` node it uses for `__FILE__`, which until #1644
  billed the whole section as one operand spelling its contents, and
  billed each of `__FILE__`, `__LINE__`, `__PACKAGE__` and `__SUB__`
  twice, once as that wrapper and once as the keyword inside it. Each
  constant is now one operand, billed once. The marker and the opaque
  data leaf are still leaves, so `tokens` still counts them.
- **JSX element text is an operand.** In JavaScript, Mozjs and TSX,
  the `hi` of `<p>hi</p>` counts the way a string's contents do. It is
  keyed on its text with leading and trailing whitespace trimmed, so a
  line of text at two indentation depths is one operand, and the
  spaces between two tags on one line count as nothing. Whitespace
  *inside* a multi-line text is kept. A character reference (`&amp;`)
  splits the text into separate operands (`a &amp; b` is three).
  Inside an attribute string, the reference is part of that string's
  one operand (#1483).
- **An overloaded operator's name is an operator.** The `+` of
  `operator +` was always one, and the names a grammar spells as whole
  tokens now agree: C#'s `operator true` / `operator false`, and C++'s
  `operator[]`, `operator()`, `operator""_x` and `operator co_await`.
  The `operator` keyword is an operator in both languages, as is C++'s
  `co_await` in an expression. C++'s `[]` and `()` names are tokens of
  their own, so `bca ops` lists `operator[]` beside a subscript's `[]`
  as a second operator, as Ruby does for `def [](i)` (#1296). C++'s coroutine keywords `co_return` and `co_yield` are
  operators as `return` is (#1547).
- **Only written tokens count.** When tree-sitter cannot parse a
  construct, its error recovery may insert zero-width tokens the source
  does not contain: a `;`, a `)`, an identifier. None of them is billed
  in either half. Before #1546 an inserted identifier became an operand
  whose text is the empty string. One case is a C++ destructor called
  through a template name (`p->~T<A>()`), which tree-sitter-cpp cannot
  parse. Another is a statement-position `typename` cast, which made
  every inserted `;` a second `;` operator.
- **A named operator is one operand.** Elixir names an operator in a
  capture (`&==/2`, `&and/2`) and as the function of a remote call
  (`Kernel.||(a, b)`), and its grammar wraps the token in an
  `operator_identifier`. That wrapper is the operand, as `foo` is in
  `&foo/2`, so the token inside it adds no operator; `&==/2` scores
  what `&foo/2` does. A captured `&&` / `||` / `and` / `or` is no
  decision either (#1534), but the remote call applies the operator,
  so `Kernel.||(a, b)` scores what `a || b` does in cyclomatic, ABC and
  cognitive alike, and a user module's `Foo.||(a, b)` in none (#1574).
  `Kernel.and(a, b)` and `Kernel.or(a, b)` decide the same way, though
  their names are plain identifiers with no operator token (#1582). This
  differs from a C# `operator <`, whose `<` has no wrapper and so stays
  the operator the rule above makes it.

### Derived metrics

Halstead then derives a set of formulas. big-code-analysis reports
all of the standard ones, plus three less common derivations
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
Algol-family languages. The `T = E / 18` "Stroud number" comes from
psychology instead: Halstead borrowed John Stroud's estimate that the
human mind makes about 18 elementary discriminations per second.

### How to read it

Halstead's *original* intent was to predict three things about a
program before it was written: how big it would be in bits, how long
it would take to implement, and how many bugs to expect in
deployment. The empirical evidence for the volume and length
predictions is reasonable; the time and bugs predictions are more
controversial and have been criticised at length, notably in the
Purdue technical report
[*Software Science Revisited*](https://docs.lib.purdue.edu/cgi/viewcontent.cgi?article=1302&context=cstech).

In modern practice the Halstead numbers are used for three things:

1. As inputs into composite metrics, most importantly the
   Maintainability Index (next section), which depends on Halstead
   *volume*.
2. As a **language-independent size proxy**: volume in bits scales
   smoothly across languages in a way that LOC does not.
3. For **comparative effort budgeting**: when two refactoring
   candidates have similar cyclomatic complexity, the one with the
   higher Halstead difficulty is the one more likely to introduce
   regressions.

## Lines of Code {#lines-of-code}

This section covers the LOC variants: SLOC, PLOC, LLOC, CLOC, BLANK,
and DLOC. "Counting lines" sounds trivial until you have to define
exactly what counts. SLOC through BLANK are the de-facto standard
breakdown, going back to Samuel Conte, Hubert Dunsmore and Vincent
Shen's 1986 textbook [*Software Engineering Metrics and
Models*](https://books.google.com/books/about/Software_Engineering_Metrics_and_Models.html?id=PKlQAAAAMAAJ)
(Benjamin/Cummings, ISBN 0-8053-2162-4), which codified the
distinction between physical and logical lines. The Wikipedia entry on
[source lines of
code](https://en.wikipedia.org/wiki/Source_lines_of_code) summarises
that physical-versus-logical distinction.

| Variant | Counts |
|---------|--------|
| **SLOC** | Source Lines Of Code: every line in the file, comments, blanks, and code alike |
| **PLOC** | Physical Lines Of Code: non-blank, non-comment-only lines |
| **LLOC** | Logical Lines Of Code: statement-bearing lines (definitions, assignments, declarations) |
| **CLOC** | Comment Lines Of Code: lines that contain a comment (with or without code on the same line) |
| **BLANK** | Blank lines: whitespace-only lines |
| **DLOC** | Documentation Lines Of Code: rows the language treats as documentation (a doc comment, a Python docstring, a [POD](https://perldoc.perl.org/perlpod) block); always a subset of CLOC |

### Algorithm

big-code-analysis derives every count from a single pass over the
tree-sitter syntax tree (see `src/metrics/loc.rs`). Comments and
strings are identified by their AST node type rather than by lexical
scanning, so multi-line strings, raw strings, doc comments, and
string interpolations are all handled correctly. The per-language
`Loc` trait specifies which node kinds count as a "statement" for
LLOC; this is the subtle one, because what counts as a statement is
language-defined.

Documentation rows are comment rows that are also reported on their
own as DLOC (`loc.dloc`), so CLOC can be read with or without them.
Each language's own documentation tool decides which comments are
documentation; there is no single rule for all:

- Where the grammar marks it, the mark decides: a Rust `///`, `//!`,
  `/** */` or `/*! */` comment (tree-sitter's `doc_comment` child;
  `////` and `/**/` carry none), a Groovy `groovydoc_comment` (but not
  the empty `/**/` that grammar also marks), a Perl POD block (but not
  one that is nothing but `=begin comment` and `=for comment` regions,
  Perl's idiom for commenting code out; POD after such a region's end
  documents, and makes the block documentation).
- Where the grammar emits one comment kind for every form, the
  opening marker decides, as the language's documentation tool reads
  it: [Doxygen](https://www.doxygen.nl/manual/docblocks.html)'s `/**`,
  `/*!`, `///` and `//!` (and their after-member `<` forms) for C, C++
  and Objective-C; `/**` for
  [Javadoc](https://docs.oracle.com/en/java/javase/21/docs/specs/javadoc/doc-comment-spec.html),
  [KDoc](https://kotlinlang.org/docs/kotlin-doc.html),
  [JSDoc](https://jsdoc.app/) and [PHPDoc](https://docs.phpdoc.org/),
  plus `///` for Java's Markdown doc comments; `///` and `/**` for
  [C# XML docs](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/xmldoc/).
  The tools disagree at the edges and so does DLOC: `/***` is
  documentation to javac, Kotlin, Groovydoc and tsc (so TypeScript and
  TSX read it, while plain JavaScript follows JSDoc) but not to
  Doxygen, JSDoc, [Roslyn](https://github.com/dotnet/roslyn) or PHP;
  `/**/` is documentation nowhere; PHP needs whitespace after `/**`;
  and `////` is never documentation, which is stricter than Doxygen's
  lexer.
- A doc comment counts only where its tool reads it. A Rust, Java,
  Kotlin, Groovy, C# or PHP doc comment inside a function or closure
  body, inside a Java or Groovy initializer, or inside a Kotlin
  accessor or `init` block, is an ordinary comment: rustc
  [warns](https://doc.rust-lang.org/rustc/lints/listing/warn-by-default.html#unused-doc-comments)
  that one on a statement is unused, Roslyn raises CS1587, javadoc and
  [Dokka](https://kotlinlang.org/docs/dokka-introduction.html)
  document declarations only, and an in-body PHPDoc `/** @var */` is
  a type hint. A working note spelled `/// …` inside such a function
  therefore stays charged by MI. C, C++ and Objective-C keep in-body
  doc blocks: Doxygen appends them to the function's documentation as
  its *in-body description*, and
  [`HIDE_IN_BODY_DOCS`](https://www.doxygen.nl/manual/config.html#cfg_hide_in_body_docs)
  defaults to NO. Two placements inside a function still document: a
  Rust inner doc comment (`//!`, `/*! */`) opening a function's body
  is that function's own documentation, and a doc comment in a
  signature counts where its tool reads it there (a Doxygen `/**<`
  parameter block or a block after `[[attributes]]`, a PHPDoc after
  `#[…]` or on a promoted constructor property, a KDoc after an
  annotation), but not in Java or C#, which bind a doc comment only
  before a declaration's first token. JavaScript, TypeScript and TSX
  keep in-body JSDoc, which documents inner symbols, and Perl keeps
  POD wherever it sits, as [perldoc](https://perldoc.perl.org/perldoc)
  does, after `__END__` or `__DATA__` too, where every other
  non-blank row of the data section counts as PLOC.
- In Rust, Java, Kotlin and Groovy, a doc comment also has to be
  attached: it counts only when the declaration after it is one its
  tool documents. Attributes, annotations, modifiers and blank rows may
  come between the comment and the declaration. A Rust 2024 `safe fn`
  in an `extern` block counts too, although tree-sitter-rust parses its
  `safe` as an error. A doc comment above anything else stays a plain
  comment row: an import (Rust's `use` and `extern crate` take one), a
  statement, a macro call, a Rust `extern` block, a closing brace, or
  the end of the file. A KDoc nested in an argument or type-parameter
  list documents nothing. The tools also differ on a run of several doc
  comments.
  [rustc](https://doc.rust-lang.org/rustc/lints/listing/warn-by-default.html#unused-doc-comments)
  attaches every outer doc comment above an item, so all of them count.
  javac and Kotlin keep only the last one; javac's
  `-Xlint:dangling-doc-comments` names the earlier ones, and they do not
  count. Consecutive `///` lines are one Java Markdown doc comment.
  Groovydoc reads only the comment nearest the declaration, so a plain
  `//` comment in between detaches it. A Java doc comment above
  `package` counts only in a file with no type after it, which is the
  shape of `package-info.java`. Rust's inner `//!` is unaffected: it
  documents the item it sits in.
- C#, PHP, JavaScript and TypeScript bind doc comments the same way.
  Each language still follows its own tool:
  - C#: Roslyn keeps the nearest run of doc comments that only blank
    rows separate, reading past plain comments and directives such as
    `#region` to find it. Every other doc comment is
    [CS1587](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/compiler-messages/xml-documentation-comments),
    and so is a doc comment above a namespace, a `using` directive, or
    an accessor such as `get;`.
  - PHP:
    [phpDocumentor](https://www.phpdoc.org/) gives a class, function,
    method, property, constant or enum case the last DocBlock before
    it. A DocBlock above a statement, a `use` or a `namespace` counts
    only as the file DocBlock. phpDocumentor takes the file's first
    DocBlock as the file DocBlock when no class, function, constant,
    interface or trait follows the DocBlocks at the top of the file. It
    also takes it when exactly two DocBlocks come before the first such
    element.
  - JavaScript: [JSDoc](https://jsdoc.app/) gives a declaration or an
    assignment the nearest block comment before it, reading past `//`
    lines, so a plain `/* */` in between detaches a doc comment. A call
    or another statement takes nothing. A block tagged `@file`,
    `@fileoverview`, `@overview` or `@module`, or with a tag that names
    a symbol (`@typedef {Object} T`), documents that symbol wherever
    it sits. `@license` does not make a block stand alone.
  - TypeScript: tsc keeps the last `/**` before a declaration, reading
    past every plain comment, and documents no assignment statement. A
    doc comment after a decorator, a modifier such as `static`, or
    `export` or `declare` is inside the declaration and documents
    nothing, and so does one after code on its own row, except before a
    parameter. A `@module` or `@packageDocumentation` block documents
    the file, as [TypeDoc](https://typedoc.org/) reads it.

  In a JavaScript or TypeScript function body every doc comment still
  counts, since JSDoc documents inner symbols.
- C, C++ and Objective-C follow [Doxygen](https://www.doxygen.nl/),
  which keeps every doc block it meets and gives them all to the next
  entry. A doc block counts when what follows it is something Doxygen
  documents: a declaration, a definition, a macro, a `typedef` or
  `using`, a namespace, a template, an enumerator, a macro call such
  as `FOO(bar);`, or an Objective-C `@interface`, `@property` or method.
  Doxygen reads past blank rows, plain comments, `#include`, `#import`,
  `#pragma` and an empty `;`, and into an include guard or another
  `#if` that holds a declaration. A doc block above a forward
  declaration (`struct s;`, `@class C;`), a closing `}` or `@end`, or
  the end of the file stays a plain comment row. A block carrying a
  structural command documents an entry of its own, so it counts
  wherever it sits: `\file`, `\mainpage`, `\page`, `\defgroup`,
  `\addtogroup` and `\name` always, and a command that names its
  subject, such as `\class X` or `\def M`, when it has a name. The doc
  blocks above a `\file` block count with it. `\brief` and `\ingroup`
  are not structural. A member-after comment (`/**<`, `///<`) documents
  the member before it and always counts, as do the blocks in a
  function body, which Doxygen publishes as the function's in-body
  description.
- In Python, every bare string statement (a string whose value is
  discarded) is a comment row, but only one where Python reads
  documentation is DLOC, following
  [PEP 257](https://peps.python.org/pep-0257/): the first statement of
  a module, class or function (the string stored as `__doc__`, which
  must be a `str` literal, never an f-string or bytes), successive
  strings after any docstring, and an *attribute docstring* directly after
  an assignment to a name or a `type` alias at module or class level,
  or after an assignment to `self.<name>` at the top of a class's
  `__init__`, alone or in a tuple of targets, which
  [Sphinx autodoc](https://www.sphinx-doc.org/en/master/usage/extensions/autodoc.html)
  reads. Sphinx also reads such an assignment inside an `if`, `with`,
  loop or `try` suite; PEP 257 names the top level only, and DLOC
  follows the PEP there. [PEP 258](https://peps.python.org/pep-0258/#additional-docstrings)
  also includes successive strings after attribute docstrings. Comments
  between them do not count as statements. Any other bare string, such
  as a triple-quoted string wrapping commented-out code in the middle
  of a function, stays a comment row and is charged by MI like a `#`
  comment. This is narrower than
  [radon](https://radon.readthedocs.io/)'s `multi`, which takes every
  bare multi-line string. A docstring row that also holds code
  (`def f(): """doc"""`) is code too. An implicitly concatenated
  literal (`"""doc""" "more"`) is one string and a bare tuple of
  strings is none, the same test Halstead applies when it drops a bare
  string's operands. A parenthesized string (`("""doc""")`) is the same
  string, because CPython drops the parentheses, and its `(` and `)`
  rows are its rows. A comment is documentation in Python only in
  Sphinx's `#:` form: consecutive `#:` lines directly above one of
  those attribute assignments or aliases, or a `#:` after the code on
  its line (`MAX = 10  #: in bytes`). A plain `#` line breaks a `#:`
  block.

Go, Ruby and Lua have no marker to read, so a comment is
documentation by where it sits, decided once per *run*: the chain of
comment siblings on consecutive rows that hold nothing but comments (a
trailing `x := 1 // t` never joins one, and a blank row ends one):

- Go: a run directly above a declaration is
  [go/doc](https://pkg.go.dev/go/doc)'s lead comment, where a
  declaration is the package clause, a `func` or method, a top-level
  `type` / `var` / `const` declaration or one of its grouped specs, a
  struct field, or an interface method; never an import, and never a
  declaration inside a function body, which go/doc does not read. A
  directive row such as `//go:generate` belongs to the group but is
  not documentation, and is not counted.
- Ruby: a run directly above a `def`, `def self.`, `class`, `module`,
  `class << Const`, a constant or class-variable assignment
  (`MAX = 10`, `Foo::BAR = 1`, `@@count = 0`), a method alias
  (`alias new old`, `alias_method :new, :old`), an `attr` or `attr_*`
  call or a visibility-wrapped `def` (`private def m`), alone or under
  an `if` / `unless` modifier, as [YARD](https://yardoc.org/) attaches
  it, including the first statement of a class body or of an `if`,
  `elsif` or `unless` body. In a method body YARD reads only a nested
  `def`, and nothing in a singleton class or conditional opened there;
  no YARD handler claims a `case`, a `begin` or a loop, so nothing in
  their bodies counts anywhere. A shebang, an encoding line or a
  `frozen_string_literal` magic comment is dropped from the run, as
  YARD drops it. Unlike YARD, one blank row between the run and the
  `def` is a gap, not a tolerance; a trailing comment on the
  statement's own row (`MAX = 3 # The maximum.`) documents nothing;
  and a statement in a call's block (`included do`, an RSpec
  `describe` block, in either `do … end` or `{ }` form), which YARD
  never reads, still counts. A block that opens with
  [RDoc](https://ruby.github.io/rdoc/)'s `##` (exactly two `#`, so a
  `####` banner or a `##### Helpers #####` divider opens nothing) or a
  YARD `@!` directive (`# @!method`, `# @!attribute`, `# @!parse`)
  documents a method with no `def` to sit above (`define_method`, a
  DSL call), so it counts whatever follows it, in a namespace body but
  never in a method's.
- Lua: a run that opens with an [LDoc](https://lunarmodules.github.io/ldoc/)
  `---` row (a row of dashes alone opens too; `--- x ---` is LDoc's
  end marker and does not) and continues over the `--` rows below it,
  or a `--[[-` long comment. A long comment ends a run, as LDoc's
  lexer ends a block there. A block with no text (a row of dashes
  alone) is a comment, as LDoc discards it. A `---` row directly under
  a line that ends in a trailing `-- note` is no block at all: LDoc's
  lexer keeps a line comment's newline and gathers consecutive
  comments into one group, whose first token (the trailing note)
  decides it. Otherwise a block counts when it is the first block of a
  file that opens with a comment or a shebang, which LDoc skips (the
  module's documentation), when one of its lines opens with an `@` tag
  (`-- @param x`, [LuaLS](https://luals.github.io/)'s `---@class`), or
  when an item follows it on the next row (the first statement of an
  `if`, `do` or loop body included): a `function`, an assignment to
  one name or dotted name (`local x = 1`, `M.a.b = 1`; not a bare
  `local x`, `local a, b = …`, `local x <const> = …` or `M[1] = …`), a
  named table field (`new = function … end`), or a `return` of a
  function or a table. Any other block is a comment, as LDoc drops it
  with a "definition cannot be parsed" warning. Three deviations from
  LDoc: a tagged block counts even with no item or name to attach to;
  a blank row between a block and its item is a gap; and a file that
  opens with code still has its blocks read, where LDoc reads none of
  them without a `module()` call. A block inside a function body is a
  comment: LDoc documents what a module declares, and an in-body `---`
  annotates the code there instead (LuaLS's `---@type` on a local), as
  PHP's in-body `@var` does.

Nothing else counts: an Elixir `@doc` heredoc is an assignment the
compiler stores and is code, and Bash, Tcl and iRules have no
documentation syntax.

The counts satisfy a few useful identities:

```text
SLOC = PLOC + BLANK + (lines that are comment-only)
CLOC ≥ (lines that are comment-only)        # CLOC also counts mixed code+comment lines
DLOC ≤ CLOC                                 # every documentation row is a comment row
```

### How to read it

- **SLOC** is what most people mean colloquially by "lines of code".
  It is the canonical size proxy, but is sensitive to formatting and
  not portable across language conventions.
- **PLOC** strips away the visual noise. It is the measure to gate
  file length on (`loc.ploc`), and the size term of the
  Maintainability Index below.
- **LLOC** is the most reliable *statement* count. It is the right
  measure if you are budgeting test cases per statement, or comparing
  the density of a Python file against a Java file.
- **CLOC**, combined with PLOC, gives you a *comment density*:
  `CLOC / PLOC` is a rough proxy for how much of the file is
  documentation versus implementation.
- **BLANK** is mostly diagnostic: a file with a very low BLANK
  proportion is often hard to read.
- **DLOC** is the slice of CLOC that is documentation: doc comments,
  docstrings, POD. `CLOC − DLOC` is the count of working comments, the
  numerator of the Maintainability Index's SEI comment share.

LOC variants are used well beyond raw size. They are the most common
input into cost-estimation models
([COCOMO](https://en.wikipedia.org/wiki/COCOMO) and COCOMO II both use
KSLOC, thousands of source lines, as their base unit), they feed
effort prediction in product-portfolio dashboards, and they are the
normalising denominator for almost every other metric: *defects per
KSLOC*, *churn per KSLOC*, *test cases per KSLOC*. The weakness is
that LOC is easy to game: a 10× difference in coding style can produce
a 2× difference in LOC. That is why this chapter has so many other
metrics in it.

## Maintainability Index (MI) {#maintainability-index-mi}

The **Maintainability Index** is a composite metric that rolls
several of the metrics above into a single number, roughly 0 to 100,
meant to be read as "how maintainable is this code?". Paul Oman and
Jack Hagemeister proposed it in their 1992 ICSM paper [*Metrics for
assessing a software system's
maintainability*](https://ieeexplore.ieee.org/document/242525), and
Don Coleman, Dan Ash, Bruce Lowther, and Paul Oman refined it in the
1994 IEEE Computer paper [*Using metrics to evaluate software system
maintainability*](https://www.ecs.csun.edu/~rlingard/comp589/ColemanPaper.pdf)
(IEEE Computer 27(8), pages 44-49). Their method was empirical: they
collected expert maintainability ratings on a handful of production
Hewlett-Packard systems, computed forty candidate metrics on each,
and let regression analysis pick the best linear combination. The
combination that survived used Halstead volume, cyclomatic
complexity, lines of code, and comment density.

big-code-analysis reports the three formulas that have stuck in
practice. The three values nest under the `mi` object as the keys
`original`, `sei`, and `visual_studio` (the dotted threshold names
`mi.original`, `mi.sei`, `mi.visual_studio`):

```text
mi.original      = 171 − 5.2·ln(HV) − 0.23·CC − 16.2·ln(PLOC)
mi.sei           = mi.original + 50·sin(√(2.4·CM))
mi.visual_studio = max(0, mi.original · 100 / 171)

CM               = (CLOC − DLOC) / (SLOC − DLOC rows that carry no code), in [0, 1]
```

- `mi.original` is the Coleman–Oman formula. It can be negative for
  pathological files.
- `mi.sei` is the Software Engineering Institute's refinement, which
  adds a comment-density term. The `sin(√(...))` shape was chosen so
  that *some* comments help, but adding more after a point does not,
  and never hurts. `CM` is the share of working (non-documentation)
  comment rows among the space's rows, a ratio in `[0, 1]`. On that
  scale the term rises monotonically from 0 with no comment to about
  50 (`50·sin(√2.4)` ≈ 49.99) when every row is a comment, so `mi.sei`
  is never below `mi.original` and equals it in a comment-free space.
- `mi.visual_studio` is the linear rescaling Microsoft chose for
  [Visual Studio](https://learn.microsoft.com/en-us/visualstudio/code-quality/code-metrics-maintainability-index-range-and-meaning),
  where the score is clamped to `[0, 100]` and shown to developers
  traffic-light style: green ≥ 20, yellow ≥ 10, red below.

The size term is `PLOC`, the rows that carry code (#1607). Comment,
documentation and blank rows never enter it, so a `// TODO`, a
commented-out block, a Rust `///` block, a docstring or a run of blank
lines leaves `mi.original` and `mi.visual_studio` exactly where they
were, with Halstead and cyclomatic unchanged. A row a doc comment or
docstring shares with code, as in `def f(): """doc"""` or
`int x; /** doc */`, is a code row and stays counted.

Documentation also leaves both sides of `CM` in every language
(#1597), so a function's `mi.sei` is the same with and without its
`///` block, its Javadoc comment, its POD or its docstring, which keeps
MI consistent with Halstead, where a docstring's operands are already
ignored. Blank rows stay in `CM`'s denominator: adding one to a space
with a working comment dilutes the comment share and lowers `mi.sei`
slightly, while a comment-free space is unmoved.

Two details differ from some published statements of the SEI formula.
Its `ln` matches `mi.original`'s; releases before #1607 took `log2` of
the volume and size in `mi.sei` alone. And `CM` is a ratio rather than
a percentage: on the 0–100 scale the `sin` term swings between −50 and
+50 several times across ordinary comment densities.

### Charging documentation {#mi-charges-docs}

With `mi_charges_docs` on, `mi.sei` counts documentation as comment:
it takes its comment share as `CLOC / SLOC`, the inputs that releases
without `loc.dloc` fed that share. Nothing else moves. The size term is
`PLOC` in both modes, so `mi.original` and `mi.visual_studio` read the
same either way, and since the comment term only rises with `CM`,
charging documentation can only raise `mi.sei`. It does not reproduce
an earlier release's numbers: those sized every variant on `SLOC` and
took `mi.sei` on a different scale. It changes no other metric:
`loc.dloc` is reported either way.

- Library: `MetricsOptions::default().with_mi_charges_docs(true)`.
- CLI: `--mi-charges-docs` (or `--mi-charges-docs=false` to say the
  default out loud), or `mi_charges_docs = true` in `bca.toml`; the CLI
  value overrides the key in either direction.
- Python: `mi_charges_docs=True` on `analyze`, `analyze_source`,
  `analyze_batch`, `analyze_paths` and `Ast.metrics`.
- The REST server has no request field for it; an embedder sets
  `WebMetricsCfg::with_mi_charges_docs`.

The LOC inputs it reads are the current ones, so a Python file whose
docstring is an implicit concatenation (`"""doc""" "more"`) or whose
bare `"a", "b"` tuple statement was once a comment row reads a
different comment share from a release that classified those rows the
other way.

A baseline records which mode wrote it, and `bca check --baseline`
warns when it reads `mi.*` entries under the other mode. Only `mi.sei`
entries can disagree, and in one direction: a value recorded with
documentation charged sits above the default's, so reading it under the
default can fire on untouched code, and a value recorded under the
default can cover a real regression under the charged mode. Switching
the mode therefore means reviewing any `mi.sei` threshold and
regenerating the baseline in the same change, as for
`cyclomatic_count_try`. `bca check --print-effective-config` names the
mode in force.

A baseline also records the MI formula revision that computed its
values (`mi_formula` in `[provenance]`; revision 2 since #1607, and an
unstamped file reads as revision 1). When a baseline holding any
`mi.*` entry was written under another revision, `bca check
--baseline` warns that its values are not comparable, in place of the
mode warning; refresh it with `--write-baseline`.

Arie van Deursen's blog post [*Think Twice Before Using the
Maintainability
Index*](https://avandeursen.com/2014/08/29/think-twice-before-using-the-maintainability-index/)
collects the historical context and a sharp critique of the metric.

### Algorithm

The implementation is purely arithmetic: `src/metrics/mi.rs`
consumes the already-computed `Halstead`, `Cyclomatic`, and `LOC`
metrics and applies the three formulas. Because the formulas use the
natural log of Halstead volume and of `PLOC`, MI is undefined for
empty files; big-code-analysis returns `0.0` for any space with zero
code lines or zero Halstead volume (a comment-only file or a
docstring-only module has no volume). That `0.0` is a sentinel, not a
score, and it cannot be told apart from a measured 0 or from
`mi.visual_studio`'s clamp: `mi::Stats::is_empty()` identifies such a
space in the library, and `bca report`'s averages skip it. The JSON
value stays `0.0` until 3.0, which is planned to emit `null` instead
(#1621).

### How to read it

MI was *originally* designed as a portfolio-level score: "how much
maintenance pain should we expect from this codebase over the next
year?". It is stable across releases of a healthy system and tends to
drop measurably before a system enters the "legacy" quadrant.

The common use is the **Visual Studio traffic-light rendering**:
Visual Studio exposes `mi.visual_studio` through green, yellow and red
indicators on a method, which is where many .NET developers meet it,
and also why it has attracted much criticism. Treat it as a smoke
detector, not a thermostat: a sudden drop is a useful signal, but the
absolute number is noisy.

## NArgs {#nargs}

**NArgs** counts the number of arguments declared by a function,
method, or closure. The metric has no well-known origin paper;
guidance on limiting argument counts appears in Kernighan and
Plauger's [*The Elements of Programming
Style*](https://en.wikipedia.org/wiki/The_Elements_of_Programming_Style)
(1974) and Robert C. Martin's
[*Clean Code*](https://www.pearson.com/en-us/subject-catalog/p/clean-code-a-handbook-of-agile-software-craftsmanship/P200000009044/9780136083252)
(2008), which suggests three arguments as a soft ceiling.

big-code-analysis splits the count by callable kind: every aggregate
is reported separately for *functions* and *closures* so a Rust file
heavy on `|…| …` closures and a Java file with only methods produce
comparable numbers. The serialised output
(`src/metrics/nargs.rs`) is `function_args`, `closure_args`,
`function_args_average`, `closure_args_average`, `total`, `value`,
`average`, `function_args_min`, `function_args_max`, `closure_args_min`,
`closure_args_max`. The implementation handles default arguments,
variadic arguments,
keyword-only arguments, and destructured parameters consistently per
language. Comments written inside the parameter list are not
parameters, including the C++ idiom that puts one where an unused
parameter's name would go: `void f(int /*unused*/)` is one argument.

### Macro-obscured C-family declarators {#nargs-macro-declarators}

C, C++, Mozcpp and Objective-C are parsed without running the
preprocessor, so a function-like macro standing where the declared name
belongs is still there in the tree. The idiom is the
[JNI](https://docs.oracle.com/en/java/javase/21/docs/specs/jni/) shim:

```c
#define RUN_STATS_METHOD(name) JNICALL Java_org_tensorflow_RunStats_##name

JNIEXPORT jlong RUN_STATS_METHOD(allocate)(JNIEnv *env, jclass clazz) { … }
```

Read as written, `RUN_STATS_METHOD(allocate)` is the declarator and
`(JNIEnv *env, jclass clazz)` belongs to the return type: one argument.
big-code-analysis reads it the other way and reports **two**, because
neither language lets a function return a function type (C11 6.7.6.3p1,
C++ [`[dcl.fct]`](https://eel.is/c++draft/dcl.fct)): a declarator
nested directly inside another one cannot be a declarator chain, so
the outer list is the function's own. Every legitimate
function-returning-a-function-pointer form, such as
`int (*fp(int a, int b))(int c)`, puts parentheses in between and is
unaffected, as is C++ `operator()`.

The space is named for the **macro**, not the function. There is no
other candidate: the real symbol is assembled by `##` token pasting and
never appears in the source. So several shims in one file share a name,
which `bca check` shows as several rows with the same label at different
lines, and which `.bca-baseline.toml` disambiguates by body hash the way
it does an overload set.

### What the threshold gate measures {#nargs-gate}

`bca check --threshold nargs=N` gates each callable on **its own**
parameter list, which is what every comparable tool measures:
[RuboCop `Metrics/ParameterLists`](https://docs.rubocop.org/rubocop/cops_metrics.html#metricsparameterlists),
[ESLint `max-params`](https://eslint.org/docs/latest/rules/max-params),
[Clippy `too_many_arguments`](https://rust-lang.github.io/rust-clippy/master/index.html#too_many_arguments),
lizard,
[SonarQube S107](https://next.sonarqube.com/sonarqube/coding_rules?open=java%3AS107&rule_key=java%3AS107)
and
[Pylint `R0913`](https://pylint.readthedocs.io/en/latest/user_guide/messages/refactor/too-many-arguments.html)
all count one callable at a time.

This is *not* the serialized `total`. The `function_args` and
`closure_args` keys above are subtree sums: a function that declares two
parameters and contains a three-parameter nested function reports
`function_args: 5`. Before
[#1196](https://github.com/dekobon/big-code-analysis/issues/1196) the
gate read that sum, so a three-parameter function with a two-parameter
sort comparator was flagged at 5, and the remediation its number
implied, fewer parameters, was not the one that would clear it.

The number the gate does read is serialized as `value`, added in
[#1236](https://github.com/dekobon/big-code-analysis/issues/1236) so a
JSON-consuming front-end can reproduce the gate. Until then the Python
`to_sarif` binding had nothing but `total` to compare and reported
breaches `bca check` did not.

Nothing escapes the narrower rule. In the twelve grammars whose closures
open their own space — Rust, JavaScript, TypeScript, TSX, MozJS, C#, Go,
PHP, Perl, Ruby, Lua and Elixir — a closure is gated on its own offender
row, which is also where its fix belongs. In Python, Java, Kotlin, C++
(and its Mozilla dialect), Groovy and Objective-C a lambda, closure or
block opens no space, so its arguments can only be attributed to the
enclosing function; there the offender row shows the split:

```text
small: nargs = 8 (1 own + 7 lambda) (limit 5)
```

The split identifies how many arguments come from the enclosing
signature and how many come from lambdas.

Both lists are read from the per-language role tables rather than
maintained by hand: a language is in the first when every kind its
`Closure` row owns is also an opener of its `FuncOpener` row, and in the
second when none is (`closures_that_open_a_space_are_what_the_book_says`
in `big-code-analysis-ast/src/roles/space_tree_tests.rs` pins the two
against this paragraph; Bash, C, Tcl and iRules have no closure at all).

### Languages where it reads 0 {#nargs-language-gaps}

A metric that silently reports 0 reads as "no offenders" rather than
"not measured", so know where the count is inert:

- **Bash**: correct and permanent. The shell has no formal parameter
  list; arguments arrive as `$1`, `$2`, and so on. It is the only
  language where every function reads 0.
- **Perl subs without a signature**: correct. Signatures
  (`sub add($x, $y)`) are counted; a sub that reads its arguments from
  `@_` declares no formal parameters to count.
- **Perl anonymous subs**: a gap, and an upstream one. The grammar
  parses an anonymous sub's signature inside an error node, so
  `my $f = sub ($x) {…}` reads 0 even though it has a signature.

A `nargs` limit is therefore inert on a Bash codebase and sparse on
Perl written before `use v5.36`. Gate it per language rather than
repository-wide; see
[Choosing thresholds](recipes/thresholds.md#language-gaps).

### How to read it

A function with many arguments is hard to call correctly and harder
to test exhaustively, since the test matrix grows roughly
exponentially. The classic refactoring advice is the *introduce
parameter object* pattern: when a function takes more than four
related arguments, group them into a record / struct / dataclass.

The common use is as a **review-blocking lint rule**: most linters
(Pylint's `R0913`, ESLint's `max-params`,
[Checkstyle's `ParameterNumber`](https://checkstyle.sourceforge.io/checks/sizes/parameternumber.html))
flag functions with more than a configurable threshold. NArgs is also
a useful component of API-design dashboards: public APIs whose
average NArgs has crept upward over time tend to be ones that have
accreted "just one more parameter" feature flags.

## NExits {#nexits}

**NExits** counts the number of distinct exit points from a
function: every explicit `return`, every `throw` / `raise`, and
(in Rust) every `?` early-return. A generator's `yield` counts in
Python, the JavaScript family, C# and PHP, because it hands control
back to the caller, and so does C++'s `co_yield`; C++'s `co_return` is
a `return` (#1547). The implicit fall-through return at the end of a
function is **not** counted; only explicit exits are (see issue #243).

The metric goes back to the
[structured-programming](https://en.wikipedia.org/wiki/Structured_programming)
literature of the 1970s, where Edsger Dijkstra and others argued that
functions should have **a single entry and a single exit point** (the
["SESE" rule](https://en.wikipedia.org/wiki/Single-entry_single-exit)).
Modern thinking is more nuanced; see Steve McConnell's
[*Code Complete*](https://www.microsoftpressstore.com/store/code-complete-9780735619678),
2nd edition (Microsoft Press, 2004), which recommends *early returns*
as a clarity-improving pattern when they reduce nesting.

big-code-analysis walks each function's syntax tree, identifies the
language-specific exit nodes (see the per-language `Exit` trait in
`src/metrics/nexits.rs`), and reports per-function counts plus
file-level `sum`, `average`, `min`, and `max`. The serialised
field name is `nexits`, matching the prose acronym used here.

### How to read it

Strict SESE coding standards ([DO-178C](https://en.wikipedia.org/wiki/DO-178C)
for avionics, [MISRA C](https://misra.org.uk/) for embedded
automotive) still require an NExits of 1 per function, because
multiple exit points complicate certified control-flow analysis.
Outside those domains, an NExits of `2-4` is usually a *good* sign: it
almost always means the function uses guard clauses to handle
preconditions and then proceeds in a flat body.

A very high NExits, say above 8, is the warning sign. It usually
means the function should have been split into several smaller
functions, with each "successful branch" becoming its own helper.

## NOM {#nom}

**NOM** stands for *Number Of Methods* and counts every function,
method, and closure defined inside a given scope (file, class, or
namespace). It is one of the first metrics Mark Lorenz and Jeff Kidd
introduced in their 1994 book
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
class static blocks. Each opens a function *space*, so it has its own
complexity scores, `bca check` can flag it, and it contributes to WMC.
But none is a callable you name at a call site, and counting an
accessor as a method would make NPM bill the same property once as an
attribute and again as a method, skewing the NPA/NPM ratio the OOP
metrics exist to report. A Kotlin file of nothing but property
accessors therefore reports `nom.functions == 0` while still reporting
their complexity (#1184).

### How to read it

NOM is the input to several other metrics: WMC sums *cyclomatic*
complexity across the same set of methods that NOM counts, and NPM
filters that same set down to public methods. As a standalone
metric, the Lorenz–Kidd recommendation is `≤ 20` methods per class.
The common use is as a *God-class detector*: a class with NOM in
the dozens is almost always doing too much, and is a strong
candidate for "extract collaborator" refactoring as documented in
Martin Fowler's [*Refactoring* catalogue
entry on Large Class](https://refactoring.com/catalog/extractClass.html).

## NPA {#npa}

**NPA** counts the **number of public attributes** (also called
fields, properties, or instance variables) declared by a class or
interface. It is part of the metric family introduced by Lorenz and
Kidd in *Object-Oriented Software Metrics* (1994) and was later folded
into the MOOD ("Metrics for Object-Oriented Design") suite proposed by
[Brito e Abreu and Carapuça
(1994)](https://www.researchgate.net/publication/267412803_Object-Oriented_Software_Engineering_Measuring_and_Controlling_the_Development_Process).

big-code-analysis splits the count by definition-site kind:
*classes* (concrete types with state) and *interfaces* (abstract
contracts). The serialised output (`src/metrics/npa.rs`) is
`class_npa_sum` (sum of NPA across all classes), `interface_npa_sum`
(sum across interfaces), `class_attributes` (sum of *all* attributes,
public or not, across classes), `interface_attributes`, `class_cda`
(class density of public attributes, an accessibility *ratio* rather
than an average), `interface_cda`, `total`, `total_attributes`, and
`cda`. The per-language `Npa` trait decides what counts as "public"
(Java `public`, C# `public`, Rust `pub`, Python's "no leading
underscore" convention, and so on) and what counts as "attribute"
rather than "method".

### Which spaces carry NPA, NPM and WMC {#oop-emission-scope}

NPA and NPM are emitted on **container spaces** (`class`, `struct`,
`trait`, `impl`, `namespace`, `interface`) and on the whole-file `unit`
root, which carries the roll-up across every container in the file. WMC
follows the same rule wherever it is computed at all, which is narrower
in two ways set out below. A **function space does not carry any of
them**: a method owns no methods or attributes of its own, so the block
would be all zeros. Before big-code-analysis 2.1.0, NPA and NPM did
emit that all-zero block on function spaces in C#, JavaScript, MozJS,
TypeScript, TSX, PHP and Ruby, and on the Kotlin, Java, Groovy and
JS-family accessor / `init` / `static` spaces. In the same release Go,
Rust, Python, C++, Objective-C and Elixir went the other way: they
decided from their own grammar node kinds, so a `struct` declared
inside a function put the block on that **function** space, while a
`namespace` or a file root with no container at file scope carried
none. Both deviations are gone; the space's kind is now the only
input, for every language.

That rule governs the *block*, not the numbers behind it: the counts
roll up through every enclosing space regardless. So a type declared
inside a function body is reported by the nearest enclosing container,
or by the file root when there is none.

Four things read differently. A Go file's NPA and NPM live on the `unit`
root and nowhere else, because Go is the one language that emits them
without having a container kind in its space tree: `type … struct` and
`type … interface` open no space of their own. (Bash, C, Lua, Perl,
Tcl and iRules have no container kind either, but they emit neither
block anywhere, so the question does not arise.)

Neither WMC narrowing moves NPA or NPM, which is why the rule above
still holds as stated for those two. The first is language-level: **Go
emits no `wmc` block at all**, on any space including the `unit` root,
because its flat space model cannot attribute a method to a receiver
class. So a Go file carries `npa` and `npm` at the root and `wmc`
nowhere. The second is space-kind-level: a **`namespace` space carries
`npa` and `npm` but no `wmc`**, because a namespace's member functions
are free functions rather than methods of a class, so there is no
per-class complexity to weight. That covers every construct mapping to
`SpaceKind::Namespace` (a C++ or Mozcpp `namespace`, and a Ruby
`module`), not just the C++ spelling. Objective-C has no namespace
construct of its own, so the case does not arise there. The class
*inside* the namespace carries all three, and so does the file root.

Finally, the CSV projection is a fixed-column format: it writes the
`npa.*` / `npm.*` columns on **every** row regardless of space kind,
carrying the real accessor values rather than eliding them.

Thresholds are narrower still: `bca check` gates `npa` / `npm` on
container spaces only, never the file root (see
[Threshold scope](commands/check.md#threshold-scope)). Taken with the
paragraph above, that has a direct consequence: since a Go file's NPA
and NPM are only ever reported at the root, **no `npa` or `npm`
threshold can fire on Go source**.

### How to read it

NPA is a *direct* measure of encapsulation. Every public attribute
is a piece of internal state that callers can read or write without
going through a method, which means it is a piece of internal state
the class cannot validate or evolve without breaking callers. The
canonical guidance, first stated in Bertrand Meyer's
[*Object-Oriented Software
Construction*](https://en.wikipedia.org/wiki/Object-Oriented_Software_Construction)
(Prentice Hall, 1988) and known as the *Uniform Access Principle*, is
to keep NPA at or near zero and to expose state through public methods
instead.

The common use is **API-stability auditing**: a public library class
whose NPA grows over time accumulates breaking-change liability
faster than its public-method surface.

## NPM {#npm}

**NPM** counts the **number of public methods** declared by a class
or interface. It is the method-side companion to NPA and was again
codified by Lorenz and Kidd (1994).

As with NPA, big-code-analysis splits NPM by definition-site kind
(classes vs. interfaces). The serialised output
(`src/metrics/npm.rs`) is `class_npm_sum` (sum of NPM across classes),
`interface_npm_sum`, `class_methods` (sum of *all* methods, public or
not, across classes), `interface_methods`, `class_coa`,
`interface_coa` (operation-accessibility *ratios*, not averages),
`total`, `total_methods`, and `coa`. It follows the same emission rule
as NPA ([above](#oop-emission-scope)). The language-specific `Npm`
trait decides what counts as public (for example, Rust's `pub`,
Python's leading-underscore convention, C++'s `public:` section) and
folds together regular methods, constructors, and operator overloads
as appropriate.

NPM is also one of the inputs into [Mark Hitz and Behzad
Montazeri's *Class Interface Size*
metric](https://link.springer.com/chapter/10.1007/978-94-011-5006-1_19),
and into Chidamber and Kemerer's *Response For a Class* (RFC).

### How to read it

NPM is the **public interface size**. A class with NPM in the dozens
is a class with too large an API contract: every public method is
something callers can come to depend on, and every change to it is a
breaking change. The Lorenz–Kidd guidance is `≤ 20` public methods
per class, with anything over `40` considered a strong refactoring
candidate. The same rule applies with particular force to
*interfaces* in Java and C#, where the contract is the shape clients
pin against.

The common use is as a **public-API change tracker** for libraries:
monitoring NPM at the package level catches accidental expansion of
a library's surface area in the same way that NPA catches accidental
exposure of internal fields.

## Tokens {#tokens}

**Tokens** is a per-function and per-file count of the *tree-sitter
leaf tokens* (identifiers, literals, keywords, punctuation),
excluding any token whose AST ancestor is a comment node. It is a
lexer-driven size proxy intended as a more formatting-resilient
alternative to LOC. The same idea is known from Terry Yin's
[`lizard`](https://github.com/terryyin/lizard) command-line tool,
where many readers will first have seen a token-count metric.

The implementation lives in `src/metrics/tokens.rs`. Because Tokens
counts *every* leaf, including punctuation that Halstead
deliberately skips, the value will *not* equal Halstead `N1 + N2`,
and because it counts tokens rather than lines it is **not**
equivalent to any LOC variant. Whitespace-only reformatting, renaming
a variable, and removing a comment all leave Tokens unchanged. Edits
that change the *tokens themselves* (adding an `if`, adding optional
braces around a single-statement block, or inserting or removing
semicolons in a language where they are optional) do change the
count.

### How to read it

Tokens is less sensitive to formatting than LOC. It is the right size
measure when you are normalising another metric
across languages or across teams with different style conventions:
`bugs per KSLOC` is sensitive to formatting, while `bugs per 1000
tokens` is much less so.

The common use is as the **defect-density denominator of choice** in
cross-language research: a 1000-line Java file and a 1000-line Lisp
file contain very different amounts of code, but a 1000-*token* slice
of each contains roughly the same amount of information. This makes
Tokens useful for machine-learning code-quality models that train
across many languages.

## WMC {#wmc}

**WMC** (*Weighted Methods per Class*) is the first metric in
the [Chidamber and Kemerer suite](https://www.eso.org/~tcsmgr/oowg-forum/TechMeetings/Articles/OOMetrics.pdf),
introduced in their 1994 IEEE Transactions on Software Engineering
paper *A Metrics Suite for Object Oriented Design* (volume 20,
issue 6, pages 476-493). The CK suite comprises WMC, DIT, NOC, CBO,
RFC and LCOM. big-code-analysis currently implements WMC and the
simpler size metrics (NOM, NPA, NPM), with the inheritance- and
coupling-based ones tracked for future work.

WMC is the **sum of the cyclomatic complexity of every method
defined in a class**. The original paper deliberately left the
"weighting" abstract: Chidamber and Kemerer wrote that "if all
method complexities are considered to be unity, then WMC = n, the
number of methods". The empirical follow-up literature has settled on
cyclomatic complexity as the weight, and that is what
big-code-analysis uses.

### Algorithm

For each class or interface found by the per-language parser,
big-code-analysis sums the standard cyclomatic complexity of every
method body inside it (`src/metrics/wmc.rs`). The file-level
serialised output is three fields: `class_wmc_sum` (sum of WMC across
all classes in the file), `interface_wmc_sum` (sum across interfaces),
and `total` (the two combined). No min/max/average aggregation is
emitted at the file scope. To rank individual classes by WMC, use the
report subcommand, which surfaces a *Type hotspots (top N by WMC)*
section (see [Commands → Report](./commands/report.md)).

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

The common use is **God-class detection**: combined with NOM, WMC is
one of the clearest signals that a class needs to be split. For a
class with high NOM but low WMC, check whether the methods form a
cohesive data API before splitting the class. A class with low NOM
and high WMC has a few gargantuan methods (split
the methods, not the class). A class with *both* high NOM and high
WMC is the classic God class.

---

## Where to go next

- The [Supported Languages](./languages.md) chapter lists every
  supported language and grammar. Metric coverage varies by
  language because some metric definitions (`NPA`, `NPM`, `WMC`)
  only make sense in languages with classes.
- The [Supported Change-history (VCS) Metrics](./metrics-vcs.md)
  chapter covers the complementary family derived from version-control
  history rather than from the source AST: commit frequency, churn,
  ownership, and the composite risk and hotspot scores.
- The [Commands → Metrics](./commands/metrics.md) page documents
  how to invoke `bca metrics` to produce the JSON / YAML / TOML /
  CBOR output for any of these numbers.
- The [Recipes](./recipes/quality-reports.md) chapter shows
  end-to-end examples of producing quality reports from these
  metrics, including pipelining them into dashboards.
