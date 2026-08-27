# Lines of Code (LoC)

In this document we give some guidance on how to implement the LoC
metrics available in this crate.
[Lines of code](https://en.wikipedia.org/wiki/Source_lines_of_code)
is a software metric that gives an indication of the size of some
source code by counting the lines of the source code.
There are many types of LoC so we will first explain those by way
of an example.

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

The example above will be used to illustrate each of the **LoC** metrics described below.

### SLOC

A straight count of all lines in the file including code, comments, and blank lines.  
METRIC VALUE: 11

### PLOC

A count of the instruction lines of code contained in the source
code. This would include any brackets or similar syntax on a new
line.
Note that comments and blank lines are not counted in this.
METRIC VALUE: 3

### LLOC

The "logical" lines is a count of the number of statements in the
code. Note that what a statement is depends on the language.
In the above example there is only a single statement which id the
function call of `product` with the `Iterator` as its argument.
METRIC VALUE: 1

### CLOC

A count of the comments in the code. The type of comment does not matter ie single line, block, or doc.  
METRIC VALUE: 6

### BLANK

Last but not least, this metric counts the blank lines present in a code.
METRIC VALUE: 2

## Whitespace-only files

Source that contains no token at all — a file of nothing but spaces,
tabs, and newlines — reports the rows it has: a four-row file of spaces
is `sloc 4`, `ploc 0`, `blank 4`, with or without a trailing newline.

This used to be the one input class where a trailing newline changed a
LoC value. Most grammars collapse tree-sitter's root node to a zero-width
node at end-of-input for such input rather than spanning the file, and
the file-level SLOC span was measured from that node — so those files
reported `sloc 0` when they ended in a newline and `sloc 1` when they did
not, while Elixir, Tcl, iRules and the `preproc` / `ccomment` helpers
kept the root span and reported the rows either way.

The file-level span is now anchored at line 1 rather than measured from
the root node's first token, so where the root node starts is no longer
observable in LoC and every grammar answers alike. The sweep that used to
pin the split — `whitespace_only_input_is_uniform_across_grammars` in
[/src/metrics/loc.rs](https://github.com/dekobon/big-code-analysis/blob/main/src/metrics/loc.rs)
— now pins the absence of one.

The same anchoring is what makes leading blank lines count. A file
opening with three blank rows before its first token reports those rows
in `sloc` and `blank`, exactly as interior blank rows are reported.

## Implementation

To implement the LoC related metrics described above you need to
implement the `Loc` trait for the language you want to support.

This requires implementing the `compute` function.
See
[/src/metrics/loc.rs](https://github.com/dekobon/big-code-analysis/blob/main/src/metrics/loc.rs)
for where to implement, as well as examples from other languages.

Take care with the catch-all `_` arm that inserts a row into PLOC. If
the grammar surfaces its row terminator as a *token* rather than as
extra — as the Tcl family does — that token's start row is the row it
terminates, so the catch-all credits comment-only and blank rows to
PLOC. Give such tokens an explicit no-op arm.
