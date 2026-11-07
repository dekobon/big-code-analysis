# Formatting beyond `cargo fmt`

Two formatting rules `cargo fmt --check` cannot enforce: match arms
rustfmt silently declines to format, and where explanatory comments go
when code is consolidated behind a `macro_rules!`. It is for anyone
editing a file `make rustfmt-bail` names or writing a macro.

## Match arms rustfmt declines to format

A comment inside a match *pattern* (between the pattern and its `=>`,
or between `|` alternatives) makes rustfmt emit the whole enclosing
match verbatim, with no diagnostic and `cargo fmt --check` still
exiting 0. The blast radius is the match, not the file: statements
around it still format, which makes the file look unaffected. Macro
bodies are not exempt.

`make rustfmt-bail` (`utils/check-rustfmt-bail.py`, in `make lint`,
`make pre-commit` and `make ci`) measures it: it over-indents every
match arm, pipes each file through rustfmt on stdin, and counts the
arms that come back untouched. Per-file counts live in
`.rustfmt-bail-baseline.txt`; increases fail, decreases ratchet with
`--update`. `./utils/check-rustfmt-bail.py --show <file>` names each
stuck arm, which is what to run before editing.

An entry has one of three causes, and the gate cannot tell them apart:

1. A comment inside a match pattern. Hoist it above the arm; never
   delete it. The per-arm rationale in the classifier tables is
   load-bearing.
2. A `macro_rules!` body rustfmt cannot parse (a metavariable in a
   non-Rust position, as in `mk_langs!`). Permanent; the baseline
   header lists the known entries.
3. An arm header wider than `max_width`. Wrap it.

Do not open a campaign to clear the existing bails: the remaining
cause-1 entries are section comments inside long or-patterns, and
every way of buying the formatting back makes the table harder to
read. Hoisting a single comment with nothing to split is welcome when
you are already in the file.

After any bulk or scripted edit under `big-code-analysis-ast/src/getter/`
or `src/metrics/`, read the diff rather than trusting the gate, and
check line length directly:

```bash
awk 'length > 100 {print FILENAME":"FNR" ("length")"}' <files>
```

## Comments at macro call sites

When N near-identical predicates or impls are consolidated behind a
`macro_rules!`, keep the macro body structural and put each call's
language-specific rationale directly above the invocation:

```rust
// PHP's String2 alias is the `string` *type* keyword, excluded so
// `find string` reports literals; String3 is the hidden `_string`
// supertype (defensive arm + drift marker in the tests below).
impl_simple_is_string!(Php, String, String3, …);
```

The macro's own `///` doc describes the shape it emits, never per-call
particulars. Call-site comments survive a later refactor of the macro;
comments inside the body do not.

A `role_table!` invocation is the largest call site in the tree, and
the same rule applies per row. Where each kind of rationale lives:

| What it explains | Where |
| --- | --- |
| The grammar fact a row encodes: what the nodes are, how they nest, why an alias is listed or left off | `//` comment above the row |
| Why a `gated_by` / `text_gated_by` gate exists and what it reads | the `///` doc on the gate `fn` |
| Why an owner is `under [..]` / `not_under [..]` its parents | `//` comment above the row |
| Why a considered kind is not on the row | the `excludes [K "why"]` string; the coverage gate prints it |
| Which consumer reads which projection, and why it counts there | at the consumer, in `big-code-analysis` |
| What `owns`, `pays`, `opens` mean | `roles/mod.rs` only |

A row comment that explains a consumer's policy is wrong the day the
consumer changes; keep the grammar fact and the policy apart.
