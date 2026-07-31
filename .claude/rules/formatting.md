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

Thirty-six sites, and **not** confined to `src/metrics/` — the
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

That list is a snapshot — regenerate it rather than trusting it, since
it moves whenever an arm gains or loses a comment, and since a
directory nobody has swept yet reads as clean. The `src/metrics/`-only
framing survived two revisions of this file for exactly that reason,
while `src/getter/` — where the bail is close to universal — went
unmentioned. Run the script over every directory of per-language
modules, not just the one you happen to be editing.

The script over-indents **every** arm header in the file, not just the
first. Probing one arm is not enough: the bail is match-scoped, so a
file whose first `match` formats cleanly still reports `ok` while a
later one bails (`src/getter/c.rs`), and a module whose arms are all
expression-bodied (`… => HalsteadType::Operator,` in
`src/getter/go.rs`) has no `=> {` to probe at all. Both shapes read as
clean.

```bash
# Prints modules rustfmt refuses to format. Run from the repo root, e.g.
#   ./thisscript src/getter
#   ./thisscript src/metrics/cognitive
dir=${1:?usage: $0 <dir>}; tmp=$(mktemp -d); cp "$dir"/*.rs "$tmp/"
for f in "$tmp"/*.rs; do
  python3 - "$f" <<'PY'
import pathlib, re, subprocess, sys

path = pathlib.Path(sys.argv[1])
lines = path.read_text().split("\n")
arms = [i for i, l in enumerate(lines)
        if re.match(r"^\s{4,}(_|[A-Za-z_|].*?)\s*(if .*)?=>", l)]
if not arms:
    print(f"SKIP  {path.name} (no match arms)")
    raise SystemExit
# Over-indent every arm header, then see which ones rustfmt puts back.
want = {i: lines[i].strip() for i in arms}
for i in arms:
    lines[i] = " " * 30 + want[i]
path.write_text("\n".join(lines))
run = subprocess.run(["rustfmt", "--edition", "2024", str(path)],
                     capture_output=True, text=True)
# Do not discard stderr: a file with `mod` decls errors out, which is
# not the same thing as a bail.
if run.returncode or run.stderr.strip():
    print(f"ERROR {path.name}: {run.stderr.strip().splitlines()[0]}")
    raise SystemExit
after = path.read_text().split("\n")
pad = " " * 30
kept = {l[30:] for l in after if l.startswith(pad)}
stuck = set(want.values()) & kept
print(f"BAILS {path.name}: {len(stuck)} arm(s)" if stuck
      else f"ok    {path.name}")
PY
done; rm -rf "$tmp"
```

`ERROR` is not `BAILS`. A file carrying `mod` declarations cannot
resolve them outside its own tree, so rustfmt refuses the whole file;
for those (`src/getter.rs`, `src/metrics/cognitive.rs`) perturb one
line in place, run `cargo fmt --all`, and check that line by number.
Doing that to `src/getter.rs`'s macro body is the sharpest single
demonstration of this whole rule: `cargo fmt --all` exits `0` and
leaves the line over-indented.

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
