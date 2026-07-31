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

## Where it currently bites

Ten sites, all under `src/metrics/`:

- `cognitive/`: `c.rs`, `cpp.rs`, `java.rs`, `mozcpp.rs`, `objc.rs`,
  `perl.rs`, `rust.rs`, plus the `js_cognitive!` macro body in
  `cognitive.rs`.
- `loc/`: `perl.rs` and `python.rs`.

`loc/tcl.rs` and `loc/irules.rs` were on that second list until #1135
hoisted their in-pattern comment above the arm; the `loc/` entries were
missing entirely until then, which is the point of the next paragraph.

That list is a snapshot — regenerate it rather than trusting it, since
it moves whenever an arm gains or loses a comment, and since a
directory nobody has swept yet reads as clean. The script below takes
the directory as an argument; run it over each `src/metrics/*/` family
rather than assuming `cognitive/` is the only one affected.

```bash
# Prints modules rustfmt refuses to format. Run from the repo root, e.g.
#   ./thisscript src/metrics/cognitive
#   ./thisscript src/metrics/loc
dir=${1:?usage: $0 <dir>}; tmp=$(mktemp -d); cp "$dir"/*.rs "$tmp/"
for f in "$tmp"/*.rs; do
  b=$(basename "$f")
  # Over-indent the first block-bodied arm, then see if rustfmt puts it back.
  line=$(rg -n '^\s+[A-Za-z_:]+.*=> \{' "$f" | head -1 | cut -d: -f1)
  [ -z "$line" ] && { echo "SKIP  $b (no block-bodied arm)"; continue; }
  python3 -c "
import pathlib
p = pathlib.Path('$f'); L = p.read_text().split('\n')
L[$line - 1] = ' ' * 30 + L[$line - 1].strip()
p.write_text('\n'.join(L))"
  # Do not discard stderr: a file with `mod` decls errors out, which is
  # not the same thing as a bail.
  err=$(rustfmt --edition 2024 "$f" 2>&1)
  [ -n "$err" ] && { echo "ERROR $b: $err"; continue; }
  if sed -n "${line}p" "$f" | rg -q '^ {26,}'; then echo "BAILS $b"; else echo "ok    $b"; fi
done; rm -rf "$tmp"
```

Checking the perturbed line *by number* matters: grepping for the
restored indentation finds some other already-correct line and reports
a false "formatted".

## Why it matters

A bulk or regex-driven edit across sibling modules is exactly the change
that produces misformatted output, and it lands in exactly the files
where the gate cannot see it. During #1086 a regex rewrite of 43 call
sites left hanging-argument calls and over-length lines across several
of these files; `cargo fmt --all` followed by `cargo fmt --all --check`
reported clean, and every one was found by reading the diff instead.

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
- To decide whether a specific region is affected, perturb *that region*
  and check *that line* by number. Two traps, both of which have already
  produced a wrong answer here:
  - Suppressing stderr. A leaf module formats standalone, but a file
    with `mod` declarations cannot resolve them outside its own tree, so
    rustfmt errors out — which reads as a bail if you discard the
    message.
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
