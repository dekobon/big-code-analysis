# Macro Comment Hoisting Rule

When consolidating N near-identical predicates, dispatch arms, or
trait impls behind a `macro_rules!` (`impl_js_family_get_op_type!`,
`impl_simple_is_string!`, `impl_cyclomatic_java_like!`, the various
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
sees only `impl_simple_is_string!(Php, String, String2, String3);`
and has no clue that `String3` is the hidden `_string` supertype
or that PHP's grammar emits multi-alias annotation-type strings.
The comment is two screens away, attached to a token-tree the
reader has no reason to scroll to.

## How to apply

At each macro invocation, place the language-specific rationale
*directly above* the call:

```rust
// PHP's grammar emits String2 for the anonymous `string` keyword
// alias (see lesson #2, fixed in #288) and lists String3 as the
// hidden `_string` supertype (lesson #34 — defensive arm + drift
// marker in tests below).
impl_simple_is_string!(Php, String, String2, String3);
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
