# Formatting Rule

`cargo fmt --check` is not a complete formatting gate. It exits `0`
over code rustfmt declined to format, and rustfmt declines silently.

## A comment inside a match pattern disables that match

When a comment sits inside a match *pattern* — between the pattern and
its `=>`, or between `|` alternatives — rustfmt emits the enclosing
match expression verbatim. No warning, no diagnostic, and
`cargo fmt --check` still exits `0`.

Both spellings trigger it:

```rust
// Block comment before `=>`
Else /* else-if also */ => { … }

// Line comments between `|` alternatives
P::IfStatement
| P::UnlessStatement
// Postfix conditional / loop forms (`return 1 if $cond;`) — the
// condition is a real cognitive branch.
| P::IfSimpleStatement
| P::ForSimpleStatement => { … }
```

A comment *above* an arm, outside the pattern, is fine and formats
normally.

**The blast radius is the match, not the file or the item.** Statements
before and after the match in the same function are still formatted. A
misindented line two statements below a bailing match gets fixed, which
makes it easy to conclude — wrongly — that the file is unaffected.

**`macro_rules!` bodies are not exempt and are not a separate cause.**
rustfmt formats macro bodies by default (`format_macro_bodies`, default
`true`), so a macro body containing a match with an in-pattern comment
bails for exactly the reason above. In this repository the
`js_cognitive!` body in `src/metrics/cognitive.rs` is unformatted solely
because of its `Else /* else-if also */ =>` arm; delete that comment and
rustfmt reformats the whole body.

## Measuring it

`make rustfmt-bail` (`utils/check-rustfmt-bail.py`, #1136) is the
measurement. It over-indents **every** match arm in every tracked Rust
file, pipes each through `rustfmt --emit stdout`, and counts the arms
that come back untouched. Per-file counts live in
`.rustfmt-bail-baseline.txt`; increases fail, decreases are silent and
ratchet with `--update`. It is wired into `make lint`, so
`make pre-commit` and `make ci` already run it.

`./utils/check-rustfmt-bail.py --show <file>` names each stuck arm with
its line number, which is what you want before editing anything.

Three things that version gets right and the hand-rolled probe below
did not:

- **It feeds rustfmt on stdin.** Given a *path*, rustfmt resolves and
  recurses into `mod` declarations, so `src/getter.rs` and
  `src/metrics/cognitive.rs` error out with "file not found for module"
  — which reads exactly like a bail if stderr is discarded. On stdin
  there is nothing to resolve, so those two probe like any other file.
  There is no unprobeable-file class.
- **It ignores `=>` inside string literals and comments.** A bare regex
  over the text matches the JS, C# and TypeScript fixtures these test
  modules are full of (`const add = (a, b) => a + b`, `public int W =>
  _w`) and Groovy's spaceship (`a <=> b`). rustfmt never rewrites string
  contents, so every one of those reads as a permanent bail: the naive
  probe reported 15 in `src/metrics/nom.rs`, 8 in `wmc.rs`, 3 in
  `nargs.rs` and 37 in `cyclomatic.rs`, and **all of them were string
  literals**. Do not act on a count from a probe that skips this step.
- **It probes every arm, not the first.** The bail is match-scoped, so a
  file whose first `match` formats cleanly still hides a later one that
  does not (`src/getter/c.rs`), and a module whose arms are all
  expression-bodied (`… => HalsteadType::Operator,` in
  `src/getter/go.rs`) has no `=> {` to probe at all. Over `src/getter`
  the first-arm-only version gave 11 false verdicts out of 18.

## Two causes, one measurement

An entry in the baseline means rustfmt declined to format that region.
It does not tell you why, and there is more than one why:

1. **A comment inside a match pattern** — this rule. Hoist it above the
   arm.
2. **A `macro_rules!` body rustfmt cannot parse** — a metavariable in a
   position that is not valid Rust, such as the pattern repetition
   `$ternary $(| $short_circuit)+` in `impl_cyclomatic_c_family!`. No
   comment is involved and hoisting one fixes nothing.

Metavariables alone do not cause a bail: `js_cognitive!` interpolates
`$lang` and still formats once its in-pattern comment is hoisted. So
check with `--show` before concluding either way. The baseline file
names the known cause-2 entries.

## Where it currently bites

Thirty-six cause-1 sites, and **not** confined to `src/metrics/` — the
per-language `Getter` modules are the largest cluster:

- `src/getter/`: `bash.rs`, `c.rs`, `cpp.rs`, `csharp.rs`, `elixir.rs`,
  `go.rs`, `groovy.rs`, `irules.rs`, `java.rs`, `kotlin.rs`, `lua.rs`,
  `mozcpp.rs`, `objc.rs`, `perl.rs`, `php.rs`, `python.rs`, `ruby.rs`,
  `tcl.rs` — 18 of the 25 modules there. Plus the JS-family macro body
  in `src/getter.rs`.
- `src/metrics/cognitive/`: `c.rs`, `cpp.rs`, `java.rs`, `mozcpp.rs`,
  `objc.rs`, `perl.rs`, `rust.rs`, plus the `js_cognitive!` macro body
  in `cognitive.rs`.
- `src/metrics/cyclomatic/`: `irules.rs`, `perl.rs`, `php.rs`,
  `ruby.rs`.
- `src/metrics/abc/`: `elixir.rs`, `perl.rs`, `php.rs`.
- `src/metrics/loc/`: `perl.rs` and `python.rs`.

`loc/tcl.rs` and `loc/irules.rs` were on that last list until #1135
hoisted their in-pattern comment above the arm.

That list is a snapshot — run the gate rather than trusting it, since it
moves whenever an arm gains or loses a comment, and since a directory
nobody has swept yet reads as clean. The `src/metrics/`-only framing
survived two revisions of this file for exactly that reason, while
`src/getter/` — where the bail is close to universal — went unmentioned.
The gate now sweeps every tracked Rust file, so no directory can go
unmentioned again.

## Why it matters

A bulk or regex-driven edit across sibling modules is exactly the change
that produces misformatted output, and it lands in exactly the files
where the gate cannot see it. During #1086 a regex rewrite of 43 call
sites left hanging-argument calls and over-length lines across several
of these files; `cargo fmt --all` followed by `cargo fmt --all --check`
reported clean, and every one was found by reading the diff instead.

## How to apply

- After any bulk, scripted, or regex edit under `src/getter/` or
  `src/metrics/`, read the resulting diff rather than trusting the fmt
  gate. Check indentation and line length by eye.
- Line length is worth a direct check, since it is mechanical:

  ```bash
  awk 'length > 100 {print FILENAME":"FNR" ("length")"}' <files>
  ```

  Expect some pre-existing hits in test string literals containing `\n`;
  those cannot be wrapped and are not what you are looking for.
- To decide whether a specific region is affected, run
  `./utils/check-rustfmt-bail.py --show <file>`; it names each stuck arm
  by line. If you perturb by hand instead, three traps, all of which
  have already produced a wrong answer here:
  - Suppressing stderr. A leaf module formats standalone, but if you
    pass rustfmt a *path* to a file with `mod` declarations it cannot
    resolve them outside its own tree and errors out — which reads as a
    bail if you discard the message. Feeding the text on stdin avoids
    the whole class.
  - Counting `=>` inside string literals. These modules are full of JS
    and C# fixtures; rustfmt never rewrites string contents, so a probe
    that does not skip them reports permanent bails in files that have
    no bail at all.
  - Grepping for the restored indentation instead of checking the
    perturbed line. Another untouched line elsewhere in the file matches
    the pattern and reports a false "formatted". Likewise, perturbing a
    function that does not contain the bailing match proves nothing,
    because the bail is match-scoped.
- Do not remove an explanatory comment merely to re-enable formatting.
  The per-arm rationale in these modules is load-bearing; hoisting it
  above the arm (outside the pattern) is a legitimate fix when the
  comment reads just as well there, but manual formatting review is
  otherwise the cheaper trade.
