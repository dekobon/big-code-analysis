# Formatting Rule

`cargo fmt --check` is not a complete formatting gate. It exits `0`
over code rustfmt declined to format, and rustfmt declines silently.

## A comment inside a match pattern disables the whole enclosing item

When a comment sits inside a match *pattern* — between the pattern and
its `=>`, or between `|` alternatives — rustfmt gives up on the entire
enclosing item and emits it verbatim. No warning, no diagnostic, and
`cargo fmt --check` still exits `0`.

Both spellings trigger it:

```rust
// Block comment before `=>`
Else /*else-if also */ => { … }

// Line comments between `|` alternatives
P::IfStatement
| P::UnlessStatement
// Postfix conditional / loop forms (`return 1 if $cond;`) — the
// condition is a real cognitive branch.
| P::IfSimpleStatement
| P::ForSimpleStatement => { … }
```

In `src/metrics/cognitive/` this currently affects seven modules:
`c.rs`, `cpp.rs`, `java.rs`, `mozcpp.rs`, `objc.rs`, `perl.rs`,
`rust.rs`. Every other module in that directory formats normally.

**Separately**, rustfmt never formats `macro_rules!` bodies at all. Code
inside `js_cognitive!` in `src/metrics/cognitive.rs` is unformatted for
that reason, not this one — do not conflate the two when deciding
whether an edit needs manual review.

## Why it matters

A bulk or regex-driven edit across sibling modules is exactly the change
that produces misformatted output, and it lands in exactly the files
where the gate cannot see it. During #1086 a regex rewrite of 43 call
sites left six files with a hanging-argument call and two lines over 100
columns; `cargo fmt --all` followed by `cargo fmt --all -- --check`
reported clean, and the problems were found only by reading the diff.

## How to apply

- After any bulk, scripted, or regex edit under `src/metrics/`, read the
  resulting diff rather than trusting the fmt gate. Check indentation
  and line length by eye.
- Line length is worth a direct check, since it is mechanical:

  ```bash
  awk 'length > 100 {print FILENAME":"FNR" ("length")"}' <files>
  ```

  Expect some pre-existing hits in test string literals containing `\n`;
  those cannot be wrapped and are not what you are looking for.
- To determine whether a specific file is affected, perturb it rather
  than guessing from its comments. Copy it to a scratch directory,
  misindent one line, run `rustfmt --edition 2024` on the copy, and see
  whether the indentation is restored. Do not suppress stderr — a leaf
  module formats standalone, but a file with `mod` declarations fails to
  resolve them outside its tree and rustfmt errors out, which is easy to
  misread as a bail.
- Do not remove an explanatory comment merely to re-enable formatting.
  The per-arm rationale in these modules is load-bearing (see
  `macro-comments.md`); manual formatting review is the cheaper trade.
