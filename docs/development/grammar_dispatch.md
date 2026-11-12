# Grammar dispatch checklist

Every per-language `Checker` predicate, `Getter` classifier and metric
arm asks a question of a node's kind. Membership (which kinds own a
construct, which tokens pay for it) is declared once per language in
`big-code-analysis-ast/src/roles/<lang>.rs` and read through the
predicates `role_table!` emits. The module doc of `roles/mod.rs` is the
reference for the row grammar, what a table emits, how a consumer reads
a row, and the section "What the mechanism cannot see". Read that
section first; this checklist is the manual half it describes.

A kind list is a coverage claim, not a specification. The compiler
cannot tell you it is incomplete, and a metric that scores a valid
construct as zero looks exactly like a construct that legitimately
scores zero. The three role gates (`make role-census`, `role-coverage`,
`role-bypass`) and the derived parity suite catch a new unaccounted
kind, a restated list inside the consumer boundary, and two consumers
disagreeing on a corpus node. None judges whether a row's membership is
right. Apply the checklist whenever you add a language, bump a grammar
pin, add a kind to a row, or copy a row or gate from a sibling.

## 1. Match every aliased variant

tree-sitter emits one `kind_id` per grammar position, so a rule used in
N positions is `PrimitiveType`, `PrimitiveType2`, … with one `kind()`
string. Matching only the unsuffixed one returns wrong numbers. List or
exclude (with a reason) every numeric-suffix variant:

```bash
rg 'Lang::([A-Za-z]+)\b' big-code-analysis-ast/src/getter/ big-code-analysis-ast/src/checker/ \
   big-code-analysis-ast/src/alterator.rs src/spaces.rs src/metrics/
```

Put a kind list a role owns on its row; a list several languages share
is a `..name!()` fragment (`roles/c_family.rs`, `js_family.rs`,
`tcl_family.rs`). `make role-coverage` catches an id no row claims; it
does not catch an alias missing from the intended row, so this
inventory stays manual. With many aliases, prefer one `node.kind()`
string comparison over enumerating them.

After bumping a grammar pin, bump the root `Cargo.toml` and
`enums/Cargo.toml` together and regenerate the enum before anything
else:

```bash
cargo run --manifest-path ./enums/Cargo.toml -- -lrust -o ./big-code-analysis-ast/src/languages
```

Stable named-node ids are no evidence the anonymous ids held; spot-check
a known anonymous token with `bca dump` against the enum.

## 2. Skip hidden-rule variants, but mark them

A rule whose name begins with `_` is hidden: the variant exists in the
enum and the parser never emits it. Check the `Lang::Variant => "name"`
arm in `language_<lang>.rs` before listing a look-alike. Keep the
defensive arm and pin its hidden status with a
`!ast_has_kind_id(&parser, Lang::HiddenVariant as u16)` assertion, so
a grammar that later promotes the rule changes behaviour visibly.

## 3. Address children by role, never by index

Use `child_by_field_name("condition")` over `node.child(1)` for any
slot the grammar can precede with an optional preamble (an
init-statement, `constexpr`, a named-argument label, a BLANK ALIAS
body). When no field exists, iterate `named_children` and pick by role,
naming the variant set you verified in a comment. A new dispatcher arm
needs one fixture per optional preamble the grammar permits.

## 4. Re-derive a sibling's gate against the target grammar

The semantic role transfers across languages; the node kind does not. A
copied child-kind gate compiles and matches nothing. `is_else_if` is
the largest example: every language answers through its `ElseIf` row, and
the strategy is the row's gate. Derive the current table rather than
trusting a copy:

```bash
rg -n 'ElseIf \{' big-code-analysis-ast/src/roles/*.rs
rg -o 'else_if_(parent_clause|prev_sibling)_gate!\(\s*(\w+)' -r '$1 $2' big-code-analysis-ast/src/roles/ --no-filename | sort
rg -n 'ElseIf \{ absent' big-code-analysis-ast/src/roles/*.rs
```

| Strategy | Emitter | Languages (re-derive; this moves) |
| --- | --- | --- |
| owner's parent is the else-clause | `else_if_parent_clause_gate!` (`roles/control_flow.rs`) | C, Cpp, Mozcpp, Objc, Rust, Javascript, Mozjs, Typescript, Tsx; PHP's `else if` by its own gate |
| owner's previous sibling is the `else` keyword | `else_if_prev_sibling_gate!` | Csharp, Groovy, Java, Kotlin |
| owner's parent is the outer `if` (no clause node) | hand-written gate on the row | Go |
| the clause node *is* the else-if (`pays_itself`) | none needed | Bash, Irules, Lua, Perl, Ruby, Tcl; PHP's `elseif` |
| owner is the sole statement of the `else:` block | hand-written gate; Checker is `Branch ∧ ElseIf` | Python |
| `absent` row, trait default `false` | — | Elixir |

The jump label is the same lesson in miniature: `StatementIdentifier`
in the JS family, `Identifier` in Java and Groovy, `LabelName` in Go,
`Label` in Perl.

A gate that is only the owner's parent kind is spelled on the row as
`K(under [..])` / `K(not_under [..])`, not as a `fn`; the `ElseIf`
emitters stay because they read more than the parent. A consumer's
projection of an owner's child is a named `fn` beside the table with a
row test, not a row. Dump the construct in the target grammar before
reusing a sibling's predicate, and add a fixture that exercises the
gated branch.

## 5. One kind per operator; parent-guard compound leaves

A wrapper node containing its keyword token (`await a()`) must be
classified once, or Halstead counts every occurrence twice. Verify the
stream with `bca ops`. The inverse: a compound operator (`not in`)
wraps leaves that are valid operators elsewhere; classify the compound
and return `Unknown` for the inner leaf only when its parent is the
compound. Any predicate listing both a container and a kind it can
contain double-counts (Ruby `Lambda | Block | DoBlock`). On a row,
`owns` and `pays` are disjoint by construction; a consumer that wants
both reads `pays_at_wrapper` and `pays_at_leaf` explicitly.

## 6. Narrow a dispatch set with a gate, never by deletion

Deleting a kind from the `FuncOpener` row to stop an over-count
regresses its childless variant to zero. Gate the owner on child
presence, which keeps `is_func_with_code`, `is_func_space_with_code`
and `get_space_kind_with_code` on the same predicate. When a
wrapper/leaf pair double-counts, the keeper is the node present for
every spelling, verified with `bca dump` on valid and
on recovery input: `node-types.json` describes the well-formed grammar
only, and an `ERROR` root routinely holds the leaf with no wrapper.
Where neither choice strands a node, keep the node whose span is the
operand. Skip invented zero-width tokens by `start_byte() == end_byte()`,
not `is_missing()`, with a malformed fixture per grammar. Decide a
grammar-reachable but language-invalid shape on what it buys for valid
input; document the gap instead of pinning numbers for invalid input.
Before writing an arm, list every `(kind, parent)` pair the dump
produced for the construct and give each a gate alternative or a
reason; dump the construct alone, since a packed fixture recovers
differently.

## 7. Walk the sibling predicates for parity

`is_string` ↔ `get_op_type` (operands), `is_call` ↔ `get_op_type`
(operators), `is_func_space` ↔ each metric's body walker, and
`is_func_space` ↔ `get_space_kind` all classify the same nodes. The
last pair fails quietly: a space the walker opens but the getter leaves
`Unknown` is not a member scope, so it serialises no `npm` / `npa`
block and nothing diffs. Cross-walk the `_with_code` spellings, which
are what the walk calls and what every language must write: the
byte-less `is_func_space` and `get_space_kind` derive from them (#1863)
and agree everywhere except where a gate reads what they drop
(Elixir's keyword-text containers, Ruby's lambda-body blocks, C#'s
primary constructor — a `parameter_list` gated `under` a type
declaration (#1828) — and a Java or Groovy record's header, gated on a
record parent whose body writes no canonical constructor, told by the
constructors' names and types as bytes (#1842); the known-empty chain
sees neither gate), which is exactly where the answer matters. The
callable test has no byte-less spelling: ask `is_func_with_code`.
`tests/parity/ops_metrics_space_parity.rs` keeps `bca ops` and
`bca metrics` opening the same spaces.

## 8. Anchor a default/fallback exclusion to the construct

A `default` / `else` / `_` arm is +0 only when the construct's own node
already paid the increment; the same kind can be +0 under `case` and
+1 under `begin`/`rescue`. Never blanket-suppress a shared token. Where
ABC and cyclomatic must agree, reuse cyclomatic's gate and pin
`abc.conditions == cyclomatic() - 1` per space.

## 9. Watch for control flow with no dedicated kind

In command-dispatched grammars (Tcl, iRules, shells) control flow can
parse as a generic `command`. Recognise it by the leading word and
locate sub-parts by structural position, not a fixed index.

## 10. Read the bytes for identity questions

Shape says "attribute access"; only bytes say whose. Compare
`code.get(start..end)`, never `to_string_lossy()`. When a language
encodes semantics in identifier text, plan the `&[u8]` widening of
`compute` with the implementation.

## 11. Give one input to each independent path

When two independent paths sum into one field, a fixture either path
can classify hides a dead path. Write a fixture only the path under
test can classify (a bare `if (x)` for a walker arm, an empty container
for a descending arm) and perturb that arm alone.

## When you fix one language, sweep the rest

Every item above is a per-language failure that usually exists in
siblings. Build the audit table by dumping the construct in each
sibling grammar, not by searching for a kind name (a §9 construct has
no kind to find). Land the sibling fixes in one commit and anchor any
known-wrong case with `FIXME(#NNN)`. A `LANG` variant that owns no file
extension (`mozcpp`) gets no corpus coverage; pin it with a parity test
against its extension-owning sibling.
