# Grammar Dispatch Rule

Every per-language metric impl, `Checker` predicate, and `Getter`
classifier is a `match` over `node.kind_id()`. **The arm list is a
coverage claim, not a specification** — the compiler cannot tell you it
is incomplete, and a metric that scores a valid construct as zero looks
exactly like a construct that legitimately scores zero.

Nothing in the standard gates catches any failure below. There is no
compile error, no clippy warning, and no test failure unless a fixture
exercises the exact shape *and* asserts a value above the base.

The checklist applies whenever you add a language, bump a grammar pin,
add an arm to a dispatch table, or copy an arm from a sibling module.

## 1. Match every aliased variant

tree-sitter emits N distinct `kind_id`s for one grammar rule used in N
positions: an unsuffixed `PrimitiveType` plus `PrimitiveType2`,
`PrimitiveType3`, …, all mapping to the same `node.kind()` string. They
are different `u16`s. Matching only the unsuffixed one compiles, runs,
and returns wrong numbers.

Confirm every numeric-suffix variant of every matched rule is either
listed or excluded with a comment:

```bash
rg 'Lang::([A-Za-z]+)\b' src/getter/ src/checker/ src/alterator.rs \
   src/spaces.rs src/metrics/
```

The bug class reaches every match on a grammar rule — `alterator.rs`,
`spaces.rs`, and `src/metrics/*` are as susceptible as `getter.rs` and
`checker.rs`. Centralised alias sets live in `src/macros/kind_sets.rs`;
prefer extending those over open-coding a list. When a rule has many
aliases, prefer one `node.kind()` string comparison over enumerating
seventeen variants — pay the small runtime cost for forward
compatibility.

**After bumping a grammar pin, regenerate the enum first.** Bump both
manifests in lockstep (root `Cargo.toml` and `enums/Cargo.toml` — the
excluded crate cannot inherit the workspace pin), then:

```bash
cargo run --manifest-path ./enums/Cargo.toml -- -lrust -o ./src/languages
```

Stable *named*-node ids are not evidence the ids held. Inserting one
anonymous terminal renumbers the whole anonymous block after it, so
`Else` can silently become `)`. Spot-check a known anonymous token with
`bca dump` against the enum.

## 2. Skip hidden-rule variants, but mark them

A rule whose name begins with `_` (`_string`, `_multiline_string_literal`)
is hidden: the variant exists in the enum and the parser never emits it.
Check the `Lang::Variant => "name"` arm in
`src/languages/language_<lang>.rs` before listing a "looks like an alias"
variant. Keep the defensive arm *and* pin its hidden status with a
`!ast_has_kind_id(&parser, Lang::HiddenVariant as u16)` assertion naming
the hidden rule — otherwise a future grammar that promotes the rule
changes behaviour invisibly.

## 3. Address children by role, never by index

Use `child_by_field_name("condition")` over `node.child(1)` for any slot
the grammar can precede with an optional preamble. The field name
survives a grammar re-order; the index does not.

The preambles that have already produced silent miscounts: an
init-statement (`if x := f(); x`), a `constexpr` keyword
(`if constexpr (cond)`), a named-argument label (`m(name: $a)` vs
`m($a)`), and BLANK ALIAS bodies (Lua `repeat … until`). When the
grammar exposes no field for the slot, iterate
`node.named_children(cursor)` and pick by role, with a comment naming
the variant set you verified.

**Minimum test bar for a new dispatcher arm:** one fixture per optional
preamble the grammar permits, not just the form already in the corpus.

## 4. Re-derive a sibling's gate against the target grammar

The semantic role transfers across languages; the node *kind* does not.
A copied child-kind gate compiles, runs, and matches nothing.

The jump label is the clearest case: it surfaces as
`StatementIdentifier` in the JS family, `Identifier` in Java and Groovy,
`LabelName` in Go, and `Label` in Perl.

`is_else_if` is the largest, and the one to consult before writing a
new one. **Re-derive it rather than trusting the list below** — it moves
whenever a language is added:

```bash
rg -o 'impl_is_else_if_(\w+)!\(\s*(\w+)' -r '$1 $2' src/checker/ --no-filename | sort
rg -l 'fn is_else_if' src/checker/          # hand-written impls
```

| Strategy | Macro | Languages |
| --- | --- | --- |
| `parent()` is the else-clause | `impl_is_else_if_parent_clause!` | C, Cpp, Mozcpp, Objc, Rust, Javascript, Mozjs, Typescript, Tsx |
| `prev_sibling()` is the `else` keyword | `impl_is_else_if_prev_sibling!` | Csharp, Groovy, Java, Kotlin |
| the clause node *is* the else-if | `impl_is_else_if_clause!` | Bash, Irules, Lua, Perl, Ruby, Tcl |
| hand-written | — | Go, PHP, Python |
| constant `false` | — | Elixir |

Two things that list is worth reading for. The third macro exists
because some grammars emit a dedicated `elif_clause` / `ElsifClause` /
`Elseif` node with no nested `if` to inspect at all — a shape neither of
the other two strategies can express. And Elixir returning a constant is
a stub, not a strategy: per lesson 10, treat that as a to-do.

Dump the AST for the construct in the *target* grammar before reusing a
sibling's predicate, and add a fixture that exercises the gated branch —
a suite of only unlabelled inputs proves nothing about a label gate.

## 5. One kind per operator; parent-guard compound leaves

A grammar routinely emits an operator as a wrapper node *containing* the
keyword token (`await a()` is an await-expression wrapping an `await`
token). Halstead keys on `kind_id`, so listing both counts every
occurrence twice and inflates `n1` through the two distinct ids.
Classify exactly one kind and verify the stream with `bca ops`.

The inverse trap: a compound operator (`not in`, `is not`) wraps leaves
that are each valid operators elsewhere. Classify the compound and
return `Unknown` for the inner leaf **only when its parent is the
compound** — a blanket suppression drops every standalone `not x`,
`a in b`, `for x in y`.

The trap generalises past Halstead: any predicate listing both a
container and a kind it can contain double-counts. Ruby's
`is_closure` matching `Lambda | Block | DoBlock` scored one stabby
lambda as two closures for the same reason.

## 6. Narrow a dispatch set with a gate, never by deletion

When a construct over-counts because its node sits in `is_func` /
`is_func_space` while its children are also functions, deleting the
construct's kind regresses its **childless** variant to zero — the form
with no qualifying children relied on that membership to be counted at
all. C# expression-bodied indexers and properties (`this[i] => …;`,
`int W => _w;`) each hit this.

Gate the arm on child-presence (`!csharp_member_has_accessors(node)`)
and keep `is_func`, `is_func_space`, and `get_space_kind` gated by the
*same* predicate, so the space tree never disagrees with itself.

The same rule decides which node *keeps* a classification when a
wrapper/leaf pair double-counts: the keeper is the node that exists for
**every** spelling of the construct, verified with `bca dump` rather
than assumed. PHP's `primitive_type` is childless for `callable`,
`iterable`, `mixed`, `void`, `false` and `true`, so keeping the keyword
leaf — the "obvious" innermost choice — would have scored six types
zero (#1293).

## 7. Walk the sibling predicates for parity

`Checker::is_string`, `Getter::get_op_type`, `Checker::is_call`,
`Checker::is_func_space`, and the per-metric body walkers all classify
the same nodes through parallel `matches!()`. Two that disagree drift
silently: `find string` counts a node that Halstead calls `Unknown`.
Each predicate reads as internally consistent, so the disagreement is
invisible from either one alone.

Minimum cross-walk when you edit one:

- `is_string` ↔ `get_op_type` operand classification of string kinds
- `is_call` ↔ `get_op_type` operator classification of call kinds
- `is_func_space` ↔ each metric's body walker
- `is_func_space` ↔ `get_space_kind` — the two halves of the space
  tree, and the pair that fails *quietly*. The three above disagree
  into a **wrong count**, which a snapshot diff shows you. This one
  disagrees into an **absent key**: a node the walker promoted but the
  getter left `Unknown` is not a member scope
  (`SpaceKind::is_member_scope`), so its space serializes no `npm` /
  `npa` block at all — which looks exactly like a language that
  legitimately has no containers. Nothing diffs. §6's "gate all three
  on the same predicate" is this bullet's fix.

### Cross-walk the `_with_code` spelling, not the byte-less one

When a predicate feeds anything the walker also consults, reach for the
`_with_code` variant — that is what the walk actually calls.
`spaces::compute` promotes a node with
`Checker::promotes_to_func_space_with_code`, labels the resulting space
with `Getter::get_space_kind_with_code` inside `open_func_space`, and
`note_member_scope` hands that recorded kind to `npm` / `npa`.

**The trap is silent because the defaults forward.**
`Checker::is_func_space_with_code` and `Getter::get_space_kind_with_code`
both discard `code` and `ancestors` and call the byte-less form, so the
two spellings compile *and behave identically* everywhere except where a
language overrides one — which is precisely where the answer matters. A
byte-less call site therefore reads as correct against every language
that has no override, and is wrong only on the one that does. You cannot
recover this from the signatures; both look total.

Elixir is that language, and currently the only one overriding either
method. `defmodule` / `def` / `defp` are not distinct grammar
productions — they parse as `Call` nodes told apart only by their target
identifier text (#275) — so `ElixirCode::is_func_space` lists just
`Source | AnonymousFunction` and `ElixirCode::get_space_kind` has no
`defmodule` arm. The byte-less pair answers `(false, Unknown)` for every
Elixir container.

This has already landed once. `ops_inner` opened spaces on the byte-less
`is_func || is_func_space`, so `bca ops` returned a bare file-level space
for every Elixir input while `bca metrics` returned a full
module/function tree — no error, no wrong number, just a missing tree
(#1130). `tests/parity/ops_metrics_space_parity.rs` exists to keep the
two walks agreeing.

## 8. Anchor a default/fallback exclusion to the construct

A `default` / `else` / `_` arm is +0 only when the construct's *own*
node already paid the nesting or decision increment. The same node kind
routinely serves both a switch-like construct (arm is +0) and a chain
construct (arm is a real +1): Ruby's `Else` is +0 under `case` — which
pays nesting — and +1 under `begin`/`rescue`, which does not.

Confirm which construct owns the arm before suppressing, and never
blanket-suppress a shared token. Where ABC and cyclomatic must agree,
reuse cyclomatic's existing gate rather than re-deriving the predicate,
and pin `abc.conditions == cyclomatic() - 1` per space (not
`cyclomatic_sum()`, which carries a per-space base of 1).

## 9. Watch for control flow with no dedicated kind

In command-dispatched languages (Tcl, iRules, shell-like grammars) a
construct that is semantically control flow may parse as the same
generic `command` node as any other builtin. There is no missing arm to
find — the *kind* the dispatcher would need does not exist. Tcl's
`switch` scored cognitive 0 for exactly this reason.

Recognise these out-of-band by the command's leading word
(`name` field == `"switch"`) and locate sub-parts by **structural
position** (the sole trailing `braced_word`), not a fixed index, so
optional `-exact` / `-glob` / `--` flags do not break detection.

## 10. Read the bytes for identity questions

Structural shape answers "is this an attribute access." It never
answers "whose attribute is it." An `Attribute` whose first child is an
`Identifier` is `self.x`, and equally `db.x` and `logger.x` — Python NPA
counted all three as class attributes until it read the receiver.

When correctness hinges on *which* object or name, compare the source
bytes with `code.get(start..end)`. Never `to_string_lossy()` (see
`AGENTS.md`). When the language encodes semantics in bare identifier
text (Ruby `private`, Elixir keyword `Call`s), the metric trait needs
`&[u8]` in `compute` — plan the widening with the impl, not as a
follow-up, and standardise on
`<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats)`.

## 11. Give one input to each independent path

When two structurally independent paths sum into the same field — a
token arm classifying leaves and a walker arm descending containers —
a fixture covered by *either* reads the right total. The dead path is
invisible from the result. C# ABC's `if`/`while` walker was dead for
every existing test because each fixture's condition contained a
comparison operator that an independent token arm already counted.

Write at least one fixture only the path under test can classify — a
bare identifier `if (x)` for a walker arm, an empty container for an arm
that descends — and test-via-revert that arm alone per
[`testing.md`](testing.md).

## When you fix one language, sweep the rest

Every item above is a per-language failure that almost always exists in
siblings. `src/languages/` modules are deliberate clones, so the fix for
one is the audit table for the other twenty. Build that table in the
issue, land the sibling fixes in one commit so the symmetry is visible
to a reviewer, and anchor any known-wrong-but-unfixed case with an
inline `FIXME(#NNN)` so the gap stays in CI rather than in a tracker.

A `LANG` variant that owns **no file extension** (`mozcpp`) gets no
integration-snapshot coverage at all, because nothing routes to it —
pin it with a parity test against its extension-owning sibling instead.
