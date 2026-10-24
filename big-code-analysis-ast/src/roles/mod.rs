//! One declaration per language of every semantic fact the classifiers
//! and the metric dispatchers ask of a node (#1690).
//!
//! A *role* is a question a consumer puts to a node — is it a call, a
//! string literal, a comparison, something that opens a space. A *row*
//! is one language's answer: which kinds *own* the construct (the node a
//! slot predicate receives), which token kinds *pay* for it (the leaf a
//! counting arm fires on), and how to get from one to the other. The
//! [`role_table!`](crate::role_table) macro turns a table of rows into the predicates the
//! `Checker`, `Getter` and root-crate consumers will read instead of
//! restating the kind lists themselves. This crate computes no metric:
//! a row says what a construct *is*; which occurrence a metric counts
//! is a policy decision in `big-code-analysis` (contract 7 of #1690).
//!
//! # The role set
//!
//! Fixed here because the census (#1692) iterates it and a role added
//! later reopens every table. A meaning names the existing `Checker` /
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
//! | `Member` | a field, property or attribute declared directly in a `Container` |
//! | `Comment` | any comment |
//! | `DocComment` | a comment a documentation tool reads |
//! | `Docstring` | a string literal that serves as documentation |
//! | `DocTarget` | a declaration a doc comment attaches to |
//! | `DocRunOpener` | a comment whose own content makes its run documentation (see below) |
//! | `DocRunDirective` | a member of a doc run that is *not* documentation (see below) |
//! | `HoldsDocInBody` | an ancestor whose body is executable-code context for a doc comment |
//!
//! Four of those need more than a line, because a short reading of the
//! word files different kinds from the contract it names:
//!
//! - `Exit` is what the exit count reads today: `return` and `throw`,
//!   a generator's `yield` (control leaves the body, resumably) and a
//!   `switch` expression's `yield` (control leaves the arm). An implicit
//!   tail expression is not one.
//! - `DocRunOpener` is `Checker::is_doc_run_opener`: a run that is
//!   documentation with no declaration under it — an LDoc `---` block,
//!   RDoc `##`, a YARD `@!` directive — not merely the first comment of
//!   a run.
//! - `DocRunDirective` is `Checker::is_doc_run_directive`: a member of a
//!   doc run that is not documentation text and is not billed as such,
//!   such as `//go:generate` or a Ruby shebang or magic comment. It is
//!   not a markup directive like `@param`.
//! - `HoldsDocInBody` is `Checker::holds_doc_comment_in_body`: an
//!   ancestor whose body is executable-code context, in which a comment
//!   is documentation only where the language's tool reads one there.
//!   A function or closure by default; Java adds `static { … }`, a bare
//!   `block` and a `switch_block` — the last reachable from a field
//!   initializer, outside any function. A class body is not one.
//!
//! # The row grammar
//!
//! ```text
//! role_table! {
//!     lang: Java;
//!     Exit       { owns [ReturnStatement, ThrowStatement] }
//!     Comparison { owns [BinaryExpression(gated_by comparison_operator),
//!                        InstanceofExpression(pays_itself)]
//!                  pays [EQEQ, LT gated_by under_binary_expression]
//!                  project "operator"
//!                  excludes [QMARK "a wildcard bound's `?` is type syntax"] }
//!     Container  { owns [ClassDeclaration(opens Class),
//!                        ObjectCreationExpression(opens Class gated_by has_class_body)] }
//!     Guard      { owns [Guard] pays never }
//!     NullSafe   { absent "no `?.` or `??`" }
//!     Branch     { pending R07 }
//! }
//! ```
//!
//! The doctest on [`role_table!`](crate::role_table) compiles the same shapes, so the
//! grammar shown here cannot drift from what the macro accepts without
//! a reviewer noticing both.
//!
//! An owner's attributes go in parentheses after its kind, in any
//! order: `gated_by f` makes the kind own the construct only when
//! `f(node, code, ancestors)` says so; `text_gated_by f` is the same
//! with a name that says the gate reads source bytes; `pays_itself`
//! marks a kind the grammar gives no token for, so the owner is also the
//! payer (`InstanceofExpression`); `opens K` records the `SpaceKind` the
//! owner opens. A payer takes `gated_by f` bare, after its kind. Per
//! row: `pays [..]` splits the paying occurrences from the owners (see
//! the projections below); `pays never` says the construct has owners
//! and no paying node at all; neither means the owners are the paying
//! nodes. `project "field"` names the child field holding an owner's
//! paying token; `wraps [..]` lists the interpolation kinds a
//! `StringLit` can hold; `excludes [K "why"]` records a kind the row's
//! author considered and rejected, for the coverage ratchet. Row
//! attributes are written in that order — `pays`, `project`, `wraps`,
//! `excludes` — and owner attributes in any. `absent "why"` is a
//! language with no such construct, and `pending Rnn` a row a later
//! package fills in; the census treats both as declared.
//!
//! # What a table emits
//!
//! For each row with owners, in a module named after the role in
//! `snake_case` (`comparison`, `func_opener`):
//!
//! - `OWNERS`, `PAYERS`, `EXCLUDED`, `WRAPS`: the kinds the row names,
//!   verbatim, for the coverage ratchet. `OWNERS` and `PAYERS` are the
//!   *declared* lists and are disjoint by construction (see below).
//! - `owns(node, code, ancestors)`: the gated owner test.
//! - `pays_at_leaf(node, code, ancestors)`: with a `pays` list, the gated
//!   payer test plus every `pays_itself` owner — where a token-counting
//!   arm fires; without one, the owner test; under `pays never`, false.
//! - `owns_kind` / `pays_at_leaf_kind`: the same two with the node's
//!   kind id passed in, for a consumer that asks several rows about one
//!   node — `kind_id()` is an FFI call per read, and a classifier on
//!   the walk's hot path reading three rows paid six of them (#1693).
//! - `pays_at_wrapper(node, code, ancestors)`: the owner test, for a
//!   consumer that pays at the owning node; false under `pays never`.
//!   A row with no `pays` list answers both projections identically, so
//!   the split is visible only where a row declares it, and a consumer
//!   counting both an owner and its leaf is a visible choice at the
//!   consumer, never an accident here.
//! - `project(owner) -> Option<Node>`: the owner's paying child, when a
//!   `project` field is declared.
//!
//! And once per table: `opens_space(node, code, ancestors) -> SpaceKind`,
//! folded over every `opens` entry on every row, so the kinds a
//! `FuncOpener` or `Container` row names are not restated.
//!
//! Every emitted predicate takes `(node, code, ancestors)`. There is no
//! byte-less spelling: the legacy `Checker::is_*` forms are hand-written
//! wrappers in `checker.rs`, and a text-gated row cannot be answered
//! without the bytes (contract 4 of #1690).
//!
//! Everything a table emits is `pub`: the root crate's metric
//! dispatchers are consumers too (`abc`, `cyclomatic`, `cognitive` read
//! the `Comparison` and `ShortCircuit` rows since #1694), and they reach
//! a row as `big_code_analysis_ast::roles::<lang>::<role>`. The AST
//! crate is outside `STABILITY.md`, so this widens no published
//! contract.
//!
//! An `owns` or `pays` list may splice a shared kind fragment with
//! `..name!()` (`roles/c_family.rs`, `roles/js_family.rs`); the
//! fragment note on the macro below has the shape.
//!
//! What a row must not say is which consumer reads which projection, or
//! why: that is policy, and it lives with the consumer in
//! `big-code-analysis`. A row's comment describes the grammar — what
//! the construct's nodes are and how they nest — and nothing else.
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
//!   second gate, `pays_itself` or `opens` on one owner matches no scan
//!   rule. All are pinned by `compile_fail` doctests on the macro.
//! - **Not expressible, by design:** a kind claimed by the *wrong* role,
//!   an incomplete entry, and whether a consumer reads the right
//!   projection. Those are what the census, the parity suites and review
//!   are for (#1690, "What this does and does not claim").
//!
//! The first consumer is `Getter::get_keyword_role` (#1693), which
//! reads the `Modifier`, `DeclKeyword` and `Primitive` rows through
//! `pays_at_leaf`; every other emitted item is read by nothing until
//! its package lands, and the module-level allow below says so.

// Only the three keyword rows have a consumer (#1693). The rest of what
// a table emits — `opens_space`, `pays_at_wrapper`, `project`, the
// `ABSENT` / `PENDING` constants — waits on R05 through R09, which
// narrow this allow as they land.
#![allow(dead_code)]

pub mod bash;
pub mod c;
pub mod c_family;
pub mod ccomment;
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
pub mod tsx;
pub mod typescript;

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
/// A second gate, `pays_itself` or `opens` on one owner finds its slot
/// in the attribute scan already filled and matches no rule:
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
/// The valid shapes compile:
///
/// ```
/// # #[macro_use] extern crate big_code_analysis_ast;
/// # use big_code_analysis_ast::{Ancestors, Java, Node, SpaceKind};
/// fn always(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
/// role_table! {
///     lang: Java;
///     Exit { owns [ReturnStatement, ThrowStatement] }
///     Comparison {
///         owns [BinaryExpression(gated_by always), InstanceofExpression(pays_itself)]
///         pays [EQEQ, LT gated_by always]
///         project "operator"
///         excludes [QMARK "type syntax"]
///     }
///     Container { owns [ClassDeclaration(opens Class), Program(opens Unit)] }
///     Guard { owns [Guard] pays never }
///     NullSafe { absent "no `?.`" }
///     Branch { pending R07 }
/// }
/// # fn main() {
/// assert_eq!(exit::OWNERS.len(), 2);
/// assert_eq!(comparison::PAYERS, &[Java::EQEQ as u16, Java::LT as u16]);
/// assert_eq!((exit::PAYS_NEVER, guard::PAYS_NEVER), (false, true));
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
        pub fn canonical() -> String {
            let mut out = format!("lang {}\n", stringify!($lang));
            $( out.push_str(&$crate::role_table!(@canonical $role)); out.push('\n'); )*
            out
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
            $crate::role_table!(@absent_predicate owns);
            $crate::role_table!(@absent_predicate pays_at_leaf);
            $crate::role_table!(@absent_predicate pays_at_wrapper);
            $crate::role_table!(@absent_predicate_kind owns_kind);
            $crate::role_table!(@absent_predicate_kind pays_at_leaf_kind);
            /// This row in the canonical form `utils/role_tables.py` emits.
            #[cfg(test)]
            pub fn canonical() -> String {
                format!("{} absent {}", stringify!($role), ABSENT)
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
            pub fn canonical() -> String {
                format!("{} pending {}", stringify!($role), PENDING)
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
            @scan opens_done [] [$l $k $n $c $a] $owner [] [] [] $($($owner_attr)*)?
        ); )*
    };
    (@row_body $lang:ident $role:ident [$mode:ident] {
        owns [ $($owner:ident $(( $($owner_attr:tt)* ))?),* $(,)? ]
        pays [ $($payer:ident $(gated_by $payer_gate:ident)?),* $(,)? ]
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
            // Whether the owners themselves are paying nodes: yes on a
            // plain row, no once a `pays` list splits the token off, no
            // under `pays never`.
            const OWNERS_PAY: bool = $crate::role_table!(@mode $mode owners);

            /// This row in the canonical form `utils/role_tables.py` emits:
            /// kinds as ids, so the two sides compare without naming
            /// variants. The Python reader is a second implementation of
            /// the DSL; the differential test in `roles/mod.rs` holds it to
            /// this.
            #[cfg(test)]
            pub fn canonical() -> String {
                let ids = |kinds: &[u16]| {
                    kinds.iter().map(u16::to_string).collect::<Vec<_>>().join(",")
                };
                let excluded = EXCLUDED
                    .iter()
                    .map(|(k, why)| format!("{k}={why}"))
                    .collect::<Vec<_>>()
                    .join(";");
                format!(
                    "{} rows owners=[{}] payers=[{}] wraps=[{}] excluded=[{}] pays_never={}",
                    stringify!($role),
                    ids(OWNERS),
                    ids(PAYERS),
                    ids(WRAPS),
                    excluded,
                    PAYS_NEVER
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

            /// Whether `node` owns the construct.
            #[inline]
            pub fn owns<'a>(
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                owns_kind(node.kind_id(), node, code, ancestors)
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
                        [] [] [] $($($owner_attr)*)?
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
                    @clause [$lang kind node code ancestors] $payer $(gated_by $payer_gate)?
                ); )*
                $( $crate::role_table!(
                    @scan self_payer_done [$mode] [$lang kind node code ancestors] $owner
                        [] [] [] $($($owner_attr)*)?
                ); )*
                OWNERS_PAY && owns_kind(kind, node, code, ancestors)
            }

            /// Whether `node` is the owning node, for a consumer that
            /// pays at the wrapper rather than at its token. Equal to
            /// `owns` unless the row says `pays never`.
            pub fn pays_at_wrapper<'a>(
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> bool {
                !PAYS_NEVER && owns(node, code, ancestors)
            }

            $(
                /// The child of an owner that pays for it.
                pub fn project<'a>(owner: &Node<'a>) -> Option<Node<'a>> {
                    owner.child_by_field_name($field)
                }
            )?
        });
    };

    // ---- the three paying modes --------------------------------------------
    //
    // `@mode <row's mode> <asked mode>`: whether the two are the same.

    (@mode never never) => { true };
    (@mode owners owners) => { true };
    (@mode $have:ident $want:ident) => { false };

    // ---- the owner-attribute scan -----------------------------------------
    //
    // One left-to-right walk over an owner's attributes, normalising them
    // into three slots — `[gate]`, `[pays_itself]`, `[space kind]` — and
    // then handing the slots to the terminal named by `$done`. A second
    // gate, a second `pays_itself` or a second `opens` finds its slot
    // already filled and matches no rule, so it fails to expand.

    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt $p:tt $s:tt) => {
        $crate::role_table!(@$d $m $ctx $k $g $p $s);
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt gated_by $g:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [$g] $p $s $($r)*);
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident [] $p:tt $s:tt text_gated_by $g:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k [$g] $p $s $($r)*);
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt [] $s:tt pays_itself $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k $g [pays_itself] $s $($r)*);
    };
    (@scan $d:ident $m:tt $ctx:tt $k:ident $g:tt $p:tt [] opens $s:ident $($r:tt)*) => {
        $crate::role_table!(@scan $d $m $ctx $k $g $p [$s] $($r)*);
    };

    // ---- the scan's terminals ----------------------------------------------

    // In `owns`: the owner's clause, gated or not.
    (@owner_done [] $ctx:tt $k:ident [$($g:ident)?] $p:tt $s:tt) => {
        $crate::role_table!(@clause $ctx $k $(gated_by $g)?);
    };
    // In `pays_at_leaf`: a `pays_itself` owner pays under its own gate.
    // The attribute is only meaningful on a split row — on a plain row
    // every owner already pays, and under `pays never` it would
    // contradict the row — so the other two modes reject it.
    (@self_payer_done [$mode:ident] $ctx:tt $k:ident $g:tt [] $s:tt) => {};
    (@self_payer_done [split] $ctx:tt $k:ident [$($g:ident)?] [pays_itself] $s:tt) => {
        $crate::role_table!(@clause $ctx $k $(gated_by $g)?);
    };
    (@self_payer_done [never] $ctx:tt $k:ident $g:tt [pays_itself] $s:tt) => {
        compile_error!("`pays_itself` contradicts `pays never`: the row says nothing pays");
    };
    (@self_payer_done [owners] $ctx:tt $k:ident $g:tt [pays_itself] $s:tt) => {
        compile_error!(
            "`pays_itself` is redundant without a `pays` list: every owner already pays"
        );
    };
    // In `opens_space`: an `opens K` owner returns its kind, under its gate.
    (@opens_done [] $ctx:tt $k:ident $g:tt $p:tt []) => {};
    (@opens_done [] $ctx:tt $k:ident [$($g:ident)?] $p:tt [$s:ident]) => {
        $crate::role_table!(@space_clause $ctx $k $s $(gated_by $g)?);
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

    (@clause [$l:ident $kind:ident $n:ident $c:ident $a:ident] $k:ident) => {
        if $kind == $l::$k as u16 {
            return true;
        }
    };
    (@clause [$l:ident $kind:ident $n:ident $c:ident $a:ident] $k:ident gated_by $g:ident) => {
        if $kind == $l::$k as u16 {
            return $g($n, $c, $a);
        }
    };
    (@space_clause [$l:ident $kind:ident $n:ident $c:ident $a:ident] $k:ident $s:ident) => {
        if $kind == $l::$k as u16 {
            return $crate::SpaceKind::$s;
        }
    };
    (@space_clause
        [$l:ident $kind:ident $n:ident $c:ident $a:ident] $k:ident $s:ident gated_by $g:ident
    ) => {
        if $kind == $l::$k as u16 && $g($n, $c, $a) {
            return $crate::SpaceKind::$s;
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
    (@canonical HoldsDocInBody) => { holds_doc_in_body::canonical() };

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
    (@named HoldsDocInBody { $($b:tt)* }) => {
        /// This language's `HoldsDocInBody` row.
        pub mod holds_doc_in_body { $($b)* }
    };
}

#[cfg(test)]
mod tests {
    use crate::{Ancestors, Java, Node};

    // `utils/role_tables.py` reads the declaration text for the three
    // role gates, so it is a second implementation of this macro's
    // meaning. This holds it to the compiled constants: every table's
    // `canonical()` must equal what `--dump` prints for the same file.
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
    // hand.
    type Canonical = fn() -> String;

    #[cfg(not(windows))]
    #[test]
    fn python_reader_agrees_with_the_compiled_tables() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the ast crate sits under the workspace root");
        let tables: &[(&str, Canonical)] = &[
            ("bash", super::bash::canonical),
            ("c", super::c::canonical),
            ("ccomment", super::ccomment::canonical),
            ("cpp", super::cpp::canonical),
            ("csharp", super::csharp::canonical),
            ("elixir", super::elixir::canonical),
            ("go", super::go::canonical),
            ("groovy", super::groovy::canonical),
            ("irules", super::irules::canonical),
            ("java", super::java::canonical),
            ("javascript", super::javascript::canonical),
            ("kotlin", super::kotlin::canonical),
            ("lua", super::lua::canonical),
            ("mozcpp", super::mozcpp::canonical),
            ("mozjs", super::mozjs::canonical),
            ("objc", super::objc::canonical),
            ("perl", super::perl::canonical),
            ("php", super::php::canonical),
            ("preproc", super::preproc::canonical),
            ("python", super::python::canonical),
            ("ruby", super::ruby::canonical),
            ("rust", super::rust::canonical),
            ("tcl", super::tcl::canonical),
            ("tsx", super::tsx::canonical),
            ("typescript", super::typescript::canonical),
        ];
        let listed: std::collections::BTreeSet<String> =
            tables.iter().map(|(s, _)| (*s).to_owned()).collect();
        let known: std::collections::BTreeSet<String> = crate::LANG::into_enum_iter()
            .map(|l| format!("{l:?}").to_lowercase())
            .collect();
        pretty_assertions::assert_eq!(listed, known, "every LANG has a table listed here");
        for (stem, canonical) in tables {
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

    // A table no language declares, to reach the one scan terminal the
    // three proof tables do not: a *gated* `pays_itself` owner. Without
    // this, dropping the gate from `@self_payer_done` failed nothing.
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

    // A fragment splices its kinds into the row in list order, before
    // the dialect's own entries, in both the row and the `opens_space`
    // fold; the dialect's attributes stay on its own entries.
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
            Exit { owns [..jvm_openers!()] pays [ReturnStatement] }
            Comparison { owns [BinaryExpression] pays [..equality_tokens!(), LT gated_by never] }
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
            asked += 1;
        });
        assert!(asked > 1);
        assert_eq!(primitive::ABSENT, "JavaScript has no type names");
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
            spliced::exit::OWNERS,
            &[
                Java::MethodDeclaration as u16,
                Java::ConstructorDeclaration as u16
            ]
        );
        assert_eq!(spliced::exit::PAYERS, &[Java::ReturnStatement as u16]);
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
}
