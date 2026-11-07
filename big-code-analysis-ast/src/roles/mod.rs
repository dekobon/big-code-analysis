//! One declaration per language of every semantic fact the classifiers
//! and the metric dispatchers ask of a node (#1690).
//!
//! A *role* is a question a consumer puts to a node — is it a call, a
//! string literal, a comparison, something that opens a space. A *row*
//! is one language's answer: which kinds *own* the construct (the node a
//! slot predicate receives), which token kinds *pay* for it (the leaf a
//! counting arm fires on), and how to get from one to the other. The
//! [`role_table!`](crate::role_table) macro turns a table of rows into
//! the predicates the `Checker`, `Getter` and root-crate consumers read
//! instead of restating the kind lists themselves. This crate computes
//! no metric: a row says what a construct *is*; which occurrence a
//! metric counts is a policy decision in `big-code-analysis` (contract
//! 7 of #1690). A row may *gate* an owner or a payer on a `fn` that
//! reads the node, its source bytes and its ancestor *chain* (the
//! walk's stack of enclosing nodes, root first); the attributes that
//! say so are in "The row grammar" below.
//!
//! This doc is the contributor reference for the tables. Read it before
//! adding a language, a row, or a consumer; the rules under
//! `.claude/rules/` and the book's new-language chapter link here rather
//! than restating it.
//!
//! # The role set
//!
//! Fixed here because the census gate (`make role-census`, #1692,
//! below) iterates it, and a role added later must be declared or
//! marked `absent` in every table. A meaning names the existing `Checker` /
//! `Getter` contract where one exists, so a row restates that contract
//! rather than a reading of the word. Each row of a table names one of:
//!
//! | role | meaning |
//! |---|---|
//! | `Call` | a call expression, the node `find` / `count call` report |
//! | `StringLit` | a string literal, one operand unless it `wraps` interpolation |
//! | `Comparison` | a relational or equality test: `<`, `==`, `instanceof`, `in` |
//! | `ShortCircuit` | a lazy boolean operator: `&&`, `\|\|`, `and`, `or` |
//! | `EagerLogical` | a boolean operator evaluating both sides: Kotlin infix `and`, Perl `xor` |
//! | `Ternary` | a conditional expression: `c ? a : b`, `a if c else b` |
//! | `NullSafe` | optional navigation or null coalescing: `?.`, `??`, `?:` |
//! | `CaseArm` | one alternative of a switch-like construct |
//! | `Guard` | the guard clause itself on a `CaseArm` or function head; the arm is the `CaseArm` |
//! | `Branch` | an `if`-shaped construct, statement or expression |
//! | `Switch` | the switch-like construct holding `CaseArm`s |
//! | `Loop` | an iteration construct, statement or expression |
//! | `Catch` | an exception handler clause |
//! | `ElseIf` | the chained `else if` form, as distinct from a nested `if` |
//! | `Exit` | a point where control leaves the body explicitly (see below) |
//! | `Modifier` | an access or storage modifier keyword: `public`, `static` |
//! | `DeclKeyword` | a declaration keyword: `class`, `interface`, `def` |
//! | `Primitive` | a built-in type name: `int`, `boolean`, `void` |
//! | `FuncOpener` | a node that opens a `Function` space |
//! | `Closure` | an anonymous function, whether or not it opens a space |
//! | `Container` | a node that opens a class-like, unit or namespace space |
//! | `Member` | a field, property or attribute of a `Container` (see below) |
//! | `Comment` | a comment kind that is not documentation by kind (see below) |
//! | `DocComment` | a comment a documentation tool reads, by kind or by its marker |
//! | `Docstring` | a string literal that serves as documentation, by position |
//! | `DocTarget` | a declaration a doc comment attaches to, as a candidate (see below) |
//! | `DocRunOpener` | a comment whose own content makes its run documentation (see below) |
//! | `DocRunDirective` | a member of a doc run that is *not* documentation (see below) |
//! | `DocBodyResidual` | a kind beyond `FuncOpener` and `Closure` whose body holds a doc comment (see below) |
//!
//! Some of those need more than a line, because a short reading of the
//! word files different kinds from the contract it names:
//!
//! - `Exit` is what the exit count reads today: `return` and `throw`,
//!   a generator's `yield` (control leaves the body, resumably) and a
//!   `switch` expression's `yield` (control leaves the arm). An implicit
//!   tail expression is not one.
//! - `Member` is what the attribute count walks: the declaration node
//!   of a field, property or attribute. In most grammars that is a
//!   child of the container's body (`field_declaration`); in Python it
//!   is a `self.x` / `cls.x` binding inside a method, which is the
//!   attribute's only declaration. The row recognises the node; which
//!   body it sits in, its visibility, and how many names one node binds
//!   are the consumer's reading.
//! - `Comment` and `DocComment` partition the comment kinds: a kind that
//!   is documentation *by kind* — Groovy's `groovydoc_comment`, Perl's
//!   `pod_statement` — sits on `DocComment` alone, under the gate that
//!   tells an empty `/**/` or a POD `=begin comment` region from
//!   documentation, and `Comment` holds the rest. "Any comment" is the
//!   two rows' kind tables as one, which is what `Checker::is_comment`
//!   reads (it has no bytes and no chain, so it reads `lists_kind`, not
//!   `owns`), and what makes `is_doc_comment ⇒ is_comment` structural.
//!   `Docstring` reaches neither predicate: a docstring is no comment.
//! - `DocTarget` is the *candidate* under the gates that read the node
//!   itself — a declarator binds a name, a specifier has a body, a Go
//!   declaration sits outside a function body. The context a row cannot
//!   see belongs to the `Checker`, which reads a named `fn` beside the
//!   row: tsc's rule that a declaration directly under `export`
//!   documents nothing on its own, Ruby's `then` wrapper and YARD
//!   scope, Rust's any-kind tuple-struct slot, Doxygen's `ERROR`
//!   holding an entry. Where that
//!   makes the row over-approximate the Checker, `doc_tests/mod.rs` pins
//!   it. Lua's row is read by the opener's content decision alone; its
//!   `Checker::is_doc_target` stays the trait default, since LDoc
//!   attaches nothing to a declaration a plain `--` run precedes.
//! - `DocRunOpener` is `Checker::is_doc_run_opener`: a run that is
//!   documentation with no declaration under it — an LDoc `---` block,
//!   RDoc `##`, a YARD `@!` directive — not merely the first comment of
//!   a run.
//! - `DocRunDirective` is `Checker::is_doc_run_directive`: a member of a
//!   doc run that is not documentation text and is not billed as such,
//!   such as `//go:generate` or a Ruby shebang or magic comment. It is
//!   not a markup directive like `@param`.
//! - `DocBodyResidual` is the *residual* of
//!   `Checker::holds_doc_comment_in_body`: the kinds beyond the
//!   `FuncOpener` and `Closure` rows in whose body the language's
//!   documentation tool reads a doc comment — Java's `static { … }`,
//!   bare `block` and `switch_block`, Kotlin's accessors and `init`,
//!   C#'s property and indexer in every form. The row is deliberately
//!   not the full set: the adapter composes `func_opener::is_callable ∪
//!   closure::owns ∪ doc_body_residual::owns`, then the language's
//!   body-child test (`checker/<lang>.rs`), which no kind spells. So a
//!   kind that moves between `FuncOpener` or `Closure` and this row
//!   changes nothing the Checker answers; only a kind on none of the
//!   three does (#1797 named the residual; `holds_doc_comment_in_body
//!   ⇒ is_func ∨ is_closure ∨ row` in `derived_parity_tests.rs` pins the
//!   composition). A class body is never one.
//!
//! The seven documentation roles are what a language's doc-comment
//! adapter (`checker/<lang>.rs`, the per-language half of the
//! doc-comment work in #1654 and #1655) uses to *recognise* candidates:
//! the rows, composed with the named context fns listed above. What is
//! not a row: the `DocBinding` (`lang_helpers/doc_comment.rs`: its `kept`
//! policy and `skips`, `forward`, `gathers` callbacks), which decides
//! *which* doc comments of a stretch attach; candidate selection between
//! the source and legacy views; owner normalisation; and text decoding.
//! Those stay the epic's mechanisms, reading the rows. The reusable
//! helpers beside the rows: the marker tests in
//! `lang_helpers/doc_text.rs`, the Doxygen entry projection and
//! conditional memo in `lang_helpers/doxygen.rs`, YARD's scope fold and
//! call forms in `lang_helpers/ruby.rs`, and Python's run and assignment
//! helpers in `lang_helpers/python.rs`.
//!
//! A row gate may be generic over its own language's `Checker` tag —
//! Lua's opener reads `documents_where_it_sits::<LuaCode>`, the C-family
//! conditional gate `conditional_declares::<CCode>` — which is a
//! type-level cycle (`roles` → `checker` → `roles`) that is harmless
//! exactly while the `Checker` methods it reaches read *other* rows, or
//! read this one only through a projection that cannot re-enter the
//! gate (`doxygen_entry` and the walk's `children_declare` each test
//! `preproc_conditional` first; `doxygen_entry_never_reenters_the_conditional_walk`
//! pins both).
//!
//! # The row grammar
//!
//! An illustrative table, not Java's: it spells every row shape the
//! macro accepts, including the two (`pays never`, `pending`) no table
//! uses today.
//!
//! ```text
//! role_table! {
//!     lang: Example;
//!     Exit       { owns [ReturnStatement, ThrowStatement] }
//!     Comparison { owns [BinaryExpression(holds_payer),
//!                        InstanceofExpression]
//!                  pays [EQEQ, LT under BinaryExpression, Instanceof]
//!                  project "operator"
//!                  excludes [QMARK "a wildcard bound's `?` is type syntax"] }
//!     Container  { owns [ClassDeclaration(opens Class),
//!                        ObjectCreationExpression(opens Class gated_by has_class_body)] }
//!     Modifier   { owns [Static(not_under [ImportDeclaration, RequiresModifier]),
//!                        Final] }
//!     DocTarget  { owns [PropertyIdentifier(under [EnumBody])] }
//!     ElseIf     { owns [IfStatement(gated_by follows_else)] pays [] }
//!     Guard      { owns [Guard] pays never }
//!     NullSafe   { absent "no `?.` or `??`" }
//!     Comment    { pending R99 }
//! }
//! ```
//!
//! The doctest on [`role_table!`](crate::role_table) compiles these
//! shapes. The block here is not compiled; when the grammar changes,
//! change both. Every table in the tree is fully declared since #1700:
//! no row is `pending`, and the census baseline that ratchets the
//! pending set is empty.
//!
//! ## Owner attributes
//!
//! An owner's attributes go in parentheses after its kind, in any
//! order:
//!
//! - `gated_by f` makes the kind own the construct only when
//!   `f(node, code, ancestors)` says so; `text_gated_by f` is the same
//!   with a name that says the gate reads source bytes. Both emit only
//!   the with-code spelling (see "Reading a row"). Either may be
//!   followed by `no_chain` — `gated_by has_body no_chain`,
//!   `text_gated_by opens_with_javadoc_marker no_chain` — which
//!   declares that `f` takes no chain: its signature is then
//!   `fn(&Node, &[u8]) -> bool` and the row calls it with two
//!   arguments, so a marked gate cannot read the chain by
//!   construction (#1793). `no_chain` proves "no chain parameter",
//!   not "subtree-only": a two-argument gate can still call
//!   `Node::parent`, which the derived suite's backstop
//!   (`no_chain_rows_answer_alike_under_the_empty_chain`) is for.
//!   The two axes are independent — Java's doc-marker gates read
//!   bytes and no chain, Groovy's `spells_var` both — so
//!   `TEXT_GATED` comes from the keyword and `CHAIN_GATED` from the
//!   absence of the modifier.
//! - `under [P, Q]` makes the kind own the construct only where its
//!   parent is one of the listed kinds, and `not_under [P, Q]` only
//!   where it is none of them, a parentless node included (#1794):
//!   Python's `if_clause` is a `case` guard under a `case_clause` and a
//!   comprehension filter anywhere else; Java's `class` under a
//!   `class_literal` ends `C.class`. The list is bracketed even for one
//!   kind, and not empty. It is the owner's gate — exclusive with
//!   `gated_by`, `text_gated_by` and `holds_payer` — and reads the
//!   chain (`Ancestors::parent`), so it sets `CHAIN_GATED`. The parent
//!   kinds are not claimed: they reach no kind list, and the coverage
//!   ratchet does not count them. A test of anything beyond the
//!   parent's kind stays a `fn`.
//! - `pays_itself` marks a kind the grammar gives no token for, so the
//!   owner is also the payer; `pays_itself_when f` one the grammar gives
//!   a token in some spellings and none in others, so the owner pays
//!   only where `f` says it holds none (Groovy's `a !in b` beside
//!   `a in b`).
//! - `holds_payer` gates the owner on its `project` field (the row
//!   attribute below) holding one of the row's payers, so a
//!   `binary_expression` owns a comparison only when its `operator`
//!   child is a comparison token. It is the one gate a kind shared by
//!   several constructs needs.
//! - `opens K` records the `SpaceKind` the owner opens, and `opens_by f`
//!   the `SpaceKind` a named `fn(&Node, &[u8], Ancestors) -> SpaceKind`
//!   answers for a kind that spells two containers at once (Kotlin's
//!   `class_declaration` is a class or an interface by its keyword
//!   token) — `Unknown` meaning it opens none. `opens_by f no_chain`
//!   takes the modifier as a gate does, with the two-argument
//!   signature. Only a `FuncOpener` or `Container` row may carry
//!   `opens` / `opens_by`: the `opens_space` fold scans every row, and
//!   the byte-less space forwards assert over exactly those two rows'
//!   flags, so an opener on a third role would bypass them — the macro
//!   rejects it (#1793).
//! - `callable` marks a `FuncOpener` owner that is a *named function* —
//!   what `Checker::is_func` answers — beside openers that are not
//!   (Java's `static { … }` opens a `Function` space and is nobody's
//!   callee; Elixir's `fn … end` opens one and is a `Closure`), and
//!   `callable_when f` one that is a named function only where `f` says
//!   so, under a gate of its own beside the owner's — a JS
//!   `function_expression` opens a space wherever it sits and is a
//!   function only when bound to a name, a closure otherwise.
//!
//! An owner takes at most one gate (a parent list is one), one
//! `pays_itself*` and one `opens*`; a second of any is a compile error.
//! An owner that needs two conditions names one `fn` that tests both.
//!
//! ## Payer attributes
//!
//! A payer takes `gated_by f` or `text_gated_by f` bare, after its kind,
//! or `under P` / `under [P, Q]`, which gates it on its parent's kind —
//! the one gate a token the grammar spells in several productions reads
//! (`?` under the ternary and not a wildcard), the same test as an
//! owner's `under`.
//!
//! ## Row attributes
//!
//! Written in this order — `pays`, `project`, `wraps`, `excludes`:
//!
//! - `pays [..]` splits the paying occurrences from the owners (see the
//!   projections below). Three modes, which `canonical()` emits as
//!   `mode=owners|split|never` so `utils/role_tables.py` is held to
//!   them (#1745): no `pays` list, and the owners are the paying nodes;
//!   `pays [..]`, and the listed tokens pay plus every `pays_itself`
//!   owner; `pays []` — the split row with *no* payers — says the owners
//!   are recognised but pay nothing at the leaf unless marked
//!   `pays_itself`, the `ElseIf` shape, where the chained `if` is
//!   `Branch`'s to pay and a dedicated `elif_clause` pays for itself
//!   (#1697). `pays never` says the construct has owners and no paying
//!   node at all — a shape the macro keeps for a construct every
//!   consumer recognises and none counts, which no row declares today
//!   (Java's `Guard` was its proof row until #1695 found cyclomatic pays
//!   the node). Neither `pays []` nor `pays never` means the owners are
//!   the paying nodes.
//! - `project "field"` names the child field holding an owner's paying
//!   token.
//! - `wraps [..]` lists the interpolation kinds a `StringLit` can hold.
//! - `excludes [K "why"]` records a kind the row's author considered
//!   and rejected, for the coverage ratchet: the kind leaves the
//!   coverage residue with the reason beside it, and emits nothing
//!   else. Groovy's `Switch` row excludes its `switch_block` this way —
//!   the owner's child, not a construct of its own (#1701).
//!
//! `absent "why"` is a language with no such construct, and
//! `pending Rnn` a row a later package fills in; the census treats both
//! as declared. An `absent` row answers every membership question
//! `false`, so a consumer shared by several languages reads it as it
//! reads a declared row. A `pending` row emits no predicate: a consumer
//! that read one would silently miss what the row will declare, and
//! the compile error says so.
//!
//! An `owns` or `pays` list may splice a shared kind fragment with
//! `..name!()` (`roles/c_family.rs`, `roles/js_family.rs`,
//! `roles/tcl_family.rs` — each a kind list and the gates that read
//! it); the fragment note on the macro below has the shape.
//!
//! # What a table emits
//!
//! For each row with owners, in a module named after the role in
//! `snake_case` (`comparison`, `func_opener`):
//!
//! - `OWNERS`, `PAYERS`, `EXCLUDED`, `WRAPS`: the kinds the row names,
//!   verbatim, for the coverage ratchet. `OWNERS` and `PAYERS` are the
//!   *declared* lists and are disjoint by construction (see below).
//! - `owns(node, code, ancestors)`: the gated owner test. A
//!   `holds_payer` owner's gate is `project(node)` landing on a payer.
//! - `pays_at_leaf(node, code, ancestors)`: with a `pays` list, the gated
//!   payer test plus every `pays_itself` owner — where a token-counting
//!   arm fires; without one, the owner test; under `pays never`, false.
//! - `owns_kind` / `pays_at_leaf_kind` / `is_callable_kind`: the same
//!   with the node's kind id passed in, for a consumer that asks several
//!   rows about one node — `kind_id()` is an FFI call per read, and a
//!   classifier on the metric walk's hot path (`spaces::compute`)
//!   reading three rows paid six of them (#1693).
//! - `lists_kind(kind)`: whether the row names `kind` among its owners,
//!   gates aside — the byte-less `Checker` spellings (`is_call`,
//!   `is_string`), which read no bytes and no chain and so answer the
//!   ungated kind set, on which a gated owner over-approximates (#1698).
//!   A chain the
//!   caller does not have is not the same as an empty one: the Tcl
//!   family's braced word reads "no parent" as a script body.
//! - `pays_at_token_kind`: `pays_at_leaf_kind` restricted to the
//!   declared payers — a `pays_itself` owner pays at the leaf but has no
//!   token, so a token-billing consumer (Halstead) reads this one.
//! - `pays_at_wrapper(node, code, ancestors)`: the owner test, for a
//!   consumer that pays at the owning node; false under `pays never`.
//!   A row with no `pays` list answers both projections identically, so
//!   the split is visible only where a row declares it, and a consumer
//!   counting both an owner and its leaf is a visible choice at the
//!   consumer, never an accident here.
//! - `project(owner) -> Option<Node>`: the owner's paying child — the
//!   declared `project` field where the row names one and the owner has
//!   it, else the first child whose kind is a declared payer, for an
//!   owner whose grammar names no field (Java's `instanceof_expression`,
//!   or a `case_statement` whose `case` keyword is a bare child); and
//!   beside it `owns_via(payer, node, …)`, the owner test narrowed to
//!   one payer, for a consumer that scores some of a row's payers and
//!   not others.
//! - `is_callable(node, code, ancestors)`: the `callable` owners under
//!   their gates, and the `callable_when f` owners under their gates
//!   *and* `f` — a function someone names at a call site, as opposed
//!   to an opener that merely holds a body. Only a `FuncOpener` row is
//!   expected to mark any; on every other row the predicate is false.
//!   `CALLABLE` is the declared list of those kinds, gates aside — the
//!   kind set a consumer that asks "is any ancestor a named function"
//!   matches ancestors against. Gates aside is the caveat: a kind
//!   callable only under a gate (C#'s property, callable without
//!   accessors) is in the list for its gated-away spellings too, so a
//!   consumer whose ancestors can carry one keeps a list of its own.
//! - `TEXT_GATED`: whether any *owner* of the row carries a
//!   `text_gated_by` annotation, so a forward that hands the row no
//!   bytes can assert at build time that no declared byte gate is
//!   dropped. That is the whole of what it sees: a payer's
//!   `text_gated_by` is carried as `gated_by`, and a gate that reads
//!   bytes under a plain `gated_by`, `callable_when`, `pays_itself_when`
//!   or `opens_by` is not flagged — the keyword is the declaration, and
//!   a gate named under the wrong keyword is the author's error.
//! - `CHAIN_GATED`: whether any owner carries a `gated_by` /
//!   `text_gated_by` gate or an `opens_by` fn *without* `no_chain`
//!   (#1793), or an `under` / `not_under` list, so a forward that
//!   hands the row a known-empty chain asserts at build time that no
//!   gate it reaches can take the chain.
//!   It covers the `owns` / `pays_at_wrapper` projections and the
//!   `opens_space` fold — the ones a chain-less forward reads — and
//!   not `callable_when`, `pays_itself_when` or a payer's gate, which
//!   only `is_callable` and `pays_at_leaf` consult and no chain-less
//!   forward reads; those three reject the modifier, so `false` here
//!   never means "nothing on this row reads ancestry". Unlike
//!   `TEXT_GATED` it is not an author's word: a marked gate has no
//!   chain parameter, so the compiler holds the flag to the fn.
//!   Both flags are in `canonical()`, so the Python reader is held to
//!   both.
//!
//! And once per table: `opens_space(node, code, ancestors) -> SpaceKind`,
//! folded over every `opens` entry on every row, so the kinds a
//! `FuncOpener` or `Container` row names are not restated. The fold
//! answers with the first `opens` clause whose gate admits the node, in
//! row order; a kind two rows both `opens` must have gates that cannot
//! both admit one node, or the earlier row decides. Under `cfg(test)` or
//! the `test-support` feature, a crate-private `rows()` enumerates every
//! row of the table as a `RowView` in declaration order (#1700), reached
//! from outside the crate through the public `roles::tables()`: the
//! derived parity suite takes its languages and roles from that, so a
//! row it does not list is a row no derived assertion asks.
//!
//! Every production predicate and constant a table emits is `pub`
//! (the test-support enumeration, `canonical()` and the per-row
//! `MODE` / `HOLDS_PAYER` are not): the root crate's metric
//! dispatchers are consumers too (`abc`, `cyclomatic`, `cognitive` read
//! the `Comparison` and `ShortCircuit` rows since #1694), and they reach
//! a row as `big_code_analysis_ast::roles::<lang>::<role>`. The AST
//! crate is outside `STABILITY.md`, so this widens no published
//! contract.
//!
//! # Reading a row from a consumer
//!
//! Every emitted predicate takes `(node, code, ancestors)`; a text-gated
//! row cannot be answered without the bytes (contract 4 of #1690), and a
//! chain-gated one not without the chain. So:
//!
//! - **Reach for the `_with_code` spelling** — that is what the walk
//!   calls (`promotes_to_func_space_with_code`,
//!   `get_space_kind_with_code`). The legacy `Checker` spellings come
//!   in three shapes, and only one of them is the ungated kind set.
//!   `is_call`, `is_string` and `is_comment` take neither bytes nor a
//!   chain and read `lists_kind`. `is_func_space` and `get_space_kind`
//!   take neither either, and forward to the `_with_code` form with
//!   `&[]` and a *known-empty* chain, `Ancestors::known(&[])` — never
//!   `Ancestors::unknown()`, which climbs through `Node::parent` and
//!   answers the gated result, so the spelling would silently stop
//!   being the kind set (lesson 114); a gate that reads only the node's
//!   own children (Java's anonymous-class body) still applies there.
//!   `is_func`, `is_closure` and `is_else_if` take the caller's chain
//!   and hand it through with `&[]` bytes, so a chain gate (Ruby's
//!   `block` under a stabby lambda) answers exactly and only a byte
//!   gate is dropped. The chain-less pair disagrees with the walk
//!   wherever a gate reads what it drops — Elixir's text-gated `call`
//!   openers, Ruby's `block` — and the derived parity suite pins each
//!   such disagreement by name (`KNOWN_DISAGREEMENTS`). Every other
//!   language's pair asserts `!TEXT_GATED && !CHAIN_GATED` on its
//!   `FuncOpener` and `Container` rows at build time (#1793).
//! - **Ask several rows of one node through the `_kind` projections**,
//!   reading `kind_id()` once.
//! - **A consumer's policy over a row is the consumer's.** Which
//!   projection it reads, which subset of a row's owners it acts on,
//!   where it opens a nesting level — those live in `big-code-analysis`
//!   and are not what a row decides. A structural projection of an
//!   owner's child that one consumer needs — the `switch_block` a
//!   `Switch` owner nests its arms in, where cognitive opens its level —
//!   lives as a named `fn` beside the table (`is_switch_body_kind` in
//!   `roles/java.rs` and `roles/groovy.rs`), with a row test pinning
//!   the parent/child relation, and the kind either claimed by whichever
//!   row holds it for its own reason (Java's `DocBodyResidual`) or
//!   recorded under `excludes` on the owner's row (Groovy). A DSL `body`
//!   attribute would be the declared form if a third grammar needs it
//!   (#1701). A grammar-fact subset of a row's owners takes the same
//!   form: `is_preproc_conditional_kind` (`roles/c_family.rs`) names the
//!   C-family `DocTarget` row's conditional owners, held to their names
//!   over every runtime id by `<dialect>_old_names_are_the_conditional_kinds`
//!   (`roles/doc_tests/doxygen.rs`, #1795).
//! - **A kind a row claims is not restated as classification inside the
//!   consumer boundary** — the per-language dispatch files, their
//!   dispatcher modules and macros (`utils/check-role-bypass.py`,
//!   `BOUNDARY`). A site that is not membership — a naming rule, a
//!   traversal step, a consumer-chosen *subset* of a row's owners the
//!   row cannot express — takes a `// role-bypass: allow(<Role>) —
//!   <reason>` on the line above, naming a role that claims the kind,
//!   with the reason beginning on that line. The exemption covers the
//!   whole statement or match arm after it, and passes any kind in that
//!   span that one of its named roles claims — a kind claimed only by
//!   some other role is still reported — so what it hides is a kind of
//!   the *same* roles later added inside the span; the Elixir byte-less
//!   space table is the one such pair of sites, and the derived
//!   forward-parity test is what sees a kind added there (#1701).
//!
//! A row's comment describes the grammar — what
//! the construct's nodes are and how they nest — and may cross-reference
//! the consumers' readings where that explains a declaration (why a
//! kind is excluded as "the consumers' own", which projection a gate
//! serves), since the reader deciding whether a row is right needs to
//! know who reads it; the decision itself stays at the consumer, and
//! nothing a row *emits* names a metric (#1690). The row-comment
//! placement rule is `.claude/rules/macro-comments.md`.
//!
//! # The gates and the derived suite
//!
//! Three gates in `make pre-commit` and `make ci` keep the tables
//! honest, each reading the declarations through `utils/role_tables.py`
//! — the one reader, held to the compiled constants by
//! `python_reader_agrees_with_the_compiled_tables` below. AGENTS.md
//! ("The role-table gates") carries the recipes; this is what each
//! decides.
//!
//! - **Census** (`make role-census`): every (language, role) cell is
//!   declared rows, `absent "why"` or `pending Rnn`; the pending set is a
//!   ratchet in `.role-census-baseline.txt`, empty since #1700.
//! - **Coverage** (`make role-coverage`): every runtime kind id of every
//!   generated enum is claimed by some row (`owns`, `pays` or `wraps`),
//!   excluded with a reason, or debt in `.role-coverage-baseline.txt`,
//!   keyed by `(language, id)` under the pinned grammar version. A moved
//!   pin fails until `--migrate-pin <lang>` confirms the baselined ids
//!   still spell the same kinds; `.role-coverage-observed.txt` records
//!   which kinds the integration corpus emits and must be refreshed
//!   (`make role-coverage-observe`) when a corpus submodule or a pin
//!   moves. `make role-coverage-claimed` lists every claimed kind with
//!   its corpus label — the "no occurrence" rows are what the derived
//!   suite cannot ask about.
//! - **Bypass** (`make role-bypass`): inside the consumer boundary, a
//!   kind a row claims is not restated as classification;
//!   `.role-bypass-baseline.txt` is empty since #1701, so a new finding
//!   fails outright, and a non-membership site takes the `allow`
//!   exemption above.
//! - **The derived parity suite** (`derived_parity_tests.rs` here;
//!   `src/metrics/derived_parity_tests.rs` in the root crate for the
//!   metric arms, #1700): for every declared row and every candidate
//!   node of a corpus, ask the row with the walk's real bytes and chain
//!   and compare against each `Checker` spelling that reads it and
//!   against the DSL's own invariants. `make role-parity-corpus` runs
//!   its forward comparison over the three repository corpora.
//!
//! All three baselines are exact record sets: `--update` only removes,
//! `--bootstrap` refuses a file that exists or is tracked at `HEAD`, so a
//! swap cannot hide behind an unchanged count.
//!
//! # What the mechanism cannot see
//!
//! The one list. The rules, the book and the lessons link here rather
//! than restating it, because a second copy is where an item goes
//! missing. Each gate prints its own part of this with every run; this
//! is the union, grouped by the kind of mistake, with the manual check
//! that covers each.
//!
//! **A wrong-role claim.** A kind claimed by the *wrong* role passes
//! every gate: the census counts the cell as declared, coverage counts
//! the id as claimed, and the derived suite takes its expected verdict
//! from the same declaration. So does an `absent` reason that is not
//! true of the language, and a kind the corpus never emits, which the
//! derived suite cannot ask about (`make role-coverage-claimed` names
//! those). The check is review of the row against the grammar — `bca
//! dump` on a one-line fixture per construct, every `(kind, parent)`
//! pair the dump shows given a row or a stated exclusion
//! (`.claude/rules/grammar-dispatch.md` §6) — and a per-language
//! spelling test for a kind no corpus file spells.
//!
//! **An incomplete entry.** A *missing cell* — a language with no row,
//! `absent` or `pending` for some role — is what the census catches. A
//! kind left off a declared row is not: the derived suite's inputs and
//! expected verdicts come from the same declaration, and coverage fails
//! only on an id that is claimed by *no* row, excluded by none and not
//! baselined — an alias omitted from its row is still debt in the
//! baseline, or claimed by some other role, and the gate passes either
//! way. Coverage is accounting, not a measured sweep (#1458). (The
//! census header's third item, a `pending` cell naming the wrong future
//! package, is retired with the pending set: no table declares one.)
//! The check
//! is the alias inventory of `grammar-dispatch.md` §1 — every
//! numeric-suffix variant listed or excluded with a comment — for row
//! declarations as for consumers, an `under` / `not_under` list
//! included, since no gate reads one, and the §7 cross-walk of the
//! sibling predicates.
//!
//! **A forward whose dropped context no gate reads yet.** A byte-less or
//! chain-less forward answers the same under any context until the day
//! a gate reads what it drops (lesson 103), and that day it answers
//! `false` silently. `TEXT_GATED` pins the declared owner byte gates at
//! build time and `CHAIN_GATED` the owner gates and `opens_by` fns
//! that take a chain (#1793) — each forward asserts exactly the flags
//! of what it drops, on the rows it reads. What neither sees: a
//! `no_chain` gate that climbs with `Node::parent` (the signature
//! proves no chain parameter, not no ancestry read — the derived
//! suite's `no_chain_rows_answer_alike_under_the_empty_chain` compares
//! every `!CHAIN_GATED` row under the real chain and the empty one and
//! counts the `Node::parent` climbs either side makes, so a climb is
//! reported whatever it answers — but only on the owner candidates the
//! corpus spells: a `no_chain` owner no fixture reaches is neither
//! compared nor counted, and `make role-coverage-claimed` lists the
//! claimed kinds with no corpus occurrence), a `callable_when` or payer
//! gate (no chain-less forward reads them), and a text gate under the
//! wrong keyword. So the derived suite still
//! compares every forward on every corpus candidate, and a known
//! disagreement is a named
//! `KNOWN_DISAGREEMENTS` entry that must still occur (a stale entry
//! fails too), never a tolerance. The check, when adding a chain- or
//! byte-reading gate to a row: list every forward of that row
//! (`rg 'Ancestors::known\(&\[\]\)'` and the `&[]` byte arguments),
//! decide for each whether the dropped input changes the answer, and
//! either route the caller to the `_with_code` spelling or add the
//! disagreement to `KNOWN_DISAGREEMENTS` with its reason. And know which
//! corpus asked: the `features` and `chain-audit` CI legs check out no
//! submodule and filter the `corpus_` tests, so on those legs the
//! forward comparison runs over the inline documentation fixtures
//! alone, and a disagreement only the control-flow corpus spells is
//! seen by the `test` and `coverage` legs.
//!
//! **A consumer reading the wrong projection, or nothing.** Which
//! projection a consumer reads — `owns` where it meant `pays_at_leaf`,
//! a kind table where the walk needed the gate — no gate judges. Nor is
//! a dead consumer path seen: reachable code no fixture exercises, or a
//! row no consumer asks at all. Text-keyed policy (Elixir's keyword
//! `call`s, Ruby's `private`) is the consumer's reading of the bytes and
//! is pinned only by the fixtures that spell it. The check is
//! `grammar-dispatch.md` §7 and §11: one input per independent path,
//! verified by revert.
//!
//! **What the bypass lint does not scan.** A restatement outside the
//! boundary, or in a `#[cfg(test)]` module; a kind reached through a
//! helper function in `lang_helpers` or any other call; a `kind_id()`
//! held in a local and compared later; a `kind()` compared to a variable
//! rather than a literal; and, inside an `allow` exemption's span, any
//! kind one of its named roles claims. A `macro_rules!` shape beyond the three it resolves is reported
//! as unsupported, never silently passed. The check, when a row's
//! membership moves: `rg` the kind's `Lang::Kind` and `"kind"`
//! spellings across `big-code-analysis-ast/src/` and `src/metrics/`,
//! and read every hit outside the boundary by hand.
//!
//! # Decision note
//!
//! The shapes #1691 asked to settle by compiling, and what settled them:
//!
//! - **A gated kind gets its own test, not a guard on a shared
//!   pattern.** Each owner expands to one `if node.kind_id() == K {
//!   return gate(..) }` clause rather than a `match` arm. The `match`
//!   form was tried first and rejected by measurement: rustfmt declines
//!   to format a `match` whose arms sit inside a `$( … )*` repetition
//!   (`check-rustfmt-bail.py` reported two stuck arms on the probe), and
//!   a `match` whose arms all return `true` trips
//!   `clippy::match_same_arms` unless they coalesce — which a gated arm
//!   cannot. The clause chain formats, has no arm for the gate to count,
//!   and still produces one branch per kind.
//! - **`opens_space` is emitted without repeating kinds.** Each `opens
//!   K` annotation is read in a second pass over the same rows, so the
//!   kind is written once and reaches both `OWNERS` and the
//!   `opens_space` chain. Equal `SpaceKind` mappings need no coalescing
//!   because there is no `match` to coalesce.
//! - **`text_gated_by` emits only the with-code spelling.** It is the
//!   same expansion as `gated_by`; the distinct keyword is documentation
//!   at the row, and the one shared signature is what makes a byte-less
//!   spelling impossible to emit by accident.
//! - **Row syntax uses `Role { … }` blocks, not `Role => …`.** The gate
//!   that counts formatting bails reads an indented `… =>` line as a
//!   match arm, so an `=>` row at call-site indentation reads as a
//!   permanent bail (two reported on the probe). Likewise `opens Class`
//!   rather than `=> Class`.
//! - **`Excluded` is a row attribute, not a standalone entry.** A
//!   standalone `Excluded(Comparison, QMARK, "…")` needs the role token
//!   mapped to a generated name from a second position, which
//!   `macro_rules!` can only do with a second per-role dispatch table.
//!   The row-level `excludes [..]` sits beside the row whose author
//!   rejected the kind, which is what the attribute is for, and costs no
//!   machinery.
//! - **Rejections are compile errors, not runtime checks.** A role both
//!   declared and `absent` / `pending`, or declared twice, collides on
//!   the role's module name. A kind in both `owns` and `pays` of one
//!   row, a kind twice in `owns` or in `pays`, or an excluded kind that
//!   the row also owns or pays for, each fail a `const` assertion over
//!   the slices (`wraps` and `excludes` are not checked for repeats).
//!   A `pays_itself` owner on a `pays never` row, or on a row with no
//!   `pays` list, hits a `compile_error!` naming the contradiction. A
//!   second gate, `pays_itself` or `opens` on one owner matches no
//!   expansion rule. All are pinned by `compile_fail` doctests on the
//!   macro.
//! - **Every declared row emits the whole predicate family.** A
//!   per-role emission profile — a recognition row with no `pays_*`
//!   family — was proposed and declined (#1792). Every production
//!   predicate and constant a table emits is `pub` in a `pub mod` of a
//!   library, and the private ones (`OWNERS_PAY`) are read by a `pub`
//!   predicate, so rustc reads none of them as dead whatever a row
//!   omits; the count that motivated the profile came
//!   from the `--tests` target, where the proof tables under `mod
//!   tests` expand the macro into private modules a test reads only
//!   part of. Which projection a consumer reads stays the manual check
//!   named above. An unread predicate costs its expansion and nothing
//!   else.
//! - **Not expressible, by design:** a kind claimed by the *wrong* role,
//!   an incomplete entry, and whether a consumer reads the right
//!   projection. Those are the list above.

/// Emits a `text_gated_by … no_chain` gate that reads the node's bytes
/// through one of `lang_helpers::doc_comment`'s marker tests (#1699):
/// `$name` is the gate the row names, `$test` the byte predicate. The
/// two-argument signature is the `no_chain` shape (#1793). Per language
/// because the gate fn resolves at the row; what each marker means to
/// its tool is the `//` comment above the invocation (a `///` on a
/// macro call is an unused doc comment).
macro_rules! byte_gate {
    ($name:ident, $test:path) => {
        fn $name(node: &$crate::Node<'_>, code: &[u8]) -> bool {
            $test($crate::lang_helpers::doc_comment::node_bytes(node, code))
        }
    };
}
pub(crate) use byte_gate;

#[cfg(any(test, feature = "test-support"))]
use crate::{Ancestors, LANG, Node, SpaceKind};

pub mod bash;
pub mod c;
pub mod c_family;
pub mod ccomment;
pub(crate) mod control_flow;
pub mod cpp;
pub mod csharp;
pub mod elixir;
pub mod go;
pub mod groovy;
pub mod irules;
pub mod java;
pub mod javascript;
pub mod js_family;
pub mod kotlin;
pub mod lua;
pub mod mozcpp;
pub mod mozjs;
pub mod objc;
pub mod perl;
pub mod php;
pub mod preproc;
pub mod python;
pub mod ruby;
pub mod rust;
pub mod tcl;
pub mod tcl_family;
pub mod tsx;
pub mod typescript;

// The space-tree rows of the kind-only languages, pinned independently
// of the classifiers that read them (#1696).
#[cfg(test)]
mod space_tree_tests;

// The `Call` and `StringLit` rows, pinned independently of the
// classifiers that read them (#1698).
#[cfg(test)]
mod call_string_tests;

// The six control-flow rows, pinned independently of the metric
// dispatchers that read them (#1697).
#[cfg(test)]
mod control_flow_tests;

// The seven documentation rows, pinned independently of the `Checker`
// predicates that read them (#1699).
#[cfg(test)]
mod doc_tests;

// The owner parent-list rejections as `compile_fail` doctests (#1794).
#[cfg(doctest)]
mod parent_list_doctests;

// Cross-consumer parity derived from the rows themselves, on every
// candidate node of the corpus (#1700). A full-enum sweep with an
// `is_enabled()` filter and a non-vacuity assertion, so it is absent
// rather than failing when no language is enabled; every item in it
// inherits this gate.
// test-lang-gates: hand-written(bash,c,c-family-helpers,cpp,csharp,elixir,go,groovy,irules,java,javascript,kotlin,lua,mozcpp,mozjs,objc,perl,php,python,ruby,rust,tcl,typescript)
//     — the sweep dispatches on `LANG` through a macro naming every
//     `Code` tag and filters at run time, so it needs the union, not
//     what its body names
#[cfg(all(
    test,
    any(
        feature = "bash",
        feature = "c",
        feature = "c-family-helpers",
        feature = "cpp",
        feature = "csharp",
        feature = "elixir",
        feature = "go",
        feature = "groovy",
        feature = "irules",
        feature = "java",
        feature = "javascript",
        feature = "kotlin",
        feature = "lua",
        feature = "mozcpp",
        feature = "mozjs",
        feature = "objc",
        feature = "perl",
        feature = "php",
        feature = "python",
        feature = "ruby",
        feature = "rust",
        feature = "tcl",
        feature = "typescript",
    )
))]
mod derived_parity_tests;

/// Whether two declared kind lists share an entry. `const` so the macro
/// can assert disjointness at expansion time rather than in a test;
/// `pub` only because a `#[macro_export]` expansion in another crate
/// reaches it through `$crate::`, and hidden for the same reason.
#[doc(hidden)]
#[must_use]
pub const fn kinds_overlap(a: &[u16], b: &[u16]) -> bool {
    let mut i = 0;
    while i < a.len() {
        let mut j = 0;
        while j < b.len() {
            if a[i] == b[j] {
                return true;
            }
            j += 1;
        }
        i += 1;
    }
    false
}

/// Whether a declared kind list names one kind twice. A second entry
/// for a kind is a dead clause — the first `if kind == K` returns — and
/// a double count for the coverage ratchet, so the macro rejects it.
#[doc(hidden)]
#[must_use]
pub const fn kinds_repeat(a: &[u16]) -> bool {
    let mut i = 0;
    while i < a.len() {
        let mut j = i + 1;
        while j < a.len() {
            if a[i] == a[j] {
                return true;
            }
            j += 1;
        }
        i += 1;
    }
    false
}

/// How many of `marked` are `Some`, for sizing `some_kinds`.
#[doc(hidden)]
#[must_use]
pub const fn count_some(marked: &[Option<u16>]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < marked.len() {
        if marked[i].is_some() {
            n += 1;
        }
        i += 1;
    }
    n
}

/// The `Some` kinds of `marked`, in order: the `CALLABLE` slice of a
/// row, built at compile time from one `Option` per owner.
#[doc(hidden)]
#[must_use]
pub const fn some_kinds<const N: usize>(marked: &[Option<u16>]) -> [u16; N] {
    let mut kinds = [0; N];
    let (mut i, mut j) = (0, 0);
    while i < marked.len() {
        if let Some(kind) = marked[i] {
            kinds[j] = kind;
            j += 1;
        }
        i += 1;
    }
    kinds
}

/// The kinds of an `EXCLUDED` list, for the same checks.
#[doc(hidden)]
#[must_use]
pub const fn excluded_kinds<const N: usize>(excluded: &[(u16, &str)]) -> [u16; N] {
    let mut kinds = [0; N];
    let mut i = 0;
    while i < N {
        kinds[i] = excluded[i].0;
        i += 1;
    }
    kinds
}

/// A gated row predicate as a value: `owns`, `pays_at_leaf`,
/// `pays_at_wrapper` or `is_callable` of one row.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub type RowPred = for<'a> fn(&Node<'a>, &[u8], Ancestors<'a, '_>) -> bool;

/// A kind-first row predicate as a value: the `*_kind` spellings.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub type RowKindPred = for<'a> fn(u16, &Node<'a>, &[u8], Ancestors<'a, '_>) -> bool;

/// One declared row of a table, the emitted items a derived test
/// reads reachable as values, so it can ask every row of every
/// language the same questions without naming a module (#1700). The
/// `*_kind` spellings and not the `node`-first ones, `pays_at_wrapper`
/// and `project` — what the two derived modules read; an item is added
/// here when a test reads it, not for uniformity. `role` is the row's
/// role token as written, which is the one spelling the `@named` table
/// and `utils/role_tables.py` share.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct DeclaredRow {
    /// The role token as written on the row.
    pub role: &'static str,
    /// `OWNERS`.
    pub owners: &'static [u16],
    /// `PAYERS`.
    pub payers: &'static [u16],
    /// `LEAF_KINDS`.
    pub leaf_kinds: &'static [u16],
    /// `CALLABLE`.
    pub callable: &'static [u16],
    /// `HOLDS_PAYER`.
    pub holds_payer: &'static [u16],
    /// `WRAPS`.
    pub wraps: &'static [u16],
    /// `EXCLUDED`.
    pub excluded: &'static [(u16, &'static str)],
    /// `TEXT_GATED`.
    pub text_gated: bool,
    /// `CHAIN_GATED`.
    pub chain_gated: bool,
    /// `PAYS_NEVER`.
    pub pays_never: bool,
    /// `MODE`.
    pub mode: &'static str,
    /// `pays_at_wrapper`.
    pub pays_at_wrapper: RowPred,
    /// `owns_kind`.
    pub owns_kind: RowKindPred,
    /// `pays_at_leaf_kind`.
    pub pays_at_leaf_kind: RowKindPred,
    /// `pays_at_token_kind`.
    pub pays_at_token_kind: RowKindPred,
    /// `is_callable_kind`.
    pub is_callable_kind: RowKindPred,
    /// `lists_kind`.
    pub lists_kind: fn(u16) -> bool,
    /// `project`.
    pub project: for<'a> fn(&Node<'a>) -> Option<Node<'a>>,
}

// `cfg(test)`, not `test-support`: nothing the feature exports reaches
// it — `pub(crate)`, and no `pub` item calls it — so under the feature
// alone it is dead. `rows()`, `view()`, `MODE` and `HOLDS_PAYER` are
// `pub(crate)` under the feature too, and live, because `tables()`
// reaches them; that reachability is the test, not the visibility.
#[cfg(test)]
impl DeclaredRow {
    /// Every kind the row names as owning, paying or paying at the
    /// leaf — the candidate set a derived census selects occurrences
    /// from, gates aside (#1700). Declared lists, never a gated
    /// verdict, so a gate's negatives are candidates too.
    pub(crate) fn candidate_kinds(&self) -> impl Iterator<Item = u16> + '_ {
        let mut seen = std::collections::BTreeSet::new();
        self.owners
            .iter()
            .chain(self.payers)
            .chain(self.leaf_kinds)
            .copied()
            .filter(move |k| seen.insert(*k))
    }
}

/// One row of a table as the test-only enumeration sees it: declared
/// with every emitted item, `absent` with its reason, or `pending`
/// with its package. A `pending` row emits no predicates on purpose
/// (a consumer that read one would miss what the row will declare),
/// so the enumeration carries none for it either.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[derive(Clone)]
pub enum RowView {
    /// A row with owners.
    Declared(Box<DeclaredRow>),
    /// An `absent "why"` row.
    Absent {
        /// The role token as written.
        role: &'static str,
        /// The declared reason.
        why: &'static str,
    },
    /// A `pending Rnn` row.
    Pending {
        /// The role token as written.
        role: &'static str,
        /// The package that fills it in.
        package: &'static str,
    },
}

#[cfg(any(test, feature = "test-support"))]
impl RowView {
    /// The role token as written on the row.
    #[must_use]
    pub fn role(&self) -> &'static str {
        match self {
            Self::Declared(row) => row.role,
            Self::Absent { role, .. } | Self::Pending { role, .. } => role,
        }
    }

    /// The row's items where it is declared, `None` for `absent` / `pending`.
    #[must_use]
    pub fn declared(&self) -> Option<&DeclaredRow> {
        match self {
            Self::Declared(row) => Some(row),
            _ => None,
        }
    }
}

/// Declares one language's role rows; see the module doc for the row
/// grammar and what each row emits.
///
/// A role declared twice, or both declared and `absent` / `pending`,
/// is rejected at expansion:
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::Java;
/// role_table! {
///     lang: Java;
///     Exit { owns [ReturnStatement] }
///     Exit { absent "cannot both exist and not" }
/// }
/// # fn main() {}
/// ```
///
/// So is a kind named as both an owner and a payer of one row — a
/// kind with no token is a `pays_itself` owner, declared once:
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::Java;
/// role_table! {
///     lang: Java;
///     Comparison { owns [InstanceofExpression] pays [InstanceofExpression] }
/// }
/// # fn main() {}
/// ```
///
/// A `pays_itself` owner on a row that says `pays never` contradicts
/// it, and on a row with no `pays` list is redundant; both are
/// rejected with a message saying which:
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::Java;
/// role_table! {
///     lang: Java;
///     Guard { owns [Guard(pays_itself)] pays never }
/// }
/// # fn main() {}
/// ```
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::Java;
/// role_table! {
///     lang: Java;
///     Exit { owns [ReturnStatement(pays_itself)] }
/// }
/// # fn main() {}
/// ```
///
/// A kind declared twice in one list, or both excluded and declared:
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::{Ancestors, Java, Node};
/// fn always(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
/// role_table! {
///     lang: Java;
///     Exit { owns [ReturnStatement(gated_by always), ReturnStatement] }
/// }
/// # fn main() {}
/// ```
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::Java;
/// role_table! {
///     lang: Java;
///     Comparison { owns [BinaryExpression] pays [EQEQ] excludes [EQEQ "no"] }
/// }
/// # fn main() {}
/// ```
///
/// A second gate, `pays_itself`, `opens` or `callable` on one owner
/// finds its slot in the attribute scan already filled and matches no
/// rule:
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::{Ancestors, Java, Node};
/// fn always(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
/// role_table! {
///     lang: Java;
///     Exit { owns [ReturnStatement(gated_by always text_gated_by always)] }
/// }
/// # fn main() {}
/// ```
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::Java;
/// role_table! {
///     lang: Java;
///     Container { owns [ClassDeclaration(opens Class opens Unit)] }
/// }
/// # fn main() {}
/// ```
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::Java;
/// role_table! {
///     lang: Java;
///     FuncOpener { owns [MethodDeclaration(callable opens Function callable)] }
/// }
/// # fn main() {}
/// ```
///
/// The owner parent-list rejections (#1794) are pinned in
/// `roles/parent_list_doctests.rs`, beside a control that compiles.
///
/// And a kind owned by two rows of the same role:
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::Java;
/// role_table! {
///     lang: Java;
///     Exit { owns [ReturnStatement] }
///     Exit { owns [ThrowStatement] }
/// }
/// # fn main() {}
/// ```
///
/// A `no_chain` gate has no chain parameter, so one declared with the
/// three-argument signature is a type error (#1793):
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::{Ancestors, Java, Node};
/// fn always(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
/// role_table! {
///     lang: Java;
///     Exit { owns [ReturnStatement(gated_by always no_chain)] }
/// }
/// # fn main() {}
/// ```
///
/// `callable_when`, `pays_itself_when` and a payer's gate take no
/// `no_chain` — nothing that drops the chain reads them — so the
/// modifier matches no rule there:
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::{Java, Node};
/// fn always(_: &Node<'_>, _: &[u8]) -> bool { true }
/// role_table! {
///     lang: Java;
///     FuncOpener { owns [LambdaExpression(opens Function callable_when always no_chain)] }
/// }
/// # fn main() {}
/// ```
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::{Java, Node};
/// fn always(_: &Node<'_>, _: &[u8]) -> bool { true }
/// role_table! {
///     lang: Java;
///     Comparison { owns [BinaryExpression] pays [EQEQ gated_by always no_chain] }
/// }
/// # fn main() {}
/// ```
///
/// Only a `FuncOpener` or `Container` row may open a space (#1793):
///
/// ```compile_fail
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::Java;
/// role_table! {
///     lang: Java;
///     StringLit { owns [StringLiteral(opens Function)] }
/// }
/// # fn main() {}
/// ```
///
/// The valid shapes compile:
///
/// ```
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::{Ancestors, Java, Node, SpaceKind};
/// fn always(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
/// fn has_body(_: &Node<'_>, _: &[u8]) -> bool { true }
/// role_table! {
///     lang: Java;
///     Exit { owns [ReturnStatement, ThrowStatement] }
///     Comparison {
///         owns [BinaryExpression(gated_by always), InstanceofExpression(pays_itself)]
///         pays [EQEQ, LT gated_by always]
///         project "operator"
///         excludes [QMARK "type syntax"]
///     }
///     Container {
///         owns [ClassDeclaration(opens Class gated_by has_body no_chain), Program(opens Unit)]
///     }
///     FuncOpener {
///         owns [MethodDeclaration(callable opens Function), StaticInitializer(opens Function)]
///     }
///     Guard { owns [Guard] pays never }
///     Modifier { owns [Static(not_under [ImportDeclaration, RequiresModifier]), Final] }
///     DeclKeyword { owns [Class(under [ClassDeclaration])] }
///     NullSafe { absent "no `?.`" }
///     Comment { pending R11 }
/// }
/// # fn main() {
/// assert_eq!(exit::OWNERS.len(), 2);
/// assert_eq!(func_opener::OWNERS.len(), 2);
/// assert_eq!(comparison::PAYERS, &[Java::EQEQ as u16, Java::LT as u16]);
/// assert_eq!((exit::PAYS_NEVER, guard::PAYS_NEVER), (false, true));
/// assert_eq!((comparison::CHAIN_GATED, container::CHAIN_GATED), (true, false));
/// // A parent list reads the chain, and its kinds are not owners.
/// assert_eq!((modifier::CHAIN_GATED, decl_keyword::CHAIN_GATED), (true, true));
/// assert_eq!(modifier::OWNERS, &[Java::Static as u16, Java::Final as u16]);
/// # }
/// ```
#[macro_export]
#[doc(hidden)]
macro_rules! role_table {
    (lang: $lang:ident; $($role:ident { $($row:tt)* })*) => {
        $( $crate::role_table!(@row $lang $role { $($row)* }); )*

        /// The whole table in the canonical form `utils/role_tables.py`
        /// emits with `--dump`, one row per line in declaration order.
        /// Test-only: the differential test is its one reader.
        #[cfg(test)]
        pub(crate) fn canonical() -> String {
            let mut out = format!("lang {}\n", stringify!($lang));
            $( out.push_str(&$crate::role_table!(@canonical $role)); out.push('\n'); )*
            out
        }

        /// Every row of this table as a value, in declaration order —
        /// the rows the invocation supplies and no others (#1700). The
        /// derived parity test enumerates languages and roles from this,
        /// so a row it does not list is a row no derived assertion asks.
        #[cfg(any(test, feature = "test-support"))]
        pub(crate) fn rows() -> Vec<$crate::roles::RowView> {
            vec![ $( $crate::role_table!(@view $role), )* ]
        }

        /// The `SpaceKind` `node` opens, folded over every `opens`
        /// annotation in this table; `Unknown` when it opens none. This
        /// is the one gated scan of the space-opening rows: a consumer
        /// that needs both "does it open a space" and "which kind"
        /// calls this once and tests for `Unknown`, rather than paying
        /// each owner's gate twice through `owns` and here.
        pub fn opens_space<'a>(
            node: &$crate::Node<'a>,
            code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> $crate::SpaceKind {
            let kind = node.kind_id();
            $( $crate::role_table!(@opens [$lang kind node code ancestors] { $($row)* }); )*
            let _ = (kind, node, code, ancestors);
            $crate::SpaceKind::Unknown
        }
    };

    // ---- row shapes ------------------------------------------------------

    // An `absent` row answers every membership question `false`, so a
    // consumer shared by several languages reads it as it reads a
    // declared row (JavaScript's `Primitive` under the JS-family
    // keyword classifier). A `pending` row emits no predicate: a
    // consumer that read one would silently miss what the row will
    // declare, and the compile error says so.
    (@row $lang:ident $role:ident { absent $why:literal }) => {
        $crate::role_table!(@named $role {
            /// Why this language has no such construct.
            pub const ABSENT: &str = $why;
            /// An absent construct has no owners.
            pub const OWNERS: &[u16] = &[];
            /// An absent construct has no payers, so a reader that asks
            /// several rows about one token can ask this one alike.
            pub const PAYERS: &[u16] = &[];
            /// Nothing pays at the leaf of an absent construct.
            pub const LEAF_KINDS: &[u16] = &[];
            /// An absent construct reads nothing.
            pub const TEXT_GATED: bool = false;
            /// An absent construct reads no chain either.
            pub const CHAIN_GATED: bool = false;
            $crate::role_table!(@absent_predicate owns);
            $crate::role_table!(@absent_predicate pays_at_leaf);
            $crate::role_table!(@absent_predicate pays_at_wrapper);
            $crate::role_table!(@absent_predicate is_callable);
            $crate::role_table!(@absent_predicate_kind is_callable_kind);
            /// Nothing is an absent construct.
            #[inline]
            #[must_use]
            pub fn lists_kind(_kind: u16) -> bool {
                false
            }
            $crate::role_table!(@absent_predicate_kind owns_kind);
            $crate::role_table!(@absent_predicate_kind pays_at_leaf_kind);
            $crate::role_table!(@absent_predicate_kind pays_at_token_kind);
            /// This row in the canonical form `utils/role_tables.py` emits.
            #[cfg(test)]
            pub(crate) fn canonical() -> String {
                format!("{} absent {}", stringify!($role), ABSENT)
            }
            /// This row as the test-only enumeration sees it (#1700).
            #[cfg(any(test, feature = "test-support"))]
            pub(crate) fn view() -> $crate::roles::RowView {
                $crate::roles::RowView::Absent { role: stringify!($role), why: ABSENT }
            }
        });
    };
    // The five predicates a declared row emits, each answering `false`
    // for an absent one.
    (@absent_predicate $name:ident) => {
        /// Nothing is an absent construct.
        #[inline]
        pub fn $name<'a>(
            _node: &$crate::Node<'a>,
            _code: &[u8],
            _ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            false
        }
    };
    (@absent_predicate_kind $name:ident) => {
        /// Nothing is an absent construct.
        #[inline]
        pub fn $name<'a>(
            _kind: u16,
            _node: &$crate::Node<'a>,
            _code: &[u8],
            _ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            false
        }
    };
    (@row $lang:ident $role:ident { pending $package:ident }) => {
        $crate::role_table!(@named $role {
            /// The work package that fills this row in.
            pub const PENDING: &str = stringify!($package);
            /// This row in the canonical form `utils/role_tables.py` emits.
            #[cfg(test)]
            pub(crate) fn canonical() -> String {
                format!("{} pending {}", stringify!($role), PENDING)
            }
            /// This row as the test-only enumeration sees it (#1700).
            #[cfg(any(test, feature = "test-support"))]
            pub(crate) fn view() -> $crate::roles::RowView {
                $crate::roles::RowView::Pending { role: stringify!($role), package: PENDING }
            }
        });
    };
    // The three paying shapes a row can take, told apart by what follows
    // `owns`: a `pays [..]` list splits the projections (leaf = the
    // payers plus every `pays_itself` owner; wrapper = the owners); no
    // list means the owners are the only paying nodes, so both
    // projections are the owner test; `pays never` means neither pays.
    (@row $lang:ident $role:ident { owns $owners:tt pays never $($tail:tt)* }) => {
        $crate::role_table!(@splice $lang $role [never] [] $owners { pays [] $($tail)* });
    };
    (@row $lang:ident $role:ident { owns $owners:tt pays [ $($payer:tt)* ] $($tail:tt)* }) => {
        $crate::role_table!(@splice $lang $role [split] [] $owners { pays [ $($payer)* ] $($tail)* });
    };
    (@row $lang:ident $role:ident { owns $owners:tt $($tail:tt)* }) => {
        $crate::role_table!(@splice $lang $role [owners] [] $owners { pays [] $($tail)* });
    };

    // ---- shared kind fragments ------------------------------------------
    //
    // An `owns` list may hold `..name!()` entries, each a `macro_rules!`
    // that expands to a kind list shared by several languages (the
    // C-family modifiers, say). `role_table!` cannot expand a macro in
    // matcher position, so the list is walked here, entry by entry, into
    // an accumulator, and a fragment is given the accumulator to append
    // to and the rest of the list to continue with (continuation
    // passing). A fragment is written by hand in this exact shape —
    // `$$` meta-variables, which would let a macro generate it, are
    // unstable:
    //
    // ```text
    // macro_rules! c_family_modifiers {
    //     (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
    //         $crate::role_table!(@owners $cont [$($acc)* Static, Extern,] $($rest)*);
    //     };
    // }
    // ```
    //
    // The same walk serves the row's `owns` list, its `pays` list and
    // the `opens_space` fold: the three continuations differ in shape
    // (`[lang role mode { tail }]`, `[pays lang role mode [owners]
    // { tail }]` and `[lang kind node code ancestors]`), so the walk's
    // terminal arms tell them apart and a fragment carries its list
    // once, whichever list splices it. A payer entry is `K` or `K
    // gated_by f`, so a fragment of payers spells its gate inline
    // (`LT gated_by applied,`) and the gate resolves at the dialect's
    // row, where the fragment is expanded. `utils/role_tables.py`
    // reads that one arm, and the differential test holds the compiled
    // row to it. A fragment contributes kinds only; a dialect-specific
    // gate stays on the dialect's row, beside its reason.

    (@splice $lang:ident $role:ident $mode:tt [$($acc:tt)*] [ $($entries:tt)* ] $tail:tt) => {
        $crate::role_table!(@owners [$lang $role $mode $tail] [$($acc)*] $($entries)*);
    };
    (@owners $cont:tt [$($acc:tt)*] ..$frag:ident!() $(, $($rest:tt)*)?) => {
        $frag!(@splice $cont [$($acc)*] $($($rest)*)?);
    };
    (@owners $cont:tt [$($acc:tt)*] $k:ident gated_by $g:ident $(, $($rest:tt)*)?) => {
        $crate::role_table!(@owners $cont [$($acc)* $k gated_by $g,] $($($rest)*)?);
    };
    // A payer's `text_gated_by` is the same expansion as `gated_by`; the
    // distinct keyword is documentation at the row, as it is on an owner.
    (@owners $cont:tt [$($acc:tt)*] $k:ident text_gated_by $g:ident $(, $($rest:tt)*)?) => {
        $crate::role_table!(@owners $cont [$($acc)* $k gated_by $g,] $($($rest)*)?);
    };
    // A payer's `under P` / `under [P, Q]` is carried as a bracketed
    // parent list, so the row body's payer pattern needs one shape.
    (@owners $cont:tt [$($acc:tt)*] $k:ident under [ $($p:ident),+ $(,)? ] $(, $($rest:tt)*)?) => {
        $crate::role_table!(@owners $cont [$($acc)* $k under [$($p),*],] $($($rest)*)?);
    };
    (@owners $cont:tt [$($acc:tt)*] $k:ident under $p:ident $(, $($rest:tt)*)?) => {
        $crate::role_table!(@owners $cont [$($acc)* $k under [$p],] $($($rest)*)?);
    };
    (@owners $cont:tt [$($acc:tt)*] $k:ident $(( $($attr:tt)* ))? $(, $($rest:tt)*)?) => {
        $crate::role_table!(@owners $cont [$($acc)* $k $(( $($attr)* ))?,] $($($rest)*)?);
    };
    // The `owns` list is complete: walk the `pays` list the same way,
    // carrying the owners along.
    (@owners [$lang:ident $role:ident $mode:tt { pays [ $($payers:tt)* ] $($tail:tt)* }]
        [$($acc:tt)*]) => {
        $crate::role_table!(
            @owners [pays $lang $role $mode [$($acc)*] { $($tail)* }] [] $($payers)*
        );
    };
    (@owners [pays $lang:ident $role:ident $mode:tt [$($owners:tt)*] { $($tail:tt)* }]
        [$($acc:tt)*]) => {
        $crate::role_table!(
            @row_body $lang $role $mode { owns [ $($owners)* ] pays [ $($acc)* ] $($tail)* }
        );
    };
    (@owners [$l:ident $k:ident $n:ident $c:ident $a:ident]
        [ $($owner:ident $(( $($owner_attr:tt)* ))?,)* ]) => {
        $( $crate::role_table!(
            @scan opens_done [] [$l $k $n $c $a] $owner [] [] [] [] $($($owner_attr)*)?
        ); )*
    };
    (@row_body $lang:ident $role:ident [$mode:ident] {
        owns [ $($owner:ident $(( $($owner_attr:tt)* ))?),* $(,)? ]
        pays [ $($payer:ident $(gated_by $payer_gate:ident)? $(under [$($payer_parent:ident),+ $(,)?])?),* $(,)? ]
        $(project $field:literal)?
        $(wraps [ $($wrapped:ident),* $(,)? ])?
        $(excludes [ $($excluded:ident $excluded_why:literal),* $(,)? ])?
    }) => {
        $crate::role_table!(@named $role {
            use $crate::{Ancestors, Node};
            #[allow(unused_imports)]
            use super::*;

            /// Every kind this row declares as owning the construct.
            pub const OWNERS: &[u16] = &[ $( $lang::$owner as u16, )* ];
            /// Every kind this row declares as paying for it.
            pub const PAYERS: &[u16] = &[ $( $lang::$payer as u16, )* ];
            /// Kinds the row's author considered and rejected, with why.
            pub const EXCLUDED: &[(u16, &str)] =
                &[ $($( ($lang::$excluded as u16, $excluded_why), )*)? ];
            /// Interpolation child kinds a `StringLit` owner can hold.
            pub const WRAPS: &[u16] = &[ $($( $lang::$wrapped as u16, )*)? ];
            /// Whether the construct has owners but no paying node at all.
            pub const PAYS_NEVER: bool = $crate::role_table!(@mode $mode never);
            /// The row's paying mode as declared: `owners`, `split` or
            /// `never` — the one field of `canonical()` the kind lists
            /// cannot reconstruct (#1745), so the test-only view carries
            /// it rather than inferring it.
            #[cfg(any(test, feature = "test-support"))]
            pub(crate) const MODE: &str = stringify!($mode);
            // Whether the owners themselves are paying nodes: yes on a
            // plain row, no once a `pays` list splits the token off, no
            // under `pays never`.
            const OWNERS_PAY: bool = $crate::role_table!(@mode $mode owners);

            /// This row in the canonical form `utils/role_tables.py` emits:
            /// `<Role> rows owners=[ids] payers=[ids] wraps=[ids]
            /// excluded=[id=why;…] mode=<owners|split|never>
            /// text_gated=<bool> chain_gated=<bool>`, kinds as ids so the
            /// two sides compare without naming variants, the mode after
            /// the lists, and the two context flags last (#1793). The
            /// Python reader is a second implementation of the DSL; the
            /// differential test in `roles/mod.rs` holds it to this.
            #[cfg(test)]
            pub(crate) fn canonical() -> String {
                let ids = |kinds: &[u16]| {
                    kinds.iter().map(u16::to_string).collect::<Vec<_>>().join(",")
                };
                let excluded = EXCLUDED
                    .iter()
                    .map(|(k, why)| format!("{k}={why}"))
                    .collect::<Vec<_>>()
                    .join(";");
                format!(
                    "{} rows owners=[{}] payers=[{}] wraps=[{}] excluded=[{}] mode={} \
                     text_gated={} chain_gated={}",
                    stringify!($role),
                    ids(OWNERS),
                    ids(PAYERS),
                    ids(WRAPS),
                    excluded,
                    stringify!($mode),
                    TEXT_GATED,
                    CHAIN_GATED
                )
            }

            const _: () = assert!(
                !$crate::roles::kinds_overlap(OWNERS, PAYERS),
                "a kind cannot be both an owner and a payer of one row; \
                 declare a tokenless owner once as `pays_itself`"
            );
            const _: () = assert!(
                !$crate::roles::kinds_repeat(OWNERS) && !$crate::roles::kinds_repeat(PAYERS),
                "a kind is declared twice in one list of this row; the second \
                 clause could never fire"
            );
            const _: () = {
                let excluded =
                    $crate::roles::excluded_kinds::<{ EXCLUDED.len() }>(EXCLUDED);
                assert!(
                    !$crate::roles::kinds_overlap(&excluded, OWNERS)
                        && !$crate::roles::kinds_overlap(&excluded, PAYERS),
                    "a kind this row excludes is also one it owns or pays for"
                );
            };
            // Only the two space-tree roles may open a space: the
            // byte-less space forwards assert `!CHAIN_GATED` on exactly
            // those two rows, and `opens_space` folds over every row
            // (#1793).
            const _: () = {
                $( $crate::role_table!(
                    @scan opens_role [$role] [] $owner [] [] [] [] $($($owner_attr)*)?
                ); )*
            };

            /// Whether `node` owns the construct.
            #[inline]
            pub fn owns<'a>(
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                owns_kind(node.kind_id(), node, code, ancestors)
            }

            /// Whether the row names `kind` among its owners, gates aside:
            /// the byte-less spelling of [`owns`]. Spelled as a chain of
            /// compares, not `OWNERS.contains`: `<[u16]>::contains` is an
            /// out-of-line loop over a runtime-length slice, which LLVM
            /// cannot fold into the kind `switch` a consumer dispatches
            /// on, while the chain becomes case labels of it (#1697).
            #[inline]
            #[must_use]
            pub fn lists_kind(kind: u16) -> bool {
                let _ = kind;
                false $(|| kind == $lang::$owner as u16)*
            }

            /// [`owns`] with `node`'s kind id already read, for a caller
            /// that asks several rows about one node: `kind_id()` is an
            /// FFI call per read.
            #[inline]
            pub fn owns_kind<'a>(
                kind: u16,
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                $( $crate::role_table!(
                    @scan owner_done [] [$lang kind node code ancestors] $owner
                        [] [] [] [] $($($owner_attr)*)?
                ); )*
                let _ = (kind, node, code, ancestors);
                false
            }

            /// Whether `node` is the token a counting arm fires on: a
            /// declared payer, or an owner that pays for itself. With no
            /// `pays` list the owners are the paying nodes; with `pays
            /// never`, nothing is.
            #[inline]
            pub fn pays_at_leaf<'a>(
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                pays_at_leaf_kind(node.kind_id(), node, code, ancestors)
            }

            /// [`pays_at_leaf`] with `node`'s kind id already read.
            #[inline]
            pub fn pays_at_leaf_kind<'a>(
                kind: u16,
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                $( $crate::role_table!(
                    @clause_gated [$lang kind node code ancestors] $payer
                        [$($payer_gate)? $(under [$($payer_parent),*])?]
                ); )*
                $( $crate::role_table!(
                    @scan self_payer_done [$mode] [$lang kind node code ancestors] $owner
                        [] [] [] [] $($($owner_attr)*)?
                ); )*
                OWNERS_PAY && owns_kind(kind, node, code, ancestors)
            }

            /// Whether `node` is a *declared payer* paying for the
            /// construct — [`pays_at_leaf_kind`] without the `pays_itself`
            /// owners, which pay at the leaf but have no token to bill.
            /// What a token-billing consumer (Halstead) reads.
            #[inline]
            pub fn pays_at_token_kind<'a>(
                kind: u16,
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                // A compare chain, not `PAYERS.contains`, for the reason on
                // `lists_kind`: this runs on every token of the Halstead
                // walk, before the byte-less fallback.
                let is_payer = false $(|| kind == $lang::$payer as u16)*;
                is_payer && pays_at_leaf_kind(kind, node, code, ancestors)
            }

            /// Whether any owner is `text_gated_by` — the row reads the
            /// bytes it is handed. A consumer that asks the row with none,
            /// as the byte-less `Checker` spellings do while handing the
            /// walk's chain through, pins that this stays `false` and
            /// fails the day it does not, instead of missing the owner.
            pub const TEXT_GATED: bool = false $( || $crate::role_table!(
                @scan text_gated_kind [] [] $owner [] [] [] [] $($($owner_attr)*)?
            ) )*;

            /// Whether any owner carries a `gated_by` / `text_gated_by`
            /// gate or an `opens_by` fn that is *not* marked `no_chain`,
            /// or an `under` / `not_under` parent list (#1794)
            /// — the row may read the chain it is handed through
            /// [`owns`], [`pays_at_wrapper`] or the table's `opens_space`
            /// fold. A forward that hands the row a known-empty chain
            /// pins that this stays `false` (#1793). It says nothing of
            /// `callable_when`, `pays_itself_when` or a payer's gate,
            /// which only [`is_callable`] and [`pays_at_leaf`] read.
            pub const CHAIN_GATED: bool = false $( || $crate::role_table!(
                @scan chain_gated_kind [] [] $owner [] [] [] [] $($($owner_attr)*)?
            ) )*;

            /// Every kind [`pays_at_leaf_kind`] can answer `true` for,
            /// gates aside: the declared payers plus the `pays_itself`
            /// owners on a split row; every owner on a plain row; none
            /// under `pays never`. A consumer that reads several rows'
            /// leaves asserts at build time that two tiers cannot fire on
            /// one kind (`impl_cyclomatic_rows!`, #1697).
            pub const LEAF_KINDS: &[u16] = &{
                const MARKED: &[Option<u16>] = &[
                    $( Some($lang::$payer as u16), )*
                    $( $crate::role_table!(
                        @scan self_payer_kind [$mode] [$lang] $owner [] [] [] [] $($($owner_attr)*)?
                    ), )*
                ];
                $crate::roles::some_kinds::<{ $crate::roles::count_some(MARKED) }>(MARKED)
            };

            /// Every kind this row marks `callable` or `callable_when`,
            /// gates aside.
            pub const CALLABLE: &[u16] = &{
                // One `Option` per owner, in list order: the marked ones.
                const MARKED: &[Option<u16>] = &[ $( $crate::role_table!(
                    @scan callable_kind [] [$lang] $owner [] [] [] [] $($($owner_attr)*)?
                ), )* ];
                $crate::roles::some_kinds::<{ $crate::roles::count_some(MARKED) }>(MARKED)
            };

            /// Every owner marked `holds_payer`, gates aside: the kinds
            /// whose ownership is their projection landing on a payer,
            /// which the test-only enumeration pins to project (#1700).
            #[cfg(any(test, feature = "test-support"))]
            pub(crate) const HOLDS_PAYER: &[u16] = &{
                const MARKED: &[Option<u16>] = &[ $( $crate::role_table!(
                    @scan holds_payer_kind [] [$lang] $owner [] [] [] [] $($($owner_attr)*)?
                ), )* ];
                $crate::roles::some_kinds::<{ $crate::roles::count_some(MARKED) }>(MARKED)
            };

            /// Whether `node` is a `callable` owner of the construct — a
            /// named function, for `Checker::is_func` — under its gate.
            #[inline]
            pub fn is_callable<'a>(
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                is_callable_kind(node.kind_id(), node, code, ancestors)
            }

            /// [`is_callable`] with `node`'s kind id already read, as
            /// [`owns_kind`] is to [`owns`].
            #[inline]
            pub fn is_callable_kind<'a>(
                kind: u16,
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                $( $crate::role_table!(
                    @scan callable_done [] [$lang kind node code ancestors] $owner
                        [] [] [] [] $($($owner_attr)*)?
                ); )*
                let _ = (kind, node, code, ancestors);
                false
            }

            /// Whether `node` is the owning node, for a consumer that
            /// pays at the wrapper rather than at its token. Equal to
            /// `owns` unless the row says `pays never`.
            #[inline]
            pub fn pays_at_wrapper<'a>(
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                !PAYS_NEVER && owns(node, code, ancestors)
            }

            /// The field holding an owner's paying token, where the row
            /// declares one.
            pub const PROJECT: Option<&str> = $crate::role_table!(@project $($field)?);

            /// The child of an owner that pays for it: the owner's
            /// declared `project` field, or — where the row declares
            /// none, or the grammar gives this owner no such field
            /// (Java's `instanceof_expression` carries its keyword as a
            /// bare child) — its first child whose kind is a declared
            /// payer. `None` when neither holds, which for a declared
            /// owner is a row defect.
            ///
            /// The field is read by grammar id, resolved once per row:
            /// `tree_sitter` resolves a name by a string compare over
            /// the grammar's field table on every call, and a row's
            /// gate asks this of every owner-kind node (#1695).
            pub fn project<'a>(owner: &Node<'a>) -> Option<Node<'a>> {
                $crate::role_table!(@projected owner $($field)?).or_else(|| {
                    owner
                        .children()
                        .find(|child| PAYERS.contains(&child.kind_id()))
                })
            }

            /// Whether `node` owns the construct *through* the payer
            /// of kind `payer`: for a consumer that scores only one
            /// of a row's payers (C#'s `??` but not its conditional
            /// access), asked of the projection once.
            pub fn owns_via<'a>(
                payer: u16,
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                owns(node, code, ancestors)
                    && project(node).is_some_and(|child| child.kind_id() == payer)
            }

            /// This row as the test-only enumeration sees it: every
            /// emitted item as a value (#1700).
            #[cfg(any(test, feature = "test-support"))]
            pub(crate) fn view() -> $crate::roles::RowView {
                $crate::roles::RowView::Declared(Box::new($crate::roles::DeclaredRow {
                    role: stringify!($role),
                    owners: OWNERS,
                    payers: PAYERS,
                    leaf_kinds: LEAF_KINDS,
                    callable: CALLABLE,
                    holds_payer: HOLDS_PAYER,
                    wraps: WRAPS,
                    excluded: EXCLUDED,
                    text_gated: TEXT_GATED,
                    chain_gated: CHAIN_GATED,
                    pays_never: PAYS_NEVER,
                    mode: MODE,
                    pays_at_wrapper,
                    owns_kind,
                    pays_at_leaf_kind,
                    pays_at_token_kind,
                    is_callable_kind,
                    lists_kind,
                    project,
                }))
            }
        });
    };

    // ---- the declared projection field, or none ---------------------------

    (@project) => { None };
    (@project $field:literal) => { Some($field) };

    // The declared field's child, through a grammar id resolved on the
    // first ask and kept in a `static` of the row's `project` fn; a row
    // with no field emits no cache and reads only the payer fallback.
    (@projected $owner:ident) => { None::<Node<'a>> };
    (@projected $owner:ident $field:literal) => {{
        static FIELD: ::std::sync::OnceLock<Option<u16>> = ::std::sync::OnceLock::new();
        FIELD
            .get_or_init(|| $owner.field_id($field))
            .and_then(|field| $owner.child_by_field_id(field))
    }};

    // ---- the three paying modes --------------------------------------------
    //
    // `@mode <row's mode> <asked mode>`: whether the two are the same.

    (@mode never never) => { true };
    (@mode owners owners) => { true };
    (@mode $have:ident $want:ident) => { false };

    // ---- the owner-attribute scan -----------------------------------------
    //
    // One left-to-right walk over an owner's attributes, normalising them
    // into four slots — `[gate]`, `[pays_itself]`, `[space kind]`,
    // `[callable]` — and then handing the slots to the terminal named by
    // `$done`. A second gate, a second `pays_itself`, a second `opens` or
    // a second `callable` finds its slot already filled and matches no
    // rule, so it fails to expand.
    //
    // A gate slot holds `[g]` for a gate with a chain parameter and
    // `[g no_chain]` for one declared without it (#1793): the clause
    // emitters below call the first with three arguments and the second
    // with two, so a `no_chain` gate cannot be given the chain by
    // construction. `opens_by f no_chain` is carried as `[by f no_chain]`
    // the same way, and an owner's parent list as `[under [P, ..]]` or
    // `[not_under [P, ..]]` (#1794). `callable_when`, `pays_itself_when`
    // and a payer's gate take no `no_chain`: nothing that drops the
    // chain reads them, and a mark no consumer asserts would be a claim
    // nothing checks.

    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt $p:tt $s:tt $c:tt) => {
        $crate::role_table!(@$d $m $ctx $k $g $p $s $c)
    };
    // `TEXT_GATED` reads `text_gated_by` off the attribute list before the
    // fold below merges it with `gated_by` into one gate slot, so these
    // two come first: the word may follow `holds_payer`, or any attribute
    // the fold has already consumed. A terminal that reads a word the fold
    // consumes must sit *above* the fold's rules: placed below them it
    // still compiles and still matches some orders, and no production row
    // need spell a losing one; `dsl_gates` declares the word in each
    // position so the pin, not a row, finds it.
    // A detector emits `true` and re-emits nothing, so the optional
    // `holds_payer` is a literal `$()?` here; the folds below cannot do
    // the same, since a literal-only repetition cannot be re-emitted.
    (@scan text_gated_kind $m:tt $ctx:tt $k:ident $g:tt $p:tt $s:tt $c:tt $(holds_payer)? text_gated_by $f:ident $($r:tt)*) => {
        true
    };
    // `CHAIN_GATED` reads a gate or `opens_by` *without* `no_chain` off
    // the list. The marked folds sit above the detectors so a marked fn
    // is consumed, modifier and all, before a detector can see its
    // keyword — a detector matching `gated_by $f $($r)*` would otherwise
    // read `gated_by f no_chain` as chain-reading, since the modifier is
    // just the head of `$r` (the review's `rustc` probe, #1793). The
    // unmarked detectors then sit above the ordinary folds for the same
    // reason `text_gated_kind` does.
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt holds_payer gated_by $g:ident no_chain $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [holds_payer $g no_chain] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt holds_payer text_gated_by $g:ident no_chain $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [holds_payer $g no_chain] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt gated_by $g:ident no_chain $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [$g no_chain] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt text_gated_by $g:ident no_chain $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [$g no_chain] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt $p:tt [] $c:tt opens_by $f:ident no_chain $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k $g $p [by $f no_chain] $c $($r)*)
    };
    (@scan chain_gated_kind $m:tt $ctx:tt $k:ident $g:tt $p:tt $s:tt $c:tt $(holds_payer)? gated_by $f:ident $($r:tt)*) => {
        true
    };
    (@scan chain_gated_kind $m:tt $ctx:tt $k:ident $g:tt $p:tt $s:tt $c:tt $(holds_payer)? text_gated_by $f:ident $($r:tt)*) => {
        true
    };
    (@scan chain_gated_kind $m:tt $ctx:tt $k:ident $g:tt $p:tt $s:tt $c:tt opens_by $f:ident $($r:tt)*) => {
        true
    };
    // `holds_payer` is a gate of its own, carried in the gate slot as
    // `[holds_payer]` or, with a gate beside it, `[holds_payer g]`.
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt holds_payer gated_by $g:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [holds_payer $g] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt holds_payer text_gated_by $g:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [holds_payer $g] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt holds_payer $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [holds_payer] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [$g:ident] $p:tt $s:tt $c:tt holds_payer $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [holds_payer $g] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [$g:ident no_chain] $p:tt $s:tt $c:tt holds_payer $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [holds_payer $g no_chain] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt gated_by $g:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [$g] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt text_gated_by $g:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [$g] $p $s $c $($r)*)
    };
    // An owner's parent list is its gate, so it needs the slot empty:
    // beside `gated_by` or `holds_payer`, in either order, it matches
    // no rule (#1794).
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt under [$($u:ident),+ $(,)?] $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [under [$($u),+]] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt $c:tt not_under [$($u:ident),+ $(,)?] $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [not_under [$($u),+]] $p $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt [] $s:tt $c:tt pays_itself $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k $g [pays_itself] $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt [] $s:tt $c:tt pays_itself_when $w:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k $g [pays_itself $w] $s $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt $p:tt [] $c:tt opens $s:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k $g $p [$s] $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt $p:tt [] $c:tt opens_by $f:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k $g $p [by $f] $c $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt $p:tt $s:tt [] callable $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k $g $p $s [callable] $($r)*)
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt $p:tt $s:tt [] callable_when $w:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k $g $p $s [callable $w] $($r)*)
    };

    // ---- the scan's terminals ----------------------------------------------

    // In `owns`: the owner's clause, gated or not. The gate slot — `[]`,
    // `[g]`, `[g no_chain]`, `[under [..]]` or `[not_under [..]]` — is
    // handed to `@gate` whole, here and in every terminal below that
    // emits a clause; an empty slot renders as `true`. `holds_payer` is
    // an `owns`-only mark, read off the slot first here since it changes
    // the clause rather than gating it; any other terminal hands it to
    // `@gate` as a gate name, so beside `callable`, `opens` or
    // `pays_itself` it does not compile.
    (@owner_done [] $ctx:tt $k:ident [holds_payer $($g:ident $($nc:ident)?)?] $p:tt $s:tt $c:tt) => {
        $crate::role_table!(@clause_payer_gated $ctx $k [$($g $($nc)?)?]);
    };
    (@owner_done [] $ctx:tt $k:ident $g:tt $p:tt $s:tt $c:tt) => {
        $crate::role_table!(@clause_gated $ctx $k $g);
    };
    // In `TEXT_GATED`: whether the owner's gate reads the bytes. The scan
    // folds `text_gated_by` and `gated_by` into one gate slot, so the
    // word is read off the attribute list before the fold.
    (@text_gated_kind [] [] $k:ident $g:tt $p:tt $s:tt $c:tt) => {
        false
    };
    // In `CHAIN_GATED`: every gate and `opens_by` the owner carries was
    // consumed with its `no_chain`, or the owner carries none — unless
    // the gate is a parent list, which reads the chain by construction
    // and takes no modifier, so the slot alone says so (#1794).
    (@chain_gated_kind [] [] $k:ident [$rel:ident [$($q:ident),+]] $p:tt $s:tt $c:tt) => {
        true
    };
    (@chain_gated_kind [] [] $k:ident $g:tt $p:tt $s:tt $c:tt) => {
        false
    };
    // The opener-role check: an owner with a filled space slot sits on
    // a `FuncOpener` or `Container` row, or the table fails to compile.
    (@opens_role [$role:ident] [] $k:ident $g:tt $p:tt [] $c:tt) => {};
    (@opens_role [FuncOpener] [] $k:ident $g:tt $p:tt $s:tt $c:tt) => {};
    (@opens_role [Container] [] $k:ident $g:tt $p:tt $s:tt $c:tt) => {};
    (@opens_role [$role:ident] [] $k:ident $g:tt $p:tt $s:tt $c:tt) => {
        compile_error!(concat!(
            "`opens` / `opens_by` on a `",
            stringify!($role),
            "` row: only `FuncOpener` and `Container` open a space (#1793)"
        ));
    };
    // In `LEAF_KINDS`: `Some(kind)` for an owner that can pay at the
    // leaf under the row's mode — every owner on a plain row, the
    // `pays_itself` ones on a split row, none under `pays never`.
    (@self_payer_kind [owners] [$l:ident] $k:ident $g:tt $p:tt $s:tt $c:tt) => {
        Some($l::$k as u16)
    };
    (@self_payer_kind [split] [$l:ident] $k:ident $g:tt [pays_itself $($w:ident)?] $s:tt $c:tt) => {
        Some($l::$k as u16)
    };
    (@self_payer_kind [$mode:ident] [$l:ident] $k:ident $g:tt $p:tt $s:tt $c:tt) => {
        None
    };
    // In `CALLABLE`: `Some(kind)` for a marked owner, `None` otherwise;
    // `some_kinds` drops the `None`s at compile time.
    // In `HOLDS_PAYER`: `Some(kind)` for an owner whose gate slot holds
    // `holds_payer`, with or without a gate beside it.
    (@holds_payer_kind [] [$l:ident] $k:ident [holds_payer $($g:ident $($nc:ident)?)?] $p:tt $s:tt $c:tt) => {
        Some($l::$k as u16)
    };
    (@holds_payer_kind [] [$l:ident] $k:ident $g:tt $p:tt $s:tt $c:tt) => {
        None
    };
    (@callable_kind [] [$l:ident] $k:ident $g:tt $p:tt $s:tt []) => {
        None
    };
    (@callable_kind [] [$l:ident] $k:ident $g:tt $p:tt $s:tt [callable $($w:ident)?]) => {
        Some($l::$k as u16)
    };
    // In `is_callable`: a `callable` owner's clause, under its gate, and
    // a `callable_when w` owner's under its gate *and* `w`; an owner
    // without either mark emits nothing.
    (@callable_done [] $ctx:tt $k:ident $g:tt $p:tt $s:tt []) => {};
    (@callable_done [] $ctx:tt $k:ident $g:tt $p:tt $s:tt [callable]) => {
        $crate::role_table!(@clause_gated $ctx $k $g);
    };
    (@callable_done [] $ctx:tt $k:ident $g:tt $p:tt $s:tt [callable $w:ident]) => {
        $crate::role_table!(@clause_when_gated $ctx $k $w $g);
    };
    // In `pays_at_leaf`: a `pays_itself` owner pays under its own gate,
    // and a `pays_itself_when w` owner under its gate *and* `w` — the
    // owner whose grammar gives it a token in one spelling and none in
    // another (Groovy's `a in b` / `a !in b`), so the token pays where
    // there is one and the owner where there is not. The attribute is
    // only meaningful on a split row — on a plain row every owner
    // already pays, and under `pays never` it would contradict the row
    // — so the other two modes reject it.
    (@self_payer_done [$mode:ident] $ctx:tt $k:ident $g:tt [] $s:tt $c:tt) => {};
    (@self_payer_done [split] $ctx:tt $k:ident $g:tt [pays_itself] $s:tt $c:tt) => {
        $crate::role_table!(@clause_gated $ctx $k $g);
    };
    (@self_payer_done [split] $ctx:tt $k:ident $g:tt [pays_itself $w:ident] $s:tt $c:tt) => {
        $crate::role_table!(@clause_when_gated $ctx $k $w $g);
    };
    (@self_payer_done [never] $ctx:tt $k:ident $g:tt [pays_itself $($w:ident)?] $s:tt $c:tt) => {
        compile_error!("`pays_itself` contradicts `pays never`: the row says nothing pays");
    };
    (@self_payer_done [owners] $ctx:tt $k:ident $g:tt [pays_itself $($w:ident)?] $s:tt $c:tt) => {
        compile_error!(
            "`pays_itself` is redundant without a `pays` list: every owner already pays"
        );
    };
    // In `opens_space`: an `opens K` owner returns its kind, under its gate.
    (@opens_done [] $ctx:tt $k:ident $g:tt $p:tt [] $c:tt) => {};
    (@opens_done [] $ctx:tt $k:ident $g:tt $p:tt [by $f:ident $($fnc:ident)?] $c:tt) => {
        $crate::role_table!(@space_clause_by_gated $ctx $k [$f $($fnc)?] $g);
    };
    (@opens_done [] $ctx:tt $k:ident $g:tt $p:tt [$s:ident] $c:tt) => {
        $crate::role_table!(@space_clause_gated $ctx $k $s $g);
    };

    // ---- the `opens_space` fold -------------------------------------------

    (@opens $ctx:tt { absent $why:literal }) => {};
    (@opens $ctx:tt { pending $package:ident }) => {};
    (@opens $ctx:tt { owns [ $($entries:tt)* ] $($r:tt)* }) => {
        $crate::role_table!(@owners $ctx [] $($entries)*);
    };

    // ---- the emitted clauses ----------------------------------------------
    //
    // `$ctx` is `[lang kind node code ancestors]`, bundled so every rule
    // above fits one line; these are the only rules that open it. `kind`
    // is the node's `kind_id()`, read once per emitted function: the
    // read is an FFI call the optimiser cannot hoist, and one read per
    // clause blocked the bitmap codegen a `matches!` gets.

    //
    // A gate call is rendered by `@gate`: three arguments for a gate
    // with a chain parameter, two for one declared `no_chain` (#1793),
    // so the chain is unobtainable by a marked gate by construction —
    // the proof is the signature, not the row. A parent list, an
    // owner's or a payer's, is the parent-kind test itself (#1794).

    (@gate [$l:ident $kind:ident $n:ident $c:ident $a:ident] []) => {
        true
    };
    (@gate [$l:ident $kind:ident $n:ident $c:ident $a:ident] [$g:ident]) => {
        $g($n, $c, $a)
    };
    (@gate [$l:ident $kind:ident $n:ident $c:ident $a:ident] [$g:ident no_chain]) => {
        $g($n, $c)
    };
    // Read off the walk's chain, `Ancestors::parent`: `Node::parent`
    // resolves a parent by descending from the root (#1084, #1584). A
    // node with no parent is under nothing, so `not_under` admits it.
    (@gate [$l:ident $kind:ident $n:ident $c:ident $a:ident] [under [$($p:ident),*]]) => {
        $a.parent($n).is_some_and(|parent| {
            let parent = parent.kind_id();
            false $(|| parent == $l::$p as u16)*
        })
    };
    (@gate [$l:ident $kind:ident $n:ident $c:ident $a:ident] [not_under [$($p:ident),*]]) => {
        !$crate::role_table!(@gate [$l $kind $n $c $a] [under [$($p),*]])
    };

    (@clause_gated [$l:ident $kind:ident $n:ident $c:ident $a:ident] $k:ident $g:tt) => {
        if $kind == $l::$k as u16 {
            return $crate::role_table!(@gate [$l $kind $n $c $a] $g);
        }
    };
    // A `holds_payer` owner: its projection lands on one of the row's
    // payers, under its gate if it has one.
    (@clause_payer_gated [$l:ident $kind:ident $n:ident $c:ident $a:ident] $k:ident $g:tt) => {
        if $kind == $l::$k as u16 {
            return $crate::role_table!(@gate [$l $kind $n $c $a] $g)
                && project($n).is_some_and(|child| PAYERS.contains(&child.kind_id()));
        }
    };
    (@clause_when_gated [$l:ident $kind:ident $n:ident $c:ident $a:ident] $k:ident $w:ident $g:tt) => {
        if $kind == $l::$k as u16 {
            return $crate::role_table!(@gate [$l $kind $n $c $a] $g) && $w($n, $c, $a);
        }
    };
    (@space_clause_gated [$l:ident $kind:ident $n:ident $c:ident $a:ident] $k:ident $s:ident $g:tt) => {
        if $kind == $l::$k as u16 && $crate::role_table!(@gate [$l $kind $n $c $a] $g) {
            return $crate::SpaceKind::$s;
        }
    };
    // An `opens_by f` owner: `f` answers the kind, `Unknown` for none;
    // `f` takes the `no_chain` mark like a gate.
    (@space_clause_by_gated [$l:ident $kind:ident $n:ident $c:ident $a:ident] $k:ident $f:tt $g:tt) => {
        if $kind == $l::$k as u16 && $crate::role_table!(@gate [$l $kind $n $c $a] $g) {
            return $crate::role_table!(@gate [$l $kind $n $c $a] $f);
        }
    };

    // ---- the fixed role → module-name mapping ----------------------------
    //
    // Generated names come from this table, not from identifier
    // concatenation; a role not listed here is a compile error, which is
    // what fixes the role set.

    (@canonical Call) => { call::canonical() };
    (@canonical StringLit) => { string_lit::canonical() };
    (@canonical Comparison) => { comparison::canonical() };
    (@canonical ShortCircuit) => { short_circuit::canonical() };
    (@canonical EagerLogical) => { eager_logical::canonical() };
    (@canonical Ternary) => { ternary::canonical() };
    (@canonical NullSafe) => { null_safe::canonical() };
    (@canonical CaseArm) => { case_arm::canonical() };
    (@canonical Guard) => { guard::canonical() };
    (@canonical Branch) => { branch::canonical() };
    (@canonical Switch) => { switch::canonical() };
    (@canonical Loop) => { r#loop::canonical() };
    (@canonical Catch) => { catch::canonical() };
    (@canonical ElseIf) => { else_if::canonical() };
    (@canonical Exit) => { exit::canonical() };
    (@canonical Modifier) => { modifier::canonical() };
    (@canonical DeclKeyword) => { decl_keyword::canonical() };
    (@canonical Primitive) => { primitive::canonical() };
    (@canonical FuncOpener) => { func_opener::canonical() };
    (@canonical Closure) => { closure::canonical() };
    (@canonical Container) => { container::canonical() };
    (@canonical Member) => { member::canonical() };
    (@canonical Comment) => { comment::canonical() };
    (@canonical DocComment) => { doc_comment::canonical() };
    (@canonical Docstring) => { docstring::canonical() };
    (@canonical DocTarget) => { doc_target::canonical() };
    (@canonical DocRunOpener) => { doc_run_opener::canonical() };
    (@canonical DocRunDirective) => { doc_run_directive::canonical() };
    (@canonical DocBodyResidual) => { doc_body_residual::canonical() };

    (@view Call) => { call::view() };
    (@view StringLit) => { string_lit::view() };
    (@view Comparison) => { comparison::view() };
    (@view ShortCircuit) => { short_circuit::view() };
    (@view EagerLogical) => { eager_logical::view() };
    (@view Ternary) => { ternary::view() };
    (@view NullSafe) => { null_safe::view() };
    (@view CaseArm) => { case_arm::view() };
    (@view Guard) => { guard::view() };
    (@view Branch) => { branch::view() };
    (@view Switch) => { switch::view() };
    (@view Loop) => { r#loop::view() };
    (@view Catch) => { catch::view() };
    (@view ElseIf) => { else_if::view() };
    (@view Exit) => { exit::view() };
    (@view Modifier) => { modifier::view() };
    (@view DeclKeyword) => { decl_keyword::view() };
    (@view Primitive) => { primitive::view() };
    (@view FuncOpener) => { func_opener::view() };
    (@view Closure) => { closure::view() };
    (@view Container) => { container::view() };
    (@view Member) => { member::view() };
    (@view Comment) => { comment::view() };
    (@view DocComment) => { doc_comment::view() };
    (@view Docstring) => { docstring::view() };
    (@view DocTarget) => { doc_target::view() };
    (@view DocRunOpener) => { doc_run_opener::view() };
    (@view DocRunDirective) => { doc_run_directive::view() };
    (@view DocBodyResidual) => { doc_body_residual::view() };

    (@named Call { $($b:tt)* }) => {
        /// This language's `Call` row.
        pub mod call { $($b)* }
    };
    (@named StringLit { $($b:tt)* }) => {
        /// This language's `StringLit` row.
        pub mod string_lit { $($b)* }
    };
    (@named Comparison { $($b:tt)* }) => {
        /// This language's `Comparison` row.
        pub mod comparison { $($b)* }
    };
    (@named ShortCircuit { $($b:tt)* }) => {
        /// This language's `ShortCircuit` row.
        pub mod short_circuit { $($b)* }
    };
    (@named EagerLogical { $($b:tt)* }) => {
        /// This language's `EagerLogical` row.
        pub mod eager_logical { $($b)* }
    };
    (@named Ternary { $($b:tt)* }) => {
        /// This language's `Ternary` row.
        pub mod ternary { $($b)* }
    };
    (@named NullSafe { $($b:tt)* }) => {
        /// This language's `NullSafe` row.
        pub mod null_safe { $($b)* }
    };
    (@named CaseArm { $($b:tt)* }) => {
        /// This language's `CaseArm` row.
        pub mod case_arm { $($b)* }
    };
    (@named Guard { $($b:tt)* }) => {
        /// This language's `Guard` row.
        pub mod guard { $($b)* }
    };
    (@named Branch { $($b:tt)* }) => {
        /// This language's `Branch` row.
        pub mod branch { $($b)* }
    };
    (@named Switch { $($b:tt)* }) => {
        /// This language's `Switch` row.
        pub mod switch { $($b)* }
    };
    (@named Loop { $($b:tt)* }) => {
        /// This language's `Loop` row.
        pub mod r#loop { $($b)* }
    };
    (@named Catch { $($b:tt)* }) => {
        /// This language's `Catch` row.
        pub mod catch { $($b)* }
    };
    (@named ElseIf { $($b:tt)* }) => {
        /// This language's `ElseIf` row.
        pub mod else_if { $($b)* }
    };
    (@named Exit { $($b:tt)* }) => {
        /// This language's `Exit` row.
        pub mod exit { $($b)* }
    };
    (@named Modifier { $($b:tt)* }) => {
        /// This language's `Modifier` row.
        pub mod modifier { $($b)* }
    };
    (@named DeclKeyword { $($b:tt)* }) => {
        /// This language's `DeclKeyword` row.
        pub mod decl_keyword { $($b)* }
    };
    (@named Primitive { $($b:tt)* }) => {
        /// This language's `Primitive` row.
        pub mod primitive { $($b)* }
    };
    (@named FuncOpener { $($b:tt)* }) => {
        /// This language's `FuncOpener` row.
        pub mod func_opener { $($b)* }
    };
    (@named Closure { $($b:tt)* }) => {
        /// This language's `Closure` row.
        pub mod closure { $($b)* }
    };
    (@named Container { $($b:tt)* }) => {
        /// This language's `Container` row.
        pub mod container { $($b)* }
    };
    (@named Member { $($b:tt)* }) => {
        /// This language's `Member` row.
        pub mod member { $($b)* }
    };
    (@named Comment { $($b:tt)* }) => {
        /// This language's `Comment` row.
        pub mod comment { $($b)* }
    };
    (@named DocComment { $($b:tt)* }) => {
        /// This language's `DocComment` row.
        pub mod doc_comment { $($b)* }
    };
    (@named Docstring { $($b:tt)* }) => {
        /// This language's `Docstring` row.
        pub mod docstring { $($b)* }
    };
    (@named DocTarget { $($b:tt)* }) => {
        /// This language's `DocTarget` row.
        pub mod doc_target { $($b)* }
    };
    (@named DocRunOpener { $($b:tt)* }) => {
        /// This language's `DocRunOpener` row.
        pub mod doc_run_opener { $($b)* }
    };
    (@named DocRunDirective { $($b:tt)* }) => {
        /// This language's `DocRunDirective` row.
        pub mod doc_run_directive { $($b)* }
    };
    (@named DocBodyResidual { $($b:tt)* }) => {
        /// This language's `DocBodyResidual` row.
        pub mod doc_body_residual { $($b)* }
    };
}

/// One language's table as the test-only enumeration sees it: its
/// `LANG` variant name, its rows in declaration order, and the
/// `opens_space` fold (#1700).
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct LanguageTable {
    /// The language.
    pub lang: LANG,
    /// The `LANG` variant's name, as the census and the Python reader
    /// spell it.
    pub name: &'static str,
    /// The table's rows, in declaration order.
    pub rows: fn() -> Vec<RowView>,
    /// The table's `opens_space` fold.
    pub opens_space: for<'a> fn(&Node<'a>, &[u8], Ancestors<'a, '_>) -> SpaceKind,
    /// The grammar's spelling of a kind id, from the generated enum —
    /// so a declared kind can be named with no node of it in hand.
    pub kind_name: fn(u16) -> &'static str,
    /// The table in the canonical form `utils/role_tables.py` emits.
    #[cfg(test)]
    pub(crate) canonical: fn() -> String,
}

/// Every language's table, named by `LANG` variant: the one list a
/// cross-language test enumerates languages from — the rows are
/// compiled unconditionally, so a test over a feature-gated set filters
/// this by the enabled languages' names — held to the enum by
/// `tables_name_every_language` below. Hand-listed like the `mod`
/// lines above, which is what that pin is for.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[must_use]
pub fn tables() -> &'static [LanguageTable] {
    // `lang: module`, not `lang => module`: an `=>` at call-site
    // indentation reads as a match arm to the rustfmt-bail probe, which
    // `role_table!`'s own decision note avoids the same way.
    macro_rules! tables {
        ($($lang:ident : $module:ident),* $(,)?) => {
            &[ $( LanguageTable {
                lang: LANG::$lang,
                name: stringify!($lang),
                rows: $module::rows,
                opens_space: $module::opens_space,
                kind_name: |id| crate::$lang::from(id).into(),
                #[cfg(test)]
                canonical: $module::canonical,
            }, )* ]
        };
    }
    tables![
        Bash: bash,
        C: c,
        Ccomment: ccomment,
        Cpp: cpp,
        Csharp: csharp,
        Elixir: elixir,
        Go: go,
        Groovy: groovy,
        Irules: irules,
        Java: java,
        Javascript: javascript,
        Kotlin: kotlin,
        Lua: lua,
        Mozcpp: mozcpp,
        Mozjs: mozjs,
        Objc: objc,
        Perl: perl,
        Php: php,
        Preproc: preproc,
        Python: python,
        Ruby: ruby,
        Rust: rust,
        Tcl: tcl,
        Tsx: tsx,
        Typescript: typescript,
    ]
}

/// The role set, in the order the `@named` table of `role_table!`
/// spells it: the one list a cross-language test checks every table's
/// `rows()` against (#1700). A role added to the macro and not here
/// fails `every_production_table_declares_every_role_once`; one added
/// here and not to the macro fails to compile on the first table that
/// declares it. `utils/role_tables.py` carries the same set for the
/// census, held to the compiled tables by the differential test.
// `cfg(test)` for the reason on `candidate_kinds`.
#[cfg(test)]
pub(crate) const ROLES: &[&str] = &[
    "Call",
    "StringLit",
    "Comparison",
    "ShortCircuit",
    "EagerLogical",
    "Ternary",
    "NullSafe",
    "CaseArm",
    "Guard",
    "Branch",
    "Switch",
    "Loop",
    "Catch",
    "ElseIf",
    "Exit",
    "Modifier",
    "DeclKeyword",
    "Primitive",
    "FuncOpener",
    "Closure",
    "Container",
    "Member",
    "Comment",
    "DocComment",
    "Docstring",
    "DocTarget",
    "DocRunOpener",
    "DocRunDirective",
    "DocBodyResidual",
];

#[cfg(test)]
mod tests {
    use crate::{Ancestors, Java, Node};

    /// The two `const fn`s that build `CALLABLE` run at compile time on
    /// every table, which executes none of their lines under
    /// instrumentation; asked at run time over the shapes the tables
    /// produce — no marks, all marks, marks at either end and in the
    /// middle, an empty list — they count and compact the `Some`s in
    /// order.
    #[test]
    fn count_some_and_some_kinds_compact_the_marked_owners_in_order() {
        use super::{count_some, some_kinds};
        const NONE: &[Option<u16>] = &[None, None, None];
        const ALL: &[Option<u16>] = &[Some(3), Some(1), Some(2)];
        const ENDS: &[Option<u16>] = &[Some(7), None, None, Some(9)];
        const MIDDLE: &[Option<u16>] = &[None, Some(5), None];
        const EMPTY: &[Option<u16>] = &[];
        assert_eq!(count_some(NONE), 0);
        assert_eq!(count_some(ALL), 3);
        assert_eq!(count_some(ENDS), 2);
        assert_eq!(count_some(MIDDLE), 1);
        assert_eq!(count_some(EMPTY), 0);
        assert_eq!(some_kinds::<0>(NONE), [0u16; 0]);
        assert_eq!(some_kinds::<3>(ALL), [3, 1, 2], "list order, not sorted");
        assert_eq!(some_kinds::<2>(ENDS), [7, 9]);
        assert_eq!(some_kinds::<1>(MIDDLE), [5]);
        assert_eq!(some_kinds::<0>(EMPTY), [0u16; 0]);
    }

    /// An absent row's `lists_kind` is false for every kind, as its
    /// `OWNERS` is empty; the one a declared row emits reads the table.
    /// The rows are declarations, built in every feature set.
    #[test]
    fn an_absent_rows_lists_kind_is_false_for_every_kind() {
        use crate::roles::java::{call, null_safe};
        assert_eq!(null_safe::OWNERS, &[] as &[u16]);
        for kind in [0u16, 1, Java::MethodInvocation as u16, u16::MAX] {
            assert!(!null_safe::lists_kind(kind), "absent row, kind {kind}");
        }
        assert!(call::lists_kind(Java::MethodInvocation as u16));
        assert!(!call::lists_kind(Java::ClassBody as u16));
    }

    // `utils/role_tables.py` reads the declaration text for the three
    // role gates, so it is a second implementation of this macro's
    // meaning. This holds it to the compiled constants: every table's
    // `canonical()` must equal what `--dump` prints for the same file.
    // The canonical form is the *kind lists* — owners, payers, wraps,
    // exclusions — which is what the three gates read, plus the paying
    // mode, which none of them reads yet: it is in the form so the
    // reader's `pays_mode` is held to the macro's dispatch (#1745),
    // because `pays []` and a plain `owns [..]` row print the same
    // lists, so a reader that misread one as the other agreed with
    // every table until the mode was emitted. An owner's or payer's
    // gate, `holds_payer`, `under`, `not_under`,
    // `pays_itself` and `opens` are not in it beyond the two flags, and
    // the reader's handling of those is pinned by
    // `utils/check-role-census-test.py` instead.
    // Every language's module is compiled unconditionally, so the
    // comparison needs no grammar and runs in every build. A missing
    // `python3` is a failure, not a skip: a pass that ran nothing would
    // read as agreement. The table below is hand-listed like the `mod`
    // lines above; the first assertion holds it to the `LANG` enum so a
    // language added there cannot be forgotten here.
    //
    // Not on Windows: the test matrix installs no Python there, and the
    // gates this test guards run in the Linux lint job. `--dump` writes
    // LF bytes, so a Windows developer with python3 can still run it by
    // hand. The alias is gated with its one user: on Windows it would
    // otherwise be the module's only dead item (#1792).
    #[cfg(not(windows))]
    type Canonical = fn() -> String;

    /// `tables` names every `LANG` once, under the variant's own
    /// spelling, so a cross-language test enumerating from it cannot
    /// miss a language the enum has or spell one differently.
    #[test]
    fn tables_name_every_language() {
        let tables = super::tables();
        let listed: std::collections::BTreeSet<String> =
            tables.iter().map(|t| t.name.to_owned()).collect();
        let known: std::collections::BTreeSet<String> = crate::LANG::into_enum_iter()
            .map(|l| format!("{l:?}"))
            .collect();
        pretty_assertions::assert_eq!(listed, known);
        assert_eq!(tables.len(), known.len(), "no language listed twice");
    }

    /// Every production table declares every role of `ROLES` exactly
    /// once — declared, `absent` or `pending` — in some order, and no
    /// role outside it. The census gate reads the same fact from the
    /// source; this reads it from what the macro emitted, so a row the
    /// macro dropped on the floor is seen here. Built in every feature
    /// set: the rows are declarations.
    #[test]
    fn every_production_table_declares_every_role_once() {
        let want: std::collections::BTreeSet<&str> = super::ROLES.iter().copied().collect();
        assert_eq!(want.len(), super::ROLES.len(), "ROLES lists a role twice");
        for table in super::tables() {
            let rows = (table.rows)();
            let roles: Vec<&str> = rows.iter().map(super::RowView::role).collect();
            let got: std::collections::BTreeSet<&str> = roles.iter().copied().collect();
            assert_eq!(
                roles.len(),
                got.len(),
                "{}: a role declared twice",
                table.lang
            );
            pretty_assertions::assert_eq!(got, want, "{}", table.lang);
        }
    }

    /// `rows()` and `canonical()` are two emissions of one declaration
    /// list; they agree row for row, in order, on the role and on the
    /// kind lists, so the Python reader — held to `canonical()` — is
    /// transitively held to the enumeration the derived test reads.
    #[test]
    fn rows_agree_with_canonical_row_for_row() {
        use super::RowView;
        for table in super::tables() {
            let canonical = (table.rows)();
            let lines: Vec<String> = (table.canonical)()
                .lines()
                .skip(1)
                .map(str::to_owned)
                .collect();
            assert_eq!(lines.len(), canonical.len(), "{}: row count", table.name);
            for (line, row) in lines.iter().zip(&canonical) {
                let ids = |kinds: &[u16]| {
                    kinds
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                };
                let want = match row {
                    RowView::Absent { role, why } => format!("{role} absent {why}"),
                    RowView::Pending { role, package } => format!("{role} pending {package}"),
                    RowView::Declared(r) => {
                        let excluded = r
                            .excluded
                            .iter()
                            .map(|(k, why)| format!("{k}={why}"))
                            .collect::<Vec<_>>()
                            .join(";");
                        format!(
                            "{} rows owners=[{}] payers=[{}] wraps=[{}] excluded=[{}] mode={} \
                             text_gated={} chain_gated={}",
                            r.role,
                            ids(r.owners),
                            ids(r.payers),
                            ids(r.wraps),
                            excluded,
                            r.mode,
                            r.text_gated,
                            r.chain_gated
                        )
                    }
                };
                pretty_assertions::assert_eq!(line, &want, "{}", table.name);
            }
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn python_reader_agrees_with_the_compiled_tables() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the ast crate sits under the workspace root");
        let tables: Vec<(String, Canonical)> = super::tables()
            .iter()
            .map(|t| (t.name.to_lowercase(), t.canonical))
            .collect();
        let listed: std::collections::BTreeSet<String> =
            tables.iter().map(|(s, _)| s.clone()).collect();
        let known: std::collections::BTreeSet<String> = crate::LANG::into_enum_iter()
            .map(|l| format!("{l:?}").to_lowercase())
            .collect();
        pretty_assertions::assert_eq!(listed, known, "every LANG has a table listed here");
        for (stem, canonical) in &tables {
            let path = root.join(format!("big-code-analysis-ast/src/roles/{stem}.rs"));
            let out = std::process::Command::new("python3")
                .arg("-B")
                .arg(root.join("utils/role_tables.py"))
                .arg("--dump")
                .arg(&path)
                .output()
                .expect("python3 is on PATH: the role gates need it, so this test does too");
            assert!(
                out.status.success(),
                "{stem}: role_tables.py --dump failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let printed = String::from_utf8(out.stdout).expect("dump is UTF-8");
            pretty_assertions::assert_eq!(printed, canonical(), "{stem}");
        }
    }

    fn never<'a>(_: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
        false
    }

    fn always<'a>(_: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
        true
    }

    /// `always` in the `no_chain` shape: no chain parameter (#1793).
    fn always_no_chain(_: &Node<'_>, _: &[u8]) -> bool {
        true
    }

    // An `always` that counts its calls, so a test can assert which
    // emitted predicates reach an owner's gate and which do not.
    crate::observation::counter!(recording_gate_calls, cfg(test));

    fn recording<'a>(_: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
        recording_gate_calls::record();
        true
    }

    // A table no language declares, to reach the one scan terminal the
    // three proof tables do not: a *gated* `pays_itself` owner. Without
    // this, dropping the gate from `@self_payer_done` failed nothing.
    //
    // A proof table exists to be expanded, not read: a test asks it for
    // the few constants and predicates that pin the shape under proof,
    // and the rest of what the macro emits uniformly is unread, so under
    // `--tests` most of each table is dead. The `allow` sits on each
    // table rather than on the module, so a helper of `mod tests` that
    // no test reads any more is still reported (#1792). What it does not
    // report: a helper reached *only* from a gate inside an allowed
    // table — `always_no_chain` today — since rustc reads an allowed
    // item's callees as live. Every proof table below carries the same
    // attribute.
    #[allow(dead_code)]
    mod gated_self_payer {
        use super::never;
        use crate::Java;

        role_table! {
            lang: Java;
            Comparison {
                owns [InstanceofExpression(pays_itself gated_by never)]
                pays [EQEQ]
            }
        }
    }

    // The two gates the DSL spells itself (#1695): an owner that
    // `holds_payer` — its projection lands on one of the row's payers,
    // optionally under a gate of its own — and a payer `under` one or
    // several parent kinds. Each is declared in the direction that
    // makes the DSL arm decide: the gated `holds_payer` under `always`,
    // so only the projection half can say no; the real parent *second*
    // in the `under [..]` list, so a walk that read only the first
    // alternative would miss it; and a `pays never` row, the shape no
    // production table declares since #1695, kept here so its two
    // projections stay pinned. The `Member`, `Call` and `FuncOpener`
    // rows spell `text_gated_by` in every position an attribute list
    // can put it — alone, after `holds_payer`, and after the fold has
    // consumed an `opens` and a `callable` (the `FuncOpener`'s
    // `RecordDeclaration`, since only an opener row may carry `opens`,
    // #1793) — so `TEXT_GATED` is pinned to read the word wherever it
    // sits.
    //
    // A proof table; see `gated_self_payer`.
    #[allow(dead_code)]
    mod dsl_gates {
        use super::{always, class_or_interface, never, recording};
        use crate::Java;

        role_table! {
            lang: Java;
            // The split-empty idiom the `ElseIf` rows spell (#1697):
            // `pays []` makes the row `[split]`, so an owner pays at the
            // leaf only when marked `pays_itself`. Three owners, each a
            // distinct scan terminal: X marked and ungated (pays), Y
            // admitted by a *recording* gate and unmarked (owns, pays
            // at the wrapper, never at the leaf — and its gate must not
            // even run on the leaf path), Z marked under `never`
            // (neither). A `never` gate alone could not tell split-empty
            // from the owner-paying mode, which is why Y's gate records.
            ElseIf {
                owns [
                    WhileStatement(pays_itself),
                    IfStatement(gated_by recording),
                    DoStatement(pays_itself gated_by never),
                ]
                pays []
            }
            Comparison {
                owns [BinaryExpression(holds_payer)]
                pays [EQEQ, LT under BinaryExpression, GT under [TernaryExpression, BinaryExpression]]
                project "operator"
            }
            ShortCircuit {
                owns [BinaryExpression(holds_payer gated_by always)]
                pays [AMPAMP under TernaryExpression]
                project "operator"
            }
            Ternary {
                owns [TernaryExpression(gated_by never), InstanceofExpression(pays_itself gated_by always)]
                pays [QMARK]
                project "operator"
            }
            Guard { owns [Guard] pays never }
            FuncOpener {
                owns [
                    MethodDeclaration(callable opens Function),
                    StaticInitializer(opens Function),
                    ConstructorDeclaration(opens Function gated_by never callable),
                    LambdaExpression(opens Function callable_when never),
                    CompactConstructorDeclaration(opens Function callable_when always),
                    RecordDeclaration(opens Function callable text_gated_by always),
                ]
            }
            Container { owns [ClassDeclaration(opens_by class_or_interface)] }
            Member { owns [FieldDeclaration(text_gated_by always)] }
            Call {
                owns [MethodInvocation(holds_payer text_gated_by always)]
                pays [LPAREN]
            }
        }
    }

    // `no_chain` in every position the attribute list can put it
    // (#1793), every row reading `false`: the `Member` row is the one
    // shape a `holds_payer` gate takes the modifier in, `Exit` the
    // plain gate alone and before `pays_itself`, `Catch` the text gate
    // alone, `Container` the two-fn owner with both fns marked in both
    // orders, `FuncOpener` the text gate after the fold has consumed an
    // `opens` and a `callable`. `Loop` and `Branch` are the unmarked
    // controls, read `true`. Each marked fn is also a compile-time
    // proof of its own consumption: it has no chain parameter, so a
    // scan that handed it three arguments would not build.
    //
    // A proof table; see `gated_self_payer`.
    #[allow(dead_code)]
    mod dsl_no_chain {
        use super::{always, always_no_chain, class_or_interface_no_chain};
        use crate::Java;

        role_table! {
            lang: Java;
            Container {
                owns [
                    ClassDeclaration(opens_by class_or_interface_no_chain no_chain gated_by always_no_chain no_chain),
                    InterfaceDeclaration(gated_by always_no_chain no_chain opens_by class_or_interface_no_chain no_chain),
                ]
            }
            FuncOpener {
                owns [LambdaExpression(opens Function callable text_gated_by always_no_chain no_chain)]
            }
            Member { owns [FieldDeclaration(holds_payer text_gated_by always_no_chain no_chain)] pays [EQEQ] }
            Exit {
                owns [ReturnStatement(gated_by always_no_chain no_chain), ThrowStatement(gated_by always_no_chain no_chain pays_itself)]
                pays [Return]
            }
            Catch { owns [CatchClause(text_gated_by always_no_chain no_chain)] }
            Loop { owns [WhileStatement(holds_payer gated_by always)] pays [While] }
            Branch { owns [IfStatement(text_gated_by always)] }
        }
    }

    // The two-fn owner with exactly one fn unmarked (#1793): four
    // shapes — which fn, and which is written first — one per row so
    // each is pinned `true` on its own, since a row's `CHAIN_GATED` is
    // an OR over its owners. A reader that recorded one owner-level
    // flag, or read only the last fn, would get one order of each pair
    // wrong; the Python reader's self-test spells the same four.
    //
    // A proof table; see `gated_self_payer`.
    #[allow(dead_code)]
    mod dsl_two_fn_gate_marked {
        use super::{always_no_chain, class_or_interface};
        use crate::Java;

        role_table! {
            lang: Java;
            Container { owns [ClassDeclaration(opens_by class_or_interface gated_by always_no_chain no_chain)] }
            FuncOpener { owns [EnumDeclaration(gated_by always_no_chain no_chain opens_by class_or_interface)] }
        }
    }

    // A proof table; see `gated_self_payer`.
    #[allow(dead_code)]
    mod dsl_two_fn_opens_by_marked {
        use super::{always, class_or_interface_no_chain};
        use crate::Java;

        role_table! {
            lang: Java;
            Container { owns [ClassDeclaration(opens_by class_or_interface_no_chain no_chain gated_by always)] }
            FuncOpener { owns [EnumDeclaration(gated_by always opens_by class_or_interface_no_chain no_chain)] }
        }
    }

    // An owner's parent list (#1794) in every terminal it reaches, each
    // the only chain reader of its row — the `Container`'s `opens_by` is
    // marked `no_chain` — so the row's `CHAIN_GATED` is the list's
    // alone. Every list names the decisive parent *second*, so a clause
    // that read only the first alternative would miss it, and the
    // fixture spells each owner under its listed parent and away from
    // it, so a terminal that dropped the list would answer both alike.
    // The `never` `when` fns are the other half: a terminal that dropped
    // the `when` would pay at the leaf, or be callable, where the list
    // admits the node.
    //
    // A proof table; see `gated_self_payer`.
    #[allow(dead_code)]
    mod dsl_parent_gates {
        use super::{always, class_or_interface_no_chain, never};
        use crate::Java;

        role_table! {
            lang: Java;
            Branch { owns [IfStatement(under [ClassBody, Block])] }
            Loop { owns [WhileStatement(not_under [ClassBody, Block])] }
            ElseIf {
                owns [
                    IfStatement(pays_itself not_under [ClassBody, Block]),
                    DoStatement(pays_itself_when always under [ClassBody, Block]),
                    WhileStatement(pays_itself_when never under [ClassBody, Block]),
                ]
                pays []
            }
            FuncOpener {
                owns [
                    ConstructorDeclaration(opens Function callable_when never under [Program, ClassBody]),
                    MethodDeclaration(callable opens Function under [Program, ClassBody]),
                    LambdaExpression(opens Function callable_when always not_under [ClassBody, VariableDeclarator]),
                ]
            }
            Container {
                owns [ClassDeclaration(opens_by class_or_interface_no_chain no_chain under [InterfaceBody, Program])]
            }
        }
    }

    /// `opens_by`'s proof: a class named `I` is an `Interface`, any
    /// other a `Class`.
    pub(super) fn class_or_interface<'a>(
        node: &Node<'a>,
        code: &[u8],
        _: Ancestors<'a, '_>,
    ) -> crate::SpaceKind {
        if node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(code))
            == Some("I")
        {
            crate::SpaceKind::Interface
        } else {
            crate::SpaceKind::Class
        }
    }

    /// `class_or_interface` in the `no_chain` shape.
    pub(super) fn class_or_interface_no_chain(node: &Node<'_>, code: &[u8]) -> crate::SpaceKind {
        class_or_interface(node, code, Ancestors::known(&[]))
    }

    // A fragment splices its kinds into the row in list order, before
    // the dialect's own entries, in both the row and the `opens_space`
    // fold; the dialect's attributes stay on its own entries.
    //
    // A proof table; see `gated_self_payer`.
    #[allow(dead_code)]
    mod spliced {
        use super::never;
        use crate::Java;

        macro_rules! jvm_openers {
            (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
                $crate::role_table!(
                    @owners $cont [$($acc)* MethodDeclaration(opens Function), ConstructorDeclaration(opens Function),]
                    $($rest)*
                );
            };
        }

        // A payer fragment spells its gate inline, so a shared list of
        // gated tokens resolves the gate at the splicing row.
        macro_rules! equality_tokens {
            (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
                $crate::role_table!(
                    @owners $cont [$($acc)* EQEQ, BANGEQ gated_by never,]
                    $($rest)*
                );
            };
        }

        role_table! {
            lang: Java;
            FuncOpener { owns [..jvm_openers!(), StaticInitializer(opens Function)] }
            // A `Container` row, not `Exit`: only the two space-tree
            // roles may splice an `opens` fragment (#1793).
            Container { owns [..jvm_openers!()] pays [ReturnStatement] }
            Comparison { owns [BinaryExpression] pays [..equality_tokens!(), LT gated_by never] }
            // A `pending` row, which no production table declares at
            // this close: the enumeration's third shape, pinned here
            // so `rows()` and `canonical()` agree on it too.
            Guard { pending R99 }
        }
    }

    // An absent row answers every predicate a declared row has, so a
    // consumer shared across languages compiles against either.
    #[cfg(feature = "javascript")]
    #[test]
    fn an_absent_row_answers_every_predicate_false() {
        use crate::JavascriptCode;
        use crate::roles::javascript::primitive;
        use crate::test_support::for_each_node_with_chain;
        let source = b"let x = 1;\n";
        let mut asked = 0;
        for_each_node_with_chain::<JavascriptCode>(source, |node, chain| {
            let a = Ancestors::known(chain);
            let kind = node.kind_id();
            assert!(!primitive::owns(node, source, a));
            assert!(!primitive::pays_at_leaf(node, source, a));
            assert!(!primitive::pays_at_wrapper(node, source, a));
            assert!(!primitive::owns_kind(kind, node, source, a));
            assert!(!primitive::pays_at_leaf_kind(kind, node, source, a));
            assert!(!primitive::pays_at_token_kind(kind, node, source, a));
            asked += 1;
        });
        assert!(asked > 1);
        assert_eq!(primitive::ABSENT, "JavaScript has no type names");
        // The constants a reader asking several rows reads alike.
        assert_eq!((primitive::OWNERS, primitive::PAYERS), (&[][..], &[][..]));
    }

    /// `LEAF_KINDS` — the kinds `pays_at_leaf_kind` can answer `true`
    /// for, gates aside — per row mode (#1697): a plain row's owners; a
    /// split row's payers plus its `pays_itself` owners, a gated one
    /// included; nothing under `pays never` or on an absent row. Read by
    /// the root crate's build-time cross-tier assert, so making every
    /// list empty would leave every metric unchanged and disable the
    /// assert — which is why the slices are pinned here by hand.
    #[test]
    fn leaf_kinds_follow_the_row_mode() {
        // plain: owners
        assert_eq!(dsl_gates::guard::LEAF_KINDS, &[] as &[u16], "pays never");
        assert_eq!(
            spliced::container::LEAF_KINDS,
            &[Java::ReturnStatement as u16],
            "split, no self-payers"
        );
        assert_eq!(
            spliced::func_opener::LEAF_KINDS,
            spliced::func_opener::OWNERS,
            "plain: every owner"
        );
        // split with self-payers, gated or not: payers first, then owners
        assert_eq!(
            dsl_gates::ternary::LEAF_KINDS,
            &[Java::QMARK as u16, Java::InstanceofExpression as u16]
        );
        assert_eq!(
            gated_self_payer::comparison::LEAF_KINDS,
            &[Java::EQEQ as u16, Java::InstanceofExpression as u16]
        );
        assert_eq!(
            dsl_gates::else_if::LEAF_KINDS,
            &[Java::WhileStatement as u16, Java::DoStatement as u16],
            "split-empty: the marked owners only, the unmarked one not"
        );
        assert_eq!(
            crate::roles::java::null_safe::LEAF_KINDS,
            &[] as &[u16],
            "absent"
        );
    }

    #[test]
    fn a_fragment_splices_its_kinds_before_the_rows_own() {
        assert_eq!(
            spliced::func_opener::OWNERS,
            &[
                Java::MethodDeclaration as u16,
                Java::ConstructorDeclaration as u16,
                Java::StaticInitializer as u16
            ]
        );
        assert_eq!(
            spliced::container::OWNERS,
            &[
                Java::MethodDeclaration as u16,
                Java::ConstructorDeclaration as u16
            ]
        );
        assert_eq!(spliced::container::PAYERS, &[Java::ReturnStatement as u16]);
        assert_eq!(
            spliced::comparison::PAYERS,
            &[Java::EQEQ as u16, Java::BANGEQ as u16, Java::LT as u16]
        );
    }

    // A payer fragment's inline gate is read: `BANGEQ gated_by never`
    // pays nowhere while the ungated `EQEQ` beside it pays, and the
    // row's own gated entry after the fragment is read the same way.
    #[cfg(feature = "java")]
    #[test]
    fn a_payer_fragment_keeps_its_inline_gates() {
        use crate::JavaCode;
        use crate::test_support::for_each_node_with_chain;
        let source = b"class A { boolean f(int a, int b) { return a == b || a != b || a < b; } }\n";
        let mut paid = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            if spliced::comparison::pays_at_leaf(node, source, Ancestors::known(chain)) {
                paid.push(node.kind());
            }
        });
        assert_eq!(paid, ["=="]);
    }

    #[cfg(feature = "java")]
    #[test]
    fn a_fragment_reaches_the_opens_space_fold() {
        use crate::test_support::for_each_node_with_chain;
        use crate::{JavaCode, SpaceKind};
        let source = b"class A { A() { } void m() { } static { } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let k = spliced::opens_space(node, source, Ancestors::known(chain));
            if k != SpaceKind::Unknown {
                seen.push((node.kind(), k));
            }
        });
        assert_eq!(
            seen,
            [
                ("constructor_declaration", SpaceKind::Function),
                ("method_declaration", SpaceKind::Function),
                ("static_initializer", SpaceKind::Function),
            ]
        );
    }

    #[cfg(feature = "java")]
    #[test]
    fn a_gated_self_payer_pays_at_the_leaf_under_its_gate() {
        use crate::JavaCode;
        use crate::test_support::for_each_node_with_chain;
        let source = b"class A { boolean f(Object x) { return x instanceof String; } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            if node.kind_id() == Java::InstanceofExpression as u16 {
                let ancestors = Ancestors::known(chain);
                seen.push((
                    gated_self_payer::comparison::owns(node, source, ancestors),
                    gated_self_payer::comparison::pays_at_leaf(node, source, ancestors),
                ));
            }
        });
        // The gate says no, so the owner neither owns nor pays; an
        // ungated clause would have paid at the leaf.
        assert_eq!(seen, [(false, false)]);
    }

    #[cfg(feature = "java")]
    #[test]
    fn holds_payer_and_under_gate_through_the_projection_and_the_parent() {
        use crate::JavaCode;
        use crate::test_support::for_each_node_with_chain;
        use dsl_gates::{comparison, short_circuit};
        // Three `binary_expression`s: `a < b` and `a + b > 0` hold a
        // `Comparison` payer, `a + b` holds none; `a < b && a + b > 0`
        // holds `&&`, the `ShortCircuit` payer, and that row's gate is
        // `always`, so only the projection decides. The `<` and `>`
        // are under their parent (the `>` through the *second* listed
        // alternative), the `>` of `List<String>` is not (a type
        // argument list), and `&&` is under a `binary_expression`, not
        // the ternary its row names.
        let source = b"class A { boolean f(int a, int b, java.util.List<String> l) { return a < b && a + b > 0; } }\n";
        let (mut owns, mut chains, mut pays) = (Vec::new(), Vec::new(), Vec::new());
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = node.utf8_text(source).expect("fixture is UTF-8");
            if comparison::owns(node, source, ancestors) {
                owns.push(text.to_owned());
            }
            if short_circuit::owns(node, source, ancestors) {
                chains.push(text.to_owned());
            }
            if comparison::pays_at_leaf(node, source, ancestors)
                || short_circuit::pays_at_leaf(node, source, ancestors)
            {
                pays.push(text.to_owned());
            }
        });
        assert_eq!(owns, ["a < b", "a + b > 0"]);
        assert_eq!(chains, ["a < b && a + b > 0"]);
        assert_eq!(pays, ["<", ">"]);
    }

    // A gated `pays_itself` owner pays where its gate says so, and a
    // gated owner owns nowhere where its gate says no — the two scan
    // terminals a `never` gate alone cannot tell from "no clause
    // emitted" (the `gated_self_payer` table pins that other side).
    #[cfg(feature = "java")]
    #[test]
    fn a_gated_self_payer_pays_where_its_gate_admits_it() {
        use crate::JavaCode;
        use crate::test_support::for_each_node_with_chain;
        use dsl_gates::ternary;
        let source =
            b"class A { int f(Object o, boolean a) { return o instanceof String ? 1 : 2; } }\n";
        let (mut owns, mut pays) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = node.utf8_text(source).expect("fixture is UTF-8");
            if ternary::owns(node, source, ancestors) {
                owns.push(text.to_owned());
            }
            if ternary::pays_at_leaf(node, source, ancestors) {
                pays.push(text.to_owned());
            }
        });
        // The ternary's gate is `never`: neither it nor its `?` (an
        // ungated payer, which pays regardless) is gated away — the `?`
        // pays; the `instanceof_expression` pays for itself under
        // `always`, and owns through the same gate.
        assert_eq!(owns, ["o instanceof String"]);
        assert_eq!(pays, ["o instanceof String", "?"]);
    }

    /// The split-empty row (`pays []`, #1697): a `pays_itself` owner
    /// pays at the leaf; an admitted unmarked owner owns and pays at the
    /// wrapper but not at the leaf, and its gate is never consulted on
    /// the leaf path — `OWNERS_PAY` is `false` on a split row, so the
    /// tail `OWNERS_PAY && owns_kind(..)` short-circuits before any
    /// gate; a marked owner under `never` neither owns nor pays. The
    /// row's declared lists are what the DSL reader sees: no payers —
    /// the same lists an owner-paying row prints, so only the `mode=`
    /// field of `canonical()` tells the two apart (#1745), which is why
    /// this pins the predicates as well.
    #[cfg(feature = "java")]
    #[test]
    fn a_split_row_with_no_payers_pays_only_at_its_self_paying_owners() {
        use crate::JavaCode;
        use crate::test_support::for_each_node_with_chain;
        use dsl_gates::else_if;
        let source =
            b"class A { void f(boolean a) { if (a) { } while (a) { } do { } while (a); } }\n";
        // (kind, owns, pays_at_leaf, pays_at_wrapper, gate calls during
        // owns, gate calls during pays_at_leaf).
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let kind: Java = node.kind_id().into();
            if !matches!(
                kind,
                Java::IfStatement | Java::WhileStatement | Java::DoStatement
            ) {
                return;
            }
            let ancestors = Ancestors::known(chain);
            let before = recording_gate_calls::observed();
            let owns = else_if::owns(node, source, ancestors);
            let during_owns = recording_gate_calls::observed() - before;
            let before = recording_gate_calls::observed();
            let leaf = else_if::pays_at_leaf(node, source, ancestors);
            let during_leaf = recording_gate_calls::observed() - before;
            let wrapper = else_if::pays_at_wrapper(node, source, ancestors);
            seen.push((kind, owns, leaf, wrapper, during_owns, during_leaf));
        });
        assert_eq!(
            seen,
            [
                // Y: the recording gate admits it; consulted by `owns`
                // (once) and never on the leaf path.
                (Java::IfStatement, true, false, true, 1, 0),
                // X: marked, ungated — owns and pays at both projections.
                (Java::WhileStatement, true, true, true, 0, 0),
                // Z: marked, gated away — nothing.
                (Java::DoStatement, false, false, false, 0, 0),
            ]
        );
        assert_eq!(
            (else_if::PAYERS, else_if::PAYS_NEVER),
            (&[] as &[u16], false)
        );
        assert!(!else_if::lists_kind(Java::ForStatement as u16));
        assert_eq!(
            else_if::canonical(),
            "ElseIf rows owners=[206,205,190] payers=[] wraps=[] excluded=[] mode=split \
             text_gated=false chain_gated=true"
        );
    }

    /// The `dsl_parent_gates` fixture: each owner of that table spelled
    /// under the parent its list names and away from it.
    #[cfg(feature = "java")]
    const PARENT_LIST_FIXTURE: &[u8] = b"class A {\n\
        A() { }\n\
        void m(boolean a) {\n\
        if (a) { } else if (a) { }\n\
        while (a) while (a) { }\n\
        do { } while (a);\n\
        if (a) do { } while (a);\n\
        Runnable r = () -> { };\n\
        run(() -> { });\n\
        }\n\
        class B { }\n\
        }\n\
        interface I { void g(); }\n";

    /// An owner's `under` / `not_under` list (#1794) in `owns`, each
    /// owner asked under the parent its list names and away from it, the
    /// parent recorded beside every answer. The parent kinds are tests,
    /// not claims, so no kind list names them.
    #[cfg(feature = "java")]
    #[test]
    fn a_parent_list_gates_an_owner_on_its_parents_kind() {
        use crate::JavaCode;
        use crate::test_support::for_each_node_with_chain;
        use dsl_parent_gates::{branch, container, r#loop};
        let source = PARENT_LIST_FIXTURE;
        let (mut branches, mut loops) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let a = Ancestors::known(chain);
            let parent = chain.last().map_or("", |p| p.kind());
            if node.kind_id() == Java::IfStatement as u16 {
                branches.push((parent, branch::owns(node, source, a)));
            }
            if node.kind_id() == Java::WhileStatement as u16 {
                loops.push((parent, r#loop::owns(node, source, a)));
            }
        });
        // `under [ClassBody, Block]`: the two `if`s in the block, not
        // the `else if` under its outer `if`.
        assert_eq!(
            branches,
            [("block", true), ("if_statement", false), ("block", true)]
        );
        // `not_under [ClassBody, Block]`: the inner `while` only.
        assert_eq!(loops, [("block", false), ("while_statement", true)]);
        assert_eq!(branch::OWNERS, &[Java::IfStatement as u16]);
        assert_eq!(container::OWNERS, &[Java::ClassDeclaration as u16]);
    }

    /// A node with no parent is under nothing (#1794): `under` rejects
    /// it and `not_under` admits it, whatever the list. Every owner in
    /// the fixture has a parent, so the test above cannot see this
    /// clause; an empty known chain is the walk's record of the root.
    #[cfg(feature = "java")]
    #[test]
    fn a_parent_list_reads_a_parentless_node_as_under_nothing() {
        use crate::JavaCode;
        use crate::test_support::for_each_node_with_chain;
        use dsl_parent_gates::{branch, r#loop};
        let source = PARENT_LIST_FIXTURE;
        let none = Ancestors::known(&[]);
        let (mut branches, mut loops) = (0, 0);
        for_each_node_with_chain::<JavaCode>(source, |node, _| {
            if node.kind_id() == Java::IfStatement as u16 {
                assert!(
                    !branch::owns(node, source, none),
                    "`under` admitted a parentless `if`"
                );
                branches += 1;
            }
            if node.kind_id() == Java::WhileStatement as u16 {
                assert!(
                    r#loop::owns(node, source, none),
                    "`not_under` rejected a parentless `while`"
                );
                loops += 1;
            }
        });
        assert_eq!((branches, loops), (3, 2));
    }

    /// A list beside `pays_itself` and `pays_itself_when` (#1794): the
    /// leaf pays where the list admits the owner and only there, and a
    /// `pays_itself_when` still asks its `when`.
    #[cfg(feature = "java")]
    #[test]
    fn a_parent_list_gates_a_self_paying_owner_at_the_leaf() {
        use crate::JavaCode;
        use crate::test_support::for_each_node_with_chain;
        use dsl_parent_gates::else_if;
        let source = PARENT_LIST_FIXTURE;
        let mut chained = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let a = Ancestors::known(chain);
            if matches!(
                node.kind_id().into(),
                Java::IfStatement | Java::WhileStatement | Java::DoStatement
            ) {
                chained.push((
                    node.kind(),
                    chain.last().map_or("", |p| p.kind()),
                    else_if::owns(node, source, a),
                    else_if::pays_at_leaf(node, source, a),
                ));
            }
        });
        // `pays_itself` beside `not_under`: the `else if` pays; the `do`
        // pays under `always` in the block, not under the `if`; the
        // `while` is admitted by its list and owns, and pays nowhere
        // under `never`.
        assert_eq!(
            chained,
            [
                ("if_statement", "block", false, false),
                ("if_statement", "if_statement", true, true),
                ("while_statement", "block", true, false),
                ("while_statement", "while_statement", false, false),
                ("do_statement", "block", true, true),
                ("if_statement", "block", false, false),
                ("do_statement", "if_statement", false, false),
            ]
        );
    }

    /// A list in `is_callable`, beside `callable` and `callable_when`,
    /// and in the `opens_space` fold, beside `opens` and `opens_by`
    /// (#1794).
    #[cfg(feature = "java")]
    #[test]
    fn a_parent_list_gates_callables_and_the_opens_space_fold() {
        use crate::test_support::for_each_node_with_chain;
        use crate::{JavaCode, SpaceKind};
        use dsl_parent_gates::{func_opener, opens_space};
        let source = PARENT_LIST_FIXTURE;
        let mut openers = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let a = Ancestors::known(chain);
            if matches!(
                node.kind_id().into(),
                Java::ClassDeclaration
                    | Java::ConstructorDeclaration
                    | Java::MethodDeclaration
                    | Java::LambdaExpression
            ) {
                openers.push((
                    node.kind(),
                    chain.last().map_or("", |p| p.kind()),
                    func_opener::owns(node, source, a),
                    func_opener::is_callable(node, source, a),
                    opens_space(node, source, a),
                ));
            }
        });
        // The constructor is admitted and never callable; the method is
        // callable in the class and nothing in the interface; the lambda
        // is nothing in its declarator and callable as an argument. The
        // top-level class opens through its `opens_by`, the nested one
        // not at all.
        assert_eq!(
            openers,
            [
                (
                    "class_declaration",
                    "program",
                    false,
                    false,
                    SpaceKind::Class
                ),
                (
                    "constructor_declaration",
                    "class_body",
                    true,
                    false,
                    SpaceKind::Function
                ),
                (
                    "method_declaration",
                    "class_body",
                    true,
                    true,
                    SpaceKind::Function
                ),
                (
                    "lambda_expression",
                    "variable_declarator",
                    false,
                    false,
                    SpaceKind::Unknown
                ),
                (
                    "lambda_expression",
                    "argument_list",
                    true,
                    true,
                    SpaceKind::Function
                ),
                (
                    "class_declaration",
                    "class_body",
                    false,
                    false,
                    SpaceKind::Unknown
                ),
                (
                    "method_declaration",
                    "interface_body",
                    false,
                    false,
                    SpaceKind::Unknown
                ),
            ]
        );
    }

    /// `canonical()` names the row's mode as its own field, so the
    /// Python reader is held to the mode and not only to the lists it
    /// prints (#1745): a split row with payers and one without both
    /// read `split`, a plain row `owners`, a `pays never` row `never`.
    /// The mode is the one encoding of the fact — the former
    /// `pays_never=` field was a projection of it that no gate read, and
    /// a fourth mode would have printed `false` on both sides while
    /// saying nothing. The two context flags follow it (#1793);
    /// `mode_of` reads the field between.
    #[test]
    fn canonical_names_each_paying_mode() {
        use dsl_gates::{comparison, else_if, guard, member};
        let mode_of = |row: String| {
            let (_, tail) = row
                .rsplit_once(" mode=")
                .expect("every declared row carries its mode");
            let (mode, _) = tail
                .split_once(' ')
                .expect("the context flags follow the mode");
            mode.to_owned()
        };
        assert_eq!(
            [
                mode_of(comparison::canonical()),
                mode_of(else_if::canonical()),
                mode_of(member::canonical()),
                mode_of(guard::canonical()),
            ],
            ["split", "split", "owners", "never"]
        );
        assert_eq!(
            member::canonical(),
            "Member rows owners=[249] payers=[] wraps=[] excluded=[] mode=owners \
             text_gated=true chain_gated=true"
        );
    }

    /// A table with one row and no fixture enumerates as exactly that
    /// row (#1700's first acceptance criterion, at the DSL): `rows()`
    /// lists what the invocation supplied and nothing else, in order,
    /// and a declared row's view carries the same items its module
    /// does — so the derived census, which reads only views, can ask
    /// about a row no fixture ever spelled. The three-row table pins
    /// declaration order against the `@named` order, which differs.
    #[test]
    fn rows_enumerate_what_the_invocation_declared_in_its_order() {
        use super::RowView;
        let one = gated_self_payer::rows();
        assert_eq!(one.len(), 1);
        let row = one[0].declared().expect("the one row is declared");
        assert_eq!(row.role, "Comparison");
        assert_eq!(row.owners, gated_self_payer::comparison::OWNERS);
        assert_eq!(row.payers, gated_self_payer::comparison::PAYERS);
        assert_eq!(row.leaf_kinds, gated_self_payer::comparison::LEAF_KINDS);
        assert_eq!(
            (row.mode, row.pays_never, row.text_gated),
            ("split", false, false)
        );
        assert_eq!(
            row.candidate_kinds().collect::<Vec<_>>(),
            [Java::InstanceofExpression as u16, Java::EQEQ as u16],
            "owners, then payers, then leaf kinds, each once"
        );
        assert!((row.lists_kind)(Java::InstanceofExpression as u16));
        assert!(!(row.lists_kind)(Java::EQEQ as u16));

        let four: Vec<&str> = spliced::rows().iter().map(RowView::role).collect();
        assert_eq!(
            four,
            ["FuncOpener", "Container", "Comparison", "Guard"],
            "declaration order"
        );
        let pending = spliced::rows().into_iter().nth(3).expect("four rows");
        assert!(matches!(
            &pending,
            RowView::Pending {
                role: "Guard",
                package: "R99"
            }
        ));
        assert!(pending.declared().is_none());
        assert_eq!(spliced::guard::canonical(), "Guard pending R99");

        let java = super::java::rows();
        let absent = java
            .iter()
            .find(|r| r.role() == "NullSafe")
            .expect("Java declares NullSafe absent");
        assert!(
            matches!(absent, RowView::Absent { why, .. } if *why == super::java::null_safe::ABSENT)
        );
        assert!(absent.declared().is_none());
    }

    /// `TEXT_GATED` is true exactly when an owner is `text_gated_by`:
    /// not for a chain-reading `gated_by`, a `holds_payer`, a
    /// `callable_when` or an `opens_by`, which read no bytes, nor for a
    /// kind-only row however marked, nor for an absent row. The
    /// byte-less `Checker` spellings pin the rows they read against it.
    /// The rows are declarations, built in every feature set.
    #[test]
    fn text_gated_is_true_exactly_for_rows_whose_owners_read_bytes() {
        use crate::roles::{cpp, csharp, elixir, groovy, java, javascript, kotlin, ruby};
        let rows: &[(&str, bool, bool)] = &[
            (
                "elixir FuncOpener `text_gated_by method_macro`",
                elixir::func_opener::TEXT_GATED,
                true,
            ),
            (
                "elixir Member `text_gated_by struct_macro`",
                elixir::member::TEXT_GATED,
                true,
            ),
            (
                "groovy Primitive `text_gated_by spells_primitive_type`",
                groovy::primitive::TEXT_GATED,
                true,
            ),
            (
                "groovy DeclKeyword `text_gated_by spells_var`",
                groovy::decl_keyword::TEXT_GATED,
                true,
            ),
            (
                "csharp Modifier `text_gated_by aliases_a_modifier_keyword`",
                csharp::modifier::TEXT_GATED,
                true,
            ),
            (
                "java FuncOpener, kind-only owners `callable` and `opens`",
                java::func_opener::TEXT_GATED,
                false,
            ),
            ("java NullSafe, absent", java::null_safe::TEXT_GATED, false),
            (
                "java Container `gated_by has_class_body` reads the node",
                java::container::TEXT_GATED,
                false,
            ),
            (
                "java Comparison `holds_payer` reads the chain",
                java::comparison::TEXT_GATED,
                false,
            ),
            (
                "ruby FuncOpener `gated_by not_a_lambda_body` reads the chain",
                ruby::func_opener::TEXT_GATED,
                false,
            ),
            (
                "javascript FuncOpener `callable_when` reads the chain",
                javascript::func_opener::TEXT_GATED,
                false,
            ),
            (
                "kotlin Container `opens_by` reads the node",
                kotlin::container::TEXT_GATED,
                false,
            ),
            (
                "cpp Container `gated_by has_body` reads the node",
                cpp::container::TEXT_GATED,
                false,
            ),
        ];
        for (row, text_gated, want) in rows {
            assert_eq!(text_gated, want, "{row}");
        }
        // The word in every position the attribute list can put it: the
        // scan reads it before the fold merges it with `gated_by`, so an
        // order the fold consumes first — after `holds_payer`, after an
        // `opens` and a `callable` — must still be seen.
        let synthetic: &[(&str, bool)] = &[
            ("text_gated_by alone", dsl_gates::member::TEXT_GATED),
            ("after holds_payer", dsl_gates::call::TEXT_GATED),
            (
                "after opens and callable",
                dsl_gates::func_opener::TEXT_GATED,
            ),
            ("gated_by, not text", dsl_gates::short_circuit::TEXT_GATED),
        ];
        for (order, text_gated) in synthetic {
            assert_eq!(*text_gated, *order != "gated_by, not text", "{order}");
        }
        // `no_chain` leaves the byte axis alone: a marked text gate is
        // still text-gated, a marked plain gate still is not. A parent
        // list reads no bytes (#1794).
        let marked: &[(&str, bool, bool)] = &[
            ("under, alone", dsl_parent_gates::branch::TEXT_GATED, false),
            (
                "text_gated_by … no_chain",
                dsl_no_chain::catch::TEXT_GATED,
                true,
            ),
            (
                "holds_payer text_gated_by … no_chain",
                dsl_no_chain::member::TEXT_GATED,
                true,
            ),
            ("gated_by … no_chain", dsl_no_chain::exit::TEXT_GATED, false),
        ];
        for (order, got, want) in marked {
            assert_eq!(got, want, "{order}");
        }
    }

    /// The four text × chain combinations, each pinned on a production
    /// row that spells it (#1793): `TEXT_GATED` comes from the keyword
    /// and `CHAIN_GATED` from the absence of `no_chain`, so the two are
    /// independent. The JS `FuncOpener` row is the `callable_when`
    /// case: it reads the chain and is outside `CHAIN_GATED`.
    #[test]
    fn text_and_chain_flags_are_independent_on_the_production_rows() {
        use crate::roles::{groovy, java, javascript, kotlin, python, ruby};
        type Flags = (bool, bool);
        let rows: &[(&str, Flags, Flags)] = &[
            (
                "java DocComment `text_gated_by opens_with_javadoc_marker no_chain`: bytes, no chain",
                (
                    java::doc_comment::TEXT_GATED,
                    java::doc_comment::CHAIN_GATED,
                ),
                (true, false),
            ),
            (
                "groovy DeclKeyword `text_gated_by spells_var`: bytes and chain",
                (
                    groovy::decl_keyword::TEXT_GATED,
                    groovy::decl_keyword::CHAIN_GATED,
                ),
                (true, true),
            ),
            (
                "java Container `gated_by has_class_body no_chain`: neither",
                (java::container::TEXT_GATED, java::container::CHAIN_GATED),
                (false, false),
            ),
            (
                "ruby FuncOpener `gated_by not_a_lambda_body`: chain, no bytes",
                (
                    ruby::func_opener::TEXT_GATED,
                    ruby::func_opener::CHAIN_GATED,
                ),
                (false, true),
            ),
            (
                "kotlin Container `opens_by class_or_interface no_chain`",
                (
                    kotlin::container::TEXT_GATED,
                    kotlin::container::CHAIN_GATED,
                ),
                (false, false),
            ),
            (
                "javascript FuncOpener: `callable_when` reads the chain and is not covered",
                (
                    javascript::func_opener::TEXT_GATED,
                    javascript::func_opener::CHAIN_GATED,
                ),
                (false, false),
            ),
            (
                "java NullSafe, absent",
                (java::null_safe::TEXT_GATED, java::null_safe::CHAIN_GATED),
                (false, false),
            ),
            (
                "python Guard `under [CaseClause]`, its only owner: chain, no bytes",
                (python::guard::TEXT_GATED, python::guard::CHAIN_GATED),
                (false, true),
            ),
            (
                "java DeclKeyword `not_under [ClassLiteral]`, its only gate: chain, no bytes",
                (
                    java::decl_keyword::TEXT_GATED,
                    java::decl_keyword::CHAIN_GATED,
                ),
                (false, true),
            ),
            (
                "java Exit, kind-only",
                (java::exit::TEXT_GATED, java::exit::CHAIN_GATED),
                (false, false),
            ),
        ];
        for (row, got, want) in rows {
            assert_eq!(got, want, "{row}");
        }
    }

    /// `CHAIN_GATED` is true exactly when an owner's `gated_by` /
    /// `text_gated_by` gate or `opens_by` fn lacks `no_chain` (#1793):
    /// false for every marked shape in every attribute position, false
    /// for a kind-only or `callable_when`-only row, and — the two-fn
    /// owner — an OR over the owner's gate and `opens_by`, each marked
    /// on its own, so the unmarked fn decides whichever is written
    /// first. Read on the synthetic tables that spell each shape.
    #[test]
    fn chain_gated_is_true_exactly_for_rows_whose_owner_fns_take_the_chain() {
        let synthetic: &[(&str, bool, bool)] = &[
            (
                "gated_by alone, and before pays_itself",
                dsl_no_chain::exit::CHAIN_GATED,
                false,
            ),
            (
                "text_gated_by after opens and callable",
                dsl_no_chain::func_opener::CHAIN_GATED,
                false,
            ),
            (
                "holds_payer text_gated_by",
                dsl_no_chain::member::CHAIN_GATED,
                false,
            ),
            (
                "text_gated_by alone",
                dsl_no_chain::catch::CHAIN_GATED,
                false,
            ),
            (
                "both fns marked, both orders",
                dsl_no_chain::container::CHAIN_GATED,
                false,
            ),
            (
                "holds_payer gated_by, unmarked",
                dsl_no_chain::r#loop::CHAIN_GATED,
                true,
            ),
            (
                "text_gated_by, unmarked",
                dsl_no_chain::branch::CHAIN_GATED,
                true,
            ),
            (
                "opens_by unmarked, written first",
                dsl_two_fn_gate_marked::container::CHAIN_GATED,
                true,
            ),
            (
                "opens_by unmarked, written second",
                dsl_two_fn_gate_marked::func_opener::CHAIN_GATED,
                true,
            ),
            (
                "gate unmarked, written second",
                dsl_two_fn_opens_by_marked::container::CHAIN_GATED,
                true,
            ),
            (
                "gate unmarked, written first",
                dsl_two_fn_opens_by_marked::func_opener::CHAIN_GATED,
                true,
            ),
            (
                "gated_by unmarked on the proof table",
                dsl_gates::ternary::CHAIN_GATED,
                true,
            ),
            (
                "opens_by unmarked on the proof table",
                dsl_gates::container::CHAIN_GATED,
                true,
            ),
            (
                "callable_when and a gated callable: the gate, not the when",
                dsl_gates::func_opener::CHAIN_GATED,
                true,
            ),
            ("kind-only owners", dsl_gates::guard::CHAIN_GATED, false),
            // A parent list, the row's only chain reader (#1794).
            ("under alone", dsl_parent_gates::branch::CHAIN_GATED, true),
            (
                "not_under alone",
                dsl_parent_gates::r#loop::CHAIN_GATED,
                true,
            ),
            (
                "a list beside pays_itself and pays_itself_when",
                dsl_parent_gates::else_if::CHAIN_GATED,
                true,
            ),
            (
                "a list beside callable and callable_when",
                dsl_parent_gates::func_opener::CHAIN_GATED,
                true,
            ),
            (
                "a list beside a no_chain opens_by",
                dsl_parent_gates::container::CHAIN_GATED,
                true,
            ),
        ];
        for (order, got, want) in synthetic {
            assert_eq!(got, want, "{order}");
        }
    }

    /// `callable_when` is outside `CHAIN_GATED` on purpose: the JS
    /// family's `FuncOpener` rows carry a chain-reading
    /// `callable_when bound_function` beside an `opens_space` fold that
    /// never calls it, and the byte-less space forward of each asserts
    /// `!CHAIN_GATED` on that row. Pinned on the production row, so a
    /// future fold of `callable_when` into the flag fails here and not
    /// as a compile error in four `Checker` impls.
    #[test]
    fn callable_when_is_outside_chain_gated() {
        use crate::roles::{javascript, mozjs, tsx, typescript};
        for (lang, chain_gated, callable) in [
            (
                "javascript",
                javascript::func_opener::CHAIN_GATED,
                javascript::func_opener::CALLABLE.len(),
            ),
            (
                "mozjs",
                mozjs::func_opener::CHAIN_GATED,
                mozjs::func_opener::CALLABLE.len(),
            ),
            (
                "tsx",
                tsx::func_opener::CHAIN_GATED,
                tsx::func_opener::CALLABLE.len(),
            ),
            (
                "typescript",
                typescript::func_opener::CHAIN_GATED,
                typescript::func_opener::CALLABLE.len(),
            ),
        ] {
            assert!(
                callable >= 6,
                "{lang}: the `callable_when` owners are declared"
            );
            assert!(
                !chain_gated,
                "{lang}: `callable_when` is not a chain gate of the row"
            );
        }
    }

    // `callable` on an owner: `is_callable` answers for the marked owners
    // only, under their gates, and `owns` / `opens_space` are unchanged
    // by the mark — the static initializer opens a space and is not
    // callable, the gated constructor is marked and gated away. A
    // `callable_when w` owner opens its space regardless and is callable
    // only where `w` admits it: the lambda never, the compact
    // constructor always.
    #[cfg(feature = "java")]
    #[test]
    fn a_callable_owner_is_told_from_an_opener_that_merely_opens() {
        use crate::JavaCode;
        use crate::SpaceKind;
        use crate::test_support::for_each_node_with_chain;
        use dsl_gates::{func_opener, opens_space};
        let source = b"record R(int a) { R { } }\nclass A { A() { } void m() { Runnable r = () -> { }; } static { } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let owns = func_opener::owns(node, source, ancestors);
            let callable = func_opener::is_callable(node, source, ancestors);
            let space = opens_space(node, source, ancestors);
            if owns || callable || space != SpaceKind::Unknown {
                seen.push((node.kind(), owns, callable, space));
            }
        });
        // The constructor is declared (six owners) and gated away on
        // every predicate, so it is absent here rather than listed. The
        // class is the table's `Container` row (`opens_by`), not an
        // opener: it labels and neither owns nor is callable here. The
        // record is the `TEXT_GATED` position probe under `always`.
        assert_eq!(
            seen,
            [
                ("record_declaration", true, true, SpaceKind::Function),
                (
                    "compact_constructor_declaration",
                    true,
                    true,
                    SpaceKind::Function
                ),
                ("class_declaration", false, false, SpaceKind::Class),
                ("method_declaration", true, true, SpaceKind::Function),
                ("lambda_expression", true, false, SpaceKind::Function),
                ("static_initializer", true, false, SpaceKind::Function),
            ]
        );
        assert_eq!(func_opener::OWNERS.len(), 6);
        // The declared list, gates aside: the gated-away constructor and
        // the never-callable lambda are both in it.
        assert_eq!(
            func_opener::CALLABLE,
            &[
                Java::MethodDeclaration as u16,
                Java::ConstructorDeclaration as u16,
                Java::LambdaExpression as u16,
                Java::CompactConstructorDeclaration as u16,
                Java::RecordDeclaration as u16,
            ]
        );
        assert_eq!(dsl_gates::guard::CALLABLE, &[] as &[u16]);
    }

    /// `HOLDS_PAYER` names every `holds_payer` owner in each spelling
    /// the attribute list can carry it — bare, beside `gated_by`,
    /// beside `text_gated_by` — and none that lacks the mark. Read
    /// below the fold, which keeps the mark in the gate slot, unlike
    /// `TEXT_GATED`; pinned so a scan-order change cannot empty it,
    /// which would satisfy the derived projection invariant vacuously.
    #[test]
    fn holds_payer_lists_the_marked_owners_in_every_spelling() {
        assert_eq!(
            dsl_gates::comparison::HOLDS_PAYER,
            &[Java::BinaryExpression as u16],
            "bare"
        );
        assert_eq!(
            dsl_gates::short_circuit::HOLDS_PAYER,
            &[Java::BinaryExpression as u16],
            "beside gated_by"
        );
        assert_eq!(
            dsl_gates::call::HOLDS_PAYER,
            &[Java::MethodInvocation as u16],
            "beside text_gated_by"
        );
        assert_eq!(
            dsl_gates::ternary::HOLDS_PAYER,
            &[] as &[u16],
            "unmarked owners"
        );
    }

    // `opens_by f`: one kind, two space kinds, by what `f` reads.
    #[cfg(feature = "java")]
    #[test]
    fn an_opens_by_owner_opens_what_its_fn_answers() {
        use crate::JavaCode;
        use crate::SpaceKind;
        use crate::test_support::for_each_node_with_chain;
        use dsl_gates::opens_space;
        let source = b"class I { }\nclass A { }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let kind = opens_space(node, source, Ancestors::known(chain));
            if kind != SpaceKind::Unknown {
                seen.push(kind);
            }
        });
        assert_eq!(seen, [SpaceKind::Interface, SpaceKind::Class]);
    }

    // `pays never`: the owner is recognised and pays under neither
    // projection, which is the whole of what the shape says.
    #[cfg(feature = "java")]
    #[test]
    fn a_pays_never_row_owns_and_pays_nowhere() {
        use crate::JavaCode;
        use crate::test_support::for_each_node_with_chain;
        use dsl_gates::guard;
        let source = b"class A { int f(Object o) { return switch (o) { case Integer i when i > 0 -> 1; default -> 0; }; } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            if node.kind_id() == Java::Guard as u16 {
                seen.push((
                    guard::owns(node, source, ancestors),
                    guard::pays_at_leaf(node, source, ancestors),
                    guard::pays_at_wrapper(node, source, ancestors),
                    guard::pays_at_token_kind(node.kind_id(), node, source, ancestors),
                ));
            }
        });
        assert_eq!(seen, [(true, false, false, false)]);
        assert_eq!(
            guard::canonical(),
            "Guard rows owners=[184] payers=[] wraps=[] excluded=[] mode=never \
             text_gated=false chain_gated=false"
        );
    }
}
