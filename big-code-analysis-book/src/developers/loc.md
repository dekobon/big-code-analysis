# Lines of Code (LoC)

How to implement the LoC metrics for a language, for contributors
adding or fixing a `Loc` impl.
[Lines of code](https://en.wikipedia.org/wiki/Source_lines_of_code)
is a software metric that indicates the size of source code by
counting its lines. There are several kinds of LoC; this page first
explains each by way of an example, then covers the implementation.

## Types of LoC

```rust
/*
Instruction: Implement factorial function
For extra credits, do not use mutable state or a imperative loop like `for` or `while`.
 */

/// Factorial: n! = n*(n-1)*(n-2)*(n-3)...3*2*1
fn factorial(num: u64) -> u64 {

    // use `product` on `Iterator`
    (1..=num).product()
}
```

The example above illustrates each of the **LoC** metrics described
below.

### SLOC

A straight count of all lines in the file including code, comments,
and blank lines.

METRIC VALUE: 11

### PLOC

A count of the instruction lines of code contained in the source
code. This includes any brackets or similar syntax on a new line.
Comments and blank lines are not counted.

METRIC VALUE: 3

### LLOC

The "logical" lines are a count of the statements in the code. What
a statement is depends on the language. In the example above there is
a single statement, the call of `product` with the `Iterator` as its
argument.

METRIC VALUE: 1

### CLOC

A count of the comment lines in the code. The type of comment does
not matter: single line, block, or doc. A Python docstring counts here
as well, and is also reported on its own as DLOC below.

METRIC VALUE: 6

### BLANK

A count of the blank lines in the code.

METRIC VALUE: 2

### DLOC

Documentation Lines Of Code: the rows the language treats as
documentation, such as a doc comment (the `/// Factorial` line above).
Documentation includes Python docstrings and
[Perl POD](https://perldoc.perl.org/perlpod) blocks. Those rows are
always counted in CLOC too, so DLOC never exceeds it. Each language
follows its documentation tool's rules
([Doxygen](https://www.doxygen.nl/),
[Javadoc](https://docs.oracle.com/en/java/javase/21/javadoc/javadoc.html),
[KDoc](https://kotlinlang.org/docs/kotlin-doc.html),
[JSDoc](https://jsdoc.app/), [PHPDoc](https://www.phpdoc.org/),
[C# XML docs](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/xmldoc/),
[LDoc](https://github.com/lunarmodules/ldoc),
[go/doc](https://pkg.go.dev/go/doc), [YARD](https://yardoc.org/)); see
the [DLOC reference](../metrics.md#lines-of-code). The SEI variant of
the Maintainability Index excludes DLOC from the comment lines used in
its formula. An Elixir `@doc` heredoc is an assignment that counts as
code.

METRIC VALUE: 1

## Whitespace-only files

Source that contains no token at all (a file of nothing but spaces,
tabs, and newlines) reports the rows it has: a four-row file of spaces
is `sloc 4`, `ploc 0`, `blank 4`, with or without a trailing newline.

This used to be the one input class where a trailing newline changed a
LoC value. Most grammars collapse tree-sitter's root node to a zero-width
node at end-of-input for such input rather than spanning the file, and
the file-level SLOC span was measured from that node, so those files
reported `sloc 0` when they ended in a newline and `sloc 1` when they did
not, while Elixir, Tcl, iRules and the `preproc` / `ccomment` helpers
kept the root span and reported the rows either way.

The file-level span is now anchored at line 1 rather than measured from
the root node's first token, so where the root node starts is no longer
observable in LoC and every grammar answers alike. The sweep that used to
pin the split, `whitespace_only_input_is_uniform_across_grammars` in
[/src/metrics/loc.rs](https://github.com/dekobon/big-code-analysis/blob/main/src/metrics/loc.rs),
now pins the absence of one.

The same anchoring is what makes leading blank lines count. A file
opening with three blank rows before its first token reports those rows
in `sloc` and `blank`, exactly as interior blank rows are reported.

## Implementation

Implement the `Loc` trait, and its `compute` function, for the
language. Each language's `impl Loc` lives in its own file under
[/src/metrics/loc/](https://github.com/dekobon/big-code-analysis/tree/main/src/metrics/loc)
(`rust.rs`, `python.rs`, …), with the row-billing helpers every arm
routes through in `shared.rs`; the tests stay in
[/src/metrics/loc.rs](https://github.com/dekobon/big-code-analysis/blob/main/src/metrics/loc.rs).
Read a sibling language's file for the shape.

Take care with the catch-all `_` arm that inserts a row into PLOC. If
the grammar surfaces its row terminator as a *token* rather than as
extra, as the Tcl family does, that token's start row is the row it
terminates, so the catch-all credits comment-only and blank rows to
PLOC. Give such tokens an explicit no-op arm.
