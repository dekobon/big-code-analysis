# Macro Comment Hoisting Rule

When consolidating N near-identical predicates, dispatch arms, or
trait impls behind a `macro_rules!` (`impl_cyclomatic_rows!`,
`impl_exit_from_row!`, `impl_else_if_from_row!`, the various
`c_macro` / `c_langs_macros` helpers), keep the macro body minimal
and hoist *per-call* rationale comments above the macro invocation,
not into the macro definition.

## Why

Each pre-consolidation impl typically accumulated language-specific
explanatory comments over months: "why does PHP include `String3`?",
"why does Bash omit `_heredoc_body`?", "why is Groovy's `Assert`
arm distinct?", "what does `OptionalChain` vs `QMARKDOT` cost us in
test fixtures?". Those comments are not redundant — each answers
a question only its language's grammar quirks produced.

If they collapse into the macro body, the reader at the call site
sees only `impl_simple_is_string!(Php, String, String3, …);` and has
no clue that `String3` is the hidden `_string` supertype, or that the
`String2` alias missing from that list is the `string` *type* keyword
rather than an oversight. The comment is two screens away, attached
to a token-tree the reader has no reason to scroll to.

## How to apply

At each macro invocation, place the language-specific rationale
*directly above* the call:

```rust
// PHP's String2 alias is the `string` *type* keyword, excluded so
// `find string` reports literals (#288 listed it, #1474 withdrew
// it); String3 is the hidden `_string` supertype (lesson #34 —
// defensive arm + drift marker in tests below).
impl_simple_is_string!(Php, String, String3, …);
```

The macro body itself should contain only the structural pattern —
not language-specific narrative. The macro's *own* doc comment
(`///`) can describe the shape (`takes a language, a list of
variants, emits a `matches!()` against them`) but not per-call
particulars.

## Failure mode this guards against

When the macro is later refactored or extended (a new language is
added, a new slot is introduced, the macro is replaced by something
narrower), the per-call comments survive the change — they are
attached to each invocation, not to the macro shape. If they were
inside the macro definition, refactoring the macro would risk
deleting them or making them irrelevant to the new shape.

Lesson #34 documents the related case of hidden-rule variants;
this rule is the call-site discipline that makes those defensive
arms self-explanatory at the call site even after another round
of macro consolidation.

## Row comments in a `role_table!`

A `role_table!` invocation (`big-code-analysis-ast/src/roles/<lang>.rs`)
is the largest macro call site in the tree, and the same rule applies
one row at a time. The module doc of `roles/mod.rs` ("Reading a row
from a consumer") states what a row's comment may and may not say —
the grammar, not the consumer's policy; cite it rather than restating
it. Where each kind of rationale goes:

| what it explains | where it lives |
| --- | --- |
| the grammar fact a row encodes — what the construct's nodes are, how they nest, why an alias is listed or left off | a `//` comment on the line above the row, inside the invocation |
| why a gate exists and what it reads (`gated_by f`, `text_gated_by f`) | the `///` doc on the gate `fn` beside the table |
| why an owner is gated `under [..]` / `not_under [..]` its parents — a gate with no `fn` to carry it | the `//` comment above the row, as the grammar fact it is |
| why a kind the author considered is not on the row | the `excludes [K "why"]` string itself — it is read by the coverage gate and printed with the residue |
| which consumer reads which projection, and why it counts there | at the consumer, in `big-code-analysis` |
| the shape of the macro — what `owns`, `pays`, `opens` mean | `roles/mod.rs` only, never a row comment |

Groovy's `Switch` row after #1701 is the model: the `//` comment above
it states the grammar fact, the `excludes` string the rejection, and
the consumer's reading sits in `src/metrics/cognitive/groovy.rs`.

The failure mode is the same as above: a row comment that explains a
consumer's reading is wrong the day that consumer changes, and the
reader deciding whether the row is right has no way to tell the stale
policy sentence from the grammar fact beside it.
